//! Native entropy for ULID issuance outside an IC execution.

use super::UlidEntropyError;

pub(super) fn identity() -> ([u8; 32], u64) {
    thread_local! {
        static SEED: [u8; 32] = {
            let mut seed = [0; 32];
            getrandom::fill(&mut seed).expect("native ULID stream entropy unavailable");
            seed
        };
    }

    SEED.with(|seed| (*seed, u64::from(std::process::id())))
}

pub(super) async fn fresh_suffix() -> Result<[u8; 10], UlidEntropyError> {
    let mut seed = [0; 10];
    getrandom::fill(&mut seed).map_err(|_| UlidEntropyError::Unavailable)?;

    Ok(seed)
}
