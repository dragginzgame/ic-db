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
//! use ic_db::design::prelude::*;
//! #[enum_(variant(name="Empty"), variant(name="Data", value(item(prim="Nat8"))))]
//! struct State {}
//! #[entity(store = ic_db::testing::FixtureStore, pk(field="id"), index(field="state"), fields(
//!     field(name="id", value(item(prim="Nat64"))),
//!     field(name="state", value(opt, item(is="State")))
//! ))]
//! struct Row {}
//! ```
//! Payload enums also lack the generated ordering required by sets/maps:
//! ```compile_fail
//! use ic_db::design::prelude::*;
//! #[enum_(variant(name="Empty"), variant(name="Data", value(item(prim="Nat8"))))]
//! struct State {}
//! #[set(item(is="State"), max_items=3)]
//! struct States {}
//! ```

#[cfg(feature = "logging")]
pub mod logging;
#[cfg(feature = "metrics")]
pub mod metrics;

// Wrap complete operation results, including early admission refusals. With
// metrics disabled the original body executes directly.
#[cfg(feature = "metrics")]
macro_rules! observe_metrics {
    ($kind:ident, $body:block) => {
        crate::metrics::$kind(|| $body)
    };
}
#[cfg(not(feature = "metrics"))]
macro_rules! observe_metrics {
    ($kind:ident, $body:block) => {
        $body
    };
}

// Both observers share the public terminal boundary. Early returns and `?`
// inside the operation must reach enabled observers exactly once.
macro_rules! observe {
    ($kind:ident, $body:block) => {{
        #[cfg(feature = "logging")]
        {
            crate::logging::observe(concat!("ic-db.", stringify!($kind)), || {
                observe_metrics!($kind, $body)
            })
        }
        #[cfg(not(feature = "logging"))]
        {
            observe_metrics!($kind, $body)
        }
    }};
}

mod canister_session;
mod clock;
#[doc(hidden)]
mod codec;
mod date;
mod decimal;
mod defaults;
mod float;
mod key;
mod managed;
mod member;
mod primary;
mod schema;
mod session;
mod store;
mod store_key;
mod time;
mod types;
mod u256;
mod ulid;
#[cfg(feature = "wire")]
mod wire;

#[cfg(all(feature = "build", not(target_arch = "wasm32")))]
mod build;

#[cfg(all(feature = "test-support", not(target_arch = "wasm32")))]
pub mod testing;

pub use canister_session::{CanisterSession, MountedStore, StoreBatch};
pub use date::Date;
pub use decimal::{Decimal, ParseDecimalError};
pub use defaults::{InsertInput, LiteralDefault};
pub use float::{Float32, Float64};
pub use ic_db_macros::{canister, entity, enum_, list, map, newtype, record, set, store};
pub use ic_platform_types::{Account, Principal, Subaccount};
pub use key::IndexValue;
pub use managed::ManagedTimestamp;
pub use primary::{Id, PrimaryKey};
pub use schema::{EntitySchema, FieldSchema, FieldType, IndexSchema, ReferenceSchema, Schema};
pub use session::{DatabaseSession, QuerySession, UniqueKey};
pub use store::{
    Aggregate, Batch, Column, Comparison, CopyStatus, CopyStep, GroupCount, GroupCursor,
    GroupOrder, GroupPage, GroupSummaries, GroupSummary, GroupSummaryPage, GroupValue, JobHandle,
    JobOutcome, JobStep, Page, QueryCursor, QueryOperand, QueryPage, SchemaTransition,
    SelectColumns, SelectedQuery, Selection, SortDirection, SummaryMeasure,
};
pub use store_key::MAX_INDEX_KEY_BYTES;
pub use time::{Duration, TimeParseError, Timestamp};
pub use types::{TypeBody, TypeLink, TypeSchema, Value, ValueSchema, VariantSchema};
pub use u256::U256;
pub use ulid::{
    SeededUlidGenerator, Ulid, UlidEntropyError, UlidGenerationError, UlidGenerator, UlidParseError,
};

#[doc(hidden)]
#[cfg(any(not(feature = "build"), target_arch = "wasm32"))]
#[macro_export]
macro_rules! __register_schema {
    ($($declaration:tt)*) => {};
}

#[doc(hidden)]
#[cfg(not(feature = "wire"))]
#[macro_export]
macro_rules! __wire_as {
    ($value:ty, $layout:ty) => {};
}

/// Include the schema assembly generated by this package’s build script.
/// Invoke once at the crate root alongside the declarations compiled by
/// `ic_db::build_schema!`. Consumers import the resulting types from that crate.
#[macro_export]
macro_rules! include_schema {
    () => {
        ::core::include!(::core::concat!(::core::env!("OUT_DIR"), "/schema.rs"));
    };
}

/// Logical database owner. Deployment and memory grants belong to the host.
pub trait Canister {
    #[doc(hidden)]
    const DECLARATION: &'static str;
}

/// Logical store and the stable allocation it uses within its canister.
pub trait Store {
    /// Canister declaring this allocation.
    type Canister: Canister;
    /// IC Storage stable allocation key.
    const KEY: &'static str;
    #[doc(hidden)]
    const DECLARATION: &'static str;
}

/// Host-facing assembly generated in the schema owner's library.
pub trait CompiledCanister: Canister {
    /// Stores owned by this canister, in stable key order.
    const STORES: &'static [CompiledStore];
}

/// Validated schema and its stable allocation key.
pub struct CompiledStore {
    /// Key used to create and reopen the allocation.
    pub key: &'static str,
    /// Obtain the admitted schema for this allocation.
    pub schema: fn() -> Schema,
}

/// Owned binary bytes. The database codec enforces authored limits and row bounds.
pub type Blob = Vec<u8>;

/// Admitted database owning an exclusive lease on a named IC Storage allocation.
pub type Database = store::Database<ic_storage::DefaultMemoryImpl>;
/// Fluent query over an entity in the admitted database.
pub type Query<'a, E> = store::Query<'a, E, ic_storage::DefaultMemoryImpl>;
/// Exclusive handle to a frozen source during maintenance.
pub type Maintenance = store::Maintenance<ic_storage::DefaultMemoryImpl>;
/// Bounded retained aggregates in their host-governed allocation.
pub type AggregateJobs = store::AggregateJobs<ic_storage::DefaultMemoryImpl>;
/// In-progress transformation into an identified target allocation.
pub type SchemaCopy = store::SchemaCopy<ic_storage::DefaultMemoryImpl>;
/// Retired source allocation held for bounded cleanup.
pub type RetiredSource = store::RetiredSource<ic_storage::DefaultMemoryImpl>;

/// Implementation methods for the concrete database handles. Installed Wasm
/// fault probes enable `test-support` to specialize these handles for their backing.
#[cfg(any(doc, feature = "test-support"))]
pub mod qualification {
    pub use crate::store::{
        AggregateJobs, Database, Maintenance, Query, RetiredSource, SchemaCopy,
    };
}

/// Failure before commit begins, or a refusal to admit stored data.
#[derive(Debug)]
pub enum Error {
    /// A session operation conflicts with an active borrow of the attached database.
    BorrowConflict,
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
    /// A query requiring at most one matching entity found a second.
    MultipleMatches,
    /// Malformed continuation or a different database/schema/query boundary.
    InvalidCursor,
    /// One matching encoded row exceeds the whole requested output-byte allowance.
    RowTooLarge,
    /// Materializing an entity with managed timestamps requires a write batch.
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
    /// #[canister]
    /// struct App {}
    ///
    /// #[store(canister = App, key = "app.events.v1")]
    /// struct Events {}
    ///
    /// #[entity(store = Events, pk(field = "id"), fields(
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
        pub use crate::{Canister, CompiledCanister, Entity, Schema, Store, Value};
        pub use crate::{canister, entity, enum_, list, map, newtype, record, set, store};
    }
}

/// Codec boundary emitted by `#[entity]`; hand-written implementations are unsupported.
/// Typed references use Rust resolution; an unresolved name fails compilation:
/// ```compile_fail
/// use ic_db::design::prelude::*;
/// #[entity(store = ic_db::testing::FixtureStore, pk(field="id"), index(field="target"), fields(
///     field(name="id",value(item(prim="Nat64"))),
///     field(name="target",value(item(prim="Nat64")),references(entity="Missing"))))]
/// struct Example {}
/// ```
/// Lookups require the declared key type; integers cannot stand in for ULIDs:
/// ```compile_fail
/// use ic_db::design::prelude::*;
/// use ic_db::Database;
/// #[entity(store = ic_db::testing::FixtureStore, pk(field="id"), fields(field(name="id",value(item(prim="Ulid")))))]
/// struct Asset {}
/// fn wrong_key(db: &Database) { let _ = db.get::<Asset>(7_u64); }
/// ```
/// Integer key widths are exact, without implicit widening:
/// ```compile_fail
/// use ic_db::design::prelude::*;
/// use ic_db::Database;
/// #[entity(store = ic_db::testing::FixtureStore, pk(field="id"), fields(field(name="id",value(item(prim="Nat8")))))]
/// struct Small {}
/// fn wrong_width(db: &Database) { let _ = db.get::<Small>(7_u64); }
/// ```
/// Field names come from the declaration. A missing `name` field cannot
/// resolve to the entity's logical name, even when `Entity` is in scope.
/// ```compile_fail
/// use ic_db::design::prelude::*;
/// #[entity(store = ic_db::testing::FixtureStore, pk(field="id"), fields(field(name="id", value(item(prim="Nat64")))))]
/// struct Entry {}
/// let _ = Entry::NAME;
/// ```
/// ```compile_fail
/// use ic_db::design::prelude::*;
/// #[entity(store = ic_db::testing::FixtureStore, pk(field="id"), fields(field(name="id", value(item(prim="Nat64")))))]
/// struct Entry {}
/// let _ = Entry::ENTITY_NAME;
/// ```
pub trait Entity: Sized {
    /// Store selected by the schema author.
    type Store: Store;
    /// Exact supplied primary-key type, selected by the declared field.
    type Key: PrimaryKey + Into<Id<Self>>;
    /// Rust-resolved declaration path for host schema assembly.
    fn declaration_path() -> &'static str;
    /// This executable's compiled entity contract.
    const SCHEMA: &'static EntitySchema;
    /// Read the application's primary key without encoding or inspecting indexes.
    fn key(&self) -> Self::Key;

    /// Return this row’s entity-typed primary key.
    fn id(&self) -> Id<Self> {
        Id::from_key(self.key())
    }

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
    #[cfg(all(feature = "build", not(target_arch = "wasm32")))]
    pub use crate::build::{__build_schema, __canister, __ctor, __entity, __store};
    #[cfg(not(feature = "wire"))]
    pub use ic_db_macros::__StorageOnly as Wire;
    #[cfg(feature = "wire")]
    pub use ic_wire::{self, typescript::Wire};
    pub use serde;
    /// Independent of the descriptor so references can form cycles.
    pub trait ReferenceTarget {
        /// Schema-authored logical name of the target entity.
        const NAME: &'static str;
    }
    pub use crate::codec::{Decoder, Encoder, MAX_ROW_BYTES};
    pub use crate::key::{MemberOperand, MemberPresence, encode_index};
    pub use crate::managed::validate_order as validate_managed_order;
    pub use crate::primary::RowKey;
    pub use crate::schema::{check_schema_member, handle};
    pub use crate::ulid::insert_ulid;
    /// Primary/index keys extracted at the small typed codec boundary.
    pub struct RowInfo {
        pub list_links: Vec<(u32, RowKey)>,
        pub key: RowKey,
        /// None means the row is outside an explicitly present-only unique index.
        pub indexes: Vec<Option<Vec<u8>>>,
    }
}

/// Join two generated field-name constants for a scalar record member.
/// For example, `ic_db::field_path!(Product::PRICE, Price::AMOUNT)` is a
/// static field name accepted by scalar literal filters and row ordering.
#[macro_export]
macro_rules! field_path {
    ($record:path, $member:path $(,)?) => {{
        const BYTES: [u8; $record.len() + 1 + $member.len()] = {
            let mut bytes = [0; $record.len() + 1 + $member.len()];
            let (parent, tail) = bytes.split_at_mut($record.len());
            parent.copy_from_slice($record.as_bytes());
            tail[0] = b'.';
            tail.split_at_mut(1).1.copy_from_slice($member.as_bytes());
            bytes
        };
        const {
            match ::core::str::from_utf8(&BYTES) {
                Ok(path) => path,
                Err(_) => panic!("invalid field name"),
            }
        }
    }};
}
