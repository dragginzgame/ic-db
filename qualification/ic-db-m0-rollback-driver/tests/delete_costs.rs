//! Compare a released or experimental batch envelope on actual installed storage.

mod wire_call;

use ic_db_m0_rollback_driver::instance;
use wire_call::call;

#[tokio::test]
#[ignore = "requires measurement Wasm and PocketIC"]
async fn atomic_delete_costs() {
    let wasm = std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").unwrap()).unwrap();
    let expanded = std::env::var_os("IC_DB_DELETE_EXPERIMENT_256").is_some();
    let owned = instance().await.unwrap();
    let pic = &owned;
    for owner in [false, true] {
        for (total, descending, interleaved) in [
            (32_u16, false, false),
            (32, true, false),
            (32, false, true),
            (256, false, false),
            (256, true, false),
            (256, false, true),
        ] {
            let canister = pic.create_canister().await;
            pic.add_cycles(canister, 3_000_000_000_000).await;
            pic.install_canister(canister, wasm.clone(), vec![], None)
                .await;
            let children = total - u16::from(owner);
            // Installation seeds entries 1..=6 and document 1 referencing entry 1.
            // Every additional document copies that reference. Entry has no child reference.
            let rows: Vec<_> = (2..=children)
                .map(|id| (u64::from(id), String::from("variant"), false))
                .collect();
            for chunk in rows.chunks(32) {
                call::<Result<(), u8>>(pic, canister, "text_index_batch", 23, &(chunk, false))
                    .await
                    .unwrap();
            }
            if interleaved {
                // Split every index by alternating its leading values across owner IDs.
                call::<Result<(), u8>>(
                    pic,
                    canister,
                    "replace_document",
                    8,
                    &(
                        "other",
                        None::<String>,
                        true,
                        None::<u64>,
                        20_i64,
                        None::<i64>,
                    ),
                )
                .await
                .unwrap();
                let even: Vec<_> = (2..=children)
                    .step_by(2)
                    .map(|id| (u64::from(id), "other", true))
                    .collect();
                for chunk in even.chunks(32) {
                    call::<Result<(), u8>>(pic, canister, "text_index_batch", 23, &(chunk, false))
                        .await
                        .unwrap();
                }
            }
            let before = pic.cycle_balance(canister).await;
            let (accepted, build, commit, remaining, parents): (bool, u64, u64, u64, u64) = call(
                pic,
                canister,
                "measured_delete",
                247,
                &(children, owner, descending),
            )
            .await;
            let cycles = before - pic.cycle_balance(canister).await;
            assert_eq!(accepted, total <= 32 || expanded);
            assert_eq!(
                (remaining, parents),
                if accepted {
                    (0, 6 - u64::from(owner))
                } else {
                    (u64::from(children), 6)
                }
            );
            println!(
                "delete,total={total},owner={owner},descending={descending},interleaved={interleaved},accepted={accepted},build={build},commit={commit},update_cycles={cycles}"
            );
        }
    }
    owned.drop().await;
}
