//! Shared private fixture requests and codecs. Platform ingress and traps require Wasm.
#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
mod hooks;
pub mod mixed;
#[cfg(target_arch = "wasm32")]
pub use hooks::*;

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

/// Decode one fixture request within the selected Wire envelope.
/// # Errors
/// Returns malformed or over-budget input for the endpoint's trapping policy.
pub fn decode<T: DeserializeOwned, const ID: u64>(bytes: &[u8]) -> Result<T, Error> {
    ic_wire::decode(
        bytes,
        Encoding::Frame {
            method: ID,
            direction: Direction::Request,
        },
        REQUEST_LIMITS,
    )
}

/// Encode one fixture response before any reply append.
/// # Errors
/// Returns encoding refusal for the endpoint's trapping policy.
pub fn encode<T: Serialize, const ID: u64>(value: T) -> Result<Vec<u8>, Error> {
    // Trap after mutation so synchronous writes roll back with the message.
    ic_wire::encode(
        &value,
        Encoding::Frame {
            method: ID,
            direction: Direction::Response,
        },
        RESPONSE_LIMITS,
    )
}

/// Lifecycle fixtures require exactly empty raw arguments, not a Wire frame.
/// # Errors
/// Refuses any nonempty lifecycle argument.
pub fn empty(bytes: &[u8]) -> Result<(), &'static str> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err("unexpected lifecycle arguments")
    }
}

/// Row predicates carried by the local query and retained-aggregate commands.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct RowFilters {
    /// Lower bound, upper bound, prefix, suffix and substring on BinaryObject.payload.
    pub payload: [Option<Vec<u8>>; 5],
    /// Left field, comparison, right field.
    pub compare: Option<[String; 3]>,
    /// Field, comparison and length threshold.
    pub length: Option<(String, String, u32)>,
    /// Require a value in this optional field.
    pub present: Option<String>,
    /// Require absence in this optional field.
    pub absent: Option<String>,
    /// Field, comparison and exact UTF-8 literal.
    pub text: Option<[String; 3]>,
    /// Field and Boolean equality operand.
    pub boolean: Option<(String, bool)>,
    /// Field, inclusive lower field and inclusive upper field.
    pub between_fields: Option<[String; 3]>,
    /// Field, lower field and upper field for the strict outside-range comparison.
    pub not_between_fields: Option<[String; 3]>,
}
