//! Fixed private fixture transport. Platform owns dispatch/reply; Wire owns frames.
//! These are codec hooks, not a product endpoint API or a Canic dependency.

#![forbid(unsafe_code)]

use ic_platform_runtime::{cdk, ic0};
use ic_wire::{Direction, Encoding, Error, Limits};
use serde::{Serialize, de::DeserializeOwned};

// Fixed fixture budgets, independently selectable in each direction.
const REQUEST_LIMITS: Limits = Limits {
    bytes: 65_536,
    allocation: 8 * 1024 * 1024,
    nodes: 65_536,
    depth: 32,
};
const RESPONSE_LIMITS: Limits = REQUEST_LIMITS;

/// Check ingress length and frame identity before Platform copies its payload.
/// # Errors
/// Never returns an error: invalid ingress traps, preserving prior fixture semantics.
pub fn admit<const ID: u64>() -> Result<(), String> {
    admit_frame(ID);
    Ok(())
}

// Shared admission before Platform copies the complete argument buffer.
#[cfg_attr(target_arch = "wasm32", inline(never))]
fn admit_frame(id: u64) {
    let length = ic0::msg_arg_data_size();
    if length > REQUEST_LIMITS.bytes {
        cdk::api::trap("Wire request exceeds byte allowance");
    }
    // Wire's header admission takes an exact 11-byte array.
    let mut header = [0; 11];
    if length < header.len() {
        trap_error(Error::Malformed);
    }
    ic0::msg_arg_data_copy(&mut header, 0);
    ic_wire::validate_request_header(id, &header).unwrap_or_else(|error| trap_error(error));
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform's decoder hook owns the copied argument buffer"
)]
/// Decode one owned fixture request, trapping on invalid or excessive input.
#[must_use]
pub fn decode<T: DeserializeOwned, const ID: u64>(bytes: Vec<u8>) -> T {
    ic_wire::decode(
        &bytes,
        Encoding::Frame {
            method: ID,
            direction: Direction::Request,
        },
        REQUEST_LIMITS,
    )
    .unwrap_or_else(|error| trap_error(error))
}

/// Encode one fixture response, trapping on failure to roll back this execution.
#[must_use]
pub fn encode<T: Serialize, const ID: u64>(value: T) -> Vec<u8> {
    // Trap after mutation so synchronous writes roll back with the message.
    ic_wire::encode(
        &value,
        Encoding::Frame {
            method: ID,
            direction: Direction::Response,
        },
        RESPONSE_LIMITS,
    )
    .unwrap_or_else(|error| trap_error(error))
}

#[cold]
#[inline(never)]
fn trap_error(error: Error) -> ! {
    cdk::api::trap(error.to_string())
}
