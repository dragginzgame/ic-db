//! One compiled scalar contract, canonical fingerprint and inspection projection.

use crate::{__private::RowInfo, Error};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Stored primitive semantics supported by this increment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    /// Exact unsigned 64-bit integer.
    Nat64,
    /// Boolean, without numeric coercion.
    Bool,
    /// Exact UTF-8 text, bounded in bytes without implicit normalization.
    Text {
        /// Maximum UTF-8 byte length (1..=4096).
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
}

/// A compiled index over ordered Nat64 field components.
#[derive(Debug, PartialEq, Eq)]
pub struct IndexSchema {
    /// Stable index identity within its entity.
    pub id: u32,
    /// Fields in declared index order.
    pub fields: &'static [&'static str],
    /// Whether the complete key must be unique in the final batch state.
    pub unique: bool,
}

/// Metadata and the necessary typed codec boundary emitted by the entity macro.
pub struct EntitySchema {
    /// Stable entity identity.
    pub id: u32,
    /// Presentation label, excluded from persisted identity.
    pub name: &'static str,
    /// Fields in canonical name order.
    pub fields: &'static [FieldSchema],
    /// Externally supplied Nat64 primary-key field.
    pub primary_key: &'static str,
    /// Indexes ordered by identity.
    pub indexes: &'static [IndexSchema],
    /// Generated decoder for key/index maintenance, not a public mutation callback.
    #[doc(hidden)]
    pub inspect_row: fn(&[u8]) -> Result<RowInfo, Error>,
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
        entities.sort_by_key(|entity| entity.id);
        let mut bytes = b"ICDBSC01".to_vec();
        // Contract grammar, row codec, key codec, semantics, supplier contract.
        // Supplier contract 1 is qualified with Storage 0.74.1; it is not a guess
        // that every future supplier release uses a compatible collection format.
        for version in [1_u32; 5] {
            bytes.extend(version.to_le_bytes());
        }
        bytes.extend((entities.len() as u32).to_le_bytes());
        let mut ids = BTreeSet::new();
        for entity in &entities {
            if entity.id == 0
                || !ids.insert(entity.id)
                || entity.fields.is_empty()
                || entity.fields.len() > 16
                || entity.indexes.len() > 8
                || !required_nat64(entity, entity.primary_key)
                || entity
                    .fields
                    .windows(2)
                    .any(|pair| pair[0].name >= pair[1].name)
            {
                return Err(Error::InvalidSchema);
            }
            bytes.extend(entity.id.to_le_bytes());
            bytes.extend((entity.fields.len() as u32).to_le_bytes());
            for field in entity.fields {
                name(&mut bytes, field.name)?;
                let tag = match field.value {
                    FieldType::Nat64 => 1,
                    FieldType::Bool => 2,
                    FieldType::Text { max_bytes } => {
                        if !(1..=4096).contains(&max_bytes) {
                            return Err(Error::InvalidSchema);
                        }
                        3
                    }
                };
                bytes.push(tag | if field.optional { 0x80 } else { 0 });
                if let FieldType::Text { max_bytes } = field.value {
                    bytes.extend(max_bytes.to_le_bytes());
                }
            }
            name(&mut bytes, entity.primary_key)?;
            bytes.extend((entity.indexes.len() as u32).to_le_bytes());
            let mut index_ids = BTreeSet::new();
            for index in entity.indexes {
                if index.id == 0
                    || !index_ids.insert(index.id)
                    || index.fields.is_empty()
                    || index.fields.len() > 4
                {
                    return Err(Error::InvalidSchema);
                }
                let mut fields = BTreeSet::new();
                bytes.extend(index.id.to_le_bytes());
                bytes.push(u8::from(index.unique));
                bytes.extend((index.fields.len() as u32).to_le_bytes());
                for field in index.fields {
                    if !required_nat64(entity, field) || !fields.insert(field) {
                        return Err(Error::InvalidSchema);
                    }
                    name(&mut bytes, field)?;
                }
            }
            if entity
                .indexes
                .windows(2)
                .any(|pair| pair[0].id >= pair[1].id)
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

    /// Compiled entities in stable identity order, for bounded inspection.
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
        if current.fields != proposed.fields
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
