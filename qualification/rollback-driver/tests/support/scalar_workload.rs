//! One fixed scalar workload and independent tuple oracle for both engines.

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_host_artifacts::wasm::{InspectionLimits, inspect};
use ic_wire::native;
use sha2::{Digest, Sha256};

type Row = (u64, u64, u64);

pub async fn run(label: &str, wasm_path: &std::ffi::OsStr, needs_settling: bool) {
    let wasm = std::fs::read(wasm_path).unwrap();
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
    let methods: Vec<_> = facts
        .exports
        .keys()
        .copied()
        .filter(|name| name.starts_with("canister_query ") || name.starts_with("canister_update "))
        .collect();
    assert_eq!(
        methods,
        [
            "canister_query listing",
            "canister_query lookup",
            "canister_update advance_startup",
            "canister_update insert",
            "canister_update read_modify_write",
        ],
        "unexpected reachable endpoint in {label} benchmark"
    );
    eprintln!(
        "{label} exports={:?}",
        facts.exports.keys().collect::<Vec<_>>()
    );
    eprintln!(
        "{label} wasm_bytes={} code_section_bytes={} sha256={}",
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
    let before_install = pic.cycle_balance(canister).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    eprintln!(
        "{label} installation_cycles={}",
        before_install
            .checked_sub(pic.cycle_balance(canister).await)
            .unwrap()
    );

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
        panic!("{label} startup did not finish within 100 messages");
    };
    // IC DB has no deferred ordinary-write work. Do not invent a settling call
    // or hide IcyDB's required follow-up calls from complete-journey totals.
    let settle = async || {
        if needs_settling {
            startup().await
        } else {
            (0, 0, 0)
        }
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
        "{label} startup_steps_instructions_cycles={:?}",
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
        settling.push(settle().await);
    }
    eprintln!("{label} insert_instructions={insert_instructions:?}");
    eprintln!("{label} insert_cycles={insert_cycles:?}");
    eprintln!("{label} insert_settling_steps_instructions_cycles={settling:?}");

    let (found, instructions) = lookup(7).await;
    assert_eq!(found, Some((7, 3, 70)));
    eprintln!("{label} lookup_hit_instructions={instructions}");
    let (found, instructions) = lookup(99).await;
    assert_eq!(found, None);
    eprintln!("{label} lookup_miss_instructions={instructions}");

    let (accepted, instructions, cycles) = insert((7, 0, 999)).await;
    assert!(!accepted);
    eprintln!("{label} rejected_insert_settling={:?}", settle().await);
    assert_eq!(lookup(7).await.0, Some((7, 3, 70)));
    eprintln!("{label} rejected_insert_instructions={instructions} cycles={cycles}");

    let (rows, instructions) = listing(3).await;
    assert_eq!(
        rows,
        source
            .iter()
            .copied()
            .filter(|row| row.1 == 3)
            .collect::<Vec<_>>()
    );
    eprintln!("{label} indexed_listing_instructions={instructions}");

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
    eprintln!("{label} replacement_settling={:?}", settle().await);
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
    eprintln!("{label} read_modify_replace_instructions={instructions} cycles={cycles}");

    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    eprintln!(
        "{label} reopen_steps_instructions_cycles={:?}",
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
        "{label} stable_memory_bytes={}",
        pic.get_stable_memory(canister).await.len()
    );
    owned.close().await;
}
