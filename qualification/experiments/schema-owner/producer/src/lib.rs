//! Shared declarations and the generated public schema exports.

pub mod model;

include!(concat!(env!("OUT_DIR"), "/schema.rs"));
