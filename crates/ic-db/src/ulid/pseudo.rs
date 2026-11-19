//! Seeded issuance and heap-local stream allocation.
use super::{Ulid, UlidEntropyError, UlidGenerationError, UlidGenerator, platform};
use crate::Timestamp;
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};

thread_local! {
    static DEFAULT_STREAM: RefCell<Option<SeededUlidGenerator>> = const { RefCell::new(None) };
    static LAST_STREAM: Cell<u64> = const { Cell::new(0) };
}

/// A canister-owned or application-owned sequence seeded once at creation.
/// Each newer millisecond starts at the stream's seed. IDs are predictable.
pub struct SeededUlidGenerator {
    issuer: UlidGenerator,
    seed: [u8; 10],
}

impl SeededUlidGenerator {
    fn new(mut seed: [u8; 10]) -> Self {
        seed[0] &= 0x7f;
        Self {
            issuer: UlidGenerator::new(),
            seed,
        }
    }

    /// Issue at the timestamp already captured for an operation such as `Batch::at`.
    /// Equal or earlier timestamps advance the suffix at the last logical millisecond.
    ///
    /// # Errors
    /// Returns the same timestamp or suffix refusal as [`UlidGenerator::generate`].
    /// A refusal preserves the stream position.
    pub fn next_at(&mut self, timestamp: Timestamp) -> Result<Ulid, UlidGenerationError> {
        self.issuer.generate(timestamp, self.seed)
    }

    fn at(&mut self, time_ns: u64) -> Ulid {
        self.next_at(Timestamp::from_millis((time_ns / 1_000_000) as i64))
            .expect("ULID suffix exhausted at the current logical timestamp")
    }

    /// Issue synchronously at the current time, advancing only this stream.
    ///
    /// # Panics
    /// Traps on exhaustion after at least 2^79 same-millisecond increments.
    pub fn next_id(&mut self) -> Ulid {
        self.at(platform::time())
    }
}

fn stream((owner, version): (impl AsRef<[u8]>, u64), number: Option<u64>) -> SeededUlidGenerator {
    let domain: &[u8] = if number.is_some() {
        b"ic-db:ulid-owned:v1"
    } else {
        b"ic-db:ulid-stream:v1"
    };
    let mut hash = Sha256::new()
        .chain_update(domain)
        .chain_update(owner.as_ref())
        .chain_update(version.to_be_bytes());
    if let Some(number) = number {
        // A separate tag and fixed-width serial distinguish owned streams.
        hash.update(number.to_be_bytes());
    }
    let digest = hash.finalize();
    SeededUlidGenerator::new(std::array::from_fn(|i| digest[i]))
}

pub(super) fn generate<T: AsRef<[u8]>>(time_ns: u64, identity: impl FnOnce() -> (T, u64)) -> Ulid {
    DEFAULT_STREAM.with_borrow_mut(|slot| {
        slot.get_or_insert_with(|| stream(identity(), None))
            .at(time_ns)
    })
}

pub(super) fn owned(identity: (impl AsRef<[u8]>, u64)) -> SeededUlidGenerator {
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

impl Ulid {
    /// Issue synchronously from the current thread’s predictable stream.
    /// # Panics
    /// Panics on suffix exhaustion or if the native OS cannot seed the thread.
    #[must_use]
    pub fn generate_prand() -> Self {
        generate(platform::time(), platform::identity)
    }

    /// Start an owned generator with fresh entropy from the IC or native OS. Later issuance is synchronous.
    /// # Errors
    /// Returns the entropy request or framing error.
    pub async fn generator() -> Result<SeededUlidGenerator, UlidEntropyError> {
        Ok(SeededUlidGenerator::new(platform::fresh_suffix().await?))
    }

    /// Start an independent predictable stream under the current canister or native thread identity.
    /// # Panics
    /// Panics on stream counter exhaustion or if the native OS cannot seed the thread.
    #[must_use]
    pub fn generator_prand() -> SeededUlidGenerator {
        owned(platform::identity())
    }
}
