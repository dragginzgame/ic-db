//! Exact typed index operands and ordered component bytes; no query execution.

use crate::{Error, FieldType, Principal, codec::Decoder};

/// One exact leading index component. Operand types must match declared fields;
/// integers are never implicitly widened, narrowed or coerced across signedness.
/// This is a key boundary, not a general expression/value representation.
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
        match (self, field) {
            (Self::Nat8(_), FieldType::Nat8)
            | (Self::Nat16(_), FieldType::Nat16)
            | (Self::Nat32(_), FieldType::Nat32)
            | (Self::Nat64(_), FieldType::Nat64)
            | (Self::Nat128(_), FieldType::Nat128)
            | (Self::Int8(_), FieldType::Int8)
            | (Self::Int16(_), FieldType::Int16)
            | (Self::Int32(_), FieldType::Int32)
            | (Self::Int64(_), FieldType::Int64)
            | (Self::Int128(_), FieldType::Int128)
            | (Self::Bool(_), FieldType::Bool) => Ok(()),
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
            _ => Err(Error::InvalidOperand),
        }
    }

    // Append a scalar payload after optional membership has been decided.
    fn append_to(self, out: &mut Vec<u8>) {
        match self {
            Self::Absent => {}
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
            Self::Bool(value) => out.push(u8::from(value)),
            Self::Text(value) => {
                // IcyDB's terminated-byte rule: preserve lexicographic order and
                // component boundaries, including empty text and embedded NUL.
                for byte in value.bytes() {
                    out.push(byte);
                    if byte == 0 {
                        out.push(255);
                    }
                }
                out.extend([0, 0]);
            }
        }
    }

    pub(crate) fn append_component(self, optional: bool, out: &mut Vec<u8>) {
        if optional {
            out.push(u8::from(self != Self::Absent));
        }
        self.append_to(out);
    }

    pub(crate) fn read(reader: &mut Decoder<'a>, field: FieldType) -> Result<Self, Error> {
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
            FieldType::Bool => Self::Bool(reader.boolean()?),
            FieldType::Text { max_bytes } => Self::Text(reader.text_ref(max_bytes as usize)?),
            _ => return Err(Error::Corrupt),
        })
    }
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

// Schema admission and byte framing share the fixed-width contract here.
fn fixed_width(field: FieldType) -> Option<usize> {
    match field {
        FieldType::Nat8 | FieldType::Int8 | FieldType::Bool => Some(1),
        FieldType::Nat16 | FieldType::Int16 => Some(2),
        FieldType::Nat32 | FieldType::Int32 => Some(4),
        FieldType::Nat64 | FieldType::Int64 => Some(8),
        FieldType::Nat128 | FieldType::Int128 => Some(16),
        _ => None,
    }
}

pub(crate) fn supports(field: FieldType) -> bool {
    fixed_width(field).is_some() || matches!(field, FieldType::Principal | FieldType::Text { .. })
}

// Consume exactly one self-delimiting component at the key integrity boundary.
pub(crate) fn consume(bytes: &mut &[u8], field: FieldType) -> Result<(), Error> {
    if let FieldType::Text { max_bytes } = field {
        let mut decoded = 0;
        loop {
            let end = bytes
                .iter()
                .position(|&byte| byte == 0)
                .ok_or(Error::Corrupt)?;
            std::str::from_utf8(&bytes[..end]).map_err(|_| Error::Corrupt)?;
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
