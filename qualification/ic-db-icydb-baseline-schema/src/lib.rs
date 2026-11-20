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
