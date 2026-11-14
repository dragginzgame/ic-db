//! Correctness-controlled measurements of the frozen SQL-disabled IcyDB engine.
//! No IC DB speedup or size comparison is inferred from this one-engine baseline.

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_host_artifacts::wasm::{InspectionLimits, inspect};
use ic_wire::native;
use sha2::{Digest, Sha256};

type Row = (u64, u64, u64);

#[tokio::test]
async fn frozen_icydb_baseline() {
    let wasm = std::fs::read(
        std::env::var_os("IC_DB_ICYDB_BASELINE_WASM").expect("run make m0-icydb-baseline"),
    )
    .unwrap();
    let facts = inspect(
        &wasm,
        InspectionLimits {
            module_bytes: 32 * 1024 * 1024,
            sections: 128,
            exports: 128,
            custom_sections: 32,
        },
    )
    .unwrap();
    eprintln!(
        "baseline wasm_bytes={} code_section_bytes={} sha256={}",
        facts.raw_bytes,
        facts.code_section_bytes,
        Sha256::digest(&wasm)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );

    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;

    let startup = async || {
        let before = pic.cycle_balance(canister).await;
        let mut instructions = 0;
        for step in 1..=100 {
            let reply = pic
                .update_call(
                    canister,
                    canister,
                    "advance_startup",
                    native::encode_request(1, &(), LIMITS).unwrap(),
                )
                .await
                .unwrap();
            let (ready, work): (bool, u64) = native::decode_response(1, &reply, LIMITS).unwrap();
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
        panic!("baseline startup did not finish within 100 messages");
    };
    let insert = async |value: Row| {
        let before = pic.cycle_balance(canister).await;
        let reply = pic
            .update_call(
                canister,
                canister,
                "insert",
                native::encode_request(2, &value, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        let cycles = before
            .checked_sub(pic.cycle_balance(canister).await)
            .unwrap();
        let (accepted, instructions): (bool, u64) =
            native::decode_response(2, &reply, LIMITS).unwrap();
        (accepted, instructions, cycles)
    };
    let lookup = async |id: u64| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "lookup",
                native::encode_request(3, &id, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Option<Row>, u64)>(3, &reply, LIMITS).unwrap()
    };
    let listing = async |category: u64| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "listing",
                native::encode_request(5, &category, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<Row>, u64)>(5, &reply, LIMITS).unwrap()
    };
    eprintln!(
        "baseline startup_steps_instructions_cycles={:?}",
        startup().await
    );

    let mut source = Vec::new();
    let mut insert_instructions = Vec::new();
    let mut insert_cycles = Vec::new();
    let mut settling = Vec::new();
    for id in 1..=32 {
        let value = (id, id % 4, id * 10);
        let (accepted, instructions, cycles) = insert(value).await;
        assert!(accepted);
        source.push(value);
        insert_instructions.push(instructions);
        insert_cycles.push(cycles);
        settling.push(startup().await);
    }
    eprintln!("baseline insert_instructions={insert_instructions:?}");
    eprintln!("baseline insert_cycles={insert_cycles:?}");
    eprintln!("baseline insert_settling_steps_instructions_cycles={settling:?}");

    let (found, instructions) = lookup(7).await;
    assert_eq!(found, Some((7, 3, 70)));
    eprintln!("baseline lookup_hit_instructions={instructions}");
    let (found, instructions) = lookup(99).await;
    assert_eq!(found, None);
    eprintln!("baseline lookup_miss_instructions={instructions}");

    let (accepted, instructions, cycles) = insert((7, 0, 999)).await;
    assert!(!accepted);
    eprintln!("baseline rejected_insert_settling={:?}", startup().await);
    assert_eq!(lookup(7).await.0, Some((7, 3, 70)));
    eprintln!("baseline rejected_insert_instructions={instructions} cycles={cycles}");

    let (rows, instructions) = listing(3).await;
    assert_eq!(
        rows,
        source
            .iter()
            .copied()
            .filter(|row| row.1 == 3)
            .collect::<Vec<_>>()
    );
    eprintln!("baseline indexed_listing_instructions={instructions}");

    let replacement: Row = (7, 0, 707);
    let before = pic.cycle_balance(canister).await;
    let reply = pic
        .update_call(
            canister,
            canister,
            "read_modify_write",
            native::encode_request(4, &replacement, LIMITS).unwrap(),
        )
        .await
        .unwrap();
    let cycles = before
        .checked_sub(pic.cycle_balance(canister).await)
        .unwrap();
    let (value, instructions): (Option<Row>, u64) =
        native::decode_response(4, &reply, LIMITS).unwrap();
    assert_eq!(value, Some(replacement));
    eprintln!("baseline replacement_settling={:?}", startup().await);
    *source.iter_mut().find(|row| row.0 == 7).unwrap() = replacement;
    assert_eq!(lookup(7).await.0, Some(replacement));
    for category in [0, 3] {
        assert_eq!(
            listing(category).await.0,
            source
                .iter()
                .copied()
                .filter(|row| row.1 == category)
                .collect::<Vec<_>>()
        );
    }
    eprintln!("baseline read_modify_replace_instructions={instructions} cycles={cycles}");

    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    eprintln!(
        "baseline reopen_steps_instructions_cycles={:?}",
        startup().await
    );
    // Independent expected tuples survive heap loss and durable startup recovery.
    for expected in &source {
        assert_eq!(lookup(expected.0).await.0, Some(*expected));
    }
    for category in 0..4 {
        assert_eq!(
            listing(category).await.0,
            source
                .iter()
                .copied()
                .filter(|row| row.1 == category)
                .collect::<Vec<_>>()
        );
    }
    eprintln!(
        "baseline stable_memory_bytes={}",
        pic.get_stable_memory(canister).await.len()
    );
}
