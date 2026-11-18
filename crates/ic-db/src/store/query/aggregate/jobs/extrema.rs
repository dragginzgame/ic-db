//! Retained extrema over the synchronous comparator and shared job lifecycle.
use super::{AggregateJobs, JobHandle, JobStep, binding, start};
use crate::{Entity, Error, GroupPage, GroupValue, Timestamp, Value, store::Query};
use ic_storage::Memory;

impl<E: Entity, M: Memory> Query<'_, E, M> {
    /// Retain minima per exact key record, without scanning input. R selects one
    /// exact ordered field, as in `group_min`; key and operand may overlap.
    /// Retain all-absent groups with None. Keep at most 32 groups within the
    /// complete 16 KiB job frame, including keys, winners and progress.
    /// Group predicates, order and total limit bind the job. No automatic spill.
    /// # Errors
    /// Refuses unsupported shapes, page modifiers, expiry or occupied capacity.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_group_min<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        let admitted = self.group_extremum_plan::<K, R>(false)?;
        self.start_group_value(jobs, now, expires, admitted)
    }

    /// Retain maxima per exact key record with `start_group_min`'s bounds.
    /// Minimum and maximum have separate job identities.
    /// # Errors
    /// The same query, expiry and capacity errors as `start_group_min`.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_group_max<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        let admitted = self.group_extremum_plan::<K, R>(true)?;
        self.start_group_value(jobs, now, expires, admitted)
    }

    /// Advance grouped minima with the retained revision, retry and expiry contract.
    /// Finish all input before group filtering, ordering and limiting. Absent keys
    /// form a group; all-absent operands yield None. Use `group_page` for completed
    /// result pagination, retaining its continuation when mapping rows.
    /// # Errors
    /// Refuses mismatched queries/operations, corrupt winners, group/frame bounds
    /// and oversized output without changing saved progress. Selection cannot
    /// hide state exhaustion. No prefix winner is returned as an exact result.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_group_min<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<GroupPage<GroupValue<K, R>>>, Error> {
        let admitted = self.group_extremum_plan::<K, R>(false)?;
        self.resume_group_value(jobs, handle, sequence, now, admitted)
    }

    /// Advance grouped maxima with `resume_group_min`'s exactness and bounds.
    /// Equal operands retain their first encountered value.
    /// # Errors
    /// The same query, checkpoint, resource and output errors as `resume_group_min`.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_group_max<K: Value, R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<GroupPage<GroupValue<K, R>>>, Error> {
        let admitted = self.group_extremum_plan::<K, R>(true)?;
        self.resume_group_value(jobs, handle, sequence, now, admitted)
    }

    /// Retain a minimum over one exact single-field result record, without scanning.
    /// Uses `min`'s ordered types and absence semantics and `start_count`'s expiry,
    /// revision and capacity bounds. Persist only the winning record, within the
    /// complete 16 KiB job frame; no partial result or automatic spill.
    /// # Errors
    /// Refuses unsupported projections/query shapes, expiry or occupied capacity.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_min<R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        self.start_extremum::<R>(jobs, now, expires, false)
    }

    /// Retain a maximum with `start_min`'s types and bounds. Minimum and maximum
    /// have distinct job identities, even when they select the same field.
    /// # Errors
    /// The same admission, expiry and capacity errors as `start_min`.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn start_max<R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        self.start_extremum::<R>(jobs, now, expires, true)
    }

    /// Advance a retained minimum with `resume_count`'s bounded work, retry,
    /// revision and expiry contract. Ignore absent operands; empty/all-absent
    /// input completes with None. Return the winning record only after exhaustion.
    /// Equal values retain the first encountered value. A completed result stays
    /// fixed after source writes, until expiry or cancellation.
    /// # Errors
    /// Refuses changed queries/operations, malformed winner bytes, work/state
    /// bounds or oversized completed output. Refusal preserves the checkpoint.
    /// A comparison key exceeding 4096 bytes refuses even for a losing candidate.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_min<R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<Option<R>>, Error> {
        let (plan, reducer) = self.extremum_plan::<R>(false)?;
        self.resume_value(jobs, handle, sequence, now, (plan, reducer))
    }

    /// Advance a retained maximum with `resume_min`'s exactness and bounds.
    /// # Errors
    /// The same query, resource and checkpoint errors as `resume_min`.
    /// # Panics
    /// Supplier mutation failures trap the enclosing message.
    pub fn resume_max<R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<JobStep<Option<R>>, Error> {
        let (plan, reducer) = self.extremum_plan::<R>(true)?;
        self.resume_value(jobs, handle, sequence, now, (plan, reducer))
    }

    fn start_extremum<R: Value>(
        self,
        jobs: &mut AggregateJobs<M>,
        now: Timestamp,
        expires: Timestamp,
        maximum: bool,
    ) -> Result<JobHandle, Error> {
        let (plan, reducer) = self.extremum_plan::<R>(maximum)?;
        start(
            jobs,
            binding(&plan, &reducer)?,
            self.db.revision_by_id(E::ID)?,
            now,
            expires,
            Vec::new(),
        )
    }
}
