//! Shared commit and traversal over one supplier-owned collection.
//! Typed codecs stay at the boundary; no executor is generated per entity.

use crate::{__private::RowInfo, Entity, EntitySchema, Error, Schema};
use ic_storage::{DefaultMemoryImpl, Memory, RuntimeMemory, adapter::StableBTreeMap};
use std::collections::{BTreeMap, BTreeSet};

const MANIFEST: &[u8] = b"ICDBMF01";
const MAX_BATCH: usize = 32;
const MAX_BATCH_BYTES: usize = 128 * 1024;
type Map<M> = StableBTreeMap<Vec<u8>, Vec<u8>, M>;

/// A bounded synchronous batch; preparation is internal to `Database::commit`.
#[derive(Default)]
pub struct Batch {
    mutations: Vec<Mutation>,
    bytes: usize,
}
struct Mutation {
    entity: &'static EntitySchema,
    key: u64,
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
    /// Insert a typed record. Primary-key/uniqueness admission happens at commit.
    /// # Errors
    /// Refuses encoding or batch capacity before persistent mutation.
    pub fn insert<E: Entity>(&mut self, row: &E) -> Result<(), Error> {
        self.put(row, false)
    }
    /// Replace an existing typed record, retaining its primary key.
    /// # Errors
    /// Refuses encoding or batch capacity before persistent mutation.
    pub fn replace<E: Entity>(&mut self, row: &E) -> Result<(), Error> {
        self.put(row, true)
    }
    /// Delete an existing primary key; no recursive cascades are implied.
    /// # Errors
    /// Refuses batch capacity before persistent mutation.
    pub fn delete<E: Entity>(&mut self, key: u64) -> Result<(), Error> {
        self.push(
            Mutation {
                entity: E::SCHEMA,
                key,
                kind: Kind::Delete,
            },
            0,
        )
    }
    fn put<E: Entity>(&mut self, row: &E, replace: bool) -> Result<(), Error> {
        let bytes = row.encode()?;
        if bytes.len() > crate::__private::ROW_LIMITS.bytes {
            return Err(Error::Limit);
        }
        let info = (E::SCHEMA.inspect_row)(&bytes)?;
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
pub struct Page<E> {
    /// Typed records from this bounded primary-key traversal.
    pub rows: Vec<E>,
    /// Last processed key, when further traversal may remain.
    pub next_after: Option<u64>,
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
        map.insert(vec![0], manifest);
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
    /// # Errors
    /// Refuses missing/corrupt manifests, foreign contracts or supplier formats.
    pub fn open(memory: RuntimeMemory<M>, schema: Schema) -> Result<Self, Error> {
        let map = Map::load(memory).map_err(Error::Storage)?;
        let manifest = map.get(&vec![0]).ok_or(Error::Corrupt)?;
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
        let database = Self {
            map,
            schema,
            incarnation,
        };
        for entity in &database.schema.entities {
            database.revision(entity.id)?;
        }
        Ok(database)
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
    pub fn revision(&self, entity: u32) -> Result<u64, Error> {
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
    pub fn get<E: Entity>(&self, key: u64) -> Result<Option<E>, Error> {
        self.schema.admits(E::SCHEMA)?;
        self.map
            .get(&row_key(E::SCHEMA.id, key))
            .map(|bytes| {
                if (E::SCHEMA.inspect_row)(&bytes)?.key != key {
                    return Err(Error::Corrupt);
                }
                E::decode(&bytes)
            })
            .transpose()
    }
    /// Read at most 32 rows in primary-key order; continuation is live, not a snapshot.
    /// # Errors
    /// Refuses zero/oversized limits, unadmitted types or corrupt rows.
    pub fn page<E: Entity>(&self, after: Option<u64>, limit: usize) -> Result<Page<E>, Error> {
        self.schema.admits(E::SCHEMA)?;
        if !(1..=32).contains(&limit) {
            return Err(Error::Limit);
        }
        let prefix = prefix(2, E::SCHEMA.id);
        let start = after.map_or_else(|| prefix.clone(), |key| row_key(E::SCHEMA.id, key));
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
            if key.len() != 13 {
                return Err(Error::Corrupt);
            }
            let id = u64::from_be_bytes(key[5..].try_into().map_err(|_| Error::Corrupt)?);
            let bytes = entry.value();
            if (E::SCHEMA.inspect_row)(&bytes)?.key != id {
                return Err(Error::Corrupt);
            }
            rows.push(E::decode(&bytes)?);
            last = Some(id);
        }
        Ok(Page {
            next_after: if rows.len() == limit { last } else { None },
            rows,
        })
    }
    /// Read the first bounded page under an exact leading Nat64 index prefix.
    /// # Errors
    /// Refuses unknown indexes, invalid prefix/limit or inconsistent index entries.
    pub fn index_prefix<E: Entity>(
        &self,
        index: u32,
        values: &[u64],
        limit: usize,
    ) -> Result<Vec<E>, Error> {
        self.schema.admits(E::SCHEMA)?;
        let index = E::SCHEMA
            .indexes
            .iter()
            .find(|value| value.id == index)
            .ok_or(Error::InvalidSchema)?;
        if values.is_empty() || values.len() > index.fields.len() || !(1..=32).contains(&limit) {
            return Err(Error::Limit);
        }
        let mut prefix = index_prefix(E::SCHEMA.id, index.id);
        for value in values {
            prefix.extend(value.to_be_bytes());
        }
        let mut rows = Vec::new();
        for entry in self
            .map
            .range(prefix.clone()..)
            .take_while(|entry| entry.key().starts_with(&prefix))
            .take(limit)
        {
            let id = u64::from_be_bytes(
                entry
                    .value()
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::Corrupt)?,
            );
            let bytes = self
                .map
                .get(&row_key(E::SCHEMA.id, id))
                .ok_or(Error::Corrupt)?;
            let info = (E::SCHEMA.inspect_row)(&bytes)?;
            if info.key != id
                || !index_entries(E::SCHEMA, &info)?
                    .iter()
                    .any(|(key, _)| key == entry.key())
            {
                return Err(Error::Corrupt);
            }
            rows.push(E::decode(&bytes)?);
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
        let mut changed = BTreeSet::new();
        let mut required = BTreeSet::new();
        let mut deleted = Vec::new();
        for mutation in batch.mutations {
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
            if let (Kind::Replace(bytes, _), Some(old)) = (&mutation.kind, &old)
                && bytes == old
            {
                continue;
            }
            if let Some(old) = old {
                let info = (mutation.entity.inspect_row)(&old)?;
                if info.key != mutation.key {
                    return Err(Error::Corrupt);
                }
                for (key, _) in index_entries(mutation.entity, &info)? {
                    if self.map.get(&key) != Some(info.key.to_be_bytes().to_vec()) {
                        return Err(Error::Corrupt);
                    }
                    removes.insert(key);
                }
            }
            changed.insert(mutation.entity.id);
            match mutation.kind {
                Kind::Delete => {
                    deleted.push((mutation.entity.id, mutation.key));
                    removes.insert(key);
                }
                Kind::Insert(bytes, info) | Kind::Replace(bytes, info) => {
                    writes.insert(key, bytes);
                    for (key, is_unique) in index_entries(mutation.entity, &info)? {
                        if is_unique && !unique.insert(key.clone()) {
                            return Err(Error::UniqueViolation);
                        }
                        writes.insert(key, info.key.to_be_bytes().to_vec());
                    }
                    for field in mutation.entity.fields {
                        if let Some(reference) = field.reference {
                            let position = mutation
                                .entity
                                .indexes
                                .iter()
                                .position(|index| index.id == reference.index)
                                .ok_or(Error::Corrupt)?;
                            let bytes = info
                                .indexes
                                .get(position)
                                .and_then(|bytes| bytes.get(..8))
                                .ok_or(Error::Corrupt)?;
                            let target =
                                u64::from_be_bytes(bytes.try_into().map_err(|_| Error::Corrupt)?);
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
        for entity in changed {
            let next = self.revision(entity)?.checked_add(1).ok_or(Error::Limit)?;
            writes.insert(revision_key(entity), next.to_le_bytes().to_vec());
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
        required: &BTreeSet<(u32, u64)>,
        deleted: &[(u32, u64)],
        removes: &BTreeSet<Vec<u8>>,
        writes: &BTreeMap<Vec<u8>, Vec<u8>>,
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
            if (schema.inspect_row)(&bytes)?.key != id {
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
                    prefix.extend(id.to_be_bytes());
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
                        let owner = u64::from_be_bytes(
                            entry
                                .value()
                                .as_slice()
                                .try_into()
                                .map_err(|_| Error::Corrupt)?,
                        );
                        let bytes = self
                            .map
                            .get(&row_key(entity.id, owner))
                            .ok_or(Error::Corrupt)?;
                        let info = (entity.inspect_row)(&bytes)?;
                        if info.key != owner
                            || !index_entries(entity, &info)?
                                .iter()
                                .any(|(key, _)| key == entry.key())
                        {
                            return Err(Error::Corrupt);
                        }
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
fn revision_key(entity: u32) -> Vec<u8> {
    prefix(1, entity)
}
fn row_key(entity: u32, id: u64) -> Vec<u8> {
    let mut key = prefix(2, entity);
    key.extend(id.to_be_bytes());
    key
}
fn index_prefix(entity: u32, index: u32) -> Vec<u8> {
    let mut key = prefix(3, entity);
    key.extend(index.to_be_bytes());
    key
}
fn index_entries(entity: &EntitySchema, info: &RowInfo) -> Result<Vec<(Vec<u8>, bool)>, Error> {
    if info.indexes.len() != entity.indexes.len() {
        return Err(Error::Corrupt);
    }
    entity
        .indexes
        .iter()
        .zip(&info.indexes)
        .map(|(index, values)| {
            if values.len() != index.fields.len() * 8 {
                return Err(Error::Corrupt);
            }
            let mut key = index_prefix(entity.id, index.id);
            key.extend(values);
            if !index.unique {
                key.extend(info.key.to_be_bytes());
            }
            Ok((key, index.unique))
        })
        .collect()
}
