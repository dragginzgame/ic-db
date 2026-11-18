//! Seeded issuance and heap-local stream allocation.
use super::{Ulid, UlidGenerator};
use crate::{Principal, Timestamp};
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};

thread_local! {
    static IC_STREAM: RefCell<Option<SeededUlidGenerator>> = const { RefCell::new(None) };
    static LAST_STREAM: Cell<u64> = const { Cell::new(0) };
}

/// A canister-owned or application-owned sequence seeded once at creation.
/// Each newer millisecond starts at the stream's seed. IDs are predictable.
pub struct SeededUlidGenerator {
    issuer: UlidGenerator,
    seed: [u8; 10],
}

impl SeededUlidGenerator {
    pub(super) fn new(mut seed: [u8; 10]) -> Self {
        seed[0] &= 0x7f;
        Self {
            issuer: UlidGenerator::new(),
            seed,
        }
    }

    pub(super) fn at(&mut self, time_ns: u64) -> Ulid {
        self.issuer
            .generate(
                Timestamp::from_millis((time_ns / 1_000_000) as i64),
                self.seed,
            )
            .expect("ULID suffix exhausted at the current logical timestamp")
    }
}

fn stream((principal, version): (Principal, u64), number: Option<u64>) -> SeededUlidGenerator {
    let domain: &[u8] = if number.is_some() {
        b"ic-db:ulid-owned:v1"
    } else {
        b"ic-db:ulid-stream:v1"
    };
    let mut hash = Sha256::new()
        .chain_update(domain)
        .chain_update(principal.as_slice())
        .chain_update(version.to_be_bytes());
    if let Some(number) = number {
        // A separate tag and fixed-width serial distinguish owned streams.
        hash.update(number.to_be_bytes());
    }
    let digest = hash.finalize();
    SeededUlidGenerator::new(std::array::from_fn(|i| digest[i]))
}

pub(super) fn generate(time_ns: u64, identity: impl FnOnce() -> (Principal, u64)) -> Ulid {
    IC_STREAM.with_borrow_mut(|slot| {
        slot.get_or_insert_with(|| stream(identity(), None))
            .at(time_ns)
    })
}

pub(super) fn owned(identity: (Principal, u64)) -> SeededUlidGenerator {
    let number = LAST_STREAM.with(|slot| {
        let next = slot
            .get()
            .checked_add(1)
            .expect("ULID stream numbers exhausted");
        slot.set(next);
        next
    });
    stream(identity, Some(number))
}
