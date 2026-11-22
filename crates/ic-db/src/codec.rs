//! Database-owned row codec v3: exact little-endian integers, strict 0/1 tags,
//! and u32-length-prefixed UTF-8/binary fields. Schema admission owns field types
//! and order; this module owns bounded payload encoding, not index ordering.
//!
//! Fixed-width payloads and exact array decoding are adapted from IcyDB's owned
//! db/data/persisted_row/codec/scalar at 6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac.
//! Extend that scalar approach to 128 bits; omit its slot envelope/value layer.

use crate::{
    Account, Date, Decimal, Duration, Error, FieldType, Float32, Float64, Principal, Subaccount,
    Timestamp, TypeBody, TypeLink, U256, Ulid, Value, ValueSchema,
    named_schema::{MAX_COLLECTION_ITEMS, MAX_DEPTH, MAX_NODES, unit_enum_len},
};
use std::collections::{BTreeMap, BTreeSet};

pub const ROW_CODEC_VERSION: u32 = 3;
pub const DEFAULT_ROW_BYTES: usize = 4096;
pub const MAX_ROW_BYTES: usize = 32 * 1024;
// A durable contract bound, independent of any future supplier API expansion.
pub(crate) const MAX_PRINCIPAL_BYTES: usize = 29;

/// Read the length prefix of a Text, Blob or collection span already validated by inspect_fields.
pub(crate) fn validated_length(span: &[u8], optional: bool) -> Option<u32> {
    if optional && span[0] == 0 {
        return None;
    }
    let value = &span[usize::from(optional)..];

    Some(u32::from_le_bytes(
        *value.first_chunk::<4>().expect("validated length prefix"),
    ))
}

pub struct Encoder {
    bytes: Vec<u8>,
    budget: Budget,
    byte_limit: usize,
}

impl Default for Encoder {
    fn default() -> Self {
        Self::with_byte_limit(DEFAULT_ROW_BYTES)
    }
}

impl Encoder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Encoder bounded by the admitted entity row budget.
    #[must_use]
    pub fn for_entity(entity: &crate::EntitySchema) -> Self {
        Self::with_byte_limit(entity.max_row_bytes)
    }

    // Internal envelopes share the scalar encoding without inheriting row policy.
    pub(crate) fn with_byte_limit(byte_limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            budget: Budget::default(),
            byte_limit,
        }
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() > self.byte_limit - self.bytes.len() {
            return Err(Error::Limit);
        }
        self.bytes.extend_from_slice(bytes);

        Ok(())
    }

    pub fn named<T: Value>(&mut self, value: &T) -> Result<(), Error> {
        if matches!(
            T::definition().body,
            TypeBody::Wrapped(_) | TypeBody::Checked(_)
        ) {
            return value.encode_into(self);
        }
        self.budget.enter().map_err(|_| Error::Limit)?;
        let result = value.encode_into(self);
        self.budget.depth -= 1;

        result
    }

    pub fn list<T>(
        &mut self,
        values: &[T],
        max: u32,
        write: fn(&mut Self, &T) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.collection_header(values.len(), max)?;
        self.budget.enter().map_err(|_| Error::Limit)?;
        let result = (|| {
            for value in values {
                write(self, value)?;
            }

            Ok(())
        })();
        self.budget.depth -= 1;

        result
    }

    pub fn nat64(&mut self, value: u64) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    // Called inside a named set; named() already accounts for its depth/node.
    pub fn set<T>(
        &mut self,
        values: &BTreeSet<T>,
        max: u32,
        write: fn(&mut Self, &T) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.collection_header(values.len(), max)?;
        for value in values {
            write(self, value)?;
        }

        Ok(())
    }

    pub fn map<K, V>(
        &mut self,
        values: &BTreeMap<K, V>,
        max: u32,
        write: fn(&mut Self, &K, &V) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.collection_header(values.len(), max)?;
        for (key, value) in values {
            write(self, key, value)?;
        }

        Ok(())
    }

    fn collection_header(&mut self, count: usize, max: u32) -> Result<(), Error> {
        self.budget.items(count, max).map_err(|_| Error::Limit)?;

        self.nat32(count as u32)
    }

    pub fn nat8(&mut self, value: u8) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn nat16(&mut self, value: u16) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn nat32(&mut self, value: u32) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn float32(&mut self, value: Float32) -> Result<(), Error> {
        self.append(&value.get().to_bits().to_le_bytes())
    }

    pub fn float64(&mut self, value: Float64) -> Result<(), Error> {
        self.append(&value.get().to_bits().to_le_bytes())
    }

    pub fn subaccount(&mut self, value: Subaccount) -> Result<(), Error> {
        self.append(&value)
    }

    pub fn ulid(&mut self, value: Ulid) -> Result<(), Error> {
        self.append(&value.to_bytes())
    }

    pub fn timestamp(&mut self, value: Timestamp) -> Result<(), Error> {
        self.int64(value.as_millis())
    }

    pub fn decimal(&mut self, value: Decimal, scale: u32) -> Result<(), Error> {
        let coefficient = value
            .coefficient_at_scale(scale)
            .ok_or(Error::InvalidOperand)?;
        self.append(&coefficient.to_be_bytes())?;

        self.nat8(scale as u8)
    }

    pub fn duration(&mut self, value: Duration) -> Result<(), Error> {
        self.nat64(value.as_millis())
    }

    pub fn date(&mut self, value: Date) -> Result<(), Error> {
        self.int32(value.as_days_since_epoch())
    }

    pub fn account(&mut self, value: Account) -> Result<(), Error> {
        self.principal(&value.owner)?;

        self.subaccount(*value.effective_subaccount())
    }

    pub fn int8(&mut self, value: i8) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn int16(&mut self, value: i16) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn int32(&mut self, value: i32) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn nat128(&mut self, value: u128) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    // IcyDB's owned U256 payload is exactly 32 big-endian bytes. Preserve that
    // choice independently of its slot envelope or transport serialization.
    pub fn u256(&mut self, value: U256) -> Result<(), Error> {
        self.append(&value.to_be_bytes())
    }

    pub fn int64(&mut self, value: i64) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn int128(&mut self, value: i128) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn boolean(&mut self, value: bool) -> Result<(), Error> {
        self.append(&[u8::from(value)])
    }

    pub fn principal(&mut self, value: &Principal) -> Result<(), Error> {
        if value.as_slice().len() > MAX_PRINCIPAL_BYTES {
            return Err(Error::Limit);
        }

        self.bytes(value.as_slice())
    }

    pub fn bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        // Admit the complete framed payload before allocating output. Length is
        // format-defined u32, never a host-sized integer or an adaptive encoding.
        let len = u32::try_from(bytes.len()).map_err(|_| Error::Limit)?;
        let framed = bytes.len().checked_add(4).ok_or(Error::Limit)?;
        if framed > self.byte_limit - self.bytes.len() {
            return Err(Error::Limit);
        }
        self.append(&len.to_le_bytes())?;

        self.append(bytes)
    }

    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

pub struct Decoder<'a> {
    bytes: &'a [u8],
    position: usize,
    budget: Budget,
}

impl<'a> Decoder<'a> {
    pub(crate) const fn position(&self) -> usize {
        self.position
    }

    pub fn new(bytes: &'a [u8]) -> Result<Self, Error> {
        Self::with_byte_limit(bytes, MAX_ROW_BYTES)
    }

    /// Check the full encoded row against its entity budget.
    pub fn for_entity(bytes: &'a [u8], entity: &crate::EntitySchema) -> Result<Self, Error> {
        Self::with_byte_limit(bytes, entity.max_row_bytes)
    }

    pub(crate) fn with_byte_limit(bytes: &'a [u8], byte_limit: usize) -> Result<Self, Error> {
        if bytes.len() > byte_limit {
            return Err(Error::Corrupt);
        }

        Ok(Self {
            bytes,
            position: 0,
            budget: Budget::default(),
        })
    }

    pub fn named<T: Value>(&mut self) -> Result<T, Error> {
        self.with_named(T::definition(), T::decode_from)
    }

    fn with_named<T>(
        &mut self,
        definition: &crate::TypeSchema,
        read: impl FnOnce(&mut Self) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if matches!(definition.body, TypeBody::Wrapped(_) | TypeBody::Checked(_)) {
            return read(self);
        }
        self.budget.enter()?;
        let result = read(self);
        self.budget.depth -= 1;

        result
    }
    // Indexed unit enums still consume the ordinary structured-value budget.
    pub(crate) fn unit_enum(&mut self, variants: usize) -> Result<u8, Error> {
        self.budget.enter()?;
        let result = self.nat8().and_then(|ordinal| {
            if usize::from(ordinal) < variants {
                Ok(ordinal)
            } else {
                Err(Error::Corrupt)
            }
        });
        self.budget.depth -= 1;

        result
    }

    fn list_count(&mut self, max: u32) -> Result<usize, Error> {
        let count = self.collection_count(max)?;
        self.budget.enter()?;

        Ok(count)
    }

    fn collection_count(&mut self, max: u32) -> Result<usize, Error> {
        let count = usize::try_from(self.nat32()?).map_err(|_| Error::Corrupt)?;
        self.budget.items(count, max)?;

        Ok(count)
    }

    pub fn list<T>(
        &mut self,
        max: u32,
        read: fn(&mut Self) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        let count = self.list_count(max)?;
        let result = (|| {
            let mut values = Vec::new();
            for _ in 0..count {
                values.push(read(self)?);
            }

            Ok(values)
        })();
        self.budget.depth -= 1;

        result
    }

    fn skip_named(&mut self, link: TypeLink) -> Result<(), Error> {
        let definition = (link.definition)();

        self.with_named(definition, |reader| {
            match &definition.body {
                TypeBody::Wrapped(value) => reader.skip(*value)?,
                TypeBody::Checked(rule @ crate::NewtypeRule::Text { .. }) => {
                    reader.checked_text(*rule)?;
                }
                TypeBody::Checked(crate::NewtypeRule::NonAnonymous) => {
                    crate::NonAnonymousPrincipal::decode_from(reader)?;
                }
                TypeBody::Set { item, max_items } => {
                    reader.skip_ordered(*item, *max_items, None)?
                }
                TypeBody::Map {
                    key,
                    value,
                    max_items,
                } => reader.skip_ordered(*key, *max_items, Some(*value))?,
                TypeBody::Record(fields) => {
                    for field in *fields {
                        if !field.optional || reader.boolean()? {
                            reader.skip(field.value)?;
                        }
                    }
                }
                TypeBody::Enum(variants) => {
                    let variant = variants
                        .get(usize::from(reader.nat8()?))
                        .ok_or(Error::Corrupt)?;
                    if let Some(payload) = variant.payload
                        && (!payload.optional || reader.boolean()?)
                    {
                        reader.skip(payload.value)?;
                    }
                }
            }

            Ok(())
        })
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], Error> {
        let end = self.position.checked_add(len).ok_or(Error::Corrupt)?;
        let bytes = self.bytes.get(self.position..end).ok_or(Error::Corrupt)?;
        self.position = end;

        Ok(bytes)
    }

    pub fn set<T: Ord>(
        &mut self,
        max: u32,
        read: fn(&mut Self) -> Result<T, Error>,
    ) -> Result<BTreeSet<T>, Error> {
        let count = self.collection_count(max)?;
        let mut values = BTreeSet::new();
        self.ordered(
            count,
            read,
            |_| Ok(()),
            |value, ()| {
                values.insert(value);
            },
        )?;

        Ok(values)
    }

    pub fn map<K: Ord, V>(
        &mut self,
        max: u32,
        read_key: fn(&mut Self) -> Result<K, Error>,
        read_value: fn(&mut Self) -> Result<V, Error>,
    ) -> Result<BTreeMap<K, V>, Error> {
        let count = self.collection_count(max)?;
        let mut values = BTreeMap::new();
        self.ordered(count, read_key, read_value, |key, value| {
            values.insert(key, value);
        })?;

        Ok(values)
    }

    // Validate before materialization can erase duplicate/order evidence. The
    // same walk serves typed construction and borrowed stored-row inspection.
    fn ordered<K: Ord, V>(
        &mut self,
        count: usize,
        mut read_key: impl FnMut(&mut Self) -> Result<K, Error>,
        mut read_value: impl FnMut(&mut Self) -> Result<V, Error>,
        mut accept: impl FnMut(K, V),
    ) -> Result<(), Error> {
        let mut previous = None;
        for _ in 0..count {
            let key = read_key(self)?;
            if let Some((last, value)) = previous {
                if last >= key {
                    return Err(Error::Corrupt);
                }
                accept(last, value);
            }
            previous = Some((key, read_value(self)?));
        }
        if let Some((key, value)) = previous {
            accept(key, value);
        }

        Ok(())
    }

    fn skip_ordered(
        &mut self,
        item: FieldType,
        max: u32,
        payload: Option<ValueSchema>,
    ) -> Result<(), Error> {
        let count = self.collection_count(max)?;
        let read_value = |reader: &mut Self| {
            if let Some(value) = payload
                && (!value.optional || reader.boolean()?)
            {
                reader.skip(value.value)?;
            }

            Ok(())
        };

        if let Some(rule) = item.newtype_rule() {
            return match rule {
                crate::NewtypeRule::Text { .. } => {
                    self.ordered(count, |r| r.checked_text(rule), read_value, |_, ()| {})
                }
                crate::NewtypeRule::NonAnonymous => self.ordered(
                    count,
                    crate::NonAnonymousPrincipal::decode_from,
                    read_value,
                    |_, ()| {},
                ),
            };
        }
        match crate::key::operand_type(item) {
            FieldType::Nat8 => self.ordered(count, Self::nat8, read_value, |_, ()| {}),
            FieldType::Nat16 => self.ordered(count, Self::nat16, read_value, |_, ()| {}),
            FieldType::Nat32 => self.ordered(count, Self::nat32, read_value, |_, ()| {}),
            FieldType::Nat64 => self.ordered(count, Self::nat64, read_value, |_, ()| {}),
            FieldType::Nat128 => self.ordered(count, Self::nat128, read_value, |_, ()| {}),
            FieldType::Float32 => self.ordered(count, Self::float32, read_value, |_, ()| {}),
            FieldType::Float64 => self.ordered(count, Self::float64, read_value, |_, ()| {}),
            FieldType::Subaccount => self.ordered(count, Self::subaccount, read_value, |_, ()| {}),
            FieldType::Account => self.ordered(count, Self::account, read_value, |_, ()| {}),
            FieldType::Ulid => self.ordered(count, Self::ulid, read_value, |_, ()| {}),
            FieldType::Timestamp => self.ordered(count, Self::timestamp, read_value, |_, ()| {}),
            FieldType::Decimal { scale } => {
                self.ordered(count, |r| r.decimal(scale), read_value, |_, ()| {})
            }
            FieldType::Duration => self.ordered(count, Self::duration, read_value, |_, ()| {}),
            FieldType::Date => self.ordered(count, Self::date, read_value, |_, ()| {}),
            FieldType::U256 => self.ordered(count, Self::u256, read_value, |_, ()| {}),
            FieldType::Int8 => self.ordered(count, Self::int8, read_value, |_, ()| {}),
            FieldType::Int16 => self.ordered(count, Self::int16, read_value, |_, ()| {}),
            FieldType::Int32 => self.ordered(count, Self::int32, read_value, |_, ()| {}),
            FieldType::Int64 => self.ordered(count, Self::int64, read_value, |_, ()| {}),
            FieldType::Int128 => self.ordered(count, Self::int128, read_value, |_, ()| {}),
            FieldType::Bool => self.ordered(count, Self::boolean, read_value, |_, ()| {}),
            FieldType::Principal => self.ordered(count, Self::principal, read_value, |_, ()| {}),
            FieldType::Text { max_bytes } => self.ordered(
                count,
                |r| r.text_ref(max_bytes as usize),
                read_value,
                |_, ()| {},
            ),
            FieldType::Blob { max_bytes } => self.ordered(
                count,
                |r| r.blob_ref(max_bytes as usize),
                read_value,
                |_, ()| {},
            ),
            FieldType::Named(link) => {
                let count_variants = unit_enum_len((link.definition)()).ok_or(Error::Corrupt)?;

                self.ordered(
                    count,
                    |r| r.unit_enum(count_variants),
                    read_value,
                    |_, ()| {},
                )
            }
            FieldType::List { .. } => Err(Error::Corrupt),
        }
    }

    // IcyDB's decode_fixed uses this exact slice-to-array check. Here the admitted
    // schema supplies the width in a sequential row; finish rejects surplus data.
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        self.take(N)?.try_into().map_err(|_| Error::Corrupt)
    }

    pub fn nat64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.fixed()?))
    }

    pub fn nat8(&mut self) -> Result<u8, Error> {
        Ok(u8::from_le_bytes(self.fixed()?))
    }

    pub fn nat16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.fixed()?))
    }

    pub fn nat32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.fixed()?))
    }

    pub fn float32(&mut self) -> Result<Float32, Error> {
        Float32::from_canonical_bits(u32::from_le_bytes(self.fixed()?)).ok_or(Error::Corrupt)
    }

    pub fn float64(&mut self) -> Result<Float64, Error> {
        Float64::from_canonical_bits(u64::from_le_bytes(self.fixed()?)).ok_or(Error::Corrupt)
    }

    pub fn subaccount(&mut self) -> Result<Subaccount, Error> {
        self.fixed()
    }

    pub fn ulid(&mut self) -> Result<Ulid, Error> {
        Ok(Ulid::from_bytes(self.fixed()?))
    }

    pub fn timestamp(&mut self) -> Result<Timestamp, Error> {
        Ok(Timestamp::from_millis(self.int64()?))
    }

    pub fn decimal(&mut self, scale: u32) -> Result<Decimal, Error> {
        let coefficient = i128::from_be_bytes(self.fixed()?);
        let stored_scale = u32::from(self.nat8()?);
        if scale > Decimal::MAX_SCALE || stored_scale != scale {
            return Err(Error::Corrupt);
        }

        Decimal::try_new(coefficient, scale).ok_or(Error::Corrupt)
    }

    pub fn duration(&mut self) -> Result<Duration, Error> {
        Ok(Duration::from_millis(self.nat64()?))
    }

    pub fn date(&mut self) -> Result<Date, Error> {
        Date::try_from_days_since_epoch(self.int32()?).ok_or(Error::Corrupt)
    }

    pub fn account(&mut self) -> Result<Account, Error> {
        let owner = self.principal()?;
        let bytes = self.subaccount()?;

        Ok(Account {
            owner,
            subaccount: (bytes != [0; 32]).then_some(bytes),
        })
    }

    pub fn int8(&mut self) -> Result<i8, Error> {
        Ok(i8::from_le_bytes(self.fixed()?))
    }

    pub fn int16(&mut self) -> Result<i16, Error> {
        Ok(i16::from_le_bytes(self.fixed()?))
    }

    pub fn int32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.fixed()?))
    }

    pub fn nat128(&mut self) -> Result<u128, Error> {
        Ok(u128::from_le_bytes(self.fixed()?))
    }

    pub fn u256(&mut self) -> Result<U256, Error> {
        Ok(U256::from_be_bytes(self.fixed()?))
    }

    pub fn int64(&mut self) -> Result<i64, Error> {
        Ok(i64::from_le_bytes(self.fixed()?))
    }

    pub fn int128(&mut self) -> Result<i128, Error> {
        Ok(i128::from_le_bytes(self.fixed()?))
    }

    pub fn boolean(&mut self) -> Result<bool, Error> {
        match self.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::Corrupt),
        }
    }

    fn bounded_bytes(&mut self, max: usize) -> Result<&'a [u8], Error> {
        let len = usize::try_from(u32::from_le_bytes(self.fixed()?)).map_err(|_| Error::Corrupt)?;
        if len > max {
            return Err(Error::Corrupt);
        }

        self.take(len)
    }

    pub fn blob(&mut self, max: usize) -> Result<Vec<u8>, Error> {
        Ok(self.blob_ref(max)?.to_vec())
    }

    pub fn principal(&mut self) -> Result<Principal, Error> {
        // IcyDB's owned scalar decoder admits raw identity bytes, not text or Wire.
        Principal::try_from_slice(self.bounded_bytes(MAX_PRINCIPAL_BYTES)?)
            .map_err(|_| Error::Corrupt)
    }

    pub(crate) fn checked_text(&mut self, rule: crate::NewtypeRule) -> Result<&'a str, Error> {
        let crate::NewtypeRule::Text { max_bytes, .. } = rule else {
            return Err(Error::Corrupt);
        };
        let value = self.text_ref(max_bytes as usize)?;
        rule.check_text(value).map_err(|_| Error::Corrupt)?;
        Ok(value)
    }

    pub fn text(&mut self, max: usize) -> Result<String, Error> {
        Ok(self.text_ref(max)?.to_owned())
    }
    // Borrow from the supplier's row buffer, as IcyDB's direct scalar views do.
    // Owning decode and inspection share these exact length/UTF-8 checks.
    pub(crate) fn blob_ref(&mut self, max: usize) -> Result<&'a [u8], Error> {
        self.bounded_bytes(max)
    }

    pub(crate) fn text_ref(&mut self, max: usize) -> Result<&'a str, Error> {
        std::str::from_utf8(self.bounded_bytes(max)?).map_err(|_| Error::Corrupt)
    }
    // Skipping avoids materialization, not validation. Reuse the scalar decoder
    // so an unselected invalid tag, UTF-8 payload or length cannot be hidden.
    pub(crate) fn skip(&mut self, value: FieldType) -> Result<(), Error> {
        match value {
            FieldType::List { item, max_items } => {
                let count = self.list_count(max_items)?;
                let result = (|| {
                    for _ in 0..count {
                        self.skip(*item)?;
                    }

                    Ok(())
                })();
                self.budget.depth -= 1;
                result?;
            }
            FieldType::Named(link) => self.skip_named(link)?,
            FieldType::Nat8 => {
                self.nat8()?;
            }
            FieldType::Nat16 => {
                self.nat16()?;
            }
            FieldType::Nat32 => {
                self.nat32()?;
            }
            FieldType::Nat64 => {
                self.nat64()?;
            }
            FieldType::Nat128 => {
                self.nat128()?;
            }
            FieldType::Float32 => {
                self.float32()?;
            }
            FieldType::Float64 => {
                self.float64()?;
            }
            FieldType::Subaccount => {
                self.subaccount()?;
            }
            FieldType::Ulid => {
                self.ulid()?;
            }
            FieldType::Timestamp => {
                self.timestamp()?;
            }
            FieldType::Decimal { scale } => {
                self.decimal(scale)?;
            }
            FieldType::Duration => {
                self.duration()?;
            }
            FieldType::Date => {
                self.date()?;
            }
            FieldType::Account => {
                self.account()?;
            }
            FieldType::U256 => {
                self.u256()?;
            }
            FieldType::Int8 => {
                self.int8()?;
            }
            FieldType::Int16 => {
                self.int16()?;
            }
            FieldType::Int32 => {
                self.int32()?;
            }
            FieldType::Int64 => {
                self.int64()?;
            }
            FieldType::Int128 => {
                self.int128()?;
            }
            FieldType::Bool => {
                self.boolean()?;
            }
            FieldType::Principal => {
                self.principal()?;
            }
            FieldType::Text { max_bytes } => {
                self.text_ref(max_bytes as usize)?;
            }
            FieldType::Blob { max_bytes } => {
                self.blob_ref(max_bytes as usize)?;
            }
        }

        Ok(())
    }

    pub fn finish(self) -> Result<(), Error> {
        if self.position != self.bytes.len() {
            return Err(Error::Corrupt);
        }

        Ok(())
    }
}

// Shared by typed encoding, typed decoding and borrowed stored-row inspection.
#[derive(Default)]
struct Budget {
    depth: usize,
    nodes: usize,
    collection_items: usize,
}
impl Budget {
    fn items(&mut self, count: usize, max: u32) -> Result<(), Error> {
        if max > MAX_COLLECTION_ITEMS as u32
            || count > max as usize
            || count > MAX_COLLECTION_ITEMS - self.collection_items
        {
            return Err(Error::Corrupt);
        }
        self.collection_items += count;

        Ok(())
    }

    fn enter(&mut self) -> Result<(), Error> {
        if self.depth >= MAX_DEPTH || self.nodes >= MAX_NODES {
            return Err(Error::Corrupt);
        }
        self.depth += 1;
        self.nodes += 1;

        Ok(())
    }
}
