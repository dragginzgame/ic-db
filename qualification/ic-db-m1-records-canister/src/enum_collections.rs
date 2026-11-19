//! Private persistence/rollback witness for enum-keyed collections.

use crate::{
    open, runtime,
    schema::{EnumCollectionsRow, Lifecycle},
};
use ic_db::{Batch, Comparison, Entity};

// enum_collections
// Exercise stored enum collections with the selected operation and bytes.
// guard  : validates the request frame for method 94.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "enum_collections",
    guard = crate::transport::admit::<94>,
    decode = crate::transport::decode::<_, 94>,
    encode = crate::transport::encode::<_, 94>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn enum_collections((mode, bytes): (u8, Vec<u8>)) -> Result<(Vec<u8>, u64), u8> {
    let host = runtime();
    let mut db = open(&host);
    if mode > 4 {
        return Err(1);
    }
    if mode != 0 {
        let mut batch = Batch::new();
        if mode == 4 {
            batch.delete::<EnumCollectionsRow>(1).map_err(|_| 2_u8)?;
        } else {
            let row = EnumCollectionsRow::decode(&bytes).map_err(|_| 1_u8)?;
            if row.id != 1 {
                return Err(1);
            }
            if mode == 1 {
                batch.insert(&row)
            } else {
                batch.replace(&row)
            }
            .map_err(|_| 2_u8)?;
        }
        db.commit(batch).map_err(|_| 2_u8)?;
        if mode == 3 {
            crate::system::trap("after enum collection replacement");
        }
    }

    Ok((
        db.get::<EnumCollectionsRow>(1)
            .unwrap()
            .map(|row| row.encode().unwrap())
            .unwrap_or_default(),
        db.revision::<EnumCollectionsRow>().unwrap(),
    ))
}

// enum_set_membership
// Check the fixture’s three enum set membership cases.
// guard  : validates the request frame for method 150.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "enum_set_membership",
    guard = crate::transport::admit::<150>,
    decode = crate::transport::decode::<_, 150>,
    encode = crate::transport::encode::<_, 150>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn enum_set_membership((): ()) -> [bool; 3] {
    let host = runtime();
    let db = open(&host);

    [Lifecycle::Archived, Lifecycle::Draft, Lifecycle::Ready].map(|state| {
        db.query::<EnumCollectionsRow>()
            .filter("states", Comparison::Contains, &state)
            .exists()
            .unwrap()
            .value
    })
}
