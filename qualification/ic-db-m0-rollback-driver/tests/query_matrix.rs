//! Broad matched queries over deterministic tuples, with no product-code oracle.
mod artifact;
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, fmt::Write};

type Row = (u64, u64, u64);
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Exists(bool),
    Row(Option<Row>),
    Rows(Vec<Row>),
    Groups(Vec<(u64, u64)>),
    Count(Option<u64>),
    Refused(u16),
}
struct Case {
    name: String,
    family: &'static str,
    method: &'static str,
    id: u64,
    request: Vec<u8>,
    expected: Answer,
    legacy_refuses: bool,
}
fn encode<T: Serialize>(id: u64, value: &T) -> Vec<u8> {
    ic_wire::encode(
        value,
        Encoding::Frame {
            method: id,
            direction: Direction::Request,
        },
        LIMITS,
    )
    .unwrap()
}

fn decode<T: DeserializeOwned>(id: u64, bytes: &[u8]) -> T {
    ic_wire::decode(
        bytes,
        Encoding::Frame {
            method: id,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap()
}

fn answer(id: u64, bytes: &[u8]) -> (Answer, u64) {
    match id {
        3 => {
            let (v, n) = decode(id, bytes);
            (Answer::Row(v), n)
        }
        5 => {
            let (v, n) = decode(id, bytes);
            (Answer::Rows(v), n)
        }
        6 => {
            let (v, n): (Result<Vec<Row>, u16>, u64) = decode(id, bytes);
            (
                match v {
                    Ok(v) => Answer::Rows(v),
                    Err(e) => Answer::Refused(e),
                },
                n,
            )
        }
        7 => {
            let (v, n) = decode(id, bytes);
            (Answer::Groups(v), n)
        }
        8 => {
            let (v, n) = decode(id, bytes);
            (Answer::Count(v), n)
        }
        9 => {
            let (value, open, execution): (Option<u64>, u64, u64) = decode(id, bytes);
            (Answer::Count(value), open + execution)
        }
        10 => {
            let (value, work) = decode(id, bytes);
            (Answer::Exists(value), work)
        }
        _ => unreachable!(),
    }
}

// The profile endpoint times open and execution separately on the same artifact.
// Legacy supplies its maintained result as the control for each IC DB path.
fn prefix_counts(source: &[Row], legacy: bool) -> Vec<Case> {
    let mut cases = Vec::new();
    for category in [0_u64, 3, 99] {
        let count = source.iter().filter(|row| row.1 == category).count() as u64;
        for (mode, family) in [(1_u8, "validated"), (5, "entries"), (6, "automatic")] {
            let (method, id, request) = if legacy {
                ("count_rows", 8, encode(8, &Some(category)))
            } else {
                ("count_profile", 9, encode(9, &(mode, Some(category))))
            };
            cases.push(Case {
                name: format!("{family}-{category}"),
                family,
                method,
                id,
                request,
                expected: Answer::Count(Some(count)),
                legacy_refuses: false,
            });
        }
    }
    cases
}

fn cases(source: &[Row], paths: bool) -> Vec<Case> {
    let mut cases = Vec::new();
    let population = source.len() as u64;
    for id in [1, population / 2, population, population + 1] {
        let expected = source.iter().find(|row| row.0 == id).copied();
        cases.push(Case {
            name: format!("lookup-{id}"),
            family: if expected.is_some() {
                "primary-hit"
            } else {
                "primary-miss"
            },
            method: "lookup",
            id: 3,
            request: encode(3, &id),
            expected: Answer::Row(expected),
            legacy_refuses: false,
        });
    }
    for category in [0_u64, 3, 99] {
        cases.push(Case {
            name: format!("prefix-{category}"),
            family: if category == 99 {
                "prefix-empty"
            } else {
                "prefix-first10"
            },
            method: "listing",
            id: 5,
            request: encode(5, &category),
            expected: Answer::Rows(
                source
                    .iter()
                    .copied()
                    .filter(|row| row.1 == category)
                    .take(10)
                    .collect(),
            ),
            legacy_refuses: false,
        });
    }
    for category in [None, Some(0_u64), Some(3), Some(99)] {
        cases.push(Case {
            name: format!("count-{}", category.map_or("all".into(), |c| c.to_string())),
            family: match category {
                None => "count-entity",
                Some(99) => "count-empty-prefix",
                _ => "count-prefix",
            },
            method: "count_rows",
            id: 8,
            request: encode(8, &category),
            expected: Answer::Count(Some(
                source
                    .iter()
                    .filter(|row| category.is_none_or(|c| c == row.1))
                    .count() as u64,
            )),
            legacy_refuses: false,
        });
    }
    for mode in 0..4_u8 {
        let categories: &[u64] = if mode == 2 { &[0] } else { &[0, 3, 99] };
        for &category in categories {
            for minimum in [0_u64, 500, 950, 1001] {
                for limit in [1_u32, 8, 32] {
                    let mut rows: Vec<_> = source
                        .iter()
                        .copied()
                        .filter(|row| row.2 >= minimum && (mode == 2 || row.1 == category))
                        .collect();
                    rows.sort_by(|a, b| match mode {
                        0 => a.0.cmp(&b.0),
                        1 => b.0.cmp(&a.0),
                        2 => (a.1, a.0).cmp(&(b.1, b.0)),
                        3 => b.2.cmp(&a.2).then(a.0.cmp(&b.0)),
                        _ => unreachable!(),
                    });
                    rows.truncate(limit as usize);
                    cases.push(Case {
                        name: format!("listing-{mode}-c{category}-min{minimum}-limit{limit}"),
                        family: match mode {
                            0 => "fluent-asc",
                            1 => "fluent-desc",
                            2 => "fluent-compound",
                            _ => "bounded-sort",
                        },
                        method: "fluent_listing",
                        id: 6,
                        request: encode(6, &(mode, category, minimum, limit)),
                        expected: Answer::Rows(rows),
                        legacy_refuses: mode == 3,
                    });
                }
            }
        }
    }
    for category in [None, Some(0_u64), Some(3), Some(99)] {
        for minimum in [0_u64, 500, 950, 1001] {
            let mut groups = BTreeMap::<u64, Vec<u64>>::new();
            for row in source
                .iter()
                .filter(|row| category.is_none_or(|c| c == row.1) && row.2 >= minimum)
            {
                groups.entry(row.1).or_default().push(row.2);
            }
            for operation in 0..if paths { 8_u8 } else { 4_u8 } {
                let expected = groups
                    .iter()
                    .map(|(&key, values)| {
                        (
                            key,
                            match operation % 4 {
                                0 => values.len() as u64,
                                1 => values.iter().sum(),
                                2 => *values.iter().min().unwrap(),
                                3 => *values.iter().max().unwrap(),
                                _ => unreachable!(),
                            },
                        )
                    })
                    .collect();
                cases.push(Case {
                    name: format!(
                        "group-{operation}-c{}-min{minimum}",
                        category.map_or("all".into(), |c| c.to_string())
                    ),
                    family: match operation {
                        0 => "group-count",
                        1 => "group-sum",
                        2 => "group-min",
                        3 => "group-max",
                        4 => "group-count-index",
                        5 => "group-sum-index",
                        6 => "group-min-index",
                        _ => "group-max-index",
                    },
                    method: "grouped_aggregate",
                    id: 7,
                    request: encode(7, &(operation, category, minimum)),
                    expected: Answer::Groups(expected),
                    legacy_refuses: false,
                });
            }
        }
    }
    if paths {
        cases.retain(|case| case.id == 7);
    }
    cases
}
struct Sample {
    engine: &'static str,
    profile: &'static str,
    population: u64,
    phase: &'static str,
    case: String,
    family: &'static str,
    refused: bool,
    instructions: u64,
}
#[tokio::test]
async fn matched_query_matrix() {
    let directory = std::path::PathBuf::from(
        std::env::var_os("IC_DB_QUERY_MATRIX_DIR").expect("run make m2-query-matrix"),
    );
    let count_paths = std::env::var_os("IC_DB_COUNT_PATHS").is_some();
    let exists = std::env::var_os("IC_DB_EXISTS_COMPARISON").is_some();
    let mut samples = Vec::new();
    for (engine, variable, legacy) in [
        ("ic-db", "IC_DB_RECORDS_BENCHMARK_WASM", false),
        ("icydb", "IC_DB_ICYDB_BASELINE_WASM", true),
    ] {
        let (wasm, facts) =
            artifact::read(std::fs::File::open(std::env::var_os(variable).unwrap()).unwrap())
                .unwrap();
        let methods: Vec<_> = facts
            .exports
            .iter()
            .filter(|name| {
                name.starts_with("canister_query ") || name.starts_with("canister_update ")
            })
            .map(String::as_str)
            .collect();
        let mut expected = vec![
            "canister_query listing",
            "canister_query lookup",
            "canister_update advance_startup",
            "canister_update insert",
            "canister_update read_modify_write",
        ];
        if exists {
            expected.push("canister_query category_exists");
        } else {
            expected.push("canister_query count_rows");
        }
        if !count_paths && !exists {
            expected.extend([
                "canister_query fluent_listing",
                "canister_query grouped_aggregate",
            ]);
        } else if count_paths && !legacy {
            expected.push("canister_query count_profile");
        }
        expected.sort_unstable();
        assert_eq!(methods, expected);
        eprintln!(
            "{engine} wasm_bytes={} code_section_bytes={} sha256={}",
            facts.identity.bytes,
            facts.code_section_bytes,
            facts
                .identity
                .sha256
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        for profile in ["balanced", "skewed"] {
            let owned = instance().await.unwrap();
            let pic = &owned;
            let canister = pic.create_canister().await;
            pic.add_cycles(canister, 100_000_000_000_000).await;
            pic.install_canister(canister, wasm.clone(), vec![], None)
                .await;
            let startup = async || {
                let before = pic.cycle_balance(canister).await;
                let mut instructions = 0;
                for calls in 1..=100 {
                    let reply = pic
                        .update_call(canister, canister, "advance_startup", encode(1, &()))
                        .await
                        .unwrap();
                    let (ready, work): (bool, u64) = decode(1, &reply);
                    instructions += work;
                    if ready {
                        return (
                            calls,
                            instructions,
                            before - pic.cycle_balance(canister).await,
                        );
                    }
                }
                panic!("startup failed to progress");
            };
            eprintln!(
                "{engine} {profile} initial_startup_calls_instructions_cycles={:?}",
                startup().await
            );
            let mut source = Vec::new();
            for population in [32_u64, 128, 511] {
                for id in source.len() as u64 + 1..=population {
                    let category = if profile == "skewed" && id % 16 < 13 {
                        0
                    } else {
                        id % 4
                    };
                    let row = (id, category, (id * 37 % 101) * 10);
                    let reply = pic
                        .update_call(canister, canister, "insert", encode(2, &row))
                        .await
                        .unwrap();
                    assert!(decode::<(bool, u64)>(2, &reply).0);
                    if legacy {
                        startup().await;
                    }
                    source.push(row);
                }
                let cases = if exists {
                    [0_u64, 3, 99]
                        .into_iter()
                        .map(|category| {
                            let found = source.iter().any(|row| row.1 == category);
                            Case {
                                name: format!("exists-{category}"),
                                family: if found { "exists-hit" } else { "exists-miss" },
                                method: "category_exists",
                                id: 10,
                                request: encode(10, &category),
                                expected: Answer::Exists(found),
                                legacy_refuses: false,
                            }
                        })
                        .collect()
                } else if count_paths {
                    prefix_counts(&source, legacy)
                } else {
                    cases(&source, std::env::var_os("IC_DB_AGGREGATE_PATHS").is_some())
                };
                for phase in ["settled", "after-upgrade-settled"] {
                    if phase == "after-upgrade-settled" {
                        let backing = pic.get_stable_memory(canister).await;
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
                        // Startup/recovery is measured separately, never hidden in query samples.
                        eprintln!(
                            "{engine} {profile} rows={population} reopen_calls_instructions_cycles={:?}",
                            startup().await
                        );
                        if !legacy {
                            assert_eq!(pic.get_stable_memory(canister).await, backing);
                        }
                    }
                    let backing = pic.get_stable_memory(canister).await;
                    for case in &cases {
                        let reply = pic
                            .query_call(canister, canister, case.method, case.request.clone())
                            .await
                            .unwrap_or_else(|e| {
                                panic!(
                                    "{engine}/{profile}/{population}/{phase}/{}: {e:?}",
                                    case.name
                                )
                            });
                        let (actual, instructions) = answer(case.id, &reply);
                        let refused = legacy && case.legacy_refuses;
                        let expected = if refused {
                            &Answer::Refused(174)
                        } else {
                            &case.expected
                        };
                        assert_eq!(
                            &actual, expected,
                            "{engine}/{profile}/{population}/{phase}/{}",
                            case.name
                        );
                        samples.push(Sample {
                            engine,
                            profile,
                            population,
                            phase,
                            case: case.name.clone(),
                            family: case.family,
                            refused,
                            instructions,
                        });
                    }
                    assert_eq!(
                        pic.get_stable_memory(canister).await,
                        backing,
                        "query matrix changed stable backing"
                    );
                    eprintln!(
                        "{engine} {profile} rows={population} {phase}: {} exact-result cases passed",
                        cases.len()
                    );
                }
            }
            owned.drop().await;
        }
    }
    report(&directory, &samples);
}

fn report(directory: &std::path::Path, samples: &[Sample]) {
    let mut csv =
        String::from("engine,profile,population,phase,case,family,outcome,instructions\n");
    let mut pairs = BTreeMap::<_, [Option<&Sample>; 2]>::new();
    for sample in samples {
        writeln!(
            csv,
            "{},{},{},{},{},{},{},{}",
            sample.engine,
            sample.profile,
            sample.population,
            sample.phase,
            sample.case,
            sample.family,
            if sample.refused { "refused-174" } else { "ok" },
            sample.instructions
        )
        .unwrap();
        pairs
            .entry((
                sample.profile,
                sample.population,
                sample.phase,
                sample.case.as_str(),
            ))
            .or_default()[usize::from(sample.engine == "icydb")] = Some(sample);
    }
    std::fs::write(directory.join("queries.csv"), csv).unwrap();
    let mut families = BTreeMap::<_, Vec<(u64, u64)>>::new();
    let mut refused = 0;
    for pair in pairs.values() {
        let [Some(db), Some(legacy)] = pair else {
            panic!("unpaired result")
        };
        if legacy.refused {
            refused += 1;
            continue;
        }
        assert!(!db.refused);
        families
            .entry((db.population, db.family))
            .or_default()
            .push((db.instructions, legacy.instructions));
    }
    let mut summary = String::from(
        "population\tfamily\tpairs\tic_db_median_instructions\ticydb_median_instructions\tmin_ic_db_over_icydb\tmedian_ic_db_over_icydb\tmax_ic_db_over_icydb\n",
    );
    for ((population, family), values) in families {
        let mut db: Vec<_> = values.iter().map(|v| v.0).collect();
        db.sort_unstable();
        let mut legacy: Vec<_> = values.iter().map(|v| v.1).collect();
        legacy.sort_unstable();
        let mut ratios: Vec<_> = values.iter().map(|v| v.0 as f64 / v.1 as f64).collect();
        ratios.sort_by(f64::total_cmp);
        writeln!(
            summary,
            "{population}\t{family}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{:.4}",
            values.len(),
            db[db.len() / 2],
            legacy[legacy.len() / 2],
            ratios[0],
            ratios[ratios.len() / 2],
            ratios[ratios.len() - 1]
        )
        .unwrap();
    }
    std::fs::write(directory.join("summary.tsv"), &summary).unwrap();
    eprintln!(
        "{} measurements; {} successful matched pairs; {refused} legacy refusals excluded\n{summary}",
        samples.len(),
        pairs.len() - refused
    );
}
