//! Whole-database copy with private target admission and message-atomic checkpoints.

use super::{Maintenance, SchemaTransition, decode as decode_source};
use crate::{
    Batch, Entity, EntitySchema, Error, PrimaryKey, QueryCursor, Schema,
    codec::{Decoder, Encoder},
    schema::MAX_ENTITIES,
    store::Database,
    store::{
        ENTITY_PREFIX_BYTES, Map, collect_references, entity_state_key, index_entries,
        live_manifest, manifest_key, read_entity_state, row_key, validate_index_owner,
        validate_reference_target,
    },
    store_key::StoreKey,
};
use ic_storage::{Memory, RuntimeMemory};
use std::{collections::BTreeSet, ops::Bound};

pub(super) const MAGIC: &[u8; 8] = b"ICDBCP02";
const MAX_FRAME: usize = 8 + 4 + super::MAX_FRAME + 20 + 2 + 4 + QueryCursor::MAX_ENCODED_BYTES;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    transition: SchemaTransition,
    source: [u8; 16],
    target: [u8; 16],
}
#[derive(Clone, Copy)]
pub(super) enum AllocationRole<Progress> {
    Source,
    Target(Progress),
    Retired,
}

pub(super) enum TargetProgress {
    Copying(u8, Option<QueryCursor>),
    Copied,
    Cancelling,
    Cancelled,
    Validating(ValidationPass, Option<StoreKey>),
    Ready,
}

///
/// ValidationPass
/// Frozen allocation being checked against its peer in a copy checkpoint.
///
#[derive(Clone, Copy)]
pub(super) enum ValidationPass {
    Source = 5,
    Target = 6,
}

impl Identity {
    fn encode(&self, role: AllocationRole<&TargetProgress>) -> Result<Vec<u8>, Error> {
        let mut writer = Encoder::new();
        writer.bytes(&self.transition.freeze(self.source)?)?;
        writer.bytes(&self.target)?;
        match role {
            AllocationRole::Source => writer.nat8(0)?,
            AllocationRole::Retired => writer.nat8(8)?,
            AllocationRole::Target(phase) => match phase {
                TargetProgress::Copying(entity, cursor) => {
                    if *entity == 0 {
                        writer.nat8(1)?;
                    } else {
                        writer.nat8(9)?;
                        writer.nat8(*entity)?;
                    }
                    writer.bytes(&cursor.as_ref().map_or_else(Vec::new, QueryCursor::to_bytes))?;
                }
                TargetProgress::Copied => writer.nat8(2)?,
                TargetProgress::Cancelling => writer.nat8(3)?,
                TargetProgress::Cancelled => writer.nat8(4)?,
                TargetProgress::Validating(pass, cursor) => {
                    writer.nat8(*pass as u8)?;
                    writer.bytes(cursor.as_ref().map_or(&[], |key| &key[..]))?;
                }
                TargetProgress::Ready => writer.nat8(7)?,
            },
        }
        let mut bytes = MAGIC.to_vec();
        bytes.extend(writer.finish());

        Ok(bytes)
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<(Identity, AllocationRole<TargetProgress>), Error> {
    if !bytes.starts_with(MAGIC) || bytes.len() > MAX_FRAME {
        return Err(Error::InvalidTransition);
    }
    let mut reader = Decoder::new(&bytes[8..])?;
    let (transition, source) = decode_source(reader.blob_ref(super::MAX_FRAME)?)?;
    let target = reader
        .blob_ref(16)?
        .try_into()
        .map_err(|_| Error::Corrupt)?;
    if target == [0; 16] || target == source {
        return Err(Error::Corrupt);
    }
    let role = match reader.nat8()? {
        0 => AllocationRole::Source,
        tag @ (1 | 9) => {
            let entity = if tag == 1 {
                0
            } else {
                let entity = reader.nat8()?;
                if !(1..MAX_ENTITIES).contains(&entity) {
                    return Err(Error::Corrupt);
                }

                entity
            };
            let cursor = reader.blob_ref(QueryCursor::MAX_ENCODED_BYTES)?;

            AllocationRole::Target(TargetProgress::Copying(
                entity,
                if cursor.is_empty() {
                    None
                } else {
                    Some(QueryCursor::from_bytes(cursor)?)
                },
            ))
        }
        2 => AllocationRole::Target(TargetProgress::Copied),
        3 => AllocationRole::Target(TargetProgress::Cancelling),
        4 => AllocationRole::Target(TargetProgress::Cancelled),
        tag @ (5 | 6) => {
            let bytes = reader.blob_ref(crate::MAX_INDEX_KEY_BYTES)?;
            let cursor = if bytes.is_empty() {
                None
            } else {
                Some(StoreKey::new(bytes.to_vec())?)
            };

            let pass = if tag == 5 {
                ValidationPass::Source
            } else {
                ValidationPass::Target
            };

            AllocationRole::Target(TargetProgress::Validating(pass, cursor))
        }
        7 => AllocationRole::Target(TargetProgress::Ready),
        8 => AllocationRole::Retired,
        _ => return Err(Error::IncompatibleSchema),
    };
    reader.finish()?;

    Ok((
        Identity {
            transition,
            source,
            target,
        },
        role,
    ))
}

fn admit(source: &Schema, target: &Schema, transition: &SchemaTransition) -> Result<(), Error> {
    if source.fingerprint() != transition.source || target.fingerprint() != transition.target {
        return Err(Error::IncompatibleSchema);
    }
    for source in source.entities() {
        let target = target
            .entity_by_name(source.name)
            .ok_or(Error::InvalidTransition)?;
        if source.primary_key != target.primary_key
            || source.key_type()? != target.key_type()?
            || managed_roles(source) != managed_roles(target)
        {
            return Err(Error::InvalidTransition);
        }
    }

    Ok(())
}

fn managed_roles(entity: &EntitySchema) -> [bool; 2] {
    let mut roles = [false; 2];
    for policy in entity.fields.iter().filter_map(|field| field.managed) {
        roles[policy.slot()] = true;
    }

    roles
}

/// A bounded copy result. Completion means all source rows were copied, not
/// that the target is validated for publication or available for ordinary access.
#[derive(Debug, PartialEq, Eq)]
pub struct CopyStep {
    /// Rows transformed and inserted by this call.
    pub rows: usize,
    /// Every entity in the frozen source has been copied.
    pub complete: bool,
}

/// Current stage of an admitted schema copy, derived from its saved checkpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStatus {
    /// Copy the next bounded chunk of this source entity.
    Copying(&'static str),
    /// All source entities are copied. Start or continue bounded validation.
    Validating,
    /// Validation finished. The host may explicitly publish or cancel.
    ReadyToPublish,
    /// Continue bounded cancellation until the source reopens.
    Cancelling,
    /// Cancellation finished on this handle. The source may reopen after drop.
    Cancelled,
}

/// Exclusive source/target custody for a key-preserving whole-database transformation.
/// Both allocations remain closed to ordinary access. Dropping preserves progress.
/// Publication requires completed validation. Allocation reuse remains unsupported.
pub struct SchemaCopy<M: Memory> {
    source: Map<M>,
    source_schema: Schema,
    target: Database<M>,
    identity: Identity,
    phase: TargetProgress,
}

impl<M: Memory> Maintenance<M> {
    /// Bind a fresh target allocation to this frozen source and initialize copying.
    /// The host supplies a fresh nonzero target incarnation, distinct from the source.
    /// Every source entity retains its name, primary key declaration and managed
    /// timestamp roles. Additional target entities start empty. References validate only after
    /// all entities are copied, including unchanged entities. All values
    /// and target indexes use ordinary codecs and shared batch validation.
    /// # Errors
    /// Refuses unsupported shapes, wrong schemas/identity or nonempty target memory
    /// before mutation. Initial supplier growth may also refuse.
    /// # Panics
    /// Failures after target initialization trap; never catch and continue.
    pub fn begin_copy(
        mut self,
        memory: RuntimeMemory<M>,
        source: Schema,
        target: Schema,
        incarnation: [u8; 16],
    ) -> Result<SchemaCopy<M>, Error> {
        admit(&source, &target, &self.transition)?;
        if incarnation == [0; 16] || incarnation == self.incarnation {
            return Err(Error::InvalidTransition);
        }
        let identity = Identity {
            transition: self.transition,
            source: self.incarnation,
            target: incarnation,
        };
        let source_gate = identity.encode(AllocationRole::Source)?;
        let phase = TargetProgress::Copying(0, None);
        let target_gate = identity.encode(AllocationRole::Target(&phase))?;
        let map = crate::store::initialize_map(memory, &target, target_gate)?;
        self.map.insert(manifest_key(), source_gate);

        Ok(SchemaCopy {
            source: self.map,
            source_schema: source,
            target: Database {
                map,
                schema: target,
                incarnation,
            },
            identity,
            phase,
        })
    }
}

impl<M: Memory> SchemaCopy<M> {
    /// Resume only the recorded source/target pair and exact transformation.
    /// # Errors
    /// Refuses swapped, unrelated, live, cancelled or corrupt allocations intact.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn resume(
        source_memory: RuntimeMemory<M>,
        target_memory: RuntimeMemory<M>,
        source_schema: Schema,
        target_schema: Schema,
        expected: &SchemaTransition,
    ) -> Result<Self, Error> {
        admit(&source_schema, &target_schema, expected)?;
        let (source, source_manifest) = crate::store::load_manifest(source_memory)?;
        let (target, target_manifest) = crate::store::load_manifest(target_memory)?;
        let (identity, role) = decode(&source_manifest)?;
        let (target_identity, target_role) = decode(&target_manifest)?;
        let (AllocationRole::Source, AllocationRole::Target(phase)) = (role, target_role) else {
            return Err(Error::InvalidTransition);
        };
        if matches!(phase, TargetProgress::Cancelled)
            || identity != target_identity
            || identity.transition != *expected
        {
            return Err(Error::InvalidTransition);
        }
        if matches!(&phase, TargetProgress::Copying(entity, _) if usize::from(*entity) >= source_schema.entities().len())
        {
            return Err(Error::Corrupt);
        }

        Ok(Self {
            source,
            source_schema,
            target: Database {
                map: target,
                schema: target_schema,
                incarnation: identity.target,
            },
            identity,
            phase,
        })
    }

    /// Inspect the checkpoint already loaded by this handle. Inspection does no storage work.
    #[must_use]
    pub fn status(&self) -> CopyStatus {
        match &self.phase {
            TargetProgress::Copying(entity, _) => {
                CopyStatus::Copying(self.source_schema.entities()[usize::from(*entity)].name)
            }
            TargetProgress::Copied | TargetProgress::Validating(..) => CopyStatus::Validating,
            TargetProgress::Ready => CopyStatus::ReadyToPublish,
            TargetProgress::Cancelling => CopyStatus::Cancelling,
            TargetProgress::Cancelled => CopyStatus::Cancelled,
        }
    }

    /// Name of the next entity to copy, in canonical schema-name order.
    /// Returns `None` after copying or once cancellation/validation has started.
    #[must_use]
    pub fn next_entity(&self) -> Option<&'static str> {
        if let CopyStatus::Copying(entity) = self.status() {
            Some(entity)
        } else {
            None
        }
    }

    /// Explicitly preserve an unchanged entity through the same bounded copy path.
    /// The entity's complete declaration must be admitted by both schemas.
    /// # Errors
    /// Refuses changed declarations, wrong copy order, bounds or corrupt source rows.
    /// # Panics
    /// Supplier mutation failures trap and roll back the whole IC message.
    pub fn copy_unchanged<E: Entity>(
        &mut self,
        limit: usize,
        bytes: usize,
    ) -> Result<CopyStep, Error> {
        self.copy_next::<E, E>(limit, bytes, |row, _| Ok(row))
    }

    /// Transform the next bounded source page and checkpoint its target inserts.
    /// The function must be deterministic for the declared version/fixed inputs,
    /// synchronous and free of external effects or reentrant database mutations.
    /// Limits match source paging: 1..=32 candidates, 1 byte..=128 KiB source output.
    /// Target preparation additionally enforces the ordinary batch/index envelope.
    /// Managed timestamp roles and their values must be preserved; field renames
    /// are allowed through the explicit transformation. No insertion clock runs.
    /// Call for `next_entity()` only. Completion advances to the next schema entity;
    /// `complete` becomes true only after all entities have been copied.
    /// Repeated calls after completion return zero rows and `complete = true`.
    /// # Errors
    /// Refusals (including transform errors, changed keys and cross-page uniqueness
    /// collisions) leave both rows and progress unchanged. Cancelling refuses copying.
    /// # Panics
    /// Mutation failures or a later enclosing-message trap roll back the whole chunk
    /// on the IC. Native callers must discard the runtime after a mutation panic.
    pub fn copy_next<S: Entity, T: Entity>(
        &mut self,
        limit: usize,
        bytes: usize,
        transform: fn(S, &[u8]) -> Result<T, Error>,
    ) -> Result<CopyStep, Error> {
        self.source_schema.admits(S::SCHEMA)?;
        self.target.schema.admits(T::SCHEMA)?;
        let (entity, cursor) = match &self.phase {
            TargetProgress::Copying(entity, cursor) => (*entity, cursor.as_ref()),
            TargetProgress::Copied => {
                return Ok(CopyStep {
                    rows: 0,
                    complete: true,
                });
            }
            _ => return Err(Error::InvalidTransition),
        };
        if S::SCHEMA.name != self.source_schema.entities()[usize::from(entity)].name
            || S::SCHEMA.name != T::SCHEMA.name
        {
            return Err(Error::InvalidTransition);
        }
        let page = self.identity.transition.read_page::<S, _>(
            &self.source,
            self.identity.source,
            cursor,
            limit,
            bytes,
        )?;
        let rows = page.rows.len();
        let phase = if page.next.is_none() {
            if usize::from(entity) + 1 == self.source_schema.entities().len() {
                TargetProgress::Copied
            } else {
                TargetProgress::Copying(entity + 1, None)
            }
        } else {
            TargetProgress::Copying(entity, page.next)
        };
        let complete = matches!(phase, TargetProgress::Copied);
        let checkpoint = self.identity.encode(AllocationRole::Target(&phase))?;
        let mut batch = Batch::new();
        for row in page.rows {
            let key = row.key().into_row_key();
            let times = if S::SCHEMA.has_managed() {
                crate::managed::values(S::SCHEMA, &row.encode()?)?
            } else {
                [None; 2]
            };
            let result = transform(row, self.identity.transition.inputs())?;
            if result.key().into_row_key() != key {
                return Err(Error::InvalidTransition);
            }
            batch.put(&result, |bytes, info| {
                crate::store::Kind::CopyInsert(bytes, info, times)
            })?;
        }
        self.target.commit(batch)?;
        // No recoverable refusal after commit begins. The IC message owns rollback.
        self.target.map.insert(manifest_key(), checkpoint);
        self.phase = phase;

        Ok(CopyStep { rows, complete })
    }

    /// Remove at most 1..=32 target entries, retaining its ownership tombstone.
    /// Copying cannot resume once cleanup starts. Return true only after cleanup
    /// and restoration of the source live manifest; drop this handle before reopen.
    /// No physical shrink or target allocation reuse is promised.
    /// # Errors
    /// Refuses invalid limits before mutation.
    /// # Panics
    /// Supplier mutation failures trap and roll back the entire IC message.
    pub fn cancel_step(&mut self, limit: usize) -> Result<bool, Error> {
        validate_limit(limit)?;
        if matches!(self.phase, TargetProgress::Cancelled) {
            return Ok(true);
        }
        let (keys, complete) = cleanup_keys(&self.target.map, limit);
        let phase = if complete {
            TargetProgress::Cancelled
        } else {
            TargetProgress::Cancelling
        };
        let checkpoint = self.identity.encode(AllocationRole::Target(&phase))?;
        let live =
            complete.then(|| live_manifest(self.identity.transition.source, self.identity.source));
        for key in &keys {
            self.target.map.remove(key);
        }
        self.target.map.insert(manifest_key(), checkpoint);
        if let Some(live) = live {
            self.source.insert(manifest_key(), live);
        }
        self.phase = phase;

        Ok(complete)
    }

    /// Validate at most 1..=32 physical source or target entries in one call.
    /// Checks complete row framing, exact key coverage in both directions, revision
    /// metadata, expected index membership and every stored index's owner/key.
    /// Each row has at most eight declared indexes and 32 list-reference entries. Each pass first
    /// checks all declared revisions (up to the schema entity limit), including empty entities. No writes to
    /// rows/indexes occur; only successful bounded progress is checkpointed.
    /// Return true only after both passes finish. Copying cannot resume afterward.
    /// # Errors
    /// Refuses before copy completion, during cancellation, or on limits/corruption.
    /// A refused chunk leaves its durable validation position unchanged.
    /// # Panics
    /// Supplier failures trap; an IC trap rolls back checkpoint advancement.
    pub fn validate_next(&mut self, limit: usize) -> Result<bool, Error> {
        validate_limit(limit)?;
        let (pass, cursor) = match &self.phase {
            TargetProgress::Copied => (ValidationPass::Source, None),
            TargetProgress::Validating(pass, cursor) => (*pass, cursor.as_ref()),
            TargetProgress::Ready => return Ok(true),
            _ => return Err(Error::InvalidTransition),
        };
        let source = (&self.source, &self.source_schema);
        let target = (&self.target.map, &self.target.schema);
        let (current, peer) = match pass {
            ValidationPass::Source => (source, target),
            ValidationPass::Target => (target, source),
        };
        let phase = match validate_page(current, peer, cursor, limit)? {
            Some(key) => TargetProgress::Validating(pass, Some(key)),
            None => match pass {
                ValidationPass::Source => TargetProgress::Validating(ValidationPass::Target, None),
                ValidationPass::Target => TargetProgress::Ready,
            },
        };
        let checkpoint = self.identity.encode(AllocationRole::Target(&phase))?;
        let complete = matches!(phase, TargetProgress::Ready);
        self.target.map.insert(manifest_key(), checkpoint);
        self.phase = phase;

        Ok(complete)
    }

    /// Atomically retire source admission and publish the fully validated target.
    /// The host selects the target allocation for subsequent ordinary opens.
    /// Source rows remain preserved but closed: this is not an operational rollback
    /// promise after target writes. Explicit `RetiredSource` cleanup may remove them.
    /// # Errors
    /// Refuses unless both validation passes have completed.
    /// # Panics
    /// Mutation failures or a later enclosing-message trap roll back both manifests.
    pub fn publish(mut self) -> Result<Database<M>, Error> {
        if !matches!(self.phase, TargetProgress::Ready) {
            return Err(Error::InvalidTransition);
        }
        let retired = self.identity.encode(AllocationRole::Retired)?;
        let live = live_manifest(self.identity.transition.target, self.identity.target);
        self.source.insert(manifest_key(), retired);
        self.target.map.insert(manifest_key(), live);

        Ok(self.target)
    }
}

/// Exclusive cleanup authority for an entire source collection retired by cutover.
/// Admission binds the exact transition and both database incarnations. Cleanup
/// never opens the target and never clears the source's retired manifest.
pub struct RetiredSource<M: Memory> {
    map: Map<M>,
}

impl<M: Memory> RetiredSource<M> {
    /// Admit only the expected retired allocation, without decoding historical rows.
    /// The host decides when preservation/recovery obligations permit deletion.
    /// # Errors
    /// Refuses live, copying, cancelled, foreign or malformed allocations intact.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn resume(
        memory: RuntimeMemory<M>,
        expected: &SchemaTransition,
        source_incarnation: [u8; 16],
        target_incarnation: [u8; 16],
    ) -> Result<Self, Error> {
        let (map, manifest) = crate::store::load_manifest(memory)?;
        let (identity, role) = decode(&manifest)?;
        if !matches!(role, AllocationRole::Retired)
            || identity.transition != *expected
            || identity.source != source_incarnation
            || identity.target != target_incarnation
        {
            return Err(Error::InvalidTransition);
        }

        Ok(Self { map })
    }

    /// Delete at most 1..=32 retired entries, preserving the ownership manifest.
    /// Return true when only that manifest remains. Repeated completion is a no-op.
    /// Deletion itself records progress; no additional cursor or phase is stored.
    /// This does not shrink physical memory, reset the allocation or reopen source.
    /// # Errors
    /// Refuses invalid limits before mutation.
    /// # Panics
    /// Supplier mutation failures trap; the IC rolls back the whole cleanup chunk.
    pub fn cleanup_step(&mut self, limit: usize) -> Result<bool, Error> {
        validate_limit(limit)?;
        let (keys, complete) = cleanup_keys(&self.map, limit);
        for key in &keys {
            self.map.remove(key);
        }

        Ok(complete)
    }
}

fn validate_limit(limit: usize) -> Result<(), Error> {
    if !(1..=32).contains(&limit) {
        return Err(Error::Limit);
    }
    Ok(())
}

// Cancellation and retired cleanup share bounded selection, keeping their
// different authority changes at the caller. Values are not decoded for deletion.
fn cleanup_keys<M: Memory>(map: &Map<M>, limit: usize) -> (Vec<StoreKey>, bool) {
    let manifest = manifest_key();
    let mut keys: Vec<_> = map
        .keys()
        .filter(|key| *key != manifest)
        .take(limit + 1)
        .collect();
    let complete = keys.len() <= limit;
    if !complete {
        keys.pop();
    }

    (keys, complete)
}

// Validate the physical collection in both directions. Forward row checks find
// missing/wrong-owner indexes; reverse index checks find stale/extra entries.
fn validate_page<M: Memory>(
    (map, schema): (&Map<M>, &Schema),
    (peer_map, peer_schema): (&Map<M>, &Schema),
    after: Option<&StoreKey>,
    limit: usize,
) -> Result<Option<StoreKey>, Error> {
    // Check every declared entity for missing metadata and equal counts for this
    // key-preserving copy; target counts were rebuilt by atomic private inserts.
    // Frozen allocations cannot change mid-pass.
    if after.is_none() {
        for entity in schema.entities() {
            let state = read_entity_state(map, entity.id)?;

            if peer_schema.entity_by_id(entity.id).is_some() {
                if state.count != read_entity_state(peer_map, entity.id)?.count {
                    return Err(Error::Corrupt);
                }
            } else if state.count != 0 || state.revision != 0 {
                // Only an added target entity can lack a peer. No copy operation owns its rows.
                return Err(Error::Corrupt);
            }
        }
    }
    let lower = after.map_or(Bound::Unbounded, |key| Bound::Excluded(key.clone()));
    let manifest = manifest_key();
    for (index, entry) in map.range((lower, Bound::Unbounded)).take(limit).enumerate() {
        let key = entry.key();
        if *key != manifest {
            let bytes = entry.value();
            let id = u32::from_be_bytes(
                key.get(1..ENTITY_PREFIX_BYTES)
                    .ok_or(Error::Corrupt)?
                    .try_into()
                    .map_err(|_| Error::Corrupt)?,
            );
            let entity = schema.entity_by_id(id).ok_or(Error::Corrupt)?;

            match key.first() {
                Some(1) if *key == entity_state_key(entity.id) => {
                    if bytes.len() != 16 {
                        return Err(Error::Corrupt);
                    }
                }
                Some(2) => {
                    let peer_entity = peer_schema.entity_by_id(id).ok_or(Error::Corrupt)?;
                    let info = entity.inspect_row(&bytes)?;
                    let owner = info.key;
                    if *key != row_key(entity.id, owner) {
                        return Err(Error::Corrupt);
                    }
                    if entity.has_managed() {
                        let other = peer_map.get(key).ok_or(Error::Corrupt)?;
                        if crate::managed::values(entity, &bytes)?
                            != crate::managed::values(peer_entity, &other)?
                        {
                            return Err(Error::ManagedField);
                        }
                    } else if !peer_map.contains_key(key) {
                        return Err(Error::Corrupt);
                    }
                    // The row-key check above established the canonical owner bytes.
                    let encoded_owner = &key[ENTITY_PREFIX_BYTES..];
                    for (index, _) in index_entries(entity, &info)? {
                        if map.get(&index).as_deref() != Some(encoded_owner) {
                            return Err(Error::Corrupt);
                        }
                    }
                    let mut required = BTreeSet::new();
                    collect_references(entity, &info, &mut required)?;
                    for (target, id) in required {
                        validate_reference_target(map, schema, target, id)?;
                    }
                }
                Some(3 | 4) => {
                    validate_index_owner(map, entity, key, &bytes)?;
                }
                _ => return Err(Error::Corrupt),
            }
        }
        if index + 1 == limit {
            return Ok(Some(entry.into_key()));
        }
    }

    Ok(None)
}
