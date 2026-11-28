//! Retained filters, Decimal rounding and Blob results through the console across upgrades.

mod wire_call;

use ic_db_m0_rollback_driver::{instance, upgrade_stopped};
use ic_db_qualification_transport::{
    ComparisonFilter, FilterComparison, PayloadFilters, RetainedResume,
    RetainedSelection as Selection, RowFilters,
};
use wire_call::call;

use ic_db_qualification_transport::{DisplayPage, RetainedPhase, RetainedProgress as Step};

fn retained_selection(entity: &str, select: impl Into<String>) -> Selection {
    Selection {
        entity: entity.into(),
        select: select.into(),
        entity_filters: Default::default(),
        limit: 32,
        group_bounds: (None, None),
        group_order: None,
        filters: RowFilters::default(),
        order: Default::default(),
    }
}

async fn resume_step(
    pic: &ic_testkit::pocket_ic::nonblocking::PocketIc,
    canister: ic_testkit::pocket_ic::CanisterId,
    request: RetainedResume,
) -> Result<Step, String> {
    call(pic, canister, "aggregate_resume", 121, &request).await
}

fn completed(step: &Step) -> &DisplayPage {
    let RetainedPhase::Complete(output) = &step.phase else {
        panic!("expected completed output, got {:?}", step.phase);
    };

    output
}

#[tokio::test]
async fn distinct_keys_page_past_32_after_upgrade() {
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    for id in 1_u64..=80 {
        call::<Result<(), u8>>(
            pic,
            canister,
            "write",
            4,
            &(id, 0_u64, id % 40, u8::from(id > 6)),
        )
        .await
        .unwrap();
    }
    // An exact count retains 32 keys. Its 33rd distinct value must allow
    // cancellation at the previous checkpoint and a fresh enumeration.
    let count = retained_selection("Entry", "distinct-count:amount");
    let refused = call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &count)
        .await
        .unwrap();
    let advance_count = async |sequence: u64, work: u32| {
        resume_step(
            pic,
            canister,
            RetainedResume {
                selection: count.clone(),
                handle: refused.to_vec(),
                sequence,
                work,
                bytes: 4096,
                page_size: 1,
                after: None,
            },
        )
        .await
    };
    let prefix = advance_count(0, 32).await.unwrap();
    assert_eq!((prefix.phase.label(), prefix.examined), ("Running", 32));

    let checkpoint = pic.get_stable_memory(canister).await;
    assert_eq!(
        advance_count(prefix.sequence, 1).await.unwrap_err(),
        "Limit"
    );
    assert_eq!(pic.get_stable_memory(canister).await, checkpoint);
    call::<Result<(), String>>(pic, canister, "aggregate_cancel", 122, &refused)
        .await
        .unwrap();
    for (order, reverse_input) in [
        (None, false),
        (Some("key-desc"), false),
        (Some("first"), false),
        (Some("first"), true),
    ] {
        let selection = Selection {
            group_order: order.map(str::to_owned),
            order: if reverse_input {
                (vec!["category".into()], true)
            } else {
                Default::default()
            },
            ..retained_selection("Entry", "distinct:amount")
        };
        let handle =
            call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &selection)
                .await
                .unwrap();
        if order.is_none() {
            assert_eq!(
                call::<Result<(), String>>(pic, canister, "aggregate_cancel", 122, &refused)
                    .await
                    .unwrap_err(),
                "InvalidJob"
            );
        }
        let mut sequence = 0_u64;
        let mut cursor = None::<Vec<u8>>;
        let mut output = Vec::new();
        for _ in 0..16 {
            let before = pic.cycle_balance(canister).await;
            let step = resume_step(
                pic,
                canister,
                RetainedResume {
                    selection: selection.clone(),
                    handle: handle.to_vec(),
                    sequence,
                    work: 17,
                    bytes: 4096,
                    page_size: 11,
                    after: cursor.clone(),
                },
            )
            .await
            .unwrap();
            sequence = step.sequence;
            if step.phase.label() == "Running" {
                if order == Some("first") && sequence == 1 {
                    upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
                        .await
                        .unwrap();
                }
                continue;
            }
            assert_eq!(step.phase.label(), "Complete");
            assert_eq!(step.examined, 80);
            let charged = before - pic.cycle_balance(canister).await;
            eprintln!(
                "retained DISTINCT start={} rows={} charged_cycles={charged}",
                output.len(),
                completed(&step).rows.len()
            );
            output.extend(
                completed(&step)
                    .rows
                    .iter()
                    .map(|row| row[0].parse::<u64>().unwrap()),
            );
            cursor = completed(&step).next.clone();
            if output.len() == 33 {
                upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
                    .await
                    .unwrap();
            }
            if cursor.is_none() {
                break;
            }
        }
        let mut expected = (0..40).collect::<Vec<_>>();
        if reverse_input {
            // IDs 80 through 41 encounter amount 0 first, then 39 down to 1.
            expected[1..].reverse();
        } else if order == Some("first") {
            expected.rotate_left(1); // First matched ID is 1; amount 0 first occurs at ID 40.
        } else if order == Some("key-desc") {
            expected.reverse();
        }
        assert_eq!(output, expected);
        assert!(cursor.is_none());
        call::<Result<(), String>>(pic, canister, "aggregate_cancel", 122, &handle)
            .await
            .unwrap();
    }
    owned.drop().await;
}

#[tokio::test]
#[ignore = "requires the retained-console Wasm built from the v0.33.1 tag"]
async fn distinct_jobs_resume_from_v0331_with_fresh_page_cursors() {
    // This archived canister accepts positional frames from before Wire 0.33.
    async fn old_call<T: serde::de::DeserializeOwned>(
        pic: &ic_testkit::pocket_ic::nonblocking::PocketIc,
        canister: ic_testkit::pocket_ic::CanisterId,
        name: &str,
        method: u64,
        body: &impl serde::Serialize,
    ) -> T {
        use ic_wire_positional::{self as wire, Direction, Encoding};
        let current = ic_db_m0_rollback_driver::LIMITS;
        let limits = wire::Limits {
            bytes: current.bytes,
            allocation: current.allocation,
            nodes: current.nodes,
            depth: current.depth,
        };
        let frame = |direction| Encoding::Frame { method, direction };
        let request = wire::encode(body, frame(Direction::Request), limits).unwrap();
        let reply = pic
            .update_call(canister, canister, name, request)
            .await
            .unwrap();
        wire::decode(&reply, frame(Direction::Response), limits).unwrap()
    }
    let old = std::fs::read(std::env::var_os("IC_DB_V0331_CONSOLE_WASM").unwrap()).unwrap();
    let new = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, old, vec![], None).await;

    let selection = retained_selection("Entry", "distinct:amount");
    type PreviousStep = (
        u64,
        String,
        u64,
        u64,
        u64,
        (Vec<String>, Vec<Vec<String>>, Option<Vec<u8>>),
    );

    let mut saved = Vec::new();
    for work in [2_u32, 32] {
        let handle =
            old_call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &selection)
                .await
                .unwrap();
        let step = old_call::<Result<PreviousStep, String>>(
            pic,
            canister,
            "aggregate_resume",
            121,
            &(
                &selection,
                &handle,
                0_u64,
                work,
                4096_u32,
                2_u32,
                None::<Vec<u8>>,
            ),
        )
        .await
        .unwrap();
        if work == 2 {
            assert_eq!((step.1.as_str(), step.2), ("Running", 2));
            assert!(step.5.1.is_empty() && step.5.2.is_none());
        } else {
            assert_eq!((step.1.as_str(), step.2), ("Complete", 6));
            assert_eq!(step.5.1, [["11"], ["20"]]);
            assert!(step.5.2.is_some());
        }
        saved.push((handle, step.0, step.5.2.is_some()));
    }

    upgrade_stopped(pic, canister, new, vec![], None)
        .await
        .unwrap();
    for (handle, sequence, was_complete) in saved {
        let resume = async |sequence: u64, after: Option<Vec<u8>>| {
            resume_step(
                pic,
                canister,
                RetainedResume {
                    selection: selection.clone(),
                    handle: handle.to_vec(),
                    sequence,
                    work: 32,
                    bytes: 4096,
                    page_size: 3,
                    after,
                },
            )
            .await
            .unwrap()
        };
        let first = resume(sequence, None).await;
        assert_eq!((first.phase.label(), first.examined), ("Complete", 6));
        assert_eq!(first.sequence, sequence + u64::from(!was_complete));
        assert_eq!(completed(&first).rows, [["11"], ["20"], ["31"]]);
        let cursor = completed(&first).next.clone().unwrap();
        assert_eq!(cursor[0], 3);
        let second = resume(first.sequence, Some(cursor)).await;
        assert_eq!(second.sequence, first.sequence);
        assert_eq!(completed(&second).rows, [["40"], ["51"], ["60"]]);
        assert!(completed(&second).next.is_none());
    }
    owned.drop().await;
}

// Hold the returned page, cursor position and access path fixed as the checkpoint grows.
#[tokio::test]
async fn completed_distinct_single_key_page_cost_by_cardinality() {
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    let selection = Selection {
        group_order: Some("first".into()),
        ..retained_selection("Entry", "distinct:amount")
    };
    let mut populated = 0;
    for keys in [8_u64, 32, 128, 255] {
        for id in populated + 1..=keys {
            call::<Result<(), u8>>(
                pic,
                canister,
                "write",
                4,
                &(id, 0_u64, id, u8::from(id > 6)),
            )
            .await
            .unwrap();
        }
        populated = keys;
        let handle =
            call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &selection)
                .await
                .unwrap();
        let before_scan = pic.cycle_balance(canister).await;
        let first = resume_step(
            pic,
            canister,
            RetainedResume {
                selection: selection.clone(),
                handle: handle.to_vec(),
                sequence: 0,
                work: 512,
                bytes: 4096,
                page_size: 1,
                after: None,
            },
        )
        .await
        .unwrap();
        let scan_cycles = before_scan - pic.cycle_balance(canister).await;
        assert_eq!(first.phase.label(), "Complete");
        assert_eq!(first.examined, keys);
        assert_eq!(completed(&first).rows, [["1"]]);
        assert!(completed(&first).next.is_some());
        let before = pic.cycle_balance(canister).await;
        let second = resume_step(
            pic,
            canister,
            RetainedResume {
                selection: selection.clone(),
                handle: handle.to_vec(),
                sequence: first.sequence,
                work: 1,
                bytes: 4096,
                page_size: 1,
                after: completed(&first).next.clone(),
            },
        )
        .await
        .unwrap();
        let charged = before - pic.cycle_balance(canister).await;
        assert_eq!(second.phase.label(), "Complete");
        assert_eq!((second.sequence, second.examined), (first.sequence, keys));
        assert_eq!(completed(&second).rows, [["2"]]);
        assert!(completed(&second).next.is_some());
        println!("distinct-scan-and-first-page,keys={keys},charged_cycles={scan_cycles}");
        println!("completed-distinct,keys={keys},page_size=1,offset=1,charged_cycles={charged}");
        call::<Result<(), String>>(pic, canister, "aggregate_cancel", 122, &handle)
            .await
            .unwrap();
    }
    owned.drop().await;
}

#[tokio::test]
async fn repeated_group_keys_preserve_counts_across_checkpoints() {
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    for id in 1_u64..=256 {
        let n = id - 1;
        call::<Result<(), u8>>(
            pic,
            canister,
            "write",
            4,
            &(id, n % 2, (n / 2) % 32, u8::from(id > 6)),
        )
        .await
        .unwrap();
    }
    for (select, expected) in [
        ("category,count", vec![vec!["0", "128"], vec!["1", "128"]]),
        (
            "category,distinct-count:amount",
            vec![vec!["0", "32"], vec!["1", "32"]],
        ),
        ("distinct-count:category", vec![vec!["2"]]),
        (
            "category,distinct-count:category",
            vec![vec!["0", "1"], vec!["1", "1"]],
        ),
    ] {
        let selection = retained_selection("Entry", select);
        let handle =
            call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &selection)
                .await
                .unwrap();
        let mut sequence = 0;
        for (index, work) in [128_u32, 256].into_iter().enumerate() {
            let before = pic.cycle_balance(canister).await;
            let step = resume_step(
                pic,
                canister,
                RetainedResume {
                    selection: selection.clone(),
                    handle: handle.to_vec(),
                    sequence,
                    work,
                    bytes: 4096,
                    page_size: 2,
                    after: None,
                },
            )
            .await
            .unwrap();
            let charged = before - pic.cycle_balance(canister).await;
            println!("repeated-keys,select={select},step={index},charged_cycles={charged}");
            sequence = step.sequence;
            if index == 0 {
                assert_eq!((step.phase.label(), step.examined), ("Running", 128));
            } else {
                assert_eq!((step.phase.label(), step.examined), ("Complete", 256));
                assert_eq!(completed(&step).rows, expected);
                assert!(completed(&step).next.is_none());
            }
        }
        call::<Result<(), String>>(pic, canister, "aggregate_cancel", 122, &handle)
            .await
            .unwrap();
    }
    owned.drop().await;
}

#[tokio::test]
async fn retained_field_substrings_bind_the_needle_and_survive_upgrade() {
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;

    // text_index_batch copies the current note from document 1 onto its rows.
    let insert = async |rows: &[(u64, &str, bool)]| {
        call::<Result<(), u8>>(pic, canister, "text_index_batch", 23, &(rows, false))
            .await
            .unwrap();
    };
    let set_note = async |note: &str| {
        call::<Result<(), u8>>(
            pic,
            canister,
            "replace_document",
            8,
            &(
                "Document",
                Some(note),
                false,
                None::<u64>,
                0_i64,
                None::<i64>,
            ),
        )
        .await
        .unwrap();
    };
    insert(&[(2, "absent needle", false)]).await;
    set_note("é").await;
    insert(&[
        (3, "café", false),
        (4, "cafe\u{301}", false),
        (5, "été", false),
        (6, "plain", false),
    ])
    .await;
    set_note("").await;

    let selection = |right: &str| -> Selection {
        Selection {
            filters: RowFilters {
                compare: Some(ComparisonFilter {
                    field: "title".into(),
                    op: FilterComparison::Contains,
                    operand: right.into(),
                }),
                ..Default::default()
            },
            ..retained_selection("Document", "count")
        }
    };
    let original = selection("note");
    let start = async || {
        call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &original)
            .await
            .unwrap()
    };
    let resume = async |query: &Selection, handle: &[u8], sequence: u64| {
        resume_step(
            pic,
            canister,
            RetainedResume {
                selection: query.clone(),
                handle: handle.to_vec(),
                sequence,
                work: 2,
                bytes: 4096,
                page_size: 1,
                after: None,
            },
        )
        .await
    };
    let handle = start().await;
    let first = resume(&original, &handle, 0).await.unwrap();
    assert_eq!(
        (first.sequence, first.phase.label(), first.examined),
        (1, "Running", 2)
    );

    let before = pic.get_stable_memory(canister).await;
    upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(
        resume(&selection("title"), &handle, first.sequence)
            .await
            .unwrap_err(),
        "InvalidJob"
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(resume(&original, &handle, 0).await.unwrap(), first);
    let second = resume(&original, &handle, first.sequence).await.unwrap();
    assert_eq!((second.phase.label(), second.examined), ("Running", 4));

    let complete = resume(&original, &handle, second.sequence).await.unwrap();
    assert_eq!((complete.phase.label(), complete.examined), ("Complete", 6));
    // Empty needle in row 1, a suffix in 3, and two occurrences in 5 count once each.
    assert_eq!(completed(&complete).rows, [vec!["3"]]);

    let handle = start().await;
    let first = resume(&original, &handle, 0).await.unwrap();
    set_note("missing").await;
    let invalidated = resume(&original, &handle, first.sequence).await.unwrap();
    assert_eq!(invalidated.phase.label(), "Invalidated");

    owned.drop().await;
}

#[tokio::test]
async fn retained_decimal_rounding_uses_complete_totals_after_upgrade() {
    // Decimal inputs/expected outputs are literal decimal strings. The native
    // oracle does not import the engine's codec, accumulator or rounding helper.
    let maximum = "1701411834604692317316873037158841057.27";
    let minimum = "-1701411834604692317316873037158841057.28";
    let negative_maximum = "-1701411834604692317316873037158841057.27";
    let cases = [
        (
            [Some("1.24"), Some("1.25"), Some("1.24"), Some("1.25")],
            false,
            Some("1.24"),
        ),
        (
            [Some("1.25"), Some("1.26"), Some("1.25"), Some("1.26")],
            false,
            Some("1.26"),
        ),
        (
            [Some("-1.24"), Some("-1.25"), Some("-1.24"), Some("-1.25")],
            false,
            Some("-1.24"),
        ),
        (
            [Some("-1.25"), Some("-1.26"), Some("-1.25"), Some("-1.26")],
            false,
            Some("-1.26"),
        ),
        (
            [Some("0"), Some("0.01"), Some("0"), Some("0.01")],
            false,
            Some("0"),
        ),
        (
            [Some("0"), Some("-0.01"), Some("0"), Some("-0.01")],
            false,
            Some("0"),
        ),
        (
            [Some("1"), Some("1"), Some("1"), Some("3")],
            false,
            Some("1.5"),
        ),
        ([Some(maximum); 4], false, Some(maximum)),
        ([Some(minimum); 4], false, Some(minimum)),
        (
            [
                Some(maximum),
                Some(maximum),
                Some(negative_maximum),
                Some(negative_maximum),
            ],
            false,
            Some("0"),
        ),
        ([Some("1.24"), None, Some("1.25"), None], true, Some("1.24")),
        ([None, Some("0"), None, None], true, Some("0")),
        ([None; 4], true, None),
    ];
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;

    for (index, (values, optional, expected)) in cases.into_iter().enumerate() {
        let rows: Vec<_> = values
            .into_iter()
            .enumerate()
            .map(|(i, value)| {
                (
                    i as u64 + 1,
                    if optional {
                        value.map(str::to_owned)
                    } else {
                        None
                    },
                    if optional {
                        "0".to_owned()
                    } else {
                        value.unwrap().to_owned()
                    },
                )
            })
            .collect();
        call::<Result<(), u8>>(pic, canister, "decimal_batch", 67, &(rows, false))
            .await
            .unwrap();
        let field = if optional { "maybe" } else { "value" };
        let mut jobs = Vec::new();
        for grouped in [false, true] {
            let select = format!("{}avg:{field}", if grouped { "zone," } else { "" });
            let selection = retained_selection("DecimalRow", select);
            let handle =
                call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &selection)
                    .await
                    .unwrap();
            let first: Step = resume_step(
                pic,
                canister,
                RetainedResume {
                    selection: selection.clone(),
                    handle: handle.to_vec(),
                    sequence: 0,
                    work: 1,
                    bytes: 4096,
                    page_size: 1,
                    after: None,
                },
            )
            .await
            .unwrap();
            assert_eq!(
                (
                    first.sequence,
                    first.phase.label(),
                    first.examined,
                    first.output_bytes
                ),
                (1, "Running", 1, 0)
            );

            jobs.push((selection, handle, grouped));
        }
        let before = pic.get_stable_memory(canister).await;
        upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(pic.get_stable_memory(canister).await, before);
        for (selection, handle, grouped) in jobs {
            // Replaying the pre-upgrade sequence cannot rescan or round a prefix.
            let retry: Step = resume_step(
                pic,
                canister,
                RetainedResume {
                    selection: selection.clone(),
                    handle: handle.to_vec(),
                    sequence: 0,
                    work: 4,
                    bytes: 4096,
                    page_size: 1,
                    after: None,
                },
            )
            .await
            .unwrap();
            assert_eq!(
                (
                    retry.sequence,
                    retry.phase.label(),
                    retry.examined,
                    retry.output_bytes
                ),
                (1, "Running", 1, 0)
            );
            let step: Step = resume_step(
                pic,
                canister,
                RetainedResume {
                    selection: selection.clone(),
                    handle: handle.to_vec(),
                    sequence: retry.sequence,
                    work: 2,
                    bytes: 4096,
                    page_size: 1,
                    after: None,
                },
            )
            .await
            .unwrap();
            assert_eq!(
                (step.phase.label(), step.examined, step.output_bytes),
                ("Running", 3, 0)
            );

            let sequence = step.sequence;
            if index == 0 {
                let before = pic.get_stable_memory(canister).await;
                let refusal = resume_step(
                    pic,
                    canister,
                    RetainedResume {
                        selection: selection.clone(),
                        handle: handle.clone(),
                        sequence,
                        work: 4,
                        bytes: 1,
                        page_size: 1,
                        after: None,
                    },
                )
                .await;
                assert!(refusal.is_err());
                assert_eq!(pic.get_stable_memory(canister).await, before);
            }
            let complete: Step = resume_step(
                pic,
                canister,
                RetainedResume {
                    selection: selection.clone(),
                    handle: handle.to_vec(),
                    sequence,
                    work: 4,
                    bytes: 4096,
                    page_size: 1,
                    after: None,
                },
            )
            .await
            .unwrap();
            assert_eq!(
                (
                    complete.phase.label(),
                    complete.examined,
                    completed(&complete).next.clone()
                ),
                ("Complete", 4, None)
            );
            let expected_rows = if grouped {
                vec![vec!["true".to_owned(), expected.unwrap_or("—").to_owned()]]
            } else {
                expected
                    .map(|value| vec![vec![value.to_owned()]])
                    .unwrap_or_default()
            };
            assert_eq!(
                completed(&complete).rows,
                expected_rows,
                "case {index}, grouped={grouped}"
            );
            call::<Result<(), String>>(pic, canister, "aggregate_cancel", 122, &handle)
                .await
                .unwrap();
        }
    }
    eprintln!(
        "retained Decimal averages: 13 datasets, scalar/grouped pairs, 13 upgrades; signed ties, absence, wide totals, unequal chunks and output retry passed"
    );
    owned.drop().await;
}

#[tokio::test]
async fn retained_blobs_preserve_values_binding_and_cleanup_after_upgrade() {
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let replace = async |payload: Vec<u8>, optional: Option<Vec<u8>>| {
        call::<Result<(), u8>>(
            pic,
            canister,
            "replace_binary_object",
            14,
            &(
                (
                    ic_wire::ByteBuf::from(payload),
                    optional.map(ic_wire::ByteBuf::from),
                ),
                false,
            ),
        )
        .await
        .unwrap();
    };
    let advance = async |selection: &Selection, handle: &[u8], sequence: u64, bytes: u32| {
        resume_step(
            pic,
            canister,
            RetainedResume {
                selection: selection.clone(),
                handle: handle.to_vec(),
                sequence,
                work: 1,
                bytes,
                page_size: 1,
                after: None,
            },
        )
        .await
    };
    // Literal byte/display oracles, with more completed jobs than the 16-slot cap.
    for (payload, optional, hex, optional_hex) in [
        (vec![], None, "0x", None),
        (vec![0], Some(vec![]), "0x00", Some("0x")),
        (vec![0, 255, 128], Some(vec![255]), "0x00ff80", Some("0xff")),
    ] {
        replace(payload, optional).await;
        let present = if optional_hex.is_some() { "1" } else { "0" };
        let cases = [
            ("count", vec![vec!["1"]]),
            ("count:payload", vec![vec!["1"]]),
            ("count:optional", vec![vec![present]]),
            ("distinct-count:payload", vec![vec!["1"]]),
            ("distinct-count:optional", vec![vec!["1"]]),
            ("min:payload", vec![vec![hex]]),
            ("max:payload", vec![vec![hex]]),
            (
                "min:optional",
                optional_hex.into_iter().map(|v| vec![v]).collect(),
            ),
            (
                "max:optional",
                optional_hex.into_iter().map(|v| vec![v]).collect(),
            ),
            ("payload,count", vec![vec![hex, "1"]]),
            ("payload,count:optional", vec![vec![hex, present]]),
            (
                "optional,count",
                vec![vec![optional_hex.unwrap_or("—"), "1"]],
            ),
            (
                "optional,count:optional",
                vec![vec![optional_hex.unwrap_or("—"), present]],
            ),
        ];
        let mut jobs = Vec::new();
        for (selector, expected) in cases {
            let selection = retained_selection("BinaryObject", selector);
            let handle =
                call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &selection)
                    .await
                    .unwrap();
            let first = advance(&selection, &handle, 0, 4096).await.unwrap();
            assert_eq!(
                (first.sequence, first.phase.label(), first.examined),
                (1, "Complete", 1)
            );
            assert_eq!(completed(&first).rows, expected);
            jobs.push((selection, handle, expected));
        }
        let before = pic.get_stable_memory(canister).await;
        upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(pic.get_stable_memory(canister).await, before);
        for (selection, handle, expected) in jobs {
            if selection.select == "min:payload" {
                let before = pic.get_stable_memory(canister).await;
                let mut changed = selection.clone();
                changed.select = "max:payload".into();
                assert!(advance(&changed, &handle, 1, 4096).await.is_err());
                assert!(advance(&selection, &handle, 1, 1).await.is_err());
                assert_eq!(pic.get_stable_memory(canister).await, before);
            }
            let complete = advance(&selection, &handle, 1, 4096).await.unwrap();
            assert_eq!((complete.phase.label(), complete.examined), ("Complete", 1));
            assert_eq!(completed(&complete).rows, expected, "{}", selection.select);
            assert!(completed(&complete).next.is_none());
            call::<Result<(), String>>(pic, canister, "aggregate_cancel", 122, &handle)
                .await
                .unwrap();
        }
    }
    // Inclusive byte bounds select the input, including empty and non-UTF-8
    // bytes. A nonmatch still consumes work; it never becomes a partial answer.
    for (payload, lower, upper, prefix, suffix, matches) in [
        (vec![], Some(vec![]), Some(vec![]), None, true),
        (vec![0], None, Some(vec![]), None, false),
        (
            vec![0, 255, 128],
            Some(vec![0, 255, 128]),
            Some(vec![0, 255, 128]),
            None,
            true,
        ),
        (vec![0, 255, 128], None, Some(vec![0, 255]), None, false),
        (
            vec![0, 255, 128],
            Some(vec![0, 255, 129]),
            None,
            None,
            false,
        ),
        (vec![255], Some(vec![128]), None, None, true),
        (vec![128], Some(vec![255]), Some(vec![0]), None, false),
        (vec![], None, None, Some(vec![]), true),
        (vec![], None, None, Some(vec![0]), false),
        (vec![0, 255, 128], None, None, Some(vec![0, 255]), true),
        (vec![0, 255, 128], None, None, Some(vec![255]), false),
        (
            vec![0, 255, 128],
            None,
            Some(vec![0, 255]),
            Some(vec![0]),
            false,
        ),
        (
            vec![255, 255],
            Some(vec![255]),
            None,
            Some(vec![255, 255]),
            true,
        ),
    ]
    .into_iter()
    .map(|(payload, lower, upper, prefix, matches)| (payload, lower, upper, prefix, None, matches))
    .chain([
        (vec![], None, None, None, Some(vec![]), true),
        (vec![0], None, None, None, Some(vec![255]), false),
        (vec![0], None, None, None, Some(vec![0]), true),
        (
            vec![0, 255, 128],
            None,
            None,
            Some(vec![0]),
            Some(vec![255, 128]),
            true,
        ),
        (
            vec![0, 255, 128],
            None,
            Some(vec![0, 255]),
            None,
            Some(vec![128]),
            false,
        ),
        (vec![0, 255, 128], None, None, None, Some(vec![255]), false),
    ]) {
        let empty_range = lower
            .as_ref()
            .zip(upper.as_ref())
            .is_some_and(|(lo, hi)| lo > hi);
        replace(payload, None).await;
        let selection = Selection {
            filters: RowFilters {
                payload: PayloadFilters {
                    minimum: lower,
                    maximum: upper,
                    prefix,
                    suffix,
                    contains: None,
                },
                ..Default::default()
            },
            ..retained_selection("BinaryObject", "count")
        };
        let handle =
            call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &selection)
                .await
                .unwrap();
        let first = advance(&selection, &handle, 0, 4096).await.unwrap();
        if empty_range {
            assert_eq!((first.phase.label(), first.examined), ("Complete", 0));
            assert_eq!(completed(&first).rows, vec![vec!["0"]]);
        } else {
            assert_eq!((first.phase.label(), first.examined), ("Complete", 1));
            assert_eq!(
                completed(&first).rows,
                [vec![if matches { "1" } else { "0" }]]
            );
        }
        upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        let before = pic.get_stable_memory(canister).await;
        let mut changed = selection.clone();
        changed.filters.payload.contains = Some(vec![255]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed = selection.clone();
        changed.filters.payload.suffix = Some(vec![42]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed.filters.payload.suffix = Some(vec![0; 1025]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed = selection.clone();
        changed.filters.payload.prefix = Some(vec![42]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed.filters.payload.prefix = Some(vec![0; 1025]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed.filters.payload = PayloadFilters {
            minimum: Some(vec![1, 2, 3]),
            ..Default::default()
        };
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed.filters.payload = PayloadFilters {
            minimum: Some(vec![0; 1025]),
            ..Default::default()
        };
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        assert!(
            call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &changed)
                .await
                .is_err()
        );
        changed.entity = "Document".into();
        changed.filters.payload = PayloadFilters {
            minimum: Some(vec![]),
            ..Default::default()
        };
        assert!(
            call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &changed)
                .await
                .is_err()
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let complete = advance(&selection, &handle, 1, 4096).await.unwrap();
        assert_eq!(
            (complete.phase.label(), complete.examined),
            ("Complete", u64::from(!empty_range))
        );
        assert_eq!(
            completed(&complete).rows,
            vec![vec![if matches { "1" } else { "0" }]]
        );
        call::<Result<(), String>>(pic, canister, "aggregate_cancel", 122, &handle)
            .await
            .unwrap();
    }
    let selection = retained_selection("BinaryObject", "max:payload");
    let handle = call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &selection)
        .await
        .unwrap();
    // Mutate before the only candidate is consumed; the job is still running.
    replace(vec![1], None).await;
    let changed = advance(&selection, &handle, 0, 4096).await.unwrap();
    assert_eq!(changed.phase, RetainedPhase::Invalidated);

    call::<Result<(), String>>(pic, canister, "aggregate_cancel", 122, &handle)
        .await
        .unwrap();
    let before = pic.get_stable_memory(canister).await;
    let mut invalid = selection;
    invalid.select = "sum:payload".into();
    assert!(
        call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &invalid)
            .await
            .is_err()
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.drop().await;
}

#[tokio::test]
#[ignore = "requires the measured-query Wasm built by make m2-sort-costs-artifact"]
async fn literal_text_scan_cost() {
    type Page = (
        Vec<String>,
        Vec<Vec<String>>,
        Option<Vec<u8>>,
        u32,
        u32,
        u32,
    );
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    let common = "a".repeat(32);
    let rare = format!("{}!", "a".repeat(31));
    for start in (1..=256).step_by(8) {
        let rows = (start..start + 8)
            .map(|id: u64| (id, if id == 256 { &rare } else { &common }, false))
            .collect::<Vec<_>>();
        call::<Result<(), u8>>(pic, canister, "text_index_batch", 23, &(rows, false))
            .await
            .unwrap();
    }
    let before = pic.get_stable_memory(canister).await;
    for (op, needle, expected) in [
        (None, "", "256"),
        (Some(FilterComparison::Eq), rare.as_str(), "1"),
        (Some(FilterComparison::Contains), "!", "1"),
        (Some(FilterComparison::Contains), rare.as_str(), "1"),
    ] {
        let filters = RowFilters {
            text: op.map(|op| ComparisonFilter {
                field: "title".into(),
                op,
                operand: needle.into(),
            }),
            ..Default::default()
        };
        let request = (
            "Document",
            None::<Vec<u8>>,
            32_u32,
            None::<u64>,
            None::<u64>,
            None::<u64>,
            Vec::<String>::new(),
            false,
            512_u32,
            4096_u32,
            "count",
            (
                false,
                (None::<String>, None::<String>),
                None::<String>,
                filters,
            ),
        );
        let (page, instructions): (Result<Page, String>, u64) =
            call(pic, canister, "measured_query", 147, &request).await;
        let page = page.unwrap();
        assert_eq!(page.1, [vec![expected]]);
        assert_eq!((page.3, page.4), (256, 256));
        assert_eq!(pic.get_stable_memory(canister).await, before);
        eprintln!(
            "text-scan op={op:?} needle_bytes={} instructions={instructions}",
            needle.len()
        );
    }
    // Measure identical filters and output across both pages. The fixture's
    // primary order happens to agree with title/ID order for the three matches.
    let rows = [
        (255_u64, "aardvark", true),
        (256, "zulu", true),
        (257, "special-a", true),
        (258, "special-b", true),
        (259, "special-b", true),
        (260, "special-a", false),
    ];
    call::<Result<(), u8>>(
        pic,
        canister,
        "text_index_batch",
        23,
        &(rows.as_slice(), false),
    )
    .await
    .unwrap();
    let before = pic.get_stable_memory(canister).await;
    for indexed in [false, true] {
        let mut cursor: Option<Vec<u8>> = None;
        for (page_number, expected) in [vec!["257", "258"], vec!["259"]].into_iter().enumerate() {
            let filters = RowFilters {
                boolean: Some(("published".into(), true)),
                text: Some(ComparisonFilter {
                    field: "title".into(),
                    op: FilterComparison::StartsWith,
                    operand: "special-".into(),
                }),
                ..Default::default()
            };
            let request = (
                "Document",
                cursor.take(),
                2_u32,
                None::<u64>,
                None::<u64>,
                None::<u64>,
                if indexed { vec!["title"] } else { vec![] },
                false,
                512_u32,
                4096_u32,
                "id",
                (
                    false,
                    (None::<String>, None::<String>),
                    None::<String>,
                    filters,
                ),
            );
            let (page, instructions): (Result<Page, String>, u64) =
                call(pic, canister, "measured_query", 147, &request).await;
            let page = page.unwrap();
            assert_eq!(
                page.1,
                expected.iter().map(|id| vec![*id]).collect::<Vec<_>>()
            );
            let examined = if indexed {
                expected.len() as u32
            } else {
                [258, 2][page_number]
            };
            assert_eq!(
                (page.3, page.4, page.5),
                (examined, examined, 8 * expected.len() as u32)
            );
            cursor = page.2;
            assert_eq!(cursor.is_some(), page_number == 0);
            eprintln!(
                "prefix-page indexed={indexed} page={} examined={examined} instructions={instructions}",
                page_number + 1
            );
        }
    }
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.drop().await;
}

#[tokio::test]
#[ignore = "matched Document update-call cycle measurement"]
async fn document_replacement_costs() {
    type Document = (String, Option<String>, bool, Option<u64>, i64, Option<i64>);
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let pic = instance().await.unwrap();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    let (mut row, mut revision): (Document, u64) = call(&pic, canister, "document", 7, &()).await;
    for shape in ["note", "one_index", "all_three_indexes"] {
        match shape {
            "note" => row.1 = Some("updated".into()),
            "one_index" => row.0.push('!'),
            _ => {
                row.0.push('!');
                row.4 += 1;
                row.3 = None;
            }
        }
        let before = pic.cycle_balance(canister).await;
        call::<Result<(), u8>>(&pic, canister, "replace_document", 8, &row)
            .await
            .unwrap();
        let charged = before - pic.cycle_balance(canister).await;
        let (stored, updated): (Document, u64) = call(&pic, canister, "document", 7, &()).await;
        assert_eq!(stored, row);
        assert_eq!(updated, revision + 1);
        revision = updated;
        eprintln!("document-replace shape={shape} cycles={charged}");
    }
    pic.drop().await;
}
