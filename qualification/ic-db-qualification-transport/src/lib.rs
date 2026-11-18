//! Shared private fixture requests and Wasm-only platform codec hooks.
#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
mod hooks;
#[cfg(target_arch = "wasm32")]
pub use hooks::*;

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
}
