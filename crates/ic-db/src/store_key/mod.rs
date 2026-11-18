//! One complete-key bound for the shared map, enforced when constructing or decoding a key.
#[cfg(test)]
mod tests;

use crate::Error;
use std::ops::Deref;

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

impl ic_storage::adapter::Storable for StoreKey {
    const BOUND: ic_storage::storable::Bound = ic_storage::storable::Bound::Bounded {
        max_size: MAX_INDEX_KEY_BYTES as u32,
        is_fixed_size: false,
    };

    fn to_bytes(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.0)
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    fn from_bytes(bytes: std::borrow::Cow<'_, [u8]>) -> Self {
        Self::new(bytes.into_owned()).expect("oversized stored DB key")
    }
}
