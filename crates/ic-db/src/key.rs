//! Exact typed index operands and ordered component bytes; no query execution.

use crate::{
    Account, Date, Decimal, Duration, Error, FieldType, Float32, Float64, Principal, Subaccount,
    Timestamp, TypeSchema, U256, Ulid, codec::Decoder, types::unit_enum_len,
};

/// One exact ordered scalar operand for index prefixes and fluent comparisons.
/// Operand types must match declared fields;
/// integers are never implicitly widened, narrowed or coerced across signedness.
/// This is a bounded scalar boundary, not a general expression/value representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexValue<'a> {
    /// An absent optional component, ordered before present values.
    Absent,
    /// Exact u8 component.
    Nat8(u8),
    /// Exact u16 component.
    Nat16(u16),
    /// Exact u32 component.
    Nat32(u32),
    /// Exact u64 component.
    Nat64(u64),
    /// Exact u128 component.
    Nat128(u128),
    /// Exact unsigned 256-bit component, without narrower-integer coercion.
    U256(U256),
    /// Exact ULID identity, without integer or text coercion.
    Ulid(Ulid),
    /// Exact bounded calendar date; no integer or text coercion.
    Date(Date),
    /// Exact millisecond duration; no integer coercion.
    Duration(Duration),
    /// Exact decimal; the declared field scale must represent it without rounding.
    Decimal(Decimal),
    /// Exact millisecond timestamp; no integer coercion.
    Timestamp(Timestamp),
    /// Exact account identity; no coercion from another key type.
    Account(Account),
    /// Exact subaccount identity; no coercion from another key type.
    Subaccount(Subaccount),
    /// Finite canonical 64-bit float; no cross-width or integer coercion.
    Float64(Float64),
    /// Finite canonical 32-bit float; no cross-width or integer coercion.
    Float32(Float32),
    /// Exact i8 component.
    Int8(i8),
    /// Exact i16 component.
    Int16(i16),
    /// Exact i32 component.
    Int32(i32),
    /// Exact i64 component.
    Int64(i64),
    /// Exact i128 component.
    Int128(i128),
    /// Canonical identity, ordered by length then significant bytes.
    Principal(Principal),
    /// False sorts before true; no numeric coercion.
    Bool(bool),
    /// Exact UTF-8 bytes, without case folding or normalization.
    Text(&'a str),
    /// Exact raw bytes in lexicographic order; shorter byte prefixes sort first.
    Blob(&'a [u8]),
    /// Unit enum in canonical variant-name order, bound to its complete definition.
    UnitEnum {
        /// Declared enum identity and alternatives.
        definition: &'static TypeSchema,
        /// Zero-based ordinal in canonical variant-name order.
        ordinal: u8,
    },
}

// Scalar wrappers preserve their schema name during admission and projection.
// Key encoding and fluent operands use the existing primitive semantics.
pub(crate) fn operand_type(field: FieldType) -> FieldType {
    if let FieldType::Named(link) = field
        && let crate::TypeBody::Wrapped(inner) = (link.definition)().body
    {
        return inner;
    }

    field
}
impl<'a> IndexValue<'a> {
    pub(crate) fn admit(self, field: FieldType, optional: bool) -> Result<(), Error> {
        if self == Self::Absent {
            return if optional {
                Ok(())
            } else {
                Err(Error::InvalidOperand)
            };
        }

        match (self, operand_type(field)) {
            (
                Self::UnitEnum {
                    definition,
                    ordinal,
                },
                FieldType::Named(link),
            ) => {
                let expected = (link.definition)();

                if unit_enum_len(definition).is_some_and(|len| usize::from(ordinal) < len)
                    && unit_enum_len(expected).is_some()
                    && link.name == definition.name
                    && definition == expected
                {
                    Ok(())
                } else {
                    Err(Error::InvalidOperand)
                }
            }
            (Self::Nat8(_), FieldType::Nat8)
            | (Self::Nat16(_), FieldType::Nat16)
            | (Self::Nat32(_), FieldType::Nat32)
            | (Self::Nat64(_), FieldType::Nat64)
            | (Self::Nat128(_), FieldType::Nat128)
            | (Self::Float32(_), FieldType::Float32)
            | (Self::Float64(_), FieldType::Float64)
            | (Self::Subaccount(_), FieldType::Subaccount)
            | (Self::U256(_), FieldType::U256)
            | (Self::Ulid(_), FieldType::Ulid)
            | (Self::Date(_), FieldType::Date)
            | (Self::Duration(_), FieldType::Duration)
            | (Self::Timestamp(_), FieldType::Timestamp)
            | (Self::Int8(_), FieldType::Int8)
            | (Self::Int16(_), FieldType::Int16)
            | (Self::Int32(_), FieldType::Int32)
            | (Self::Int64(_), FieldType::Int64)
            | (Self::Int128(_), FieldType::Int128)
            | (Self::Bool(_), FieldType::Bool) => Ok(()),
            (Self::Decimal(value), FieldType::Decimal { scale }) => value
                .coefficient_at_scale(scale)
                .map(|_| ())
                .ok_or(Error::InvalidOperand),
            (Self::Account(value), FieldType::Account) => {
                Self::Principal(value.owner).admit(FieldType::Principal, false)
            }
            (Self::Principal(value), FieldType::Principal) => {
                if value.as_slice().len() <= crate::codec::MAX_PRINCIPAL_BYTES {
                    Ok(())
                } else {
                    Err(Error::Limit)
                }
            }
            (Self::Text(value), FieldType::Text { max_bytes }) => {
                if value.len() <= max_bytes as usize {
                    Ok(())
                } else {
                    Err(Error::Limit)
                }
            }
            (Self::Blob(value), FieldType::Blob { max_bytes }) => {
                if value.len() <= max_bytes as usize {
                    Ok(())
                } else {
                    Err(Error::Limit)
                }
            }
            _ => Err(Error::InvalidOperand),
        }
    }

    // Append a scalar payload after optional membership has been decided.
    fn append_to(self, out: &mut Vec<u8>) {
        match self {
            Self::Absent => {}
            Self::UnitEnum { ordinal, .. } => out.push(ordinal),
            Self::Nat8(value) => out.extend(value.to_be_bytes()),
            Self::Nat16(value) => out.extend(value.to_be_bytes()),
            Self::Nat32(value) => out.extend(value.to_be_bytes()),
            Self::Nat64(value) => out.extend(value.to_be_bytes()),
            Self::Nat128(value) => out.extend(value.to_be_bytes()),
            Self::Float32(value) => out.extend(value.ordered_bytes()),
            Self::Float64(value) => out.extend(value.ordered_bytes()),
            Self::Subaccount(value) => out.extend(value),
            Self::Account(value) => {
                Self::Principal(value.owner).append_to(out);
                out.extend(value.effective_subaccount());
            }
            Self::U256(value) => out.extend(value.to_be_bytes()),
            Self::Ulid(value) => out.extend(value.to_bytes()),
            Self::Timestamp(value) => Self::Int64(value.as_millis()).append_to(out),
            Self::Decimal(value) => value.append_ordered(out),
            Self::Duration(value) => Self::Nat64(value.as_millis()).append_to(out),
            Self::Date(value) => Self::Int32(value.as_days_since_epoch()).append_to(out),
            Self::Int8(value) => {
                let mut bytes = value.to_be_bytes();
                bytes[0] ^= 0x80;
                out.extend(bytes);
            }
            Self::Int16(value) => {
                let mut bytes = value.to_be_bytes();
                bytes[0] ^= 0x80;
                out.extend(bytes);
            }
            Self::Int32(value) => {
                let mut bytes = value.to_be_bytes();
                bytes[0] ^= 0x80;
                out.extend(bytes);
            }
            Self::Int64(value) => {
                let mut bytes = value.to_be_bytes();
                bytes[0] ^= 0x80;
                out.extend(bytes);
            }
            Self::Int128(value) => {
                let mut bytes = value.to_be_bytes();
                bytes[0] ^= 0x80;
                out.extend(bytes);
            }
            Self::Principal(value) => {
                // Row/query admission enforces the durable 29-byte bound.
                out.push(value.as_slice().len() as u8);
                out.extend(value.as_slice());
            }
            Self::Bool(value) => out.push(u8::from(value)),
            Self::Text(value) => append_bytes(value.as_bytes(), out),
            Self::Blob(value) => append_bytes(value, out),
        }
    }

    pub(crate) fn append_component(self, optional: bool, out: &mut Vec<u8>) {
        if optional {
            out.push(u8::from(self != Self::Absent));
        }
        self.append_to(out);
    }

    pub(crate) fn read(reader: &mut Decoder<'a>, field: FieldType) -> Result<Self, Error> {
        Ok(match operand_type(field) {
            FieldType::Named(link) => {
                let definition = (link.definition)();
                let len = unit_enum_len(definition).ok_or(Error::Corrupt)?;
                let ordinal = reader.unit_enum(len)?;

                Self::UnitEnum {
                    definition,
                    ordinal,
                }
            }
            FieldType::Nat8 => Self::Nat8(reader.nat8()?),
            FieldType::Nat16 => Self::Nat16(reader.nat16()?),
            FieldType::Nat32 => Self::Nat32(reader.nat32()?),
            FieldType::Nat64 => Self::Nat64(reader.nat64()?),
            FieldType::Nat128 => Self::Nat128(reader.nat128()?),
            FieldType::Float32 => Self::Float32(reader.float32()?),
            FieldType::Float64 => Self::Float64(reader.float64()?),
            FieldType::Subaccount => Self::Subaccount(reader.subaccount()?),
            FieldType::Account => Self::Account(reader.account()?),
            FieldType::U256 => Self::U256(reader.u256()?),
            FieldType::Ulid => Self::Ulid(reader.ulid()?),
            FieldType::Date => Self::Date(reader.date()?),
            FieldType::Decimal { scale } => Self::Decimal(reader.decimal(scale)?),
            FieldType::Duration => Self::Duration(reader.duration()?),
            FieldType::Timestamp => Self::Timestamp(reader.timestamp()?),
            FieldType::Int8 => Self::Int8(reader.int8()?),
            FieldType::Int16 => Self::Int16(reader.int16()?),
            FieldType::Int32 => Self::Int32(reader.int32()?),
            FieldType::Int64 => Self::Int64(reader.int64()?),
            FieldType::Int128 => Self::Int128(reader.int128()?),
            FieldType::Principal => Self::Principal(reader.principal()?),
            FieldType::Bool => Self::Bool(reader.boolean()?),
            FieldType::Text { max_bytes } => Self::Text(reader.text_ref(max_bytes as usize)?),
            FieldType::Blob { max_bytes } => Self::Blob(reader.blob_ref(max_bytes as usize)?),
            _ => return Err(Error::Corrupt),
        })
    }
}

// Minimum completion of a query prefix, including unknown trailing components.
// Variable-width identities reserve their shortest valid frame, not their maximum.
pub(crate) fn minimum_bytes(field: FieldType) -> Result<usize, Error> {
    Ok(match operand_type(field) {
        FieldType::Named(link) if unit_enum_len((link.definition)()).is_some() => 1,
        FieldType::Bool
        | FieldType::Nat8
        | FieldType::Int8
        | FieldType::Principal
        | FieldType::Decimal { .. } => 1,
        FieldType::Nat16 | FieldType::Int16 | FieldType::Text { .. } | FieldType::Blob { .. } => 2,
        FieldType::Nat32 | FieldType::Int32 | FieldType::Float32 | FieldType::Date => 4,
        FieldType::Nat64
        | FieldType::Int64
        | FieldType::Float64
        | FieldType::Timestamp
        | FieldType::Duration => 8,
        FieldType::Nat128 | FieldType::Int128 | FieldType::Ulid => 16,
        FieldType::U256 | FieldType::Subaccount => 32,
        FieldType::Account => 33,
        _ => return Err(Error::InvalidOperand),
    })
}

/// Shared generated/stored-row membership and component encoding.
#[doc(hidden)]
#[must_use]
pub fn encode_index(values: &[(IndexValue<'_>, bool)], present_only: bool) -> Option<Vec<u8>> {
    if present_only && values.iter().any(|(value, _)| *value == IndexValue::Absent) {
        return None;
    }
    let mut bytes = Vec::new();
    for &(value, optional) in values {
        value.append_component(optional, &mut bytes);
    }

    Some(bytes)
}

impl<'a, T: Into<IndexValue<'a>>> From<Option<T>> for IndexValue<'a> {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Absent, Into::into)
    }
}

impl From<u8> for IndexValue<'_> {
    fn from(value: u8) -> Self {
        Self::Nat8(value)
    }
}
impl From<u16> for IndexValue<'_> {
    fn from(value: u16) -> Self {
        Self::Nat16(value)
    }
}
impl From<u32> for IndexValue<'_> {
    fn from(value: u32) -> Self {
        Self::Nat32(value)
    }
}
impl From<u64> for IndexValue<'_> {
    fn from(value: u64) -> Self {
        Self::Nat64(value)
    }
}
impl From<u128> for IndexValue<'_> {
    fn from(value: u128) -> Self {
        Self::Nat128(value)
    }
}
impl From<i8> for IndexValue<'_> {
    fn from(value: i8) -> Self {
        Self::Int8(value)
    }
}
impl From<i16> for IndexValue<'_> {
    fn from(value: i16) -> Self {
        Self::Int16(value)
    }
}
impl From<i32> for IndexValue<'_> {
    fn from(value: i32) -> Self {
        Self::Int32(value)
    }
}
impl From<i64> for IndexValue<'_> {
    fn from(value: i64) -> Self {
        Self::Int64(value)
    }
}
impl From<i128> for IndexValue<'_> {
    fn from(value: i128) -> Self {
        Self::Int128(value)
    }
}
impl From<Principal> for IndexValue<'_> {
    fn from(value: Principal) -> Self {
        Self::Principal(value)
    }
}

impl From<U256> for IndexValue<'_> {
    fn from(value: U256) -> Self {
        Self::U256(value)
    }
}

impl From<Ulid> for IndexValue<'_> {
    fn from(value: Ulid) -> Self {
        Self::Ulid(value)
    }
}

impl From<Timestamp> for IndexValue<'_> {
    fn from(value: Timestamp) -> Self {
        Self::Timestamp(value)
    }
}

impl From<Decimal> for IndexValue<'_> {
    fn from(value: Decimal) -> Self {
        Self::Decimal(value)
    }
}

impl From<Duration> for IndexValue<'_> {
    fn from(value: Duration) -> Self {
        Self::Duration(value)
    }
}

impl From<Date> for IndexValue<'_> {
    fn from(value: Date) -> Self {
        Self::Date(value)
    }
}

impl From<Float32> for IndexValue<'_> {
    fn from(value: Float32) -> Self {
        Self::Float32(value)
    }
}

impl From<Float64> for IndexValue<'_> {
    fn from(value: Float64) -> Self {
        Self::Float64(value)
    }
}

impl From<Subaccount> for IndexValue<'_> {
    fn from(value: Subaccount) -> Self {
        Self::Subaccount(value)
    }
}

impl From<Account> for IndexValue<'_> {
    fn from(value: Account) -> Self {
        Self::Account(value)
    }
}

// Schema admission and byte framing share the fixed-width contract here.
fn fixed_width(field: FieldType) -> Option<usize> {
    match field {
        FieldType::Named(link) if unit_enum_len((link.definition)()).is_some() => Some(1),
        FieldType::Nat8 | FieldType::Int8 | FieldType::Bool => Some(1),
        FieldType::Nat16 | FieldType::Int16 => Some(2),
        FieldType::Nat32 | FieldType::Int32 | FieldType::Float32 | FieldType::Date => Some(4),
        FieldType::Nat64
        | FieldType::Int64
        | FieldType::Float64
        | FieldType::Timestamp
        | FieldType::Duration => Some(8),
        FieldType::Nat128 | FieldType::Int128 | FieldType::Ulid => Some(16),
        FieldType::U256 | FieldType::Subaccount => Some(32),
        _ => None,
    }
}

pub(crate) fn supports(field: FieldType) -> bool {
    let field = operand_type(field);

    fixed_width(field).is_some()
        || matches!(
            field,
            FieldType::Principal
                | FieldType::Account
                | FieldType::Text { .. }
                | FieldType::Blob { .. }
                | FieldType::Decimal { .. }
        )
}

// Stored entries and cursor positions share the same optional-component framing.
pub(crate) fn consume_component(
    bytes: &mut &[u8],
    field: &crate::FieldSchema,
    present_only: bool,
    invalid_presence: Error,
) -> Result<(), Error> {
    if field.optional {
        match *bytes {
            [0, rest @ ..] if !present_only => {
                *bytes = rest;

                return Ok(());
            }
            [1, rest @ ..] => *bytes = rest,
            _ => return Err(invalid_presence),
        }
    }

    consume(bytes, field.value)
}

// Consume exactly one self-delimiting component at the key integrity boundary.
pub(crate) fn consume(bytes: &mut &[u8], field: FieldType) -> Result<(), Error> {
    let field = operand_type(field);
    if let FieldType::Named(link) = field {
        let len = unit_enum_len((link.definition)()).ok_or(Error::Corrupt)?;
        let (&ordinal, rest) = bytes.split_first().ok_or(Error::Corrupt)?;
        if usize::from(ordinal) >= len {
            return Err(Error::Corrupt);
        }
        *bytes = rest;

        return Ok(());
    }
    if let FieldType::Decimal { scale } = field {
        let (value, width) = Decimal::read_ordered(bytes).ok_or(Error::Corrupt)?;
        value.coefficient_at_scale(scale).ok_or(Error::Corrupt)?;
        *bytes = &bytes[width..];

        return Ok(());
    }
    if let FieldType::Text { max_bytes } | FieldType::Blob { max_bytes } = field {
        let mut decoded = 0;

        loop {
            let end = bytes
                .iter()
                .position(|&byte| byte == 0)
                .ok_or(Error::Corrupt)?;
            if matches!(field, FieldType::Text { .. }) {
                std::str::from_utf8(&bytes[..end]).map_err(|_| Error::Corrupt)?;
            }
            decoded += end;
            let escaped = *bytes.get(end + 1).ok_or(Error::Corrupt)?;
            *bytes = &bytes[end + 2..];
            match escaped {
                0 if decoded <= max_bytes as usize => return Ok(()),
                255 => decoded += 1,
                _ => return Err(Error::Corrupt),
            }
            if decoded > max_bytes as usize {
                return Err(Error::Corrupt);
            }
        }
    }
    if field == FieldType::Bool && !matches!(bytes.first(), Some(0 | 1)) {
        return Err(Error::Corrupt);
    }
    let width = if matches!(field, FieldType::Principal | FieldType::Account) {
        let (&len, _) = bytes.split_first().ok_or(Error::Corrupt)?;
        if usize::from(len) > crate::codec::MAX_PRINCIPAL_BYTES {
            return Err(Error::Corrupt);
        }

        usize::from(len) + 1 + if field == FieldType::Account { 32 } else { 0 }
    } else {
        fixed_width(field).ok_or(Error::Corrupt)?
    };
    let component = bytes.get(..width).ok_or(Error::Corrupt)?;
    match field {
        FieldType::Date => {
            let mut encoded: [u8; 4] = component.try_into().map_err(|_| Error::Corrupt)?;
            encoded[0] ^= 0x80;
            Date::try_from_days_since_epoch(i32::from_be_bytes(encoded)).ok_or(Error::Corrupt)?;
        }
        FieldType::Float32 => {
            Float32::from_ordered_bytes(component.try_into().map_err(|_| Error::Corrupt)?)
                .ok_or(Error::Corrupt)?;
        }
        FieldType::Float64 => {
            Float64::from_ordered_bytes(component.try_into().map_err(|_| Error::Corrupt)?)
                .ok_or(Error::Corrupt)?;
        }
        _ => {}
    }
    *bytes = &bytes[width..];

    Ok(())
}

impl From<bool> for IndexValue<'_> {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl<'a> From<&'a str> for IndexValue<'a> {
    fn from(value: &'a str) -> Self {
        Self::Text(value)
    }
}

impl<'a> From<&'a String> for IndexValue<'a> {
    fn from(value: &'a String) -> Self {
        Self::Text(value)
    }
}

impl<'a> From<&'a Vec<u8>> for IndexValue<'a> {
    fn from(value: &'a Vec<u8>) -> Self {
        Self::Blob(value)
    }
}

impl<'a, const N: usize> From<&'a [u8; N]> for IndexValue<'a> {
    fn from(value: &'a [u8; N]) -> Self {
        Self::Blob(value)
    }
}

impl<'a> From<&'a [u8]> for IndexValue<'a> {
    fn from(value: &'a [u8]) -> Self {
        Self::Blob(value)
    }
}

// IcyDB's terminated-byte rule, also used for raw bytes: preserve lexical order
// and component boundaries, including empty values and embedded zeroes.
fn append_bytes(value: &[u8], out: &mut Vec<u8>) {
    for &byte in value {
        out.push(byte);
        if byte == 0 {
            out.push(255);
        }
    }
    out.extend([0, 0]);
}
