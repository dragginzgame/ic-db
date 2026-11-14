#![forbid(unsafe_code)]
//! Experimental IC database: compiled schemas and synchronous bounded records.
//!
//! This M1 slice supports 8/16/32/64/128-bit integers, Bool, Principal and bounded Text/Blob, with optional
//! values. Named records and unit/payload enums support bounded recursion via explicit
//! indirection and ordered lists; traversals share byte, depth, structured-value
//! and total collection-item limits. Named sets/maps use strict scalar key order.
//! Secondary indexes admit integers, Principal, Bool and Text, including optional values; primary keys
//! require non-optional Nat64 fields. References admit required or optional Nat64;
//! absence has no target, while every present target must survive the batch.
//! Atomicity relies on the enclosing IC message: after persistent mutation starts,
//! supplier failures trap. A later application trap also rolls back the batch.
//! Native callers must discard their runtime after a mutation panic. No SQL,
//! endpoint exports, automatic migration or production compatibility is promised.

#[doc(hidden)]
mod codec;
mod key;
mod schema;
mod store;
mod types;

#[cfg(all(feature = "build", not(target_arch = "wasm32")))]
pub mod build;

/// Emit constructor registration only in explicitly selected host schema builds.
#[doc(hidden)]
#[cfg(all(feature = "build", not(target_arch = "wasm32")))]
#[macro_export]
macro_rules! __register_schema {
    ($($item:item)*) => { $($item)* };
}

#[doc(hidden)]
#[cfg(not(all(feature = "build", not(target_arch = "wasm32"))))]
#[macro_export]
macro_rules! __register_schema {
    ($($item:item)*) => {};
}

/// Exact unsigned 256-bit integer. IC DB owns its durable and index encodings.
pub use ethnum::U256;
pub use ic_db_macros::{entity, enum_, map, record, set};
pub use ic_platform_types::Principal;
pub use key::IndexValue;
/// Owned binary bytes. The database codec enforces authored limits and row bounds.
pub type Blob = Vec<u8>;
pub use schema::{EntitySchema, FieldSchema, FieldType, IndexSchema, ReferenceSchema, Schema};
pub use store::{Batch, Database, Page};

/// Failure before commit begins, or a refusal to admit stored data.
#[derive(Debug)]
pub enum Error {
    /// Invalid or unsupported compiled schema.
    InvalidSchema,
    /// An index operand has a different exact type from its declared field.
    InvalidOperand,
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

pub use types::{TypeBody, TypeLink, TypeSchema, Value, ValueSchema, VariantSchema};

/// Codec boundary emitted by `#[entity]`; hand-written implementations are unsupported.
/// Typed references use Rust resolution; an unresolved name fails compilation:
/// ```compile_fail
/// use ic_db::entity;
/// #[entity(pk(field="id"), index(field="target"), fields(
///     field(name="id",value(item(prim="Nat64"))),
///     field(name="target",value(item(prim="Nat64")),references(entity="Missing"))))]
/// struct Example {}
/// ```
pub trait Entity: Sized {
    /// Generated handle, also usable in cyclic typed references; not authored.
    #[doc(hidden)]
    const ID: u32;
    /// Logical entity name under the stored contract.
    const NAME: &'static str;
    /// Rust-resolved declaration path for host schema assembly, not durable identity.
    const PATH: &'static str;
    /// This executable's compiled entity contract.
    const SCHEMA: &'static EntitySchema;
    /// Read the application's primary key without encoding or inspecting indexes.
    fn key(&self) -> u64;
    /// Extract keys from an already typed value for the shared mutation/index paths.
    #[doc(hidden)]
    fn __row_info(&self) -> __private::RowInfo;
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
    pub use crate::codec::{Decoder, Encoder, MAX_ROW_BYTES};
    pub use crate::key::encode_index;
    pub use crate::schema::handle;
    /// Primary/index keys extracted at the small typed codec boundary.
    pub struct RowInfo {
        pub key: u64,
        /// None means the row is outside an explicitly present-only unique index.
        pub indexes: Vec<Option<Vec<u8>>>,
    }
}
