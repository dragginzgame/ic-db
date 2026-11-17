//! Retained Decimal rounding and Blob results through the console, across heap loss.
use ic_db_m0_rollback_driver::{LIMITS, instance, upgrade_stopped};
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
    (
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
    ),
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
                (None, None, None, None),
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
                (None, None, None, None),
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
            (lower, upper, prefix, suffix),
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
        changed.6.3 = Some(vec![42]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed.6.3 = Some(vec![0; 1025]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed = selection.clone();
        changed.6.2 = Some(vec![42]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed.6.2 = Some(vec![0; 1025]);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed.6 = (Some(vec![1, 2, 3]), None, None, None);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        changed.6 = (Some(vec![0; 1025]), None, None, None);
        assert!(advance(&changed, &handle, 1, 4096).await.is_err());
        assert!(
            call::<Result<Vec<u8>, String>>(pic, canister, "aggregate_start", 120, &changed)
                .await
                .is_err()
        );
        changed.0 = "Document".into();
        changed.6 = (Some(vec![]), None, None, None);
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
        (None, None, None, None),
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
