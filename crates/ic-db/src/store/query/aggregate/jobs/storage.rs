//! Fixed slots in one host-owned allocation; bounded frames use the owned codec.

use super::{JobHandle, JobOutcome, MAX_CANDIDATES};
use crate::{
    Error, QueryCursor, Timestamp,
    codec::{Decoder, Encoder},
};
use ic_storage::{Memory, RuntimeMemory, WASM_PAGE_SIZE_BYTES as PAGE_BYTES};

pub(super) const SLOTS: u8 = 16;
pub(super) const MAX_JOB_BYTES: usize = 16 * 1024;
const MAGIC: &[u8; 8] = b"ICDBAJ03";

const HEADER_BYTES: usize = 36;
const DIRECTORY_BYTES: usize = SLOTS as usize * 2;
const GENERATION_OFFSET: usize = HEADER_BYTES - 8;

// Slot zero is reserved for the store header. Empty lengths make retired frames
// unreachable. The supplier grows the named allocation only as frames need it.
fn frame_offset(slot: u8) -> u64 {
    (HEADER_BYTES + DIRECTORY_BYTES) as u64 + u64::from(slot - 1) * MAX_JOB_BYTES as u64
}

fn length_offset(slot: u8) -> u64 {
    HEADER_BYTES as u64 + 2 * u64::from(slot - 1)
}

/// Durable bounded aggregates in a host-governed allocation.
/// Holds at most 16 jobs, each at most 16 KiB encoded bytes. Creation requires a
/// fresh nonzero incarnation. Handles identify jobs; they do not authorize access.
/// All mutation must run synchronously within one IC message, without awaits.
/// Supplier failures trap; a later message trap rolls these changes back too.
pub struct AggregateJobs<M: Memory> {
    memory: RuntimeMemory<M>,
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
        if memory.size() != 0 {
            return Err(Error::Exists);
        }
        memory.grow(1).expect("job allocation growth failed");
        memory.write(0, &header(incarnation, 0));

        Ok(Self {
            memory,
            incarnation,
        })
    }

    /// Reopen the current job-store format without modifying it.
    /// # Errors
    /// Refuses missing/corrupt headers and unsupported formats intact.
    pub fn open(memory: RuntimeMemory<M>) -> Result<Self, Error> {
        if memory.size() == 0 {
            return Err(Error::Corrupt);
        }
        let mut bytes = [0; HEADER_BYTES];
        memory.read(0, &mut bytes);
        let (incarnation, _) = read_header(&bytes)?;
        let jobs = Self {
            memory,
            incarnation,
        };
        jobs.lengths()?;
        Ok(jobs)
    }

    fn lengths(&self) -> Result<[u16; SLOTS as usize], Error> {
        let mut directory = [[0; 2]; SLOTS as usize];
        self.memory
            .read(HEADER_BYTES as u64, directory.as_flattened_mut());
        let lengths = directory.map(u16::from_le_bytes);
        let pages = self.memory.size();
        for (index, &len) in lengths.iter().enumerate() {
            if usize::from(len) > MAX_JOB_BYTES
                || (len != 0
                    && (frame_offset(index as u8 + 1) + u64::from(len)).div_ceil(PAGE_BYTES)
                        > pages)
            {
                return Err(Error::Corrupt);
            }
        }
        Ok(lengths)
    }

    fn read_frame(&self, slot: u8, len: u16) -> Vec<u8> {
        let mut bytes = vec![0; usize::from(len)];
        self.memory.read(frame_offset(slot), &mut bytes);
        bytes
    }

    fn write_frame(&self, slot: u8, bytes: &[u8]) {
        let offset = frame_offset(slot);
        let pages = (offset + bytes.len() as u64).div_ceil(PAGE_BYTES);
        let current = self.memory.size();
        if pages > current {
            self.memory
                .grow(pages - current)
                .expect("job allocation growth failed");
        }
        self.memory.write(offset, bytes);
        self.memory
            .write(length_offset(slot), &(bytes.len() as u16).to_le_bytes());
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
        self.memory.write(length_offset(handle.slot), &[0; 2]);

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
        let mut lengths = self.lengths()?;
        let mut removed = 0;
        for (index, len) in lengths.iter_mut().enumerate() {
            if *len != 0 {
                let bytes = self.read_frame(index as u8 + 1, *len);
                let record = Record::decode(&bytes)?;
                if record.expires <= now && removed < limit {
                    *len = 0;
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            let directory = lengths.map(u16::to_le_bytes);
            self.memory
                .write(HEADER_BYTES as u64, directory.as_flattened());
        }
        Ok(removed)
    }

    pub(super) fn insert(&mut self, mut record: Record) -> Result<JobHandle, Error> {
        let slot = self
            .lengths()?
            .iter()
            .position(|len| *len == 0)
            .ok_or(Error::JobCapacity)? as u8
            + 1;
        let mut bytes = [0; HEADER_BYTES];
        self.memory.read(0, &mut bytes);
        let (incarnation, generation) = read_header(&bytes)?;
        if incarnation != self.incarnation {
            return Err(Error::Corrupt);
        }
        let generation = generation.checked_add(1).ok_or(Error::Limit)?;
        record.generation = generation;
        let bytes = record.encode()?;
        // No recoverable refusal after the first persistent write.
        self.memory
            .write(GENERATION_OFFSET as u64, &generation.to_le_bytes());
        self.write_frame(slot, &bytes);

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
        let len = self.lengths()?[usize::from(handle.slot - 1)];
        if len == 0 {
            return Err(Error::InvalidJob);
        }
        let bytes = self.read_frame(handle.slot, len);
        let record = Record::decode(&bytes)?;
        if record.generation != handle.generation {
            return Err(Error::InvalidJob);
        }

        Ok(inspect(record))
    }

    pub(super) fn save_if_changed(
        &mut self,
        handle: &JobHandle,
        record: &Record,
        changed: bool,
    ) -> Result<(), Error> {
        if changed {
            self.write_frame(handle.slot, &record.encode()?);
        }

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
    if bytes.len() != HEADER_BYTES {
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
        u64::from_le_bytes(
            bytes[GENERATION_OFFSET..]
                .try_into()
                .map_err(|_| Error::Corrupt)?,
        ),
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
            || record.input_bytes
                > record.examined
                    * super::super::super::candidate_byte_limit(crate::codec::MAX_ROW_BYTES) as u64
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
