//! Compiled schema and record types for the M1 qualification canisters.

mod model;

pub use model::*;

ic_db::include_schema!();
