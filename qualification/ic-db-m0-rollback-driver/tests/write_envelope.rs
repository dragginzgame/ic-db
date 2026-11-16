//! Real Wasm envelope witness with independent row/index byte expectations.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_testkit::pocket_ic::ErrorCode;
use ic_wire::{Direction, Encoding};

use std::collections::BTreeMap;

type RawPage = Vec<(Vec<u8>, Vec<u8>)>;
type Read = (Vec<u64>, u64, u64);
mod envelope_support;

#[tokio::test]
async fn maximum_rows_batch_and_fanout_complete_or_refuse_atomically() {
    let wasm = std::fs::read(
        std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-write-envelope"),
    )
    .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |mode: u8, count: u8, generation: u8, oversized: bool, trap: bool| {
        let before = pic.cycle_balance(canister).await;
        let reply = pic
            .update_call(
                canister,
                canister,
                "envelope_write",
                ic_wire::encode(
                    &(mode, count, generation, oversized, trap),
                    Encoding::Frame {
                        method: 88,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await;
        let cycles = before
            .checked_sub(pic.cycle_balance(canister).await)
            .unwrap();
        (reply, cycles)
    };
    let decode = |bytes: Vec<u8>| {
        ic_wire::decode::<(Result<(), u8>, u64)>(
            &bytes,
            Encoding::Frame {
                method: 88,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let read = async |index: u8, generation: u8| {
        let bytes = pic
            .query_call(
                canister,
                canister,
                "envelope_read",
                ic_wire::encode(
                    &(index, generation),
                    Encoding::Frame {
                        method: 89,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Read>(
            &bytes,
            Encoding::Frame {
                method: 89,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let audit = async |generation: Option<u8>| {
        let mut actual = BTreeMap::new();
        for tag in [2_u8, 3] {
            let mut after: Option<Vec<u8>> = None;
            for step in 0..=129 {
                let bytes = pic
                    .query_call(
                        canister,
                        canister,
                        "envelope_raw",
                        ic_wire::encode(
                            &(tag, &after),
                            Encoding::Frame {
                                method: 90,
                                direction: Direction::Request,
                            },
                            LIMITS,
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap();
                let entries: RawPage = ic_wire::decode(
                    &bytes,
                    Encoding::Frame {
                        method: 90,
                        direction: Direction::Response,
                    },
                    LIMITS,
                )
                .unwrap();
                assert!(entries.len() <= 2);
                let next = entries.last().map(|(key, _)| key.clone());
                for (key, value) in entries {
                    assert!(actual.insert(key, value).is_none());
                }
                if next.is_none() {
                    break;
                }
                assert!(
                    next > after && step < 129,
                    "raw audit must make bounded progress"
                );
                after = next;
            }
        }
        let expected = generation.map_or_else(BTreeMap::new, |generation| {
            envelope_support::expected(generation, 32)
        });
        assert_eq!(actual.len(), expected.len());
        assert!(
            actual == expected,
            "raw rows/indexes differ from independent framing oracle"
        );
    };
    let original = pic.get_stable_memory(canister).await;
    // Four 61-zero-byte components plus namespace/owner make a 513-byte key,
    // while the row is comfortably below 4096 bytes. Refuse before mutation.
    let bytes = pic
        .update_call(
            canister,
            canister,
            "envelope_comparison_write",
            ic_wire::encode(
                &(0_u8, 0_u8, 61_u16),
                Encoding::Frame {
                    method: 93,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let (result, _): (Result<(), u16>, u64) = ic_wire::decode(
        &bytes,
        Encoding::Frame {
            method: 93,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap();
    assert_eq!(result, Err(1));
    assert!(pic.get_stable_memory(canister).await == original);
    for (count, oversized) in [(33, false), (1, true)] {
        let (reply, cycles) = write(0, count, 0, oversized, false).await;
        let (result, work) = decode(reply.unwrap());
        assert_eq!(result, Err(1));
        assert!(pic.get_stable_memory(canister).await == original);
        eprintln!(
            "envelope refusal count={count} oversized={oversized}: instructions={work} cycles={cycles}"
        );
    }
    let (reply, cycles) = write(0, 32, 0, false, false).await;
    let (result, work) = decode(reply.unwrap());
    result.unwrap();
    let populated = pic.get_stable_memory(canister).await;
    eprintln!(
        "envelope insert: instructions={work} cycles={cycles} physical_bytes={}",
        populated.len()
    );
    audit(Some(0)).await;
    for index in 0..8 {
        let (ids, revision, work) = read(index, 0).await;
        assert_eq!(ids, (1..=32).collect::<Vec<_>>());
        assert_eq!(revision, 1);
        eprintln!("envelope index {index}: instructions={work}");
    }
    let (reply, cycles) = write(1, 32, 0, false, false).await;
    let (result, work) = decode(reply.unwrap());
    result.unwrap();
    assert!(pic.get_stable_memory(canister).await == populated);
    eprintln!("envelope no-op: instructions={work} cycles={cycles}");
    let (reply, _) = write(0, 32, 0, false, false).await;
    assert_eq!(decode(reply.unwrap()).0, Err(2));
    assert!(pic.get_stable_memory(canister).await == populated);
    let (reply, _) = write(1, 32, 1, false, true).await;
    assert_eq!(reply.unwrap_err().error_code, ErrorCode::CanisterCalledTrap);
    assert!(pic.get_stable_memory(canister).await == populated);
    let (reply, cycles) = write(1, 32, 1, false, false).await;
    let (result, work) = decode(reply.unwrap());
    result.unwrap();
    eprintln!("envelope replacement: instructions={work} cycles={cycles}");
    audit(Some(1)).await;
    let updated = pic.get_stable_memory(canister).await;
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(read(0, 1).await.1, 2);
    assert!(pic.get_stable_memory(canister).await == updated);
    let (reply, cycles) = write(2, 32, 0, false, false).await;
    let (result, work) = decode(reply.unwrap());
    result.unwrap();
    for index in 0..8 {
        let (ids, revision, _) = read(index, 1).await;
        assert!(ids.is_empty());
        assert_eq!(revision, 3);
    }
    eprintln!(
        "envelope delete: instructions={work} cycles={cycles} physical_bytes={}",
        pic.get_stable_memory(canister).await.len()
    );
    audit(None).await;
    owned.close().await;
}
