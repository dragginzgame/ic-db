#![forbid(unsafe_code)]
//! Experimental IC database: compiled schemas and synchronous bounded records.
//! Schema modules can import the declaration attributes and value types from
//! [`design::prelude`].
//!
//! This M1 slice supports 8/16/32/64/128-bit integers, U256, Ulid, Date, Timestamp, Duration, Decimal,
//! finite Float32/Float64, Bool, Principal, Account/Subaccount and bounded Text/Blob, with optional
//! values. Named records and unit/payload enums support bounded recursion via explicit
//! indirection and ordered lists; traversals share byte, depth, structured-value
//! and total collection-item limits. Sets/maps use strict scalar/unit-enum key order.
//! Secondary indexes admit integers, Ulid, Date, Timestamp, Duration, Decimal, finite floats,
//! Account/Subaccount, Principal, Bool, Text, Blob and unit enums, including optional values; primary keys
//! require non-optional integer, U256, Ulid, Principal, Account or Subaccount fields. References use
//! the target’s exact key type;
//! absence has no target, while every present target must survive the batch.
//! Atomicity relies on the enclosing IC message: after persistent mutation starts,
//! supplier failures trap. A later application trap also rolls back the batch.
//! Native callers must discard their runtime after a mutation panic. No SQL,
//! endpoint exports, automatic migration or production compatibility is promised.
//!
//! Payload enums cannot become index keys, even when a row selects a unit variant:
//! ```compile_fail
//! use ic_db::{entity, enum_};
//! #[enum_(variant(name="Empty"), variant(name="Data", value(item(prim="Nat8"))))]
//! struct State {}
//! #[entity(pk(field="id"), index(field="state"), fields(
//!     field(name="id", value(item(prim="Nat64"))),
//!     field(name="state", value(opt, item(is="State")))
//! ))]
//! struct Row {}
//! ```
//! Payload enums also lack the generated ordering required by sets/maps:
//! ```compile_fail
//! use ic_db::{enum_, set};
//! #[enum_(variant(name="Empty"), variant(name="Data", value(item(prim="Nat8"))))]
//! struct State {}
//! #[set(item(is="State"), max_items=3)]
//! struct States {}
//! ```

#[doc(hidden)]
mod codec;
mod date;
mod decimal;
mod defaults;
mod float;
mod key;
mod managed;
mod primary;
mod schema;
mod store;
mod store_key;
mod time;
mod types;
mod ulid;

#[cfg(all(feature = "build", not(target_arch = "wasm32")))]
pub mod build;

#[cfg(all(feature = "test-support", not(target_arch = "wasm32")))]
pub mod testing;

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

pub use date::Date;
pub use decimal::{Decimal, ParseDecimalError};
pub use defaults::{InsertInput, LiteralDefault};
/// Exact unsigned 256-bit integer. IC DB owns its durable and index encodings.
pub use ethnum::U256;
pub use float::{Float32, Float64};
pub use ic_db_macros::{entity, enum_, list, map, newtype, record, set};
pub use ic_platform_types::{Account, Principal, Subaccount};
pub use key::IndexValue;
pub use managed::ManagedTimestamp;
pub use primary::PrimaryKey;
pub use time::{Duration, TimeParseError, Timestamp};
pub use ulid::{Ulid, UlidGenerationError, UlidGenerator, UlidParseError};
/// Owned binary bytes. The database codec enforces authored limits and row bounds.
pub type Blob = Vec<u8>;
pub use schema::{EntitySchema, FieldSchema, FieldType, IndexSchema, ReferenceSchema, Schema};
pub use store::{
    Aggregate, AggregateJobs, Batch, Comparison, CopyStep, Database, GroupCount, GroupCursor,
    GroupOrder, GroupPage, GroupValue, JobHandle, JobOutcome, JobStep, Maintenance, Page, Query,
    QueryCursor, QueryOperand, QueryPage, RetiredSource, SchemaCopy, SchemaTransition,
    SortDirection,
};
pub use store_key::MAX_INDEX_KEY_BYTES;

/// Failure before commit begins, or a refusal to admit stored data.
#[derive(Debug)]
pub enum Error {
    /// Ordinary access is closed by an admitted durable maintenance gate.
    Maintenance,
    /// A maintenance identity is invalid or does not match this source/active transition.
    InvalidTransition,
    /// The retained job handle, query, result shape or requested sequence does not match.
    InvalidJob,
    /// All retained job slots are occupied; cancel or clean up expired jobs explicitly.
    JobCapacity,
    /// An arithmetic total exceeds its accumulator or explicitly selected result range.
    ArithmeticOverflow,
    /// Unsupported query field/shape or unavailable ordered access path.
    InvalidQuery,
    /// Malformed continuation or a different database/schema/query boundary.
    InvalidCursor,
    /// One matching encoded row exceeds the whole requested output-byte allowance.
    RowTooLarge,
    /// Managed insert/replacement requires an explicitly captured batch time.
    MissingOperationTime,
    /// A caller attempted to assign a database-managed field.
    ManagedField,
    /// A real change would move managed time behind its stored value.
    TimestampRegression,
    /// An insert input omitted a required field with no declared default.
    MissingField(&'static str),
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
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

pub use types::{TypeBody, TypeLink, TypeSchema, Value, ValueSchema, VariantSchema};

/// Schema authoring for application design modules.
pub mod design {
    /// Declaration attributes, database scalar types and schema assembly.
    ///
    /// ```
    /// use ic_db::design::prelude::*;
    ///
    /// #[newtype(item(prim = "Timestamp"))]
    /// struct StartedAt {}
    ///
    /// #[list(item(is = "StartedAt"), max_items = 3)]
    /// struct StartHistory {}
    ///
    /// #[entity(pk(field = "id"), fields(
    ///     field(name = "id", value(item(prim = "Nat64"))),
    ///     field(name = "history", value(item(is = "StartHistory")))
    /// ))]
    /// struct Event {}
    ///
    /// let event = Event { id: 1, history: StartHistory(vec![StartedAt(Timestamp::from_millis(0))]) };
    /// let schema = Schema::new(&[Event::SCHEMA]).unwrap();
    /// assert_eq!(schema.entities()[0].name, "Event");
    /// assert_eq!(event.history.0.len(), 1);
    /// ```
    pub mod prelude {
        pub use crate::{
            Account, Blob, Date, Decimal, Duration, Float32, Float64, Principal, Subaccount,
            Timestamp, U256, Ulid,
        };
        pub use crate::{Entity, Schema, Value};
        pub use crate::{entity, enum_, list, map, newtype, record, set};
    }
}

/// Codec boundary emitted by `#[entity]`; hand-written implementations are unsupported.
/// Typed references use Rust resolution; an unresolved name fails compilation:
/// ```compile_fail
/// use ic_db::entity;
/// #[entity(pk(field="id"), index(field="target"), fields(
///     field(name="id",value(item(prim="Nat64"))),
///     field(name="target",value(item(prim="Nat64")),references(entity="Missing"))))]
/// struct Example {}
/// ```
/// Lookups require the declared key type; integers cannot stand in for ULIDs:
/// ```compile_fail
/// use ic_db::{entity, Database};
/// #[entity(pk(field="id"), fields(field(name="id",value(item(prim="Ulid")))))]
/// struct Asset {}
/// fn wrong_key(db: &Database) { let _ = db.get::<Asset>(7_u64); }
/// ```
/// Integer key widths are exact, without implicit widening:
/// ```compile_fail
/// use ic_db::{entity, Database};
/// #[entity(pk(field="id"), fields(field(name="id",value(item(prim="Nat8")))))]
/// struct Small {}
/// fn wrong_width(db: &Database) { let _ = db.get::<Small>(7_u64); }
/// ```
pub trait Entity: Sized {
    /// Exact supplied primary-key type, selected by the declared field.
    type Key: PrimaryKey;
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
    fn key(&self) -> Self::Key;

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
    pub use crate::managed::validate_order as validate_managed_order;
    pub use crate::primary::RowKey;
    pub use crate::schema::handle;
    /// Primary/index keys extracted at the small typed codec boundary.
    pub struct RowInfo {
        pub key: RowKey,
        /// None means the row is outside an explicitly present-only unique index.
        pub indexes: Vec<Option<Vec<u8>>>,
    }
}
