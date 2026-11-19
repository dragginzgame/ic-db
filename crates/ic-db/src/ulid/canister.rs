//! Update-call convenience API. Entropy is requested only by the async generator constructor.
use super::UlidEntropyError;
use crate::Principal;
use ic_platform_protocol::raw_rand;
use ic_platform_runtime::{
    call::{Call, Outcome, Wait},
    system,
};

pub(super) async fn fresh_suffix() -> Result<[u8; 10], UlidEntropyError> {
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

pub(super) fn identity() -> (Principal, u64) {
    (system::canister_self(), system::canister_version())
}

pub(super) use system::time;
