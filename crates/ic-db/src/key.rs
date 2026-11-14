//! Exact typed index operands and ordered component bytes; no query execution.

use crate::{Error, FieldType, Principal, codec::Decoder};

/// One exact leading index component. Operand types must match declared fields;
/// integers are never implicitly widened, narrowed or coerced across signedness.
/// This is a key boundary, not a general expression/value representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexValue {
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
}

impl IndexValue {
    pub(crate) const fn field_type(self) -> FieldType {
        match self {
            Self::Nat8(..) => FieldType::Nat8,
            Self::Nat16(..) => FieldType::Nat16,
            Self::Nat32(..) => FieldType::Nat32,
            Self::Nat64(..) => FieldType::Nat64,
            Self::Nat128(..) => FieldType::Nat128,
            Self::Int8(..) => FieldType::Int8,
            Self::Int16(..) => FieldType::Int16,
            Self::Int32(..) => FieldType::Int32,
            Self::Int64(..) => FieldType::Int64,
            Self::Int128(..) => FieldType::Int128,
            Self::Principal(..) => FieldType::Principal,
        }
    }

    /// Generated code appends already admitted typed components through this owner.
    #[doc(hidden)]
    pub fn __append_to(self, out: &mut Vec<u8>) {
        match self {
            Self::Nat8(value) => out.extend(value.to_be_bytes()),
            Self::Nat16(value) => out.extend(value.to_be_bytes()),
            Self::Nat32(value) => out.extend(value.to_be_bytes()),
            Self::Nat64(value) => out.extend(value.to_be_bytes()),
            Self::Nat128(value) => out.extend(value.to_be_bytes()),
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
        }
    }

    pub(crate) fn read(reader: &mut Decoder<'_>, field: FieldType) -> Result<Self, Error> {
        Ok(match field {
            FieldType::Nat8 => Self::Nat8(reader.nat8()?),
            FieldType::Nat16 => Self::Nat16(reader.nat16()?),
            FieldType::Nat32 => Self::Nat32(reader.nat32()?),
            FieldType::Nat64 => Self::Nat64(reader.nat64()?),
            FieldType::Nat128 => Self::Nat128(reader.nat128()?),
            FieldType::Int8 => Self::Int8(reader.int8()?),
            FieldType::Int16 => Self::Int16(reader.int16()?),
            FieldType::Int32 => Self::Int32(reader.int32()?),
            FieldType::Int64 => Self::Int64(reader.int64()?),
            FieldType::Int128 => Self::Int128(reader.int128()?),
            FieldType::Principal => Self::Principal(reader.principal()?),
            _ => return Err(Error::Corrupt),
        })
    }
}

impl From<u8> for IndexValue {
    fn from(value: u8) -> Self {
        Self::Nat8(value)
    }
}
impl From<u16> for IndexValue {
    fn from(value: u16) -> Self {
        Self::Nat16(value)
    }
}
impl From<u32> for IndexValue {
    fn from(value: u32) -> Self {
        Self::Nat32(value)
    }
}
impl From<u64> for IndexValue {
    fn from(value: u64) -> Self {
        Self::Nat64(value)
    }
}
impl From<u128> for IndexValue {
    fn from(value: u128) -> Self {
        Self::Nat128(value)
    }
}
impl From<i8> for IndexValue {
    fn from(value: i8) -> Self {
        Self::Int8(value)
    }
}
impl From<i16> for IndexValue {
    fn from(value: i16) -> Self {
        Self::Int16(value)
    }
}
impl From<i32> for IndexValue {
    fn from(value: i32) -> Self {
        Self::Int32(value)
    }
}
impl From<i64> for IndexValue {
    fn from(value: i64) -> Self {
        Self::Int64(value)
    }
}
impl From<i128> for IndexValue {
    fn from(value: i128) -> Self {
        Self::Int128(value)
    }
}
impl From<Principal> for IndexValue {
    fn from(value: Principal) -> Self {
        Self::Principal(value)
    }
}

// Schema admission and byte framing share the fixed-width contract here.
fn fixed_width(field: FieldType) -> Option<usize> {
    match field {
        FieldType::Nat8 | FieldType::Int8 => Some(1),
        FieldType::Nat16 | FieldType::Int16 => Some(2),
        FieldType::Nat32 | FieldType::Int32 => Some(4),
        FieldType::Nat64 | FieldType::Int64 => Some(8),
        FieldType::Nat128 | FieldType::Int128 => Some(16),
        _ => None,
    }
}

pub(crate) fn supports(field: FieldType) -> bool {
    fixed_width(field).is_some() || field == FieldType::Principal
}

// Consume exactly one self-delimiting component at the key integrity boundary.
pub(crate) fn consume(bytes: &mut &[u8], field: FieldType) -> Result<(), Error> {
    let width = if field == FieldType::Principal {
        let (&len, _) = bytes.split_first().ok_or(Error::Corrupt)?;
        if usize::from(len) > crate::codec::MAX_PRINCIPAL_BYTES {
            return Err(Error::Corrupt);
        }
        usize::from(len) + 1
    } else {
        fixed_width(field).ok_or(Error::Corrupt)?
    };
    *bytes = bytes.get(width..).ok_or(Error::Corrupt)?;
    Ok(())
}
