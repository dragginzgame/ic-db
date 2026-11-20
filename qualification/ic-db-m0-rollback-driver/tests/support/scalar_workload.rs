//! One fixed scalar workload and independent tuple oracle for both engines.

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

type Row = (u64, u64, u64);

pub async fn run(label: &str, wasm_path: &std::ffi::OsStr, needs_settling: bool) {
    let fluent = std::env::var_os("IC_DB_FLUENT_COMPARISON").is_some();
    let keyset = std::env::var_os("IC_DB_KEYSET_COMPARISON").is_some();
    let aggregate = std::env::var_os("IC_DB_AGGREGATE_COMPARISON").is_some();
    let profile = std::env::var_os("IC_DB_COUNT_PROFILE").is_some();
    let counts = std::env::var_os("IC_DB_COUNT_COMPARISON").is_some();
    let prefix_candidate = label == "ic-db" && std::env::var_os("IC_DB_PREFIX_CANDIDATE").is_some();
    assert!(!prefix_candidate || (counts && !profile));
    assert!(
        [fluent, keyset, aggregate, counts]
            .into_iter()
            .filter(|selected| *selected)
            .count()
            == 1,
        "select one comparison workload"
    );
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
    if keyset {
        expected_methods.insert(0, "canister_query keyset_listing");
    }
    if fluent {
        expected_methods.insert(0, "canister_query fluent_listing");
    }
    if aggregate {
        expected_methods.insert(0, "canister_query grouped_aggregate");
    }
    if counts {
        expected_methods.insert(0, "canister_query count_rows");
    }
    if profile {
        assert!(counts && label == "ic-db");
        expected_methods.insert(0, "canister_query count_profile");
    }
    if prefix_candidate {
        expected_methods.insert(3, "canister_query prefix_count");
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
    let pic = &owned;
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
        let (accepted, _): (bool, u64) = ic_wire::decode(
            &reply,
            Encoding::Frame {
                method: 2,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap();

        accepted
    };
    let replace = async |replacement: Row| {
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

        (value, instructions, cycles)
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

    if keyset {
        // Final item state of the skewed application after its move, seeded
        // directly: the timed workload contains reads on immutable input.
        let mut expected = Vec::new();
        let mut sparse = Vec::new();
        for id in 1..=128 {
            let category = if id % 8 == 0 && id != 128 { 2 } else { 1 };
            let amount = if id == 128 { 10 } else { id % 11 };
            let value = (id, category, amount);
            assert!(insert(value).await);
            settle().await;

            if category == 1 && amount >= 10 {
                expected.push(value);
            } else if category == 2 && amount >= 10 {
                sparse.push(value);
            }
        }
        assert_eq!(expected.len(), 10);
        let page = async |after: Option<u64>, category: u64, hinted: bool| {
            let encoding = Encoding::Frame {
                method: 11,
                direction: Direction::Request,
            };
            let request = if label == "ic-db" {
                ic_wire::encode(&(after, category, hinted), encoding, LIMITS)
            } else {
                assert!(!hinted);

                ic_wire::encode(&(after, category), encoding, LIMITS)
            }
            .unwrap();
            let reply = pic
                .query_call(canister, canister, "keyset_listing", request)
                .await
                .unwrap();
            let (rows, instructions): (Vec<Row>, u64) = ic_wire::decode(
                &reply,
                Encoding::Frame {
                    method: 11,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap();

            (rows, instructions)
        };
        let traverse =
            async |mut after: Option<u64>, expected: &[Row], category: u64, hinted: bool| {
                let before = pic.get_stable_memory(canister).await;
                let mut costs = Vec::new();
                for offset in (0..=expected.len()).step_by(2) {
                    let (rows, instructions) = page(after, category, hinted).await;
                    assert_eq!(
                        rows,
                        expected[offset..(offset + 2).min(expected.len())],
                        "{label} after {after:?}"
                    );
                    after = rows.last().map(|row| row.0);
                    costs.push(instructions);
                }
                assert_eq!(pic.get_stable_memory(canister).await, before);

                costs
            };
        for phase in ["seeded", "after-upgrade-settled"] {
            if phase == "after-upgrade-settled" {
                ic_db_m0_rollback_driver::upgrade_stopped(
                    pic,
                    canister,
                    wasm.clone(),
                    vec![],
                    None,
                )
                .await
                .unwrap();
                eprintln!(
                    "{label} keyset reopen_steps_instructions_cycles={:?}",
                    startup().await
                );
            }
            for (category, rows) in [(1, &expected), (2, &sparse)] {
                for &hinted in if label == "ic-db" {
                    &[false, true][..]
                } else {
                    &[false][..]
                } {
                    let costs = traverse(None, rows, category, hinted).await;
                    eprintln!(
                        "{label} keyset {phase} category={category} hinted={hinted} rows={} pages={costs:?} total={}",
                        rows.len(),
                        costs.iter().sum::<u64>()
                    );
                }
            }
        }
        let (first, first_cost) = page(None, 1, false).await;
        assert_eq!(first, expected[..2]);
        let boundary = first.last().unwrap().0;
        assert_eq!(boundary, 21);
        for change in [(8, 1, 10), (32, 1, 10), (43, 2, 10)] {
            let (actual, _, _) = replace(change).await;
            assert_eq!(actual, Some(change));
            settle().await;
            expected.retain(|row| row.0 != change.0);
            if change.1 == 1 && change.2 >= 10 {
                expected.push(change);
            }
        }
        expected.sort_by_key(|row| row.0);
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
            .await
            .unwrap();
        let (_, reopen_cost, _) = startup().await;
        let suffix = &expected[expected.partition_point(|row| row.0 <= boundary)..];
        let resumed = traverse(Some(boundary), suffix, 1, false).await;
        let fresh = traverse(None, &expected, 1, false).await;
        eprintln!(
            "{label} keyset edited first={first_cost} readiness={reopen_cost} resumed={resumed:?} fresh={fresh:?}"
        );
        owned.drop().await;

        return;
    }
    let mut source = Vec::new();
    for id in 1..=32 {
        let value = (id, id % 4, id * 10);
        assert!(insert(value).await);
        settle().await;
        source.push(value);
    }

    let (found, instructions) = lookup(7).await;
    assert_eq!(found, Some((7, 3, 70)));
    eprintln!("{label} lookup_hit_instructions={instructions}");
    let (found, instructions) = lookup(99).await;
    assert_eq!(found, None);
    eprintln!("{label} lookup_miss_instructions={instructions}");

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
    let (value, _, _) = replace(replacement).await;
    assert_eq!(value, Some(replacement));
    settle().await;
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
    if label == "ic-db" && std::env::var_os("IC_DB_PREFIX_WRITE_PROFILE").is_some() {
        assert!(counts && !profile);
        for (name, value) in [
            ("same_prefix", (7_u64, 0_u64, 708_u64)),
            ("no_op", (7, 0, 708)),
        ] {
            let backing = pic.get_stable_memory(canister).await;
            let (actual, instructions, cycles) = replace(value).await;
            assert_eq!(actual, Some(value));
            eprintln!(
                "{label} {name}_replace_instructions={instructions} cycles={cycles} settling={:?}",
                settle().await
            );
            if name == "no_op" && !needs_settling {
                assert_eq!(pic.get_stable_memory(canister).await, backing);
            }
            *source.iter_mut().find(|row| row.0 == 7).unwrap() = value;
            assert_eq!(lookup(7).await.0, Some(value));
        }
    }

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
    let compare_aggregates = async |phase: &str| {
        if !aggregate {
            return;
        }
        for (category, minimum) in [
            (Some(3_u64), 0_u64),
            (Some(3), 200),
            (Some(0), 700),
            (Some(3), 999),
            (Some(99), 0),
            (None, 0),
            (None, 700),
            (None, 999),
        ] {
            // Independent source tuples and arithmetic, not either engine's reducer.
            let mut groups = std::collections::BTreeMap::<u64, Vec<u64>>::new();
            for row in &source {
                if category.is_none_or(|category| row.1 == category) && row.2 >= minimum {
                    groups.entry(row.1).or_default().push(row.2);
                }
            }
            for operation in 0..4_u8 {
                let expected: Vec<(u64, u64)> = groups
                    .iter()
                    .map(|(&key, values)| {
                        let result = match operation {
                            0 => values.len() as u64,
                            1 => values.iter().sum(),
                            2 => *values.iter().min().unwrap(),
                            3 => *values.iter().max().unwrap(),
                            _ => unreachable!(),
                        };

                        (key, result)
                    })
                    .collect();
                let backing = pic.get_stable_memory(canister).await;
                let reply = pic
                    .query_call(
                        canister,
                        canister,
                        "grouped_aggregate",
                        ic_wire::encode(
                            &(operation, category, minimum),
                            Encoding::Frame {
                                method: 7,
                                direction: Direction::Request,
                            },
                            LIMITS,
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap();
                let (result, instructions): (Vec<(u64, u64)>, u64) = ic_wire::decode(
                    &reply,
                    Encoding::Frame {
                        method: 7,
                        direction: Direction::Response,
                    },
                    LIMITS,
                )
                .unwrap();
                assert_eq!(
                    result, expected,
                    "{label}: {phase}, aggregate {operation}, category {category:?}, minimum {minimum}"
                );
                assert_eq!(pic.get_stable_memory(canister).await, backing);
                eprintln!(
                    "{label} aggregate phase={phase} operation={operation} category={category:?} minimum={minimum} result={result:?} instructions={instructions}"
                );
            }
        }
    };
    let profile_count = async |mode: u8, operand: Option<u64>| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "count_profile",
                ic_wire::encode(
                    &(mode, operand),
                    Encoding::Frame {
                        method: 9,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();

        ic_wire::decode::<(Option<u64>, u64, u64)>(
            &reply,
            Encoding::Frame {
                method: 9,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let compare_counts = async |phase: &str, expected_rows: &[Row]| {
        if !counts {
            return;
        }
        for category in [None, Some(0_u64), Some(3), Some(99)] {
            let expected = expected_rows
                .iter()
                .filter(|row| category.is_none_or(|category| row.1 == category))
                .count() as u64;
            let backing = pic.get_stable_memory(canister).await;
            let reply = pic
                .query_call(
                    canister,
                    canister,
                    "count_rows",
                    ic_wire::encode(
                        &category,
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
            let (result, instructions): (Option<u64>, u64) = ic_wire::decode(
                &reply,
                Encoding::Frame {
                    method: 8,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap();
            let expected_result = Some(expected);
            assert_eq!(
                result, expected_result,
                "{label}: {phase}, count {category:?}"
            );
            assert_eq!(pic.get_stable_memory(canister).await, backing);
            eprintln!(
                "{label} exact_count phase={phase} rows={} category={category:?} expected={expected} result={result:?} instructions={instructions}",
                expected_rows.len()
            );
            if prefix_candidate && let Some(category) = category {
                let reply = pic
                    .query_call(
                        canister,
                        canister,
                        "prefix_count",
                        ic_wire::encode(
                            &category,
                            Encoding::Frame {
                                method: 9,
                                direction: Direction::Request,
                            },
                            LIMITS,
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap();
                let (result, instructions): (u64, u64) = ic_wire::decode(
                    &reply,
                    Encoding::Frame {
                        method: 9,
                        direction: Direction::Response,
                    },
                    LIMITS,
                )
                .unwrap();
                assert_eq!(result, expected);
                assert_eq!(pic.get_stable_memory(canister).await, backing);
                eprintln!(
                    "ic-db prefix_metadata phase={phase} rows={} category={category} expected={expected} instructions={instructions}",
                    expected_rows.len()
                );
            }
            if profile {
                let modes: &[u8] = if category.is_none() {
                    &[0, 2]
                } else {
                    &[0, 1, 3, 4, 5]
                };
                for &mode in modes {
                    let (result, opening, work) = profile_count(mode, category).await;
                    let expected_result = if mode == 0 && expected_rows.len() >= 512 {
                        None
                    } else {
                        Some(expected)
                    };
                    assert_eq!(
                        result, expected_result,
                        "profile mode {mode}, {phase}, {category:?}"
                    );
                    assert_eq!(pic.get_stable_memory(canister).await, backing);
                    eprintln!(
                        "ic-db count_profile phase={phase} rows={} mode={mode} category={category:?} result={result:?} opening={opening} work={work}",
                        expected_rows.len()
                    );
                }
            }
        }
        if profile {
            let backing = pic.get_stable_memory(canister).await;
            for upper in [1_u64, 2, 33, 257, 512, 513] {
                let matches = expected_rows
                    .iter()
                    .filter(|row| (1..upper).contains(&row.0))
                    .count();
                let expected = (matches < 512).then_some(matches as u64);
                for (mode, path) in [(7, "automatic"), (8, "validated")] {
                    let (result, opening, work) = profile_count(mode, Some(upper)).await;
                    assert_eq!(result, expected, "{path}, {phase}, id < {upper}");
                    assert_eq!(pic.get_stable_memory(canister).await, backing);
                    eprintln!(
                        "ic-db primary_count phase={phase} rows={} upper={upper} path={path} matches={matches} result={result:?} opening={opening} work={work}",
                        expected_rows.len()
                    );
                }
            }
        }
    };
    compare_counts("settled", &source).await;
    compare_aggregates("settled").await;
    compare_fluent("settled").await;
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
        .await
        .unwrap();
    eprintln!(
        "{label} reopen_steps_instructions_cycles={:?}",
        startup().await
    );
    compare_fluent("settled-after-upgrade").await;
    compare_aggregates("settled-after-upgrade").await;
    compare_counts("settled-after-upgrade", &source).await;
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
    if counts {
        for population in [128, 256, 512] {
            for id in source.len() as u64 + 1..=population {
                let value = (id, id % 4, id * 10);
                assert!(insert(value).await);
                settle().await;
                source.push(value);
            }
            compare_counts("grown", &source).await;
        }
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
            .await
            .unwrap();
        eprintln!(
            "{label} count_reopen_steps_instructions_cycles={:?}",
            startup().await
        );
        compare_counts("grown-after-upgrade", &source).await;
        // Independent row witnesses, in addition to the count oracle.
        for id in [7, 33, 128, 256, 512] {
            assert_eq!(
                lookup(id).await.0,
                source.iter().copied().find(|row| row.0 == id)
            );
        }
    }
    eprintln!(
        "{label} stable_memory_bytes={}",
        pic.get_stable_memory(canister).await.len()
    );
    owned.drop().await;
}
