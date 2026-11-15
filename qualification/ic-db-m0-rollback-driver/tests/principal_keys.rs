//! Length-framed Principal identity through the real product and IC rollback.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_testkit::pocket_ic::ErrorCode;
use ic_wire::{Direction, Encoding};

type Row = (Vec<u8>, Option<Vec<u8>>, u64);
type Page = (Vec<Row>, Option<Vec<u8>>, u64, Vec<Vec<u8>>);

#[tokio::test]
async fn principal_identity_pages_references_and_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records-wasm"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |mode: u8, rows: Vec<Row>, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "principal_keys_write",
            ic_wire::encode(
                &(mode, rows, trap),
                Encoding::Frame {
                    method: 84,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let decode = |bytes: Vec<u8>| {
        ic_wire::decode::<Result<(), u8>>(
            &bytes,
            Encoding::Frame {
                method: 84,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let page = async |after: Option<Vec<u8>>, limit: u8, rank: u64| {
        let bytes = pic
            .query_call(
                canister,
                canister,
                "principal_keys_page",
                ic_wire::encode(
                    &(after, limit, rank),
                    Encoding::Frame {
                        method: 85,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Page>(
            &bytes,
            Encoding::Frame {
                method: 85,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let ids = [vec![], vec![9], vec![1, 0], vec![255; 29]];
    let mut rows: Vec<Row> = ids
        .iter()
        .enumerate()
        .map(|(n, id)| (id.clone(), Some(ids[(n + 1) % ids.len()].clone()), n as u64))
        .collect();
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        decode(write(0, vec![rows[0].clone()], false).await.unwrap()),
        Err(1)
    );
    assert!(pic.get_stable_memory(canister).await == before);
    decode(
        write(0, rows.iter().rev().cloned().collect(), false)
            .await
            .unwrap(),
    )
    .unwrap();
    let first = page(None, 2, 0).await;
    assert_eq!(
        first,
        (
            rows[..2].to_vec(),
            Some(ids[1].clone()),
            1,
            vec![ids[0].clone()]
        )
    );
    assert_eq!(page(first.1, 2, 2).await.0, rows[2..]);
    assert!(page(Some(ids[3].clone()), 2, 3).await.0.is_empty());
    let before = pic.get_stable_memory(canister).await;
    decode(write(1, rows.clone(), false).await.unwrap()).unwrap();
    assert!(pic.get_stable_memory(canister).await == before);
    assert_eq!(
        decode(write(2, vec![rows[0].clone()], false).await.unwrap()),
        Err(1)
    );
    assert_eq!(
        decode(write(0, vec![rows[0].clone()], false).await.unwrap()),
        Err(3)
    );
    let mut collision = rows[0].clone();
    collision.2 = 1;
    assert_eq!(
        decode(write(1, vec![collision], false).await.unwrap()),
        Err(2)
    );
    assert!(pic.get_stable_memory(canister).await == before);
    // Exchange unique ranks; the surviving cycle includes Some(empty Principal).
    rows[0].2 = 3;
    rows[3].2 = 0;
    let swap = vec![rows[0].clone(), rows[3].clone()];
    assert_eq!(
        write(1, swap.clone(), true).await.unwrap_err().error_code,
        ErrorCode::CanisterCalledTrap
    );
    assert!(pic.get_stable_memory(canister).await == before);
    decode(write(1, swap, false).await.unwrap()).unwrap();
    let expected = (rows.clone(), None, 2, vec![ids[3].clone()]);
    assert_eq!(page(None, 32, 0).await, expected);
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(page(None, 32, 0).await, expected);
    decode(write(2, rows, false).await.unwrap()).unwrap();
    assert_eq!(page(None, 32, 0).await, (vec![], None, 3, vec![]));
    owned.close().await;
}
