//! Consumer with a renamed Cargo dependency and an ordinary Rust module alias.

use accounts as directory;
use ic_db::CompiledCanister;
use ic_db_qualification_transport as transport;

// schema_size
// Returns the number of entities in the selected directory schema.
// guard  : validates the request frame for method 1.
// limits : 128 bytes request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "schema_size",
    guard = crate::transport::admit::<1>,
    decode = crate::transport::decode::<_, 1>,
    encode = crate::transport::encode::<_, 1>,
    max_bytes = 128,
    on_error = crate::transport::endpoint_error
)]
fn schema_size(hub: bool) -> u64 {
    let stores = if hub {
        directory::model::Hub::STORES
    } else {
        directory::model::Shard::STORES
    };
    stores
        .iter()
        .map(|store| (store.schema)().entities().len() as u64)
        .sum()
}
