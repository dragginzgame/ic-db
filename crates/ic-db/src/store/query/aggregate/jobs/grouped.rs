//! Retained grouped counts and numeric totals with bounded complete finalization.
use super::super::super::Plan;
use super::super::{
    Accumulator, GroupCount, GroupOrder, GroupValue, Groups, Query, Reducer, count_output,
    numeric::Operation, value_output,
};
use super::{AggregateJobs, JobHandle, JobStep, count, prepare, start};
use crate::{Entity, Error, Timestamp, Value};
use ic_storage::Memory;
use sha2::{Digest, Sha256};

impl<E: Entity, M: Memory> Query<'_, E, M> {
    /// Retain grouped row counts without scanning input. K is the same exact key
    /// record as synchronous `group_count`. Group predicates, order and output
    /// row limit bind the job; work/output-byte allowances may change on resume.
    /// Keep at most 32 groups and the complete job frame within 4096 bytes,
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
        let plan = self.aggregate_plan(Some(K::definition()), true)?;
        let groups = self.admit_groups(&plan, count())?;
        start(
            jobs,
            group_binding(&plan, &groups, self.request.rows)?,
            self.db.revision_by_id(E::ID)?,
            now,
            expires,
            vec![0],
        )
    }

    /// Advance grouped counts with scalar jobs' revision/retry/work/expiry contract.
    /// Empty input completes as an empty vector; absent keys form their own group.
    /// Finalize all input before filtering, ordering and selecting completed groups.
    /// Output is bounded to 32 groups; this terminal does not paginate finished output.
    /// # Errors
    /// Refuses mismatched queries/keys/selection, cardinality or the 4096-byte job
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
    ) -> Result<JobStep<Vec<GroupCount<K>>>, Error> {
        let plan = self.aggregate_plan(Some(K::definition()), true)?;
        let mut groups = self.admit_groups(&plan, count())?;
        let expected = group_binding(&plan, &groups, self.request.rows)?;
        let mut record = jobs.load(handle)?;
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
        .finish(bytes, || count_output(groups, rows, bytes))?;
        jobs.save(handle, &record)?;
        Ok(step)
    }

    /// Retain grouped integer/Decimal sums with `group_sum`'s exact result types.
    /// Uses grouped counts' 32-group/4096-byte frame, revision and expiry bounds.
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
    /// Returns only complete selected groups, without finished-output pagination.
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
    ) -> Result<JobStep<Vec<GroupValue<K, R>>>, Error> {
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
    ) -> Result<JobStep<Vec<GroupValue<K, R>>>, Error> {
        self.resume_group_numeric::<K, R>(jobs, handle, sequence, now, Operation::Average)
    }

    fn start_group_numeric<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<impl Memory>,
        now: Timestamp,
        expires: Timestamp,
        operation: Operation,
    ) -> Result<JobHandle, Error> {
        let (plan, reducer) = self.numeric_plan::<R>(Some(K::definition()), operation)?;
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
    ) -> Result<JobStep<Vec<GroupValue<K, R>>>, Error> {
        let (plan, reducer) = self.numeric_plan::<R>(Some(K::definition()), operation)?;
        let mut groups = self.admit_groups(&plan, reducer)?;
        let expected = group_binding(&plan, &groups, self.request.rows)?;
        let mut record = jobs.load(handle)?;
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
        .finish(bytes, || value_output(groups, rows, bytes))?;
        jobs.save(handle, &record)?;
        Ok(step)
    }
}
fn group_binding(plan: &Plan<'_>, groups: &Groups, rows: usize) -> Result<[u8; 32], Error> {
    let mut hash = Sha256::new();
    if matches!(groups.initial, Reducer::Numeric(_)) {
        hash.update(b"ic-db:group-numeric-job:v1\0");
        // Reuse the scalar numeric identity for operand, operation and result scale.
        hash.update(super::binding(plan, &groups.initial)?);
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
        },
    ]);
    hash.update([groups.selection.predicates.len() as u8]);
    for (comparison, operand) in &groups.selection.predicates {
        hash.update([*comparison as u8]);
        // Frame admitted operands so binding never relies on implicit widths.
        let bytes = operand.as_deref().unwrap_or_default();
        hash.update((bytes.len() as u32).to_le_bytes());
        hash.update(bytes);
    }
    Ok(hash.finalize().into())
}
