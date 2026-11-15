//! Same logical high-fan-out writes, with each engine's required completion work.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::native;
use sha2::{Digest, Sha256};

#[tokio::test]
async fn matched_high_fanout_writes() {
    for (label, variable, needs_settling) in [
        ("ic-db", "IC_DB_M1_RECORDS_WASM", false),
        ("icydb", "IC_DB_ICYDB_ENVELOPE_WASM", true),
    ] {
        let path = std::env::var_os(variable).expect("run make m1-envelope-comparison");
        let wasm = std::fs::read(path).unwrap();
        let hash: String = Sha256::digest(&wasm)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        eprintln!("{label} sha256={hash}");
        let owned = instance().await.unwrap();
        let pic = owned.instance();
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 100_000_000_000_000).await;
        pic.install_canister(canister, wasm, vec![], None).await;
        let settle = async || {
            let before = pic.cycle_balance(canister).await;
            let mut instructions = 0_u64;
            if needs_settling {
                for step in 1..=100 {
                    let bytes = pic
                        .update_call(
                            canister,
                            canister,
                            "advance_startup",
                            native::encode_request(1, &(), LIMITS).unwrap(),
                        )
                        .await
                        .unwrap();
                    let (ready, work): (bool, u64) =
                        native::decode_response(1, &bytes, LIMITS).unwrap();
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
                panic!("{label} settling exceeded 100 messages");
            }
            (0, 0, 0)
        };
        eprintln!("{label} startup={:?}", settle().await);
        if needs_settling {
            // Frozen IcyDB refuses the original 1018-byte indexed Text values.
            // Preserve that contract difference rather than timing it as a write.
            let before = pic.get_stable_memory(canister).await;
            let bytes = pic
                .update_call(
                    canister,
                    canister,
                    "envelope_comparison_write",
                    native::encode_request(93, &(0_u8, 0_u8, 1018_u16), LIMITS).unwrap(),
                )
                .await
                .unwrap();
            let (result, instructions): (Result<(), u16>, u64) =
                native::decode_response(93, &bytes, LIMITS).unwrap();
            assert_eq!(result, Err(22)); // frozen index-bounds diagnostic
            assert!(pic.get_stable_memory(canister).await == before);
            eprintln!(
                "icydb original shape refused: code=22 instructions={instructions}; stable bytes unchanged"
            );
            eprintln!("icydb refusal settling={:?}", settle().await);
        }
        for (operation, mode, generation) in [("insert", 0_u8, 0_u8), ("replace", 1, 1)] {
            let before = pic.cycle_balance(canister).await;
            let bytes = pic
                .update_call(
                    canister,
                    canister,
                    "envelope_comparison_write",
                    native::encode_request(93, &(mode, generation, 48_u16), LIMITS).unwrap(),
                )
                .await
                .unwrap();
            let cycles = before
                .checked_sub(pic.cycle_balance(canister).await)
                .unwrap();
            let (result, instructions): (Result<(), u16>, u64) =
                native::decode_response(93, &bytes, LIMITS).unwrap();
            result.unwrap();
            let (steps, settling_instructions, settling_cycles) = settle().await;
            eprintln!(
                "{label} {operation}: operation_instructions={instructions} settling_steps={steps} settling_instructions={settling_instructions} total_instructions={} operation_cycles={cycles} settling_cycles={settling_cycles}",
                instructions + settling_instructions
            );
            // Independent logical values, outside the measurement interval.
            let zero = "\0".repeat(48);
            let mut a = zero.clone();
            if generation == 1 {
                a.replace_range(..1, "\u{1}");
            }
            for id in 1_u64..=32 {
                let bytes = pic
                    .query_call(
                        canister,
                        canister,
                        "envelope_row",
                        native::encode_request(91, &id, LIMITS).unwrap(),
                    )
                    .await
                    .unwrap();
                let row: (String, String, String, String) =
                    native::decode_response(91, &bytes, LIMITS).unwrap();
                assert_eq!(row, (a.clone(), zero.clone(), zero.clone(), zero.clone()));
            }
            for input in 0_u8..=generation {
                let bytes = pic
                    .query_call(
                        canister,
                        canister,
                        "envelope_listing",
                        native::encode_request(92, &input, LIMITS).unwrap(),
                    )
                    .await
                    .unwrap();
                let ids: Vec<u64> = native::decode_response(92, &bytes, LIMITS).unwrap();
                let expected: Vec<u64> = if input == generation {
                    (1..=32).collect()
                } else {
                    Vec::new()
                };
                assert_eq!(ids, expected);
            }
        }
        owned.close().await;
    }
}
