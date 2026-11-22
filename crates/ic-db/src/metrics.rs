//! Volatile, canister-wide DB observations. Standalone use records into local
//! heap storage. A host can attach its own storage before the first operation.

use crate::{Aggregate, Error, JobStep, QueryPage};
use ic_metrics::MeasurementCounter;
use std::{
    cell::{Cell, OnceCell},
    rc::Rc,
};

/// Totals for one recorder lifetime. Counters saturate and report actual overflow.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Metrics {
    /// Returned synchronous fluent query terminals, including refusals.
    pub queries: MeasurementCounter,
    /// Synchronous query terminals returning an error.
    pub query_failures: MeasurementCounter,
    /// Candidates examined by successful synchronous queries.
    pub examined: MeasurementCounter,
    /// Source rows fetched by successful synchronous queries.
    pub fetched: MeasurementCounter,
    /// Result rows, groups or scalar values delivered by successful synchronous queries.
    pub returned: MeasurementCounter,
    /// Encoded result bytes delivered by successful synchronous queries.
    pub output_bytes: MeasurementCounter,
    /// Returned database commit calls, including empty and refused batches.
    pub commits: MeasurementCounter,
    /// Commit calls that returned an error.
    pub commit_failures: MeasurementCounter,
    /// Returned aggregate job start calls, including refusals.
    pub job_starts: MeasurementCounter,
    /// Returned aggregate job resume calls, including retries and refusals.
    pub job_resumes: MeasurementCounter,
    /// Job start or resume calls that returned an error.
    pub job_failures: MeasurementCounter,
}

/// Heap recorder suitable for a Canic metrics store source.
#[derive(Default)]
pub struct MetricsStorage(Cell<Metrics>);

impl MetricsStorage {
    /// Read this recorder without changing the counters.
    #[must_use]
    pub fn snapshot(&self) -> Metrics {
        self.0.get()
    }
}

thread_local! {
    static STORAGE: OnceCell<Rc<MetricsStorage>> = const { OnceCell::new() };
}

/// Another recorder is already active in this canister incarnation.
#[derive(Debug, PartialEq, Eq)]
pub struct StorageConflict;

impl std::fmt::Display for StorageConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IC DB metrics storage is already initialized")
    }
}
impl std::error::Error for StorageConflict {}

/// Attach a host-owned recorder. Reattaching the same recorder preserves all samples.
/// # Errors
/// Returns a conflict if another recorder has observed operations or was attached.
pub fn initialize_with_metrics(storage: Rc<MetricsStorage>) -> Result<(), StorageConflict> {
    STORAGE.with(|slot| {
        if let Some(active) = slot.get() {
            return if Rc::ptr_eq(active, &storage) {
                Ok(())
            } else {
                Err(StorageConflict)
            };
        }
        slot.set(storage).map_err(|_| StorageConflict)
    })
}

/// Snapshot this canister's recorder. Before initialization all counters are zero.
#[must_use]
pub fn snapshot() -> Metrics {
    STORAGE.with(|slot| slot.get().map_or_else(Metrics::default, |s| s.snapshot()))
}

fn update(f: impl FnOnce(&mut Metrics)) {
    STORAGE.with(|slot| {
        let storage = slot.get_or_init(Rc::default);
        let mut totals = storage.snapshot();
        f(&mut totals);
        storage.0.set(totals);
    });
}

pub(crate) trait QueryOutput {
    fn counts(&self) -> [usize; 4];
}
impl<T> QueryOutput for QueryPage<T> {
    fn counts(&self) -> [usize; 4] {
        [
            self.examined,
            self.fetched,
            self.rows.len(),
            self.output_bytes,
        ]
    }
}
trait ResultRows {
    fn rows(&self) -> usize;
}
impl ResultRows for u64 {
    fn rows(&self) -> usize {
        1
    }
}
impl ResultRows for bool {
    fn rows(&self) -> usize {
        1
    }
}
impl<T> ResultRows for Option<T> {
    fn rows(&self) -> usize {
        usize::from(self.is_some())
    }
}
impl<T> ResultRows for Vec<T> {
    fn rows(&self) -> usize {
        self.len()
    }
}
impl<T: ResultRows> QueryOutput for Aggregate<T> {
    fn counts(&self) -> [usize; 4] {
        [
            self.examined,
            self.fetched,
            self.value.rows(),
            self.output_bytes,
        ]
    }
}
pub(crate) fn query<T: QueryOutput>(run: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    let result = run();
    record_query(result.as_ref().ok().map(QueryOutput::counts));
    result
}
fn record_query(counts: Option<[usize; 4]>) {
    update(|m| {
        m.queries.add(1);
        if let Some([examined, fetched, returned, bytes]) = counts {
            m.examined.add(examined as u128);
            m.fetched.add(fetched as u128);
            m.returned.add(returned as u128);
            m.output_bytes.add(bytes as u128);
        } else {
            m.query_failures.add(1);
        }
    });
}
pub(crate) fn commit<T>(run: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    let result = run();
    update(|m| {
        m.commits.add(1);
        m.commit_failures.add(u128::from(result.is_err()));
    });
    result
}
pub(crate) fn start(
    run: impl FnOnce() -> Result<crate::JobHandle, Error>,
) -> Result<crate::JobHandle, Error> {
    let result = run();
    update(|m| {
        m.job_starts.add(1);
        m.job_failures.add(u128::from(result.is_err()));
    });
    result
}
pub(crate) fn resume<T>(
    run: impl FnOnce() -> Result<JobStep<T>, Error>,
) -> Result<JobStep<T>, Error> {
    let result = run();
    update(|m| {
        m.job_resumes.add(1);
        m.job_failures.add(u128::from(result.is_err()));
    });
    result
}
