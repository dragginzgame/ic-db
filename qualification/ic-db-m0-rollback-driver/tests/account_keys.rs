//! Canonical Account/Subaccount keys across cross-entity batches and IC messages.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_testkit::pocket_ic::ErrorCode;
use ic_wire::{Direction, Encoding};

type Identity = (Vec<u8>, Option<[u8; 32]>);
type AccountRow = (Identity, Option<[u8; 32]>, u64);
type SubaccountRow = ([u8; 32], Option<Identity>, u64);
type Page = (
    Vec<AccountRow>,
    Option<Identity>,
    Vec<SubaccountRow>,
    Option<[u8; 32]>,
    (u64, u64),
    Vec<Identity>,
);

#[tokio::test]
async fn account_aliases_and_mixed_references_survive_rollback_and_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records-wasm"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write =
        async |mode: u8, accounts: Vec<AccountRow>, subs: Vec<SubaccountRow>, trap: bool| {
            pic.update_call(
                canister,
                canister,
                "account_keys_write",
                ic_wire::encode(
                    &(mode, accounts, subs, trap),
                    Encoding::Frame {
                        method: 86,
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
                method: 86,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let page =
        async |after: Option<Identity>, sub_after: Option<[u8; 32]>, limit: u8, rank: u64| {
            let bytes = pic
                .query_call(
                    canister,
                    canister,
                    "account_keys_page",
                    ic_wire::encode(
                        &(after, sub_after, limit, rank),
                        Encoding::Frame {
                            method: 87,
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
                    method: 87,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap()
        };
    let ids = [
        (vec![], None),
        (vec![], Some([255; 32])),
        (vec![9], None),
        (vec![255; 29], Some([255; 32])),
    ];
    let mut small = [0; 32];
    small[31] = 1;
    let subs = [[0; 32], small, [1; 32], [255; 32]];
    let mut accounts: Vec<AccountRow> = ids
        .iter()
        .enumerate()
        .map(|(n, id)| (id.clone(), Some(subs[n]), n as u64))
        .collect();
    let sub_rows: Vec<SubaccountRow> = subs
        .iter()
        .enumerate()
        .map(|(n, id)| (*id, Some(ids[(n + 1) % 4].clone()), n as u64))
        .collect();
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        decode(write(0, accounts.clone(), vec![], false).await.unwrap()),
        Err(1)
    );
    assert!(pic.get_stable_memory(canister).await == before);
    decode(
        write(0, accounts.clone(), sub_rows.clone(), false)
            .await
            .unwrap(),
    )
    .unwrap();
    let first = page(None, None, 2, 0).await;
    assert_eq!(
        first,
        (
            accounts[..2].to_vec(),
            Some(ids[1].clone()),
            sub_rows[..2].to_vec(),
            Some(subs[1]),
            (1, 1),
            vec![ids[0].clone()]
        )
    );
    let rest = page(first.1, first.3, 32, 3).await;
    assert_eq!(rest.0, accounts[2..]);
    assert_eq!(rest.2, sub_rows[2..]);
    let empty = page(Some(ids[3].clone()), Some(subs[3]), 2, 0).await;
    assert!(empty.0.is_empty() && empty.2.is_empty());
    let before = pic.get_stable_memory(canister).await;
    let mut alias = accounts[0].clone();
    alias.0.1 = Some([0; 32]);
    assert_eq!(
        decode(write(0, vec![alias.clone()], vec![], false).await.unwrap()),
        Err(3)
    );
    decode(write(1, vec![alias.clone()], vec![], false).await.unwrap()).unwrap();
    assert_eq!(
        decode(write(2, vec![alias.clone()], vec![], false).await.unwrap()),
        Err(1)
    );
    assert_eq!(
        decode(
            write(2, vec![alias, accounts[0].clone()], vec![], false)
                .await
                .unwrap()
        ),
        Err(4)
    );
    assert_eq!(
        decode(
            write(2, vec![], vec![sub_rows[0].clone()], false)
                .await
                .unwrap()
        ),
        Err(1)
    );
    let mut collision = accounts[0].clone();
    collision.2 = 1;
    assert_eq!(
        decode(write(1, vec![collision], vec![], false).await.unwrap()),
        Err(2)
    );
    assert!(pic.get_stable_memory(canister).await == before);
    // The first account has an explicit-zero alias in continuation too.
    assert_eq!(
        page(Some((vec![], Some([0; 32]))), None, 32, 0).await.0,
        accounts[1..]
    );
    accounts[0].2 = 3;
    accounts[3].2 = 0;
    let swap = vec![accounts[0].clone(), accounts[3].clone()];
    assert_eq!(
        write(1, swap.clone(), vec![], true)
            .await
            .unwrap_err()
            .error_code,
        ErrorCode::CanisterCalledTrap
    );
    assert!(pic.get_stable_memory(canister).await == before);
    decode(write(1, swap, vec![], false).await.unwrap()).unwrap();
    let expected = (
        accounts.clone(),
        None,
        sub_rows.clone(),
        None,
        (2, 1),
        vec![ids[3].clone()],
    );
    assert_eq!(page(None, None, 32, 0).await, expected);
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(page(None, None, 32, 0).await, expected);
    decode(write(2, accounts, sub_rows, false).await.unwrap()).unwrap();
    assert_eq!(
        page(None, None, 32, 0).await,
        (vec![], None, vec![], None, (3, 2), vec![])
    );
    owned.close().await;
}
