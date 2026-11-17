//! Console group selection uses the same typed semantics in both execution modes.
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
    let pic = &owned;
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
                (
                    false,
                    (lower, upper),
                    Some("key"),
                    (None::<Vec<u8>>, None::<Vec<u8>>, None::<Vec<u8>>),
                ),
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
                (None::<Vec<u8>>, None::<Vec<u8>>, None::<Vec<u8>>),
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
                    let mut changed = selection.clone();
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
                    ic_db_m0_rollback_driver::upgrade_stopped(
                        pic,
                        canister,
                        wasm.clone(),
                        vec![],
                        None,
                    )
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
    owned.drop().await;
}

#[tokio::test]
async fn descending_group_keys_survive_upgrade_and_page_in_selected_order() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-cli")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;

    // Independent results for the fixture's alternating categories and six amounts.
    for (select, odd, even) in [
        ("category,count", "3", "3"),
        ("category,min:amount", "11", "20"),
        ("category,max:amount", "51", "60"),
        ("category,sum:amount", "93", "120"),
        ("category,avg:amount", "31", "40"),
        ("category,distinct-count:amount", "3", "3"),
    ] {
        let request = (
            "Entry",
            None::<Vec<u8>>,
            2_u32,
            None::<u64>,
            None::<u64>,
            None::<u64>,
            Vec::<String>::new(),
            false,
            128_u32,
            4096_u32,
            select,
            (
                false,
                (None::<String>, None::<String>),
                Some("key-desc"),
                (None::<Vec<u8>>, None::<Vec<u8>>, None::<Vec<u8>>),
            ),
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
        .unwrap()
        .unwrap();
        let expected = vec![
            vec!["1".to_string(), odd.into()],
            vec!["0".to_string(), even.into()],
        ];
        assert_eq!(page.1, expected, "{select}");
        assert!(page.2.is_none());
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }

    let selection = (
        "Entry",
        "category,count",
        (None::<u64>, None::<u64>, None::<u64>),
        2_u32,
        (None::<String>, None::<String>),
        Some("key-desc"),
        (None::<Vec<u8>>, None::<Vec<u8>>, None::<Vec<u8>>),
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
    .unwrap()
    .unwrap();
    let mut sequence = 0_u64;
    let mut cursor = None::<Vec<u8>>;
    let mut rows = Vec::new();
    let mut finished = false;
    for step_number in 0..8 {
        let request = (
            &selection, &handle, sequence, 2_u32, 4096_u32, 1_u32, &cursor,
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
        cursor = step.5.2;
        if step.1 == "Complete" {
            assert_eq!(step.5.1.len(), 1);
            rows.extend(step.5.1);
            if cursor.is_none() {
                finished = true;
                break;
            }
        } else {
            assert_eq!(step.1, "Running");
            assert!(step.5.1.is_empty());
            assert!(cursor.is_none());
        }
        if step_number == 0 || cursor.is_some() {
            // Recover both accumulation and already-selected output after upgrade.
            ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
            let mut changed = selection.clone();
            changed.5 = Some("key");
            let request = (&changed, &handle, sequence, 2_u32, 4096_u32, 1_u32, &cursor);
            let before = pic.get_stable_memory(canister).await;
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
                    LIMITS,
                )
                .unwrap()
                .is_err()
            );
            assert_eq!(pic.get_stable_memory(canister).await, before);
        }
    }
    assert!(finished, "bounded input and completed pages must finish");
    assert_eq!(rows, vec![vec!["1", "3"], vec!["0", "3"]]);
    owned.drop().await;
}

#[tokio::test]
async fn blob_console_preserves_empty_absent_and_refusal_boundaries() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-cli")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let query_with_prefix = async |entity: &str,
                                   select: &str,
                                   lower: Option<Vec<u8>>,
                                   upper: Option<Vec<u8>>,
                                   prefix: Option<Vec<u8>>,
                                   bytes: u32| {
        let request = (
            entity,
            None::<Vec<u8>>,
            32_u32,
            None::<u64>,
            None::<u64>,
            None::<u64>,
            Vec::<String>::new(),
            false,
            128_u32,
            bytes,
            select,
            (
                false,
                (None::<String>, None::<String>),
                None::<String>,
                (lower, upper, prefix),
            ),
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
        assert_eq!(pic.get_stable_memory(canister).await, before);
        ic_wire::decode::<Result<Page, String>>(
            &reply,
            Encoding::Frame {
                method: 3,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let query = async |entity: &str, select: &str, lower, upper, bytes| {
        query_with_prefix(entity, select, lower, upper, None, bytes).await
    };
    for (payload, optional, hex) in [
        (vec![], None, "0x"),
        (vec![0], Some(vec![]), "0x00"),
        (vec![0, 255, 128], Some(vec![255]), "0x00ff80"),
    ] {
        let expected_optional = match &optional {
            None => "—",
            Some(v) if v.is_empty() => "0x",
            _ => "0xff",
        };
        let request = (
            (
                ic_wire::ByteBuf::from(payload.clone()),
                optional.map(ic_wire::ByteBuf::from),
            ),
            false,
        );
        let reply = pic
            .update_call(
                canister,
                canister,
                "replace_binary_object",
                ic_wire::encode(
                    &request,
                    Encoding::Frame {
                        method: 14,
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
                    method: 14,
                    direction: Direction::Response
                },
                LIMITS
            )
            .unwrap(),
            Ok(())
        );
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        for prefix in [
            vec![],
            vec![0],
            vec![0, 255],
            vec![255],
            vec![0, 255, 128, 0],
        ] {
            let page = query_with_prefix(
                "BinaryObject",
                "payload",
                None,
                None,
                Some(prefix.clone()),
                4096,
            )
            .await
            .unwrap();
            let expected = if payload.starts_with(&prefix) {
                vec![vec![hex.to_owned()]]
            } else {
                vec![]
            };
            assert_eq!(page.1, expected);
        }
        for select in ["payload", "min:payload", "max:payload"] {
            let page = query(
                "BinaryObject",
                select,
                Some(payload.clone()),
                Some(payload.clone()),
                4096,
            )
            .await
            .unwrap();
            assert_eq!(page.1, vec![vec![hex]]);
        }
        assert_eq!(
            query("BinaryObject", "optional", None, None, 4096)
                .await
                .unwrap()
                .1,
            vec![vec![expected_optional]]
        );
        assert_eq!(
            query("BinaryObject", "payload,count", None, None, 4096)
                .await
                .unwrap()
                .1,
            vec![vec![hex, "1"]]
        );
        for (select, expected) in [
            ("distinct-count:optional", vec![vec!["1"]]),
            ("optional,count", vec![vec![expected_optional, "1"]]),
            (
                "optional,count:optional",
                vec![vec![
                    expected_optional,
                    if expected_optional == "—" { "0" } else { "1" },
                ]],
            ),
        ] {
            assert_eq!(
                query("BinaryObject", select, None, None, 4096)
                    .await
                    .unwrap()
                    .1,
                expected,
            );
        }
        assert!(
            query("BinaryObject", "payload", None, None, 1)
                .await
                .unwrap_err()
                .contains("exceeds --bytes")
        );
    }
    assert!(
        query_with_prefix(
            "BinaryObject",
            "payload",
            None,
            None,
            Some(vec![0; 1025]),
            4096
        )
        .await
        .is_err()
    );
    assert!(
        query_with_prefix("Entry", "id", None, None, Some(vec![]), 4096)
            .await
            .is_err()
    );
    assert!(
        query("BinaryObject", "payload", Some(vec![1; 1025]), None, 4096)
            .await
            .is_err()
    );
    assert!(
        query("Entry", "id", Some(vec![]), None, 4096)
            .await
            .unwrap_err()
            .contains("apply to BinaryObject")
    );
    assert!(
        query("BinaryObject", "sum:payload", None, None, 4096)
            .await
            .is_err()
    );
    assert!(
        query(
            "BinaryObject",
            "payload",
            Some(vec![255]),
            Some(vec![]),
            4096
        )
        .await
        .unwrap()
        .1
        .is_empty()
    );
    owned.drop().await;
}
