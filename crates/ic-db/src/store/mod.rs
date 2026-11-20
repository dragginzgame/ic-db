//! Shared commit and traversal over one supplier-owned collection.
//! Typed codecs stay at the boundary; no executor is generated per entity.

mod maintenance;
mod query;

use crate::{
    __private::RowInfo, Entity, EntitySchema, Error, FieldType, IndexValue, PrimaryKey, Schema,
    key, primary::RowKey, store_key::StoreKey,
};
use ic_storage::{
    Memory, RuntimeMemory,
    adapter::{MapIter, StableBTreeMap},
};
pub use maintenance::{
    CopyStatus, CopyStep, Maintenance, RetiredSource, SchemaCopy, SchemaTransition,
};
pub use query::{
    Aggregate, AggregateJobs, Comparison, GroupCount, GroupCursor, GroupOrder, GroupPage,
    GroupValue, JobHandle, JobOutcome, JobStep, Query, QueryCursor, QueryOperand, QueryPage,
    SortDirection,
};
use std::collections::{BTreeMap, BTreeSet};

const MANIFEST: &[u8] = b"ICDBMF02";
const MAX_BATCH: usize = 32;
const MAX_BATCH_BYTES: usize = 128 * 1024;
type Map<M> = StableBTreeMap<StoreKey, Vec<u8>, M>;

// Share supplier advancement and physical-key decoding without buffering a page
// or moving the typed row/key consistency check out of the public terminal.
#[inline(never)]
fn next_primary<M: Memory>(
    entries: &mut MapIter<'_, StoreKey, Vec<u8>, M>,
    prefix: &[u8],
    key_type: FieldType,
) -> Result<Option<(RowKey, Vec<u8>)>, Error> {
    let Some(entry) = entries.next() else {
        return Ok(None);
    };
    if !entry.key().starts_with(prefix) {
        return Ok(None);
    }
    let id = RowKey::decode(&entry.key()[ENTITY_PREFIX_BYTES..], key_type)?;

    Ok(Some((id, entry.value())))
}

// The typed caller checks the decoded row's index components against this key.
#[inline(never)]
fn next_index<M: Memory>(
    map: &Map<M>,
    entries: &mut MapIter<'_, StoreKey, Vec<u8>, M>,
    prefix: &[u8],
    entity: u32,
    key_type: FieldType,
) -> Result<Option<(StoreKey, RowKey, Vec<u8>)>, Error> {
    let Some(entry) = entries.next() else {
        return Ok(None);
    };
    if !entry.key().starts_with(prefix) {
        return Ok(None);
    }
    let id = RowKey::decode(&entry.value(), key_type)?;
    let bytes = map.get(&row_key(entity, id)).ok_or(Error::Corrupt)?;

    Ok(Some((entry.key().clone(), id, bytes)))
}

#[inline(never)]
fn index_prefix_key(
    entity: &EntitySchema,
    index: &str,
    values: &[IndexValue<'_>],
    limit: usize,
) -> Result<StoreKey, Error> {
    let index = entity.index_by_name(index).ok_or(Error::InvalidSchema)?;
    if values.is_empty() || !(1..=32).contains(&limit) {
        return Err(Error::Limit);
    }

    StoreKey::new(admit_index_prefix(entity, index, values)?)
}

// Both ordinary batches and complete-target migration validation derive present
// references from the same admitted reverse-index components.
fn collect_references(
    entity: &EntitySchema,
    info: &RowInfo,
    required: &mut BTreeSet<(u32, RowKey)>,
) -> Result<(), Error> {
    for field in entity.fields {
        if let Some(reference) = field.reference {
            let position = entity
                .indexes
                .iter()
                .position(|index| index.id == reference.index)
                .ok_or(Error::Corrupt)?;
            let mut bytes = info
                .indexes
                .get(position)
                .and_then(Option::as_ref)
                .map(Vec::as_slice)
                .ok_or(Error::Corrupt)?;
            if field.optional {
                // Absence has no target; zero remains an ordinary target key.
                match bytes.split_first() {
                    Some((0, _)) => continue,
                    Some((1, rest)) => bytes = rest,
                    _ => return Err(Error::Corrupt),
                }
            }
            let mut remaining = bytes;
            key::consume(&mut remaining, field.value)?;
            let target = RowKey::decode(&bytes[..bytes.len() - remaining.len()], field.value)?;
            required.insert((reference.entity, target));
        }
    }

    Ok(())
}

fn validate_reference_target<M: Memory>(
    map: &Map<M>,
    schema: &Schema,
    entity: u32,
    id: RowKey,
) -> Result<(), Error> {
    let bytes = map
        .get(&row_key(entity, id))
        .ok_or(Error::ReferenceViolation)?;
    let entity = schema.entity_by_id(entity).ok_or(Error::Corrupt)?;
    if entity.inspect_row(&bytes)?.key != id {
        return Err(Error::Corrupt);
    }

    Ok(())
}

fn manifest_key() -> StoreKey {
    StoreKey::new(vec![0]).expect("manifest key fits")
}

fn live_manifest(schema: [u8; 32], incarnation: [u8; 16]) -> Vec<u8> {
    let mut bytes = MANIFEST.to_vec();
    bytes.extend(schema);
    bytes.extend(incarnation);

    bytes
}

/// A bounded synchronous batch; preparation is internal to `Database::commit`.
#[derive(Default)]
pub struct Batch {
    operation_time: Option<crate::Timestamp>,
    mutations: Vec<Mutation>,
    bytes: usize,
}
struct Mutation {
    entity: &'static EntitySchema,
    key: RowKey,
    kind: Kind,
}
enum Kind {
    Insert(Vec<u8>, RowInfo),
    // Only maintenance can construct this; ordinary insert policies stay intact.
    CopyInsert(Vec<u8>, RowInfo, [Option<crate::Timestamp>; 2]),
    Replace(Vec<u8>, RowInfo),
    Delete,
}

impl Batch {
    /// Begin an empty batch without persistent effects.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a batch with one host-captured operation time for all managed fields.
    /// The database reads no clock; this value remains fixed for the whole batch.
    #[must_use]
    pub fn at(time: crate::Timestamp) -> Self {
        Self {
            operation_time: Some(time),
            ..Self::default()
        }
    }

    /// Insert a typed record. Primary-key/uniqueness admission happens at commit.
    /// # Errors
    /// Refuses managed-field entities (use `insert_input`), encoding or batch capacity.
    pub fn insert<E: Entity>(&mut self, row: &E) -> Result<(), Error> {
        if E::SCHEMA.has_managed() {
            return Err(Error::ManagedField);
        }

        self.put(row, Kind::Insert)
    }

    /// Resolve a typed insert input, append it to this batch and return its full row.
    /// This is preparation, not a commit; the returned row is not yet persistent.
    /// Explicit `None` remains absence, while omitted fields select their declared
    /// literal default or optional absence. No defaults run during replacement.
    /// # Errors
    /// Refuses missing required values, field/row bounds or batch capacity.
    /// A refused input appends no mutation to the batch.
    pub fn insert_input<I: crate::InsertInput>(&mut self, input: I) -> Result<I::Entity, Error> {
        let row = input.__into_entity(self.operation_time)?;
        self.put(&row, Kind::Insert)?;

        Ok(row)
    }

    /// Replace an existing typed record, retaining its primary key.
    /// Managed fields must echo the current stored values; commit sets update time
    /// only on a real change. Read the row after commit to obtain that assigned time.
    /// # Errors
    /// Refuses missing operation time, encoding or batch capacity before mutation.
    pub fn replace<E: Entity>(&mut self, row: &E) -> Result<(), Error> {
        self.put(row, Kind::Replace)
    }

    /// Delete an existing primary key; no recursive cascades are implied.
    /// # Errors
    /// Refuses batch capacity before persistent mutation.
    pub fn delete<E: Entity>(&mut self, key: E::Key) -> Result<(), Error> {
        self.push(
            Mutation {
                entity: E::SCHEMA,
                key: key.into_row_key(),
                kind: Kind::Delete,
            },
            0,
        )
    }

    fn put<E: Entity>(
        &mut self,
        row: &E,
        kind: impl FnOnce(Vec<u8>, RowInfo) -> Kind,
    ) -> Result<(), Error> {
        let bytes = row.encode()?;
        if bytes.len() > crate::__private::MAX_ROW_BYTES {
            return Err(Error::Limit);
        }
        // encode() admitted field/row bounds. Keys come from that same typed
        // value; decoding freshly encoded bytes would duplicate codec work.
        let info = row.__row_info();
        let key = info.key;
        let len = bytes.len();
        let kind = kind(bytes, info);
        if E::SCHEMA.has_managed()
            && self.operation_time.is_none()
            && !matches!(kind, Kind::CopyInsert(..))
        {
            return Err(Error::MissingOperationTime);
        }

        self.push(
            Mutation {
                entity: E::SCHEMA,
                key,
                kind,
            },
            len,
        )
    }

    fn push(&mut self, mutation: Mutation, bytes: usize) -> Result<(), Error> {
        if self.mutations.len() == MAX_BATCH || bytes > MAX_BATCH_BYTES - self.bytes {
            return Err(Error::Limit);
        }
        self.bytes += bytes;
        self.mutations.push(mutation);

        Ok(())
    }
}

/// A page with explicit progress; a full page may require an empty final call.
pub struct Page<E: Entity> {
    /// Typed records from this bounded primary-key traversal.
    pub rows: Vec<E>,
    /// Last processed key, when further traversal may remain.
    pub next_after: Option<E::Key>,
}

/// Admitted database owning the supplier's exclusive collection lease.
pub struct Database<M: Memory> {
    map: Map<M>,
    schema: Schema,
    incarnation: [u8; 16],
}

impl<M: Memory> Database<M> {
    /// Create in a proven empty governed allocation. Never resets existing data.
    /// # Errors
    /// Refuses an all-zero incarnation or nonempty/unsupported supplier allocation.
    /// # Panics
    /// Supplier failures after initialization starts trap the enclosing IC message.
    pub fn create(
        memory: RuntimeMemory<M>,
        schema: Schema,
        incarnation: [u8; 16],
    ) -> Result<Self, Error> {
        if incarnation == [0; 16] {
            return Err(Error::InvalidSchema);
        }
        if memory.size() != 0 {
            return Err(Error::Exists);
        }
        let mut map = Map::create(memory);
        map.insert(
            manifest_key(),
            live_manifest(schema.fingerprint, incarnation),
        );
        for entity in schema.entities() {
            map.insert(entity_state_key(entity.id), EntityState::default().encode());
        }

        Ok(Self {
            map,
            schema,
            incarnation,
        })
    }

    /// Open only an exact manifest/compiled-contract match, without mutation.
    /// This is not a whole-database integrity scan. Entity metadata is checked
    /// when read explicitly or when preparing a write to that entity.
    /// # Errors
    /// Refuses active maintenance, missing/corrupt manifests, foreign contracts or supplier formats.
    /// # Panics
    /// Supplier corruption, including overlong stored keys, traps without repair.
    pub fn open(memory: RuntimeMemory<M>, schema: Schema) -> Result<Self, Error> {
        if memory.size() == 0 {
            return Err(Error::Corrupt);
        }
        let map = Map::load(memory);
        let manifest = map.get(&manifest_key()).ok_or(Error::Corrupt)?;
        if maintenance::admits_gate(&manifest)? {
            return Err(Error::Maintenance);
        }
        if manifest.len() != 56 {
            return Err(Error::Corrupt);
        }
        if &manifest[..8] != MANIFEST || manifest[8..40] != schema.fingerprint {
            return Err(Error::IncompatibleSchema);
        }
        let incarnation = manifest[40..].try_into().map_err(|_| Error::Corrupt)?;
        if incarnation == [0; 16] {
            return Err(Error::Corrupt);
        }

        Ok(Self {
            map,
            schema,
            incarnation,
        })
    }

    /// Borrow the admitted executable description for inspection.
    #[must_use]
    pub const fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Durable database identity, distinct from schema identity.
    #[must_use]
    pub const fn incarnation(&self) -> [u8; 16] {
        self.incarnation
    }

    /// Read the atomic revision of one admitted entity.
    /// # Errors
    /// Refuses unknown entities and malformed revision metadata.
    pub fn revision<E: Entity>(&self) -> Result<u64, Error> {
        let entity = self.schema.admits(E::SCHEMA)?;

        read_revision(&self.map, entity.id)
    }

    /// Read the exact maintained whole-entity row count without scanning rows.
    /// This trusts atomic entity metadata; it does not validate row/index integrity.
    /// Filtered counts and row-validating enumeration use the fluent query API.
    /// # Errors
    /// Refuses unadmitted entities and missing or malformed entity metadata.
    /// # Panics
    /// Supplier corruption may trap; this operation never repairs storage.
    pub fn cardinality<E: Entity>(&self) -> Result<u64, Error> {
        let entity = self.schema.admits(E::SCHEMA)?;

        Ok(read_entity_state(&self.map, entity.id)?.count)
    }

    /// Inspect the maintained whole-entity count by exact admitted schema name.
    /// Like [`Self::cardinality`], this reads metadata without validating rows or indexes.
    /// # Errors
    /// Refuses unknown names and missing or malformed entity metadata.
    /// # Panics
    /// Supplier corruption may trap; this operation never repairs storage.
    pub fn cardinality_by_name(&self, name: &str) -> Result<u64, Error> {
        let entity = self
            .schema
            .entity_by_name(name)
            .ok_or(Error::UnknownEntity)?;

        Ok(read_entity_state(&self.map, entity.id)?.count)
    }

    fn revision_by_id(&self, entity: u32) -> Result<u64, Error> {
        if self.schema.entity_by_id(entity).is_none() {
            return Err(Error::UnknownEntity);
        }

        read_revision(&self.map, entity)
    }

    /// Read a typed primary key without planning a scan.
    /// # Errors
    /// Refuses unadmitted types or malformed rows.
    #[inline]
    pub fn get<E: Entity>(&self, key: E::Key) -> Result<Option<E>, Error> {
        let entity = self.schema.admits(E::SCHEMA)?;

        self.get_typed::<E>(entity, key)
    }

    /// Read up to 32 primary keys, returning one slot per input position.
    /// Missing keys yield `None`; repeated keys repeat the row at each position.
    /// # Errors
    /// Returns `Limit` for oversized input, or the same errors as [`Self::get`].
    pub fn get_many<E: Entity>(&self, keys: &[E::Key]) -> Result<Vec<Option<E>>, Error> {
        if keys.len() > 32 {
            return Err(Error::Limit);
        }
        let entity = self.schema.admits(E::SCHEMA)?;

        keys.iter()
            .map(|&key| self.get_typed::<E>(entity, key))
            .collect()
    }

    // Keep traversal separate from the static type's admission call. Passing
    // its admitted descriptor lets compatible typed readers share machine code.
    #[inline(never)]
    fn get_typed<E: Entity>(&self, entity: &EntitySchema, key: E::Key) -> Result<Option<E>, Error> {
        self.map
            .get(&row_key(entity.id, key.into_row_key()))
            .map(|bytes| {
                let row = E::decode(&bytes)?;
                if row.key() != key {
                    return Err(Error::Corrupt);
                }

                Ok(row)
            })
            .transpose()
    }

    /// Read at most 32 rows in primary-key order; continuation is live, not a snapshot.
    /// # Errors
    /// Refuses zero/oversized limits, unadmitted types or corrupt rows.
    /// # Panics
    /// Supplier corruption, including overlong stored keys, traps without repair.
    #[inline]
    pub fn page<E: Entity>(&self, after: Option<E::Key>, limit: usize) -> Result<Page<E>, Error> {
        let entity = self.schema.admits(E::SCHEMA)?;

        self.page_typed::<E>(entity, after, limit)
    }

    #[inline(never)]
    fn page_typed<E: Entity>(
        &self,
        entity: &EntitySchema,
        after: Option<E::Key>,
        limit: usize,
    ) -> Result<Page<E>, Error> {
        if !(1..=32).contains(&limit) {
            return Err(Error::Limit);
        }
        let prefix = prefix(2, entity.id);
        let start = after.map_or_else(
            || StoreKey::new(prefix.clone()).expect("row namespace fits"),
            |key| row_key(entity.id, key.into_row_key()),
        );
        let lower = if after.is_some() {
            std::ops::Bound::Excluded(start)
        } else {
            std::ops::Bound::Included(start)
        };
        let mut entries = self.map.range((lower, std::ops::Bound::Unbounded));
        let key_type = entity.key_type()?;
        let mut rows = Vec::new();
        let mut last = None;
        for _ in 0..limit {
            let Some((id, bytes)) = next_primary(&mut entries, &prefix, key_type)? else {
                break;
            };
            let id = E::Key::from_row_key(id)?;
            let row = E::decode(&bytes)?;
            if row.key() != id {
                return Err(Error::Corrupt);
            }
            rows.push(row);
            last = Some(id);
        }

        Ok(Page {
            next_after: if rows.len() == limit { last } else { None },
            rows,
        })
    }

    /// Read the first bounded collection under an exact, nonempty leading index prefix.
    /// Exact operands obey field bounds and the 512-byte complete-key budget.
    /// Use `IndexValue::Absent` for an absent optional component. A present-only
    /// unique index refuses absent operands because it does not enumerate those rows.
    /// # Errors
    /// Refuses unknown indexes, mismatched operand types, invalid prefix/limit or
    /// inconsistent index entries. This method does not yet return continuation.
    #[inline]
    pub fn index_prefix<E: Entity>(
        &self,
        index: &str,
        values: &[IndexValue<'_>],
        limit: usize,
    ) -> Result<Vec<E>, Error> {
        let entity = self.schema.admits(E::SCHEMA)?;

        self.index_prefix_typed::<E>(entity, index, values, limit)
    }

    #[inline(never)]
    fn index_prefix_typed<E: Entity>(
        &self,
        entity: &EntitySchema,
        index: &str,
        values: &[IndexValue<'_>],
        limit: usize,
    ) -> Result<Vec<E>, Error> {
        let prefix = index_prefix_key(entity, index, values, limit)?;
        let mut rows = Vec::new();
        let mut entries = self.map.range(prefix.clone()..);
        let key_type = entity.key_type()?;
        for _ in 0..limit {
            let Some((key, id, bytes)) =
                next_index(&self.map, &mut entries, &prefix, entity.id, key_type)?
            else {
                break;
            };
            let row = E::decode(&bytes)?;
            validate_index_entry(entity, &row.__row_info(), id, &key)?;
            rows.push(row);
        }

        Ok(rows)
    }

    /// Prepare final state, then synchronously apply rows, indexes, revisions and counts.
    /// A normal refusal occurs before persistent writes. Success is not a separate
    /// durability point from the surrounding IC message. Empty/no-op batches preserve revisions.
    /// # Errors
    /// Refuses missing/existing rows, duplicate targets, uniqueness/reference violations,
    /// corruption or limits. References use the intended final batch state.
    /// # Panics
    /// Supplier failures after the first mutation trap; never catch and continue an IC commit.
    pub fn commit(&mut self, batch: Batch) -> Result<(), Error> {
        observe!(commit, { self.commit_batch(batch) })
    }

    fn commit_batch(&mut self, batch: Batch) -> Result<(), Error> {
        let mut targets = BTreeSet::new();
        let mut removes = BTreeSet::new();
        let mut writes = BTreeMap::new();
        let mut unique = BTreeSet::new();
        // Observe each entity once, then apply its final net row delta. A balanced
        // delete/insert batch must not depend on mutation order. No state survives calls.
        let mut entities = BTreeMap::new();
        let mut required = BTreeSet::new();
        let mut deleted = Vec::new();
        for mut mutation in batch.mutations {
            self.schema.admits(mutation.entity)?;
            let key = row_key(mutation.entity.id, mutation.key);
            if !targets.insert(key.clone()) {
                return Err(Error::DuplicateTarget);
            }
            let old = self.map.get(&key);
            match (&mutation.kind, &old) {
                (Kind::Insert(..) | Kind::CopyInsert(..), Some(_)) => return Err(Error::Exists),
                (Kind::Replace(..) | Kind::Delete, None) => return Err(Error::Missing),
                _ => {}
            }
            if mutation.entity.has_managed() {
                match &mut mutation.kind {
                    Kind::Insert(bytes, _) => crate::managed::validate_insert(
                        mutation.entity,
                        bytes,
                        batch.operation_time.ok_or(Error::MissingOperationTime)?,
                    )?,
                    Kind::Replace(bytes, info) => {
                        if crate::managed::replace(
                            mutation.entity,
                            bytes,
                            old.as_deref().ok_or(Error::Missing)?,
                            batch.operation_time.ok_or(Error::MissingOperationTime)?,
                        )? {
                            *info = mutation.entity.inspect_row(bytes)?;
                        }
                    }
                    Kind::CopyInsert(bytes, _, expected) => {
                        if crate::managed::values(mutation.entity, bytes)? != *expected {
                            return Err(Error::ManagedField);
                        }
                    }
                    Kind::Delete => {}
                }
            }
            // Admit complete after-image keys even when the row is a no-op.
            // Retain these buffers for publication instead of encoding twice.
            let mut after_indexes = match &mutation.kind {
                Kind::Insert(_, info) | Kind::CopyInsert(_, info, _) | Kind::Replace(_, info) => {
                    index_entries(mutation.entity, info)?
                }
                Kind::Delete => Vec::new(),
            };
            let (_, changed, delta) = match entities.entry(mutation.entity.id) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => entry.insert((
                    read_entity_state(&self.map, mutation.entity.id)?,
                    false,
                    0_i64,
                )),
            };
            if let (Kind::Replace(bytes, _), Some(old)) = (&mutation.kind, &old)
                && bytes == old
            {
                continue;
            }
            if let Some(old) = old {
                let info = mutation.entity.inspect_row(&old)?;
                if info.key != mutation.key {
                    return Err(Error::Corrupt);
                }
                let owner = info.key.encode();
                for (key, _) in index_entries(mutation.entity, &info)? {
                    if self.map.get(&key).as_deref() != Some(owner.as_slice()) {
                        return Err(Error::Corrupt);
                    }

                    // Same physical key and owner: leave the verified entry in place.
                    // Its unique value remains occupied during final-state admission.
                    if let Some(position) =
                        after_indexes.iter().position(|(after, _)| *after == key)
                    {
                        after_indexes.remove(position);
                    } else {
                        removes.insert(key);
                    }
                }
            }
            *changed = true;
            *delta += match &mutation.kind {
                Kind::Insert(..) | Kind::CopyInsert(..) => 1,
                Kind::Delete => -1,
                Kind::Replace(..) => 0,
            };
            // Private migration inserts may precede their targets. The frozen
            // complete target validates references before it can be published.
            let copy_insert = matches!(&mutation.kind, Kind::CopyInsert(..));

            match mutation.kind {
                Kind::Delete => {
                    deleted.push((mutation.entity.id, mutation.key));
                    removes.insert(key);
                }
                Kind::Insert(bytes, info)
                | Kind::CopyInsert(bytes, info, _)
                | Kind::Replace(bytes, info) => {
                    writes.insert(key, bytes);
                    for (key, is_unique) in after_indexes {
                        if is_unique && !unique.insert(key.clone()) {
                            return Err(Error::UniqueViolation);
                        }
                        writes.insert(key, info.key.encode());
                    }
                    if !copy_insert {
                        collect_references(mutation.entity, &info, &mut required)?;
                    }
                }
            }
        }
        // Check existing owners only after all releases in this batch are known.
        for key in unique {
            if self.map.contains_key(&key) && !removes.contains(&key) {
                return Err(Error::UniqueViolation);
            }
        }
        self.validate_references(&required, &deleted, &removes, &writes)?;
        for (entity, (mut state, changed, delta)) in entities {
            if changed {
                state.revision = state.revision.checked_add(1).ok_or(Error::Limit)?;
                state.count = if delta >= 0 {
                    state.count.checked_add(delta as u64).ok_or(Error::Limit)?
                } else {
                    state
                        .count
                        .checked_sub((-delta) as u64)
                        .ok_or(Error::Corrupt)?
                };
                writes.insert(entity_state_key(entity), state.encode());
            }
        }
        // Proven IC rollback boundary: no recoverable Result branch below this line.
        for key in removes {
            self.map.remove(&key);
        }
        for (key, value) in writes {
            self.map.insert(key, value);
        }

        Ok(())
    }

    // Reuse the prepared final rows and ordinary index changes. No durable
    // relationship catalog or second mutation executor is involved.
    fn validate_references(
        &self,
        required: &BTreeSet<(u32, RowKey)>,
        deleted: &[(u32, RowKey)],
        removes: &BTreeSet<StoreKey>,
        writes: &BTreeMap<StoreKey, Vec<u8>>,
    ) -> Result<(), Error> {
        for &(entity, id) in required {
            let key = row_key(entity, id);
            if writes.contains_key(&key) {
                continue;
            } // prepared, validated final target
            if removes.contains(&key) {
                return Err(Error::ReferenceViolation);
            }
            validate_reference_target(&self.map, &self.schema, entity, id)?;
        }
        for &(target, id) in deleted {
            for entity in self.schema.entities() {
                for field in entity.fields {
                    let Some(reference) = field.reference.filter(|value| value.entity == target)
                    else {
                        continue;
                    };
                    let mut prefix = index_prefix(entity.id, reference.index);
                    id.index_value()
                        .append_component(field.optional, &mut prefix);
                    let prefix = StoreKey::new(prefix)?;
                    // Each skipped entry belongs to a distinct changed row in
                    // this range. At most MAX_BATCH skips plus one survivor.
                    for (position, entry) in self
                        .map
                        .range(prefix.clone()..)
                        .take_while(|entry| entry.key().starts_with(&prefix))
                        .take(MAX_BATCH + 1)
                        .enumerate()
                    {
                        if removes.contains(entry.key()) {
                            if position == MAX_BATCH {
                                return Err(Error::Corrupt);
                            }
                            continue;
                        }
                        let owner = RowKey::decode(&entry.value(), entity.key_type()?)?;
                        let bytes = self
                            .map
                            .get(&row_key(entity.id, owner))
                            .ok_or(Error::Corrupt)?;
                        let info = entity.inspect_row(&bytes)?;
                        validate_index_entry(entity, &info, owner, entry.key())?;

                        return Err(Error::ReferenceViolation);
                    }
                }
            }
        }

        Ok(())
    }
}

// Physical key headers contain a namespace tag and fixed-width schema handles.
const ENTITY_PREFIX_BYTES: usize = 1 + size_of::<u32>();
const INDEX_PREFIX_BYTES: usize = ENTITY_PREFIX_BYTES + size_of::<u32>();

fn prefix(tag: u8, entity: u32) -> Vec<u8> {
    let mut key = vec![tag];
    key.extend(entity.to_be_bytes());

    key
}

fn entity_state_key(entity: u32) -> StoreKey {
    StoreKey::new(prefix(1, entity)).expect("entity state key fits")
}

fn read_revision<M: Memory>(map: &Map<M>, entity: u32) -> Result<u64, Error> {
    Ok(read_entity_state(map, entity)?.revision)
}

/// One atomic metadata record; neither value is reconstructed during ordinary open.
#[derive(Default)]
struct EntityState {
    revision: u64,
    count: u64,
}
impl EntityState {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = self.revision.to_le_bytes().to_vec();
        bytes.extend(self.count.to_le_bytes());

        bytes
    }
}
fn read_entity_state<M: Memory>(map: &Map<M>, entity: u32) -> Result<EntityState, Error> {
    let bytes = map.get(&entity_state_key(entity)).ok_or(Error::Corrupt)?;
    let (revision, count) = bytes.split_first_chunk::<8>().ok_or(Error::Corrupt)?;
    let revision = u64::from_le_bytes(*revision);
    let count = u64::from_le_bytes(count.try_into().map_err(|_| Error::Corrupt)?);
    // The first insertion advances the revision in the same batch.
    if revision == 0 && count != 0 {
        return Err(Error::Corrupt);
    }

    Ok(EntityState { revision, count })
}

fn row_key(entity: u32, id: RowKey) -> StoreKey {
    let mut key = prefix(2, entity);
    id.append_to(&mut key);

    StoreKey::new(key).expect("bounded primary key fits")
}

fn index_prefix(entity: u32, index: u32) -> Vec<u8> {
    let mut key = prefix(3, entity);
    key.extend(index.to_be_bytes());

    key
}
// Direct lookups and fluent reads share exact prefix admission and completion bounds.
fn admit_index_prefix(
    entity: &EntitySchema,
    index: &crate::IndexSchema,
    values: &[IndexValue<'_>],
) -> Result<Vec<u8>, Error> {
    if values.len() > index.fields.len() {
        return Err(Error::Limit);
    }
    let mut prefix = index_prefix(entity.id, index.id);
    for (value, name) in values.iter().zip(index.fields) {
        let field = entity.field_by_name(name).ok_or(Error::InvalidSchema)?;
        value.admit(field.value, field.optional)?;
        if index.present_only && *value == IndexValue::Absent {
            return Err(Error::InvalidOperand);
        }
        value.append_component(field.optional, &mut prefix);
        if prefix.len() > crate::MAX_INDEX_KEY_BYTES {
            return Err(Error::Limit);
        }
    }
    let mut completion = if index.unique {
        0
    } else {
        key::minimum_bytes(entity.key_type()?)?
    };
    for name in &index.fields[values.len()..] {
        let field = entity.field_by_name(name).ok_or(Error::InvalidSchema)?;
        completion += if field.optional && !index.present_only {
            1
        } else {
            key::minimum_bytes(field.value)? + usize::from(field.optional)
        };
    }
    if prefix.len() + completion > crate::MAX_INDEX_KEY_BYTES {
        return Err(Error::Limit);
    }

    Ok(prefix)
}
// RowInfo may come from a generated entity or the shared stored-row decoder.
fn visit_index_components(
    entity: &EntitySchema,
    info: &RowInfo,
    mut visit: impl FnMut(&crate::IndexSchema, &[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    if info.indexes.len() != entity.indexes.len() {
        return Err(Error::Corrupt);
    }
    for (index, values) in entity.indexes.iter().zip(&info.indexes) {
        let Some(values) = values else {
            if index.present_only {
                continue;
            }

            return Err(Error::Corrupt);
        };
        let mut remaining = values.as_slice();
        for name in index.fields {
            let field = entity.field_by_name(name).ok_or(Error::Corrupt)?;
            key::consume_component(&mut remaining, field, index.present_only, Error::Corrupt)?;
        }
        if !remaining.is_empty() {
            return Err(Error::Corrupt);
        }
        visit(index, values)?;
    }

    Ok(())
}

fn index_entries(entity: &EntitySchema, info: &RowInfo) -> Result<Vec<(StoreKey, bool)>, Error> {
    let mut entries = Vec::new();
    visit_index_components(entity, info, |index, values| {
        let mut key = index_prefix(entity.id, index.id);
        key.extend_from_slice(values);
        if !index.unique {
            info.key.append_to(&mut key);
        }
        entries.push((StoreKey::new(key)?, index.unique));

        Ok(())
    })?;

    Ok(entries)
}

// A visited entry must agree with its owner in namespace, components and suffix.
// Unrelated entries retain their framing and size checks without building physical keys.
fn validate_index_entry(
    entity: &EntitySchema,
    info: &RowInfo,
    owner: RowKey,
    key: &[u8],
) -> Result<(), Error> {
    if info.key != owner {
        return Err(Error::Corrupt);
    }
    let (header, payload) = key
        .split_at_checked(INDEX_PREFIX_BYTES)
        .ok_or(Error::Corrupt)?;
    if header[0] != 3 || header[1..ENTITY_PREFIX_BYTES] != entity.id.to_be_bytes() {
        return Err(Error::Corrupt);
    }
    let mut matches = false;
    let owner = owner.encode();
    visit_index_components(entity, info, |index, values| {
        let suffix = if index.unique {
            &[][..]
        } else {
            owner.as_slice()
        };
        if INDEX_PREFIX_BYTES + values.len() + suffix.len() > crate::MAX_INDEX_KEY_BYTES {
            return Err(Error::Limit);
        }
        if header[ENTITY_PREFIX_BYTES..] == index.id.to_be_bytes() {
            matches = payload.strip_prefix(values) == Some(suffix);
        }

        Ok(())
    })?;
    if !matches {
        return Err(Error::Corrupt);
    }

    Ok(())
}
