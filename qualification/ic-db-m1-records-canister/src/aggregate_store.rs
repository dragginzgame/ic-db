//! Host custody shared by retained-job qualification and the local console.

use super::{Backing, MemoryRuntime, StableKey};
use crate::handles::AggregateJobs;

pub(super) const KEY: &str = "m1.aggregate_jobs.v1";

pub(super) fn initialize(host: &MemoryRuntime<Backing>) {
    AggregateJobs::create(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
        [3; 16],
    )
    .unwrap();
}

pub(super) fn open_jobs(host: &MemoryRuntime<Backing>) -> AggregateJobs {
    AggregateJobs::open(host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap()).unwrap()
}
