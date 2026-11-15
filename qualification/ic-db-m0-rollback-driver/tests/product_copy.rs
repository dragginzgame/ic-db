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
    let pic = owned.instance();
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
        pic.upgrade_canister(canister, code.clone(), vec![], None)
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
}
