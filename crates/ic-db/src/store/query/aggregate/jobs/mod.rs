//! Retained aggregates: exact input revision, atomic progress, explicit retry.
mod checkpoint;
mod distinct;
mod extrema;
mod grouped;
pub(super) mod pages;
mod storage;
pub use pages::{GroupCursor, GroupPage};
pub use storage::AggregateJobs;

use super::{
    super::{Database, Plan, Query, QueryCursor, Request, execute_inner},
    Accumulator, Reducer, decode_record,
    numeric::Operation,
};
use crate::{Entity, EntitySchema, Error, Timestamp, Value};
use ic_storage::Memory;
use sha2::{Digest, Sha256};
use storage::Record;

const MAX_CANDIDATES: u64 = 1_000_000;
const MAX_LIFETIME_MS: i64 = 3_600_000;

/// Opaque retained-job identity. The host must check caller authorization separately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobHandle {
    incarnation: [u8; 16],
    slot: u8,
    generation: u64,
}
impl JobHandle {
    /// Encode an exact, bounded transport handle; no query or accumulator travels in it.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![1, self.slot];
        bytes.extend(self.incarnation);
        bytes.extend(self.generation.to_le_bytes());
        bytes
    }

    /// Decode framing; the job store also validates incarnation and slot generation.
    /// # Errors
    /// Refuses unsupported, malformed, zero or oversized handles.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != 26 || bytes[0] != 1 || !(1..=storage::SLOTS).contains(&bytes[1]) {
            return Err(Error::InvalidJob);
        }
        let incarnation = bytes[2..18].try_into().map_err(|_| Error::InvalidJob)?;
        let generation = u64::from_le_bytes(bytes[18..].try_into().map_err(|_| Error::InvalidJob)?);
        if incarnation == [0; 16] || generation == 0 {
            return Err(Error::InvalidJob);
        }
        Ok(Self {
            incarnation,
            slot: bytes[1],
            generation,
        })
    }
}

/// Only Complete contains a result. Running never exposes a prefix as an exact total.
#[derive(Debug, PartialEq, Eq)]
pub enum JobOutcome<T> {
    /// More input remains, or exhaustion has not yet been observed.
    Running,
    /// Exact result for the captured source revision, retained until expiry/cancellation.
    Complete(T),
    /// The source entity changed before completion. Start a new job explicitly.
    Invalidated,
    /// The host-captured time reached the fixed expiry. Cleanup is explicit.
    Expired,
}
/// Saved progress. Older sequences do not rescan; revision and expiry checks still apply.
#[derive(Debug)]
pub struct JobStep<T> {
    /// Supply this sequence on the next advance. Newly created jobs start at zero.
    pub sequence: u64,
    /// Entity revision captured at creation, including for completed historical results.
    pub source_revision: u64,
    /// Cumulative examined primary/index candidates, including residual nonmatches.
    pub examined: u64,
    /// Cumulative source row/key bytes, excluding supplier traversal/allocator overhead.
    pub input_bytes: u64,
    /// Encoded returned result/page size; zero while no result is available.
    pub output_bytes: usize,
    /// Explicit progress, completion, invalidation or expiry.
    pub outcome: JobOutcome<T>,
}

impl JobStep<()> {
    // Conversion and output refusal happen before saving the prepared checkpoint.
    // A refused result must leave the previous checkpoint available for retry.
    fn finish_and_save<T, M: Memory>(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        record: &Record,
        limit: usize,
        finish: impl FnOnce() -> Result<(T, usize), Error>,
    ) -> Result<JobStep<T>, Error> {
        let (outcome, output_bytes) = match self.outcome {
            JobOutcome::Running => (JobOutcome::Running, 0),
            JobOutcome::Invalidated => (JobOutcome::Invalidated, 0),
            JobOutcome::Expired => (JobOutcome::Expired, 0),
            JobOutcome::Complete(()) => {
                let (value, bytes) = finish()?;
                if bytes > limit {
                    return Err(Error::RowTooLarge);
                }
                (JobOutcome::Complete(value), bytes)
            }
        };
        jobs.save(handle, record)?;
        Ok(JobStep {
            sequence: self.sequence,
            source_revision: self.source_revision,
            examined: self.examined,
            input_bytes: self.input_bytes,
            output_bytes,
            outcome,
        })
    }
}

impl<E: Entity, M: Memory> Query<'_, E, M> {
    /// Retain an exact count job without scanning input. Expiry must be after the
    /// host-captured `now` and at most one hour later. No write freeze is acquired.
    /// Reconstruct this fluent query on every advance; predicates/access path bind
    /// the job, while the per-call work limit may change. Limit never truncates input.
    /// # Errors
    /// Ordinary count admission, invalid expiry, or all 16 retained slots occupied.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_count(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        self.start_count_job(jobs, now, expires, None)
    }

    /// Retain the count of present values in a named top-level field, of any stored
    /// type. Empty values count; absent values do not. The field binds the job,
    /// even for a required field. Uses `start_count`'s limits and expiry contract.
    /// # Errors
    /// Refuses unknown fields and ordinary count-job admission failures.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_count_field(
        self,
        field: &str,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        self.start_count_job(jobs, now, expires, Some(field))
    }

    fn start_count_job(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
        field: Option<&str>,
    ) -> Result<JobHandle, Error> {
        let plan = self.aggregate_plan(None, false)?;
        let mut reducer = self.count_reducer(field)?;
        if self.request.bytes < 8 {
            return Err(Error::RowTooLarge);
        }
        start(
            jobs,
            field_binding(binding(&plan, &reducer)?, field),
            self.db.read()?.revision_by_id(E::SCHEMA.id)?,
            now,
            expires,
            &Accumulator::Scalar(&mut reducer),
        )
    }

    /// Advance count within `work_limit` (1..=512) and 64 KiB of source row/key
    /// bytes. Each job admits at most one million examined candidates. A budget
    /// boundary returns Running; it never changes the mathematical input set.
    /// Supply zero first, then the last returned sequence. An older sequence only
    /// reads saved progress; a future sequence refuses. Source changes invalidate
    /// running jobs, including on retries; completed results keep their boundary.
    /// # Errors
    /// Refuses a foreign query/handle, future sequence, backward host time, resource
    /// bounds or corruption without changing progress. No automatic restart.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_count(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<u64>, Error> {
        self.resume_count_job(jobs, handle, sequence, now, None)
    }

    /// Advance the present-field count with `resume_count`'s bounds, revision,
    /// retry and expiry rules. Empty/all-absent input completes with zero.
    /// # Errors
    /// Refuses changed fields, row-count jobs and ordinary count resume failures.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_count_field(
        self,
        field: &str,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<u64>, Error> {
        self.resume_count_job(jobs, handle, sequence, now, Some(field))
    }

    fn resume_count_job(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
        field: Option<&str>,
    ) -> Result<JobStep<u64>, Error> {
        let plan = self.aggregate_plan(None, false)?;
        let mut record = jobs.load(handle)?;
        let mut reducer = self.count_reducer(field)?;
        let expected = field_binding(binding(&plan, &reducer)?, field);
        let bytes = self.request.bytes;
        prepare(
            &*self.db.read()?,
            E::SCHEMA,
            self.request,
            &plan,
            Accumulator::Scalar(&mut reducer),
            &mut record,
            expected,
            sequence,
            now,
        )?
        .finish_and_save(jobs, handle, &record, bytes, || {
            Ok((reducer.finish()?.into_count()?, 8))
        })
    }

    /// Retain an integer/Decimal sum job with synchronous sum's exact result shape.
    /// Same expiry, capacity and query-binding contract as [`Self::start_count`].
    /// # Errors
    /// Refuses unsupported sum shapes, query admission, expiry or occupied capacity.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_sum<R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        self.start_numeric::<R>(jobs, now, expires, Operation::Sum)
    }

    /// Advance exact sum under count's work/retry/invalidation contract. Check the
    /// declared result width only after all input completes, allowing signed
    /// prefixes to cancel across messages. Empty/all-absent input completes as None.
    /// # Errors
    /// Count's refusals plus arithmetic overflow or completed-result byte limits.
    /// A refused finalization preserves the previous checkpoint.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_sum<R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<Option<R>>, Error> {
        self.resume_numeric::<R>(jobs, handle, sequence, now, Operation::Sum)
    }

    /// Retain an exact integer/Decimal average. R declares one Decimal field
    /// with the source name and optionality, and its desired result scale (0–28).
    /// Shares count/sum's expiry, capacity and revision contract. Average versus
    /// sum and the result scale are bound to the job; they cannot change on resume.
    /// # Errors
    /// Refuses unsupported average shapes, query admission, expiry or occupied capacity.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_avg<R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        self.start_numeric::<R>(jobs, now, expires, Operation::Average)
    }

    /// Advance average under count's bounded work/retry/invalidation contract.
    /// Retain the exact total and present count across steps; divide and round once
    /// at completion to the declared Decimal scale, nearest with ties to even.
    /// Empty/all-absent input completes as None. Never average rounded page means.
    /// # Errors
    /// Count's refusals plus arithmetic overflow or completed-result byte limits.
    /// A refused finalization preserves the prior exact checkpoint; no scale fallback.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_avg<R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<Option<R>>, Error> {
        self.resume_numeric::<R>(jobs, handle, sequence, now, Operation::Average)
    }

    fn start_numeric<R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
        operation: Operation,
    ) -> Result<JobHandle, Error> {
        let (plan, mut reducer) = self.numeric_plan::<R>(None, operation)?;
        start(
            jobs,
            binding(&plan, &reducer)?,
            self.db.read()?.revision_by_id(E::SCHEMA.id)?,
            now,
            expires,
            &Accumulator::Scalar(&mut reducer),
        )
    }

    fn resume_numeric<R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
        operation: Operation,
    ) -> Result<JobStep<Option<R>>, Error> {
        let (plan, reducer) = self.numeric_plan::<R>(None, operation)?;
        self.resume_value(jobs, handle, sequence, now, (plan, reducer))
    }

    fn resume_value<R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
        admitted: (Plan<'_>, Reducer),
    ) -> Result<JobStep<Option<R>>, Error> {
        let (plan, mut reducer) = admitted;
        let mut record = jobs.load(handle)?;
        let expected = binding(&plan, &reducer)?;
        let limit = self.request.bytes;
        prepare(
            &*self.db.read()?,
            E::SCHEMA,
            self.request,
            &plan,
            Accumulator::Scalar(&mut reducer),
            &mut record,
            expected,
            sequence,
            now,
        )?
        .finish_and_save(jobs, handle, &record, limit, || {
            let bytes = reducer.finish()?.into_value()?;
            let size = bytes.as_ref().map_or(0, Vec::len);
            let value = bytes.as_deref().map(decode_record).transpose()?;
            Ok((value, size))
        })
    }
}
fn field_binding(binding: [u8; 32], field: Option<&str>) -> [u8; 32] {
    let Some(field) = field else { return binding };
    let mut hash = Sha256::new();
    hash.update(b"ic-db:field-count-job:v1\0");
    hash.update(binding);
    hash.update((field.len() as u32).to_le_bytes());
    hash.update(field.as_bytes());
    hash.finalize().into()
}

fn binding(plan: &Plan<'_>, reducer: &Reducer) -> Result<[u8; 32], Error> {
    let mut hash = Sha256::new();
    // Algorithm identity is independent of the row schema. Changing checkpoint or
    // accumulation semantics requires a new identity and explicit resume support.
    hash.update(b"ic-db:scalar-job:v1\0");
    hash.update(plan.binding);
    match reducer {
        // A field-count terminal adds its admitted field identity with field_binding.
        Reducer::Count { .. } => hash.update([0]),
        Reducer::Numeric(state) => {
            hash.update([state.operation as u8, state.position as u8]);
            let output = state.output();
            let mut bytes = Vec::new();
            crate::schema::encode_type(&mut bytes, output.value, output.optional, false)?;
            hash.update(bytes);
        }
        // New payload identities; existing count/sum/average bindings stay stable.
        Reducer::Extremum {
            position, maximum, ..
        } => {
            hash.update([if *maximum { 4 } else { 3 }, *position as u8]);
        }
        _ => return Err(Error::InvalidQuery),
    }
    Ok(hash.finalize().into())
}

fn start<M: Memory>(
    jobs: &mut AggregateJobs<M>,
    binding: [u8; 32],
    revision: u64,
    now: Timestamp,
    expires: Timestamp,
    accumulator: &Accumulator<'_>,
) -> Result<JobHandle, Error> {
    let lifetime = expires
        .as_millis()
        .checked_sub(now.as_millis())
        .ok_or(Error::Limit)?;
    if !(1..=MAX_LIFETIME_MS).contains(&lifetime) {
        return Err(Error::Limit);
    }
    let (count, state) = accumulator.checkpoint()?;
    jobs.insert(Record {
        generation: 0,
        binding,
        revision,
        expires,
        observed: now,
        sequence: 0,
        examined: 0,
        input_bytes: 0,
        phase: JobOutcome::Running,
        cursor: Vec::new(),
        count,
        state,
    })
}

// One shared concrete execution path. Type conversion happens before save, outside
// this function. Preparation errors never persist a partial accumulator or cursor.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit admitted query and checkpoint inputs keep the shared resume boundary local"
)]
fn prepare<M: Memory>(
    db: &Database<M>,
    entity: &EntitySchema,
    request: Request<'_>,
    plan: &Plan<'_>,
    mut accumulator: Accumulator<'_>,
    record: &mut Record,
    expected: [u8; 32],
    sequence: u64,
    now: Timestamp,
) -> Result<JobStep<()>, Error> {
    if record.binding != expected || sequence > record.sequence {
        return Err(Error::InvalidJob);
    }
    if now < record.observed {
        return Err(Error::TimestampRegression);
    }
    accumulator.restore(entity, record)?;
    if now >= record.expires && !matches!(record.phase, JobOutcome::Expired) {
        record.phase = JobOutcome::Expired;
        record.sequence += 1;
    } else if matches!(record.phase, JobOutcome::Running) {
        if db.revision_by_id(entity.id)? != record.revision {
            record.phase = JobOutcome::Invalidated;
            record.sequence += 1;
        } else if sequence == record.sequence {
            let cursor = if record.cursor.is_empty() {
                None
            } else {
                let cursor = QueryCursor::from_bytes(&record.cursor)?;
                if cursor.binding != plan.binding {
                    return Err(Error::Corrupt);
                }
                plan.validate_position(entity, &cursor.position)?;
                Some(cursor)
            };
            let request = Request {
                cursor: cursor.as_ref(),
                work: request
                    .work
                    .min((MAX_CANDIDATES - record.examined) as usize),
                ..request
            };
            let mut bytes = 0;
            let page = execute_inner(
                &db.map,
                entity,
                &request,
                plan,
                Some(&mut accumulator),
                Some(&mut bytes),
            )?;
            record.examined += page.examined as u64;
            record.input_bytes += bytes as u64;
            record.cursor = page
                .next
                .as_ref()
                .map_or_else(Vec::new, QueryCursor::to_bytes);
            record.phase = if page.next.is_none() {
                JobOutcome::Complete(())
            } else {
                JobOutcome::Running
            };
            record.sequence += 1;
            (record.count, record.state) = accumulator.checkpoint()?;
        }
    }
    record.observed = now;
    let outcome = match record.phase {
        JobOutcome::Running => JobOutcome::Running,
        JobOutcome::Invalidated => JobOutcome::Invalidated,
        JobOutcome::Expired => JobOutcome::Expired,
        JobOutcome::Complete(()) => JobOutcome::Complete(()),
    };
    Ok(JobStep {
        sequence: record.sequence,
        source_revision: record.revision,
        examined: record.examined,
        input_bytes: record.input_bytes,
        output_bytes: 0,
        outcome,
    })
}
