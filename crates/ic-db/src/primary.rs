//! Small typed identity boundary over shared commit/traversal execution.
//! Primary and secondary keys share their ordered component encoding.

use crate::{Account, Error, FieldType, IndexValue, Principal, Subaccount, U256, Ulid, key};

/// An application key branded with the entity that owns it. Constructing one
/// does not read storage or prove that the row exists.
/// An ID for one entity cannot address another entity with the same raw key.
/// ```compile_fail
/// use ic_db::{Database, Entity, Id};
/// fn wrong<A: Entity, B: Entity<Key = A::Key>>(db: &Database, id: Id<A>) {
///     let _ = db.get::<B>(id);
/// }
/// ```
#[repr(transparent)]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(
    transparent,
    bound(
        serialize = "E::Key: serde::Serialize",
        deserialize = "E::Key: serde::Deserialize<'de>"
    )
)]
pub struct Id<E: crate::Entity>(E::Key);

impl<E: crate::Entity> Id<E> {
    /// Associate a raw key with its entity at an application boundary.
    pub const fn from_key(key: E::Key) -> Self {
        Self(key)
    }

    /// Return the stored primary key.
    pub const fn key(self) -> E::Key {
        self.0
    }
}
impl<E: crate::Entity> Copy for Id<E> {}
impl<E: crate::Entity> Clone for Id<E> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<E: crate::Entity> PartialEq for Id<E> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<E: crate::Entity> Eq for Id<E> {}
impl<E: crate::Entity> std::fmt::Debug for Id<E>
where
    E::Key: std::fmt::Debug,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

mod sealed {
    pub trait Sealed {}
}
/// Supported supplied primary key: integers, U256, Ulid, Principal, Account or Subaccount.
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
            impl<E: crate::Entity<Key = $ty>> From<$ty> for Id<E> {
                fn from(key: $ty) -> Self { Self::from_key(key) }
            }
            impl sealed::Sealed for $ty {}
            impl PrimaryKey for $ty {
                fn into_row_key(self) -> RowKey { RowKey::$variant(self) }

                fn from_row_key(key: RowKey) -> Result<Self, Error> {
                    match key { RowKey::$variant(value) => Ok(value), _ => Err(Error::Corrupt) }
                }
            }
        )*
        impl RowKey {
            pub(crate) fn supports(field: FieldType) -> bool {
                matches!(field, $( FieldType::$variant )|*)
            }

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
    Ulid(Ulid),
    Principal(Principal),
    Subaccount(Subaccount),
    Account(Account)
);
impl RowKey {
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
            FieldType::Subaccount => Ok(Self::Subaccount(
                bytes.try_into().map_err(|_| Error::Corrupt)?,
            )),
            FieldType::Principal | FieldType::Account => {
                let mut remaining = bytes;
                key::consume(&mut remaining, field)?;
                if !remaining.is_empty() {
                    return Err(Error::Corrupt);
                }
                // Component admission proves the owner length and, for an
                // account, the complete effective-subaccount suffix.
                let owner_end = 1 + usize::from(bytes[0]);
                let owner =
                    Principal::try_from_slice(&bytes[1..owner_end]).map_err(|_| Error::Corrupt)?;

                if field == FieldType::Principal {
                    Ok(Self::Principal(owner))
                } else {
                    let subaccount: Subaccount =
                        bytes[owner_end..].try_into().map_err(|_| Error::Corrupt)?;

                    Ok(Self::Account(Account {
                        owner,
                        subaccount: (subaccount != [0; 32]).then_some(subaccount),
                    }))
                }
            }
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
