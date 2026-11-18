//! Retained tuple DISTINCT counts using the existing exact key-set reducer.
use super::{
    super::{Accumulator, GroupCount, Query},
    AggregateJobs, GroupPage, JobHandle, JobStep,
    grouped::group_binding,
    pages, prepare, start,
};
use crate::{Entity, Error, Timestamp, Value};
use ic_storage::Memory;
use sha2::{Digest, Sha256};

impl<E: Entity, M: Memory> Query<'_, E, M> {
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
        jobs: &mut AggregateJobs<impl Memory>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        let (plan, _) = self.distinct_plan::<K>()?;
        if self.request.bytes < 8 {
            return Err(Error::RowTooLarge);
        }
        start(
            jobs,
            binding(plan.binding),
            self.db.revision_by_id(E::ID)?,
            now,
            expires,
            vec![0],
        )
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
        jobs: &mut AggregateJobs<impl Memory>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<u64>, Error> {
        let (plan, mut reducer) = self.distinct_plan::<K>()?;
        let mut record = jobs.load(handle)?;
        let bytes = self.request.bytes;
        let step = prepare(
            self.db,
            E::SCHEMA,
            self.request,
            &plan,
            Accumulator::Scalar(&mut reducer),
            &mut record,
            binding(plan.binding),
            sequence,
            now,
        )?
        .finish(bytes, || Ok((reducer.finish()?.into_count()?, 8)))?;
        jobs.save(handle, &record)?;
        Ok(step)
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
        jobs: &mut AggregateJobs<impl Memory>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        if self.request.groups.page.is_some() {
            return Err(Error::InvalidQuery);
        }
        let (plan, reducer) = self.group_distinct_plan::<K, R>()?;
        let groups = self.admit_groups(&plan, reducer)?;
        start(
            jobs,
            group_binding(&plan, &groups, self.request.rows)?,
            self.db.revision_by_id(E::ID)?,
            now,
            expires,
            vec![0],
        )
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
        jobs: &mut AggregateJobs<impl Memory>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<GroupPage<GroupCount<K>>>, Error> {
        let (plan, reducer) = self.group_distinct_plan::<K, R>()?;
        let mut groups = self.admit_groups(&plan, reducer)?;
        let expected = group_binding(&plan, &groups, self.request.rows)?;
        let mut record = jobs.load(handle)?;
        let page = self.request.groups.page;
        pages::admit(page, handle, &record)?;
        let (rows, bytes) = (self.request.rows, self.request.bytes);
        let step = prepare(
            self.db,
            E::SCHEMA,
            self.request,
            &plan,
            Accumulator::Groups(&mut groups),
            &mut record,
            expected,
            sequence,
            now,
        )?
        .finish(bytes, || {
            pages::count_page(groups, rows, bytes, page, handle)
        })?;
        jobs.save(handle, &record)?;
        Ok(step)
    }
}

fn binding(query: [u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"ic-db:distinct-count-job:v1\0");
    hash.update(query);
    hash.finalize().into()
}
