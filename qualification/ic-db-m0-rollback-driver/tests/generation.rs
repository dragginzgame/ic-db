//! Actual IC issuance consumption, message rollback and explicit upgrade restoration.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

type Row = ([u8; 16], Option<u64>, u64);
type Page = (Vec<Row>, Option<[u8; 16]>, u64);

#[tokio::test]
async fn issued_keys_follow_message_boundaries_and_explicit_restore() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records-wasm"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let issue = async |time: i64, value: u64, mode: u8| {
        pic.update_call(
            canister,
            canister,
            "issue_key",
            ic_wire::encode(
                &(time, [0_u8; 10], value, mode),
                Encoding::Frame {
                    method: 77,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let last = async || {
        let reply = pic
            .query_call(
                canister,
                canister,
                "last_issued_key",
                ic_wire::encode(
                    &(),
                    Encoding::Frame {
                        method: 78,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Option<[u8; 16]>>(
            &reply,
            Encoding::Frame {
                method: 78,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let result = |bytes: Vec<u8>| {
        ic_wire::decode::<Result<[u8; 16], u8>>(
            &bytes,
            Encoding::Frame {
                method: 77,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    assert_eq!(last().await, None);
    let first = result(issue(1000, 1, 0).await.unwrap()).unwrap();
    assert_eq!(u128::from_be_bytes(first), 1000_u128 << 80);
    assert_eq!(last().await, Some(first));
    let before = pic.get_stable_memory(canister).await;
    let error = issue(1000, 2, 1).await.unwrap_err();
    assert!(
        error.reject_message.contains("after generated-key commit"),
        "{error:?}"
    );
    assert!(pic.get_stable_memory(canister).await == before);
    assert_eq!(last().await, Some(first));
    let second = result(issue(999, 2, 0).await.unwrap()).unwrap();
    assert_eq!(u128::from_be_bytes(second), u128::from_be_bytes(first) + 1);
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(result(issue(-1, 3, 0).await.unwrap()), Err(1));
    assert_eq!(last().await, Some(second));
    assert_eq!(result(issue(1000, 3, 2).await.unwrap()), Err(3));
    let consumed = last().await.unwrap();
    assert_eq!(
        u128::from_be_bytes(consumed),
        u128::from_be_bytes(second) + 1
    );
    assert!(pic.get_stable_memory(canister).await == before);
    // A real database uniqueness refusal also consumes the already-issued ID.
    assert_eq!(result(issue(1000, 1, 0).await.unwrap()), Err(5));
    let checkpoint = last().await.unwrap();
    assert_eq!(
        u128::from_be_bytes(checkpoint),
        u128::from_be_bytes(consumed) + 1
    );
    assert!(pic.get_stable_memory(canister).await == before);
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(
        last().await,
        None,
        "heap issuance state is not implicitly durable"
    );
    let reply = pic
        .update_call(
            canister,
            canister,
            "restore_issuer",
            ic_wire::encode(
                &checkpoint,
                Encoding::Frame {
                    method: 79,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        ic_wire::decode::<bool>(
            &reply,
            Encoding::Frame {
                method: 79,
                direction: Direction::Response
            },
            LIMITS
        )
        .unwrap()
    );
    let resumed = result(issue(999, 3, 0).await.unwrap()).unwrap();
    assert_eq!(
        u128::from_be_bytes(resumed),
        u128::from_be_bytes(checkpoint) + 1
    );
    // Check the real row/index result independently from issuance state.
    let reply = pic
        .query_call(
            canister,
            canister,
            "key_page",
            ic_wire::encode(
                &(None::<[u8; 16]>, 10_u8),
                Encoding::Frame {
                    method: 71,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let page: Page = ic_wire::decode(
        &reply,
        Encoding::Frame {
            method: 71,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap();
    assert_eq!(
        page.0,
        vec![(first, None, 1), (second, None, 2), (resumed, None, 3)]
    );
    assert_eq!(page.2, 3);
    let reply = pic
        .query_call(
            canister,
            canister,
            "key_lookup",
            ic_wire::encode(
                &(resumed, 3_u64),
                Encoding::Frame {
                    method: 72,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let lookup: (Option<Row>, Vec<Row>) = ic_wire::decode(
        &reply,
        Encoding::Frame {
            method: 72,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap();
    assert_eq!(lookup, (Some((resumed, None, 3)), vec![(resumed, None, 3)]));
    owned.drop().await;
}
