//! Cross-runtime reopen: the predecessor writes real identity records; the new
//! Platform cohort must preserve bytes, decoding, uniqueness and message rollback.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{ByteBuf, Direction, Encoding};
use serde::{Serialize, de::DeserializeOwned};

fn encode<T: Serialize>(method: u64, value: &T) -> Vec<u8> {
    ic_wire::encode(
        value,
        Encoding::Frame {
            method,
            direction: Direction::Request,
        },
        LIMITS,
    )
    .unwrap()
}

fn decode<T: DeserializeOwned>(method: u64, bytes: &[u8]) -> T {
    ic_wire::decode(
        bytes,
        Encoding::Frame {
            method,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap()
}

fn wasm(variable: &str) -> Vec<u8> {
    std::fs::read(std::env::var_os(variable).expect("select both old/new Wasm explicitly")).unwrap()
}

#[tokio::test]
async fn platform_upgrade_preserves_owned_identity_bytes_and_atomicity() {
    let old = wasm("IC_DB_PREVIOUS_PLATFORM_WASM");
    let new = wasm("IC_DB_M1_RECORDS_WASM");
    assert_ne!(old, new, "this is a cross-artifact upgrade probe");
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, old, vec![], None).await;
    let identities = (ByteBuf::from(vec![]), Some(ByteBuf::from(vec![91; 29])));
    let reply = pic
        .update_call(
            canister,
            canister,
            "replace_principal_record",
            encode(19, &(identities.clone(), false)),
        )
        .await
        .unwrap();
    assert_eq!(decode::<Result<(), u8>>(19, &reply), Ok(()));
    type Account = (Vec<u8>, Option<[u8; 32]>);
    type Row = (u64, Option<Account>, [u8; 32], Account);
    let rows: Vec<Row> = vec![
        (1, Some((vec![], None)), [7; 32], (vec![], None)),
        (2, None, [8; 32], (vec![91; 29], Some([2; 32]))),
    ];
    let reply = pic
        .update_call(
            canister,
            canister,
            "account_batch",
            encode(49, &(rows, false)),
        )
        .await
        .unwrap();
    assert_eq!(decode::<Result<(), u8>>(49, &reply), Ok(()));
    let principal = pic
        .query_call(canister, canister, "principal_record", encode(18, &()))
        .await
        .unwrap();
    assert_eq!(
        decode::<((ByteBuf, Option<ByteBuf>), u64)>(18, &principal).0,
        identities
    );
    let accounts = pic
        .query_call(canister, canister, "account_rows", encode(50, &false))
        .await
        .unwrap();
    let backing = pic.get_stable_memory(canister).await;
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, new, vec![], None)
        .await
        .unwrap();
    assert!(
        backing == pic.get_stable_memory(canister).await,
        "upgrade preserves all backing bytes"
    );
    assert_eq!(
        principal,
        pic.query_call(canister, canister, "principal_record", encode(18, &()))
            .await
            .unwrap()
    );
    assert_eq!(
        accounts,
        pic.query_call(canister, canister, "account_rows", encode(50, &false))
            .await
            .unwrap()
    );
    let changed = (ByteBuf::from(vec![4]), None::<ByteBuf>);
    let error = pic
        .update_call(
            canister,
            canister,
            "replace_principal_record",
            encode(19, &(changed.clone(), true)),
        )
        .await
        .unwrap_err();
    assert!(
        error
            .reject_message
            .contains("M1 principal application trap")
    );
    assert!(backing == pic.get_stable_memory(canister).await);
    // Absent and explicit-zero subaccounts retain one unique identity.
    let duplicate: Vec<Row> = vec![(
        3,
        Some((vec![], Some([0; 32]))),
        [9; 32],
        (vec![], Some([0; 32])),
    )];
    let reply = pic
        .update_call(
            canister,
            canister,
            "account_batch",
            encode(49, &(duplicate, false)),
        )
        .await
        .unwrap();
    assert!(decode::<Result<(), u8>>(49, &reply).is_err());
    assert!(backing == pic.get_stable_memory(canister).await);
    let reply = pic
        .update_call(
            canister,
            canister,
            "replace_principal_record",
            encode(19, &(changed.clone(), false)),
        )
        .await
        .unwrap();
    assert_eq!(decode::<Result<(), u8>>(19, &reply), Ok(()));
    let reply = pic
        .query_call(canister, canister, "principal_record", encode(18, &()))
        .await
        .unwrap();
    assert_eq!(
        decode::<((ByteBuf, Option<ByteBuf>), u64)>(18, &reply).0,
        changed
    );
    owned.drop().await;
}
