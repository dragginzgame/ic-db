//! Compiled schema and record types for the M1 qualification canisters.

mod model;

pub use model::*;

include!(concat!(env!("OUT_DIR"), "/schema.rs"));
