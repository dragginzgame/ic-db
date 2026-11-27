//! Independent decoder for the installed job_advance reply.
#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
pub struct Step {
    pub sequence: u64,
    pub source_revision: u64,
    pub examined: u64,
    pub input_bytes: u64,
    pub phase: u8,
    pub value: Option<String>,
    pub writes: u64,
}
