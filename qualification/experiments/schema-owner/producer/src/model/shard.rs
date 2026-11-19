///
/// Entry
/// Shard entry record.
///

#[ic_db::entity(
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
pub struct Entry {}
