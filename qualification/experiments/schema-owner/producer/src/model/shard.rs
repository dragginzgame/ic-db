use ic_db::design::prelude::*;

///
/// Entry
/// Shard entry record.
///

#[entity(
    store = crate::model::Accounts,
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
pub struct Entry {}
