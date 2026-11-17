//! Entity growth with independently checked reachability, isolation and upgrade.
mod artifact;
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_testkit::pocket_ic::ErrorCode;
use ic_wire::{Direction, Encoding};

type Row = (u64, u64, u64);
type Observation = (Option<Row>, Vec<Row>, Vec<Row>, u64, u8);

#[tokio::test]
async fn typed_entity_growth() {
    let directory = std::path::PathBuf::from(
        std::env::var_os("IC_DB_ENTITY_GROWTH_DIR").expect("run make m1-entity-growth"),
    );
    for (variant, count) in [
        ("entities", 1_u8),
        ("entities", 8),
        ("entities", 32),
        ("retained", 1),
        ("retained", 8),
        ("retained", 32),
    ] {
        let (wasm, facts) = crate::artifact::read(
            std::fs::File::open(directory.join(format!("{variant}-{count}.wasm"))).unwrap(),
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
            "host={variant} entities={count} wasm_bytes={} code_section_bytes={} sha256={}",
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
            "host={variant} entities={count} installation_cycles={}",
            before_install
                .checked_sub(pic.cycle_balance(canister).await)
                .unwrap()
        );
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
            let (observed, opening, work): (Observation, [u64; 4], u64) = ic_wire::decode(
                &reply,
                Encoding::Frame {
                    method: 1,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap();
            (observed, (opening, work, cycles))
        };
        // Reuse the same primary key across every entity, with distinct values.
        // Validate all rows after all writes so later entities cannot mask aliasing.
        for mode in [1, 2] {
            let mut samples = Vec::new();
            for entity in 0..count {
                let row = (
                    1,
                    u64::from(mode),
                    u64::from(entity) * 100 + u64::from(mode),
                );
                let (observed, measurement) = call(entity, mode, row).await;
                assert_eq!(
                    observed,
                    (Some(row), vec![row], vec![row], u64::from(mode), count)
                );
                samples.push(measurement);
            }
            eprintln!(
                "host={variant} entities={count} mode={mode} opening_stages_work_cycles={samples:?}"
            );
            let mut reads = Vec::new();
            for entity in 0..count {
                let row = (
                    1,
                    u64::from(mode),
                    u64::from(entity) * 100 + u64::from(mode),
                );
                let (observed, measurement) = call(entity, 0, row).await;
                assert_eq!(
                    observed,
                    (Some(row), vec![row], vec![row], u64::from(mode), count)
                );
                reads.push(measurement);
                if mode == 2 {
                    let (old_index, _) = call(entity, 0, (1, 1, 0)).await;
                    assert_eq!(old_index, (Some(row), vec![row], vec![], 2, count));
                }
            }
            eprintln!(
                "host={variant} entities={count} after_mode={mode} read_opening_stages_work_cycles={reads:?}"
            );
        }
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
            .await
            .unwrap();
        // The first execution after upgrade constructs cold heap state, mutates
        // and traps. Both persistent effects and any retained host state roll back.
        let before = pic.get_stable_memory(canister).await;
        let error = pic
            .update_call(
                canister,
                canister,
                "exercise",
                ic_wire::encode(
                    &(0_u8, 4_u8, (1_u64, 9_u64, 999_u64)),
                    Encoding::Frame {
                        method: 1,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.error_code, ErrorCode::CanisterCalledTrap);
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let mut reopened = Vec::new();
        for entity in 0..count {
            let row = (1, 2, u64::from(entity) * 100 + 2);
            let (observed, measurement) = call(entity, 0, row).await;
            assert_eq!(observed, (Some(row), vec![row], vec![row], 2, count));
            reopened.push(measurement);
        }
        eprintln!(
            "host={variant} entities={count} after_upgrade_opening_stages_work_cycles={reopened:?}"
        );
        let mut deletes = Vec::new();
        for entity in 0..count {
            let (observed, measurement) = call(entity, 3, (1, 2, 0)).await;
            assert_eq!(observed, (None, vec![], vec![], 3, count));
            deletes.push(measurement);
        }
        for entity in 0..count {
            assert_eq!(
                call(entity, 0, (1, 2, 0)).await.0,
                (None, vec![], vec![], 3, count)
            );
        }
        eprintln!("host={variant} entities={count} delete_opening_stages_work_cycles={deletes:?}");
        owned.drop().await;
    }
}
