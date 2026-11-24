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
    /// Replace two managed rows and choose how the first persisted row is retrieved.
    ReplaceManaged {
        /// Capture the selected after-image during commit when true, otherwise read it back.
        returning: bool,
        /// New body for the selected row.
        body: String,
    },
    /// Upsert row 1 alone, or together with row 2, returning the persisted first row.
    UpsertManaged(String, Option<String>),
}

/// Actions on a retained count.
#[derive(Debug, Serialize, Deserialize)]
pub enum CountRequest {
    /// Run the count against the source or target schema selected by the fixture.
    Run(bool, CountAction),
    /// Release the retained job independently of schema admission.
    Cancel(Vec<u8>),
    /// Remove at most this many expired jobs at host time.
    CleanupExpired(u8),
    /// Run an item count with the given parent equality on the source schema.
    ForParent {
        /// Required parent ID.
        parent: u64,
        /// Select the compound parent index explicitly.
        indexed: bool,
        /// Start or advance the selected job.
        action: CountAction,
    },
}

/// Count operations that require a live entity.
#[derive(Debug, Serialize, Deserialize)]
pub enum CountAction {
    /// Capture the item revision for an exact count.
    Start,
    /// Resume at the supplied sequence with a per-call candidate budget.
    Resume(Vec<u8>, u64, u16),
    /// Start counting items per parent.
    StartGrouped,
    /// Advance counting by parent within the candidate budget.
    ResumeGrouped(Vec<u8>, u64, u16),
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
    /// Completed counts in parent key order.
    Groups(Vec<(u64, u64)>),
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
    /// Sequence, cumulative examined candidates and grouped outcome.
    GroupAdvanced(u64, u64, CountOutcome),
    /// Number of expired jobs removed by this update.
    Removed(u64),
}

/// Result from the application update mount.
#[derive(Debug, Serialize, Deserialize)]
pub enum Reply {
    /// Batch outcome and instructions inside its handler.
    Batch(Result<(), u8>, u64),
    /// Retained job outcome or refusal, and instructions inside its handler.
    Count(Result<CountReply, String>, u64),
    /// Selected row (key, body, creation and update milliseconds) and handler instructions.
    Managed(Result<(u64, String, i64, i64), u8>, u64),
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
    /// Look up a parent by primary ID.
    ParentById(u64),
    /// Look up a parent by its unique code.
    ParentByCode(u64),
    /// Count the selected parent's items within 512 candidates.
    ParentCount(u64),
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
    /// List source items at or above a score in ascending (score, ID) order.
    RankedItems {
        /// Inclusive lower bound.
        minimum: u64,
        /// Continuation returned by the previous ranked page.
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
    /// Owner rows fetched during this page.
    pub fetched: u64,
}

/// Query result and the work measured inside the handler.
#[derive(Debug, Serialize, Deserialize)]
pub enum QueryReply {
    /// Parent ID and code, or absence, with handler instructions.
    Parent(Result<Option<(u64, u64)>, u8>, u64),
    /// Count, examined candidates and fetched rows, with handler instructions.
    Count(Result<(u64, u64, u64), u8>, u64),
    /// Complete model observation or refusal, and instructions.
    Snapshot(Result<Snapshot, u8>, u64),
    /// Bounded listing or refusal, and instructions.
    Items(Result<ItemPage, u8>, u64),
}
