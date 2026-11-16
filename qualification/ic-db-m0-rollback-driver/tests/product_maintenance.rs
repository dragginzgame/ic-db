//! Real product manifest gate, exact executable identity and IC rollback boundary.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

type Status = ([u8; 32], [u8; 16], u64, u64);
type SourcePage = (Vec<(u64, u64, u64)>, Option<Vec<u8>>);

#[tokio::test]
async fn freeze_resume_refusal_and_cancel_survive_traps_and_executable_changes() {
    let directory = std::path::PathBuf::from(
        std::env::var_os("IC_DB_M3_DIR").expect("run make m3-maintenance"),
    );
    let wasm = std::fs::read(directory.join("maintenance.wasm")).unwrap();
    let changed = std::fs::read(directory.join("maintenance-v2.wasm")).unwrap();
    // A recorded 0.15 artifact additionally proves that the old manifest reader
    // cannot reopen frozen data. Without it, only current ordinary admission is tested.
    let predecessor =
        std::env::var_os("IC_DB_M3_PREDECESSOR_WASM").map(|path| std::fs::read(path).unwrap());
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(
        canister,
        predecessor.as_ref().unwrap_or(&wasm).clone(),
        vec![],
        None,
    )
    .await;
    let upgrade = async |bytes: &Vec<u8>| {
        pic.advance_time(std::time::Duration::from_secs(600)).await;
        pic.tick().await;
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, bytes.clone(), vec![], None)
            .await
            .unwrap();
    };
    let inspect = async || {
        pic.query_call(
            canister,
            canister,
            "inspect",
            ic_wire::encode(
                &(),
                Encoding::Frame {
                    method: 1,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let decode_status = |bytes: Vec<u8>| {
        ic_wire::decode::<Status>(
            &bytes,
            Encoding::Frame {
                method: 1,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let original = decode_status(inspect().await.unwrap());
    upgrade(&wasm).await;
    let call = async |operation: u8, inputs: Vec<u8>, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "maintenance_control",
            ic_wire::encode(
                &(operation, inputs, trap),
                Encoding::Frame {
                    method: 133,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let decode = |bytes: Vec<u8>| {
        ic_wire::decode::<Result<(), String>>(
            &bytes,
            Encoding::Frame {
                method: 133,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let page = async |after: Option<Vec<u8>>, rows: u8, bytes: u32| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "maintenance_source",
                ic_wire::encode(
                    &(after, rows, bytes),
                    Encoding::Frame {
                        method: 134,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<SourcePage, String>>(
            &reply,
            Encoding::Frame {
                method: 134,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let before = pic.get_stable_memory(canister).await;
    assert!(call(0, vec![9], true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(decode_status(inspect().await.unwrap()), original);
    decode(call(0, vec![9], false).await.unwrap()).unwrap();
    let frozen = pic.get_stable_memory(canister).await;
    assert!(inspect().await.is_err());
    assert_eq!(page(None, 2, 23).await, Err("RowTooLarge".into()));
    let (first, cursor) = page(None, 4, 48).await.unwrap();
    assert_eq!(first, [(1, 1, 11), (2, 0, 20)]);
    let cursor = cursor.unwrap();
    assert_eq!(
        decode(call(1, vec![8], false).await.unwrap()),
        Err("InvalidTransition".into())
    );
    assert_eq!(
        decode(call(2, vec![8], false).await.unwrap()),
        Err("InvalidTransition".into())
    );
    assert_eq!(pic.get_stable_memory(canister).await, frozen);
    // Same source/target schemas, but this executable only supports version two.
    upgrade(&changed).await;
    assert_eq!(
        page(Some(cursor.clone()), 2, 48).await,
        Err("InvalidTransition".into())
    );
    assert_eq!(
        decode(call(1, vec![9], false).await.unwrap()),
        Err("InvalidTransition".into())
    );
    assert_eq!(
        decode(call(2, vec![9], false).await.unwrap()),
        Err("InvalidTransition".into())
    );
    assert!(inspect().await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, frozen);
    if let Some(old) = &predecessor {
        upgrade(old).await;
        assert!(inspect().await.is_err());
        assert!(
            pic.update_call(
                canister,
                canister,
                "write",
                ic_wire::encode(
                    &(1_u64, 1_u64, 99_u64, 0_u8),
                    Encoding::Frame {
                        method: 4,
                        direction: Direction::Request
                    },
                    LIMITS
                )
                .unwrap()
            )
            .await
            .is_err()
        );
        assert_eq!(pic.get_stable_memory(canister).await, frozen);
    }
    upgrade(&wasm).await;
    decode(call(1, vec![9], false).await.unwrap()).unwrap();
    let (middle, next) = page(Some(cursor.clone()), 2, 48).await.unwrap();
    assert_eq!(middle, [(3, 1, 31), (4, 0, 40)]);
    let (last, next) = page(next, 2, 48).await.unwrap();
    assert_eq!(last, [(5, 1, 51), (6, 0, 60)]);
    assert_eq!(page(next, 2, 48).await.unwrap(), (vec![], None));
    assert!(call(2, vec![9], true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, frozen);
    decode(call(2, vec![9], false).await.unwrap()).unwrap();
    let restored = decode_status(inspect().await.unwrap());
    assert_eq!(
        (restored.0, restored.1, restored.2),
        (original.0, original.1, original.2)
    );
    if let Some(old) = &predecessor {
        upgrade(old).await;
    }
    let reopened = decode_status(inspect().await.unwrap());
    assert_eq!(
        (reopened.0, reopened.1, reopened.2),
        (original.0, original.1, original.2)
    );
    let reply = pic
        .update_call(
            canister,
            canister,
            "write",
            ic_wire::encode(
                &(1_u64, 1_u64, 99_u64, 0_u8),
                Encoding::Frame {
                    method: 4,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        ic_wire::decode::<Result<(), u8>>(
            &reply,
            Encoding::Frame {
                method: 4,
                direction: Direction::Response
            },
            LIMITS
        )
        .unwrap(),
        Ok(())
    );
    upgrade(&wasm).await;
    decode(call(0, vec![9], false).await.unwrap()).unwrap();
    let frozen = pic.get_stable_memory(canister).await;
    assert_eq!(page(Some(cursor), 2, 48).await, Err("InvalidCursor".into()));
    assert_eq!(pic.get_stable_memory(canister).await, frozen);
    decode(call(2, vec![9], false).await.unwrap()).unwrap();
    owned.drop().await;
}
