//! Installed fault probes specialize the same engine for instrumented stable memory.
use crate::Backing;
use ic_db::qualification;
pub(crate) type Database = qualification::Database<Backing>;
pub(crate) type Query<'a, E> = qualification::Query<'a, E, Backing>;
#[cfg(any(
    feature = "aggregate-jobs",
    feature = "mixed-lifecycle",
    feature = "retained-console"
))]
pub(crate) type AggregateJobs = qualification::AggregateJobs<Backing>;
#[cfg(feature = "maintenance")]
pub(crate) type Maintenance = qualification::Maintenance<Backing>;
#[cfg(any(
    feature = "maintenance",
    feature = "retained-console",
    feature = "mixed-lifecycle"
))]
pub(crate) type RetiredSource = qualification::RetiredSource<Backing>;
#[cfg(any(
    feature = "maintenance",
    feature = "retained-console",
    feature = "mixed-lifecycle"
))]
pub(crate) type SchemaCopy = qualification::SchemaCopy<Backing>;
