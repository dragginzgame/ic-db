//! Experimental IC database: compiled schemas and synchronous bounded records.
//!
//! This M1 slice supports Nat64/Int64, Nat128/Int128, Bool and bounded Text, with optional
//! values. Primary and index keys currently require non-optional Nat64 fields.
//! Atomicity relies on the enclosing IC message: after persistent mutation starts,
//! supplier failures trap. A later application trap also rolls back the batch.
//! Native callers must discard their runtime after a mutation panic. No SQL,
//! endpoint exports, automatic migration or production compatibility is promised.

mod schema;
mod store;

pub use ic_db_macros::entity;
pub use schema::{EntitySchema, FieldSchema, FieldType, IndexSchema, ReferenceSchema, Schema};
pub use store::{Batch, Database, Page};

/// Failure before commit begins, or a refusal to admit stored data.
#[derive(Debug)]
pub enum Error {
    /// Invalid or unsupported compiled schema.
    InvalidSchema,
    /// The typed entity is not part of this admitted schema.
    UnknownEntity,
    /// The stored manifest belongs to another schema or format.
    IncompatibleSchema,
    /// Missing, malformed or internally inconsistent stored data.
    Corrupt,
    /// Insert requires an absent primary key.
    Exists,
    /// Replacement/deletion requires an existing primary key.
    Missing,
    /// Two batch commands address the same entity and primary key.
    DuplicateTarget,
    /// The intended final state violates a unique index.
    UniqueViolation,
    /// A reference target is absent, or a deleted target still has a dependent.
    ReferenceViolation,
    /// A declared resource bound or revision range is exceeded.
    Limit,
    /// The supplier refused collection creation or opening.
    Storage(ic_storage::adapter::CollectionError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}

/// Codec boundary emitted by `#[entity]`; hand-written implementations are unsupported.
pub trait Entity: Sized {
    /// This executable's compiled entity contract.
    const SCHEMA: &'static EntitySchema;
    /// Validate declared field bounds before encoding; no persistent effects.
    /// # Errors
    /// Refuses values outside the authored field contract.
    fn validate(&self) -> Result<(), Error>;
    /// Encode canonical stored fields within the row envelope.
    /// # Errors
    /// Returns a limit refusal when the row cannot be encoded within its bounds.
    fn encode(&self) -> Result<Vec<u8>, Error>;
    /// Decode an admitted row; this does not open a database or admit a schema.
    /// # Errors
    /// Returns corruption for malformed stored bytes.
    fn decode(bytes: &[u8]) -> Result<Self, Error>;
}

/// Implementation support for generated code; not a second query frontend.
#[doc(hidden)]
pub mod __private {
    pub use ic_wire::{Limits, native};
    /// Primary/index keys extracted at the small typed codec boundary.
    pub struct RowInfo {
        pub key: u64,
        pub indexes: Vec<Vec<u8>>,
    }
    /// First scalar-slice envelope; not a rich-row capacity claim.
    pub const ROW_LIMITS: Limits = Limits {
        bytes: 4096,
        allocation: 65536,
        nodes: 512,
        depth: 16,
    };
}
