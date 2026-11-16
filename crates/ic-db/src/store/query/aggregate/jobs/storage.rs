//! Fixed slots in one host-owned allocation; bounded frames use the owned codec.
use super::{JobHandle, JobOutcome, MAX_CANDIDATES};
use crate::{
    Error, QueryCursor, Timestamp,
    codec::{Decoder, Encoder},
};
use ic_storage::{DefaultMemoryImpl, Memory, RuntimeMemory, adapter::StableBTreeMap};

pub(super) const SLOTS: u8 = 16;
pub(super) const MAX_JOB_BYTES: usize = 16 * 1024;
const LEGACY_MAX_JOB_BYTES: usize = 4096;
const MAGIC: &[u8; 8] = b"ICDBAJ02";
const LEGACY_MAGIC: &[u8; 8] = b"ICDBAJ01";

/// Durable bounded aggregates in a host-governed allocation.
/// Holds at most 16 jobs, each at most 16 KiB encoded bytes. Creation requires a
/// fresh nonzero incarnation. Handles identify jobs; they do not authorize access.
/// All mutation must run synchronously within one IC message, without awaits.
/// Supplier failures trap; a later message trap rolls these changes back too.
pub struct AggregateJobs<M: Memory = DefaultMemoryImpl> {
    map: StableBTreeMap<u8, Vec<u8>, M>,
    incarnation: [u8; 16],
    legacy: bool,
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
        Ok(Self {
            map,
            incarnation,
            legacy: false,
        })
    }

    /// Reopen a supported job-store format without modifying it. Version-one
    /// jobs remain readable; the first successful insert/save atomically admits
    /// version two and its larger frame bound. Older executables then refuse.
    /// # Errors
    /// Refuses missing/corrupt headers and unsupported formats intact.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn open(memory: RuntimeMemory<M>) -> Result<Self, Error> {
        let map: StableBTreeMap<u8, Vec<u8>, M> =
            StableBTreeMap::load(memory).map_err(Error::Storage)?;
        let bytes = map.get(&0).ok_or(Error::Corrupt)?;
        let (incarnation, _, legacy) = read_header(&bytes)?;
        if map.len() > u64::from(SLOTS) + 1
            || map
                .range(..)
                .next_back()
                .is_some_and(|entry| *entry.key() > SLOTS)
        {
            return Err(Error::Corrupt);
        }
        Ok(Self {
            map,
            incarnation,
            legacy,
        })
    }

    /// Cancel one job, immediately releasing its logical slot. Repeated/stale
    /// cancellation refuses; it cannot delete a subsequent occupant of that slot.
    /// # Errors
    /// Refuses a foreign, missing, stale or malformed handle/job frame. Cancellation
    /// need not interpret a known frame's query-specific accumulator payload.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn cancel(&mut self, handle: &JobHandle) -> Result<(), Error> {
        self.with_record(handle, |_| ())?;
        self.map.remove_discard(&handle.slot);
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
                let record = self.decode(&bytes)?;
                if record.expires <= now && expired.len() < limit {
                    expired.push(slot);
                }
            }
        }
        for slot in &expired {
            self.map.remove_discard(slot);
        }
        Ok(expired.len())
    }

    pub(super) fn insert(&mut self, mut record: Record) -> Result<JobHandle, Error> {
        let slot = (1..=SLOTS)
            .find(|slot| !self.map.contains_key(slot))
            .ok_or(Error::JobCapacity)?;
        let (incarnation, generation, _) = read_header(&self.map.get(&0).ok_or(Error::Corrupt)?)?;
        if incarnation != self.incarnation {
            return Err(Error::Corrupt);
        }
        let generation = generation.checked_add(1).ok_or(Error::Limit)?;
        record.generation = generation;
        let bytes = record.encode()?;
        // No recoverable refusal after the first persistent write.
        self.map.insert(0, header(incarnation, generation));
        self.map.insert(slot, bytes);
        self.legacy = false;
        Ok(JobHandle {
            incarnation,
            slot,
            generation,
        })
    }

    pub(super) fn load(&self, handle: &JobHandle) -> Result<Record, Error> {
        self.with_record(handle, |record| record.into_owned())
    }

    // One identity/framing admission for owned resume and borrowed cancellation.
    // Only resume needs owned cursor/accumulator bytes after the supplier buffer dies.
    fn with_record<R>(
        &self,
        handle: &JobHandle,
        inspect: impl FnOnce(Record<&[u8]>) -> R,
    ) -> Result<R, Error> {
        if handle.incarnation != self.incarnation {
            return Err(Error::InvalidJob);
        }
        let bytes = self.map.get(&handle.slot).ok_or(Error::InvalidJob)?;
        let record = self.decode(&bytes)?;
        if record.generation != handle.generation {
            return Err(Error::InvalidJob);
        }
        Ok(inspect(record))
    }

    pub(super) fn save(&mut self, handle: &JobHandle, record: &Record) -> Result<(), Error> {
        let bytes = record.encode()?;
        // Prepare every refusal before publishing the wider bound. Both writes
        // share the enclosing message's rollback boundary, just like insertion.
        if self.legacy {
            let (incarnation, generation, _) =
                read_header(&self.map.get(&0).ok_or(Error::Corrupt)?)?;
            self.map.insert(0, header(incarnation, generation));
        }
        self.map.insert(handle.slot, bytes);
        self.legacy = false;
        Ok(())
    }

    fn decode<'a>(&self, bytes: &'a [u8]) -> Result<Record<&'a [u8]>, Error> {
        if self.legacy && bytes.len() > LEGACY_MAX_JOB_BYTES {
            return Err(Error::Corrupt);
        }
        Record::decode(bytes)
    }
}
fn header(incarnation: [u8; 16], generation: u64) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend(crate::codec::ROW_CODEC_VERSION.to_le_bytes());
    bytes.extend(incarnation);
    bytes.extend(generation.to_le_bytes());
    bytes
}
fn read_header(bytes: &[u8]) -> Result<([u8; 16], u64, bool), Error> {
    if bytes.len() != 36 {
        return Err(Error::Corrupt);
    }
    if (&bytes[..8] != MAGIC && &bytes[..8] != LEGACY_MAGIC)
        || bytes[8..12] != crate::codec::ROW_CODEC_VERSION.to_le_bytes()
    {
        return Err(Error::InvalidJob);
    }
    let incarnation = bytes[12..28].try_into().map_err(|_| Error::Corrupt)?;
    if incarnation == [0; 16] {
        return Err(Error::Corrupt);
    }
    Ok((
        incarnation,
        u64::from_le_bytes(bytes[28..].try_into().map_err(|_| Error::Corrupt)?),
        &bytes[..8] == LEGACY_MAGIC,
    ))
}

// The byte boundary has two representations: a validated borrowed frame for
// inspection, and owned cursor/state for advancing a job. Execution stays shared.
pub(super) struct Record<Bytes = Vec<u8>> {
    pub generation: u64,
    pub binding: [u8; 32],
    pub revision: u64,
    pub expires: Timestamp,
    pub observed: Timestamp,
    pub sequence: u64,
    pub examined: u64,
    pub input_bytes: u64,
    pub phase: JobOutcome<()>,
    pub cursor: Bytes,
    pub count: u64,
    // Interpretation belongs to the query-bound accumulator, not the job store.
    pub state: Bytes,
}
impl Record {
    fn encode(&self) -> Result<Vec<u8>, Error> {
        let mut e = Encoder::with_byte_limit(MAX_JOB_BYTES);
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
}
impl<'a> Record<&'a [u8]> {
    fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        let mut d = Decoder::with_byte_limit(bytes, MAX_JOB_BYTES)?;
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
            cursor: d.blob_ref(QueryCursor::MAX_ENCODED_BYTES)?,
            count: d.nat64()?,
            state: d.blob_ref(MAX_JOB_BYTES)?,
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

    fn into_owned(self) -> Record {
        Record {
            generation: self.generation,
            binding: self.binding,
            revision: self.revision,
            expires: self.expires,
            observed: self.observed,
            sequence: self.sequence,
            examined: self.examined,
            input_bytes: self.input_bytes,
            phase: self.phase,
            cursor: self.cursor.to_vec(),
            count: self.count,
            state: self.state.to_vec(),
        }
    }
}
