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

#[tokio::test]
async fn repeated_parent_delete_refusals() {
    let wasm = std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").unwrap()).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;

    // Installation gives entry 1 one child. Grow the same reverse-index range
    // between trials and submit the identical parent-only request each time.
    let mut seeded = 1_u64;
    for children in [1, 32, 255] {
        let rows: Vec<_> = (seeded + 1..=children)
            .map(|id| (id, "variant", false))
            .collect();
        for chunk in rows.chunks(32) {
            call::<Result<(), u8>>(pic, canister, "text_index_batch", 23, &(chunk, false))
                .await
                .unwrap();
        }
        seeded = children;
        let backing = pic.get_stable_memory(canister).await;
        // The same endpoint opens the store before rejecting an unknown mode.
        let before = pic.cycle_balance(canister).await;
        let result: Result<(), u8> = call(pic, canister, "reference_batch", 10, &255_u8).await;
        let control = before - pic.cycle_balance(canister).await;
        assert_eq!(result, Err(2));
        assert_eq!(pic.get_stable_memory(canister).await, backing);
        println!("open-only control: children={children} update_cycles={control}");
        for attempt in 1..=3 {
            let before = pic.cycle_balance(canister).await;
            let result: Result<(), u8> = call(pic, canister, "reference_batch", 10, &1_u8).await;
            let charged = before - pic.cycle_balance(canister).await;
            assert_eq!(result, Err(1)); // Only ReferenceViolation has this code.
            assert_eq!(pic.get_stable_memory(canister).await, backing);
            println!(
                "restricted delete: children={children} attempt={attempt} update_cycles={charged}"
            );
        }
    }
    owned.drop().await;
}
