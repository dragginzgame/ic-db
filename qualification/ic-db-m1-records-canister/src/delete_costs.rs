//! Measure one atomic delete of documents linked to one entry, optionally deleting that entry.

use crate::{
    open, runtime,
    schema::{Document, Entry},
};
use ic_db::{Batch, Error};

// measured_delete
// Measure batch construction and commit, then inspect both entity counts.
// guard  : validates the request frame for method 247.
// limits : 64 KiB request, 64 KiB reply, at most 256 attempted deletes.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "measured_delete",
    guard = crate::transport::admit::<247>,
    decode = crate::transport::decode_measured::<_, 247>,
    encode = crate::transport::encode::<_, 247>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn measured_delete(
    ((children, owner, descending), counter): crate::transport::Measured<(u16, bool, bool)>,
) -> (bool, u64, u64, u64, u64) {
    assert!(usize::from(children) + usize::from(owner) <= 256);
    let host = runtime();
    let mut db = open(&host);
    let start = counter();
    let mut batch = Batch::new();
    let admitted = (|| {
        if owner {
            batch.delete::<Entry>(1_u64)?;
        }
        for offset in 0..children {
            let id = if descending {
                children - offset
            } else {
                offset + 1
            };
            batch.delete::<Document>(u64::from(id))?;
        }
        Ok(())
    })();
    let prepared = counter();
    match admitted {
        Ok(()) => db.commit(batch).unwrap(),
        Err(Error::Limit) => (),
        Err(other) => panic!("unexpected delete refusal: {other:?}"),
    }
    let committed = counter();
    (
        admitted.is_ok(),
        prepared - start,
        committed - prepared,
        db.cardinality::<Document>().unwrap(),
        db.cardinality::<Entry>().unwrap(),
    )
}
