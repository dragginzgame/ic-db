//! Small typed identity boundary over shared commit/traversal execution.
//! Primary and secondary keys share their ordered component encoding.
use crate::{Error, FieldType, IndexValue, U256, Ulid};

mod sealed {
    pub trait Sealed {}
}
/// Supported supplied primary key: fixed-width integers, U256 or Ulid.
/// This sealed boundary keeps lookup/delete/page arguments typed to their entity.
pub trait PrimaryKey: sealed::Sealed + Copy + Eq {
    /// Convert at the generated entity boundary.
    #[doc(hidden)]
    fn into_row_key(self) -> RowKey;
    /// Restore the exact key type after shared storage validation.
    #[doc(hidden)]
    fn from_row_key(key: RowKey) -> Result<Self, Error>;
}
macro_rules! primary_keys {
    ($( $variant:ident($ty:ty) ),* $(,)?) => {
        /// Internal identity used by shared mutation and reference preparation.
        #[doc(hidden)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum RowKey { $( $variant($ty), )* }
        $(
            impl sealed::Sealed for $ty {}
            impl PrimaryKey for $ty {
                fn into_row_key(self) -> RowKey { RowKey::$variant(self) }
                fn from_row_key(key: RowKey) -> Result<Self, Error> {
                    match key { RowKey::$variant(value) => Ok(value), _ => Err(Error::Corrupt) }
                }
            }
        )*
        impl RowKey {
            pub(crate) fn from_index(value: IndexValue<'_>) -> Result<Self, Error> {
                match value { $( IndexValue::$variant(value) => Ok(Self::$variant(value)), )* _ => Err(Error::Corrupt) }
            }
            pub(crate) fn index_value(self) -> IndexValue<'static> {
                match self { $( Self::$variant(value) => value.into(), )* }
            }
        }
    };
}
primary_keys!(
    Nat8(u8),
    Nat16(u16),
    Nat32(u32),
    Nat64(u64),
    Nat128(u128),
    Int8(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    Int128(i128),
    U256(U256),
    Ulid(Ulid)
);
impl RowKey {
    pub(crate) fn width(field: FieldType) -> Result<usize, Error> {
        match field {
            FieldType::Nat8 => Ok(1),
            FieldType::Nat16 => Ok(2),
            FieldType::Nat32 => Ok(4),
            FieldType::Nat64 => Ok(8),
            FieldType::Nat128 => Ok(16),
            FieldType::Int8 => Ok(1),
            FieldType::Int16 => Ok(2),
            FieldType::Int32 => Ok(4),
            FieldType::Int64 => Ok(8),
            FieldType::Int128 => Ok(16),
            FieldType::U256 => Ok(32),
            FieldType::Ulid => Ok(16),
            _ => Err(Error::Corrupt),
        }
    }
    pub(crate) fn decode(bytes: &[u8], field: FieldType) -> Result<Self, Error> {
        match field {
            FieldType::Nat8 => Ok(Self::Nat8(u8::from_be_bytes(
                bytes.try_into().map_err(|_| Error::Corrupt)?,
            ))),
            FieldType::Nat16 => Ok(Self::Nat16(u16::from_be_bytes(
                bytes.try_into().map_err(|_| Error::Corrupt)?,
            ))),
            FieldType::Nat32 => Ok(Self::Nat32(u32::from_be_bytes(
                bytes.try_into().map_err(|_| Error::Corrupt)?,
            ))),
            FieldType::Nat64 => Ok(Self::Nat64(u64::from_be_bytes(
                bytes.try_into().map_err(|_| Error::Corrupt)?,
            ))),
            FieldType::Nat128 => Ok(Self::Nat128(u128::from_be_bytes(
                bytes.try_into().map_err(|_| Error::Corrupt)?,
            ))),
            FieldType::Int8 => {
                let mut bytes: [u8; 1] = bytes.try_into().map_err(|_| Error::Corrupt)?;
                bytes[0] ^= 0x80;
                Ok(Self::Int8(i8::from_be_bytes(bytes)))
            }
            FieldType::Int16 => {
                let mut bytes: [u8; 2] = bytes.try_into().map_err(|_| Error::Corrupt)?;
                bytes[0] ^= 0x80;
                Ok(Self::Int16(i16::from_be_bytes(bytes)))
            }
            FieldType::Int32 => {
                let mut bytes: [u8; 4] = bytes.try_into().map_err(|_| Error::Corrupt)?;
                bytes[0] ^= 0x80;
                Ok(Self::Int32(i32::from_be_bytes(bytes)))
            }
            FieldType::Int64 => {
                let mut bytes: [u8; 8] = bytes.try_into().map_err(|_| Error::Corrupt)?;
                bytes[0] ^= 0x80;
                Ok(Self::Int64(i64::from_be_bytes(bytes)))
            }
            FieldType::Int128 => {
                let mut bytes: [u8; 16] = bytes.try_into().map_err(|_| Error::Corrupt)?;
                bytes[0] ^= 0x80;
                Ok(Self::Int128(i128::from_be_bytes(bytes)))
            }
            FieldType::U256 => Ok(Self::U256(U256::from_be_bytes(
                bytes.try_into().map_err(|_| Error::Corrupt)?,
            ))),
            FieldType::Ulid => Ok(Self::Ulid(Ulid::from_bytes(
                bytes.try_into().map_err(|_| Error::Corrupt)?,
            ))),
            _ => Err(Error::Corrupt),
        }
    }
    pub(crate) fn append_to(self, bytes: &mut Vec<u8>) {
        self.index_value().append_component(false, bytes);
    }
    pub(crate) fn encode(self) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.append_to(&mut bytes);
        bytes
    }
}
