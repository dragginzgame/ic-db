//! Cross-chunk migration integrity; literal row/index bytes remain independent.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};
use std::collections::BTreeMap;

type Reply = (Result<(u32, bool), u8>, u64, u64);
type RawPage = (Vec<(Vec<u8>, Vec<u8>)>, Option<Vec<u8>>);
type Entries = BTreeMap<Vec<u8>, Vec<u8>>;

// Independently calculated logical-name handles. No product schema or codec imports.
const ENTITY: u32 = 270_368_461;
const NEXT: u32 = 2_005_894_529;
const RANK: u32 = 2_895_879_719;

fn expected(target: bool, ids: impl Iterator<Item = u64>, fan_in: bool) -> Entries {
    let mut result = Entries::new();
    for id in ids {
        let peer = if fan_in { 255 } else { (id + 128) % 256 };
        let mut key = vec![2];
        key.extend(ENTITY.to_be_bytes());
        key.extend(id.to_be_bytes());
        let mut row = id.to_le_bytes().to_vec();
        row.push(1);
        row.extend(peer.to_le_bytes());
        if target {
            row.extend((id + 1000).to_le_bytes());
        }
        result.insert(key, row);
        let mut key = vec![3];
        key.extend(ENTITY.to_be_bytes());
        key.extend(NEXT.to_be_bytes());
        key.push(1);
        key.extend(peer.to_be_bytes());
        key.extend(id.to_be_bytes()); // declared second component
        key.extend(id.to_be_bytes()); // nonunique owner suffix
        result.insert(key, id.to_be_bytes().to_vec());
        if target {
            let mut key = vec![3];
            key.extend(ENTITY.to_be_bytes());
            key.extend(RANK.to_be_bytes());
            key.extend((id + 1000).to_be_bytes());
            result.insert(key, id.to_be_bytes().to_vec());
        }
    }
    result
}
fn data(mut entries: Entries) -> Entries {
    entries.retain(|key, _| matches!(key.first(), Some(2 | 3)));
    entries
}

#[tokio::test]
async fn cross_chunk_references_validate_before_publication_and_cancel_safely() {
    let directory = std::path::PathBuf::from(std::env::var_os("IC_DB_M3_DIR").unwrap());
    let wasm = std::fs::read(directory.join("maintenance-faults.wasm")).unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    for bad in [false, true] {
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 10_000_000_000_000).await;
        pic.install_canister(canister, wasm.clone(), vec![], None)
            .await;
        let call = async |operation: u8, first: u16, limit: u8, stop: u64| {
            pic.update_call(
                canister,
                canister,
                "migration_references",
                ic_wire::encode(
                    &(operation, first, limit, bad, stop),
                    Encoding::Frame {
                        method: 144,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
        };
        let decode = |reply: Vec<u8>| {
            ic_wire::decode::<Reply>(
                &reply,
                Encoding::Frame {
                    method: 144,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap()
        };
        let raw = async |target: bool| {
            let mut result = Entries::new();
            let mut after: Option<Vec<u8>> = None;
            for _ in 0..64 {
                let reply = pic
                    .query_call(
                        canister,
                        canister,
                        "migration_references_raw",
                        ic_wire::encode(
                            &(target, &after),
                            Encoding::Frame {
                                method: 145,
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
                        method: 145,
                        direction: Direction::Response,
                    },
                    LIMITS,
                )
                .unwrap();
                for (key, value) in rows {
                    assert!(result.insert(key, value).is_none());
                }
                if next.is_none() {
                    return result;
                }
                assert!(next > after);
                after = next;
            }
            panic!("raw inspection failed to finish within its bound");
        };
        decode(call(0, 0, 32, 0).await.unwrap()).0.unwrap();
        for operation in [1, 2] {
            for first in (0..256).step_by(32) {
                assert_eq!(
                    decode(call(operation, first, 32, 0).await.unwrap())
                        .0
                        .unwrap()
                        .0,
                    32
                );
            }
        }
        assert_eq!(data(raw(false).await), expected(false, 0..256, false));
        decode(call(3, 0, 32, 0).await.unwrap()).0.unwrap();
        decode(call(4, 0, 32, 0).await.unwrap()).0.unwrap();
        for operation in [10, 11] {
            let before = pic.get_stable_memory(canister).await;
            assert_eq!(decode(call(operation, 0, 32, 0).await.unwrap()).0, Err(2));
            assert_eq!(pic.get_stable_memory(canister).await, before);
        }
        let before = pic.get_stable_memory(canister).await;
        assert!(call(5, 0, 32, 1).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let mut total = 0;
        let mut complete = false;
        let mut maximum_copy = 0;
        for _ in 0..16 {
            let (result, _, instructions) = decode(call(5, 0, 32, 0).await.unwrap());
            maximum_copy = maximum_copy.max(instructions);
            let (rows, done) = result.unwrap();
            total += rows;
            if total == 32 {
                // All these rows reference targets that have not been copied yet.
                assert_eq!(data(raw(true).await), expected(true, 0..32, false));
                let before = pic.get_stable_memory(canister).await;
                assert_eq!(decode(call(7, 0, 32, 0).await.unwrap()).0, Err(3));
                assert_eq!(pic.get_stable_memory(canister).await, before);
                pic.advance_time(std::time::Duration::from_secs(600)).await;
                pic.tick().await;
                pic.upgrade_canister(canister, wasm.clone(), vec![], None)
                    .await
                    .unwrap();
            }
            if done {
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert_eq!(total, 256);
        // A trap after the validation checkpoint must restore data and progress.
        let before = pic.get_stable_memory(canister).await;
        assert!(call(6, 0, 32, u64::MAX).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let mut validated = false;
        let mut refused = false;
        let mut maximum_validation = 0;
        for step in 0..64 {
            let before = pic.get_stable_memory(canister).await;
            let (result, writes, instructions) = decode(call(6, 0, 32, 0).await.unwrap());
            maximum_validation = maximum_validation.max(instructions);
            if result == Err(1) {
                assert!(
                    bad && step > 1,
                    "failure must occur after durable validation progress"
                );
                assert_eq!(writes, 0);
                assert_eq!(pic.get_stable_memory(canister).await, before);
                refused = true;
                break;
            }
            if result.unwrap().1 {
                validated = true;
                break;
            }
            if step == 1 {
                pic.advance_time(std::time::Duration::from_secs(600)).await;
                pic.tick().await;
                pic.upgrade_canister(canister, wasm.clone(), vec![], None)
                    .await
                    .unwrap();
            }
        }
        assert_eq!((validated, refused), (!bad, bad));
        if bad {
            let before = pic.get_stable_memory(canister).await;
            assert_eq!(decode(call(7, 0, 32, 0).await.unwrap()).0, Err(3));
            assert_eq!(pic.get_stable_memory(canister).await, before);
        } else {
            decode(call(7, 0, 32, 0).await.unwrap()).0.unwrap();
            assert_eq!(data(raw(true).await), expected(true, 0..256, false));
            decode(call(11, 0, 32, 0).await.unwrap()).0.unwrap();
        }
        let mut cleaned = false;
        let mut maximum_cleanup = 0;
        for _ in 0..32 {
            let (result, _, instructions) =
                decode(call(if bad { 9 } else { 8 }, 0, 32, 0).await.unwrap());
            maximum_cleanup = maximum_cleanup.max(instructions);
            if result.unwrap().1 {
                cleaned = true;
                break;
            }
        }
        assert!(cleaned);
        if bad {
            decode(call(10, 0, 32, 0).await.unwrap()).0.unwrap();
            assert_eq!(data(raw(false).await), expected(false, 0..256, false));
            assert_eq!(
                raw(true).await.keys().cloned().collect::<Vec<_>>(),
                [vec![0]]
            );
        } else {
            assert_eq!(
                raw(false).await.keys().cloned().collect::<Vec<_>>(),
                [vec![0]]
            );
            let before = pic.get_stable_memory(canister).await;
            assert_eq!(decode(call(12, 0, 32, 0).await.unwrap()).0, Err(1));
            assert_eq!(pic.get_stable_memory(canister).await, before);
            decode(call(12, 1, 32, 0).await.unwrap()).0.unwrap();
            let remaining = || (0..256).filter(|id| !matches!(id, 0 | 128));
            assert_eq!(data(raw(true).await), expected(true, remaining(), false));

            // Many-to-one references after publication: 253 dependants and the
            // target's self-reference. Repoint using bounded ordinary batches.
            for first in (0..256).step_by(32) {
                decode(call(13, first, 32, 0).await.unwrap()).0.unwrap();
            }
            assert_eq!(data(raw(true).await), expected(true, remaining(), true));
            let before = pic.get_stable_memory(canister).await;
            let (result, writes, refusal_instructions) = decode(call(14, 0, 32, 0).await.unwrap());
            assert_eq!(result, Err(1));
            assert_eq!(writes, 0);
            assert_eq!(pic.get_stable_memory(canister).await, before);

            // A later application trap rolls back a successful deletion batch.
            assert!(call(15, 0, 32, u64::MAX).await.is_err());
            assert_eq!(pic.get_stable_memory(canister).await, before);
            let mut maximum_delete = 0;
            for first in (0..256).step_by(32) {
                let (result, _, instructions) = decode(call(15, first, 32, 0).await.unwrap());
                result.unwrap();
                maximum_delete = maximum_delete.max(instructions);
                let survivors = remaining().filter(|id| *id >= u64::from(first) + 32);
                assert_eq!(data(raw(true).await), expected(true, survivors, true));
                if first == 0 {
                    pic.advance_time(std::time::Duration::from_secs(600)).await;
                    pic.tick().await;
                    pic.upgrade_canister(canister, wasm.clone(), vec![], None)
                        .await
                        .unwrap();
                }
            }
            // The final 32-row batch removes the target and every remaining edge.
            decode(call(11, 0, 32, 0).await.unwrap()).0.unwrap();
            println!(
                "post-migration fan-in: refusal={refusal_instructions} delete={maximum_delete} maximum measured instructions"
            );
        }
        println!(
            "reference migration bad={bad}: copy={maximum_copy} validation={maximum_validation} cleanup={maximum_cleanup} maximum measured instructions"
        );
    }
}
