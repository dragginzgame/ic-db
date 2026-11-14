//! One compiled scalar contract, canonical fingerprint and inspection projection.

use crate::{__private::RowInfo, Error, IndexValue, codec::Decoder, key};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const MAX_FIELDS: usize = 16;

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

/// Stored primitive semantics supported by this increment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
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
    /// Boolean, without numeric coercion.
    Bool,
    /// Opaque IC identity, zero to 29 significant bytes; no authority policy.
    Principal,
    /// Exact UTF-8 text, bounded in bytes without implicit normalization.
    Text {
        /// Maximum UTF-8 byte length (0..=4096); zero admits only empty text.
        max_bytes: u32,
    },
    /// Exact binary bytes without UTF-8 interpretation or normalization.
    Blob {
        /// Maximum payload length (0..=4096); zero admits only an empty value.
        max_bytes: u32,
    },
}

/// One named stored field; names, types, bounds and optionality define identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSchema {
    /// Stable field identity within its entity.
    pub name: &'static str,
    /// Declared primitive and its bounds.
    pub value: FieldType,
    /// Whether absence is a stored value, distinct from empty/zero/false.
    pub optional: bool,
    /// Present targets must exist; deletion is restricted through a declared index.
    pub reference: Option<ReferenceSchema>,
}

/// A required or optional Nat64 reference using an ordinary reverse-lookup index.
/// An absent optional value has no target; zero remains an ordinary target key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReferenceSchema {
    /// Full logical target identity; its primary key must be a required Nat64.
    pub entity_name: &'static str,
    /// Generated target storage handle.
    #[doc(hidden)]
    pub entity: u32,
    /// Index on this entity whose first component is the referencing field.
    pub index: u32,
}

/// A compiled index over exact integer, Principal, Bool or Text components.
#[derive(Debug, PartialEq, Eq)]
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

/// Compiled entity metadata emitted by the entity macro.
pub struct EntitySchema {
    /// Generated storage handle; schema authors do not assign this number.
    #[doc(hidden)]
    pub id: u32,
    /// Logical entity identity, included in the stored contract.
    pub name: &'static str,
    /// Fields in canonical name order.
    pub fields: &'static [FieldSchema],
    /// Externally supplied Nat64 primary-key field.
    pub primary_key: &'static str,
    /// Indexes ordered by logical name.
    pub indexes: &'static [IndexSchema],
}

impl EntitySchema {
    // Called only for an admitted schema. Validate every stored field but retain
    // only selected key values; no entity or owned text/blob materialization.
    pub(crate) fn inspect_row(&self, bytes: &[u8]) -> Result<RowInfo, Error> {
        let mut reader = Decoder::new(bytes)?;
        let mut keys = [IndexValue::Absent; MAX_FIELDS];
        for (position, field) in self.fields.iter().enumerate() {
            if field.optional && !reader.boolean()? {
                continue;
            }
            if field.name == self.primary_key
                || self
                    .indexes
                    .iter()
                    .any(|index| index.fields.contains(&field.name))
            {
                *keys.get_mut(position).ok_or(Error::Corrupt)? =
                    IndexValue::read(&mut reader, field.value)?;
            } else {
                reader.skip(field.value)?;
            }
        }
        reader.finish()?;
        let key = |name: &str| {
            self.fields
                .iter()
                .position(|field| field.name == name)
                .and_then(|position| keys.get(position).copied())
                .ok_or(Error::Corrupt)
        };
        let indexes = self
            .indexes
            .iter()
            .map(|index| {
                let mut components = [(IndexValue::Absent, false); 4];
                for (slot, name) in components.iter_mut().zip(index.fields) {
                    let field = self
                        .fields
                        .iter()
                        .find(|field| field.name == *name)
                        .ok_or(Error::Corrupt)?;
                    *slot = (key(name)?, field.optional);
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
        let IndexValue::Nat64(key) = key(self.primary_key)? else {
            return Err(Error::Corrupt);
        };
        Ok(RowInfo { key, indexes })
    }
}

/// Validated executable schema. It is never reconstructed from stored data.
pub struct Schema {
    pub(crate) entities: Vec<&'static EntitySchema>,
    pub(crate) fingerprint: [u8; 32],
}

impl Schema {
    /// Validate one bounded compiled schema and compute its canonical fingerprint.
    /// # Errors
    /// Refuses duplicate/invalid identities, fields, keys, indexes or capacity.
    pub fn new(entities: &[&'static EntitySchema]) -> Result<Self, Error> {
        if entities.is_empty() || entities.len() > 32 {
            return Err(Error::InvalidSchema);
        }
        let mut entities = entities.to_vec();
        entities.sort_by_key(|entity| entity.name);
        let mut bytes = b"ICDBSC01".to_vec();
        // Contract grammar, row codec, key codec, semantics, supplier contract.
        // Supplier contract 1 is qualified with Storage 0.74.1; it is not a guess
        // that every future supplier release uses a compatible collection format.
        for version in [2, crate::codec::ROW_CODEC_VERSION, 1, 1, 1] {
            bytes.extend(version.to_le_bytes());
        }
        bytes.extend((entities.len() as u32).to_le_bytes());
        let mut ids = BTreeSet::new();
        for entity in &entities {
            if entity.id != handle(entity.name)
                || !ids.insert(entity.id)
                || entity.fields.is_empty()
                || entity.fields.len() > MAX_FIELDS
                || entity.indexes.len() > 8
                || !required_nat64(entity, entity.primary_key)
                || entity
                    .fields
                    .windows(2)
                    .any(|pair| pair[0].name >= pair[1].name)
            {
                return Err(Error::InvalidSchema);
            }
            name(&mut bytes, entity.name)?;
            bytes.extend((entity.fields.len() as u32).to_le_bytes());
            for field in entity.fields {
                name(&mut bytes, field.name)?;
                let tag = match field.value {
                    FieldType::Nat8 => 8,
                    FieldType::Nat16 => 9,
                    FieldType::Nat32 => 10,
                    FieldType::Int8 => 11,
                    FieldType::Int16 => 12,
                    FieldType::Int32 => 13,
                    FieldType::Nat64 => 1,
                    FieldType::Int64 => 4,
                    FieldType::Nat128 => 5,
                    FieldType::Int128 => 6,
                    FieldType::Bool => 2,
                    FieldType::Principal => 14,
                    FieldType::Text { max_bytes } => {
                        if max_bytes > 4096 {
                            return Err(Error::InvalidSchema);
                        }
                        3
                    }
                    FieldType::Blob { max_bytes } => {
                        if max_bytes > 4096 {
                            return Err(Error::InvalidSchema);
                        }
                        7
                    }
                };
                bytes.push(
                    tag | if field.optional { 0x80 } else { 0 }
                        | if field.reference.is_some() { 0x40 } else { 0 },
                );
                if let FieldType::Text { max_bytes } | FieldType::Blob { max_bytes } = field.value {
                    bytes.extend(max_bytes.to_le_bytes());
                }
                if let Some(reference) = field.reference {
                    if field.value != FieldType::Nat64
                        || !entities.iter().any(|target| {
                            target.id == reference.entity && target.name == reference.entity_name
                        })
                        || !entity.indexes.iter().any(|index| {
                            index.id == reference.index
                                && !index.present_only
                                && index.fields.first() == Some(&field.name)
                        })
                    {
                        return Err(Error::InvalidSchema);
                    }
                    let target = entities
                        .iter()
                        .find(|target| target.id == reference.entity)
                        .ok_or(Error::InvalidSchema)?;
                    let index = entity
                        .indexes
                        .iter()
                        .find(|index| index.id == reference.index)
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
                    if !entity
                        .fields
                        .iter()
                        .any(|declared| declared.name == *field && key::supports(declared.value))
                        || !fields.insert(field)
                    {
                        return Err(Error::InvalidSchema);
                    }
                    name(&mut bytes, field)?;
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
        let mut hash = Sha256::new();
        hash.update(b"ic-db:schema:v1\0");
        hash.update(bytes);
        Ok(Self {
            entities,
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
        &self.entities
    }

    pub(crate) fn admits(&self, proposed: &EntitySchema) -> Result<(), Error> {
        let current = self
            .entities
            .iter()
            .find(|entity| entity.id == proposed.id)
            .ok_or(Error::UnknownEntity)?;
        if current.name != proposed.name
            || current.fields != proposed.fields
            || current.primary_key != proposed.primary_key
            || current.indexes != proposed.indexes
        {
            return Err(Error::UnknownEntity);
        }
        Ok(())
    }
}

fn required_nat64(entity: &EntitySchema, name: &str) -> bool {
    entity
        .fields
        .iter()
        .any(|field| field.name == name && field.value == FieldType::Nat64 && !field.optional)
}

fn name(bytes: &mut Vec<u8>, value: &str) -> Result<(), Error> {
    if value.is_empty() || value.len() > 64 {
        return Err(Error::InvalidSchema);
    }
    bytes.extend((value.len() as u32).to_le_bytes());
    bytes.extend(value.as_bytes());
    Ok(())
}
