//! One frozen source and one exact executable-declared transformation identity.
mod copy;
use super::{Database, Map, live_manifest, manifest_key, query, read_revision};
use crate::{
    Entity, Error, QueryCursor, QueryPage, Schema,
    codec::{Decoder, Encoder},
};
pub use copy::{CopyStep, RetiredSource, SchemaCopy};
use ic_storage::{DefaultMemoryImpl, Memory, RuntimeMemory};
use sha2::{Digest, Sha256};

pub(super) const MAGIC: &[u8; 8] = b"ICDBMT01";
const MAX_NAME: usize = 64;
const MAX_INPUTS: usize = 256;
const MAX_FRAME: usize = 421;

pub(super) fn admits_gate(bytes: &[u8]) -> Result<bool, Error> {
    if bytes.starts_with(MAGIC) {
        decode(bytes)?;
    } else if bytes.starts_with(copy::MAGIC) {
        copy::decode(bytes)?;
    } else {
        return Ok(false);
    }
    Ok(true)
}

/// Identity of one explicitly supported source-to-target transformation.
/// Changing code behavior requires a new version, even when both schemas match.
/// This descriptor does not execute a transformation or authorize target publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaTransition {
    source: [u8; 32],
    target: [u8; 32],
    name: String,
    version: u32,
    inputs: Vec<u8>,
}

impl SchemaTransition {
    /// Bind compiled source/target contracts, a semantic name/version and fixed inputs.
    /// Names contain 1..=64 UTF-8 bytes; fixed inputs contain at most 256 bytes.
    /// # Errors
    /// Refuses identical schemas, version zero or exceeded identity bounds.
    pub fn new(
        source: &Schema,
        target: &Schema,
        name: &str,
        version: u32,
        inputs: &[u8],
    ) -> Result<Self, Error> {
        if source.fingerprint() == target.fingerprint()
            || name.is_empty()
            || name.len() > MAX_NAME
            || version == 0
            || inputs.len() > MAX_INPUTS
        {
            return Err(Error::InvalidTransition);
        }
        Ok(Self {
            source: source.fingerprint(),
            target: target.fingerprint(),
            name: name.into(),
            version,
            inputs: inputs.into(),
        })
    }
    /// Required source contract fingerprint.
    #[must_use]
    pub const fn source(&self) -> [u8; 32] {
        self.source
    }
    /// Intended target contract fingerprint.
    #[must_use]
    pub const fn target(&self) -> [u8; 32] {
        self.target
    }
    /// Exact semantic transformation name, without normalization.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Declared version of the transformation's behavior.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }
    /// Fixed transformation inputs, bound byte-for-byte.
    #[must_use]
    pub fn inputs(&self) -> &[u8] {
        &self.inputs
    }

    fn freeze(&self, incarnation: [u8; 16]) -> Result<Vec<u8>, Error> {
        let mut payload = Encoder::new();
        payload.nat8(0)?; // Only the source-frozen, no-target-allocation phase is admitted.
        payload.nat32(self.version)?;
        payload.bytes(self.name.as_bytes())?;
        payload.bytes(&self.inputs)?;
        let mut bytes = MAGIC.to_vec();
        bytes.extend(self.source);
        bytes.extend(self.target);
        bytes.extend(incarnation);
        bytes.extend(payload.finish());
        Ok(bytes)
    }
}

/// Exclusive handle to a durably frozen source, before any target allocation/copy.
/// Dropping this handle leaves maintenance active. Ordinary reads/writes remain closed.
/// Host adapters own authorization and endpoint exposure.
pub struct Maintenance<M: Memory = DefaultMemoryImpl> {
    map: Map<M>,
    transition: SchemaTransition,
    incarnation: [u8; 16],
}

impl<M: Memory> Database<M> {
    /// Close ordinary access and bind one transition, consuming the live handle.
    /// Performs one synchronous manifest replacement after complete preparation.
    /// A later trap in the enclosing IC message rolls back the freeze as well.
    /// # Errors
    /// Refuses a transition for another source contract before persistent mutation.
    /// # Panics
    /// Supplier failures during mutation trap the enclosing IC message.
    pub fn begin_maintenance(
        mut self,
        transition: &SchemaTransition,
    ) -> Result<Maintenance<M>, Error> {
        if transition.source != self.schema.fingerprint() {
            return Err(Error::InvalidTransition);
        }
        let bytes = transition.freeze(self.incarnation)?;
        let transition = transition.clone();
        self.map.insert(manifest_key(), bytes);
        Ok(Maintenance {
            map: self.map,
            transition,
            incarnation: self.incarnation,
        })
    }
}

impl<M: Memory> Maintenance<M> {
    /// Reopen a frozen source only for the exact supported transition.
    /// # Errors
    /// Refuses live data, different identities, unsupported phases and corrupt frames intact.
    /// # Panics
    /// Supplier corruption traps without repair, as for ordinary database admission.
    pub fn resume(memory: RuntimeMemory<M>, expected: &SchemaTransition) -> Result<Self, Error> {
        let map = Map::load(memory).map_err(Error::Storage)?;
        let bytes = map.get(&manifest_key()).ok_or(Error::Corrupt)?;
        let (transition, incarnation) = decode(&bytes)?;
        if transition != *expected {
            return Err(Error::InvalidTransition);
        }
        Ok(Self {
            map,
            transition,
            incarnation,
        })
    }
    /// Inspect the admitted transformation identity.
    #[must_use]
    pub const fn transition(&self) -> &SchemaTransition {
        &self.transition
    }
    /// Original database incarnation, preserved by cancellation.
    #[must_use]
    pub const fn incarnation(&self) -> [u8; 16] {
        self.incarnation
    }
    /// Read frozen source entities in ascending primary-key order through their
    /// explicitly supplied compiled source schema and decoder. Limits are 1..=32
    /// candidates and 1..=128 KiB of encoded output per call. Only `next == None`
    /// establishes completion; a byte-limited page retries its unreturned row.
    /// Cursors bind the transition, database, entity and source revision. Cancelling,
    /// editing that entity and refreezing invalidates earlier cursors. These are
    /// caller-controlled read positions, not trusted migration checkpoints.
    /// # Errors
    /// Refuses incompatible source schemas/decoders, invalid limits/cursors and
    /// corrupt rows or revisions. An individually oversized row returns `RowTooLarge`.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn source_page<E: Entity>(
        &self,
        source: &Schema,
        after: Option<&QueryCursor>,
        limit: usize,
        bytes: usize,
    ) -> Result<QueryPage<E>, Error> {
        if source.fingerprint() != self.transition.source {
            return Err(Error::IncompatibleSchema);
        }
        source.admits(E::SCHEMA)?;
        let revision = read_revision(&self.map, E::ID)?;
        let mut hash = Sha256::new();
        hash.update(b"ic-db:maintenance-source:v1\0");
        hash.update(self.transition.freeze(self.incarnation)?);
        hash.update(revision.to_le_bytes());
        query::source_page(
            &self.map,
            E::SCHEMA,
            hash.finalize().into(),
            self.incarnation,
            after,
            limit,
            bytes,
        )?
        .decode_entities()
    }
    /// Cancel before target allocation/copy and restore the original live manifest.
    /// Reopening then requires an executable compatible with the source contract.
    /// A later trap in this IC message restores the frozen state.
    /// # Panics
    /// Supplier mutation failures trap the enclosing IC message.
    pub fn cancel(mut self) {
        let bytes = live_manifest(self.transition.source, self.incarnation);
        self.map.insert(manifest_key(), bytes);
    }
}

// Admission is shared by ordinary open and maintenance resume. Unknown future
// phases refuse: an older executable must never cancel a copy it cannot interpret.
pub(super) fn decode(bytes: &[u8]) -> Result<(SchemaTransition, [u8; 16]), Error> {
    if !bytes.starts_with(MAGIC) {
        return Err(Error::InvalidTransition);
    }
    if !(102..=MAX_FRAME).contains(&bytes.len()) {
        return Err(Error::Corrupt);
    }
    let source = bytes[8..40].try_into().map_err(|_| Error::Corrupt)?;
    let target = bytes[40..72].try_into().map_err(|_| Error::Corrupt)?;
    let incarnation = bytes[72..88].try_into().map_err(|_| Error::Corrupt)?;
    let mut reader = Decoder::new(&bytes[88..])?;
    if reader.nat8()? != 0 {
        return Err(Error::IncompatibleSchema);
    }
    let version = reader.nat32()?;
    let name = reader.text(MAX_NAME)?;
    let inputs = reader.blob(MAX_INPUTS)?;
    reader.finish()?;
    if source == target || incarnation == [0; 16] || version == 0 || name.is_empty() {
        return Err(Error::Corrupt);
    }
    Ok((
        SchemaTransition {
            source,
            target,
            name,
            version,
            inputs,
        },
        incarnation,
    ))
}
