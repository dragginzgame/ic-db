//! Explicit host-owned issuance state, independent of schema-managed fields.

use crate::{open, runtime, schema::KeyRow};
use ic_db::{Batch, Timestamp, Ulid, UlidGenerationError, UlidGenerator};
use std::cell::RefCell;

thread_local! {
    static ISSUER: RefCell<UlidGenerator> = const { RefCell::new(UlidGenerator::new()) };
}

/// Issue a ULID using the supplied time and randomness.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "issue_key",
    guard = crate::transport::admit::<77>,
    decode = crate::transport::decode::<_, 77>,
    encode = crate::transport::encode::<_, 77>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
async fn issue_key(
    (millis, randomness, value, mode): (i64, [u8; 10], u64, u8),
) -> Result<[u8; 16], u8> {
    let id = match mode {
        3 => Ulid::generate_prand(),
        4 | 5 => {
            let mut stream = if mode == 4 {
                Ulid::generator().await.map_err(|_| 6_u8)?
            } else {
                Ulid::generator_prand()
            };
            let first = stream.next_id();
            let second = stream.next_id();
            assert_eq!(
                u128::from_be_bytes(second.to_bytes()),
                u128::from_be_bytes(first.to_bytes()) + 1
            );
            second
        }
        _ => ISSUER
            .with(|issuer| {
                issuer
                    .borrow_mut()
                    .generate(Timestamp::from_millis(millis), randomness)
            })
            .map_err(|error| match error {
                UlidGenerationError::InvalidTimestamp => 1_u8,
                UlidGenerationError::Exhausted => 2,
            })?,
    };
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
        ic_platform_runtime::system::trap("after generated-key commit");
    }
    Ok(id.to_bytes())
}

/// Read the issuer checkpoint for the next restore.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "last_issued_key",
    guard = crate::transport::admit::<78>,
    decode = crate::transport::decode::<_, 78>,
    encode = crate::transport::encode::<_, 78>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn last_issued_key((): ()) -> Option<[u8; 16]> {
    ISSUER.with(|issuer| issuer.borrow().last().map(Ulid::to_bytes))
}

/// Restore the supplied last-issued ULID for subsequent issuance.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "restore_issuer",
    guard = crate::transport::admit::<79>,
    decode = crate::transport::decode::<_, 79>,
    encode = crate::transport::encode::<_, 79>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn restore_issuer(last: [u8; 16]) -> bool {
    ISSUER
        .with(|issuer| *issuer.borrow_mut() = UlidGenerator::resume_after(Ulid::from_bytes(last)));
    true
}
