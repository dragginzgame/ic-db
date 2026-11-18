//! Fluent query pages remain bounded and resume across upgrades in real Wasm.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_db_qualification_transport::RowFilters;
use ic_wire::{Direction, Encoding};

type Page = (Vec<(u64, u64)>, Option<Vec<u8>>, u32, u32, u32, u64);
type Request = (Option<Vec<u8>>, bool, Option<u64>, u64, u32, u32, u32);
type ConsoleRequest<'a> = (
    &'a str,
    Option<Vec<u8>>,
    u32,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Vec<&'a str>,
    bool,
    u32,
    u32,
    &'a str,
    bool,
);
type DisplayPage = (
    Vec<String>,
    Vec<Vec<String>>,
    Option<Vec<u8>>,
    u32,
    u32,
    u32,
);

#[tokio::test]
async fn indexed_text_prefixes_seek_exact_bytes_and_retry_after_upgrade() {
    type IdPage = (Vec<u64>, Option<Vec<u8>>, u32);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let setup = pic
        .update_call(
            canister,
            canister,
            "reference_batch",
            ic_wire::encode(
                &4_u8,
                Encoding::Frame {
                    method: 10,
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
            &setup,
            Encoding::Frame {
                method: 10,
                direction: Direction::Response
            },
            LIMITS
        )
        .unwrap(),
        Ok(())
    );
    let call = async |needle: &str, descending: bool, cursor: Option<Vec<u8>>, bytes: u32| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_text_seek",
                ic_wire::encode(
                    &(needle, descending, cursor, 4_u32, bytes),
                    Encoding::Frame {
                        method: 132,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<IdPage, u8>>(
            &reply,
            Encoding::Frame {
                method: 132,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
        .unwrap()
    };
    for title in ["", "\0x", "a\0b", "\u{10ffff}x", "Dependent\0"] {
        let reply = pic
            .update_call(
                canister,
                canister,
                "replace_document",
                ic_wire::encode(
                    &(
                        title,
                        None::<String>,
                        false,
                        Some(0_u64),
                        -10_i64,
                        None::<i64>,
                    ),
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
        let before = pic.get_stable_memory(canister).await;
        for needle in ["", "D", "Dependent\0", "a\0", "\0", "\u{10ffff}", "missing"] {
            let mut expected: Vec<_> = [(title, 1_u64), ("Dependent first", 2)]
                .into_iter()
                .filter(|(value, _)| value.starts_with(needle))
                .collect();
            expected.sort();
            for descending in [false, true] {
                let page = call(needle, descending, None, 32).await;
                let mut ids: Vec<_> = expected.iter().map(|(_, id)| *id).collect();
                if descending {
                    ids.reverse();
                }
                assert_eq!(page, (ids, None, expected.len() as u32));
            }
        }
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    let before = pic.get_stable_memory(canister).await;
    let ascending = call("Dependent", false, None, 8).await;
    let descending = call("Dependent", true, None, 8).await;
    assert_eq!((&ascending.0, ascending.2), (&vec![1], 2));
    assert_eq!((&descending.0, descending.2), (&vec![2], 2));
    assert_eq!(ascending.1.as_ref().unwrap()[1], 1);
    assert_eq!(descending.1.as_ref().unwrap()[1], 1);
    pic.advance_time(std::time::Duration::from_secs(600)).await;
    pic.tick().await;
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(
        call("Dependent", false, ascending.1, 8).await,
        (vec![2], None, 1)
    );
    assert_eq!(
        call("Dependent", true, descending.1, 8).await,
        (vec![1], None, 1)
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.drop().await;
}

#[tokio::test]
async fn index_ranges_preserve_duplicate_classes_prefixes_and_upgrade_continuation() {
    type IdPage = (Vec<u64>, Option<Vec<u8>>, u32);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |cursor: Option<Vec<u8>>,
                      descending: bool,
                      category: Option<u64>,
                      lower: u64,
                      upper: u64,
                      bytes: u32| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_index_range",
                ic_wire::encode(
                    &(cursor, descending, category, lower, upper, 32_u32, bytes),
                    Encoding::Frame {
                        method: 131,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<IdPage, u8>>(
            &reply,
            Encoding::Frame {
                method: 131,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
        .unwrap()
    };
    let before = pic.get_stable_memory(canister).await;
    let ascending = call(None, false, None, 0, 0, 8).await;
    let descending = call(None, true, None, 0, 0, 8).await;
    assert_eq!((&ascending.0, ascending.2), (&vec![2], 2));
    assert_eq!((&descending.0, descending.2), (&vec![6], 2));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    pic.advance_time(std::time::Duration::from_secs(600)).await;
    pic.tick().await;
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    for (direction, first, expected) in [
        (false, ascending, vec![2, 4, 6]),
        (true, descending, vec![6, 4, 2]),
    ] {
        let mut rows = first.0;
        let mut cursor = first.1;
        for _ in 0..3 {
            let page = call(cursor, direction, None, 0, 0, 8).await;
            rows.extend(page.0);
            cursor = page.1;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(rows, expected);
        assert!(cursor.is_none());
    }
    let prefixed = call(None, false, Some(1), 3, 5, 32).await;
    assert_eq!(prefixed, (vec![3, 5], None, 2));
    assert_eq!(call(None, false, None, 2, 3, 8).await, (vec![], None, 0));
    assert_eq!(call(None, true, Some(1), 5, 2, 8).await, (vec![], None, 0));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.drop().await;
}

#[tokio::test]
async fn primary_ranges_seek_and_resume_in_both_directions_across_upgrade() {
    type IdPage = (Vec<u64>, Option<Vec<u8>>, u32);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |cursor: Option<Vec<u8>>,
                      descending: bool,
                      lower: u64,
                      upper: u64,
                      work: u32,
                      bytes: u32| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_primary_range",
                ic_wire::encode(
                    &(cursor, descending, lower, upper, work, bytes),
                    Encoding::Frame {
                        method: 130,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<IdPage, u8>>(
            &reply,
            Encoding::Frame {
                method: 130,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
        .unwrap()
    };
    let backing = pic.get_stable_memory(canister).await;
    let ascending = call(None, false, 3, 5, 1, 8).await;
    let descending = call(None, true, 3, 5, 1, 8).await;
    assert_eq!((&ascending.0, ascending.2), (&vec![3], 1));
    assert_eq!((&descending.0, descending.2), (&vec![5], 1));
    assert_eq!(pic.get_stable_memory(canister).await, backing);
    pic.advance_time(std::time::Duration::from_secs(600)).await;
    pic.tick().await;
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    for (direction, first, expected) in [
        (false, ascending, vec![3, 4, 5]),
        (true, descending, vec![5, 4, 3]),
    ] {
        let mut rows = first.0;
        let mut cursor = first.1;
        for _ in 0..3 {
            let page = call(cursor, direction, 3, 5, 1, 8).await;
            assert_eq!(page.2 as usize, page.0.len());
            rows.extend(page.0);
            cursor = page.1;
            if cursor.is_none() {
                break;
            }
        }
        assert!(cursor.is_none());
        assert_eq!(rows, expected);
    }
    for (lower, upper) in [(8, 9), (5, 2)] {
        assert_eq!(
            call(None, false, lower, upper, 1, 8).await,
            (vec![], None, 0)
        );
    }
    let first = call(None, false, 3, 5, 10, 8).await;
    assert_eq!(first.0, [3]);
    assert_eq!(first.2, 2);
    let rest = call(first.1, false, 3, 5, 10, 16).await;
    assert_eq!(rest.0, [4, 5]);
    assert!(rest.1.is_none());
    assert_eq!(pic.get_stable_memory(canister).await, backing);
    owned.drop().await;
}

#[tokio::test]
async fn existence_queries_keep_work_bounds_and_upgrade_behavior() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |minimum: Option<u64>, work: u32| {
        let request = (
            "Entry",
            None::<Vec<u8>>,
            32_u32,
            minimum,
            None::<u64>,
            None::<u64>,
            Vec::<String>::new(),
            false,
            work,
            1_u32,
            "exists",
            (
                false,
                (None::<String>, None::<String>),
                None::<String>,
                RowFilters::default(),
            ),
        );
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
        ic_wire::decode::<Result<DisplayPage, String>>(
            &reply,
            Encoding::Frame {
                method: 3,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let before = pic.get_stable_memory(canister).await;
    for upgraded in [false, true] {
        if upgraded {
            pic.advance_time(std::time::Duration::from_secs(600)).await;
            pic.tick().await;
            ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
        }
        for (minimum, work, expected, examined) in [
            (None, 1, "true", 1),
            (Some(50), 5, "true", 5),
            (Some(999), 7, "false", 6),
        ] {
            let result = call(minimum, work).await.unwrap();
            assert_eq!(result.0, vec!["exists"]);
            assert_eq!(result.1, vec![vec![expected]]);
            assert!(result.2.is_none());
            assert_eq!((result.3, result.4, result.5), (examined, examined, 1));
        }
        assert!(call(Some(999), 6).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    owned.drop().await;
}

#[tokio::test]
async fn optional_list_filters_project_and_continue_across_upgrade() {
    type IdPage = (Vec<u64>, Option<Vec<u8>>, u32);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |mode: u8, cursor: Option<Vec<u8>>| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_list_presence",
                ic_wire::encode(
                    &(mode, cursor),
                    Encoding::Frame {
                        method: 129,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<IdPage, u8>>(
            &reply,
            Encoding::Frame {
                method: 129,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    for (mode, shape, size) in [
        (1_u8, 0_u8, 0_u8),
        (2, 0, 2),
        (2, 2, 0),
        (2, 2, 1),
        (2, 2, 2),
    ] {
        let reply = pic
            .update_call(
                canister,
                canister,
                "list_write",
                ic_wire::encode(
                    &(mode, shape, size),
                    Encoding::Frame {
                        method: 33,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            ic_wire::decode::<Result<(Vec<u8>, u64), u8>>(
                &reply,
                Encoding::Frame {
                    method: 33,
                    direction: Direction::Response
                },
                LIMITS
            )
            .unwrap()
            .is_ok()
        );
        let before = pic.get_stable_memory(canister).await;
        for (filter, matches) in [
            (0, shape != 2),
            (1, shape == 2),
            (2, shape == 2),
            (3, shape != 0 || size == 0),
            (4, shape == 2 && size == 0),
            (5, shape == 2 && size == 1),
            (6, shape == 0 && size > 0),
            (7, false),
        ] {
            let first = call(filter, None).await.unwrap();
            assert_eq!(first.0, if matches { vec![1] } else { vec![] });
            assert_eq!(first.2, 1);
            let cursor = first.1.unwrap();
            assert_eq!(call(filter ^ 1, Some(cursor.clone())).await.unwrap_err(), 1);
            if matches!(filter, 1 | 2 | 5) {
                pic.advance_time(std::time::Duration::from_secs(600)).await;
                pic.tick().await;
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
            let done = call(filter, Some(cursor)).await.unwrap();
            assert!(done.0.is_empty());
            assert!(done.1.is_none());
        }
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    owned.drop().await;
}

#[tokio::test]
async fn sparse_presence_order_resumes_after_upgrade_without_losing_absent_matches() {
    type SparsePage = (Vec<u64>, Option<Vec<u8>>, u32);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let rows: Vec<_> = [Some("3.00"), None, Some("1.00"), Some("2.00"), None]
        .into_iter()
        .enumerate()
        .map(|(i, maybe)| (i as u64 + 1, maybe, "0.00"))
        .collect();
    let reply = pic
        .update_call(
            canister,
            canister,
            "decimal_batch",
            ic_wire::encode(
                &(rows, false),
                Encoding::Frame {
                    method: 67,
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
                method: 67,
                direction: Direction::Response
            },
            LIMITS
        )
        .unwrap(),
        Ok(())
    );
    let call =
        async |include_absent: bool, descending: bool, cursor: Option<Vec<u8>>, bytes: u32| {
            let reply = pic
                .query_call(
                    canister,
                    canister,
                    "fluent_sparse_presence",
                    ic_wire::encode(
                        &(include_absent, descending, cursor, bytes),
                        Encoding::Frame {
                            method: 128,
                            direction: Direction::Request,
                        },
                        LIMITS,
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            ic_wire::decode::<Result<SparsePage, u8>>(
                &reply,
                Encoding::Frame {
                    method: 128,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap()
        };
    let before = pic.get_stable_memory(canister).await;
    // OR admits absence here: the only available optional-field order is incomplete.
    assert_eq!(call(true, false, None, 8).await.unwrap_err(), 3);
    assert_eq!(call(false, false, None, 7).await.unwrap_err(), 2);
    for descending in [false, true] {
        let first = call(false, descending, None, 8).await.unwrap();
        let mut actual = first.0;
        let mut cursor = first.1;
        assert_eq!(first.2, 1);
        assert_eq!(
            call(false, !descending, cursor.clone(), 8)
                .await
                .unwrap_err(),
            1
        );
        pic.advance_time(std::time::Duration::from_secs(600)).await;
        pic.tick().await;
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        let mut examined = first.2;
        for step in 0..4 {
            let page = call(false, descending, cursor, 8).await.unwrap();
            actual.extend(page.0);
            examined += page.2;
            cursor = page.1;
            if cursor.is_none() {
                break;
            }
            assert!(step < 3);
        }
        assert_eq!(
            actual,
            if descending {
                vec![1, 4, 3]
            } else {
                vec![3, 4, 1]
            }
        );
        assert_eq!(examined, 3);
    }
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.drop().await;
}

#[tokio::test]
async fn text_matching_uses_exact_utf8_and_resumes_across_upgrade() {
    type TextPage = (Vec<Option<String>>, Option<Vec<u8>>, u32, u32);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |mode: u8, needle: &str, cursor: Option<Vec<u8>>, bytes: u32| {
        let request = (needle, cursor, 1_u32, bytes);
        let (name, method) = match mode {
            0 => ("fluent_text_prefix", 125),
            1 => ("fluent_text_suffix", 126),
            _ => ("fluent_text_contains", 127),
        };
        let reply = pic
            .query_call(
                canister,
                canister,
                name,
                ic_wire::encode(
                    &request,
                    Encoding::Frame {
                        method,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<TextPage, u8>>(
            &reply,
            Encoding::Frame {
                method,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    for note in [
        None,
        Some(""),
        Some("a\0b"),
        Some("éclair"),
        Some("e\u{301}clair"),
        Some("\u{1}"),
        Some("end\0"),
        Some("\0\u{1}"),
    ] {
        let request = ("Document", note, false, Some(0_u64), -10_i64, None::<i64>);
        let reply = pic
            .update_call(
                canister,
                canister,
                "replace_document",
                ic_wire::encode(
                    &request,
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
        let before = pic.get_stable_memory(canister).await;
        for mode in 0..3 {
            for needle in [
                "", "a", "a\0", "a\0c", "A", "é", "e\u{301}", "\0b", "air", "\u{1}", "\0",
                "\0\u{1}", "lai", "\u{301}",
            ] {
                let page = call(mode, needle, None, 80).await.unwrap();
                let expected: Vec<_> = note
                    .filter(|note| match mode {
                        0 => note.starts_with(needle),
                        1 => note.ends_with(needle),
                        _ => note.contains(needle),
                    })
                    .map(|text| Some(text.to_owned()))
                    .into_iter()
                    .collect();
                assert_eq!(page.0, expected);
                assert_eq!(page.2, 1);
                let cursor = page.1.unwrap();
                assert!(
                    call((mode + 1) % 3, needle, Some(cursor.clone()), 80)
                        .await
                        .is_err()
                );
                assert!(
                    call(mode, "different", Some(cursor.clone()), 80)
                        .await
                        .is_err()
                );
                if note == Some("a\0b")
                    && needle
                        == match mode {
                            0 => "a\0",
                            1 => "\0b",
                            _ => "\0",
                        }
                {
                    assert_eq!(call(mode, needle, None, 1).await.unwrap_err(), 2);
                    pic.advance_time(std::time::Duration::from_secs(600)).await;
                    pic.tick().await;
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
                let finished = call(mode, needle, Some(cursor), 80).await.unwrap();
                assert!(finished.0.is_empty());
                assert!(finished.1.is_none());
            }
            assert!(call(mode, &"x".repeat(65), None, 80).await.is_err());
        }
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    owned.drop().await;
}

#[tokio::test]
async fn membership_filters_do_not_duplicate_rows_and_resume_after_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let before = pic.get_stable_memory(canister).await;
    let call = async |request: Request| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_membership",
                ic_wire::encode(
                    &request,
                    Encoding::Frame {
                        method: 124,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<Page, u8>>(
            &reply,
            Encoding::Frame {
                method: 124,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    for descending in [false, true] {
        let mut cursor = None;
        let mut rows = Vec::new();
        for step in 0..10 {
            let page = call((cursor, descending, None, 51, 1, 1, 16))
                .await
                .unwrap();
            assert!(page.2 <= 1);
            rows.extend(page.0);
            cursor = page.1;
            if cursor.is_none() {
                break;
            }
            assert!(
                step < 9,
                "membership must progress over rejected candidates"
            );
            if step == 0 {
                assert!(
                    call((cursor.clone(), descending, None, 60, 1, 1, 16))
                        .await
                        .is_err()
                );
                pic.advance_time(std::time::Duration::from_secs(600)).await;
                pic.tick().await;
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
        let mut expected = vec![(2, 20), (5, 51)];
        if descending {
            expected.reverse();
        }
        assert_eq!(rows, expected);
    }
    assert_eq!(
        call((None, false, Some(1), 51, 32, 128, 4096))
            .await
            .unwrap()
            .0,
        [(5, 51)]
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.drop().await;
}

#[tokio::test]
async fn any_filters_page_and_preserve_existing_conjunction_cursors() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let predecessor = std::env::var_os("IC_DB_QUERY_PREDECESSOR_WASM")
        .map(|path| std::fs::read(path).unwrap())
        .unwrap_or_else(|| wasm.clone());
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, predecessor, vec![], None)
        .await;
    let call = async |any: bool, request: Request| {
        let (method, id) = if any {
            ("fluent_any", 123)
        } else {
            ("fluent_query", 95)
        };
        let reply = pic
            .query_call(
                canister,
                canister,
                method,
                ic_wire::encode(
                    &request,
                    Encoding::Frame {
                        method: id,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<Page, u8>>(
            &reply,
            Encoding::Frame {
                method: id,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let page = call(false, (None, false, None, 20, 1, 128, 4096))
        .await
        .unwrap();
    assert_eq!(page.0, [(2, 20)]);
    let before = pic.get_stable_memory(canister).await;
    // Let the IC install-code rate allowance recover between synthetic upgrades.
    pic.advance_time(std::time::Duration::from_secs(600)).await;
    pic.tick().await;
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
        .await
        .unwrap();
    let resumed = call(false, (page.1, false, None, 20, 1, 128, 4096))
        .await
        .unwrap();
    assert_eq!(resumed.0, [(3, 31)]);

    for category in [None, Some(1)] {
        for descending in [false, true] {
            let mut cursor = None;
            let mut rows = Vec::new();
            for step in 0..16 {
                let page = call(true, (cursor, descending, category, 50, 2, 2, 16))
                    .await
                    .unwrap();
                assert!(page.2 <= 2);
                assert!(page.4 <= 16);
                rows.extend(page.0);
                cursor = page.1;
                if cursor.is_none() {
                    break;
                }
                assert!(step < 15, "sparse/byte-limited pages must progress");
                if step == 0 {
                    assert!(
                        call(false, (cursor.clone(), descending, category, 50, 2, 2, 16))
                            .await
                            .is_err()
                    );
                    assert!(
                        call(true, (cursor.clone(), descending, category, 30, 2, 2, 16))
                            .await
                            .is_err()
                    );
                    pic.advance_time(std::time::Duration::from_secs(600)).await;
                    pic.tick().await;
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
            let mut expected = vec![(1, 11), (2, 20), (5, 51), (6, 60)];
            if category.is_some() {
                expected.retain(|(id, _)| id % 2 == 1);
            }
            if descending {
                expected.reverse();
            }
            assert_eq!(rows, expected);
        }
    }
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.drop().await;
}

#[tokio::test]
async fn sparse_fluent_pages_retry_and_reopen() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let query = async |request: Request| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_query",
                ic_wire::encode(
                    &request,
                    Encoding::Frame {
                        method: 95,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<Page, u8>>(
            &reply,
            Encoding::Frame {
                method: 95,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let before = pic.get_stable_memory(canister).await;
    let selected =
        async |request: ConsoleRequest<'_>, group_min: Option<&str>, group_order: Option<&str>| {
            let (
                entity,
                cursor,
                limit,
                min,
                category,
                entry_id,
                order,
                desc,
                work,
                bytes,
                select,
                distinct,
            ) = request;
            let request = (
                entity,
                cursor,
                limit,
                min,
                category,
                entry_id,
                order,
                desc,
                work,
                bytes,
                select,
                (
                    distinct,
                    (group_min, None::<&str>),
                    group_order,
                    RowFilters::default(),
                ),
            );
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
            ic_wire::decode::<Result<DisplayPage, String>>(
                &reply,
                Encoding::Frame {
                    method: 3,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap()
        };
    let console = async |request: ConsoleRequest<'_>| selected(request, None, None).await;
    let complete_distinct = async |limit, work, bytes, descending| {
        console((
            "Entry",
            None,
            limit,
            Some(0),
            None,
            None,
            vec![],
            descending,
            work,
            bytes,
            "category",
            true,
        ))
        .await
    };
    let collected = complete_distinct(2, 7, 16, false).await.unwrap();
    assert_eq!(collected.1, [vec!["1"], vec!["0"]]);
    assert_eq!((collected.3, collected.4, collected.5), (6, 6, 16));
    assert!(collected.2.is_none());
    assert_eq!(
        complete_distinct(2, 7, 16, true).await.unwrap().1,
        [vec!["0"], vec!["1"]]
    );
    for (limit, work, bytes) in [(1, 7, 16), (2, 6, 16), (2, 7, 15)] {
        assert_eq!(
            complete_distinct(limit, work, bytes, false)
                .await
                .unwrap_err(),
            "query exceeds a database limit"
        );
    }
    assert_eq!(
        complete_distinct(2, 7, 7, false).await.unwrap_err(),
        "one matching result exceeds --bytes"
    );
    let ordered_request = (
        "Entry",
        None,
        1,
        None,
        None,
        None,
        vec![],
        false,
        7,
        16,
        "category,min:amount",
        false,
    );
    let selected_groups = selected(ordered_request.clone(), Some("15"), Some("desc"))
        .await
        .unwrap();
    assert_eq!(selected_groups.1, [vec!["0", "20"]]);
    assert_eq!((selected_groups.3, selected_groups.5), (6, 16));
    for operand in ["-1", "1.5", "18446744073709551616", "NaN", "1e2", ""] {
        assert!(
            selected(ordered_request.clone(), Some(operand), None)
                .await
                .is_err(),
            "{operand}"
        );
    }
    let oversized = "0".repeat(65);
    assert!(
        selected(ordered_request.clone(), Some(&oversized), None)
            .await
            .is_err()
    );
    let ascending = selected(ordered_request.clone(), None, Some("asc"))
        .await
        .unwrap();
    assert_eq!(ascending.1, [vec!["1", "11"]]);
    let mut row_request = ordered_request.clone();
    row_request.10 = "full";
    assert!(selected(row_request, None, Some("key")).await.is_err());
    let distinct = async |cursor: Option<Vec<u8>>,
                          work: u32,
                          bytes: u32,
                          descending: bool,
                          category: Option<u64>| {
        console((
            "Entry",
            cursor,
            1,
            None,
            category,
            None,
            vec!["category"],
            descending,
            work,
            bytes,
            "category",
            true,
        ))
        .await
        .unwrap()
    };
    let aggregate = async |select: &str, limit: u32, work: u32, bytes: u32| {
        console((
            "Entry",
            None,
            limit,
            None,
            None,
            None,
            vec![],
            false,
            work,
            bytes,
            select,
            false,
        ))
        .await
    };
    let sum = aggregate("sum:amount", 1, 7, 16).await.unwrap();
    assert_eq!(sum.1, [vec!["213"]]);
    assert_eq!((sum.3, sum.4, sum.5), (6, 6, 16));
    assert!(sum.2.is_none());
    assert!(aggregate("sum:amount", 1, 6, 16).await.is_err());
    assert!(aggregate("sum:amount", 1, 7, 15).await.is_err());
    let count = aggregate("count", 1, 1, 8).await.unwrap();
    assert_eq!(count.1, [vec!["6"]]);
    assert_eq!((count.3, count.4, count.5), (0, 0, 8));
    let grouped = aggregate("category,count", 1, 7, 16).await.unwrap();
    assert_eq!(grouped.1, [vec!["0", "3"]]);
    assert_eq!((grouped.3, grouped.5), (6, 16));
    assert!(grouped.2.is_none());
    assert!(aggregate("count", 1, 1, 7).await.is_err());
    assert!(aggregate("category,count", 2, 7, 31).await.is_err());
    let distinct_count = aggregate("distinct-count:category", 1, 7, 8).await.unwrap();
    assert_eq!(distinct_count.1, [vec!["2"]]);
    assert_eq!(
        (distinct_count.3, distinct_count.4, distinct_count.5),
        (6, 6, 8)
    );
    assert!(distinct_count.2.is_none());
    assert_eq!(
        aggregate("category,distinct-count:amount", 1, 7, 16)
            .await
            .unwrap(),
        grouped
    );
    let distinct_groups = aggregate("category,distinct-count:category", 2, 7, 32)
        .await
        .unwrap();
    assert_eq!(distinct_groups.1, [vec!["0", "1"], vec!["1", "1"]]);
    assert!(
        aggregate("category,distinct-count:amount", 2, 6, 32)
            .await
            .is_err()
    );
    let absent_note_group = console((
        "Document",
        None,
        1,
        None,
        None,
        None,
        vec![],
        false,
        8,
        16,
        "entry_id,distinct-count:note",
        false,
    ))
    .await
    .unwrap();
    assert_eq!(absent_note_group.1, [vec!["1", "1"]]);
    let field_count = aggregate("count:amount", 1, 7, 8).await.unwrap();
    assert_eq!(field_count.1, count.1);
    assert_eq!((field_count.3, field_count.4, field_count.5), (6, 6, 8));
    assert_eq!(
        aggregate("category,count:amount", 1, 7, 16).await.unwrap(),
        grouped
    );
    assert!(aggregate("distinct-count:category", 1, 6, 8).await.is_err());
    assert!(aggregate("count:missing", 1, 7, 8).await.is_err());
    let notes = console((
        "Document",
        None,
        1,
        None,
        None,
        None,
        vec![],
        false,
        128,
        8,
        "count:note",
        false,
    ))
    .await
    .unwrap();
    assert_eq!(notes.1, [vec!["0"]]);
    assert_eq!((notes.3, notes.4, notes.5), (1, 1, 8));
    let minimum = aggregate("min:amount", 1, 7, 8).await.unwrap();
    let maximum = aggregate("max:amount", 1, 7, 8).await.unwrap();
    assert_eq!(minimum.1, [vec!["11"]]);
    assert_eq!(maximum.1, [vec!["60"]]);
    assert_eq!((maximum.3, maximum.4, maximum.5), (6, 6, 8));
    assert!(maximum.2.is_none());
    assert!(aggregate("max:amount", 1, 6, 8).await.is_err());
    assert!(aggregate("min:amount", 1, 7, 7).await.is_err());
    let grouped_sum = aggregate("category,sum:amount", 2, 7, 48).await.unwrap();
    assert_eq!(grouped_sum.1, [vec!["0", "120"], vec!["1", "93"]]);
    assert_eq!((grouped_sum.3, grouped_sum.5), (6, 48));
    let sum_request = (
        "Entry",
        None,
        1,
        None,
        None,
        None,
        vec![],
        false,
        7,
        24,
        "category,sum:amount",
        false,
    );
    let selected_sum = selected(sum_request.clone(), Some("100"), Some("desc"))
        .await
        .unwrap();
    assert_eq!(selected_sum.1, [vec!["0", "120"]]);
    assert_eq!(
        selected(sum_request.clone(), None, Some("asc"))
            .await
            .unwrap()
            .1,
        [vec!["1", "93"]]
    );
    for operand in [
        "18446744073709551616",
        "340282366920938463463374607431768211455",
    ] {
        assert!(
            selected(sum_request.clone(), Some(operand), None)
                .await
                .unwrap()
                .1
                .is_empty()
        );
    }
    for operand in ["-1", "1.5", "340282366920938463463374607431768211456"] {
        assert!(
            selected(sum_request.clone(), Some(operand), None)
                .await
                .is_err(),
            "{operand}"
        );
    }
    assert!(aggregate("category,sum:amount", 2, 6, 48).await.is_err());
    assert!(aggregate("category,sum:amount", 2, 7, 47).await.is_err());
    let group_min = aggregate("category,min:amount", 2, 7, 32).await.unwrap();
    let group_max = aggregate("category,max:amount", 2, 7, 32).await.unwrap();
    assert_eq!(group_min.1, [vec!["0", "20"], vec!["1", "11"]]);
    assert_eq!(group_max.1, [vec!["0", "60"], vec!["1", "51"]]);
    assert_eq!((group_max.3, group_max.4, group_max.5), (6, 6, 32));
    assert!(group_max.2.is_none());
    assert_eq!(
        aggregate("category,max:amount", 1, 7, 16).await.unwrap().1,
        [vec!["0", "60"]]
    );
    assert!(aggregate("category,min:amount", 1, 6, 16).await.is_err());
    assert!(aggregate("category,max:amount", 2, 7, 31).await.is_err());
    for (select, expected, bytes) in [
        ("entry_id,min:note", "—", 8),
        ("entry_id,max:priority", "0", 17),
        ("entry_id,sum:priority", "0", 25),
        ("entry_id,sum:score", "-10", 24),
    ] {
        let result = console((
            "Document",
            None,
            1,
            None,
            None,
            None,
            vec![],
            false,
            128,
            bytes,
            select,
            false,
        ))
        .await
        .unwrap();
        assert_eq!(result.1, [vec!["1", expected]]);
        assert_eq!((result.3, result.4, result.5), (1, 1, bytes));
        assert!(result.2.is_none());
    }
    for select in ["min:note", "max:note", "min:priority", "max:priority"] {
        let result = console((
            "Document",
            None,
            1,
            None,
            None,
            None,
            vec![],
            false,
            128,
            9,
            select,
            false,
        ))
        .await
        .unwrap();
        if select.ends_with("note") {
            assert!(result.1.is_empty());
            assert_eq!(result.5, 0);
        } else {
            assert_eq!(result.1, [vec!["0"]]);
            assert_eq!(result.5, 9);
        }
        assert_eq!((result.3, result.4), (1, 1));
        assert!(result.2.is_none());
    }
    let unique_first = distinct(None, 1, 8, false, None).await;
    assert_eq!(unique_first.1, [vec!["0"]]);
    assert_eq!((unique_first.3, unique_first.5), (1, 8));
    let filtered_unique = distinct(None, 1, 8, false, Some(1)).await;
    assert_eq!(filtered_unique.1, [vec!["1"]]);
    assert_eq!(filtered_unique.3, 1);
    let ordered = async |request: Request| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_ordered",
                ic_wire::encode(
                    &request,
                    Encoding::Frame {
                        method: 98,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<Page, u8>>(
            &reply,
            Encoding::Frame {
                method: 98,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let order_start = ordered((None, false, Some(1), 40, 2, 1, 4096))
        .await
        .unwrap();
    assert!(order_start.0.is_empty());
    assert_eq!(order_start.2, 1);
    assert!(order_start.1.is_some());
    let all = ordered((None, false, None, 0, 32, 128, 4096))
        .await
        .unwrap();
    assert_eq!(
        all.0,
        [(2, 20), (4, 40), (6, 60), (1, 11), (3, 31), (5, 51)]
    );
    assert_eq!(all.2, 6);
    let reverse = ordered((None, true, None, 0, 32, 128, 4096)).await.unwrap();
    assert_eq!(reverse.0, all.0.into_iter().rev().collect::<Vec<_>>());
    let project = async |request: Request| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_projection",
                ic_wire::encode(
                    &request,
                    Encoding::Frame {
                        method: 96,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<Page, u8>>(
            &reply,
            Encoding::Frame {
                method: 96,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let projected = project((None, false, None, 0, 32, 128, 31)).await.unwrap();
    assert_eq!(projected.0, [(1, 11)]);
    assert_eq!((projected.2, projected.4), (2, 16));
    assert_eq!(
        query((projected.1.clone(), false, None, 0, 32, 128, 4096))
            .await
            .unwrap_err(),
        1
    );
    let first = query((None, false, Some(1), 40, 2, 1, 4096)).await.unwrap();
    assert!(first.0.is_empty());
    assert!(first.1.is_some());
    assert_eq!((first.2, first.3, first.4), (1, 1, 0));
    println!("fluent_sparse_first_instructions={}", first.5);
    assert_eq!(
        query((first.1.clone(), true, Some(1), 40, 2, 1, 4096))
            .await
            .unwrap_err(),
        1
    );
    assert_eq!(
        query((first.1.clone(), false, Some(1), 41, 2, 1, 4096))
            .await
            .unwrap_err(),
        1
    );
    // Only queries ran since install, so no update rounds drained the IC's
    // install-code rate limiter. Advance virtual time, not wall-clock time.
    pic.advance_time(std::time::Duration::from_secs(600)).await;
    pic.tick().await;
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(complete_distinct(2, 7, 16, false).await.unwrap(), collected);
    assert_eq!(
        aggregate("category,count", 1, 7, 16).await.unwrap(),
        grouped
    );
    assert_eq!(
        aggregate("distinct-count:category", 1, 7, 8).await.unwrap(),
        distinct_count
    );
    assert_eq!(
        aggregate("category,distinct-count:category", 2, 7, 32)
            .await
            .unwrap(),
        distinct_groups
    );
    assert_eq!(
        selected(ordered_request, Some("15"), Some("desc"))
            .await
            .unwrap(),
        selected_groups
    );
    assert_eq!(
        aggregate("category,sum:amount", 2, 7, 48).await.unwrap(),
        grouped_sum
    );
    assert_eq!(
        selected(sum_request, Some("100"), Some("desc"))
            .await
            .unwrap(),
        selected_sum
    );
    assert_eq!(aggregate("sum:amount", 1, 7, 16).await.unwrap(), sum);
    assert_eq!(aggregate("max:amount", 1, 7, 8).await.unwrap(), maximum);
    assert_eq!(
        aggregate("category,max:amount", 2, 7, 32).await.unwrap(),
        group_max
    );
    let filtered_done = distinct(filtered_unique.2, 4, 8, false, Some(1)).await;
    assert!(filtered_done.1.is_empty());
    assert!(filtered_done.2.is_none());
    assert_eq!(filtered_done.3, 2);
    let empty_unique = distinct(unique_first.2, 2, 8, false, None).await;
    assert!(empty_unique.1.is_empty());
    assert_eq!(empty_unique.3, 2);
    assert!(empty_unique.2.is_some());
    let second_unique = distinct(empty_unique.2, 4, 8, false, None).await;
    assert_eq!(second_unique.1, [vec!["1"]]);
    let done_unique = distinct(second_unique.2, 4, 8, false, None).await;
    assert!(done_unique.1.is_empty());
    assert!(done_unique.2.is_none());
    assert_eq!(distinct(None, 4, 8, true, None).await.1, [vec!["1"]]);
    let after_upgrade = ordered((order_start.1.clone(), false, Some(1), 40, 2, 128, 4096))
        .await
        .unwrap();
    assert_eq!(after_upgrade.0, [(5, 51)]);
    assert_eq!(after_upgrade.2, 2); // only the two remaining category=1 entries
    assert_eq!(
        ordered((order_start.1.clone(), true, Some(1), 40, 2, 128, 4096))
            .await
            .unwrap_err(),
        1
    );
    assert_eq!(
        ordered((order_start.1, false, Some(0), 40, 2, 128, 4096))
            .await
            .unwrap_err(),
        1
    );
    let resumed = project((projected.1, false, None, 0, 32, 128, 4096))
        .await
        .unwrap();
    assert_eq!(resumed.0, [(2, 20), (3, 31), (4, 40), (5, 51), (6, 60)]);
    assert_eq!(resumed.4, 80);
    let second = query((first.1, false, Some(1), 40, 2, 1, 4096))
        .await
        .unwrap();
    assert!(second.0.is_empty());
    assert!(second.1.is_some());
    let third = query((second.1, false, Some(1), 40, 2, 1, 4096))
        .await
        .unwrap();
    assert_eq!(third.0, [(5, 51)]);
    assert_eq!(third.4, 24);
    assert!(
        query((third.1, false, Some(1), 40, 2, 1, 4096))
            .await
            .unwrap()
            .1
            .is_none()
    );
    let bytes = query((None, false, None, 0, 32, 128, 47)).await.unwrap();
    assert_eq!(bytes.0, [(1, 11)]);
    assert_eq!((bytes.2, bytes.3, bytes.4), (2, 2, 24));
    assert_eq!(
        query((bytes.1, false, None, 0, 32, 128, 4096))
            .await
            .unwrap()
            .0,
        [(2, 20), (3, 31), (4, 40), (5, 51), (6, 60)]
    );
    assert_eq!(
        query((None, false, None, 0, 32, 128, 23))
            .await
            .unwrap_err(),
        2
    );
    let descending = query((None, true, Some(1), 0, 32, 128, 4096))
        .await
        .unwrap();
    assert_eq!(descending.0, [(5, 51), (3, 31), (1, 11)]);
    assert!(descending.1.is_none());
    println!("fluent_descending_three_instructions={}", descending.5);
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let reply = pic
        .update_call(
            canister,
            canister,
            "replace_binary_object",
            ic_wire::encode(
                &(
                    (
                        ic_wire::ByteBuf::from(vec![7; 1024]),
                        None::<ic_wire::ByteBuf>,
                    ),
                    false,
                ),
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
    ic_wire::decode::<Result<(), u8>>(
        &reply,
        Encoding::Frame {
            method: 14,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap()
    .unwrap();
    let before = pic.get_stable_memory(canister).await;
    let binary = async |projected: bool| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_binary_projection",
                ic_wire::encode(
                    &projected,
                    Encoding::Frame {
                        method: 97,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<(Vec<u64>, u32, u64)>(
            &reply,
            Encoding::Frame {
                method: 97,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let full = binary(false).await;
    let projected = binary(true).await;
    assert_eq!(full.0, [1]);
    assert_eq!(full.0, projected.0);
    assert_eq!((full.1, projected.1), (1037, 8));
    println!("fluent_blob_full_instructions={}", full.2);
    println!("fluent_blob_projected_instructions={}", projected.2);
    assert_eq!(pic.get_stable_memory(canister).await, before);
    // Explicit result widening stays exact beyond the source's 64-bit range in Wasm.
    for id in 1_u64..=2 {
        let reply = pic
            .update_call(
                canister,
                canister,
                "write",
                ic_wire::encode(
                    &(id, id % 2, u64::MAX, 0_u8),
                    Encoding::Frame {
                        method: 4,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<(), u8>>(
            &reply,
            Encoding::Frame {
                method: 4,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
        .unwrap();
    }
    let before = pic.get_stable_memory(canister).await;
    let widened = aggregate("sum:amount", 1, 7, 16).await.unwrap();
    assert_eq!(
        widened.1,
        [vec![(u128::from(u64::MAX) * 2 + 182).to_string()]]
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let widened_groups = aggregate("category,sum:amount", 2, 7, 48).await.unwrap();
    assert_eq!(
        widened_groups.1,
        [
            vec!["0".to_string(), (u128::from(u64::MAX) + 100).to_string()],
            vec!["1".to_string(), (u128::from(u64::MAX) + 82).to_string()]
        ]
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.drop().await;
}

#[tokio::test]
async fn decimal_aggregates_preserve_scale_rounding_and_upgrade_behavior() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |values: Vec<String>| {
        let rows: Vec<_> = values
            .into_iter()
            .enumerate()
            .map(|(i, value)| (i as u64 + 1, None::<String>, value))
            .collect();
        let reply = pic
            .update_call(
                canister,
                canister,
                "decimal_batch",
                ic_wire::encode(
                    &(rows, false),
                    Encoding::Frame {
                        method: 67,
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
                    method: 67,
                    direction: Direction::Response
                },
                LIMITS
            )
            .unwrap(),
            Ok(())
        );
    };
    let query = async |select: &str, minimum: Option<&str>| {
        let request = (
            "DecimalRow",
            None::<Vec<u8>>,
            1_u32,
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
                (minimum, None::<&str>),
                select.starts_with("zone,").then_some("desc"),
                RowFilters::default(),
            ),
        );
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
        ic_wire::decode::<Result<DisplayPage, String>>(
            &reply,
            Encoding::Frame {
                method: 3,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    write(vec!["1.25".into(), "2.50".into(), "-0.75".into()]).await;
    for upgraded in [false, true] {
        if upgraded {
            ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
        }
        let before = pic.get_stable_memory(canister).await;
        let sum = query("sum:value", None).await.unwrap();
        assert_eq!(sum.1, vec![vec!["3".to_owned()]]);
        assert_eq!(sum.5, 17);
        assert_eq!(
            query("avg:value", None).await.unwrap().1,
            vec![vec!["1".to_owned()]]
        );
        for threshold in ["0.75", "-1.25", "1.00"] {
            assert_eq!(
                query("zone,avg:value", Some(threshold)).await.unwrap().1,
                vec![vec!["true".to_owned(), "1".to_owned()]]
            );
        }
        assert!(
            query("zone,avg:value", Some("1.01"))
                .await
                .unwrap()
                .1
                .is_empty()
        );
        for threshold in [
            "0.001",
            "NaN",
            "1e0",
            "1701411834604692317316873037158841057.28",
            "",
        ] {
            assert!(
                query("zone,avg:value", Some(threshold)).await.is_err(),
                "{threshold}"
            );
        }
        assert!(query("avg:maybe", None).await.unwrap().1.is_empty());
        assert_eq!(
            query("zone,avg:value", Some("1")).await.unwrap().1,
            vec![vec!["true".to_owned(), "1".to_owned()]]
        );
        assert!(
            query("zone,avg:value", Some("2"))
                .await
                .unwrap()
                .1
                .is_empty()
        );
        assert_eq!(
            query("zone,avg:maybe", None).await.unwrap().1,
            vec![vec!["true".to_owned(), "—".to_owned()]]
        );
        assert!(query("sum:maybe", None).await.unwrap().1.is_empty());
        assert_eq!(
            query("zone,sum:value", Some("3")).await.unwrap().1,
            vec![vec!["true".to_owned(), "3".to_owned()]]
        );
        assert!(
            query("zone,sum:value", Some("4"))
                .await
                .unwrap()
                .1
                .is_empty()
        );
        assert_eq!(
            query("zone,sum:maybe", None).await.unwrap().1,
            vec![vec!["true".to_owned(), "—".to_owned()]]
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    for aliases in [["1", "1.0", "1.00"], ["0", "-0.00", "0.0"]] {
        write(aliases.into_iter().map(str::to_owned).collect()).await;
        for upgraded in [false, true] {
            if upgraded {
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
            let before = pic.get_stable_memory(canister).await;
            for (select, expected) in [
                ("count:value", vec![vec!["3"]]),
                ("count:maybe", vec![vec!["0"]]),
                ("distinct-count:value", vec![vec!["1"]]),
                ("distinct-count:maybe", vec![vec!["1"]]),
                ("zone,distinct-count:value", vec![vec!["true", "1"]]),
                ("zone,distinct-count:maybe", vec![vec!["true", "1"]]),
            ] {
                assert_eq!(query(select, None).await.unwrap().1, expected, "{select}");
            }
            assert_eq!(pic.get_stable_memory(canister).await, before);
        }
    }
    // Fixed scale 2: max coefficient + one cent - one cent cancels exactly.
    let maximum = "1701411834604692317316873037158841057.27";
    write(vec![maximum.into(), "0.01".into(), "-0.01".into()]).await;
    assert_eq!(
        query("sum:value", None).await.unwrap().1,
        vec![vec![maximum.to_owned()]]
    );
    write(vec![maximum.into(), "0.01".into(), "0".into()]).await;
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        query("sum:value", None).await.unwrap_err(),
        "aggregate exceeds its declared result range"
    );
    assert_eq!(
        query("zone,sum:value", None).await.unwrap_err(),
        "aggregate exceeds its declared result range"
    );
    assert_eq!(
        query("avg:value", None).await.unwrap().1,
        vec![vec!["567137278201564105772291012386280352.43".to_owned()]]
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for (values, expected) in [
        (["1.24", "1.25", "1.24", "1.25"], "1.24"),
        (["-1.25", "-1.26", "-1.25", "-1.26"], "-1.26"),
    ] {
        write(values.into_iter().map(str::to_owned).collect()).await;
        let before = pic.get_stable_memory(canister).await;
        assert_eq!(
            query("avg:value", None).await.unwrap().1,
            vec![vec![expected.to_owned()]]
        );
        assert_eq!(
            query("zone,avg:value", None).await.unwrap().1,
            vec![vec!["true".to_owned(), expected.to_owned()]]
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    owned.drop().await;
}
