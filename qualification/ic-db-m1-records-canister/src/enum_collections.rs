//! Private persistence/rollback witness for enum-keyed collections.
use crate::{open, runtime, schema::EnumCollectionsRow};
use ic_db::{Batch, Entity};

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
            ic_platform_runtime::system::trap("after enum collection replacement");
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
