//! Retained filters, Decimal rounding and Blob results through the console across upgrades.
use ic_db_m0_rollback_driver::{LIMITS, instance, upgrade_stopped};
use ic_db_qualification_transport::RowFilters;
use ic_testkit::pocket_ic::{CanisterId, nonblocking::PocketIc};
use ic_wire::{Direction, Encoding};
use serde::{Serialize, de::DeserializeOwned};

type Selection = (
    String,
    String,
    (Option<u64>, Option<u64>, Option<u64>),
    u32,
    (Option<String>, Option<String>),
    Option<String>,
    RowFilters,
);
type Step = (
    u64,
    String,
    u64,
    u64,
    u64,
    (Vec<String>, Vec<Vec<String>>, Option<Vec<u8>>),
);

async fn call<T: DeserializeOwned>(
    pic: &PocketIc,
    canister: CanisterId,
    name: &str,
    method: u64,
    value: &impl Serialize,
) -> T {
    let bytes = ic_wire::encode(
        value,
        Encoding::Frame {
            method,
            direction: Direction::Request,
        },
        LIMITS,
    )
    .unwrap();
    let reply = pic
        .update_call(canister, canister, name, bytes)
        .await
        .unwrap();
    ic_wire::decode(
        &reply,
        Encoding::Frame {
            method,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap()
}

#[tokio::test]
async fn retained_field_substrings_bind_the_needle_and_survive_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-cli")).unwrap();
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
        (
            "Document".into(),
            "count".into(),
            (None, None, None),
            32,
            (None, None),
            None,
            RowFilters {
                compare: Some(["title".into(), "contains".into(), right.into()]),
                ..Default::default()
            },
        )
    };
    let original = selection("note");
    let start = async || {
        call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &original)
            .await
            .unwrap()
    };
    let resume = async |query: &Selection, handle: &[u8], sequence: u64| {
        call::<Result<Step, String>>(
            pic,
            canister,
            "aggregate_resume",
            121,
            &(
                query,
                handle,
                sequence,
                2_u32,
                4096_u32,
                1_u32,
                None::<Vec<u8>>,
            ),
        )
        .await
    };
    let handle = start().await;
    let first = resume(&original, &handle, 0).await.unwrap();
    assert_eq!((first.0, first.1.as_str(), first.2), (1, "Running", 2));
    assert!(first.5.1.is_empty());
    let before = pic.get_stable_memory(canister).await;
    upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(
        resume(&selection("title"), &handle, first.0)
            .await
            .unwrap_err(),
        "InvalidJob"
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(resume(&original, &handle, 0).await.unwrap(), first);
    let mut sequence = first.0;
    for examined in [4, 6] {
        let next = resume(&original, &handle, sequence).await.unwrap();
        assert_eq!((next.1.as_str(), next.2), ("Running", examined));
        assert!(next.5.1.is_empty());
        sequence = next.0;
    }
    let complete = resume(&original, &handle, sequence).await.unwrap();
    assert_eq!((complete.1.as_str(), complete.2), ("Complete", 6));
    // Empty needle in row 1, a suffix in 3, and two occurrences in 5 count once each.
    assert_eq!(complete.5.1, [vec!["3"]]);

    let handle = start().await;
    let first = resume(&original, &handle, 0).await.unwrap();
    set_note("missing").await;
    let invalidated = resume(&original, &handle, first.0).await.unwrap();
    assert_eq!(invalidated.1, "Invalidated");
    assert!(invalidated.5.1.is_empty());
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
    let wasm = std::fs::read(
        std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("build with make m2-cli-artifact"),
    )
    .unwrap();
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
            let selection: Selection = (
                "DecimalRow".into(),
                select,
                (None, None, None),
                32,
                (None, None),
                None,
                RowFilters::default(),
            );
            let handle =
                call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &selection)
                    .await
                    .unwrap();
            let first: Step = call::<Result<Step, String>>(
                pic,
                canister,
                "aggregate_resume",
                121,
                &(
                    &selection,
                    &handle,
                    0_u64,
                    1_u32,
                    4096_u32,
                    1_u32,
                    None::<Vec<u8>>,
                ),
            )
            .await
            .unwrap();
            assert_eq!(
                (first.0, first.1.as_str(), first.2, first.4),
                (1, "Running", 1, 0)
            );
            assert!(first.5.1.is_empty() && first.5.2.is_none());
            jobs.push((selection, handle, grouped));
        }
        let before = pic.get_stable_memory(canister).await;
        upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(pic.get_stable_memory(canister).await, before);
        for (selection, handle, grouped) in jobs {
            // Replaying the pre-upgrade sequence cannot rescan or round a prefix.
            let retry: Step = call::<Result<Step, String>>(
                pic,
                canister,
                "aggregate_resume",
                121,
                &(
                    &selection,
                    &handle,
                    0_u64,
                    4_u32,
                    4096_u32,
                    1_u32,
                    None::<Vec<u8>>,
                ),
            )
            .await
            .unwrap();
            assert_eq!(
                (retry.0, retry.1.as_str(), retry.2, retry.4),
                (1, "Running", 1, 0)
            );
            let mut sequence = retry.0;
            for (work, examined) in [(2_u32, 3), (1_u32, 4)] {
                let step: Step = call::<Result<Step, String>>(
                    pic,
                    canister,
                    "aggregate_resume",
                    121,
                    &(
                        &selection,
                        &handle,
                        sequence,
                        work,
                        4096_u32,
                        1_u32,
                        None::<Vec<u8>>,
                    ),
                )
                .await
                .unwrap();
                assert_eq!((step.1.as_str(), step.2, step.4), ("Running", examined, 0));
                assert!(step.5.1.is_empty());
                sequence = step.0;
            }
            if index == 0 {
                let before = pic.get_stable_memory(canister).await;
                let refusal: Result<Step, String> = call(
                    pic,
                    canister,
                    "aggregate_resume",
                    121,
                    &(
                        &selection,
                        &handle,
                        sequence,
                        4_u32,
                        1_u32,
                        1_u32,
                        None::<Vec<u8>>,
                    ),
                )
                .await;
                assert!(refusal.is_err());
                assert_eq!(pic.get_stable_memory(canister).await, before);
            }
            let complete: Step = call::<Result<Step, String>>(
                pic,
                canister,
                "aggregate_resume",
                121,
                &(
                    &selection,
                    &handle,
                    sequence,
                    4_u32,
                    4096_u32,
                    1_u32,
                    None::<Vec<u8>>,
                ),
            )
            .await
            .unwrap();
            assert_eq!(
                (complete.1.as_str(), complete.2, complete.5.2),
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
                complete.5.1, expected_rows,
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
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-cli")).unwrap();
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
        call::<Result<Step, String>>(
            pic,
            canister,
            "aggregate_resume",
            121,
            &(
                selection,
                handle,
                sequence,
                1_u32,
                bytes,
                1_u32,
                None::<Vec<u8>>,
            ),
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
            let selection: Selection = (
                "BinaryObject".into(),
                selector.into(),
                (None, None, None),
                32,
                (None, None),
                None,
                RowFilters::default(),
            );
            let handle =
                call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &selection)
                    .await
                    .unwrap();
            let first = advance(&selection, &handle, 0, 4096).await.unwrap();
            assert_eq!(
                (first.0, first.1.as_str(), first.2, first.4),
                (1, "Running", 1, 0)
            );
            assert!(first.5.1.is_empty());
            jobs.push((selection, handle, expected));
        }
        let before = pic.get_stable_memory(canister).await;
        upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(pic.get_stable_memory(canister).await, before);
        for (selection, handle, expected) in jobs {
            if selection.1 == "min:payload" {
                let before = pic.get_stable_memory(canister).await;
                let mut changed = selection.clone();
                changed.1 = "max:payload".into();
                assert!(advance(&changed, &handle, 1, 4096).await.is_err());
                assert!(advance(&selection, &handle, 1, 1).await.is_err());
                assert_eq!(pic.get_stable_memory(canister).await, before);
            }
            let complete = advance(&selection, &handle, 1, 4096).await.unwrap();
            assert_eq!((complete.1.as_str(), complete.2), ("Complete", 1));
            assert_eq!(complete.5.1, expected, "{}", selection.1);
            assert!(complete.5.2.is_none());
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
        replace(payload, None).await;
        let selection: Selection = (
            "BinaryObject".into(),
            "count".into(),
            (None, None, None),
            32,
            (None, None),
            None,
            RowFilters {
                payload: [lower, upper, prefix, suffix, None],
                ..Default::default()
            },
        );
        let handle =
            call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &selection)
                .await
                .unwrap();
        let first = advance(&selection, &handle, 0, 4096).await.unwrap();
        assert_eq!((first.1.as_str(), first.2), ("Running", 1));
        assert!(first.5.1.is_empty());
        upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        let before = pic.get_stable_memory(canister).await;
        let mut changed = selection.clone();
        changed.6.payload[4] = Some(vec![255]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed = selection.clone();
        changed.6.payload[3] = Some(vec![42]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed.6.payload[3] = Some(vec![0; 1025]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed = selection.clone();
        changed.6.payload[2] = Some(vec![42]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed.6.payload[2] = Some(vec![0; 1025]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed.6.payload = [Some(vec![1, 2, 3]), None, None, None, None];
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed.6.payload = [Some(vec![0; 1025]), None, None, None, None];
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        assert!(
            call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &changed)
                .await
                .is_err()
        );
        changed.0 = "Document".into();
        changed.6.payload = [Some(vec![]), None, None, None, None];
        assert!(
            call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &changed)
                .await
                .is_err()
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let complete = advance(&selection, &handle, 1, 4096).await.unwrap();
        assert_eq!((complete.1.as_str(), complete.2), ("Complete", 1));
        assert_eq!(complete.5.1, vec![vec![if matches { "1" } else { "0" }]]);
        call::<Result<(), String>>(pic, canister, "aggregate_cancel", 122, &handle)
            .await
            .unwrap();
    }
    let selection: Selection = (
        "BinaryObject".into(),
        "max:payload".into(),
        (None, None, None),
        32,
        (None, None),
        None,
        RowFilters::default(),
    );
    let handle = call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &selection)
        .await
        .unwrap();
    advance(&selection, &handle, 0, 4096).await.unwrap();
    replace(vec![1], None).await;
    let changed = advance(&selection, &handle, 1, 4096).await.unwrap();
    assert_eq!(changed.1, "Invalidated");
    assert!(changed.5.1.is_empty());
    call::<Result<(), String>>(pic, canister, "aggregate_cancel", 122, &handle)
        .await
        .unwrap();
    let before = pic.get_stable_memory(canister).await;
    let mut invalid = selection;
    invalid.1 = "sum:payload".into();
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
    let wasm = std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").unwrap()).unwrap();
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
        ("", "", "256"),
        ("eq", rare.as_str(), "1"),
        ("contains", "!", "1"),
        ("contains", rare.as_str(), "1"),
    ] {
        let filters = RowFilters {
            text: (!op.is_empty()).then(|| ["title".into(), op.into(), needle.into()]),
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
                text: Some(["title".into(), "starts-with".into(), "special-".into()]),
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
    let wasm = std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").unwrap()).unwrap();
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

#[tokio::test]
#[ignore = "requires the query-measurements Wasm; run make m2-listing-costs"]
async fn ordered_listing_paths_match_sqlite_page_by_page() {
    type Sample = (Vec<u64>, Option<Vec<u8>>, u32, u32, u64);
    let wasm = std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").unwrap()).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    let sql = rusqlite::Connection::open_in_memory().unwrap();
    sql.execute_batch("CREATE TABLE docs(id INTEGER PRIMARY KEY, entry_id INTEGER, score INTEGER, published INTEGER);
        INSERT INTO docs VALUES (1,1,-10,0);").unwrap();
    let set_template = async |visible| {
        call::<Result<(), u8>>(
            pic,
            canister,
            "replace_document",
            8,
            &(
                "template",
                None::<String>,
                visible,
                None::<u64>,
                -10_i64,
                None::<i64>,
            ),
        )
        .await
        .unwrap();
    };
    // Move the visible template to entry 2 and seed a high-scoring distractor.
    set_template(true).await;
    call::<Result<(), u8>>(pic, canister, "reference_batch", 10, &3_u8)
        .await
        .unwrap();
    let outside = [(
        2_u64,
        ic_wire::ByteBuf::from(2_u64.to_be_bytes().to_vec()),
        99_i64,
    )];
    call::<Result<(), u8>>(pic, canister, "indexed_batch", 21, &(&outside[..], false))
        .await
        .unwrap();
    sql.execute("INSERT INTO docs VALUES (2,2,99,1)", [])
        .unwrap();
    call::<Result<(), u8>>(pic, canister, "write", 4, &(1_u64, 1_u64, 11_u64, 1_u8))
        .await
        .unwrap();

    // replace_document selects entry 1; indexed_batch copies its visibility.
    for visible in [true, false] {
        set_template(visible).await;
        let mut rows = Vec::new();
        for id in 3..=130_u64 {
            let score = (id % 12) as i64;
            if (score % 4 == 0 || id % 13 == 0) == visible {
                rows.push((id, ic_wire::ByteBuf::from(id.to_be_bytes().to_vec()), score));
                sql.execute(
                    "INSERT INTO docs VALUES (?1,1,?2,?3)",
                    rusqlite::params![id as i64, score, visible],
                )
                .unwrap();
            }
        }
        for chunk in rows.chunks(8) {
            call::<Result<(), u8>>(pic, canister, "indexed_batch", 21, &(chunk, false))
                .await
                .unwrap();
        }
    }
    let expected = sql
        .prepare("SELECT id FROM docs WHERE entry_id=1 AND published=1 ORDER BY score DESC,id DESC")
        .unwrap()
        .query_map([], |r| r.get::<_, u32>(0).map(u64::from))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let before = pic.get_stable_memory(canister).await;
    let page = async |hinted: bool, cursor: &Option<Vec<u8>>, work: u32, bytes: u32| {
        call::<Sample>(
            pic,
            canister,
            "measured_listing",
            189,
            &(hinted, cursor, work, bytes),
        )
        .await
    };
    for (shape, work, bytes) in [("full", 128, 160), ("sparse", 5, 160), ("byte", 32, 16)] {
        let (mut auto, mut hint) = (None, None);
        let mut offset = 0;
        let mut saw_boundary = false;
        for number in 1..=150 {
            let a = page(false, &auto, work, bytes).await;
            let h = page(true, &hint, work, bytes).await;
            assert_eq!(
                (&a.0, a.1.is_some(), a.2, a.3),
                (&h.0, h.1.is_some(), h.2, h.3)
            );
            let end = offset + a.0.len();
            assert_eq!(a.0, expected[offset..end]);
            assert!(a.2 <= work && a.3 <= a.2);
            assert!(a.0.len() <= 20 && a.0.len() * 8 <= bytes as usize);
            println!(
                "listing-page,{shape},{number},{},{},{},{},{}",
                a.0.len(),
                a.2,
                a.3,
                a.4,
                h.4
            );
            saw_boundary |= match shape {
                "full" => a.0.len() == 20,
                "sparse" => a.0.is_empty() && a.1.is_some() && a.2 == work,
                "byte" if a.0.len() == 2 && a.1.is_some() => {
                    let wider = page(false, &auto, work, 160).await;
                    wider
                        .0
                        .get(2)
                        .is_some_and(|id| Some(id) == expected.get(end))
                }
                _ => false,
            };
            offset = end;
            if a.1.is_none() {
                break;
            }
            assert!(number < 150 && a.1 != auto && h.1 != hint);
            auto = a.1;
            hint = h.1;
        }
        assert_eq!(offset, expected.len());
        assert!(saw_boundary, "{shape} fixture missed its page boundary");
    }
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.drop().await;
}
