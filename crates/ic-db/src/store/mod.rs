//! Shared commit and traversal over one supplier-owned collection.
//! Typed codecs stay at the boundary; no executor is generated per entity.

mod maintenance;
mod query;

use crate::{
    __private::RowInfo, Entity, EntitySchema, Error, FieldType, IndexValue, PrimaryKey, Schema,
    UniqueKey, key, primary::RowKey, store_key::StoreKey,
};
use ic_storage::{
    Memory, RuntimeMemory,
    adapter::{MapIter, StableBTreeMap},
};
pub use maintenance::{
    CopyStatus, CopyStep, Maintenance, RetiredSource, SchemaCopy, SchemaTransition,
};
pub use query::{
    Aggregate, AggregateJobs, Column, Comparison, GroupCount, GroupCursor, GroupOrder, GroupPage,
    GroupSummaries, GroupSummary, GroupSummaryPage, GroupValue, JobHandle, JobOutcome, JobStep,
    Query, QueryCursor, QueryOperand, QueryPage, SelectColumns, SelectedQuery, Selection,
    SortDirection, SummaryMeasure,
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

fn decode_primary<E: Entity>(bytes: &[u8], key: E::Key) -> Result<E, Error> {
    let row = E::decode(bytes)?;
    if row.key() != key {
        return Err(Error::Corrupt);
    }

    Ok(row)
}

// The index value is already the encoded primary key. Validate its exact type
// and framing before using those same bytes to address the owner.
fn read_index_owner<M: Memory>(
    map: &Map<M>,
    entity: u32,
    key_type: FieldType,
    encoded: &[u8],
) -> Result<(RowKey, Vec<u8>), Error> {
    let owner = RowKey::decode(encoded, key_type)?;
    let key = prefix(2, entity, encoded);
    let row = map.get(&StoreKey::new(key)?).ok_or(Error::Corrupt)?;

    Ok((owner, row))
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
    let (id, bytes) = read_index_owner(map, entity, key_type, &entry.value())?;

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
            if matches!(field.value, FieldType::List { .. }) {
                required.extend(
                    info.list_links
                        .iter()
                        .filter(|(index, _)| *index == reference.index)
                        .map(|(_, key)| (reference.entity, *key)),
                );
                continue;
            }
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
struct ReturningRow<'a> {
    entity: u32,
    key: RowKey,
    decode: &'a mut dyn FnMut(&[u8]) -> Result<(), Error>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WriteMode {
    Insert,
    Replace,
    Upsert,
}

enum Kind {
    Put(WriteMode, Vec<u8>, RowInfo),
    // Only maintenance can construct this; ordinary insert policies stay intact.
    CopyInsert(Vec<u8>, RowInfo, [Option<crate::Timestamp>; 2]),
    Delete,
}

impl Batch {
    /// Begin an empty batch without persistent effects.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Supply a deterministic clock value in qualification fixtures.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn at_for_test(time: crate::Timestamp) -> Self {
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

        self.save(row, WriteMode::Insert)
    }

    /// Resolve a typed insert input, append it to this batch and return its full row.
    /// This is preparation, not a commit; the returned row is not yet persistent.
    /// Explicit `None` remains absence, while omitted fields select their declared
    /// literal default or optional absence. An omitted ULID primary key is issued
    /// from the local stream at the operation time. No defaults run during replacement.
    /// # Errors
    /// Refuses missing required values, field/row bounds or batch capacity.
    /// A refused input appends no mutation to the batch.
    pub fn insert_input<I: crate::InsertInput>(&mut self, input: I) -> Result<I::Entity, Error> {
        let time = *self
            .operation_time
            .get_or_insert_with(crate::Timestamp::now);
        let row = input.__into_entity(Some(time))?;
        self.save(&row, WriteMode::Insert)?;

        Ok(row)
    }

    /// Replace an existing typed record, retaining its primary key.
    /// Managed fields must echo the current stored values; commit sets update time
    /// only on a real change. Queuing a replacement does not produce a committed row.
    /// # Errors
    /// Refuses encoding or batch capacity before mutation.
    pub fn replace<E: Entity>(&mut self, row: &E) -> Result<(), Error> {
        self.save(row, WriteMode::Replace)
    }

    /// Queue a full row for insertion or replacement by primary key. Managed
    /// times are assigned by the database. Defaults and key generation do not run.
    /// # Errors
    /// Refuses encoding or batch capacity. Commit enforces ordinary constraints.
    pub fn upsert<E: Entity>(&mut self, row: &E) -> Result<(), Error> {
        self.save(row, WriteMode::Upsert)
    }

    fn save<E: Entity>(&mut self, row: &E, mode: WriteMode) -> Result<(), Error> {
        if E::SCHEMA.has_managed() {
            self.operation_time
                .get_or_insert_with(crate::Timestamp::now);
        }
        self.put(row, |bytes, info| Kind::Put(mode, bytes, info))
    }

    /// Delete the matching row if present. An absent key has no effect.
    /// # Errors
    /// Refuses batch capacity before persistent mutation.
    pub fn delete<E: Entity>(&mut self, key: impl Into<crate::Id<E>>) -> Result<(), Error> {
        self.push(
            Mutation {
                entity: E::SCHEMA,
                key: key.into().key().into_row_key(),
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
        if bytes.len() > E::SCHEMA.max_row_bytes {
            return Err(Error::Limit);
        }
        // encode() admitted field/row bounds. Keys come from that same typed
        // value; decoding freshly encoded bytes would duplicate codec work.
        let info = row.__row_info();
        let key = info.key;
        let len = bytes.len();
        let kind = kind(bytes, info);
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

/// A bounded page of rows in live primary-key order.
pub struct Page<E: Entity> {
    /// Typed records from this bounded primary-key traversal.
    pub rows: Vec<E>,
    /// Last returned key, present when another key in this entity follows at read time.
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
    pub fn get<E: Entity>(&self, key: impl Into<crate::Id<E>>) -> Result<Option<E>, Error> {
        let entity = self.schema.admits(E::SCHEMA)?;

        self.get_typed::<E>(entity, key.into().key())
    }

    /// Read the owner of a complete generated unique key, or `None` on a miss.
    /// # Errors
    /// Propagates schema, key admission and row integrity errors.
    pub fn get_unique<K: UniqueKey>(&self, key: &K) -> Result<Option<K::Entity>, Error> {
        let components = key.components();
        let values = components.as_ref();
        if !K::Entity::SCHEMA
            .index_by_name(K::INDEX)
            .is_some_and(|index| index.is_unique_lookup(values.len()))
        {
            return Err(Error::InvalidQuery);
        }

        self.query::<K::Entity>()
            .using_index(K::INDEX, values)
            .work_limit(1)
            .try_one()
    }

    /// Read up to 32 raw primary keys or entity-typed IDs, returning one slot per input position.
    /// Each distinct key is looked up once. Missing keys yield `None`; repeats have independent output slots.
    /// An empty slice needs an element type, e.g. `&[] as &[u64]` for a Nat64 key.
    /// ```compile_fail
    /// use ic_db::{Database, Entity, Id};
    /// fn wrong<A: Entity, B: Entity<Key = A::Key>>(db: &Database, ids: &[Id<A>]) {
    ///     let _ = db.get_many::<B>(ids);
    /// }
    /// ```
    /// # Errors
    /// Returns `Limit` for oversized input, or the same errors as [`Self::get`].
    pub fn get_many<E: Entity>(
        &self,
        keys: &[impl Copy + Into<crate::Id<E>>],
    ) -> Result<Vec<Option<E>>, Error> {
        if keys.len() > 32 {
            return Err(Error::Limit);
        }
        let entity = self.schema.admits(E::SCHEMA)?;

        let keys: Vec<_> = keys.iter().map(|&key| key.into().key()).collect();
        self.get_many_typed::<E>(entity, &keys)
    }

    // Share grouping and storage access across branded and raw inputs.
    #[inline(never)]
    fn get_many_typed<E: Entity>(
        &self,
        entity: &EntitySchema,
        keys: &[E::Key],
    ) -> Result<Vec<Option<E>>, Error> {
        let mut rows: Vec<_> = (0..keys.len()).map(|_| None).collect();
        let mut handled = 0_u32;
        for (i, &key) in keys.iter().enumerate() {
            if handled & (1 << i) != 0 {
                continue;
            }
            let bytes = self.map.get(&row_key(entity.id, key.into_row_key()));
            for (j, &other) in keys.iter().enumerate().skip(i) {
                if other == key {
                    handled |= 1 << j;
                    if let Some(bytes) = &bytes {
                        rows[j] = Some(decode_primary::<E>(bytes, key)?);
                    }
                }
            }
        }

        Ok(rows)
    }

    // Keep traversal separate from the static type's admission call. Passing
    // its admitted descriptor lets compatible typed readers share machine code.
    #[inline(never)]
    fn get_typed<E: Entity>(&self, entity: &EntitySchema, key: E::Key) -> Result<Option<E>, Error> {
        self.map
            .get(&row_key(entity.id, key.into_row_key()))
            .map(|bytes| decode_primary::<E>(&bytes, key))
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
        let prefix = prefix(2, entity.id, &[]);
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
            rows.push(decode_primary::<E>(&bytes, id)?);
            last = Some(id);
        }

        // Look ahead only on a full page. The next row is decoded on its own page.
        let more = rows.len() == limit
            && entries
                .next()
                .is_some_and(|entry| entry.key().starts_with(&prefix));
        Ok(Page {
            next_after: if more { last } else { None },
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
        observe!(commit, { self.commit_batch(batch, None, None).map(|_| ()) })
    }

    /// Insert or replace one row by primary key and return its stored value.
    /// # Errors
    /// Propagates batch preparation and commit errors, including unique conflicts.
    /// # Panics
    /// Supplier failures during publication trap as in [`Self::commit`].
    pub fn upsert<E: Entity>(&mut self, row: &E) -> Result<E, Error> {
        let mut batch = Batch::new();
        batch.upsert(row)?;
        self.commit_returning(batch, row.id())
    }

    /// Commit the whole batch and return the selected insert or replacement,
    /// including its final managed timestamps. The row is decoded during preparation.
    /// # Errors
    /// Returns [`Error::Missing`] if the batch has no insert or replacement for
    /// this entity and key. All ordinary [`Self::commit`] refusals apply.
    /// # Panics
    /// Supplier failures during publication trap as in [`Self::commit`].
    pub fn commit_returning<E: Entity>(
        &mut self,
        batch: Batch,
        key: impl Into<crate::Id<E>>,
    ) -> Result<E, Error> {
        observe!(commit, {
            self.schema.admits(E::SCHEMA)?;
            let mut row = None;
            let selected = ReturningRow {
                entity: E::SCHEMA.id,
                key: key.into().key().into_row_key(),
                decode: &mut |bytes| {
                    row = Some(E::decode(bytes)?);
                    Ok(())
                },
            };
            self.commit_batch(batch, Some(selected), None)?;
            Ok(row.expect("commit validated the selected row before publication"))
        })
    }

    pub(crate) fn insert_if_absent<K: UniqueKey>(
        &mut self,
        input: impl crate::InsertInput<Entity = K::Entity>,
    ) -> Result<crate::InsertOutcome<K::Entity>, Error> {
        let entity = K::Entity::SCHEMA;
        let index = entity
            .index_by_name(K::INDEX)
            .filter(|i| i.unique)
            .ok_or(Error::InvalidQuery)?;
        let mut batch = Batch::new();
        let row = batch.insert_input(input)?;
        let prefix = index_prefix(entity.id, index.id);
        let applied = observe!(commit, {
            self.commit_batch(batch, None, Some((entity, &prefix)))
        })?;
        Ok(if applied {
            crate::InsertOutcome::Inserted(row)
        } else {
            crate::InsertOutcome::AlreadyPresent
        })
    }

    // False means the single conditional insert was validated and skipped.
    fn commit_batch(
        &mut self,
        batch: Batch,
        mut returning: Option<ReturningRow<'_>>,
        conflict_target: Option<(&EntitySchema, &[u8])>,
    ) -> Result<bool, Error> {
        let mut targets = BTreeSet::new();
        let mut removes = BTreeSet::new();
        let mut writes = BTreeMap::new();
        let mut unique = BTreeSet::new();
        // Observe each entity once, then apply its final net row delta. A balanced
        // delete/insert batch must not depend on mutation order. No state survives calls.
        let mut entities = BTreeMap::new();
        let mut required = BTreeSet::new();
        let mut deleted = Vec::new();
        let mut index_runs = Vec::new();
        for mut mutation in batch.mutations {
            let direction = deleted.last().and_then(|&(entity, key)| {
                (entity == mutation.entity.id && matches!(mutation.kind, Kind::Delete))
                    .then(|| mutation.key < key)
            });
            if direction.is_none() {
                index_runs.clear();
            }
            self.schema.admits(mutation.entity)?;
            let key = row_key(mutation.entity.id, mutation.key);
            if !targets.insert(key.clone()) {
                return Err(Error::DuplicateTarget);
            }
            let old = self.map.get(&key);
            match (&mutation.kind, &old) {
                (Kind::Put(WriteMode::Insert, ..) | Kind::CopyInsert(..), Some(_)) => {
                    return Err(Error::Exists);
                }
                (Kind::Put(WriteMode::Replace, ..), None) => return Err(Error::Missing),
                (Kind::Delete, None) => continue,
                _ => {}
            }
            if mutation.entity.has_managed() {
                match &mut mutation.kind {
                    Kind::Put(mode, bytes, info) => {
                        if crate::managed::save(
                            mutation.entity,
                            bytes,
                            old.as_deref(),
                            *mode == WriteMode::Upsert,
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
            if let Kind::Put(_, bytes, _) = &mutation.kind
                && let Some(selected) = returning
                    .take_if(|row| row.entity == mutation.entity.id && row.key == mutation.key)
            {
                (selected.decode)(bytes)?;
            }
            // Admit complete after-image keys even when the row is a no-op.
            // Retain these buffers for publication instead of encoding twice.
            let mut after_indexes = match &mutation.kind {
                Kind::Put(_, _, info) | Kind::CopyInsert(_, info, _) => {
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
            if let (Kind::Put(_, bytes, _), Some(old)) = (&mutation.kind, &old)
                && bytes == old
            {
                continue;
            }
            if let Some(old) = &old {
                let info = mutation.entity.inspect_row(old)?;
                if info.key != mutation.key {
                    return Err(Error::Corrupt);
                }
                let owner = info.key.encode();
                for (slot, (key, _)) in index_entries(mutation.entity, &info)?
                    .into_iter()
                    .enumerate()
                {
                    let value = if let Some(descending) = direction
                        && key.first() == Some(&3)
                    {
                        read_adjacent_index(&self.map, &mut index_runs, slot, &key, descending)
                    } else {
                        self.map.get(&key)
                    };
                    if value.as_deref() != Some(owner.as_slice()) {
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
                Kind::Put(..) | Kind::CopyInsert(..) => i64::from(old.is_none()),
                Kind::Delete => -1,
            };
            // Private migration inserts may precede their targets. The frozen
            // complete target validates references before it can be published.
            let copy_insert = matches!(&mutation.kind, Kind::CopyInsert(..));

            match mutation.kind {
                Kind::Delete => {
                    deleted.push((mutation.entity.id, mutation.key));
                    removes.insert(key);
                }
                Kind::Put(_, bytes, info) | Kind::CopyInsert(bytes, info, _) => {
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
        drop(index_runs);
        if returning.is_some() {
            return Err(Error::Missing);
        }
        // Check existing owners only after all releases in this batch are known.
        let mut skip_insert = false;
        for key in unique {
            if let Some((entity, _)) = conflict_target.filter(|(_, prefix)| key.starts_with(prefix))
            {
                if let Some(encoded) = self.map.get(&key) {
                    let (owner, bytes) =
                        read_index_owner(&self.map, entity.id, entity.key_type()?, &encoded)?;
                    validate_index_entry(entity, &entity.inspect_row(&bytes)?, owner, &key)?;
                    skip_insert = true;
                }
                continue;
            }
            if self.map.contains_key(&key) && !removes.contains(&key) {
                return Err(Error::UniqueViolation);
            }
        }
        if skip_insert {
            // No candidate row will be published to satisfy its own references.
            self.validate_references(&required, &deleted, &removes, &BTreeMap::new())?;
            return Ok(false);
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
        let expected = removes.len() as u64;
        assert_eq!(
            self.map.remove_many(removes),
            expected,
            "prepared removal set changed"
        );
        for (key, value) in writes {
            self.map.insert(key, value);
        }

        Ok(true)
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
        // A lower-bound seek proves the gap before its first physical key is empty.
        // Keep one such gap only for this immutable preparation pass.
        let mut gap: Option<(StoreKey, Option<StoreKey>)> = None;
        for &(target, id) in deleted {
            for entity in self.schema.entities() {
                for field in entity.fields {
                    let Some(reference) = field.reference.filter(|value| value.entity == target)
                    else {
                        continue;
                    };
                    let prefix = if matches!(field.value, FieldType::List { .. }) {
                        list_target_prefix(entity.id, reference.index, id)
                    } else {
                        let mut prefix = index_prefix(entity.id, reference.index);
                        id.index_value()
                            .append_component(field.optional, &mut prefix);
                        prefix
                    };
                    let prefix = StoreKey::new(prefix)?;
                    if let Some((start, next)) = &gap
                        && start <= &prefix
                        && next
                            .as_ref()
                            .is_none_or(|key| key > &prefix && !key.starts_with(&prefix))
                    {
                        continue;
                    }
                    // Each skipped entry belongs to a distinct changed row in
                    // this range. At most MAX_BATCH skips plus one survivor.
                    let mut entries = self.map.range(prefix.clone()..);
                    for position in 0..=MAX_BATCH {
                        let entry = match entries.next() {
                            Some(entry) if entry.key().starts_with(&prefix) => entry,
                            boundary => {
                                if position == 0 && deleted.len() > 1 {
                                    gap = Some((prefix, boundary.map(|entry| entry.key().clone())));
                                }
                                break;
                            }
                        };
                        if removes.contains(entry.key()) {
                            if position == MAX_BATCH {
                                return Err(Error::Corrupt);
                            }
                            continue;
                        }
                        let (owner, bytes) = read_index_owner(
                            &self.map,
                            entity.id,
                            entity.key_type()?,
                            &entry.value(),
                        )?;
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

// Probe at most one neighboring entry per expected secondary key. A gap or different order
// switches that slot to point reads for the rest of the same-entity delete run.
// Every successful probe checks the full key; the caller checks its stored owner.
fn read_adjacent_index<'a, M: Memory>(
    map: &'a Map<M>,
    runs: &mut Vec<Option<MapIter<'a, StoreKey, Vec<u8>, M>>>,
    slot: usize,
    key: &StoreKey,
    descending: bool,
) -> Option<Vec<u8>> {
    if slot == runs.len() {
        let entries = if descending {
            map.range(..=key.clone())
        } else {
            map.range(key.clone()..)
        };
        runs.push(Some(entries));
    }
    let run = &mut runs[slot];
    if let Some(entry) = run.as_mut().and_then(|entries| {
        if descending {
            entries.next_back()
        } else {
            entries.next()
        }
    }) && entry.key() == key
    {
        return Some(entry.value());
    }
    *run = None;
    map.get(key)
}

// Physical key headers contain a namespace tag and fixed-width schema handles.
const ENTITY_PREFIX_BYTES: usize = 1 + size_of::<u32>();
const INDEX_PREFIX_BYTES: usize = ENTITY_PREFIX_BYTES + size_of::<u32>();

fn prefix(tag: u8, entity: u32, suffix: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(ENTITY_PREFIX_BYTES + suffix.len());
    key.push(tag);
    key.extend_from_slice(&entity.to_be_bytes());
    key.extend_from_slice(suffix);

    key
}

fn entity_state_key(entity: u32) -> StoreKey {
    StoreKey::new(prefix(1, entity, &[])).expect("entity state key fits")
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
    let mut key = prefix(2, entity, &[]);
    id.append_to(&mut key);

    StoreKey::new(key).expect("bounded primary key fits")
}

fn index_prefix(entity: u32, index: u32) -> Vec<u8> {
    prefix(3, entity, &index.to_be_bytes())
}
// One entry per distinct target of a list field, suffixed by its owner key.
fn list_target_prefix(entity: u32, field: u32, target: RowKey) -> Vec<u8> {
    let mut key = prefix(4, entity, &field.to_be_bytes());
    target.append_to(&mut key);
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
        let field = entity.index_field(name).ok_or(Error::InvalidSchema)?;
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
        let field = entity.index_field(name).ok_or(Error::InvalidSchema)?;
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
            let field = entity.index_field(name).ok_or(Error::Corrupt)?;
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

    let mut links = BTreeSet::new();
    for &(field, target) in &info.list_links {
        if links.insert((field, target)) {
            let mut key = list_target_prefix(entity.id, field, target);
            info.key.append_to(&mut key);
            entries.push((StoreKey::new(key)?, false));
        }
    }
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
    if key.first() == Some(&4) {
        return index_entries(entity, info)?
            .iter()
            .any(|(entry, _)| entry.as_ref() == key)
            .then_some(())
            .ok_or(Error::Corrupt);
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
