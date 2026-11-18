//! Update-call convenience API. Entropy is requested only by the async generator constructor.
use super::{SeededUlidGenerator, Ulid, UlidEntropyError, pseudo};
use crate::Principal;
use ic_platform_protocol::raw_rand;
use ic_platform_runtime::{
    call::{Call, Outcome, Wait},
    system,
};

async fn fresh_suffix() -> Result<[u8; 10], UlidEntropyError> {
    let call = Call::new(
        Principal::management_canister(),
        raw_rand::METHOD,
        Wait::Bounded { seconds: 10 },
    )
    .args(raw_rand::encode(6).map_err(|_| UlidEntropyError::InvalidFrame)?)
    .execute()
    .await
    .map_err(|_| UlidEntropyError::Unavailable)?;
    let Outcome::Reply(reply) = call.into_parts().0 else {
        return Err(UlidEntropyError::Unavailable);
    };
    let bytes = raw_rand::decode(&reply, 42).map_err(|_| UlidEntropyError::InvalidFrame)?;
    Ok(std::array::from_fn(|i| bytes[i]))
}

fn identity() -> (Principal, u64) {
    (system::canister_self(), system::canister_version())
}

impl SeededUlidGenerator {
    /// Issue synchronously at IC time, advancing only this stream.
    ///
    /// # Panics
    /// Traps on exhaustion after at least 2^79 same-millisecond increments.
    pub fn next_id(&mut self) -> Ulid {
        self.at(system::time())
    }
}

impl Ulid {
    /// Issue synchronously from the canister's private predictable stream in an update.
    /// # Panics
    /// Traps if the stream exhausts its suffix at the current logical millisecond.
    #[must_use]
    pub fn generate_prand() -> Self {
        pseudo::generate(system::time(), identity)
    }

    /// Start an owned generator with one fresh entropy request. Later issuance is synchronous.
    /// # Errors
    /// Returns the entropy request or framing error.
    pub async fn generator() -> Result<SeededUlidGenerator, UlidEntropyError> {
        Ok(SeededUlidGenerator::new(fresh_suffix().await?))
    }

    /// Start an independent predictable stream in a canister update without requesting entropy.
    /// # Panics
    /// Traps if the heap's 64-bit stream allocation counter is exhausted.
    #[must_use]
    pub fn generator_prand() -> SeededUlidGenerator {
        pseudo::owned(identity())
    }
}
