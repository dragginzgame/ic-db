//! Shared commit and traversal over one supplier-owned collection.
//! Typed codecs stay at the boundary; no executor is generated per entity.

use crate::{
    __private::RowInfo, Entity, EntitySchema, Error, IndexValue, PrimaryKey, Schema, key,
    primary::RowKey, store_key::StoreKey,
};
use ic_storage::{DefaultMemoryImpl, Memory, RuntimeMemory, adapter::StableBTreeMap};
use std::collections::{BTreeMap, BTreeSet};

mod query;
pub use query::{
    Aggregate, AggregateJobs, Comparison, GroupCount, GroupCursor, GroupOrder, GroupPage,
    GroupValue, JobHandle, JobOutcome, JobStep, Query, QueryCursor, QueryPage,
};

const MANIFEST: &[u8] = b"ICDBMF01";
const MAX_BATCH: usize = 32;
const MAX_BATCH_BYTES: usize = 128 * 1024;
type Map<M> = StableBTreeMap<StoreKey, Vec<u8>, M>;

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
        self.put(row, false)
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
        self.put(&row, false)?;
        Ok(row)
    }
    /// Replace an existing typed record, retaining its primary key.
    /// Managed fields must echo the current stored values; commit sets update time
    /// only on a real change. Read the row after commit to obtain that assigned time.
    /// # Errors
    /// Refuses missing operation time, encoding or batch capacity before mutation.
    pub fn replace<E: Entity>(&mut self, row: &E) -> Result<(), Error> {
        self.put(row, true)
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
    fn put<E: Entity>(&mut self, row: &E, replace: bool) -> Result<(), Error> {
        if E::SCHEMA.has_managed() && self.operation_time.is_none() {
            return Err(Error::MissingOperationTime);
        }
        let bytes = row.encode()?;
        if bytes.len() > crate::__private::MAX_ROW_BYTES {
            return Err(Error::Limit);
        }
        // encode() admitted field/row bounds. Keys come from that same typed
        // value; decoding freshly encoded bytes would duplicate codec work.
        let info = row.__row_info();
        let len = bytes.len();
        self.push(
            Mutation {
                entity: E::SCHEMA,
                key: info.key,
                kind: if replace {
                    Kind::Replace(bytes, info)
                } else {
                    Kind::Insert(bytes, info)
                },
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
pub struct Database<M: Memory = DefaultMemoryImpl> {
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
        let mut map = Map::create(memory).map_err(Error::Storage)?;
        let mut manifest = MANIFEST.to_vec();
        manifest.extend(schema.fingerprint);
        manifest.extend(incarnation);
        map.insert(StoreKey::new(vec![0]).expect("manifest key fits"), manifest);
        for entity in &schema.entities {
            map.insert(revision_key(entity.id), 0_u64.to_le_bytes().to_vec());
        }
        Ok(Self {
            map,
            schema,
            incarnation,
        })
    }
    /// Open only an exact manifest/compiled-contract match, without mutation.
    /// This is not a whole-database integrity scan. Entity revisions are checked
    /// when read explicitly or when preparing a write to that entity.
    /// # Errors
    /// Refuses missing/corrupt manifests, foreign contracts or supplier formats.
    /// # Panics
    /// Supplier corruption, including overlong stored keys, traps without repair.
    pub fn open(memory: RuntimeMemory<M>, schema: Schema) -> Result<Self, Error> {
        let map = Map::load(memory).map_err(Error::Storage)?;
        let manifest = map
            .get(&StoreKey::new(vec![0]).expect("manifest key fits"))
            .ok_or(Error::Corrupt)?;
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
        self.schema.admits(E::SCHEMA)?;
        self.revision_by_id(E::ID)
    }
    fn revision_by_id(&self, entity: u32) -> Result<u64, Error> {
        if !self.schema.entities.iter().any(|value| value.id == entity) {
            return Err(Error::UnknownEntity);
        }
        let bytes = self.map.get(&revision_key(entity)).ok_or(Error::Corrupt)?;
        Ok(u64::from_le_bytes(
            bytes.as_slice().try_into().map_err(|_| Error::Corrupt)?,
        ))
    }
    /// Read a typed primary key without planning a scan.
    /// # Errors
    /// Refuses unadmitted types or malformed rows.
    pub fn get<E: Entity>(&self, key: E::Key) -> Result<Option<E>, Error> {
        self.schema.admits(E::SCHEMA)?;
        self.map
            .get(&row_key(E::SCHEMA.id, key.into_row_key()))
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
    pub fn page<E: Entity>(&self, after: Option<E::Key>, limit: usize) -> Result<Page<E>, Error> {
        self.schema.admits(E::SCHEMA)?;
        if !(1..=32).contains(&limit) {
            return Err(Error::Limit);
        }
        let prefix = prefix(2, E::SCHEMA.id);
        let start = after.map_or_else(
            || StoreKey::new(prefix.clone()).expect("row namespace fits"),
            |key| row_key(E::SCHEMA.id, key.into_row_key()),
        );
        let lower = if after.is_some() {
            std::ops::Bound::Excluded(start)
        } else {
            std::ops::Bound::Included(start)
        };
        let entries = self
            .map
            .range((lower, std::ops::Bound::Unbounded))
            .take_while(|entry| entry.key().starts_with(&prefix))
            .take(limit);
        let mut rows = Vec::new();
        let mut last = None;
        for entry in entries {
            let key = entry.key();
            let id = E::Key::from_row_key(RowKey::decode(&key[5..], E::SCHEMA.key_type()?)?)?;
            let bytes = entry.value();
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
    pub fn index_prefix<E: Entity>(
        &self,
        index: &str,
        values: &[IndexValue<'_>],
        limit: usize,
    ) -> Result<Vec<E>, Error> {
        self.schema.admits(E::SCHEMA)?;
        let index = E::SCHEMA
            .indexes
            .iter()
            .find(|value| value.name == index)
            .ok_or(Error::InvalidSchema)?;
        if values.is_empty() || values.len() > index.fields.len() || !(1..=32).contains(&limit) {
            return Err(Error::Limit);
        }
        let prefix = StoreKey::new(admit_index_prefix(E::SCHEMA, index, values)?)?;
        let mut rows = Vec::new();
        for entry in self
            .map
            .range(prefix.clone()..)
            .take_while(|entry| entry.key().starts_with(&prefix))
            .take(limit)
        {
            let id = RowKey::decode(&entry.value(), E::SCHEMA.key_type()?)?;
            let bytes = self
                .map
                .get(&row_key(E::SCHEMA.id, id))
                .ok_or(Error::Corrupt)?;
            let row = E::decode(&bytes)?;
            validate_index_entry(E::SCHEMA, &row.__row_info(), id, entry.key())?;
            rows.push(row);
        }
        Ok(rows)
    }
    /// Prepare the complete final state, then synchronously apply rows/indexes/revisions.
    /// A normal refusal occurs before persistent writes. Success is not a separate
    /// durability point from the surrounding IC message. Empty/no-op batches preserve revisions.
    /// # Errors
    /// Refuses missing/existing rows, duplicate targets, uniqueness/reference violations,
    /// corruption or limits. References use the intended final batch state.
    /// # Panics
    /// Supplier failures after the first mutation trap; never catch and continue an IC commit.
    pub fn commit(&mut self, batch: Batch) -> Result<(), Error> {
        let mut targets = BTreeSet::new();
        let mut removes = BTreeSet::new();
        let mut writes = BTreeMap::new();
        let mut unique = BTreeSet::new();
        // Per-batch preparation only: observed revision and whether any row
        // actually changes. Nothing is retained between calls.
        let mut revisions = BTreeMap::new();
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
                (Kind::Insert(..), Some(_)) => return Err(Error::Exists),
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
                    Kind::Delete => {}
                }
            }
            // Admit complete after-image keys even when the row is a no-op.
            // Retain these buffers for publication instead of encoding twice.
            let after_indexes = match &mutation.kind {
                Kind::Insert(_, info) | Kind::Replace(_, info) => {
                    index_entries(mutation.entity, info)?
                }
                Kind::Delete => Vec::new(),
            };
            let (_, changed) = match revisions.entry(mutation.entity.id) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert((self.revision_by_id(mutation.entity.id)?, false))
                }
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
                for (key, _) in index_entries(mutation.entity, &info)? {
                    if self.map.get(&key) != Some(info.key.encode()) {
                        return Err(Error::Corrupt);
                    }
                    removes.insert(key);
                }
            }
            *changed = true;
            match mutation.kind {
                Kind::Delete => {
                    deleted.push((mutation.entity.id, mutation.key));
                    removes.insert(key);
                }
                Kind::Insert(bytes, info) | Kind::Replace(bytes, info) => {
                    writes.insert(key, bytes);
                    for (key, is_unique) in after_indexes {
                        if is_unique && !unique.insert(key.clone()) {
                            return Err(Error::UniqueViolation);
                        }
                        writes.insert(key, info.key.encode());
                    }
                    for field in mutation.entity.fields {
                        if let Some(reference) = field.reference {
                            let position = mutation
                                .entity
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
                                // index_entries already validated the complete key.
                                // Absence creates no relationship, including to key zero.
                                match bytes.split_first() {
                                    Some((0, _)) => continue,
                                    Some((1, rest)) => bytes = rest,
                                    _ => return Err(Error::Corrupt),
                                }
                            }
                            let mut remaining = bytes;
                            key::consume(&mut remaining, field.value)?;
                            let target = RowKey::decode(
                                &bytes[..bytes.len() - remaining.len()],
                                field.value,
                            )?;
                            required.insert((reference.entity, target));
                        }
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
        for (entity, (current, changed)) in revisions {
            if changed {
                let next = current.checked_add(1).ok_or(Error::Limit)?;
                writes.insert(revision_key(entity), next.to_le_bytes().to_vec());
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
            let bytes = self.map.get(&key).ok_or(Error::ReferenceViolation)?;
            let schema = self
                .schema
                .entities
                .iter()
                .find(|value| value.id == entity)
                .ok_or(Error::Corrupt)?;
            if schema.inspect_row(&bytes)?.key != id {
                return Err(Error::Corrupt);
            }
        }
        for &(target, id) in deleted {
            for entity in &self.schema.entities {
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

fn prefix(tag: u8, entity: u32) -> Vec<u8> {
    let mut key = vec![tag];
    key.extend(entity.to_be_bytes());
    key
}
fn revision_key(entity: u32) -> StoreKey {
    StoreKey::new(prefix(1, entity)).expect("revision key fits")
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
        let field = entity
            .fields
            .iter()
            .find(|field| field.name == *name)
            .ok_or(Error::InvalidSchema)?;
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
        let field = entity
            .fields
            .iter()
            .find(|field| field.name == *name)
            .ok_or(Error::InvalidSchema)?;
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
fn index_entries(entity: &EntitySchema, info: &RowInfo) -> Result<Vec<(StoreKey, bool)>, Error> {
    if info.indexes.len() != entity.indexes.len() {
        return Err(Error::Corrupt);
    }
    let mut entries = Vec::new();
    for (index, values) in entity.indexes.iter().zip(&info.indexes) {
        let Some(values) = values else {
            if index.present_only {
                continue;
            }
            return Err(Error::Corrupt);
        };
        let mut remaining = values.as_slice();
        for name in index.fields {
            let field = entity
                .fields
                .iter()
                .find(|field| field.name == *name)
                .ok_or(Error::Corrupt)?;
            if field.optional {
                let (&presence, rest) = remaining.split_first().ok_or(Error::Corrupt)?;
                remaining = rest;
                match presence {
                    0 if !index.present_only => continue,
                    1 => {}
                    _ => return Err(Error::Corrupt),
                }
            }
            key::consume(&mut remaining, field.value)?;
        }
        if !remaining.is_empty() {
            return Err(Error::Corrupt);
        }
        let mut key = index_prefix(entity.id, index.id);
        key.extend(values);
        if !index.unique {
            info.key.append_to(&mut key);
        }
        entries.push((StoreKey::new(key)?, index.unique));
    }
    Ok(entries)
}

// Both indexed reads and reverse-reference checks must prove the physical entry
// belongs to the decoded row, rather than trusting a valid-looking owner alone.
fn validate_index_entry(
    entity: &EntitySchema,
    info: &RowInfo,
    owner: RowKey,
    key: &[u8],
) -> Result<(), Error> {
    if info.key != owner
        || !index_entries(entity, info)?
            .iter()
            .any(|(entry, _)| &entry[..] == key)
    {
        return Err(Error::Corrupt);
    }
    Ok(())
}
