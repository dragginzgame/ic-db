//! Finite compiled named-type graph. No persisted catalog or materialized value tree.

use crate::{
    Error, FieldSchema, FieldType,
    codec::{Decoder, Encoder},
    schema::{MAX_FIELDS, encode_type, name},
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const MAX_DEPTH: usize = 32;
pub(crate) const MAX_NODES: usize = 256;
pub(crate) const MAX_COLLECTION_ITEMS: usize = 256;

/// Compiler-resolved link. Names carry identity; the function breaks constant cycles.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct TypeLink {
    /// Logical name under the stored contract.
    pub name: &'static str,
    /// Resolve the compiled definition, never stored or dynamically loaded code.
    #[serde(skip)]
    pub definition: fn() -> &'static TypeSchema,
}
impl PartialEq for TypeLink {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for TypeLink {}

/// Named schema; its links are shallow so recursive graphs never expand on comparison.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct TypeSchema {
    /// Logical type identity.
    pub name: &'static str,
    /// The complete named value shape, including any wrapped type and its bounds.
    pub body: TypeBody,
}
/// A versioned constraint on the inner primitive of a named newtype.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum NewtypeRule {
    /// A Principal other than the anonymous identity.
    NonAnonymous,
    /// Bounded UTF-8 text with a conjunction of optional refinements.
    Text {
        /// Inner primitive byte limit.
        max_bytes: u32,
        /// Require at least one scalar outside Unicode White_Space.
        non_blank: bool,
        /// Maximum number of Unicode scalar values.
        max_chars: Option<u32>,
    },
}

impl NewtypeRule {
    /// The existing primitive used for storage and ordering.
    #[must_use]
    pub const fn primitive(self) -> FieldType {
        match self {
            Self::NonAnonymous => FieldType::Principal,
            Self::Text { max_bytes, .. } => FieldType::Text { max_bytes },
        }
    }
    pub(crate) fn check_text(self, text: &str) -> Result<(), Error> {
        let mut check = self.text_check().ok_or(Error::InvalidOperand)?;
        check.segment(text)?;
        check.finish()
    }

    pub(crate) fn text_check(self) -> Option<TextCheck> {
        match self {
            Self::Text {
                max_bytes,
                non_blank,
                max_chars,
            } => Some(TextCheck {
                bytes_left: max_bytes as usize,
                chars_left: max_chars.filter(|n| *n < max_bytes).map(|n| n as usize),
                needs_non_blank: non_blank,
            }),
            Self::NonAnonymous => None,
        }
    }
}

// Ordered index text can arrive in segments separated by escaped NUL bytes.
pub(crate) struct TextCheck {
    bytes_left: usize,
    chars_left: Option<usize>,
    needs_non_blank: bool,
}

impl TextCheck {
    pub(crate) fn segment(&mut self, text: &str) -> Result<(), Error> {
        self.bytes_left = self
            .bytes_left
            .checked_sub(text.len())
            .ok_or(Error::Limit)?;
        if let Some(left) = &mut self.chars_left {
            *left = left
                .checked_sub(text.chars().count())
                .ok_or(Error::InvalidOperand)?;
        }
        // Unicode White_Space, fixed for version one of non_blank.
        if self.needs_non_blank {
            self.needs_non_blank = text.chars().all(|c| matches!(c as u32, 0x09..=0x0d | 0x20 | 0x85 | 0xa0 | 0x1680 | 0x2000..=0x200a | 0x2028..=0x2029 | 0x202f | 0x205f | 0x3000));
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<(), Error> {
        if self.needs_non_blank {
            Err(Error::InvalidOperand)
        } else {
            Ok(())
        }
    }
}

impl FieldType {
    pub(crate) fn newtype_rule(self) -> Option<NewtypeRule> {
        if let Self::Named(link) = self
            && let TypeBody::Checked(rule) = (link.definition)().body
        {
            return Some(rule);
        }
        None
    }
}
/// A compiled named type; record/enum members use canonical name order.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub enum TypeBody {
    /// Named wrapper around a primitive or a bounded ordered list.
    Wrapped(FieldType),
    /// Scalar wrapper whose values must satisfy the declared rule.
    Checked(#[serde(serialize_with = "serialize_checked")] NewtypeRule),
    /// Canonical map with scalar/unit-enum keys and arbitrary admitted value shapes.
    Map {
        /// Required primitive, scalar newtype or unit-enum key.
        key: FieldType,
        /// Value type and optionality, independent of map presence.
        value: ValueSchema,
        /// Maximum entries, sharing the enclosing row's collection budget.
        max_items: u32,
    },
    /// Canonical scalar/unit-enum set. Stored elements must be strictly increasing.
    Set {
        /// Primitive, scalar newtype or unit enum with a strict logical order.
        item: FieldType,
        /// Maximum entries, also subject to the enclosing row's shared budget.
        max_items: u32,
    },
    /// Stored fields; nested entity references are not supported.
    Record(&'static [FieldSchema]),
    /// Named alternatives, each with at most one typed payload.
    Enum(&'static [VariantSchema]),
}
// A reader can inspect the storage primitive without knowing how to enforce the rule.
fn serialize_checked<S: serde::Serializer>(
    rule: &NewtypeRule,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeStruct;
    let mut checked = serializer.serialize_struct("Checked", 2)?;
    checked.serialize_field("primitive", &rule.primitive())?;
    checked.serialize_field("rule", rule)?;
    checked.end()
}

/// One unit or payload variant.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct VariantSchema {
    /// Logical variant identity.
    pub name: &'static str,
    /// None means a unit variant, distinct from an optional absent payload.
    pub payload: Option<ValueSchema>,
}

// Ordered capability belongs to the complete enum, including unused variants.
// Bound unchecked operand definitions before comparing them with admitted metadata.
pub(crate) fn unit_enum_len(definition: &TypeSchema) -> Option<usize> {
    let TypeBody::Enum(variants) = &definition.body else {
        return None;
    };

    (!definition.name.is_empty()
        && definition.name.len() <= 64
        && !variants.is_empty()
        && variants.len() <= 64
        && variants
            .iter()
            .all(|v| v.payload.is_none() && !v.name.is_empty() && v.name.len() <= 64)
        && variants.windows(2).all(|w| w[0].name < w[1].name))
    .then_some(variants.len())
}
/// Payload shape, shared with the field type/optionality rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ValueSchema {
    /// Exact scalar, bounded list or compiler-resolved named type.
    pub value: FieldType,
    /// Whether the payload carries an explicit presence byte.
    pub optional: bool,
}
/// Codec boundary generated by named-type macros; handwritten implementations are unsupported.
pub trait Value: Sized {
    /// Logical name; independent of source path and declaration order.
    const NAME: &'static str;
    /// Compiled descriptor, including recursive links.
    fn definition() -> &'static TypeSchema;

    /// Append a value within the enclosing row's shared budget.
    /// # Errors
    /// Refuses field, byte, depth or node bounds.
    #[doc(hidden)]
    fn encode_into(&self, writer: &mut Encoder) -> Result<(), Error>;

    /// Decode within the enclosing row's shared budget.
    /// # Errors
    /// Refuses malformed values and exceeded bounds.
    #[doc(hidden)]
    fn decode_from(reader: &mut Decoder<'_>) -> Result<Self, Error>;
}

#[derive(Default)]
pub(crate) struct TypeGraph {
    definitions: BTreeMap<&'static str, &'static TypeSchema>,
    // Addresses only terminate traversal of compiler-emitted static nodes; they
    // are never identities, persisted bytes or function-pointer comparisons.
    visited: BTreeSet<usize>,
}
impl TypeGraph {
    pub fn include(&mut self, value: FieldType) -> Result<(), Error> {
        let mut pending = Vec::new();
        let mut first = Some(value);
        while let Some(value) = first.take().or_else(|| pending.pop()) {
            let value = if let FieldType::List { item, max_items } = value {
                if max_items > MAX_COLLECTION_ITEMS as u32 || matches!(item, FieldType::List { .. })
                {
                    return Err(Error::InvalidSchema);
                }

                *item
            } else {
                value
            };
            let FieldType::Named(link) = value else {
                continue;
            };
            let definition = (link.definition)();
            if definition.name != link.name {
                return Err(Error::InvalidSchema);
            }
            if !self.visited.insert(std::ptr::from_ref(definition).addr()) {
                continue;
            }
            if self
                .definitions
                .insert(link.name, definition)
                .is_some_and(|old| old != definition)
            {
                return Err(Error::InvalidSchema);
            }
            match &definition.body {
                TypeBody::Wrapped(FieldType::Named(_)) => return Err(Error::InvalidSchema),
                TypeBody::Wrapped(value) => pending.push(*value),
                TypeBody::Checked(_) => {}
                TypeBody::Map {
                    key,
                    value,
                    max_items,
                } => {
                    if *max_items > MAX_COLLECTION_ITEMS as u32 {
                        return Err(Error::InvalidSchema);
                    }
                    Self::validate_ordered(*key)?;
                    pending.push(*key);
                    pending.push(value.value);
                }
                TypeBody::Set { item, max_items } => {
                    if *max_items > MAX_COLLECTION_ITEMS as u32 {
                        return Err(Error::InvalidSchema);
                    }
                    Self::validate_ordered(*item)?;
                    pending.push(*item);
                }
                TypeBody::Record(fields) => {
                    if fields.len() > MAX_FIELDS {
                        return Err(Error::InvalidSchema);
                    }
                    for field in *fields {
                        pending.push(field.value);
                    }
                }
                TypeBody::Enum(variants) => {
                    if variants.len() > 64 {
                        return Err(Error::InvalidSchema);
                    }
                    for variant in *variants {
                        if let Some(payload) = variant.payload {
                            pending.push(payload.value);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_ordered(value: FieldType) -> Result<(), Error> {
        match crate::key::operand_type(value) {
            FieldType::List { .. } => return Err(Error::InvalidSchema),
            FieldType::Named(link) if unit_enum_len((link.definition)()).is_none() => {
                return Err(Error::InvalidSchema);
            }
            _ => {}
        }

        Ok(())
    }

    pub fn definitions(&self) -> impl Iterator<Item = &'static TypeSchema> + '_ {
        self.definitions.values().copied()
    }

    pub fn contains(&self, other: &Self) -> bool {
        other
            .definitions
            .iter()
            .all(|(name, definition)| self.definitions.get(name) == Some(definition))
    }

    pub fn encode(&mut self, bytes: &mut Vec<u8>) -> Result<(), Error> {
        if self.definitions.is_empty() {
            return Ok(());
        }
        bytes.extend(b"ic-db:named-types\0");
        bytes.extend((MAX_DEPTH as u32).to_le_bytes());
        bytes.extend((MAX_NODES as u32).to_le_bytes());
        let count = u32::try_from(self.definitions.len()).map_err(|_| Error::InvalidSchema)?;
        bytes.extend(count.to_le_bytes());
        for definition in self.definitions.values() {
            let (kind, count) = match &definition.body {
                TypeBody::Enum(v) => (1, v.len()),
                TypeBody::Record(f) => (2, f.len()),
                TypeBody::Set { .. } => (3, 1),
                TypeBody::Map { .. } => (4, 2),
                TypeBody::Wrapped(_) => (5, 1),
                TypeBody::Checked(_) => (6, 1),
            };
            bytes.push(kind);
            name(bytes, definition.name)?;
            bytes.extend((count as u32).to_le_bytes());

            match &definition.body {
                TypeBody::Wrapped(value) => encode_type(bytes, *value, false, false)?,
                TypeBody::Checked(rule) => {
                    encode_type(bytes, rule.primitive(), false, false)?;
                    match rule {
                        NewtypeRule::NonAnonymous => bytes.push(1),
                        NewtypeRule::Text {
                            non_blank,
                            max_chars,
                            max_bytes,
                        } => {
                            if (!non_blank && max_chars.is_none())
                                || (*non_blank && (*max_bytes == 0 || *max_chars == Some(0)))
                            {
                                return Err(Error::InvalidSchema);
                            }
                            bytes.extend([2, u8::from(*non_blank), u8::from(max_chars.is_some())]);
                            if let Some(max) = max_chars {
                                bytes.extend(max.to_le_bytes());
                            }
                        }
                    }
                }
                TypeBody::Map {
                    key,
                    value,
                    max_items,
                } => {
                    bytes.extend(max_items.to_le_bytes());
                    bytes.extend((MAX_COLLECTION_ITEMS as u32).to_le_bytes());
                    encode_type(bytes, *key, false, false)?;
                    encode_type(bytes, value.value, value.optional, false)?;
                }
                TypeBody::Set { item, max_items } => {
                    bytes.extend(max_items.to_le_bytes());
                    bytes.extend((MAX_COLLECTION_ITEMS as u32).to_le_bytes());
                    encode_type(bytes, *item, false, false)?;
                }
                TypeBody::Enum(variants) => {
                    if variants.is_empty()
                        || variants.len() > 64
                        || variants.windows(2).any(|w| w[0].name >= w[1].name)
                    {
                        return Err(Error::InvalidSchema);
                    }
                    for variant in *variants {
                        name(bytes, variant.name)?;
                        bytes.push(u8::from(variant.payload.is_some()));
                        if let Some(payload) = variant.payload {
                            encode_type(bytes, payload.value, payload.optional, false)?;
                        }
                    }
                }
                TypeBody::Record(fields) => {
                    if fields.len() > MAX_FIELDS
                        || fields.windows(2).any(|w| w[0].name >= w[1].name)
                    {
                        return Err(Error::InvalidSchema);
                    }
                    for field in *fields {
                        if field.reference.is_some()
                            || field.default.is_some()
                            || field.managed.is_some()
                        {
                            return Err(Error::InvalidSchema);
                        }
                        name(bytes, field.name)?;
                        encode_type(bytes, field.value, field.optional, false)?;
                    }
                }
            }
        }
        self.validate_finite()?;
        self.visited.clear();
        Ok(())
    }

    fn validate_finite(&self) -> Result<(), Error> {
        let mut dependents = BTreeMap::<_, Vec<_>>::new();
        let mut remaining = BTreeMap::new();
        let mut ready = Vec::new();
        for definition in self.definitions() {
            // Only required record/enum edges can propagate a recursive obligation.
            let mut requires = |value: FieldType, optional: bool| {
                if let FieldType::Named(link) = value
                    && !optional
                    && matches!(
                        (link.definition)().body,
                        TypeBody::Record(_) | TypeBody::Enum(_)
                    )
                {
                    dependents
                        .entry(link.name)
                        .or_default()
                        .push(definition.name);
                    true
                } else {
                    false
                }
            };
            // A record needs every required field. An enum needs one finite alternative.
            let needed = match definition.body {
                TypeBody::Record(fields) => fields
                    .iter()
                    .filter(|f| requires(f.value, f.optional))
                    .count(),
                TypeBody::Enum(variants) => usize::from(
                    variants
                        .iter()
                        .all(|v| v.payload.is_some_and(|p| requires(p.value, p.optional))),
                ),
                TypeBody::Wrapped(_)
                | TypeBody::Checked(_)
                | TypeBody::Set { .. }
                | TypeBody::Map { .. } => continue,
            };
            if needed == 0 {
                ready.push(definition.name);
            } else {
                remaining.insert(definition.name, needed);
            }
        }
        while let Some(name) = ready.pop() {
            for parent in dependents.remove(name).into_iter().flatten() {
                if let std::collections::btree_map::Entry::Occupied(mut entry) =
                    remaining.entry(parent)
                {
                    *entry.get_mut() -= 1;
                    if *entry.get() == 0 {
                        entry.remove();
                        ready.push(parent);
                    }
                }
            }
        }
        if remaining.is_empty() {
            Ok(())
        } else {
            Err(Error::InvalidSchema)
        }
    }
}
