//! Transport types shared by the local DB console and its fixture canister.
#![forbid(unsafe_code)]

#[cfg(feature = "host")]
pub mod host;

/// A local query result, in the fixture's positional Wire field order.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ConsolePage {
    /// Display column names.
    pub columns: Vec<String>,
    /// Display cells in column order.
    pub rows: Vec<Vec<String>>,
    /// Encoded continuation for a paged read.
    pub next: Option<Vec<u8>>,
    /// Examined query candidates.
    pub examined: u32,
    /// Fetched source rows.
    pub fetched: u32,
    /// Encoded result bytes before transport framing.
    pub output_bytes: u32,
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

/// Selection shared by the private retained fixture when starting and resuming a job.
pub type RetainedSelection = (
    String,
    String,
    (Option<u64>, Option<u64>, Option<u64>),
    u32,
    (Option<String>, Option<String>),
    Option<String>,
    RowFilters,
    (Vec<String>, bool),
);

/// Display columns, rows and optional next cursor of a completed retained query.
pub type RetainedOutput = (Vec<String>, Vec<Vec<String>>, Option<Vec<u8>>);

/// Progress reported by the local fixture after one retained query step.
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RetainedProgress {
    /// Last saved step.
    pub sequence: u64,
    /// Candidates examined so far.
    pub examined: u64,
    /// Input bytes examined so far.
    pub input_bytes: u64,
    /// Encoded result bytes in this step.
    pub output_bytes: u64,
    /// Current phase and any completed result.
    pub phase: RetainedPhase,
}

/// Only a completed job has output.
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RetainedPhase {
    /// Additional input remains.
    Running,
    /// Completed rows and continuation.
    Complete(RetainedOutput),
    /// Source rows changed.
    Invalidated,
    /// Job lifetime elapsed.
    Expired,
}

impl RetainedPhase {
    /// Name used by console progress messages.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Complete(_) => "Complete",
            Self::Invalidated => "Invalidated",
            Self::Expired => "Expired",
        }
    }
}
