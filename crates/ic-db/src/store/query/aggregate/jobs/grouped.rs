//! Retained grouped aggregates with bounded complete finalization.
use super::{
    super::super::Plan,
    super::{
        Accumulator, GroupCount, GroupOrder, GroupValue, Groups, Query, Reducer, numeric::Operation,
    },
    AggregateJobs, GroupPage, JobHandle, JobStep, field_binding, pages, prepare, start,
};
use crate::{Entity, Error, Timestamp, Value};
use ic_storage::Memory;
use sha2::{Digest, Sha256};

impl<E: Entity, M: Memory> Query<'_, E, M> {
    /// Retain grouped row counts without scanning input. K is the same exact key
    /// record as synchronous `group_count`. Group predicates, order and output
    /// row limit bind the job; work/output-byte allowances may change on resume.
    /// Keep at most 32 groups and the complete job frame within 16 KiB,
    /// including progress and encoded key records. No spill or partial output.
    /// # Errors
    /// Refuses group/query admission, invalid expiry or occupied job capacity.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_group_count<K: Value>(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        self.start_group_count_job::<K>(jobs, now, expires, None)
    }

    /// Retain present-field counts per exact key record. Any top-level stored field
    /// is accepted; empty values count. Groups containing only absent values remain
    /// with count zero. Uses `start_group_count`'s bounds and selection binding.
    /// # Errors
    /// Refuses unknown fields and ordinary grouped-count admission failures.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_group_count_field<K: Value>(
        self,
        field: &str,
        jobs: &mut AggregateJobs<impl Memory>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        self.start_group_count_job::<K>(jobs, now, expires, Some(field))
    }

    fn start_group_count_job<K: Value>(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        now: Timestamp,
        expires: Timestamp,
        field: Option<&str>,
    ) -> Result<JobHandle, Error> {
        if self.request.groups.page.is_some() {
            return Err(Error::InvalidQuery);
        }
        let plan = self.aggregate_plan(Some(K::definition()), true)?;
        let groups = self.admit_groups(&plan, self.count_reducer(field)?)?;
        start(
            jobs,
            field_binding(group_binding(&plan, &groups, self.request.rows)?, field),
            self.db.revision_by_id(E::ID)?,
            now,
            expires,
            vec![0],
        )
    }

    /// Advance grouped counts with scalar jobs' revision/retry/work/expiry contract.
    /// Empty input completes as an empty vector; absent keys form their own group.
    /// Finalize all input before filtering, ordering and selecting completed groups.
    /// Use `group_page` to page completed output; otherwise all selected groups must fit.
    /// # Errors
    /// Refuses mismatched queries/keys/selection, cardinality or the 16 KiB job
    /// frame. Group/output bounds apply before saving; a refusal preserves prior
    /// progress. `limit` never limits input or relaxes retained-state bounds.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_group_count<K: Value>(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<GroupPage<GroupCount<K>>>, Error> {
        self.resume_group_count_job::<K>(jobs, handle, sequence, now, None)
    }

    /// Advance grouped present-field counts with `resume_group_count`'s lifecycle.
    /// Finish all input before selection; zero-count groups are retained. The field
    /// binds the job, including required fields whose count equals the row count.
    /// # Errors
    /// Refuses changed fields, row-count jobs and ordinary grouped-count failures.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_group_count_field<K: Value>(
        self,
        field: &str,
        jobs: &mut AggregateJobs<impl Memory>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<GroupPage<GroupCount<K>>>, Error> {
        self.resume_group_count_job::<K>(jobs, handle, sequence, now, Some(field))
    }

    fn resume_group_count_job<K: Value>(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
        field: Option<&str>,
    ) -> Result<JobStep<GroupPage<GroupCount<K>>>, Error> {
        let plan = self.aggregate_plan(Some(K::definition()), true)?;
        let mut groups = self.admit_groups(&plan, self.count_reducer(field)?)?;
        let expected = field_binding(group_binding(&plan, &groups, self.request.rows)?, field);
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

    /// Retain grouped integer/Decimal sums with `group_sum`'s exact result types.
    /// Uses grouped counts' 32-group/16 KiB frame, revision and expiry bounds.
    /// Key/result types, operation and completed-group selection bind the job.
    /// # Errors
    /// Refuses invalid query/result shapes, expiry or exhausted job capacity.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_group_sum<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        self.start_group_numeric::<K, R>(jobs, now, expires, Operation::Sum)
    }

    /// Advance grouped sums; all-absent groups have `None`, present zero is a value.
    /// Finish every group before filtering/order/limit. Overflow in an excluded
    /// group still refuses. Output and frame refusals preserve the saved checkpoint.
    /// Returns a GroupPage; use `group_page` to page the completed selected groups.
    /// # Errors
    /// Grouped-count resume failures and `group_sum` arithmetic/result errors apply.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_group_sum<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<GroupPage<GroupValue<K, R>>>, Error> {
        self.resume_group_numeric::<K, R>(jobs, handle, sequence, now, Operation::Sum)
    }

    /// Retain grouped averages, saving exact totals and each group's present count.
    /// Round once at completion to R's Decimal scale, nearest with ties to even.
    /// Bounds and query binding match `start_group_sum`; no precision fallback.
    /// # Errors
    /// Refuses invalid average shapes, expiry or exhausted job capacity.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_group_avg<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        self.start_group_numeric::<K, R>(jobs, now, expires, Operation::Average)
    }

    /// Advance grouped averages with `resume_group_sum`'s complete-input contract.
    /// Selection compares completed rounded values. Absent operands do not enter
    /// the denominator; all-absent groups remain `None`. Never average chunk means.
    /// # Errors
    /// Grouped-count resume failures and `group_avg` arithmetic/result errors apply.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_group_avg<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<GroupPage<GroupValue<K, R>>>, Error> {
        self.resume_group_numeric::<K, R>(jobs, handle, sequence, now, Operation::Average)
    }

    fn start_group_numeric<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        now: Timestamp,
        expires: Timestamp,
        operation: Operation,
    ) -> Result<JobHandle, Error> {
        let admitted = self.numeric_plan::<R>(Some(K::definition()), operation)?;
        self.start_group_value(jobs, now, expires, admitted)
    }

    pub(super) fn start_group_value(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        now: Timestamp,
        expires: Timestamp,
        admitted: (Plan<'_>, Reducer),
    ) -> Result<JobHandle, Error> {
        if self.request.groups.page.is_some() {
            return Err(Error::InvalidQuery);
        }
        let (plan, reducer) = admitted;
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

    fn resume_group_numeric<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
        operation: Operation,
    ) -> Result<JobStep<GroupPage<GroupValue<K, R>>>, Error> {
        let admitted = self.numeric_plan::<R>(Some(K::definition()), operation)?;
        self.resume_group_value(jobs, handle, sequence, now, admitted)
    }

    pub(super) fn resume_group_value<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
        admitted: (Plan<'_>, Reducer),
    ) -> Result<JobStep<GroupPage<GroupValue<K, R>>>, Error> {
        let (plan, reducer) = admitted;
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
            pages::value_page(groups, rows, bytes, page, handle)
        })?;
        jobs.save(handle, &record)?;
        Ok(step)
    }
}
pub(super) fn group_binding(
    plan: &Plan<'_>,
    groups: &Groups,
    rows: usize,
) -> Result<[u8; 32], Error> {
    let mut hash = Sha256::new();
    if matches!(groups.initial, Reducer::Numeric(_)) {
        hash.update(b"ic-db:group-numeric-job:v1\0");
        // Reuse the scalar numeric identity for operand, operation and result scale.
        hash.update(super::binding(plan, &groups.initial)?);
    } else if matches!(groups.initial, Reducer::Extremum { .. }) {
        hash.update(b"ic-db:group-extremum-job:v1\0");
        hash.update(super::binding(plan, &groups.initial)?);
    } else if let Reducer::Distinct { mask, .. } = groups.initial {
        hash.update(b"ic-db:group-distinct-count-job:v1\0");
        hash.update(mask.to_le_bytes());
    } else {
        // Preserve existing grouped-count identities and payloads.
        hash.update(b"ic-db:group-count-job:v1\0");
    }
    hash.update(plan.binding);
    hash.update([
        rows as u8,
        match groups.selection.order {
            GroupOrder::Key => 0,
            GroupOrder::AggregateAscending => 1,
            GroupOrder::AggregateDescending => 2,
            GroupOrder::KeyDescending => 3,
        },
    ]);
    hash.update([groups.selection.predicates.len() as u8]);
    for (comparison, operand, _) in &groups.selection.predicates {
        hash.update([*comparison as u8]);
        // Frame admitted operands so binding never relies on implicit widths.
        let bytes = operand.as_deref().unwrap_or_default();
        hash.update((bytes.len() as u32).to_le_bytes());
        hash.update(bytes);
    }
    // Preserve conjunction-only bindings; add identity only for actual OR clauses.
    let clauses = groups
        .selection
        .predicates
        .iter()
        .enumerate()
        .fold(0_u8, |mask, (i, (_, _, or_next))| {
            mask | (u8::from(*or_next) << i)
        });
    if clauses != 0 {
        hash.update(b"group-or-clauses\0");
        hash.update([clauses]);
    }
    Ok(hash.finalize().into())
}
