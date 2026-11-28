//! Transport types shared by the local DB console and its fixture canister.
#![forbid(unsafe_code)]

#[cfg(feature = "fixture-wire")]
pub mod fixture_wire;

#[cfg(feature = "host")]
pub mod host;

/// A local query result, in the fixture's positional Wire field order.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ConsolePage {
    /// Displayed rows and their continuation.
    pub page: DisplayPage,
    /// Examined query candidates.
    pub examined: u32,
    /// Fetched source rows.
    pub fetched: u32,
    /// Encoded result bytes before transport framing.
    pub output_bytes: u32,
}

/// Byte predicates on the fixture payload column.
#[derive(Clone, Default, Debug, serde::Deserialize, serde::Serialize)]
pub struct PayloadFilters {
    /// Inclusive lower bound.
    pub minimum: Option<Vec<u8>>,
    /// Inclusive upper bound.
    pub maximum: Option<Vec<u8>>,
    /// Required prefix.
    pub prefix: Option<Vec<u8>>,
    /// Required suffix.
    pub suffix: Option<Vec<u8>>,
    /// Required contiguous bytes.
    pub contains: Option<Vec<u8>>,
}

/// Entity restrictions shared by ordinary and retained fixture queries.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "console", derive(clap::Args))]
pub struct EntityFilters {
    /// Entry.amount >= VALUE.
    #[cfg_attr(feature = "console", arg(long = "min"))]
    pub minimum: Option<u64>,
    /// Entry.category == VALUE.
    #[cfg_attr(feature = "console", arg(long))]
    pub category: Option<u64>,
    /// Document.entry_id == VALUE.
    #[cfg_attr(feature = "console", arg(long))]
    pub entry_id: Option<u64>,
}

/// Local query command in fixture Wire field order.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct QueryRequest {
    /// Selected entity.
    pub entity: String,
    /// Encoded continuation.
    pub cursor: Option<Vec<u8>>,
    /// Maximum rows per page.
    pub limit: u32,
    /// Restrictions on fixture entity fields.
    pub entity_filters: EntityFilters,
    /// Ordered field specifications.
    pub order: Vec<String>,
    /// Reverse traversal.
    pub descending: bool,
    /// Candidate budget.
    pub work: u32,
    /// Output byte budget.
    pub bytes: u32,
    /// Selected output.
    pub select: String,
    /// Grouping and predicates.
    pub options: QueryOptions,
}

/// Options carried together by the query endpoint.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct QueryOptions {
    /// Request distinct output.
    pub distinct: bool,
    /// Lower and upper group bounds.
    pub group_bounds: (Option<String>, Option<String>),
    /// Group ordering.
    pub group_order: Option<String>,
    /// Row predicates.
    pub filters: RowFilters,
}

/// Local fixture status.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct Status {
    /// Schema fingerprint.
    pub schema: [u8; 32],
    /// Store incarnation.
    pub incarnation: [u8; 16],
    /// Entry revision.
    pub revision: u64,
    /// Wasm linear memory pages.
    pub pages: u64,
}

/// Local schema inspection reply.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct SchemaResponse {
    /// Fixture status.
    pub status: Status,
    /// Matching entities.
    pub entities: Vec<EntityView>,
    /// Named value types.
    pub types: Vec<TypeView>,
}

/// Inspected entity.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct EntityView {
    /// Entity name.
    pub name: String,
    /// Schema entity identifier.
    pub id: u32,
    /// Primary key field.
    pub primary: String,
    /// Stored fields.
    pub fields: Vec<FieldView>,
    /// Declared indexes.
    pub indexes: Vec<IndexView>,
}

/// Inspected entity field.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct FieldView {
    /// Field name.
    pub name: String,
    /// Field type.
    pub kind: String,
    /// Optionality.
    pub optional: bool,
    /// Constraints and metadata.
    pub info: String,
    /// Reference target and index.
    pub reference: Option<ReferenceView>,
}

/// Target and supporting index of an inspected reference.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ReferenceView {
    /// Qualified target field.
    pub target: String,
    /// Supporting index name.
    pub index: String,
}

///
/// IndexView
/// An index in the inspected schema.
///
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct IndexView {
    /// Index name.
    pub name: String,
    /// Component fields in index order.
    pub fields: Vec<String>,
    /// Whether the key must be unique.
    pub unique: bool,
    /// Whether rows with absent components are excluded.
    pub present_only: bool,
}

///
/// TypeView
/// A named type in the inspected schema.
///
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TypeView {
    /// Schema name.
    pub name: String,
    /// Type category.
    pub kind: String,
    /// Declared fields, variants or collection roles.
    pub members: Vec<TypeMemberView>,
}

///
/// TypeMemberView
/// One member of a named schema type.
///
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TypeMemberView {
    /// Member name.
    pub name: String,
    /// Member type.
    #[serde(rename = "type")]
    pub kind: String,
    /// Whether the member permits absence.
    pub optional: bool,
    /// Constraints and metadata.
    pub info: String,
}

///
/// FilterComparison
/// Comparison carried by a fixture filter.
///
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, Copy)]
#[cfg_attr(feature = "console", derive(clap::ValueEnum))]
pub enum FilterComparison {
    /// Equal.
    Eq,
    /// Unequal.
    Ne,
    /// Less than.
    Lt,
    /// At most.
    Le,
    /// Greater than.
    Gt,
    /// At least.
    Ge,
    /// Prefix match.
    StartsWith,
    /// Suffix match.
    EndsWith,
    /// Substring match.
    Contains,
}

///
/// ComparisonFilter
/// A field compared with a typed right operand.
///
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ComparisonFilter<T> {
    /// Left field.
    pub field: String,
    /// Comparison to apply.
    pub op: FilterComparison,
    /// Right operand.
    pub operand: T,
}

///
/// FieldRange
/// Three field names in a range predicate.
///
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct FieldRange {
    /// Tested field.
    pub field: String,
    /// Lower end.
    pub lower: String,
    /// Upper end.
    pub upper: String,
}

/// Row predicates carried by the local query and retained-aggregate commands.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct RowFilters {
    /// Lower bound, upper bound, prefix, suffix and substring on BinaryObject.payload.
    pub payload: PayloadFilters,
    /// Left field, comparison, right field.
    pub compare: Option<ComparisonFilter<String>>,
    /// Field, comparison and length threshold.
    pub length: Option<ComparisonFilter<u32>>,
    /// Require a value in this optional field.
    pub present: Option<String>,
    /// Require absence in this optional field.
    pub absent: Option<String>,
    /// Field, comparison and exact UTF-8 literal.
    pub text: Option<ComparisonFilter<String>>,
    /// Field and Boolean equality operand.
    pub boolean: Option<(String, bool)>,
    /// Field, inclusive lower field and inclusive upper field.
    pub between_fields: Option<FieldRange>,
    /// Field, lower field and upper field for the strict outside-range comparison.
    pub not_between_fields: Option<FieldRange>,
}

/// Selection shared by the private retained fixture when starting and resuming a job.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct RetainedSelection {
    /// Entity name.
    pub entity: String,
    /// Aggregate selection.
    pub select: String,
    /// Minimum, category and entry ID restrictions.
    pub entity_filters: EntityFilters,
    /// Maximum output groups.
    pub limit: u32,
    /// Inclusive lower and upper group bounds.
    pub group_bounds: (Option<String>, Option<String>),
    /// Completed-group order.
    pub group_order: Option<String>,
    /// Row predicates.
    pub filters: RowFilters,
    /// Input fields and overall descending flag.
    pub order: (Vec<String>, bool),
}

/// One bounded step of a retained fixture query.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct RetainedResume {
    /// Original query.
    pub selection: RetainedSelection,
    /// Retained job handle.
    pub handle: Vec<u8>,
    /// Last saved step.
    pub sequence: u64,
    /// Input work allowance.
    pub work: u32,
    /// Output byte allowance.
    pub bytes: u32,
    /// Maximum results per page.
    pub page_size: u32,
    /// Previous result cursor.
    pub after: Option<Vec<u8>>,
}

/// Display columns, rows and continuation shared by ordinary and retained results.
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DisplayPage {
    /// Display column names.
    pub columns: Vec<String>,
    /// Display cells in column order.
    pub rows: Vec<Vec<String>>,
    /// Encoded continuation for a paged read.
    pub next: Option<Vec<u8>>,
}

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
    Complete(DisplayPage),
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

/// Render bytes using two lowercase hexadecimal digits per byte and the supplied display prefix.
#[must_use]
pub fn lowercase_hex(bytes: &[u8], prefix: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(prefix.len() + bytes.len() * 2);
    output.push_str(prefix);
    for &byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    output
}
