//! Retained tuple DISTINCT counts using the existing exact key-set reducer.

use super::{
    super::{
        Accumulator, Completed, GroupCount, Query, Reducer, decode_group_count, decode_record,
    },
    AggregateJobs, GroupCursor, GroupPage, JobHandle, JobStep, prepare, start,
};
use crate::{Entity, Error, Timestamp, Value};
use ic_storage::Memory;
use sha2::{Digest, Sha256};

impl<'a, E: Entity, M: Memory> Query<'a, E, M> {
    /// Start exact enumeration of distinct projected records.
    /// Defaults to ascending. Use `order_groups` for descending keys or first occurrence
    /// in input traversal order. First occurrence defaults to ascending primary key.
    /// Use `order_by` for an order supplied by primary or index traversal and
    /// `descending` to reverse the traversal, including its primary-key tie-breaker.
    /// The job retains up to 255 tuples in its 16 KiB frame and invalidates on a source
    /// change before completion. Absent tuple components participate in equality.
    /// # Errors
    /// Refuses incompatible projections, group modifiers, unsupported input orders, expiry or capacity.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_distinct<K: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        observe!(start, {
            let plan = self.aggregate_plan(Some(K::definition()), true)?;
            self.start_group_value(jobs, now, expires, (plan, Reducer::Keys(0)))
        })
    }

    /// Advance a DISTINCT job or page its completed result. Pass `None` until a
    /// completed page supplies a cursor. `limit` selects the page size (1–32);
    /// it and the per-call work and byte allowances may change between calls.
    /// # Errors
    /// Refuses incomplete-state cursors, changed queries and exhausted tuple, frame
    /// or output bounds. Tuple or frame exhaustion returns `Error::Limit` and preserves
    /// the last successful checkpoint. Reducing the output page size cannot resolve
    /// retained-state exhaustion. Cancel the job to release its slot.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_distinct<K: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
        after: Option<&'a GroupCursor>,
    ) -> Result<JobStep<GroupPage<K>>, Error> {
        observe!(resume, {
            if self.request.groups.page.is_some() {
                return Err(Error::InvalidQuery);
            }
            let page_size = self.request.rows;
            let query = self.group_page(page_size, after);
            let plan = query.aggregate_plan(Some(K::definition()), true)?;
            let job = query.retained_groups(plan, Reducer::Keys(0), None)?;
            query.resume_groups(jobs, handle, sequence, now, job, |bytes, result| {
                if !matches!(result, Completed::Key) {
                    return Err(Error::Corrupt);
                }
                decode_record(bytes)
            })
        })
    }

    /// Retain an exact count of projected K tuples, including an absent tuple.
    /// Uses synchronous `count_distinct` admission and equality without an index.
    /// Retain at most 32 distinct keys inside the 16 KiB whole-job frame;
    /// no spill. Query/projection/algorithm bind the job; work/byte limits may vary.
    /// # Errors
    /// Refuses unsupported projections, group modifiers, expiry and capacity.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_count_distinct<K: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        observe!(start, {
            let (plan, mut reducer) = self.distinct_plan::<K>()?;
            if self.request.bytes < 8 {
                return Err(Error::RowTooLarge);
            }

            start(
                jobs,
                binding(plan.binding),
                self.db.read()?.revision_by_id(E::SCHEMA.id)?,
                now,
                expires,
                &Accumulator::Scalar(&mut reducer),
            )
        })
    }

    /// Advance DISTINCT count under scalar jobs' work/retry/revision/expiry rules.
    /// Duplicates across steps count once; empty input returns zero, all-absent
    /// tuples count once. Finish all input before returning a count. `limit`
    /// never truncates input or relaxes key-set bounds.
    /// # Errors
    /// Refuses mismatched queries/projections, malformed saved keys, cardinality,
    /// output or whole-frame bounds without changing the previous checkpoint.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_count_distinct<K: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<u64>, Error> {
        observe!(resume, {
            let (plan, mut reducer) = self.distinct_plan::<K>()?;
            let mut record = jobs.load(handle)?;
            let bytes = self.request.bytes;
            let changed = prepare(
                &*self.db.read()?,
                E::SCHEMA,
                self.request,
                &plan,
                Accumulator::Scalar(&mut reducer),
                &mut record,
                binding(plan.binding),
                sequence,
                now,
            )?;

            record.finish_and_save(jobs, handle, changed, bytes, || {
                Ok((reducer.finish()?.into_count()?, 8))
            })
        })
    }

    /// Retain distinct R tuples separately in each K group, including absent tuples.
    /// Projections may overlap. Uses synchronous `group_count_distinct` admission.
    /// Keep at most 32 groups and 32 distinct keys per group inside one 16 KiB
    /// whole-job frame. Key/operand projections, group selection and limit bind
    /// the job. Work/output-byte limits may vary; no automatic spill.
    /// # Errors
    /// Refuses unsupported projections, expiry or occupied job capacity.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_group_count_distinct<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        observe!(start, {
            if self.request.groups.page.is_some() {
                return Err(Error::InvalidQuery);
            }
            let (plan, reducer) = self.group_distinct_plan::<K, R>()?;

            self.start_groups(jobs, now, expires, plan, reducer, None)
        })
    }

    /// Advance grouped DISTINCT counts with grouped jobs' revision/retry/expiry rules.
    /// Finish all input before filtering, ordering or limiting groups. Empty input
    /// returns an empty vector; an all-absent tuple contributes one to its group.
    /// Returns a GroupPage; use `group_page` to page completed selected groups.
    /// # Errors
    /// Refuses mismatched bindings, malformed saved sets, group/key cardinality,
    /// output or frame bounds. Refusal preserves the previous checkpoint; output
    /// filters and limits cannot conceal oversized state.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_group_count_distinct<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<GroupPage<GroupCount<K>>>, Error> {
        observe!(resume, {
            let (plan, reducer) = self.group_distinct_plan::<K, R>()?;
            let job = self.retained_groups(plan, reducer, None)?;

            self.resume_groups(jobs, handle, sequence, now, job, decode_group_count)
        })
    }
}

fn binding(query: [u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"ic-db:distinct-count-job:v1\0");
    hash.update(query);

    hash.finalize().into()
}
