//! The directory schema, organised by domain.

/// Account directory.
pub mod hub;

/// Shard records.
pub mod shard;

use ic_db::design::prelude::*;

///
/// Hub
/// Owns the account directory and its audit store.
///

#[canister]
pub struct Hub {}

///
/// Accounts
/// Stores the directory’s user and entry records.
///

#[store(
    canister = Hub,
    key = "probe.account.v1"
)]
pub struct Accounts {}

///
/// AuditStore
/// Stores audit events for the hub.
///

#[store(
    canister = Hub,
    key = "probe.audit.v1"
)]
pub struct AuditStore {}

///
/// Shard
/// Owns the shard’s record store.
///

#[canister]
pub struct Shard {}

///
/// ShardStore
/// Stores records owned by the shard.
///

#[store(
    canister = Shard,
    key = "probe.shard.v1"
)]
pub struct ShardStore {}

///
/// Revision
/// Revision number shared by hub audit events and shard records.
///

#[newtype(item(prim = "Nat64"))]
pub struct Revision {}

///
/// AuditEvent
/// Records the revision associated with a hub audit event.
///

#[entity(
    store = AuditStore,
    pk(field = "id"),
    index(field = "revision", unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "revision", value(item(is = "Revision")))
    )
)]
pub struct AuditEvent {}

///
/// ShardRecord
/// Records the revision associated with a shard record.
///

#[entity(
     store = ShardStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "revision", value(item(is = "Revision")))
    )
)]
pub struct ShardRecord {}
