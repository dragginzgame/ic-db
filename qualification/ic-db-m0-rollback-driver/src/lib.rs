#![forbid(unsafe_code)]
//! Shared simulator operations exposed to the qualification probes.

pub use ic_db_cli::host::{LIMITS, instance, upgrade_stopped};

/// Read the records or console fixture selected by the qualification target.
/// # Panics
/// Panics if the fixture path is missing or unreadable.
#[must_use]
pub fn records_wasm() -> Vec<u8> {
    fixture_wasm("IC_DB_M1_RECORDS_WASM")
}

/// Read the M0 core fixture selected by the qualification target.
/// # Panics
/// Panics if the fixture path is missing or unreadable.
#[must_use]
pub fn core_wasm() -> Vec<u8> {
    fixture_wasm("IC_DB_M0_WASM")
}

fn fixture_wasm(variable: &str) -> Vec<u8> {
    let path = std::path::PathBuf::from(
        std::env::var_os(variable)
            .unwrap_or_else(|| panic!("qualification target must set {variable}")),
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}
