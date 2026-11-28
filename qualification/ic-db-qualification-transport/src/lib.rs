//! Shared private fixture requests, codecs and Platform dispatch hooks.
#![forbid(unsafe_code)]

mod hooks;
pub mod mixed;

pub use hooks::*;
pub use ic_db_cli::fixture_wire;

use ic_wire::{Direction, Encoding, Error, Limits};
use serde::{Serialize, de::DeserializeOwned};

/// Decoded request and the instruction meter supplied by installed dispatch.
pub type Measured<T> = (T, fn() -> u64);

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
    fixture_wire::decode(
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
    fixture_wire::encode(
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

pub use ic_db_cli::{
    ComparisonFilter, ConsolePage, DisplayPage, EntityFilters, EntityView, FieldRange, FieldView,
    FilterComparison, IndexView, PayloadFilters, QueryOptions, QueryRequest, RetainedPhase,
    RetainedProgress, RetainedResume, RetainedSelection, RowFilters, SchemaResponse, Status,
    TypeMemberView, TypeView, lowercase_hex,
};
