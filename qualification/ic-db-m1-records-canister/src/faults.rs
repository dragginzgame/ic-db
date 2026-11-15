//! Feature-selected backing faults around the unmodified product commit path.
//! No persistent fault state, allocator replacement or product testing API.

use super::{Batch, DefaultMemoryImpl, Document, Entry, cdk, open, runtime};
use ic_storage::{BackingIdentity, Memory, MemoryBacking};
use std::cell::Cell;

#[derive(Clone, Copy)]
struct Injection {
    stop_after: u64,
    deny_growth: bool,
    exhaust_instructions: bool,
    writes: u64,
    grows: u64,
}
thread_local! {
    static INJECTION: Cell<Option<Injection>> = const { Cell::new(None) };
}

#[cfg(feature = "aggregate-jobs")]
pub(super) fn arm_job_write(stop_after: u64) {
    INJECTION.with(|slot| {
        slot.set(Some(Injection {
            stop_after,
            deny_growth: false,
            exhaust_instructions: false,
            writes: 0,
            grows: 0,
        }))
    });
}

#[cfg(feature = "aggregate-jobs")]
pub(super) fn finish_job_write() -> u64 {
    INJECTION.with(|slot| slot.take().unwrap().writes)
}

#[derive(Default)]
pub(super) struct FaultMemory(DefaultMemoryImpl);

impl MemoryBacking for FaultMemory {
    fn backing_identity(&self) -> BackingIdentity {
        self.0.backing_identity()
    }
}

impl Memory for FaultMemory {
    fn size(&self) -> u64 {
        self.0.size()
    }
    fn read(&self, offset: u64, bytes: &mut [u8]) {
        self.0.read(offset, bytes);
    }
    fn grow(&self, pages: u64) -> i64 {
        let refused = INJECTION.with(|slot| {
            let Some(mut state) = slot.get() else {
                return false;
            };
            state.grows += 1;
            slot.set(Some(state));
            state.deny_growth
        });
        if refused { -1 } else { self.0.grow(pages) }
    }
    fn write(&self, offset: u64, bytes: &[u8]) {
        self.0.write(offset, bytes);
        INJECTION.with(|slot| {
            if let Some(mut state) = slot.get() {
                state.writes += 1;
                slot.set(Some(state));
                if state.writes == state.stop_after {
                    if state.exhaust_instructions {
                        // Stay inside the actual commit after a backing write.
                        // The IC instruction meter must terminate this message;
                        // do not simulate exhaustion with an application trap.
                        let mut work = [0_u8; 4096];
                        loop {
                            work.fill(cdk::api::instruction_counter() as u8);
                            std::hint::black_box(&work);
                        }
                    }
                    cdk::api::trap("M1 injected trap after backing write");
                }
            }
        });
    }
}

// first=0 changes two entries and repoints their dependent. Otherwise insert
// one maximum-cardinality batch. Arm only after open and typed batch construction,
// so the counted backing writes belong to Database::commit and its supplier.
#[ic_platform_runtime::cdk::update(
    guard = "crate::transport::admit::<6>",
    decode_with = "crate::transport::decode::<_, 6>",
    encode_with = "crate::transport::encode::<_, 6>"
)]
fn fault_batch(
    (first, stop_after, deny_growth, exhaust_instructions): (u64, u64, bool, bool),
) -> (u64, u64) {
    assert!(first <= u64::MAX - 32);
    assert!(!exhaust_instructions || stop_after > 0);
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    if first == 0 {
        let mut document = db.get::<Document>(1).unwrap().unwrap();
        document.entry_id = 2;
        batch.replace(&document).unwrap();
        for id in 1..=2 {
            batch
                .replace(&Entry {
                    id,
                    category: 9,
                    amount: 900 + id,
                })
                .unwrap();
        }
    } else {
        for id in first..first + 32 {
            batch
                .insert(&Entry {
                    id,
                    category: 9,
                    amount: u64::MAX,
                })
                .unwrap();
        }
    }
    INJECTION.with(|slot| {
        slot.set(Some(Injection {
            stop_after,
            deny_growth,
            exhaust_instructions,
            writes: 0,
            grows: 0,
        }))
    });
    db.commit(batch)
        .expect("product commit must complete or trap after mutation");
    let state = INJECTION.with(|slot| slot.take().unwrap());
    (state.writes, state.grows)
}
