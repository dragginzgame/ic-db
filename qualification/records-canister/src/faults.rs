//! Feature-selected backing faults around the unmodified product commit path.
//! No persistent fault state, allocator replacement or product testing API.

use super::{Batch, DefaultMemoryImpl, Document, Entry, cdk, open, runtime};
use ic_storage::{BackingIdentity, Memory, MemoryBacking};
use std::cell::Cell;

#[derive(Clone, Copy)]
struct Injection {
    stop_after: u64,
    deny_growth: bool,
    writes: u64,
    grows: u64,
}
thread_local! {
    static INJECTION: Cell<Option<Injection>> = const { Cell::new(None) };
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
                    cdk::api::trap("M1 injected trap after backing write");
                }
            }
        });
    }
}

// first=0 changes two entries and repoints their dependent. Otherwise insert
// one maximum-cardinality batch. Arm only after open and typed batch construction,
// so the counted backing writes belong to Database::commit and its supplier.
#[ic_wire_endpoint::update(id = 6)]
fn fault_batch((first, stop_after, deny_growth): (u64, u64, bool)) -> (u64, u64) {
    assert!(first <= u64::MAX - 32);
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
            writes: 0,
            grows: 0,
        }))
    });
    db.commit(batch)
        .expect("product commit must complete or trap after mutation");
    let state = INJECTION.with(|slot| slot.take().unwrap());
    (state.writes, state.grows)
}
