//! Platform dispatch and Wire frame handling for private Wasm fixtures.

use super::REQUEST_LIMITS;
use ic_platform_runtime::{
    entrypoints::{AdmissionError, EndpointError, UpdateError},
    raw, system,
};
use ic_wire::Error;

// Native processes have no IC instruction meter. Only installed dispatch can
// supply this capability; a measured request cannot report a native substitute.
#[cfg(target_arch = "wasm32")]
const METER: Option<fn() -> u64> = Some(raw::instruction_counter);
#[cfg(not(target_arch = "wasm32"))]
const METER: Option<fn() -> u64> = None;

/// Obtain the IC instruction counter for a measured qualification handler.
/// # Errors
/// A native process has no IC instruction meter.
pub fn instruction_meter() -> Result<fn() -> u64, &'static str> {
    METER.ok_or("IC instruction meter unavailable")
}

/// Decode the wire request and supply the installed instruction meter.
/// # Errors
/// Refuses invalid frames or an unavailable IC instruction meter.
pub fn decode_measured<T: serde::de::DeserializeOwned, const ID: u64>(
    bytes: &[u8],
) -> Result<super::Measured<T>, String> {
    let request = super::decode::<T, ID>(bytes).map_err(|error| error.to_string())?;

    Ok((request, instruction_meter()?))
}

/// Capture the IC message time in nanoseconds for a decoded request.
/// # Errors
/// Returns the same frame errors as the ordinary fixture decoder.
pub fn decode_timed<T: serde::de::DeserializeOwned, const ID: u64>(
    bytes: &[u8],
) -> Result<(T, u64), Error> {
    Ok((super::decode::<T, ID>(bytes)?, system::time()))
}

/// Check ingress length and frame identity before Platform copies its payload.
/// # Errors
/// Never returns an error: invalid ingress traps, preserving prior fixture semantics.
pub fn admit<const ID: u64>() -> Result<(), String> {
    install_panic_hook();
    admit_frame(ID);

    Ok(())
}

// Shared admission before Platform copies the complete argument buffer.
#[inline(never)]
fn admit_frame(id: u64) {
    let length = raw::msg_arg_data_size();
    if length > REQUEST_LIMITS.bytes {
        ic_platform_runtime::system::trap("Wire request exceeds byte allowance");
    }
    // Wire's header admission takes an exact 11-byte array.
    let mut header = [0; 11];
    if length < header.len() {
        trap_error(Error::Malformed);
    }
    raw::msg_arg_data_copy(&mut header, 0);
    ic_wire::validate_request_header(id, &header).unwrap_or_else(|error| trap_error(error));
}

/// Refuse an endpoint failure through the enclosing message rollback boundary.
pub fn endpoint_error(
    error: EndpointError<String, impl std::fmt::Display + std::fmt::Debug, Error>,
) -> ! {
    match error {
        EndpointError::Admission(AdmissionError::Guard(error)) => system::trap(&error),
        EndpointError::Admission(AdmissionError::Decode(error)) => system::trap(&error.to_string()),
        EndpointError::Encode(error) => trap_error(error),
        other => system::trap(&format!("Platform endpoint: {other:?}")),
    }
}

/// Qualification lifecycle has no application authorization policy.
/// # Errors
/// Never refuses; lifecycle ownership is exercised by the explicit host hook.
#[must_use]
pub fn allow() -> Result<(), std::convert::Infallible> {
    install_panic_hook();

    Ok(())
}

// Platform no longer chooses an application's panic reporting policy. Preserve
// this fixture's explicit IC trap diagnostics, including after heap-resetting upgrades.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| system::trap(&info.to_string())));
}

/// Preserve lifecycle failure as an upgrade/init trap, never a normal return.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform transfers the owned lifecycle error to its non-returning callback"
)]
pub fn lifecycle_error(error: UpdateError<std::convert::Infallible, &'static str>) -> ! {
    system::trap(&format!("Platform lifecycle: {error:?}"))
}

#[cold]
#[inline(never)]
fn trap_error(error: Error) -> ! {
    system::trap(&error.to_string())
}
