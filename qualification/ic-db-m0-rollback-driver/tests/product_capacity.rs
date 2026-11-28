//! Real migration growth refusal with unchanged backing and independent bytes.

mod envelope_support;
mod wire_call;

use ic_db_m0_rollback_driver::instance;
use std::collections::BTreeMap;
use wire_call::wire_frame;

type Reply = (Result<(u64, bool), u8>, u64, u64, u64);
type RawPage = (Vec<(Vec<u8>, ic_wire::ByteBuf)>, Option<Vec<u8>>);

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
    let migration =
        async |second: bool, op: u8, first: u16, limit: u8, bytes: u32, deny: bool, stop: u64| {
            pic.update_call(
                canister,
                canister,
                "migration_capacity",
                wire_frame::request(140, &((second, op), first, limit, bytes, deny, stop)),
            )
            .await
        };
    let call =
        |op, first, limit, bytes, deny, stop| migration(false, op, first, limit, bytes, deny, stop);
    let decode = |bytes: Vec<u8>| wire_frame::response::<Reply>(140, &bytes);
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
    let frozen_bytes = before.len();
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
    // The byte budget can stop a chunk before the row limit. The second row
    // must still be available when the next message resumes after an upgrade.
    let before_copy = pic.cycle_balance(canister).await;
    let (result, _, _, first_cost) = decode(call(4, 0, 32, 8191, false, 0).await.unwrap());
    println!(
        "first copy charged cycles={}",
        before_copy - pic.cycle_balance(canister).await
    );
    let first = result.unwrap();
    assert_eq!(first, (1, false));
    let before = pic.get_stable_memory(canister).await;
    let (result, writes, grows, _) = decode(call(4, 0, 32, 4095, false, 0).await.unwrap());
    assert_eq!((result, writes, grows), (Err(2), 0, 0));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let trap = call(4, 0, 32, 4096, false, 1).await.unwrap_err();
    assert!(
        trap.reject_message
            .contains("M1 injected trap after backing write"),
        "{trap:?}"
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    pic.advance_time(std::time::Duration::from_secs(600)).await;
    pic.tick().await;
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
        .await
        .unwrap();
    let (result, _, _, retry_cost) = decode(call(4, 0, 32, 4096, false, 0).await.unwrap());
    let second = result.unwrap();
    assert_eq!(second, (1, false));
    println!(
        "single-row migration copy: 8191-byte budget={first_cost} instructions, 4096-byte retry={retry_cost} instructions"
    );
    let mut growth_checked = false;
    let mut total = first.0 + second.0;
    let mut complete = false;
    let mut max_copy_instructions = first_cost.max(retry_cost);
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
    let copied_bytes = pic.get_stable_memory(canister).await.len();
    let mut validated = false;
    let mut max_validation_instructions = 0;
    for page in 0..48 {
        let before = pic.cycle_balance(canister).await;
        let (result, _, _, instructions) = decode(call(5, 0, 32, 131072, false, 0).await.unwrap());
        if page == 0 {
            println!(
                "first validation page instructions={instructions} charged cycles={}",
                before - pic.cycle_balance(canister).await
            );
        }
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
    let published_bytes = pic.get_stable_memory(canister).await.len();
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
    let cleaned_bytes = pic.get_stable_memory(canister).await.len();
    println!(
        "stable allocation bytes: frozen={frozen_bytes} copied={copied_bytes} published={published_bytes} cleaned={cleaned_bytes}"
    );
    // Audit after source cleanup as well as publication. This oracle imports
    // neither the product codec nor schema, and each response is bounded.
    let audit = async |second: bool| {
        let mut actual = BTreeMap::new();
        let mut after: Option<Vec<u8>> = None;
        let mut audited = false;
        for _ in 0..600 {
            let reply = pic
                .query_call(
                    canister,
                    canister,
                    "migration_capacity_raw",
                    wire_frame::request(141, &(second, after.clone())),
                )
                .await
                .unwrap();
            let (rows, next): RawPage = wire_frame::response(141, &reply);
            for (key, value) in rows {
                if matches!(key[0], 2 | 3) {
                    assert!(actual.insert(key, value.into_vec()).is_none());
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
    };
    audit(false).await;

    let next = async |op| {
        decode(migration(true, op, 0, 32, 131072, false, 0).await.unwrap())
            .0
            .unwrap()
    };
    next(2).await;
    let frozen = pic.get_stable_memory(canister).await.len();
    next(3).await;
    let initialized = pic.get_stable_memory(canister).await.len();
    let mut total = 0;
    for step in 1..=5 {
        let (rows, complete) = next(4).await;
        assert!(rows <= 32 && (rows > 0 || complete));
        total += rows;
        if complete {
            break;
        }
        assert!(step < 5, "second copy did not finish");
    }
    assert_eq!(total, 128);
    let copied = pic.get_stable_memory(canister).await.len();
    for step in 1..=96 {
        if next(5).await.1 {
            break;
        }
        assert!(step < 96, "second validation did not finish");
    }
    assert!(next(6).await.1);
    let published = pic.get_stable_memory(canister).await.len();
    for step in 1..=48 {
        if next(7).await.1 {
            break;
        }
        assert!(step < 48, "indexed source cleanup did not finish");
    }
    let cleaned = pic.get_stable_memory(canister).await.len();
    audit(true).await;
    println!(
        "second stable allocation bytes: frozen={frozen} initialized={initialized} copied={copied} published={published} cleaned={cleaned}"
    );
    println!(
        "migration envelope maximum measured instructions: copy={max_copy_instructions} validation={max_validation_instructions} cleanup={max_cleanup_instructions}"
    );
    owned.drop().await;
}

#[tokio::test]
async fn completed_cancellation_cost_on_the_same_handle() {
    let dir = std::path::PathBuf::from(std::env::var_os("IC_DB_M3_DIR").unwrap());
    let wasm = std::fs::read(dir.join("maintenance-faults.wasm")).unwrap();
    let pic = instance().await.unwrap();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    for op in [0u8, 2, 3, 8] {
        let response: Reply = wire_call::call(
            &pic,
            canister,
            "migration_capacity",
            140,
            &((false, op), 0u16, 32u8, 131072u32, false, 0u64),
        )
        .await;
        let (inner, done) = response.0.unwrap();
        if op == 8 {
            assert!(done);
            println!("repeat completed cancellation instructions={inner}");
        }
    }
    pic.drop().await;
}
