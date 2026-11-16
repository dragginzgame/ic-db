//! One fixed scalar workload and independent tuple oracle for both engines.

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

type Row = (u64, u64, u64);

pub async fn run(label: &str, wasm_path: &std::ffi::OsStr, needs_settling: bool, fluent: bool) {
    let (wasm, facts) = crate::artifact::read(std::fs::File::open(wasm_path).unwrap()).unwrap();
    let methods: Vec<_> = facts
        .exports
        .iter()
        .map(String::as_str)
        .filter(|name| name.starts_with("canister_query ") || name.starts_with("canister_update "))
        .collect();
    let mut expected_methods = vec![
        "canister_query listing",
        "canister_query lookup",
        "canister_update advance_startup",
        "canister_update insert",
        "canister_update read_modify_write",
    ];
    if fluent {
        expected_methods.insert(0, "canister_query fluent_listing");
    }
    assert_eq!(
        methods, expected_methods,
        "unexpected reachable endpoint in {label} benchmark"
    );
    eprintln!(
        "{label} exports={:?}",
        facts.exports.iter().collect::<Vec<_>>()
    );
    eprintln!(
        "{label} wasm_bytes={} code_section_bytes={} sha256={}",
        facts.identity.bytes,
        facts.code_section_bytes,
        facts
            .identity
            .sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );

    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    let before_install = pic.cycle_balance(canister).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    eprintln!(
        "{label} installation_cycles={}",
        before_install
            .checked_sub(pic.cycle_balance(canister).await)
            .unwrap()
    );

    let startup = async || {
        let before = pic.cycle_balance(canister).await;
        let mut instructions = 0;
        for step in 1..=100 {
            let reply = pic
                .update_call(
                    canister,
                    canister,
                    "advance_startup",
                    ic_wire::encode(
                        &(),
                        Encoding::Frame {
                            method: 1,
                            direction: Direction::Request,
                        },
                        LIMITS,
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            let (ready, work): (bool, u64) = ic_wire::decode(
                &reply,
                Encoding::Frame {
                    method: 1,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap();
            instructions += work;
            if ready {
                return (
                    step,
                    instructions,
                    before
                        .checked_sub(pic.cycle_balance(canister).await)
                        .unwrap(),
                );
            }
        }
        panic!("{label} startup did not finish within 100 messages");
    };
    // IC DB has no deferred ordinary-write work. Do not invent a settling call
    // or hide IcyDB's required follow-up calls from complete-journey totals.
    let settle = async || {
        if needs_settling {
            startup().await
        } else {
            (0, 0, 0)
        }
    };
    let insert = async |value: Row| {
        let before = pic.cycle_balance(canister).await;
        let reply = pic
            .update_call(
                canister,
                canister,
                "insert",
                ic_wire::encode(
                    &value,
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
        let cycles = before
            .checked_sub(pic.cycle_balance(canister).await)
            .unwrap();
        let (accepted, instructions): (bool, u64) = ic_wire::decode(
            &reply,
            Encoding::Frame {
                method: 2,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap();
        (accepted, instructions, cycles)
    };
    let lookup = async |id: u64| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "lookup",
                ic_wire::encode(
                    &id,
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
        ic_wire::decode::<(Option<Row>, u64)>(
            &reply,
            Encoding::Frame {
                method: 3,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let listing = async |category: u64| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "listing",
                ic_wire::encode(
                    &category,
                    Encoding::Frame {
                        method: 5,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<(Vec<Row>, u64)>(
            &reply,
            Encoding::Frame {
                method: 5,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    eprintln!(
        "{label} startup_steps_instructions_cycles={:?}",
        startup().await
    );

    let mut source = Vec::new();
    let mut insert_instructions = Vec::new();
    let mut insert_cycles = Vec::new();
    let mut settling = Vec::new();
    for id in 1..=32 {
        let value = (id, id % 4, id * 10);
        let (accepted, instructions, cycles) = insert(value).await;
        assert!(accepted);
        source.push(value);
        insert_instructions.push(instructions);
        insert_cycles.push(cycles);
        settling.push(settle().await);
    }
    eprintln!("{label} insert_instructions={insert_instructions:?}");
    eprintln!("{label} insert_cycles={insert_cycles:?}");
    eprintln!("{label} insert_settling_steps_instructions_cycles={settling:?}");

    let (found, instructions) = lookup(7).await;
    assert_eq!(found, Some((7, 3, 70)));
    eprintln!("{label} lookup_hit_instructions={instructions}");
    let (found, instructions) = lookup(99).await;
    assert_eq!(found, None);
    eprintln!("{label} lookup_miss_instructions={instructions}");

    let (accepted, instructions, cycles) = insert((7, 0, 999)).await;
    assert!(!accepted);
    eprintln!("{label} rejected_insert_settling={:?}", settle().await);
    assert_eq!(lookup(7).await.0, Some((7, 3, 70)));
    eprintln!("{label} rejected_insert_instructions={instructions} cycles={cycles}");

    let (rows, instructions) = listing(3).await;
    assert_eq!(
        rows,
        source
            .iter()
            .copied()
            .filter(|row| row.1 == 3)
            .collect::<Vec<_>>()
    );
    eprintln!("{label} indexed_listing_instructions={instructions}");

    let replacement: Row = (7, 0, 707);
    let before = pic.cycle_balance(canister).await;
    let reply = pic
        .update_call(
            canister,
            canister,
            "read_modify_write",
            ic_wire::encode(
                &replacement,
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
    let cycles = before
        .checked_sub(pic.cycle_balance(canister).await)
        .unwrap();
    let (value, instructions): (Option<Row>, u64) = ic_wire::decode(
        &reply,
        Encoding::Frame {
            method: 4,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap();
    assert_eq!(value, Some(replacement));
    eprintln!("{label} replacement_settling={:?}", settle().await);
    *source.iter_mut().find(|row| row.0 == 7).unwrap() = replacement;
    assert_eq!(lookup(7).await.0, Some(replacement));
    for category in [0, 3] {
        assert_eq!(
            listing(category).await.0,
            source
                .iter()
                .copied()
                .filter(|row| row.1 == category)
                .collect::<Vec<_>>()
        );
    }
    eprintln!("{label} read_modify_replace_instructions={instructions} cycles={cycles}");

    let compare_fluent = async |phase: &str| {
        if !fluent {
            return;
        }
        // Compare only the first requested rows on unchanged data. The engines'
        // cursor/limit contracts differ; this is not a continuation parity test.
        for (mode, category, minimum, limit) in [
            (0_u8, 3_u64, 0_u64, 8_u32),
            (0, 3, 200, 8),
            (0, 99, 0, 8),
            (1, 3, 200, 8),
            (2, 0, 700, 8),
            (2, 0, 999, 8),
            (3, 0, 0, 8),
            (3, 0, 200, 1),
            (3, 0, 0, 32),
        ] {
            let mut expected: Vec<Row> = source
                .iter()
                .copied()
                // For mode 2, category >= 0 includes the full Nat64 domain.
                .filter(|row| row.2 >= minimum && (mode == 2 || row.1 == category))
                .collect();
            expected.sort_by(|left, right| match mode {
                1 => right.0.cmp(&left.0),
                2 => (left.1, left.0).cmp(&(right.1, right.0)),
                3 => right.2.cmp(&left.2).then(left.0.cmp(&right.0)),
                _ => left.0.cmp(&right.0),
            });
            expected.truncate(limit as usize);
            let backing = pic.get_stable_memory(canister).await;
            let reply = pic
                .query_call(
                    canister,
                    canister,
                    "fluent_listing",
                    ic_wire::encode(
                        &(mode, category, minimum, limit),
                        Encoding::Frame {
                            method: 6,
                            direction: Direction::Request,
                        },
                        LIMITS,
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            let (result, instructions): (Result<Vec<Row>, u16>, u64) = ic_wire::decode(
                &reply,
                Encoding::Frame {
                    method: 6,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap();
            if label == "icydb" && mode == 3 {
                // PublicRead forbids this materialized order. Do not silently
                // switch to a trusted lane or count refusal as query savings.
                assert_eq!(result, Err(174), "legacy materialization admission");
            } else {
                assert_eq!(result, Ok(expected), "{label}: {phase}, shape {mode}");
            }
            assert_eq!(pic.get_stable_memory(canister).await, backing);
            eprintln!(
                "{label} fluent phase={phase} mode={mode} category={category} minimum={minimum} limit={limit} result={:?} instructions={instructions}",
                result.as_ref().map(Vec::len)
            );
        }
    };
    compare_fluent("settled").await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    eprintln!(
        "{label} reopen_steps_instructions_cycles={:?}",
        startup().await
    );
    compare_fluent("settled-after-upgrade").await;
    // Independent expected tuples survive heap loss and durable startup recovery.
    for expected in &source {
        assert_eq!(lookup(expected.0).await.0, Some(*expected));
    }
    for category in 0..4 {
        assert_eq!(
            listing(category).await.0,
            source
                .iter()
                .copied()
                .filter(|row| row.1 == category)
                .collect::<Vec<_>>()
        );
    }
    eprintln!(
        "{label} stable_memory_bytes={}",
        pic.get_stable_memory(canister).await.len()
    );
    owned.close().await;
}
