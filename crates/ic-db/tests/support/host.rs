#[path = "allocations.rs"]
mod allocations;
use ic_storage::{MemoryBacking, MemoryRuntime};

pub fn host<M: MemoryBacking>(backing: M) -> MemoryRuntime<M> {
    allocations::host_with(backing, &["test.db.v1"])
}
