//! Explicit host-owned issuance state, independent of schema-managed fields.

use crate::{open, runtime, schema::KeyRow};
use ic_db::{Batch, Timestamp, Ulid, UlidGenerationError, UlidGenerator};
use ic_platform_runtime::cdk;
use std::cell::RefCell;

thread_local! {
    static ISSUER: RefCell<UlidGenerator> = const { RefCell::new(UlidGenerator::new()) };
}

#[cdk::update(
    guard = "crate::transport::admit::<77>",
    decode_with = "crate::transport::decode::<_, 77>",
    encode_with = "crate::transport::encode::<_, 77>"
)]
fn issue_key((millis, randomness, value, mode): (i64, [u8; 10], u64, u8)) -> Result<[u8; 16], u8> {
    let id = ISSUER
        .with(|issuer| {
            issuer
                .borrow_mut()
                .generate(Timestamp::from_millis(millis), randomness)
        })
        .map_err(|error| match error {
            UlidGenerationError::InvalidTimestamp => 1_u8,
            UlidGenerationError::Exhausted => 2,
        })?;
    if mode == 2 {
        return Err(3); // Expected application refusal consumes issuance but writes no row.
    }
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    batch
        .insert(&KeyRow {
            id,
            owner: None,
            value,
        })
        .map_err(|_| 4_u8)?;
    db.commit(batch).map_err(|_| 5_u8)?;
    if mode == 1 {
        cdk::api::trap("after generated-key commit");
    }
    Ok(id.to_bytes())
}

#[cdk::query(
    guard = "crate::transport::admit::<78>",
    decode_with = "crate::transport::decode::<_, 78>",
    encode_with = "crate::transport::encode::<_, 78>"
)]
fn last_issued_key() -> Option<[u8; 16]> {
    ISSUER.with(|issuer| issuer.borrow().last().map(Ulid::to_bytes))
}

#[cdk::update(
    guard = "crate::transport::admit::<79>",
    decode_with = "crate::transport::decode::<_, 79>",
    encode_with = "crate::transport::encode::<_, 79>"
)]
fn restore_issuer(last: [u8; 16]) -> bool {
    ISSUER
        .with(|issuer| *issuer.borrow_mut() = UlidGenerator::resume_after(Ulid::from_bytes(last)));
    true
}
