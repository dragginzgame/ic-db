//! IC operations used by the installed probes. Native execution has no IC instruction meter.
#[cfg(target_arch = "wasm32")]
pub(crate) use ic_platform_runtime::{raw::instruction_counter, system::trap};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn instruction_counter() -> u64 {
    panic!("IC instruction measurements require an installed canister")
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn trap(message: &str) -> ! {
    panic!("{message}")
}
