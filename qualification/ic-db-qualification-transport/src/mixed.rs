//! Application protocol for the three-entity workload.

use serde::{Deserialize, Serialize};

/// Entity, operation, key and two operands in a fixture batch.
pub type Command = (u8, u8, u64, u64, u64);

/// Authorized actions on the Canic update mount.
#[derive(Debug, Serialize, Deserialize)]
pub enum Update {
    /// Commit the commands to the selected schema.
    Batch(bool, Vec<Command>),
    /// Manage one bounded count.
    Count(CountRequest),
}

/// Actions on a retained count.
#[derive(Debug, Serialize, Deserialize)]
pub enum CountRequest {
    /// Run the count against the source or target schema selected by the fixture.
    Run(bool, CountAction),
    /// Release the retained job independently of schema admission.
    Cancel(Vec<u8>),
}

/// Count operations that require a live entity.
#[derive(Debug, Serialize, Deserialize)]
pub enum CountAction {
    /// Capture the item revision for an exact count.
    Start,
    /// Process at most two candidates.
    Resume(Vec<u8>, u64),
}

/// Result of advancing a retained count at its captured revision.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CountOutcome {
    /// More work remains.
    Running,
    /// Exact result for the captured revision.
    Complete(u64),
    /// The source revision changed.
    Invalidated,
    /// The application deadline passed.
    Expired,
}

/// Successful job action.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CountReply {
    /// Opaque handle for subsequent calls.
    Started(Vec<u8>),
    /// New sequence and job outcome.
    Advanced(u64, CountOutcome),
    /// Job storage was released.
    Cancelled,
}

/// Result from the application update mount.
#[derive(Debug, Serialize, Deserialize)]
pub enum Reply {
    /// Batch outcome and instructions inside its handler.
    Batch(Result<(), u8>, u64),
    /// Retained job outcome or refusal, and instructions inside its handler.
    Count(Result<CountReply, String>, u64),
}

/// Complete model observation: rows by entity, counts, revisions, filtered IDs,
/// grouped counts and IDs in score order.
pub type Snapshot = (
    Vec<(u64, u64)>,
    Vec<(u64, u64, u128)>,
    Vec<(u64, u64)>,
    [u64; 3],
    [u64; 3],
    Vec<u64>,
    Vec<(u64, u64)>,
    Vec<u64>,
);

/// Reads accepted by the application.
#[derive(Debug, Serialize, Deserialize)]
pub enum Query {
    /// Compare the selected schema with the independent model.
    Snapshot(bool, u64, u64),
    /// List items in one parent with a minimum score, after an optional cursor.
    Items {
        /// Select the published target or the original source in this migration fixture.
        target: bool,
        /// Parent ID selecting the index prefix.
        parent: u64,
        /// Inclusive lower score bound.
        minimum: u64,
        /// Optional candidate budget. The application selects its listing default when omitted.
        work: Option<u16>,
        /// Opaque continuation returned by the previous page.
        after: Option<Vec<u8>>,
    },
}

/// A bounded item listing.
#[derive(Debug, Serialize, Deserialize)]
pub struct ItemPage {
    /// Matching item IDs in index order.
    pub ids: Vec<u64>,
    /// Position after the last examined candidate.
    pub next: Option<Vec<u8>>,
    /// Candidates examined by this call.
    pub examined: u64,
}

/// Query result and the work measured inside the handler.
#[derive(Debug, Serialize, Deserialize)]
pub enum QueryReply {
    /// Complete model observation or refusal, and instructions.
    Snapshot(Result<Snapshot, u8>, u64),
    /// Bounded listing or refusal, and instructions.
    Items(Result<ItemPage, u8>, u64),
}
