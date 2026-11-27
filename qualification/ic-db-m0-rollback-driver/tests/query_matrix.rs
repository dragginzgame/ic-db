//! Broad matched queries over deterministic tuples, with no product-code oracle.

mod artifact;
mod wire_frame;

use ic_db_m0_rollback_driver::instance;
use std::{collections::BTreeMap, fmt::Write};
use wire_frame::{request as encode, response as decode};

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
    let (method, id, request) = if legacy {
        ("count_rows", 8, encode(8, &None::<u64>))
    } else {
        ("count_profile", 9, encode(9, &(6_u8, None::<u64>)))
    };
    let mut cases = vec![Case {
        name: "count-all".into(),
        family: "count-entity",
        method,
        id,
        request,
        expected: Answer::Count(Some(source.len() as u64)),
        legacy_refuses: false,
    }];
    for category in [0_u64, 3, 99] {
        let count = source.iter().filter(|row| row.1 == category).count() as u64;
        for (mode, family) in [
            (1_u8, "validated"),
            (5, "entries"),
            (6, "automatic"),
            (9, "minimum-zero"),
            (10, "minimum-zero-validated"),
        ] {
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
    #[derive(PartialEq, Eq)]
    enum Workload {
        Matrix,
        CountPaths,
        Exists,
        AggregatePaths,
    }
    let workload = match std::env::var("IC_DB_COMPARISON_WORKLOAD").as_deref() {
        Ok("query-matrix") => Workload::Matrix,
        Ok("count-paths") => Workload::CountPaths,
        Ok("exists") => Workload::Exists,
        Ok("aggregate-paths") => Workload::AggregatePaths,
        _ => panic!("select a query matrix workload with scripts/measure-records.sh"),
    };
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
        if workload == Workload::Exists {
            expected.push("canister_query category_exists");
            if !legacy {
                expected.push("canister_query filtered_exists");
            }
        } else {
            expected.push("canister_query count_rows");
        }
        if matches!(workload, Workload::Matrix | Workload::AggregatePaths) {
            expected.extend([
                "canister_query fluent_listing",
                "canister_query grouped_aggregate",
            ]);
        } else if workload == Workload::CountPaths && !legacy {
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
                let mut insert_instructions = 0;
                let first = source.len() as u64 + 1;
                for id in first..=population {
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
                    let (accepted, instructions) = decode::<(bool, u64)>(2, &reply);
                    assert!(accepted);
                    insert_instructions += instructions;
                    if legacy {
                        startup().await;
                    }
                    source.push(row);
                }
                eprintln!(
                    "{engine} {profile} inserts={first}..={population} handler_instructions={insert_instructions}"
                );
                let cases = if workload == Workload::Exists {
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
                } else if workload == Workload::CountPaths {
                    prefix_counts(&source, legacy)
                } else {
                    cases(&source, workload == Workload::AggregatePaths)
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
                    if workload == Workload::Exists && !legacy && population == 511 {
                        let candidates: Vec<_> = source.iter().filter(|row| row.1 == 0).collect();
                        for amount in [candidates[0].2, candidates[63].2, 1001] {
                            let first = candidates.iter().position(|row| row.2 == amount);
                            let needed = first.map_or(candidates.len(), |index| index + 1);
                            for budget in [needed.saturating_sub(1), needed]
                                .into_iter()
                                .filter(|n| *n > 0)
                            {
                                let bytes = pic
                                    .query_call(
                                        canister,
                                        canister,
                                        "filtered_exists",
                                        encode(
                                            12,
                                            &(0_u64, amount, u16::try_from(budget).unwrap()),
                                        ),
                                    )
                                    .await
                                    .unwrap();
                                let (result, work): (Option<(bool, u64, u64)>, u64) =
                                    decode(12, &bytes);
                                assert_eq!(
                                    result,
                                    (budget == needed).then_some((
                                        first.is_some(),
                                        needed as u64,
                                        needed as u64
                                    ))
                                );
                                eprintln!(
                                    "filtered-exists {profile} {phase} amount={amount} budget={budget} result={result:?} instructions={work}"
                                );
                            }
                        }
                    }
                    if workload == Workload::CountPaths && !legacy && population == 511 {
                        // Bare count uses metadata. A category count must finish the
                        // index range within the default 128 candidates, across upgrade.
                        for category in [None, Some(0_u64), Some(3), Some(99)] {
                            let matches = source
                                .iter()
                                .filter(|row| category.is_none_or(|c| c == row.1))
                                .count() as u64;
                            let expected =
                                (category.is_none() || matches <= 128).then_some(matches);
                            let bytes = pic
                                .query_call(
                                    canister,
                                    canister,
                                    "count_profile",
                                    encode(9, &(11_u8, category)),
                                )
                                .await
                                .unwrap();
                            assert_eq!(
                                answer(9, &bytes).0,
                                Answer::Count(expected),
                                "default count: {profile}/{phase}/{category:?}, {matches} matches"
                            );
                        }
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
            // Sample writes after all read comparisons, on the settled 511-row fixture.
            if workload == Workload::AggregatePaths && profile == "skewed" {
                assert_eq!(source[0], (1, 0, 370));
                for (change, row) in [
                    ("cross-minimum", (1, 0, 990)),
                    ("move-category", (1, 3, 990)),
                    ("no-op", (1, 3, 990)),
                ] {
                    let backing = if !legacy && change == "no-op" {
                        Some(pic.get_stable_memory(canister).await)
                    } else {
                        None
                    };
                    let before = pic.cycle_balance(canister).await;
                    let reply = pic
                        .update_call(canister, canister, "read_modify_write", encode(4, &row))
                        .await
                        .unwrap();
                    let cycles = before - pic.cycle_balance(canister).await;
                    let (stored, instructions) = decode::<(Option<Row>, u64)>(4, &reply);
                    assert_eq!(stored, Some(row));
                    if let Some(bytes) = backing {
                        assert_eq!(pic.get_stable_memory(canister).await, bytes);
                    }
                    let settling = if legacy { Some(startup().await) } else { None };
                    source[0] = row;
                    let reply = pic
                        .query_call(canister, canister, "lookup", encode(3, &row.0))
                        .await
                        .unwrap();
                    assert_eq!(answer(3, &reply).0, Answer::Row(Some(row)));
                    // Cover both the full category and its high-amount suffix.
                    for (category, minimum) in [(0_u64, 0_u64), (0, 950), (3, 0), (3, 950)] {
                        let count = source
                            .iter()
                            .filter(|r| r.1 == category && r.2 >= minimum)
                            .count() as u64;
                        for operation in [0_u8, 4] {
                            let args = (operation, Some(category), minimum);
                            let reply = pic
                                .query_call(
                                    canister,
                                    canister,
                                    "grouped_aggregate",
                                    encode(7, &args),
                                )
                                .await
                                .unwrap();
                            assert_eq!(
                                answer(7, &reply).0,
                                Answer::Groups(vec![(category, count)])
                            );
                        }
                    }
                    eprintln!(
                        "{engine} grouped-index replace={change} instructions={instructions} cycles={cycles} settling={settling:?}"
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

/// Matched size subjects are built separately from the scalar query matrix.
#[tokio::test]
#[ignore = "requires the paired growth artifacts"]
async fn paired_entity_growth() {
    let directory = std::path::PathBuf::from(std::env::var_os("IC_DB_MATCHED_GROWTH_DIR").unwrap());
    let mut receipt =
        String::from("engine\tentities\tfluent_shapes\twasm_bytes\tcode_bytes\tsha256\n");
    let mut eight_entity_bytes = std::collections::BTreeMap::new();
    for count in [1_u8, 8, 32] {
        for shapes in [0_u8, 1, 8] {
            for engine in ["ic-db", "icydb"] {
                let label = format!("{engine}-{count}-{shapes}");
                let (wasm, facts) = artifact::read(
                    std::fs::File::open(directory.join(format!("{label}.wasm"))).unwrap(),
                )
                .unwrap();
                assert!(facts.exports.contains("canister_update exercise"));
                let pic = instance().await.unwrap();
                let canister = pic.create_canister().await;
                pic.add_cycles(canister, 100_000_000_000_000).await;
                pic.install_canister(canister, wasm, vec![], None).await;
                let settle = async || {
                    if engine == "icydb" {
                        for _ in 0..100 {
                            let bytes = pic
                                .update_call(canister, canister, "advance_startup", encode(1, &()))
                                .await
                                .unwrap();
                            if decode::<(bool, u64)>(1, &bytes).0 {
                                return;
                            }
                        }
                        panic!("{label} startup did not converge");
                    }
                };
                settle().await;
                let call = async |selector: u8, mode: u8, row: Row| {
                    let bytes = pic
                        .update_call(
                            canister,
                            canister,
                            "exercise",
                            encode(2, &(selector, mode, row)),
                        )
                        .await
                        .unwrap_or_else(|e| panic!("{label} entity={selector} mode={mode}: {e:?}"));
                    decode::<Vec<Row>>(2, &bytes)
                };
                // Overlapping keys expose cross-entity dispatch mistakes. A distinct
                // category supplies an excluded high-value row in every entity.
                for entity in 0..count {
                    for (id, category, amount) in [(3, 7, 30), (1, 7, 10), (2, 7, 20), (4, 8, 40)] {
                        assert!(
                            call(entity, 0, (id, category, amount + u64::from(entity)))
                                .await
                                .is_empty()
                        );
                    }
                    settle().await;
                }
                for entity in 0..count {
                    let offset = u64::from(entity);
                    let input = (2, 7, 20 + offset);
                    assert_eq!(call(entity, 1, input).await, [input]);
                    for shape in 0..shapes {
                        for (key, threshold) in [(2, 20), (1, 30)] {
                            let expected: Vec<Row> = (1..=3)
                                .map(|id| (id, 7, id * 10 + offset))
                                .filter(|&(id, _, amount)| match shape {
                                    0 => true,
                                    1 => amount >= threshold + offset,
                                    2 => amount <= threshold + offset,
                                    3 => amount == threshold + offset,
                                    4 => amount > threshold + offset,
                                    5 => amount < threshold + offset,
                                    6 => id >= key,
                                    7 => id <= key,
                                    _ => unreachable!(),
                                })
                                .take(2)
                                .collect();
                            assert_eq!(
                                call(entity, 2 + shape, (key, 7, threshold + offset)).await,
                                expected,
                                "{label} entity={entity} shape={shape}"
                            );
                        }
                    }
                }
                println!(
                    "paired-growth {label} raw={} code={}",
                    facts.identity.bytes, facts.code_section_bytes
                );
                if engine == "ic-db" {
                    const BYTES_PER_ADDED_ENTITY: u64 = 8 * 1024;
                    match count {
                        8 => {
                            eight_entity_bytes.insert(shapes, facts.identity.bytes);
                        }
                        32 => {
                            let added = facts
                                .identity
                                .bytes
                                .saturating_sub(eight_entity_bytes[&shapes]);
                            assert!(
                                added <= 24 * BYTES_PER_ADDED_ENTITY,
                                "{label} added {added} Wasm bytes over 24 entities, budget {}",
                                24 * BYTES_PER_ADDED_ENTITY
                            );
                        }
                        _ => {}
                    }
                }
                let hash = facts
                    .identity
                    .sha256
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                writeln!(
                    receipt,
                    "{engine}\t{count}\t{shapes}\t{}\t{}\t{hash}",
                    facts.identity.bytes, facts.code_section_bytes
                )
                .unwrap();
                pic.drop().await;
            }
        }
    }
    std::fs::write(directory.join("verified.tsv"), receipt).unwrap();
}
