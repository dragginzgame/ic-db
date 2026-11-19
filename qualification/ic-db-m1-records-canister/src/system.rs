//! IC operations used by the installed probes. Native execution has no IC instruction meter.
#[cfg(target_arch = "wasm32")]
pub(crate) use ic_platform_runtime::system::trap;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn trap(message: &str) -> ! {
    panic!("{message}")
}
