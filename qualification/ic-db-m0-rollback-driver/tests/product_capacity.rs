//! Real migration growth refusal with unchanged backing and independent bytes.
mod envelope_support;
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};
use std::collections::BTreeMap;

type Reply = (Result<(u64, bool), u8>, u64, u64, u64);
type RawPage = (Vec<(Vec<u8>, Vec<u8>)>, Option<Vec<u8>>);

#[tokio::test]
async fn bounded_migration_retries_real_growth_refusal_and_finishes_validation_cleanup() {
    let directory = std::path::PathBuf::from(std::env::var_os("IC_DB_M3_DIR").unwrap());
    let wasm = std::fs::read(directory.join("maintenance-faults.wasm")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |op: u8, first: u16, limit: u8, bytes: u32, deny: bool, stop: u64| {
        pic.update_call(
            canister,
            canister,
            "migration_capacity",
            ic_wire::encode(
                &(op, first, limit, bytes, deny, stop),
                Encoding::Frame {
                    method: 140,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let decode = |bytes: Vec<u8>| {
        ic_wire::decode::<Reply>(
            &bytes,
            Encoding::Frame {
                method: 140,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    decode(call(0, 0, 32, 131072, false, 0).await.unwrap())
        .0
        .unwrap();
    for first in [1, 33, 65, 97] {
        assert_eq!(
            decode(call(1, first, 32, 131072, false, 0).await.unwrap())
                .0
                .unwrap()
                .0,
            32
        );
    }
    decode(call(2, 0, 32, 131072, false, 0).await.unwrap())
        .0
        .unwrap();
    let before = pic.get_stable_memory(canister).await;
    let initialized = match call(3, 0, 32, 131072, true, 0).await {
        Ok(reply) => {
            let (result, _, grows, _) = decode(reply);
            if result.is_ok() {
                assert_eq!(grows, 0);
                true
            } else {
                assert!(matches!(result, Err(3 | 4)));
                assert!(grows > 0);
                false
            }
        }
        Err(_) => false,
    };
    if !initialized {
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let (result, _, grows, instructions) =
            decode(call(3, 0, 32, 131072, false, 0).await.unwrap());
        result.unwrap();
        assert!(
            grows > 0,
            "initialization retry must cross the refused growth boundary"
        );
        println!("migration initialization: grows={grows}, instructions={instructions}");
    }
    // Structural refusals must return normally without moving the copy checkpoint.
    for (limit, bytes, expected) in [(33, 131072, 1), (32, 4095, 2)] {
        let before = pic.get_stable_memory(canister).await;
        let (result, writes, grows, _) = decode(call(4, 0, limit, bytes, false, 0).await.unwrap());
        assert_eq!(result, Err(expected));
        assert_eq!((writes, grows), (0, 0));
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    let before = pic.get_stable_memory(canister).await;
    assert!(call(4, 0, 32, 131072, false, 1).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let mut growth_checked = false;
    let mut total = 0;
    let mut complete = false;
    let mut max_copy_instructions = 0;
    for _ in 0..8 {
        let before = pic.get_stable_memory(canister).await;
        let reply = call(4, 0, 32, 131072, true, 0).await;
        let success = match reply {
            Ok(reply) => {
                let (result, writes, grows, instructions) = decode(reply);
                match result {
                    Ok(step) => {
                        assert_eq!(grows, 0);
                        Some((step, writes, grows, instructions))
                    }
                    Err(code) => {
                        assert_eq!(code, 3);
                        assert!(grows > 0);
                        None
                    }
                }
            }
            Err(_) => None,
        };
        let (step, _, _, instructions) = if let Some(reply) = success {
            reply
        } else {
            assert_eq!(
                pic.get_stable_memory(canister).await,
                before,
                "growth refusal changed data, indexes, allocator metadata or progress"
            );
            // Retry after upgrade to prove durable progress, not a warm heap accident.
            pic.advance_time(std::time::Duration::from_secs(600)).await;
            pic.tick().await;
            ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
            let (result, writes, grows, instructions) =
                decode(call(4, 0, 32, 131072, false, 0).await.unwrap());
            let step = result.unwrap();
            assert!(grows > 0 && step.0 > 0);
            assert!(pic.get_stable_memory(canister).await.len() > before.len());
            println!(
                "migration copy retry: rows={} writes={writes} grows={grows} instructions={instructions}",
                step.0
            );
            growth_checked = true;
            (step, writes, grows, instructions)
        };
        max_copy_instructions = max_copy_instructions.max(instructions);
        total += step.0;
        if step.1 {
            complete = true;
            break;
        }
    }
    assert!(growth_checked, "fixture never encountered real copy growth");
    assert!(complete);
    assert_eq!(total, 128);
    let mut validated = false;
    let mut max_validation_instructions = 0;
    for _ in 0..48 {
        let (result, _, _, instructions) = decode(call(5, 0, 32, 131072, false, 0).await.unwrap());
        max_validation_instructions = max_validation_instructions.max(instructions);
        if result.unwrap().1 {
            validated = true;
            break;
        }
    }
    assert!(validated);
    decode(call(6, 0, 32, 131072, false, 0).await.unwrap())
        .0
        .unwrap();
    let mut cleaned = false;
    let mut max_cleanup_instructions = 0;
    for _ in 0..8 {
        let (result, _, _, instructions) = decode(call(7, 0, 32, 131072, false, 0).await.unwrap());
        max_cleanup_instructions = max_cleanup_instructions.max(instructions);
        if result.unwrap().1 {
            cleaned = true;
            break;
        }
    }
    assert!(cleaned);
    // Audit after source cleanup as well as publication. This oracle imports
    // neither the product codec nor schema, and each response is bounded.
    let mut actual = BTreeMap::new();
    let mut after: Option<Vec<u8>> = None;
    let mut audited = false;
    for _ in 0..600 {
        let reply = pic
            .query_call(
                canister,
                canister,
                "migration_capacity_raw",
                ic_wire::encode(
                    &after,
                    Encoding::Frame {
                        method: 141,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let (rows, next): RawPage = ic_wire::decode(
            &reply,
            Encoding::Frame {
                method: 141,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap();
        for (key, value) in rows {
            if matches!(key[0], 2 | 3) {
                assert!(actual.insert(key, value).is_none());
            }
        }
        if next.is_none() {
            audited = true;
            break;
        }
        assert!(next > after);
        after = next;
    }
    assert!(audited);
    assert_eq!(actual.len(), 128 * 9);
    assert!(
        actual == envelope_support::expected(0, 128),
        "rows/indexes differ from independent oracle"
    );
    println!(
        "migration envelope maximum measured instructions: copy={max_copy_instructions} validation={max_validation_instructions} cleanup={max_cleanup_instructions}"
    );
    owned.drop().await;
}
