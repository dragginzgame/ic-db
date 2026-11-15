//! One-entity copy with private target admission and message-atomic checkpoints.
use super::{Maintenance, SchemaTransition, decode as decode_source};
use crate::codec::{Decoder, Encoder};
use crate::store::{
    Map, index_entries, live_manifest, manifest_key, read_revision, revision_key, row_key,
    validate_index_entry,
};
use crate::{
    Batch, Database, Entity, EntitySchema, Error, PrimaryKey, QueryCursor, Schema, primary::RowKey,
    store_key::StoreKey,
};
use ic_storage::{DefaultMemoryImpl, Memory, RuntimeMemory};
use std::ops::Bound;

pub(super) const MAGIC: &[u8; 8] = b"ICDBCP01";
const MAX_FRAME: usize = 8 + 4 + super::MAX_FRAME + 20 + 1 + 4 + QueryCursor::MAX_ENCODED_BYTES;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    transition: SchemaTransition,
    source: [u8; 16],
    target: [u8; 16],
}
pub(super) enum Phase {
    Source,
    Copying(Option<QueryCursor>),
    Copied,
    Cancelling,
    Cancelled,
    ValidatingSource(Option<StoreKey>),
    ValidatingTarget(Option<StoreKey>),
    Ready,
    Retired,
}
impl Identity {
    fn encode(&self, phase: &Phase) -> Result<Vec<u8>, Error> {
        let mut writer = Encoder::new();
        writer.bytes(&self.transition.freeze(self.source)?)?;
        writer.bytes(&self.target)?;
        writer.nat8(match phase {
            Phase::Source => 0,
            Phase::Copying(_) => 1,
            Phase::Copied => 2,
            Phase::Cancelling => 3,
            Phase::Cancelled => 4,
            Phase::ValidatingSource(_) => 5,
            Phase::ValidatingTarget(_) => 6,
            Phase::Ready => 7,
            Phase::Retired => 8,
        })?;
        if let Phase::Copying(cursor) = phase {
            writer.bytes(&cursor.as_ref().map_or_else(Vec::new, QueryCursor::to_bytes))?;
        }
        if let Phase::ValidatingSource(cursor) | Phase::ValidatingTarget(cursor) = phase {
            writer.bytes(cursor.as_ref().map_or(&[], |key| &key[..]))?;
        }
        let mut bytes = MAGIC.to_vec();
        bytes.extend(writer.finish());
        Ok(bytes)
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<(Identity, Phase), Error> {
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
    let phase = match reader.nat8()? {
        0 => Phase::Source,
        1 => {
            let cursor = reader.blob_ref(QueryCursor::MAX_ENCODED_BYTES)?;
            Phase::Copying(if cursor.is_empty() {
                None
            } else {
                Some(QueryCursor::from_bytes(cursor)?)
            })
        }
        2 => Phase::Copied,
        3 => Phase::Cancelling,
        4 => Phase::Cancelled,
        tag @ (5 | 6) => {
            let bytes = reader.blob_ref(crate::MAX_INDEX_KEY_BYTES)?;
            let cursor = if bytes.is_empty() {
                None
            } else {
                Some(StoreKey::new(bytes.to_vec())?)
            };
            if tag == 5 {
                Phase::ValidatingSource(cursor)
            } else {
                Phase::ValidatingTarget(cursor)
            }
        }
        7 => Phase::Ready,
        8 => Phase::Retired,
        _ => return Err(Error::IncompatibleSchema),
    };
    reader.finish()?;
    Ok((
        Identity {
            transition,
            source,
            target,
        },
        phase,
    ))
}

fn admit(source: &Schema, target: &Schema, transition: &SchemaTransition) -> Result<(), Error> {
    if source.fingerprint() != transition.source || target.fingerprint() != transition.target {
        return Err(Error::IncompatibleSchema);
    }
    let ([source], [target]) = (source.entities(), target.entities()) else {
        return Err(Error::InvalidTransition);
    };
    if source.name != target.name
        || source.primary_key != target.primary_key
        || source.key_type()? != target.key_type()?
        || [source, target].iter().any(|entity| {
            entity
                .fields
                .iter()
                .any(|field| field.reference.is_some() || field.managed.is_some())
        })
    {
        return Err(Error::InvalidTransition);
    }
    Ok(())
}

/// A bounded copy result. Completion means all source rows were copied, not
/// that the target is validated for publication or available for ordinary access.
#[derive(Debug, PartialEq, Eq)]
pub struct CopyStep {
    /// Rows transformed and inserted by this call.
    pub rows: usize,
    /// End of the frozen source was observed.
    pub complete: bool,
}

/// Exclusive source/target custody for one key-preserving entity transformation.
/// Both allocations remain closed to ordinary access. Dropping preserves progress.
/// Publication requires completed validation. Allocation reuse remains unsupported.
pub struct SchemaCopy<M: Memory = DefaultMemoryImpl> {
    source: Maintenance<M>,
    source_schema: Schema,
    target: Database<M>,
    identity: Identity,
    phase: Phase,
}

impl<M: Memory> Maintenance<M> {
    /// Bind a fresh target allocation to this frozen source and initialize copying.
    /// The host supplies a fresh nonzero target incarnation, distinct from the source.
    /// The schemas must each contain one same-named entity with the same primary
    /// key declaration, without references or managed timestamps. All values and
    /// target indexes use ordinary codec and batch validation.
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
            transition: self.transition.clone(),
            source: self.incarnation,
            target: incarnation,
        };
        let source_gate = identity.encode(&Phase::Source)?;
        let phase = Phase::Copying(None);
        let target_gate = identity.encode(&phase)?;
        let revision = revision_key(target.entities()[0].id);
        let mut map = Map::create(memory).map_err(Error::Storage)?;
        map.insert(manifest_key(), target_gate);
        map.insert(revision, 0_u64.to_le_bytes().to_vec());
        self.map.insert(manifest_key(), source_gate);
        Ok(SchemaCopy {
            source: self,
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
        let source = Map::load(source_memory).map_err(Error::Storage)?;
        let target = Map::load(target_memory).map_err(Error::Storage)?;
        let (identity, role) = decode(&source.get(&manifest_key()).ok_or(Error::Corrupt)?)?;
        let (target_identity, phase) = decode(&target.get(&manifest_key()).ok_or(Error::Corrupt)?)?;
        if !matches!(role, Phase::Source)
            || matches!(phase, Phase::Source | Phase::Cancelled | Phase::Retired)
            || identity != target_identity
            || identity.transition != *expected
        {
            return Err(Error::InvalidTransition);
        }
        Ok(Self {
            source: Maintenance {
                map: source,
                transition: expected.clone(),
                incarnation: identity.source,
            },
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

    /// Transform the next bounded source page and checkpoint its target inserts.
    /// The function must be deterministic for the declared version/fixed inputs,
    /// synchronous and free of external effects or reentrant database mutations.
    /// Limits match source paging: 1..=32 candidates, 1 byte..=128 KiB source output.
    /// Target preparation additionally enforces the ordinary batch/index envelope.
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
        let cursor = match &self.phase {
            Phase::Copying(cursor) => cursor.as_ref(),
            Phase::Copied => {
                return Ok(CopyStep {
                    rows: 0,
                    complete: true,
                });
            }
            _ => return Err(Error::InvalidTransition),
        };
        let page = self
            .source
            .source_page::<S>(&self.source_schema, cursor, limit, bytes)?;
        let rows = page.rows.len();
        let complete = page.next.is_none();
        let phase = if complete {
            Phase::Copied
        } else {
            Phase::Copying(page.next)
        };
        let checkpoint = self.identity.encode(&phase)?;
        let mut batch = Batch::new();
        for row in page.rows {
            let key = row.key().into_row_key();
            let result = transform(row, self.identity.transition.inputs())?;
            if result.key().into_row_key() != key {
                return Err(Error::InvalidTransition);
            }
            batch.insert(&result)?;
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
        if !(1..=32).contains(&limit) {
            return Err(Error::Limit);
        }
        if matches!(self.phase, Phase::Cancelled) {
            return Ok(true);
        }
        let keys: Vec<_> = self
            .target
            .map
            .keys()
            .filter(|key| *key != manifest_key())
            .take(limit + 1)
            .collect();
        let complete = keys.len() <= limit;
        let phase = if complete {
            Phase::Cancelled
        } else {
            Phase::Cancelling
        };
        let checkpoint = self.identity.encode(&phase)?;
        let live = live_manifest(self.identity.transition.source, self.identity.source);
        for key in keys.iter().take(limit) {
            self.target.map.remove_discard(key);
        }
        self.target.map.insert(manifest_key(), checkpoint);
        if complete {
            self.source.map.insert(manifest_key(), live);
        }
        self.phase = phase;
        Ok(complete)
    }

    /// Validate at most 1..=32 physical source or target entries in one call.
    /// Checks complete row framing, exact key coverage in both directions, revision
    /// metadata, expected index membership and every stored index's owner/key.
    /// Each row has at most eight indexes under the admitted schema. No writes to
    /// rows/indexes occur; only successful bounded progress is checkpointed.
    /// Return true only after both passes finish. Copying cannot resume afterward.
    /// # Errors
    /// Refuses before copy completion, during cancellation, or on limits/corruption.
    /// A refused chunk leaves its durable validation position unchanged.
    /// # Panics
    /// Supplier failures trap; an IC trap rolls back checkpoint advancement.
    pub fn validate_next(&mut self, limit: usize) -> Result<bool, Error> {
        if !(1..=32).contains(&limit) {
            return Err(Error::Limit);
        }
        let source_entity = self.source_schema.entities()[0];
        let target_entity = self.target.schema.entities()[0];
        let phase = match &self.phase {
            Phase::Copied | Phase::ValidatingSource(_) => {
                let cursor = if let Phase::ValidatingSource(cursor) = &self.phase {
                    cursor.as_ref()
                } else {
                    None
                };
                let next = validate_page(
                    &self.source.map,
                    source_entity,
                    (&self.target.map, target_entity),
                    cursor,
                    limit,
                )?;
                match next {
                    Some(key) => Phase::ValidatingSource(Some(key)),
                    None => Phase::ValidatingTarget(None),
                }
            }
            Phase::ValidatingTarget(cursor) => {
                let next = validate_page(
                    &self.target.map,
                    target_entity,
                    (&self.source.map, source_entity),
                    cursor.as_ref(),
                    limit,
                )?;
                match next {
                    Some(key) => Phase::ValidatingTarget(Some(key)),
                    None => Phase::Ready,
                }
            }
            Phase::Ready => return Ok(true),
            _ => return Err(Error::InvalidTransition),
        };
        let checkpoint = self.identity.encode(&phase)?;
        let complete = matches!(phase, Phase::Ready);
        self.target.map.insert(manifest_key(), checkpoint);
        self.phase = phase;
        Ok(complete)
    }

    /// Atomically retire source admission and publish the fully validated target.
    /// The host selects the target allocation for subsequent ordinary opens.
    /// Source rows remain preserved but closed: this is not an operational rollback
    /// promise after target writes. Retired-source cleanup is not yet implemented.
    /// # Errors
    /// Refuses unless both validation passes have completed.
    /// # Panics
    /// Mutation failures or a later enclosing-message trap roll back both manifests.
    pub fn publish(mut self) -> Result<Database<M>, Error> {
        if !matches!(self.phase, Phase::Ready) {
            return Err(Error::InvalidTransition);
        }
        let retired = self.identity.encode(&Phase::Retired)?;
        let live = live_manifest(self.identity.transition.target, self.identity.target);
        self.source.map.insert(manifest_key(), retired);
        self.target.map.insert(manifest_key(), live);
        Ok(self.target)
    }
}

// Validate the physical collection in both directions. Forward row checks find
// missing/wrong-owner indexes; reverse index checks find stale/extra entries.
fn validate_page<M: Memory>(
    map: &Map<M>,
    entity: &EntitySchema,
    peer: (&Map<M>, &EntitySchema),
    after: Option<&StoreKey>,
    limit: usize,
) -> Result<Option<StoreKey>, Error> {
    read_revision(map, entity.id)?;
    let lower = after.map_or(Bound::Unbounded, |key| Bound::Excluded(key.clone()));
    let mut last = None;
    let mut examined = 0;
    for entry in map.range((lower, Bound::Unbounded)).take(limit) {
        let key = entry.key();
        if *key != manifest_key() {
            let bytes = entry.value();
            match key.first() {
                Some(1) if *key == revision_key(entity.id) => {
                    if bytes.len() != 8 {
                        return Err(Error::Corrupt);
                    }
                }
                Some(2) if key.get(1..5) == Some(entity.id.to_be_bytes().as_slice()) => {
                    let info = entity.inspect_row(&bytes)?;
                    let owner = RowKey::decode(&key[5..], entity.key_type()?)?;
                    if info.key != owner || !peer.0.contains_key(&row_key(peer.1.id, owner)) {
                        return Err(Error::Corrupt);
                    }
                    for (index, _) in index_entries(entity, &info)? {
                        let stored = map.get(&index).ok_or(Error::Corrupt)?;
                        if RowKey::decode(&stored, entity.key_type()?)? != owner {
                            return Err(Error::Corrupt);
                        }
                    }
                }
                Some(3) => {
                    let owner = RowKey::decode(&bytes, entity.key_type()?)?;
                    let row = map.get(&row_key(entity.id, owner)).ok_or(Error::Corrupt)?;
                    validate_index_entry(entity, &entity.inspect_row(&row)?, owner, key)?;
                }
                _ => return Err(Error::Corrupt),
            }
        }
        last = Some(key.clone());
        examined += 1;
    }
    Ok(if examined == limit { last } else { None })
}
