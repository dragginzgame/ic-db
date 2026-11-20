//! Update-call convenience API. Entropy is requested only by the async generator constructor.

use super::UlidEntropyError;
use crate::Principal;
use ic_platform_runtime::{raw_rand, system};

pub(super) async fn fresh_suffix() -> Result<[u8; 10], UlidEntropyError> {
    let bytes = raw_rand::request(10).await.map_err(|error| match error {
        raw_rand::Error::Frame(_) => UlidEntropyError::InvalidFrame,
        raw_rand::Error::Submission(_)
        | raw_rand::Error::Rejected(_)
        | raw_rand::Error::OutcomeUnknown(_) => UlidEntropyError::Unavailable,
    })?;

    Ok(std::array::from_fn(|i| bytes[i]))
}

pub(super) fn identity() -> (Principal, u64) {
    (system::canister_self(), system::canister_version())
}
