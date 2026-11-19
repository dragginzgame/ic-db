//! Monotonic issuance for explicitly owned generators.

use super::Ulid;
use crate::Timestamp;
use std::fmt;

/// Monotonic ULIDs for one explicitly owned issuance stream.
///
/// Supply the timestamp captured for the write operation and fresh host-provided
/// randomness. This generator never reads a clock, seeds an RNG or makes a call.
/// Equal/backward timestamps retain the last timestamp and increment its suffix.
/// That logical timestamp can differ from the operation's managed timestamps.
///
/// Successful issuance consumes an ID even if a later database batch is refused;
/// gaps are permitted. An IC trap reverts in-canister state in that execution.
/// The host must preserve the last issued ID and restore it after upgrades if
/// monotonicity must continue. Restoring an older checkpoint or starting another
/// stream can repeat IDs. Independent streams need independently provisioned
/// randomness; neither global uniqueness nor secret-token suitability is promised.
#[derive(Debug, Default)]
pub struct UlidGenerator {
    last: Option<Ulid>,
}

impl UlidGenerator {
    /// Start a new stream with no preceding identifier, including at Unix epoch.
    #[must_use]
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// Continue after the host's authoritative last issued identifier.
    /// Restoring stale state can repeat identifiers; the database does not own this checkpoint.
    #[must_use]
    pub const fn resume_after(last: Ulid) -> Self {
        Self { last: Some(last) }
    }

    /// The last successfully issued identifier, suitable for a host-owned checkpoint.
    #[must_use]
    pub const fn last(&self) -> Option<Ulid> {
        self.last
    }

    /// Issue an ID at the operation's timestamp using an explicit 80-bit random suffix.
    ///
    /// Randomness is used only for the first ID or a timestamp newer than the last
    /// issued timestamp. The caller must obtain it before the synchronous write
    /// boundary; it may use an already seeded host stream without an inter-canister call.
    ///
    /// # Errors
    /// Rejects negative or greater-than-48-bit millisecond timestamps. If the
    /// suffix is exhausted at an equal/backward time, refuses without advancing
    /// time or wrapping the suffix. Every error preserves the previous state.
    pub fn generate(
        &mut self,
        timestamp: Timestamp,
        randomness: [u8; 10],
    ) -> Result<Ulid, UlidGenerationError> {
        let millis = u64::try_from(timestamp.as_millis())
            .ok()
            .filter(|value| *value < (1_u64 << 48))
            .ok_or(UlidGenerationError::InvalidTimestamp)?;
        let previous = self.last.map(|id| ::ulid::Ulid::from_bytes(id.to_bytes()));
        let id = if let Some(previous) = previous.filter(|id| millis <= id.timestamp_ms()) {
            let next = previous
                .increment()
                .map_err(|_| UlidGenerationError::Exhausted)?;

            Ulid::from_bytes(next.to_bytes())
        } else {
            Ulid::from_millis_and_suffix(millis, randomness)
        };
        self.last = Some(id);

        Ok(id)
    }
}

/// A refused issuance; the generator's previous state remains unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UlidGenerationError {
    /// Unix milliseconds must fit the ULID's unsigned 48-bit timestamp.
    InvalidTimestamp,
    /// All suffixes at the last logical timestamp have been consumed.
    Exhausted,
}

impl fmt::Display for UlidGenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidTimestamp => "ULID timestamp must fit unsigned 48-bit milliseconds",
            Self::Exhausted => "ULID suffix exhausted at the current logical timestamp",
        })
    }
}
impl std::error::Error for UlidGenerationError {}
