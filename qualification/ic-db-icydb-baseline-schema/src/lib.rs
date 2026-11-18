//! Synthetic persistent schema for the frozen, SQL-disabled IcyDB baseline.

use icydb::model::prelude::*;

/// One benchmark database; independent of any application's schema.
#[canister(memory_namespace = "icdb_baseline")]
pub struct BaselineCanister {}

/// IcyDB's durable journaled storage path, including its real commit machinery.
#[store(canister = "BaselineCanister", storage(journaled(key = "main")))]
pub struct BaselineStore {}

/// Small rows with one compound secondary index and externally supplied keys.
#[entity(
    store = "BaselineStore",
    version = 1,
    pk(field = "id"),
    index(fields = ["category", "id"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat64"))),
        field(name = "amount", value(item(prim = "Nat64")))
    )
)]
pub struct Entry {}

/// Same logical values and index orders as IC DB's maximum-row witness.
/// Kept out of the ordinary scalar baseline's generated schema.
#[cfg(feature = "envelope")]
#[entity(
    store = "BaselineStore",
    version = 1,
    pk(field = "id"),
    index(fields = ["a", "b", "c", "d"]),
    index(fields = ["a", "b", "d", "c"]),
    index(fields = ["a", "c", "b", "d"]),
    index(fields = ["a", "c", "d", "b"]),
    index(fields = ["a", "d", "b", "c"]),
    index(fields = ["a", "d", "c", "b"]),
    index(fields = ["b", "a", "c", "d"]),
    index(fields = ["b", "a", "d", "c"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "a", value(item(prim = "Text", max_len = 1024))),
        field(name = "b", value(item(prim = "Text", max_len = 1024))),
        field(name = "c", value(item(prim = "Text", max_len = 1024))),
        field(name = "d", value(item(prim = "Text", max_len = 1024))),
        field(name = "padding", value(item(prim = "Blob", max_len = 4096)))
    )
)]
pub struct EnvelopeRow {}

/// Isolated equality-lookup witness, excluded from the measured scalar schema.
#[cfg(feature = "unique-lookup")]
#[entity(
    store = "BaselineStore", version = 1, pk(field = "id"),
    index(fields = ["issuer", "subject"], unique),
    fields(field(name = "id", value(item(prim = "Nat64"))),
           field(name = "issuer", value(item(prim = "Principal"))),
           field(name = "subject", value(item(prim = "Principal"))))
)]
pub struct Route {}
