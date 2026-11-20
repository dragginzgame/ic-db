//! Current host time in nanoseconds since the Unix epoch.

#[cfg(target_arch = "wasm32")]
pub(crate) use ic_platform_runtime::system::time as time_ns;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn time_ns() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX)
}
