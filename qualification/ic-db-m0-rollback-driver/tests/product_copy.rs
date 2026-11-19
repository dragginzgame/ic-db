//! Copy/checkpoint and cancellation must share the real IC rollback boundary.

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

type Entries = (bool, Vec<(Vec<u8>, Vec<u8>)>);

#[tokio::test]
async fn copy_progress_and_cleanup_survive_traps_and_upgrade_without_publishing() {
    exercise(false).await;
}

#[tokio::test]
async fn validation_and_publication_resume_atomically_and_retire_source() {
    exercise(true).await;
}

async fn exercise(publish: bool) {
    let directory = std::path::PathBuf::from(
        std::env::var_os("IC_DB_M3_DIR").expect("run make m3-maintenance"),
    );
    let wasm = std::fs::read(directory.join("maintenance.wasm")).unwrap();
    let changed = std::fs::read(directory.join("maintenance-v2.wasm")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |operation: u8, limit: u8, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "maintenance_copy",
            ic_wire::encode(
                &(operation, limit, trap),
                Encoding::Frame {
                    method: 135,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let decode = |reply: Vec<u8>| {
        ic_wire::decode::<Result<(u64, bool), String>>(
            &reply,
            Encoding::Frame {
                method: 135,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let entries = async |target: bool| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "maintenance_copy_entries",
                ic_wire::encode(
                    &target,
                    Encoding::Frame {
                        method: 136,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();

        ic_wire::decode::<Entries>(
            &reply,
            Encoding::Frame {
                method: 136,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let upgrade = async |code: &Vec<u8>| {
        pic.advance_time(std::time::Duration::from_secs(600)).await;
        pic.tick().await;
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, code.clone(), vec![], None)
            .await
            .unwrap();
    };
    decode(call(0, 2, false).await.unwrap()).unwrap();
    let source = entries(false).await.1;
    let before = pic.get_stable_memory(canister).await;
    assert!(call(1, 2, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    decode(call(1, 2, false).await.unwrap()).unwrap();
    assert!(!entries(false).await.0 && !entries(true).await.0);
    let before = pic.get_stable_memory(canister).await;
    assert!(call(2, 2, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        decode(call(2, 2, false).await.unwrap()).unwrap(),
        (2, false)
    );
    let before = pic.get_stable_memory(canister).await;
    upgrade(&changed).await;
    assert_eq!(
        decode(call(2, 2, false).await.unwrap()),
        Err("InvalidTransition".into())
    );
    assert_eq!(
        decode(call(3, 2, false).await.unwrap()),
        Err("InvalidTransition".into())
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    upgrade(&wasm).await;
    assert_eq!(
        decode(call(2, 3, false).await.unwrap()).unwrap(),
        (3, false)
    );
    assert_eq!(decode(call(2, 3, false).await.unwrap()).unwrap(), (1, true));
    let (open, target) = entries(true).await;
    assert!(!open);
    let rows: Vec<_> = target.iter().filter(|(key, _)| key[0] == 2).collect();
    assert_eq!(rows.len(), 6);
    for (position, (key, value)) in rows.iter().enumerate() {
        let id = position as u64 + 1;
        assert_eq!(&key[5..], &id.to_be_bytes());
        let mut expected = (u128::from(id) * 10 + 100).to_le_bytes().to_vec();
        expected.extend((id % 2).to_le_bytes());
        expected.extend(id.to_le_bytes());
        assert_eq!(*value, expected);
    }
    assert_eq!(target.iter().filter(|(key, _)| key[0] == 3).count(), 6);
    if publish {
        let before = pic.get_stable_memory(canister).await;
        assert_eq!(
            decode(call(5, 2, false).await.unwrap()),
            Err("InvalidTransition".into())
        );
        assert!(call(4, 2, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert_eq!(
            decode(call(4, 2, false).await.unwrap()).unwrap(),
            (0, false)
        );
        let before = pic.get_stable_memory(canister).await;
        upgrade(&changed).await;
        assert_eq!(
            decode(call(4, 2, false).await.unwrap()),
            Err("InvalidTransition".into())
        );
        assert_eq!(
            decode(call(5, 2, false).await.unwrap()),
            Err("InvalidTransition".into())
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
        upgrade(&wasm).await;
        for iteration in 0..32 {
            if decode(call(4, 2, false).await.unwrap()).unwrap().1 {
                break;
            }
            assert!(iteration < 31, "validation must progress");
        }
        assert!(!entries(false).await.0 && !entries(true).await.0);
        let before = pic.get_stable_memory(canister).await;
        assert!(call(5, 2, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        upgrade(&wasm).await;
        assert_eq!(decode(call(5, 2, false).await.unwrap()).unwrap(), (0, true));
        assert!(!entries(false).await.0 && entries(true).await.0);
        decode(call(6, 2, false).await.unwrap()).unwrap();
        upgrade(&wasm).await;
        let (open, updated) = entries(true).await;
        assert!(open);
        let row = updated.iter().find(|(key, _)| key[0] == 2).unwrap();
        assert_eq!(&row.1[..16], &999_u128.to_le_bytes());
        let (open, retired) = entries(false).await;
        assert!(!open);
        assert_eq!(&retired[1..], &source[1..]);
        let before = pic.get_stable_memory(canister).await;
        assert_eq!(
            decode(call(3, 2, false).await.unwrap()),
            Err("InvalidTransition".into())
        );
        assert_eq!(
            decode(call(2, 2, false).await.unwrap()),
            Err("InvalidTransition".into())
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let target_before = entries(true).await.1;
        assert!(call(7, 2, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        upgrade(&changed).await;
        assert_eq!(
            decode(call(7, 2, false).await.unwrap()),
            Err("InvalidTransition".into())
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
        upgrade(&wasm).await;
        assert_eq!(
            decode(call(7, 2, false).await.unwrap()).unwrap(),
            (0, false)
        );
        assert_eq!(entries(false).await.1.len(), retired.len() - 2);
        // A recorded 0.16 executable proves that partially cleaned source remains
        // closed while its published target still serves ordinary writes.
        if let Some(path) = std::env::var_os("IC_DB_M3_RETIRED_PREDECESSOR_WASM") {
            upgrade(&std::fs::read(path).unwrap()).await;
            assert!(!entries(false).await.0 && entries(true).await.0);
            decode(call(6, 2, false).await.unwrap()).unwrap();
        }
        upgrade(&wasm).await;
        for iteration in 0..16 {
            if decode(call(7, 2, false).await.unwrap()).unwrap().1 {
                break;
            }
            assert!(iteration < 15, "retired cleanup must progress");
        }
        let (open, cleaned) = entries(false).await;
        assert!(!open);
        assert_eq!(cleaned, vec![retired[0].clone()]);
        assert_eq!(entries(true).await.1, target_before);
        let before = pic.get_stable_memory(canister).await;
        assert_eq!(decode(call(7, 2, false).await.unwrap()).unwrap(), (0, true));
        assert_eq!(pic.get_stable_memory(canister).await, before);
        owned.drop().await;

        return;
    }
    let before = pic.get_stable_memory(canister).await;
    assert!(call(3, 2, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        decode(call(3, 2, false).await.unwrap()).unwrap(),
        (0, false)
    );
    assert_eq!(
        decode(call(2, 2, false).await.unwrap()),
        Err("InvalidTransition".into())
    );
    upgrade(&wasm).await;
    for iteration in 0..8 {
        if decode(call(3, 2, false).await.unwrap()).unwrap().1 {
            break;
        }
        assert!(iteration < 7, "cleanup must progress");
    }
    let (open, restored) = entries(false).await;
    assert!(open);
    assert_eq!(&restored[1..], &source[1..]);
    let (open, target) = entries(true).await;
    assert!(!open);
    assert_eq!(target.len(), 1);
    owned.drop().await;
}

#[tokio::test]
async fn managed_times_survive_copy_rollback_resume_and_cutover() {
    let directory = std::path::PathBuf::from(std::env::var_os("IC_DB_M3_DIR").unwrap());
    let wasm = std::fs::read(directory.join("maintenance.wasm")).unwrap();
    let changed = std::fs::read(directory.join("maintenance-v2.wasm")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |operation: u8, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "maintenance_managed_copy",
            ic_wire::encode(
                &(operation, trap),
                Encoding::Frame {
                    method: 137,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    type Reply = (bool, Vec<(u64, String, i64, i64)>);
    let decode = |bytes: Vec<u8>| {
        ic_wire::decode::<Result<Reply, String>>(
            &bytes,
            Encoding::Frame {
                method: 137,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let upgrade = async |code: &Vec<u8>| {
        pic.advance_time(std::time::Duration::from_secs(600)).await;
        pic.tick().await;
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, code.clone(), vec![], None)
            .await
            .unwrap();
    };
    decode(call(0, false).await.unwrap()).unwrap();
    let before = pic.get_stable_memory(canister).await;
    assert!(decode(call(2, false).await.unwrap()).is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(call(1, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(!decode(call(1, false).await.unwrap()).unwrap().0);
    upgrade(&changed).await;
    let before = pic.get_stable_memory(canister).await;
    assert!(decode(call(1, false).await.unwrap()).is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    upgrade(&wasm).await;
    assert!(!decode(call(1, false).await.unwrap()).unwrap().0);
    assert!(decode(call(1, false).await.unwrap()).unwrap().0);
    let mut complete = false;
    for _ in 0..16 {
        if decode(call(3, false).await.unwrap()).unwrap().0 {
            complete = true;
            break;
        }
    }
    assert!(complete);
    let before = pic.get_stable_memory(canister).await;
    assert!(call(4, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    decode(call(4, false).await.unwrap()).unwrap();
    upgrade(&wasm).await;
    assert_eq!(
        decode(call(5, false).await.unwrap()).unwrap().1,
        vec![(2, "updated".into(), -10, 20)]
    );
    assert_eq!(
        decode(call(6, false).await.unwrap()).unwrap().1,
        vec![(2, "after cutover".into(), -10, 30)]
    );
    owned.drop().await;
}

#[tokio::test]
async fn successive_whole_database_copies_preserve_intervening_writes_and_cleanup() {
    whole_database_copy(false, false).await;
}

#[tokio::test]
async fn whole_database_invalid_reference_cancels_to_preserved_source() {
    whole_database_copy(true, false).await;
}

#[tokio::test]
async fn second_migration_cancellation_restores_latest_source_not_original_database() {
    whole_database_copy(false, true).await;
}

async fn whole_database_copy(invalid_reference: bool, cancel_second: bool) {
    let directory = std::path::PathBuf::from(std::env::var_os("IC_DB_M3_DIR").unwrap());
    let wasm = std::fs::read(directory.join("maintenance.wasm")).unwrap();
    let changed = std::fs::read(directory.join("maintenance-v2.wasm")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |operation: u8, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "maintenance_whole_copy",
            ic_wire::encode(
                &(operation, trap),
                Encoding::Frame {
                    method: 138,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    type Reply = (bool, Option<String>, Vec<(u64, u64, u128)>);
    let decode = |bytes: Vec<u8>| {
        ic_wire::decode::<Result<Reply, String>>(
            &bytes,
            Encoding::Frame {
                method: 138,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let upgrade = async |code: &Vec<u8>| {
        pic.advance_time(std::time::Duration::from_secs(600)).await;
        pic.tick().await;
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, code.clone(), vec![], None)
            .await
            .unwrap();
    };
    assert_eq!(
        decode(call(0, false).await.unwrap()).unwrap().1.as_deref(),
        Some("A_Mapping")
    );
    let before = pic.get_stable_memory(canister).await;
    assert!(decode(call(2, false).await.unwrap()).is_err()); // wrong entity order
    assert!(decode(call(4, false).await.unwrap()).is_err()); // no early validation
    assert!(call(1, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let mapping_op = if invalid_reference { 8 } else { 1 };
    assert_eq!(
        decode(call(mapping_op, false).await.unwrap())
            .unwrap()
            .1
            .as_deref(),
        Some("A_Mapping")
    );
    assert_eq!(
        decode(call(mapping_op, false).await.unwrap())
            .unwrap()
            .1
            .as_deref(),
        Some("Entry")
    );
    upgrade(&changed).await;
    let before = pic.get_stable_memory(canister).await;
    assert!(decode(call(2, false).await.unwrap()).is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    // Older code must not reopen or modify an intermediate multi-entity target.
    if let Some(path) = std::env::var_os("IC_DB_M3_RETIRED_PREDECESSOR_WASM") {
        upgrade(&std::fs::read(path).unwrap()).await;
        let before = pic.get_stable_memory(canister).await;
        let reply = pic
            .update_call(
                canister,
                canister,
                "maintenance_copy",
                ic_wire::encode(
                    &(2_u8, 2_u8, false),
                    Encoding::Frame {
                        method: 135,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            ic_wire::decode::<Result<(u64, bool), String>>(
                &reply,
                Encoding::Frame {
                    method: 135,
                    direction: Direction::Response,
                },
                LIMITS
            )
            .unwrap()
            .is_err()
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    upgrade(&wasm).await;
    assert_eq!(
        decode(call(2, false).await.unwrap()).unwrap().1.as_deref(),
        Some("Entry")
    );
    assert_eq!(
        decode(call(2, false).await.unwrap()).unwrap().1.as_deref(),
        Some("Z_Empty")
    );
    assert!(decode(call(3, false).await.unwrap()).unwrap().0);
    let before = pic.get_stable_memory(canister).await;
    assert!(decode(call(5, false).await.unwrap()).is_err());
    assert!(call(4, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let mut finished = false;
    for _ in 0..40 {
        let before = pic.get_stable_memory(canister).await;

        match decode(call(4, false).await.unwrap()) {
            Ok((true, _, _)) => {
                assert!(!invalid_reference);
                finished = true;
                break;
            }
            Ok(_) => {}
            Err(_) => {
                assert!(invalid_reference);
                assert_eq!(pic.get_stable_memory(canister).await, before);
                assert!(decode(call(5, false).await.unwrap()).is_err());
                finished = true;
                break;
            }
        }
    }
    assert!(finished);
    if invalid_reference {
        let before = pic.get_stable_memory(canister).await;
        assert!(call(7, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert!(!decode(call(7, false).await.unwrap()).unwrap().0);
        upgrade(&wasm).await;
        let mut complete = false;
        for _ in 0..32 {
            if decode(call(7, false).await.unwrap()).unwrap().0 {
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert_eq!(
            decode(call(9, false).await.unwrap()).unwrap().2,
            vec![(0, 1, 0), (1, 1, 10)]
        );
    } else {
        let before = pic.get_stable_memory(canister).await;
        assert!(call(5, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        decode(call(5, false).await.unwrap()).unwrap();
        upgrade(&wasm).await;
        let (restricted, _, rows) = decode(call(6, false).await.unwrap()).unwrap();
        assert!(restricted);
        assert_eq!(rows, vec![(0, 1, 100), (1, 1, 110)]);
        let first_published_bytes = pic.get_stable_memory(canister).await.len();
        let next = async |operation: u8, trap: bool| {
            pic.update_call(
                canister,
                canister,
                "maintenance_next_copy",
                ic_wire::encode(
                    &(operation, trap),
                    Encoding::Frame {
                        method: 139,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
        };
        let decode_next = |bytes: Vec<u8>| {
            ic_wire::decode::<Result<Reply, String>>(
                &bytes,
                Encoding::Frame {
                    method: 139,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap()
        };
        decode_next(next(0, false).await.unwrap()).unwrap(); // ordinary B writes
        assert_eq!(
            decode_next(next(13, false).await.unwrap()).unwrap().2,
            vec![(0, 1, 999), (1, 1, 110), (2, 1, 222)]
        );
        let before = pic.get_stable_memory(canister).await;
        assert!(next(1, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        decode_next(next(1, false).await.unwrap()).unwrap();
        let before = pic.get_stable_memory(canister).await;
        assert!(next(2, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        decode_next(next(2, false).await.unwrap()).unwrap();
        assert_eq!(
            decode_next(next(11, false).await.unwrap()),
            Err("Maintenance".into())
        );
        // Neither the first transition nor retired cleanup owns the new live source.
        let before = pic.get_stable_memory(canister).await;
        assert!(decode(call(2, false).await.unwrap()).is_err());
        assert!(decode(call(7, false).await.unwrap()).is_err());
        assert!(decode_next(next(8, false).await.unwrap()).is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert!(next(3, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        decode_next(next(3, false).await.unwrap()).unwrap();
        upgrade(&changed).await;
        let before = pic.get_stable_memory(canister).await;
        assert!(decode_next(next(3, false).await.unwrap()).is_err());
        assert!(decode_next(next(9, false).await.unwrap()).is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        upgrade(&wasm).await;
        // A may be cleaned while B is frozen as the source of a different transition.
        let before = pic.get_stable_memory(canister).await;
        assert!(next(7, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let mut cleaned_a = false;
        for _ in 0..32 {
            if decode_next(next(7, false).await.unwrap()).unwrap().0 {
                cleaned_a = true;
                break;
            }
        }
        assert!(cleaned_a);
        let after_a_cleanup_bytes = pic.get_stable_memory(canister).await.len();
        if cancel_second {
            let before = pic.get_stable_memory(canister).await;
            assert!(next(9, true).await.is_err());
            assert_eq!(pic.get_stable_memory(canister).await, before);
            assert!(!decode_next(next(9, false).await.unwrap()).unwrap().0);
            upgrade(&wasm).await;
            let mut cancelled = false;
            for _ in 0..32 {
                if decode_next(next(9, false).await.unwrap()).unwrap().0 {
                    cancelled = true;
                    break;
                }
            }
            assert!(cancelled);
            assert!(decode_next(next(11, false).await.unwrap()).is_err());
            assert_eq!(
                decode_next(next(13, false).await.unwrap()).unwrap().2,
                vec![(0, 1, 999), (1, 1, 110), (2, 1, 222)]
            );
            let before = pic.get_stable_memory(canister).await;
            assert!(decode_next(next(6, false).await.unwrap()).is_err());
            assert!(decode_next(next(8, false).await.unwrap()).is_err());
            assert!(decode(call(9, false).await.unwrap()).is_err());
            assert_eq!(pic.get_stable_memory(canister).await, before);
            owned.drop().await;

            return;
        }
        let mut copied = false;
        for _ in 0..16 {
            if decode_next(next(3, false).await.unwrap()).unwrap().0 {
                copied = true;
                break;
            }
        }
        assert!(copied);
        let mut validated = false;
        for _ in 0..48 {
            if decode_next(next(4, false).await.unwrap()).unwrap().0 {
                validated = true;
                break;
            }
        }
        assert!(validated);
        let before = pic.get_stable_memory(canister).await;
        assert!(next(5, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        upgrade(&wasm).await;
        assert_eq!(
            decode_next(next(13, false).await.unwrap()),
            Err("Maintenance".into())
        );
        decode_next(next(5, false).await.unwrap()).unwrap();
        upgrade(&wasm).await;
        let before = pic.get_stable_memory(canister).await;
        assert_eq!(
            decode_next(next(12, false).await.unwrap()),
            Err("ReferenceViolation".into())
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert!(decode_next(next(11, false).await.unwrap()).unwrap().0);
        let expected = vec![(0, 1, 1999), (1, 1, 1110), (2, 1, 1222)];
        assert_eq!(
            decode_next(next(13, false).await.unwrap()).unwrap().2,
            expected
        );
        let (restricted, _, rows) = decode_next(next(6, false).await.unwrap()).unwrap();
        assert!(restricted);
        assert_eq!(rows, expected); // returned through the newly built note index
        let second_published_bytes = pic.get_stable_memory(canister).await.len();
        let before = pic.get_stable_memory(canister).await;
        assert!(decode_next(next(9, false).await.unwrap()).is_err());
        assert!(decode_next(next(10, false).await.unwrap()).is_err());
        assert!(next(8, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert!(!decode_next(next(8, false).await.unwrap()).unwrap().0);
        upgrade(&wasm).await;
        let mut cleaned_b = false;
        for _ in 0..32 {
            if decode_next(next(8, false).await.unwrap()).unwrap().0 {
                cleaned_b = true;
                break;
            }
        }
        assert!(cleaned_b);
        assert_eq!(
            decode_next(next(13, false).await.unwrap()).unwrap().2,
            expected
        );
        let (restricted, _, rows) = decode_next(next(6, false).await.unwrap()).unwrap();
        assert!(restricted); // only the new Document still references the migrated Entry
        assert_eq!(rows, expected);
        let before = pic.get_stable_memory(canister).await;
        assert!(decode_next(next(7, false).await.unwrap()).unwrap().0);
        assert!(decode_next(next(8, false).await.unwrap()).unwrap().0);
        assert_eq!(pic.get_stable_memory(canister).await, before);
        println!(
            "successive copy backing bytes: first_published={first_published_bytes}, after_a_cleanup={after_a_cleanup_bytes}, second_published={second_published_bytes}, after_b_cleanup={}",
            before.len()
        );
    }
    owned.drop().await;
}
