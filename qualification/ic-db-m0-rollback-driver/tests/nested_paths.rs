//! Compare reachable nested paths with a fixed installed schema and result oracle.
mod artifact;
mod wire_call;

#[tokio::test]
#[ignore = "run make m2-nested-path-costs"]
async fn reachable_paths() {
    let dir = std::path::PathBuf::from(std::env::var_os("IC_DB_NESTED_PATH_DIR").unwrap());
    let owned = ic_db_m0_rollback_driver::instance().await.unwrap();
    for (label, tier) in [
        ("1", 1_u8),
        ("8", 8),
        ("32", 32),
        ("indexed", 32),
        ("member", 32),
        ("optional", 32),
    ] {
        let (wasm, facts) = artifact::read(
            std::fs::File::open(dir.join(format!("nested-paths-{label}.wasm"))).unwrap(),
        )
        .unwrap();
        assert!(facts.exports.contains("canister_update measure"));
        println!(
            "{label}: {} Wasm bytes, {} code bytes",
            facts.identity.bytes, facts.code_section_bytes
        );
        let canister = owned.create_canister().await;
        owned.add_cycles(canister, 2_000_000_000_000).await;
        owned.install_canister(canister, wasm, vec![], None).await;
        let write =
            async |mode: u8| -> u64 { wire_call::call(&owned, canister, "write", 2, &mode).await };
        println!("{label} insert {}", write(0).await);
        if label == "optional" {
            // Every call reopens the same store. Replacement sets, changes and clears indexed members.
            for phase in 0..3_u8 {
                if phase != 0 {
                    println!(
                        "{label} write mode={phase} instructions={}",
                        write(phase).await
                    );
                }
                for mode in [4_u8, 5] {
                    let (ids, instructions): (Vec<u64>, u64) =
                        wire_call::call(&owned, canister, "measure", 1, &(7_u8, mode)).await;
                    let expected: Vec<u64> = (0..24)
                        .filter(|id| {
                            let flat = id + u64::from(phase);
                            phase != 2
                                && if mode == 5 {
                                    flat % 3 == 0
                                } else {
                                    flat % 3 != 0 && flat >= 22
                                }
                        })
                        .collect();
                    assert_eq!(ids, expected);
                    println!(
                        "{label} phase={phase} mode={mode} ids={ids:?} instructions={instructions}"
                    );
                }
            }
            continue;
        }
        for selector in (0..tier).chain([32]) {
            for mode in 0..5_u8 {
                let (ids, instructions): (Vec<u64>, u64) =
                    wire_call::call(&owned, canister, "measure", 1, &(selector, mode)).await;
                let mut expected: Vec<_> = (0..24)
                    .filter(|id| match mode {
                        1 => true,
                        3 => *id == 16,
                        4 => *id >= 22,
                        _ => *id >= 16,
                    })
                    .collect();
                if matches!(mode, 1 | 2) {
                    expected.reverse();
                }
                expected.truncate(8);
                assert_eq!(
                    ids, expected,
                    "tier={label} selector={selector} mode={mode}"
                );
                if matches!(selector, 0 | 7 | 32) {
                    println!(
                        "tier={label} selector={selector} mode={mode} instructions={instructions}"
                    );
                }
            }
        }
        for (mode, expected) in [(1_u8, vec![15_u64]), (2, vec![])] {
            println!(
                "{label} write mode={mode} instructions={}",
                write(mode).await
            );
            for &selector in if tier >= 8 {
                &[0_u8, 7, 32][..]
            } else {
                &[0_u8, 32][..]
            } {
                let (ids, _): (Vec<u64>, u64) =
                    wire_call::call(&owned, canister, "measure", 1, &(selector, 3_u8)).await;
                assert_eq!(ids, expected);
            }
        }
    }
    owned.drop().await;
}
