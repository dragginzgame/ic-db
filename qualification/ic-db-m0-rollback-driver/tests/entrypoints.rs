//! Standalone installed hooks: raw refusals and reply failures across IC boundaries.

use ic_db_cli::fixture_wire;
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_testkit::pocket_ic::ErrorCode;
use ic_wire::{ByteBuf, Direction, Encoding};

type Entries = Vec<(u64, u64)>;
type Snapshot = (Entries, Entries, Entries, [u64; 2]);

#[tokio::test]
async fn admission_and_decode_refuse_before_dispatch() {
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 2_000_000_000_000).await;
    pic.install_canister(
        canister,
        ic_db_m0_rollback_driver::core_wasm(),
        vec![],
        None,
    )
    .await;
    let before = pic.get_stable_memory(canister).await;
    let valid = fixture_wire::encode(
        &(0_u8, 7_u64, 11_u64),
        Encoding::Frame {
            method: 1,
            direction: Direction::Request,
        },
        LIMITS,
    )
    .unwrap();
    let wrong_id = fixture_wire::encode(
        &(0_u8, 7_u64, 11_u64),
        Encoding::Frame {
            method: 2,
            direction: Direction::Request,
        },
        LIMITS,
    )
    .unwrap();
    let wrong_direction = fixture_wire::encode(
        &(0_u8, 7_u64, 11_u64),
        Encoding::Frame {
            method: 1,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap();
    let mut trailing = valid.clone();
    trailing.push(0);
    let mut bad_version = valid.clone();
    bad_version[0] ^= 0xff;
    let cases = [
        vec![],
        valid[..10].to_vec(), // Truncated 11-byte Wire header.
        wrong_id,
        wrong_direction,
        bad_version,
        valid[..valid.len() - 1].to_vec(),
        trailing,
        vec![0; LIMITS.bytes + 1],
    ];
    for bytes in cases {
        let expected = if bytes.len() > LIMITS.bytes {
            "Wire request exceeds byte allowance".to_owned()
        } else {
            fixture_wire::decode::<(u8, u64, u64)>(
                &bytes,
                Encoding::Frame {
                    method: 1,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap_err()
            .to_string()
        };
        let error = pic
            .update_call(canister, canister, "mutate", bytes)
            .await
            .unwrap_err();
        assert_eq!(error.error_code, ErrorCode::CanisterCalledTrap, "{error:?}");
        assert!(error.reject_message.contains(&expected), "{error:?}");
        assert!(before == pic.get_stable_memory(canister).await);
    }
    // Guard rejection wins even over malformed input: decoding and writes must not run.
    let error = pic
        .update_call(canister, canister, "endpoint_denied", vec![])
        .await
        .unwrap_err();
    assert!(
        error.reject_message.contains("M0 standalone guard refusal"),
        "{error:?}"
    );
    assert!(before == pic.get_stable_memory(canister).await);
    // A no-argument endpoint still validates the framed unit payload.
    let error = pic
        .query_call(canister, canister, "inspect", vec![])
        .await
        .unwrap_err();
    assert_eq!(error.error_code, ErrorCode::CanisterCalledTrap);
    let reply = pic
        .update_call(canister, canister, "mutate", valid)
        .await
        .unwrap();
    assert_eq!(
        fixture_wire::decode::<Result<(), u8>>(
            &reply,
            Encoding::Frame {
                method: 1,
                direction: Direction::Response
            },
            LIMITS
        )
        .unwrap(),
        Ok(())
    );
    let reply = pic
        .query_call(
            canister,
            canister,
            "inspect",
            fixture_wire::encode(
                &(),
                Encoding::Frame {
                    method: 2,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let snapshot: Snapshot = fixture_wire::decode(
        &reply,
        Encoding::Frame {
            method: 2,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap();
    assert_eq!(snapshot.0, vec![(7, 11)]);
    assert_eq!(snapshot.1, vec![(11, 7)]);
    assert_eq!(snapshot.2, vec![(7, 11)]);
    owned.drop().await;
}

#[tokio::test]
async fn reply_failure_rolls_back_only_the_current_execution() {
    let wasm = ic_db_m0_rollback_driver::core_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    for (method, id, asynchronous) in [("endpoint_sync", 8, false), ("endpoint_async", 10, true)] {
        for oversized in [false, true] {
            let canister = pic.create_canister().await;
            pic.add_cycles(canister, 2_000_000_000_000).await;
            pic.install_canister(canister, wasm.clone(), vec![], None)
                .await;
            let before = pic.get_stable_memory(canister).await;
            let result = pic
                .update_call(
                    canister,
                    canister,
                    method,
                    fixture_wire::encode(
                        &oversized,
                        Encoding::Frame {
                            method: id,
                            direction: Direction::Request,
                        },
                        LIMITS,
                    )
                    .unwrap(),
                )
                .await;
            if oversized {
                let expected = fixture_wire::encode(
                    &ByteBuf::from(vec![7; 65_536]),
                    Encoding::Frame {
                        method: id,
                        direction: Direction::Response,
                    },
                    LIMITS,
                )
                .unwrap_err()
                .to_string();
                let error = result.unwrap_err();
                assert_eq!(error.error_code, ErrorCode::CanisterCalledTrap, "{error:?}");
                assert!(error.reject_message.contains(&expected), "{error:?}");
                if !asynchronous {
                    assert!(
                        before == pic.get_stable_memory(canister).await,
                        "rows/indexes/allocator must revert"
                    );
                }
            } else {
                assert_eq!(
                    fixture_wire::decode::<ByteBuf>(
                        &result.unwrap(),
                        Encoding::Frame {
                            method: id,
                            direction: Direction::Response
                        },
                        LIMITS
                    )
                    .unwrap()
                    .as_ref(),
                    &[7]
                );
            }
            let expected = match (oversized, asynchronous) {
                (false, _) => vec![(1, 101), (2, 102)],
                (true, true) => vec![(1, 101)],
                (true, false) => vec![],
            };
            let reply = pic
                .query_call(
                    canister,
                    canister,
                    "inspect",
                    fixture_wire::encode(
                        &(),
                        Encoding::Frame {
                            method: 2,
                            direction: Direction::Request,
                        },
                        LIMITS,
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            let snapshot: Snapshot = fixture_wire::decode(
                &reply,
                Encoding::Frame {
                    method: 2,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap();
            assert_eq!(snapshot.0, expected, "{method}, oversized={oversized}");
            assert_eq!(
                snapshot.1,
                expected
                    .iter()
                    .map(|(id, principal)| (*principal, *id))
                    .collect::<Vec<_>>()
            );
            assert_eq!(snapshot.2, expected);
        }
    }
    owned.drop().await;
}
