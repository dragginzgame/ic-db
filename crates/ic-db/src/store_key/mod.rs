//! One complete-key bound for the shared map, enforced before stored payload reads.
use crate::Error;
use ic_storage::adapter::{
    Storable,
    storable::{Bound, CodecError},
};
use std::{borrow::Cow, ops::Deref};

#[cfg(test)]
mod tests;

/// Maximum complete encoded secondary-index key, including namespace and owner suffix.
pub const MAX_INDEX_KEY_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct StoreKey(Vec<u8>);

impl StoreKey {
    pub(crate) fn new(bytes: Vec<u8>) -> Result<Self, Error> {
        if bytes.len() > MAX_INDEX_KEY_BYTES {
            return Err(Error::Limit);
        }
        Ok(Self(bytes))
    }
}
impl Deref for StoreKey {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Storable for StoreKey {
    const BOUND: Bound = Bound::Bounded {
        max_size: MAX_INDEX_KEY_BYTES as u32,
        is_fixed_size: false,
    };
    fn to_bytes(&self) -> Result<Cow<'_, [u8]>, CodecError> {
        Ok(Cow::Borrowed(&self.0))
    }
    fn into_bytes(self) -> Result<Vec<u8>, CodecError> {
        Ok(self.0)
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Result<Self, CodecError> {
        if bytes.len() > MAX_INDEX_KEY_BYTES {
            return Err(CodecError::Bounds);
        }
        Ok(Self(bytes.into_owned()))
    }
}
