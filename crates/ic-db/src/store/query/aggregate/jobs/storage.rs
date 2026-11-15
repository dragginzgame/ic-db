//! Fixed slots in one host-owned allocation; bounded frames use the owned codec.
use super::{JobHandle, JobOutcome, MAX_CANDIDATES};
use crate::{
    Error, QueryCursor, Timestamp,
    codec::{Decoder, Encoder},
};
use ic_storage::{DefaultMemoryImpl, Memory, RuntimeMemory, adapter::StableBTreeMap};

pub(super) const SLOTS: u8 = 16;
const MAGIC: &[u8; 8] = b"ICDBAJ01";

/// Durable scalar/grouped counts, sums and averages in a host-governed allocation.
/// Holds at most 16 jobs, each at most 4096 encoded bytes. Creation requires a
/// fresh nonzero incarnation. Handles identify jobs; they do not authorize access.
/// All mutation must run synchronously within one IC message, without awaits.
/// Supplier failures trap; a later message trap rolls these changes back too.
pub struct AggregateJobs<M: Memory = DefaultMemoryImpl> {
    map: StableBTreeMap<u8, Vec<u8>, M>,
    incarnation: [u8; 16],
}
impl<M: Memory> AggregateJobs<M> {
    /// Create in an empty allocation, with a host-supplied fresh incarnation.
    /// # Errors
    /// Refuses a zero incarnation or a nonempty/unsupported supplier allocation.
    /// # Panics
    /// Supplier failures after mutation begins must trap the enclosing message.
    pub fn create(memory: RuntimeMemory<M>, incarnation: [u8; 16]) -> Result<Self, Error> {
        if incarnation == [0; 16] {
            return Err(Error::InvalidJob);
        }
        let mut map = StableBTreeMap::create(memory).map_err(Error::Storage)?;
        map.insert(0, header(incarnation, 0));
        Ok(Self { map, incarnation })
    }

    /// Reopen a supported job-store format without modifying it.
    /// # Errors
    /// Refuses missing/corrupt headers and unsupported formats intact.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn open(memory: RuntimeMemory<M>) -> Result<Self, Error> {
        let map: StableBTreeMap<u8, Vec<u8>, M> =
            StableBTreeMap::load(memory).map_err(Error::Storage)?;
        let bytes = map.get(&0).ok_or(Error::Corrupt)?;
        let (incarnation, _) = read_header(&bytes)?;
        if map.len() > u64::from(SLOTS) + 1
            || map
                .range(..)
                .next_back()
                .is_some_and(|entry| *entry.key() > SLOTS)
        {
            return Err(Error::Corrupt);
        }
        Ok(Self { map, incarnation })
    }

    /// Cancel one job, immediately releasing its logical slot. Repeated/stale
    /// cancellation refuses; it cannot delete a subsequent occupant of that slot.
    /// # Errors
    /// Refuses a foreign, missing, stale or malformed handle/job frame. Cancellation
    /// need not interpret a known frame's query-specific accumulator payload.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn cancel(&mut self, handle: &JobHandle) -> Result<(), Error> {
        self.load(handle)?;
        self.map.remove(&handle.slot);
        Ok(())
    }

    /// Examine at most 16 slots and remove at most `limit` expired jobs (1..=16).
    /// Time must be captured by the host; the database reads no clock. This does
    /// not shrink stable memory or promise physical reclamation by the supplier.
    /// # Errors
    /// Refuses invalid limits or malformed records before removing any job.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn cleanup_expired(&mut self, now: Timestamp, limit: usize) -> Result<usize, Error> {
        if !(1..=usize::from(SLOTS)).contains(&limit) {
            return Err(Error::Limit);
        }
        let mut expired = Vec::new();
        for slot in 1..=SLOTS {
            if let Some(bytes) = self.map.get(&slot) {
                let record = Record::decode(&bytes)?;
                if record.expires <= now && expired.len() < limit {
                    expired.push(slot);
                }
            }
        }
        for slot in &expired {
            self.map.remove(slot);
        }
        Ok(expired.len())
    }

    pub(super) fn insert(&mut self, mut record: Record) -> Result<JobHandle, Error> {
        let slot = (1..=SLOTS)
            .find(|slot| !self.map.contains_key(slot))
            .ok_or(Error::JobCapacity)?;
        let (incarnation, generation) = read_header(&self.map.get(&0).ok_or(Error::Corrupt)?)?;
        if incarnation != self.incarnation {
            return Err(Error::Corrupt);
        }
        let generation = generation.checked_add(1).ok_or(Error::Limit)?;
        record.generation = generation;
        let bytes = record.encode()?;
        // No recoverable refusal after the first persistent write.
        self.map.insert(0, header(incarnation, generation));
        self.map.insert(slot, bytes);
        Ok(JobHandle {
            incarnation,
            slot,
            generation,
        })
    }

    pub(super) fn load(&self, handle: &JobHandle) -> Result<Record, Error> {
        if handle.incarnation != self.incarnation {
            return Err(Error::InvalidJob);
        }
        let bytes = self.map.get(&handle.slot).ok_or(Error::InvalidJob)?;
        let record = Record::decode(&bytes)?;
        if record.generation != handle.generation {
            return Err(Error::InvalidJob);
        }
        Ok(record)
    }

    pub(super) fn save(&mut self, handle: &JobHandle, record: &Record) -> Result<(), Error> {
        let bytes = record.encode()?;
        self.map.insert(handle.slot, bytes);
        Ok(())
    }
}
fn header(incarnation: [u8; 16], generation: u64) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend(crate::codec::ROW_CODEC_VERSION.to_le_bytes());
    bytes.extend(incarnation);
    bytes.extend(generation.to_le_bytes());
    bytes
}
fn read_header(bytes: &[u8]) -> Result<([u8; 16], u64), Error> {
    if bytes.len() != 36 {
        return Err(Error::Corrupt);
    }
    if &bytes[..8] != MAGIC || bytes[8..12] != crate::codec::ROW_CODEC_VERSION.to_le_bytes() {
        return Err(Error::InvalidJob);
    }
    let incarnation = bytes[12..28].try_into().map_err(|_| Error::Corrupt)?;
    if incarnation == [0; 16] {
        return Err(Error::Corrupt);
    }
    Ok((
        incarnation,
        u64::from_le_bytes(bytes[28..].try_into().map_err(|_| Error::Corrupt)?),
    ))
}

pub(super) struct Record {
    pub generation: u64,
    pub binding: [u8; 32],
    pub revision: u64,
    pub expires: Timestamp,
    pub observed: Timestamp,
    pub sequence: u64,
    pub examined: u64,
    pub input_bytes: u64,
    pub phase: JobOutcome<()>,
    pub cursor: Vec<u8>,
    pub count: u64,
    // Interpretation belongs to the query-bound accumulator, not the job store.
    pub state: Vec<u8>,
}
impl Record {
    fn encode(&self) -> Result<Vec<u8>, Error> {
        let mut e = Encoder::new();
        e.nat64(self.generation)?;
        e.bytes(&self.binding)?;
        e.nat64(self.revision)?;
        e.timestamp(self.expires)?;
        e.timestamp(self.observed)?;
        e.nat64(self.sequence)?;
        e.nat64(self.examined)?;
        e.nat64(self.input_bytes)?;
        e.nat8(match self.phase {
            JobOutcome::Running => 0,
            JobOutcome::Complete(()) => 1,
            JobOutcome::Invalidated => 2,
            JobOutcome::Expired => 3,
        })?;
        e.bytes(&self.cursor)?;
        e.nat64(self.count)?;
        e.bytes(&self.state)?;
        Ok(e.finish())
    }
    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut d = Decoder::new(bytes)?;
        let record = Self {
            generation: d.nat64()?,
            binding: d.blob_ref(32)?.try_into().map_err(|_| Error::Corrupt)?,
            revision: d.nat64()?,
            expires: d.timestamp()?,
            observed: d.timestamp()?,
            sequence: d.nat64()?,
            examined: d.nat64()?,
            input_bytes: d.nat64()?,
            phase: match d.nat8()? {
                0 => JobOutcome::Running,
                1 => JobOutcome::Complete(()),
                2 => JobOutcome::Invalidated,
                3 => JobOutcome::Expired,
                _ => return Err(Error::Corrupt),
            },
            cursor: d.blob(QueryCursor::MAX_ENCODED_BYTES)?,
            count: d.nat64()?,
            state: d.blob(crate::codec::MAX_ROW_BYTES)?,
        };
        d.finish()?;
        if record.generation == 0
            || record.examined > MAX_CANDIDATES
            || record.count > record.examined
            || record.sequence > record.examined + 2
            || record.input_bytes > record.examined * super::super::super::CANDIDATE_BYTES as u64
            || (matches!(record.phase, JobOutcome::Complete(())) && !record.cursor.is_empty())
            || (matches!(record.phase, JobOutcome::Running)
                && (record.observed >= record.expires
                    || (record.examined > 0 && record.cursor.is_empty())))
        {
            return Err(Error::Corrupt);
        }
        Ok(record)
    }
}
