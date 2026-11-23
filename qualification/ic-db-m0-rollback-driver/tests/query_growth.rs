//! Hold host/transport fixed while varying reachable fluent shapes and mapping sites.

mod artifact;

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

type Row = (u64, u64, u64);
type Observation = (Option<Row>, Vec<Row>, Vec<Row>, u64, u8);

// Independent tuple oracle, never the product planner/codec. Application maps
// deliberately change returned values so each closure is observably reachable.
fn expected(input: &[Row], mode: u8, args: Row) -> Vec<Row> {
    let mut rows: Vec<_> = input
        .iter()
        .copied()
        .filter(|&(id, category, amount)| match mode {
            10 | 13 | 20..=27 => amount >= args.2,
            11 => amount <= args.2,
            12 => amount == args.2 || id == args.0,
            14 => true,
            15 => category == args.1,
            16 => id >= args.0 && id <= args.2,
            17 => category != args.1 && category != args.2,
            _ => panic!("unsupported oracle mode"),
        })
        .collect();
    if mode == 14 {
        rows.sort_by_key(|r| (r.2, r.0));
    } else {
        rows.sort_by_key(|r| r.0);
    }
    if matches!(mode, 13 | 14) {
        rows.reverse();
    }
    rows.truncate(2);
    for row in &mut rows {
        match mode {
            20 => row.0 = row.0.wrapping_add(args.0),
            21 => row.1 = row.1.wrapping_add(args.1),
            22 => row.2 = row.2.wrapping_add(args.2),
            23 => row.0 ^= args.0,
            24 => row.1 ^= args.1,
            25 => row.2 ^= args.2,
            26 => std::mem::swap(&mut row.0, &mut row.2),
            27 => std::mem::swap(&mut row.1, &mut row.2),
            _ => {}
        }
    }

    rows
}

#[tokio::test]
async fn fluent_shapes_and_mapping_growth() {
    let directory = std::path::PathBuf::from(
        std::env::var_os("IC_DB_QUERY_GROWTH_DIR").expect("run make m2-query-growth"),
    );
    exercise(
        &directory,
        [
            ("direct", 1, 0),
            ("two-indexes", 1, 0),
            ("direct-entities", 8, 0),
            ("direct-32", 32, 0),
            ("one", 1, 10),
            ("one-8", 8, 10),
            ("one-32", 32, 10),
            ("shapes", 1, 17),
            ("maps", 1, 27),
            ("entities", 8, 17),
            ("shapes-32", 32, 17),
        ],
    )
    .await;
}

#[tokio::test]
#[ignore = "run make m2-named-growth"]
async fn named_growth() {
    let directory = std::path::PathBuf::from(std::env::var_os("IC_DB_QUERY_GROWTH_DIR").unwrap());
    let roster = std::fs::read_to_string(directory.join("variants.tsv")).unwrap();
    let variants = roster.lines().map(|line| {
        let (variant, count) = line.split_once('\t').unwrap();
        (variant, count.parse().unwrap(), 17)
    });
    exercise(&directory, variants).await;
}

async fn exercise<'a>(
    directory: &std::path::Path,
    variants: impl IntoIterator<Item = (&'a str, u8, u8)>,
) {
    for (variant, count, last_mode) in variants {
        let (wasm, facts) = crate::artifact::read(
            std::fs::File::open(directory.join(format!("{variant}.wasm"))).unwrap(),
        )
        .unwrap();
        let exports: Vec<_> = facts
            .exports
            .iter()
            .map(String::as_str)
            .filter(|name| name.starts_with("canister_"))
            .collect();
        assert_eq!(exports, ["canister_init", "canister_update exercise"]);
        eprintln!(
            "variant={variant} entities={count} wasm_bytes={} code_section_bytes={} sha256={}",
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
        pic.install_canister(canister, wasm.clone(), vec![], None)
            .await;
        let call = async |entity: u8, mode: u8, row: Row| {
            let before = pic.cycle_balance(canister).await;
            let reply = pic
                .update_call(
                    canister,
                    canister,
                    "exercise",
                    ic_wire::encode(
                        &(entity, mode, row),
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
            let cycles = before
                .checked_sub(pic.cycle_balance(canister).await)
                .unwrap();
            let (observation, opening, work): (Observation, [u64; 4], u64) = ic_wire::decode(
                &reply,
                Encoding::Frame {
                    method: 1,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap();

            (observation, opening, work, cycles)
        };
        let inputs: Vec<Vec<Row>> = (0..count)
            .map(|entity| {
                [40, 10, 50, 10, 80, 60, 30, 20]
                    .into_iter()
                    .enumerate()
                    .map(|(i, amount)| {
                        (
                            i as u64 + 1,
                            (i as u64 + 1) % 3,
                            amount + u64::from(entity) * 100,
                        )
                    })
                    .collect()
            })
            .collect();
        for (entity, rows) in inputs.iter().enumerate() {
            for &row in rows {
                let result = call(entity as u8, 1, row).await;
                assert_eq!(result.0.0, Some(row));
                assert_eq!(result.0.4, count);
                if entity == 0 && matches!(row.0, 1 | 8) {
                    eprintln!(
                        "variant={variant} insert_id={} opening={:?} work={} cycles={}",
                        row.0, result.1, result.2, result.3
                    );
                }
            }
        }
        let modes: Vec<_> = (10..=last_mode)
            .filter(|mode| !matches!(mode, 18 | 19))
            .collect();
        for (entity, rows) in inputs.iter().enumerate() {
            let args = (3, 1, 45 + entity as u64 * 100);
            let direct = call(entity as u8, 0, args).await;
            assert_eq!(direct.0.0, Some(rows[2]));
            assert_eq!(direct.0.1, rows[..2]);
            assert_eq!(direct.0.2, vec![rows[0], rows[3]]);
            assert_eq!((direct.0.3, direct.0.4), (8, count));
            if entity == 0 {
                eprintln!(
                    "variant={variant} direct_opening={:?} direct_work={} direct_cycles={}",
                    direct.1, direct.2, direct.3
                );
            }
            for &mode in &modes {
                for repeat in 0..2 {
                    let result = call(entity as u8, mode, args).await;
                    assert_eq!(
                        result.0,
                        (None, expected(rows, mode, args), vec![], 8, count)
                    );
                    if entity == 0 && repeat == 1 {
                        eprintln!(
                            "variant={variant} mode={mode} warm_opening={:?} work={} cycles={}",
                            result.1, result.2, result.3
                        );
                    }
                }
            }
        }
        // The identical public query stays reachable after a real executable
        // upgrade; compare cold opening separately from repeated warm execution.
        pic.advance_time(std::time::Duration::from_secs(600)).await;
        pic.tick().await;
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
            .await
            .unwrap();
        let mode = if last_mode == 0 { 0 } else { 10 };
        let cold = call(0, mode, (3, 1, 45)).await;
        if mode == 0 {
            assert_eq!(cold.0.0, Some(inputs[0][2]));
        } else {
            assert_eq!(
                cold.0,
                (
                    None,
                    expected(&inputs[0], mode, (3, 1, 45)),
                    vec![],
                    8,
                    count
                )
            );
            for entity in 0..count {
                let empty = call(entity, 10, (3, 1, 10_000)).await;
                assert_eq!(empty.0, (None, vec![], vec![], 8, count));
            }
        }
        eprintln!(
            "variant={variant} cold_mode={mode} opening={:?} work={} cycles={}",
            cold.1, cold.2, cold.3
        );
        // Measure a caller read followed by replacement through the same public
        // host. Keep this after the query samples so their data/revision stay fixed.
        let loaded = call(0, 0, (3, 0, 0)).await;
        let row = loaded.0.0.unwrap();
        let changed = (row.0, 4, row.2 + 1);
        let replaced = call(0, 2, changed).await;
        assert_eq!(
            replaced.0,
            (
                Some(changed),
                inputs[0][..2].to_vec(),
                vec![changed],
                9,
                count
            )
        );
        eprintln!(
            "variant={variant} read_replace_work={} read_replace_cycles={}",
            loaded.2 + replaced.2,
            loaded.3 + replaced.3
        );
        owned.drop().await;
    }
}
