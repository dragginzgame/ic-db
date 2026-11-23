//! One compiled contract, canonical fingerprint and inspection projection.

use crate::{
    __private::RowInfo,
    Error, IndexValue,
    codec::Decoder,
    key,
    named_schema::{TypeGraph, TypeLink},
    primary::RowKey,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::Arc};

/// Verify producer-owned membership during constant evaluation. Entries are Rust
/// paths relative to the crate, separated by a character forbidden in identifiers.
/// Root entries end with `::` so sibling names sharing a prefix stay independent.
#[doc(hidden)]
#[must_use]
pub const fn check_schema_member(path: &str, root: &str, members: Option<&str>) -> bool {
    const fn listed(path: &[u8], list: &[u8]) -> bool {
        let mut start = 0;
        while start < list.len() {
            let mut end = start;
            let mut same = true;
            while end < list.len() && list[end] != b'|' {
                let i = end - start;
                same = same && i < path.len() && path[i] == list[end];
                end += 1;
            }
            if same && end - start == path.len() {
                return true;
            }
            start = end + 1;
        }

        false
    }
    let path = path.as_bytes();
    let mut first_separator = 0;
    while first_separator < path.len() && path[first_separator] != b':' {
        first_separator += 1;
    }
    let (_, relative) = path.split_at(first_separator);

    let root = root.as_bytes();
    let mut index = 0;
    while index < root.len() {
        if index == relative.len() || relative[index] != root[index] {
            return true;
        }
        index += 1;
    }

    match members {
        Some(paths) => listed(relative, paths.as_bytes()),
        None => false,
    }
}

pub(crate) const MAX_ENTITIES: u8 = 128;
pub(crate) const MAX_FIELDS: usize = 32;

// FNV-1a is only a compact physical handle, never schema authority. Admission
// rejects collisions; full names and semantics enter the SHA-256 contract.
#[doc(hidden)]
#[must_use]
pub const fn handle(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut value = 2_166_136_261_u32;
    let mut position = 0;
    while position < bytes.len() {
        value = (value ^ bytes[position] as u32).wrapping_mul(16_777_619);
        position += 1;
    }

    value
}

/// Stored value semantics supported by this increment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum FieldType {
    /// An ordered bounded list of scalars or named values.
    List {
        /// Exact element type; optional elements and inline lists are not admitted yet.
        item: &'static FieldType,
        /// Maximum number of elements, from zero through 256.
        max_items: u32,
    },
    /// A compiler-resolved named value. Scalar newtypes and unit enums support
    /// secondary keys. Primary and reference keys require supported primitives.
    Named(TypeLink),
    /// Exact unsigned 8-bit integer.
    Nat8,
    /// Exact unsigned 16-bit integer.
    Nat16,
    /// Exact unsigned 32-bit integer.
    Nat32,
    /// Exact signed 8-bit integer.
    Int8,
    /// Exact signed 16-bit integer.
    Int16,
    /// Exact signed 32-bit integer.
    Int32,
    /// Exact unsigned 64-bit integer.
    Nat64,
    /// Exact signed 64-bit integer, without unsigned or floating-point coercion.
    Int64,
    /// Exact unsigned 128-bit integer, without narrowing or floating-point coercion.
    Nat128,
    /// Exact signed 128-bit integer, without narrowing or floating-point coercion.
    Int128,
    /// Exact unsigned 256-bit integer, stored as 32 big-endian bytes.
    U256,
    /// Exact 16-byte ULID, ordered by timestamp prefix then remaining bits.
    Ulid,
    /// Gregorian epoch days, bounded to years 0000 through 9999.
    Date,
    /// Signed Unix milliseconds; full i64 domain.
    Timestamp,
    /// Nonnegative milliseconds; full u64 domain.
    Duration,
    /// Exact decimal, stored at the declared fractional scale without rounding.
    Decimal {
        /// Number of fractional places, 0 through 28.
        scale: u32,
    },
    /// Finite IEEE-754 single precision, canonical positive zero.
    Float32,
    /// Finite IEEE-754 double precision, canonical positive zero.
    Float64,
    /// Boolean, without numeric coercion.
    Bool,
    /// Opaque IC identity, zero to 29 significant bytes; no authority policy.
    Principal,
    /// Exact 32-byte opaque subaccount, ordered lexicographically.
    Subaccount,
    /// Platform account identity: owner then effective subaccount; absent and zero agree.
    Account,
    /// Exact UTF-8 text, bounded in bytes without implicit normalization.
    Text {
        /// Maximum UTF-8 byte length (0..=32768); zero admits only empty text.
        max_bytes: u32,
    },
    /// Exact binary bytes without UTF-8 interpretation or normalization.
    Blob {
        /// Maximum payload length (0..=32768); zero admits only an empty value.
        max_bytes: u32,
    },
}

/// One named stored field; type, bounds, optionality and write policy define identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FieldSchema {
    /// Database-owned insertion/update time policy, never applied during decode.
    pub managed: Option<crate::ManagedTimestamp>,
    /// Compiled insert-omission policy; no default is applied during decode/update.
    pub default: Option<&'static crate::LiteralDefault>,
    /// Stable field identity within its entity.
    pub name: &'static str,
    /// Declared primitive and its bounds.
    pub value: FieldType,
    /// Whether absence is a stored value, distinct from empty/zero/false.
    pub optional: bool,
    /// Present targets must exist; deletion is restricted through a declared index.
    pub reference: Option<ReferenceSchema>,
}

/// A required or optional typed reference using an ordinary reverse-lookup index.
/// An absent optional value has no target; zero remains an ordinary target key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ReferenceSchema {
    /// Full logical target identity; its required key type must match this field.
    pub entity_name: &'static str,
    /// Generated target storage handle.
    #[doc(hidden)]
    pub entity: u32,
    /// Index on this entity whose first component is the referencing field.
    pub index: u32,
}

/// A compiled index over supported exact scalar components.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct IndexSchema {
    /// Generated storage handle; use the name when selecting an index.
    #[doc(hidden)]
    pub id: u32,
    /// Logical identity within its entity, included in the stored contract.
    pub name: &'static str,
    /// Fields in declared index order.
    pub fields: &'static [&'static str],
    /// Whether the complete key must be unique in the final batch state.
    pub unique: bool,
    /// Unique membership requires every optional component to be present.
    pub present_only: bool,
}

impl IndexSchema {
    /// An admitted equality prefix identifies at most one owner when it fixes the entire unique key.
    pub(crate) fn is_unique_lookup(&self, fixed_components: usize) -> bool {
        self.unique && fixed_components == self.fields.len()
    }
}

/// Compiled entity metadata emitted by the entity macro.
#[derive(serde::Serialize)]
pub struct EntitySchema {
    /// Maximum encoded row size, including keys and framing.
    pub max_row_bytes: usize,
    /// Generated storage handle; schema authors do not assign this number.
    #[doc(hidden)]
    pub id: u32,
    /// Logical entity identity, included in the stored contract.
    pub name: &'static str,
    /// Fields in canonical name order.
    pub fields: &'static [FieldSchema],
    /// Supplied scalar primary-key field; its type must implement `PrimaryKey`.
    pub primary_key: &'static str,
    /// Indexes ordered by logical name.
    pub indexes: &'static [IndexSchema],
}

impl EntitySchema {
    pub(crate) fn field_position(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|field| field.name == name)
    }

    pub(crate) fn field_by_name(&self, name: &str) -> Option<&FieldSchema> {
        self.field_position(name)
            .map(|position| &self.fields[position])
    }

    pub(crate) fn field_is_required(&self, path: &str) -> bool {
        self.index_field(path).is_some_and(|field| !field.optional)
    }

    pub(crate) fn record_member(&self, path: &str) -> Option<(usize, crate::member::RecordMember)> {
        let (parent, child) = path.split_once('.')?;
        let position = self.field_position(parent)?;
        let member =
            crate::member::RecordMember::resolve(self.fields[position].value, child).ok()?;
        Some((position, member))
    }

    pub(crate) fn index_field(&self, name: &str) -> Option<crate::ValueSchema> {
        let (parent, member) = crate::member::resolve_field(self, name).ok()?;
        let field = &self.fields[parent];
        let leaf = member.map_or(field, |m| m.field());

        Some(crate::ValueSchema {
            value: leaf.value,
            optional: field.optional || leaf.optional,
        })
    }

    pub(crate) fn index_by_name(&self, name: &str) -> Option<&IndexSchema> {
        self.indexes.iter().find(|index| index.name == name)
    }

    pub(crate) fn has_managed(&self) -> bool {
        self.fields.iter().any(|field| field.managed.is_some())
    }

    pub(crate) fn key_type(&self) -> Result<FieldType, Error> {
        self.field_by_name(self.primary_key)
            .filter(|field| !field.optional)
            .map(|f| f.value)
            .ok_or(Error::Corrupt)
    }
    // Called only for an admitted schema. Validate every stored field but retain
    // only selected key values; no entity or owned text/blob materialization.
    pub(crate) fn inspect_row(&self, bytes: &[u8]) -> Result<RowInfo, Error> {
        let mut values = [IndexValue::Absent; MAX_FIELDS];
        let mut spans = [(0, 0); MAX_FIELDS];
        self.inspect_fields(bytes, self.key_fields(), &mut values, Some(&mut spans))?;

        self.row_info(&values, bytes, &spans)
    }

    pub(crate) fn key_fields(&self) -> u32 {
        self.fields
            .iter()
            .enumerate()
            .fold(0, |mask, (position, field)| {
                if field.name == self.primary_key
                    || self.indexes.iter().any(|i| i.fields.contains(&field.name))
                {
                    mask | (1 << position)
                } else {
                    mask
                }
            })
    }
    // One bounded borrowed field view serves mutation integrity and predicates.
    // Unselected fields are still validated, never silently trusted or materialized.
    // Callers supply fresh absent slots; filling in place avoids copying the view.
    // Projection captures validated byte ranges, including presence tags. There
    // is still one row decoder and one shared depth/node/collection budget.
    pub(crate) fn inspect_fields<'a>(
        &self,
        bytes: &'a [u8],
        selected: u32,
        keys: &mut [IndexValue<'a>; MAX_FIELDS],
        mut spans: Option<&mut [(u16, u16); MAX_FIELDS]>,
    ) -> Result<(), Error> {
        let mut reader = Decoder::for_entity(bytes, self)?;
        let mut times = [None; 2];
        for (position, field) in self.fields.iter().enumerate() {
            let start = reader.position();
            let present = !field.optional || reader.boolean()?;
            if !present {
                // The fresh slot remains absent; its presence tag is projected.
            } else if let Some(managed) = field.managed {
                let value = reader.timestamp()?;
                times[managed.slot()] = Some(value);
                keys[position] = IndexValue::from(value);
            } else if selected & (1 << position) != 0 {
                *keys.get_mut(position).ok_or(Error::Corrupt)? =
                    IndexValue::read(&mut reader, field.value)?;
            } else {
                reader.skip(field.value)?;
            }
            if let Some(spans) = &mut spans {
                spans[position] = (start as u16, reader.position() as u16);
            }
        }
        reader.finish()?;
        crate::managed::validate_order(times)?;

        Ok(())
    }

    pub(crate) fn row_info<'a>(
        &self,
        keys: &[IndexValue<'a>; MAX_FIELDS],
        bytes: &'a [u8],
        spans: &[(u16, u16); MAX_FIELDS],
    ) -> Result<RowInfo, Error> {
        let position = |name: &str| self.field_position(name).ok_or(Error::Corrupt);
        let indexes = self
            .indexes
            .iter()
            .map(|index| {
                let mut components = [(IndexValue::Absent, false); 4];
                for (slot, name) in components.iter_mut().zip(index.fields) {
                    *slot = if let Some(position) = self.field_position(name) {
                        (keys[position], self.fields[position].optional)
                    } else {
                        let (parent, member) = self.record_member(name).ok_or(Error::Corrupt)?;
                        let (start, end) = spans[parent];
                        let optional = self.fields[parent].optional;
                        (
                            member.read(&bytes[usize::from(start)..usize::from(end)], optional)?,
                            optional || member.field().optional,
                        )
                    };
                    if slot.0 == IndexValue::Absent && !slot.1 {
                        return Err(Error::Corrupt);
                    }
                }

                Ok(key::encode_index(
                    &components[..index.fields.len()],
                    index.present_only,
                ))
            })
            .collect::<Result<_, Error>>()?;
        let key = RowKey::from_index(keys[position(self.primary_key)?])?;

        let mut list_links = Vec::new();
        for (field, &(start, end)) in self.fields.iter().zip(spans) {
            if let (Some(reference), FieldType::List { item, max_items }) =
                (field.reference, field.value)
            {
                let mut reader = Decoder::new(&bytes[usize::from(start)..usize::from(end)])?;
                if !field.optional || reader.boolean()? {
                    let count = reader.nat32()?;
                    if count > max_items {
                        return Err(Error::Corrupt);
                    }
                    for _ in 0..count {
                        list_links.push((
                            reference.index,
                            RowKey::from_index(IndexValue::read(&mut reader, *item)?)?,
                        ));
                    }
                }
                reader.finish()?;
            }
        }
        Ok(RowInfo {
            key,
            indexes,
            list_links,
        })
    }
}

/// Validated executable schema. It is never reconstructed from stored data.
/// Clones share the admitted entity roster and named definitions; each open still
/// validates the current durable manifest independently. Revision reads and
/// writes validate the affected counters when used, without caching them.
#[derive(Clone)]
pub struct Schema {
    metadata: Arc<SchemaMetadata>,
    pub(crate) fingerprint: [u8; 32],
}

struct SchemaMetadata {
    entities: Vec<&'static EntitySchema>,
    types: TypeGraph,
}

/// Serializable inspection of one compiled store or a single entity in it.
#[derive(serde::Serialize)]
pub struct SchemaDescription<'a> {
    /// Fingerprint of the entire compiled store, including for an entity selection.
    pub fingerprint: [u8; 32],
    /// Selected entities in logical-name order.
    pub entities: &'a [&'static EntitySchema],
    /// Reachable named definitions in logical-name order. Links serialize by name.
    pub types: Vec<&'static crate::TypeSchema>,
}

impl Schema {
    /// Validate one bounded compiled schema and compute its canonical fingerprint.
    /// # Errors
    /// Refuses duplicate/invalid identities, fields, keys, indexes or capacity.
    pub fn new(entities: &[&'static EntitySchema]) -> Result<Self, Error> {
        Self::admit_schema(entities, |_, _, _, _| {})
    }

    // The host formats reference failures at the same check used by ordinary admission.
    pub(crate) fn admit_schema(
        entities: &[&'static EntitySchema],
        report_reference: impl FnOnce(&str, &FieldSchema, &str, Option<(&str, FieldType)>),
    ) -> Result<Self, Error> {
        if entities.is_empty() || entities.len() > usize::from(MAX_ENTITIES) {
            return Err(Error::InvalidSchema);
        }
        let mut entities = entities.to_vec();
        entities.sort_by_key(|entity| entity.name);
        let mut bytes = Vec::new();
        // The hash domain identifies the compiled schema grammar. These versions
        // identify independent row, key, semantics and supplier contracts.
        // Supplier contract 1 is qualified with Storage 0.74.1; it is not a guess
        // that every future supplier release uses a compatible collection format.
        // Semantics 2 admits complete physical index keys of at most 512 bytes.
        // Older wider-key contracts refuse intact; stored component bytes are unchanged.
        for version in [crate::codec::ROW_CODEC_VERSION, 1, 2, 1] {
            bytes.extend(version.to_le_bytes());
        }
        bytes.extend((entities.len() as u32).to_le_bytes());
        let mut ids = BTreeSet::new();
        let mut types = TypeGraph::default();
        for entity in &entities {
            if entity.id != handle(entity.name)
                || !ids.insert(entity.id)
                || entity.fields.is_empty()
                || !(1..=crate::codec::MAX_ROW_BYTES).contains(&entity.max_row_bytes)
                || entity.fields.len() > MAX_FIELDS
                || entity.indexes.len() > 8
                || !entity.key_type().is_ok_and(RowKey::supports)
                || entity
                    .fields
                    .windows(2)
                    .any(|pair| pair[0].name >= pair[1].name)
            {
                return Err(Error::InvalidSchema);
            }
            name(&mut bytes, entity.name)?;
            if entity.max_row_bytes != crate::codec::DEFAULT_ROW_BYTES {
                // A zero field count is reserved; a real entity always has a key.
                bytes.extend(0_u32.to_le_bytes());
                bytes.extend((entity.max_row_bytes as u32).to_le_bytes());
            }
            bytes.extend((entity.fields.len() as u32).to_le_bytes());
            let mut list_fanout = 0;
            let mut list_indexes = BTreeSet::new();
            for field in entity.fields {
                name(&mut bytes, field.name)?;
                encode_type(
                    &mut bytes,
                    field.value,
                    field.optional,
                    field.reference.is_some(),
                )?;
                types.include(field.value)?;
                if let Some(reference) = field.reference {
                    let Some(target) = entities.iter().find(|target| {
                        target.id == reference.entity && target.name == reference.entity_name
                    }) else {
                        report_reference(entity.name, field, reference.entity_name, None);

                        return Err(Error::InvalidSchema);
                    };
                    let key_type = target.key_type().map_err(|_| Error::InvalidSchema)?;
                    let referenced_type = match field.value {
                        FieldType::List { item, .. } => *item,
                        scalar => scalar,
                    };
                    if referenced_type != key_type {
                        report_reference(
                            entity.name,
                            field,
                            target.name,
                            Some((target.primary_key, key_type)),
                        );

                        return Err(Error::InvalidSchema);
                    }
                    if let FieldType::List { max_items, .. } = field.value {
                        list_fanout += max_items;
                        if list_fanout > 32
                            || reference.index != handle(field.name)
                            || !list_indexes.insert(reference.index)
                        {
                            return Err(Error::InvalidSchema);
                        }
                        name(&mut bytes, target.name)?;
                        name(&mut bytes, field.name)?;
                        continue;
                    }
                    if !RowKey::supports(field.value) {
                        return Err(Error::InvalidSchema);
                    }
                    let index = entity
                        .indexes
                        .iter()
                        .find(|index| {
                            index.id == reference.index
                                && !index.present_only
                                && index.fields.first() == Some(&field.name)
                        })
                        .ok_or(Error::InvalidSchema)?;
                    name(&mut bytes, target.name)?;
                    name(&mut bytes, index.name)?;
                }
            }
            name(&mut bytes, entity.primary_key)?;
            bytes.extend((entity.indexes.len() as u32).to_le_bytes());
            let mut index_ids = BTreeSet::new();
            for index in entity.indexes {
                if index.id != handle(index.name)
                    || !index_ids.insert(index.id)
                    || index.fields.is_empty()
                    || index.fields.len() > 4
                {
                    return Err(Error::InvalidSchema);
                }
                let mut fields = BTreeSet::new();
                let has_optional = index.fields.iter().any(|name| {
                    entity
                        .fields
                        .iter()
                        .any(|field| field.name == *name && field.optional)
                });
                if (index.unique && has_optional && !index.present_only)
                    || (index.present_only && (!index.unique || !has_optional))
                {
                    return Err(Error::InvalidSchema);
                }
                name(&mut bytes, index.name)?;
                bytes.push(u8::from(index.unique) | (u8::from(index.present_only) << 1));
                bytes.extend((index.fields.len() as u32).to_le_bytes());
                for field in index.fields {
                    if !fields.insert(field) {
                        return Err(Error::InvalidSchema);
                    }
                    if let Some(declared) = entity.field_by_name(field) {
                        if !key::supports(declared.value) {
                            return Err(Error::InvalidSchema);
                        }
                        name(&mut bytes, field)?;
                    } else {
                        let (parent, member) =
                            entity.record_member(field).ok_or(Error::InvalidSchema)?;
                        let parent = &entity.fields[parent];
                        let leaf = member.field();
                        if index.unique || index.fields.len() != 1 {
                            return Err(Error::InvalidSchema);
                        }
                        // Outside the 1..=64 length domain of a flat field. Each path part
                        // owns its own frame, including when the index has an explicit name.
                        bytes.extend(u32::MAX.to_le_bytes());
                        name(&mut bytes, parent.name)?;
                        name(&mut bytes, leaf.name)?;
                    }
                }
            }
            if entity
                .indexes
                .windows(2)
                .any(|pair| pair[0].name >= pair[1].name)
            {
                return Err(Error::InvalidSchema);
            }
        }
        types.encode(&mut bytes)?;
        // Versioned policy trailer only for schemas that declare defaults. Existing
        // contracts retain their bytes; the trailer is not a second durable catalog.
        let defaults: Vec<_> = entities
            .iter()
            .flat_map(|entity| {
                entity
                    .fields
                    .iter()
                    .filter_map(move |field| field.default.map(|value| (entity, field, value)))
            })
            .collect();
        if !defaults.is_empty() {
            bytes.extend(b"ic-db:insert-defaults:v1\0");
            bytes.extend((defaults.len() as u32).to_le_bytes());
            for (entity, field, value) in defaults {
                if field.name == entity.primary_key {
                    return Err(Error::InvalidSchema);
                }
                name(&mut bytes, entity.name)?;
                name(&mut bytes, field.name)?;
                let encoded = value.encode(field.value)?;
                bytes.extend((encoded.len() as u32).to_le_bytes());
                bytes.extend(encoded);
            }
        }
        let mut policies = Vec::new();
        for entity in &entities {
            let mut seen = [false; 2];
            for field in entity.fields {
                if let Some(managed) = field.managed {
                    if crate::key::operand_type(field.value) != FieldType::Timestamp
                        || field.optional
                        || field.default.is_some()
                        || field.reference.is_some()
                        || field.name == entity.primary_key
                        || seen[managed.slot()]
                    {
                        return Err(Error::InvalidSchema);
                    }
                    seen[managed.slot()] = true;
                    policies.push((entity.name, field.name, managed));
                }
            }
        }
        if !policies.is_empty() {
            bytes.extend(b"ic-db:managed-times:v1\0");
            bytes.extend((policies.len() as u32).to_le_bytes());
            for (entity, field, managed) in policies {
                name(&mut bytes, entity)?;
                name(&mut bytes, field)?;
                bytes.push(managed.slot() as u8 + 1);
            }
        }
        let mut hash = Sha256::new();
        hash.update(b"ic-db:compiled-schema:v1\0");
        hash.update(bytes);

        Ok(Self {
            metadata: Arc::new(SchemaMetadata { entities, types }),
            fingerprint: hash.finalize().into(),
        })
    }

    /// Full domain-separated contract identity used by the durable manifest.
    #[must_use]
    pub const fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    /// Compiled entities in logical-name order, for bounded inspection.
    #[must_use]
    pub fn entities(&self) -> &[&'static EntitySchema] {
        &self.metadata.entities
    }

    /// Describe the compiled schema without opening storage. A selected entity
    /// includes its reachable named values. References identify other entities
    /// without expanding them. Schema admission bounds the entities and definitions.
    /// # Errors
    /// Returns `UnknownEntity` for an unknown name or a schema error if a type
    /// resolver fails to reproduce its admitted declaration.
    pub fn describe(&self, selected: Option<&str>) -> Result<SchemaDescription<'_>, Error> {
        let (entities, types) = if let Some(name) = selected {
            let position = self
                .entities()
                .binary_search_by_key(&name, |entity| entity.name)
                .map_err(|_| Error::UnknownEntity)?;
            let entity = self.entities()[position];
            let mut reachable = TypeGraph::default();
            for field in entity.fields {
                reachable.include(field.value)?;
            }
            if !self.metadata.types.contains(&reachable) {
                return Err(Error::InvalidSchema);
            }
            (
                &self.entities()[position..=position],
                reachable.definitions().collect(),
            )
        } else {
            (self.entities(), self.types().collect())
        };
        Ok(SchemaDescription {
            fingerprint: self.fingerprint(),
            entities,
            types,
        })
    }

    pub(crate) fn entity_by_id(&self, id: u32) -> Option<&'static EntitySchema> {
        self.entities()
            .iter()
            .copied()
            .find(|entity| entity.id == id)
    }

    pub(crate) fn entity_by_name(&self, name: &str) -> Option<&'static EntitySchema> {
        self.entities()
            .binary_search_by_key(&name, |entity| entity.name)
            .ok()
            .map(|position| self.entities()[position])
    }

    /// Reachable named definitions in logical-name order, without expanding cycles.
    pub fn types(&self) -> impl Iterator<Item = &'static crate::TypeSchema> + '_ {
        self.metadata.types.definitions()
    }

    // Return the existing admitted descriptor after checking the complete type
    // contract. Typed callers can use it without specializing execution around
    // their executable's descriptor address.
    pub(crate) fn admits(&self, proposed: &EntitySchema) -> Result<&'static EntitySchema, Error> {
        let current = self.entity_by_id(proposed.id).ok_or(Error::UnknownEntity)?;
        // The exact immutable descriptor and its compiled links were checked by Schema::new.
        if std::ptr::eq(current, proposed) {
            return Ok(current);
        }
        if current.max_row_bytes != proposed.max_row_bytes
            || current.name != proposed.name
            || current.fields != proposed.fields
            || current.primary_key != proposed.primary_key
            || current.indexes != proposed.indexes
        {
            return Err(Error::UnknownEntity);
        }
        let mut proposed_types = TypeGraph::default();
        for field in proposed.fields {
            proposed_types
                .include(field.value)
                .map_err(|_| Error::UnknownEntity)?;
        }
        if !self.metadata.types.contains(&proposed_types) {
            return Err(Error::UnknownEntity);
        }

        Ok(current)
    }
}

pub(crate) fn name(bytes: &mut Vec<u8>, value: &str) -> Result<(), Error> {
    if value.is_empty() || value.len() > 64 {
        return Err(Error::InvalidSchema);
    }
    bytes.extend((value.len() as u32).to_le_bytes());
    bytes.extend(value.as_bytes());

    Ok(())
}

pub(crate) fn encode_type(
    bytes: &mut Vec<u8>,
    value: FieldType,
    optional: bool,
    reference: bool,
) -> Result<(), Error> {
    let tag = match value {
        FieldType::Nat64 => 1,
        FieldType::Bool => 2,
        FieldType::Text { .. } => 3,
        FieldType::Int64 => 4,
        FieldType::Nat128 => 5,
        FieldType::Int128 => 6,
        FieldType::Blob { .. } => 7,
        FieldType::Nat8 => 8,
        FieldType::Nat16 => 9,
        FieldType::Nat32 => 10,
        FieldType::Int8 => 11,
        FieldType::Int16 => 12,
        FieldType::Int32 => 13,
        FieldType::Principal => 14,
        FieldType::Named(_) => 15,
        FieldType::List { .. } => 16,
        FieldType::U256 => 17,
        FieldType::Float32 => 18,
        FieldType::Float64 => 19,
        FieldType::Subaccount => 20,
        FieldType::Account => 21,
        FieldType::Ulid => 22,
        FieldType::Date => 23,
        FieldType::Timestamp => 24,
        FieldType::Duration => 25,
        FieldType::Decimal { .. } => 26,
    };
    bytes.push(tag | if optional { 0x80 } else { 0 } | if reference { 0x40 } else { 0 });
    match value {
        FieldType::Decimal { scale } => {
            if scale > crate::Decimal::MAX_SCALE {
                return Err(Error::InvalidSchema);
            }
            bytes.extend(scale.to_le_bytes());
        }
        FieldType::Text { max_bytes } | FieldType::Blob { max_bytes } => {
            if max_bytes > crate::codec::MAX_ROW_BYTES as u32 {
                return Err(Error::InvalidSchema);
            }
            bytes.extend(max_bytes.to_le_bytes());
        }
        FieldType::Named(link) => name(bytes, link.name)?,
        FieldType::List { item, max_items } => {
            if max_items > crate::named_schema::MAX_COLLECTION_ITEMS as u32
                || matches!(item, FieldType::List { .. })
            {
                return Err(Error::InvalidSchema);
            }
            for bound in [
                max_items,
                crate::named_schema::MAX_DEPTH as u32,
                crate::named_schema::MAX_NODES as u32,
                crate::named_schema::MAX_COLLECTION_ITEMS as u32,
            ] {
                bytes.extend(bound.to_le_bytes());
            }
            encode_type(bytes, *item, false, false)?;
        }
        _ => {}
    }

    Ok(())
}
