//! Independent physical-byte oracle for DB allocations. Storage 0.80 persists
//! both codecs' bounds, even when the caller only needs to inspect raw bytes.
use ic_storage::{
    Memory, RuntimeMemory,
    adapter::{Bound, MapEntry, StableBTreeMap, Storable},
};
use std::{
    borrow::Cow,
    ops::{Deref, RangeBounds},
};

/// Raw variable-width key with the DB allocation's declared 512-byte maximum.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RawKey(pub Vec<u8>);

impl Deref for RawKey {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl Storable for RawKey {
    const BOUND: Bound = Bound::Bounded {
        max_size: 512,
        is_fixed_size: false,
    };
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        Self(bytes.into_owned())
    }
}

/// Raw DB allocation view used only in tests and measurement probes.
pub struct RawMap<M: Memory>(StableBTreeMap<RawKey, Vec<u8>, M>);

impl<M: Memory> RawMap<M> {
    /// Open an existing DB collection with its declared physical bounds.
    #[must_use]
    pub fn load(memory: RuntimeMemory<M>) -> Self {
        Self(StableBTreeMap::load(memory))
    }
    /// Read a physical value.
    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.get(&RawKey(key.to_vec()))
    }
    /// Set a physical value.
    pub fn insert(&mut self, key: Vec<u8>, value: Vec<u8>) -> Option<Vec<u8>> {
        self.0.insert(RawKey(key), value)
    }
    /// Remove a physical entry.
    pub fn remove(&mut self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.remove(&RawKey(key.to_vec()))
    }
    /// Traverse the stored entries.
    pub fn iter(&self) -> impl Iterator<Item = MapEntry<RawKey, Vec<u8>>> {
        self.0.iter()
    }
    /// Traverse the supplied physical interval.
    pub fn range(
        &self,
        bounds: impl RangeBounds<Vec<u8>>,
    ) -> impl Iterator<Item = MapEntry<RawKey, Vec<u8>>> {
        self.0.range((
            bounds.start_bound().cloned().map(RawKey),
            bounds.end_bound().cloned().map(RawKey),
        ))
    }
    /// Traverse the physical keys.
    pub fn keys(&self) -> impl Iterator<Item = Vec<u8>> {
        self.0.keys().map(|key| key.0)
    }
}
