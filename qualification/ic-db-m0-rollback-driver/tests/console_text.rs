//! Text aggregate thresholds use the same typed semantics in both console modes.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

type Page = (
    Vec<String>,
    Vec<Vec<String>>,
    Option<Vec<u8>>,
    u32,
    u32,
    u32,
);
type Progress = (
    u64,
    String,
    u64,
    u64,
    u64,
    (Vec<String>, Vec<Vec<String>>, Option<Vec<u8>>),
);

#[tokio::test]
async fn text_bounds_preserve_exact_values_and_retained_progress() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-cli")).unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;

    let utf8 = "é".repeat(32);
    let too_long = "é".repeat(33);
    // One group isolates exact text admission/order from numeric parsing and absence.
    for (note, lower, upper, expected, valid) in [
        (None, None, None, Some("—"), true),
        (None, Some(""), None, None, true),
        (Some(""), Some(""), Some(""), Some(""), true),
        (Some("10"), Some("2"), None, None, true),
        (Some("2"), None, Some("10"), None, true),
        (Some("None"), Some("None"), Some("None"), Some("None"), true),
        (
            Some(utf8.as_str()),
            Some(utf8.as_str()),
            Some(utf8.as_str()),
            Some(utf8.as_str()),
            true,
        ),
        (Some("a"), Some("z"), Some("a"), None, true),
        (Some("a"), Some(too_long.as_str()), None, None, false),
        (Some("a"), Some("z"), Some(too_long.as_str()), None, false),
    ] {
        let row = ("Document", note, false, Some(0_u64), -10_i64, None::<i64>);
        let reply = pic
            .update_call(
                canister,
                canister,
                "replace_document",
                ic_wire::encode(
                    &row,
                    Encoding::Frame {
                        method: 8,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            ic_wire::decode::<Result<(), u8>>(
                &reply,
                Encoding::Frame {
                    method: 8,
                    direction: Direction::Response
                },
                LIMITS
            )
            .unwrap(),
            Ok(())
        );
        let expected_rows: Vec<Vec<String>> = expected
            .into_iter()
            .map(|text| vec!["1".into(), text.into()])
            .collect();
        for select in ["entry_id,min:note", "entry_id,max:note"] {
            let request = (
                "Document",
                None::<Vec<u8>>,
                3_u32,
                None::<u64>,
                None::<u64>,
                None::<u64>,
                Vec::<String>::new(),
                false,
                128_u32,
                4096_u32,
                select,
                (false, (lower, upper), Some("key")),
            );
            let before = pic.get_stable_memory(canister).await;
            let reply = pic
                .query_call(
                    canister,
                    canister,
                    "query",
                    ic_wire::encode(
                        &request,
                        Encoding::Frame {
                            method: 3,
                            direction: Direction::Request,
                        },
                        LIMITS,
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            let page = ic_wire::decode::<Result<Page, String>>(
                &reply,
                Encoding::Frame {
                    method: 3,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap();
            assert_eq!(page.is_ok(), valid, "{select}: {lower:?}..{upper:?}");
            if valid {
                assert_eq!(page.unwrap().1, expected_rows);
            }
            assert_eq!(pic.get_stable_memory(canister).await, before);

            let selection = (
                "Document",
                select,
                (None::<u64>, None::<u64>, None::<u64>),
                3_u32,
                (lower, upper),
                Some("key"),
            );
            let reply = pic
                .update_call(
                    canister,
                    canister,
                    "aggregate_start",
                    ic_wire::encode(
                        &selection,
                        Encoding::Frame {
                            method: 120,
                            direction: Direction::Request,
                        },
                        LIMITS,
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            let handle = ic_wire::decode::<Result<Vec<u8>, String>>(
                &reply,
                Encoding::Frame {
                    method: 120,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap();
            assert_eq!(handle.is_ok(), valid);
            if !valid {
                assert_eq!(pic.get_stable_memory(canister).await, before);
                continue;
            }
            let handle = handle.unwrap();
            let mut sequence = 0_u64;
            let mut complete = false;
            for _ in 0..4 {
                let request = (
                    &selection,
                    &handle,
                    sequence,
                    1_u32,
                    4096_u32,
                    1_u32,
                    None::<Vec<u8>>,
                );
                let reply = pic
                    .update_call(
                        canister,
                        canister,
                        "aggregate_resume",
                        ic_wire::encode(
                            &request,
                            Encoding::Frame {
                                method: 121,
                                direction: Direction::Request,
                            },
                            LIMITS,
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap();
                let step = ic_wire::decode::<Result<Progress, String>>(
                    &reply,
                    Encoding::Frame {
                        method: 121,
                        direction: Direction::Response,
                    },
                    LIMITS,
                )
                .unwrap()
                .unwrap();
                sequence = step.0;
                if step.1 == "Complete" {
                    assert_eq!(step.5.1, expected_rows);
                    assert!(step.5.2.is_none());
                    complete = true;
                    break;
                }
                assert_eq!(step.1, "Running");
                assert!(step.5.1.is_empty());
                if note == Some(utf8.as_str()) {
                    let mut changed = selection;
                    changed.4.0 = Some("different");
                    let request = (
                        &changed,
                        &handle,
                        sequence,
                        1_u32,
                        4096_u32,
                        1_u32,
                        None::<Vec<u8>>,
                    );
                    let checkpoint = pic.get_stable_memory(canister).await;
                    let reply = pic
                        .update_call(
                            canister,
                            canister,
                            "aggregate_resume",
                            ic_wire::encode(
                                &request,
                                Encoding::Frame {
                                    method: 121,
                                    direction: Direction::Request,
                                },
                                LIMITS,
                            )
                            .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert!(
                        ic_wire::decode::<Result<Progress, String>>(
                            &reply,
                            Encoding::Frame {
                                method: 121,
                                direction: Direction::Response
                            },
                            LIMITS
                        )
                        .unwrap()
                        .is_err()
                    );
                    assert_eq!(pic.get_stable_memory(canister).await, checkpoint);
                    pic.upgrade_canister(canister, wasm.clone(), vec![], None)
                        .await
                        .unwrap();
                }
            }
            assert!(complete, "bounded input must finish");
            let reply = pic
                .update_call(
                    canister,
                    canister,
                    "aggregate_cancel",
                    ic_wire::encode(
                        &handle,
                        Encoding::Frame {
                            method: 122,
                            direction: Direction::Request,
                        },
                        LIMITS,
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                ic_wire::decode::<Result<(), String>>(
                    &reply,
                    Encoding::Frame {
                        method: 122,
                        direction: Direction::Response
                    },
                    LIMITS
                )
                .unwrap(),
                Ok(())
            );
        }
    }
}
