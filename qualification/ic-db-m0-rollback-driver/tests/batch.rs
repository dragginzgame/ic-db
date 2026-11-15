//! Final-state batch constraints checked against independent complete row/index expectations.

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

type Command = (u8, u64, u64, u64);
type Entries = Vec<(u64, u64)>;
type Snapshot = (Entries, Entries, Entries, [u64; 4]);

#[tokio::test]
async fn final_state_batches() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M0_WASM").expect("run make m0-batch")).unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |commands: Vec<Command>| -> Result<u64, u8> {
        let reply = pic
            .update_call(
                canister,
                canister,
                "batch",
                ic_wire::encode(
                    &(commands, 0_u8),
                    Encoding::Frame {
                        method: 3,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode(
            &reply,
            Encoding::Frame {
                method: 3,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let refuse = async |commands: Vec<Command>, expected: u8| {
        let before = pic.get_stable_memory(canister).await;
        assert_eq!(call(commands).await, Err(expected));
        assert!(
            before == pic.get_stable_memory(canister).await,
            "refusals must precede all mutation"
        );
    };
    let check = async |users: &[u64], mappings: &[(u64, u64, u64)]| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "inspect",
                ic_wire::encode(
                    &(),
                    Encoding::Frame {
                        method: 2,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let snapshot: Snapshot = ic_wire::decode(
            &reply,
            Encoding::Frame {
                method: 2,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap();
        // Full expected contents, not lookups through the fixture's admission algorithm.
        let mut rows: Vec<_> = users.iter().map(|id| (*id, 0)).collect();
        rows.extend(
            mappings
                .iter()
                .map(|&(id, user, principal)| (100 + id, user * 1000 + principal)),
        );
        let mut unique: Vec<_> = mappings
            .iter()
            .filter(|(_, _, principal)| *principal != 0)
            .map(|&(id, _, principal)| (principal, id))
            .collect();
        let mut reverse: Vec<_> = mappings
            .iter()
            .map(|&(id, user, principal)| (user * 100 + id, principal))
            .collect();
        rows.sort();
        unique.sort();
        reverse.sort();
        assert_eq!(snapshot.0, rows);
        assert_eq!(snapshot.1, unique);
        assert_eq!(snapshot.2, reverse);
    };

    refuse(vec![], 1).await;
    refuse(vec![(0, 1, 0, 0); 9], 1).await;
    refuse(vec![(0, 1, 0, 0), (1, 1, 0, 0)], 2).await;
    refuse(vec![(2, 1, 1, 11)], 5).await;
    // Mapping before user in request order is valid when the final state is valid.
    assert_eq!(
        call(vec![
            (2, 1, 1, 11),
            (0, 1, 0, 0),
            (0, 2, 0, 0),
            (2, 2, 2, 22)
        ])
        .await,
        Ok(0)
    );
    check(&[1, 2], &[(1, 1, 11), (2, 2, 22)]).await;
    refuse(vec![(2, 3, 1, 33), (2, 4, 2, 33)], 4).await;
    refuse(vec![(2, 3, 1, 11)], 4).await;
    refuse(vec![(2, 1, 1, 33), (2, 1, 1, 44)], 2).await;
    refuse(vec![(1, 1, 0, 0)], 5).await;
    refuse(vec![(1, 1, 0, 0), (2, 3, 1, 33)], 5).await;

    let swap = vec![(2, 1, 1, 22), (2, 2, 2, 11)];
    for boundary in 1..=5_u8 {
        let before = pic.get_stable_memory(canister).await;
        let rejection = pic
            .update_call(
                canister,
                canister,
                "batch",
                ic_wire::encode(
                    &(swap.clone(), boundary),
                    Encoding::Frame {
                        method: 3,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap_err();
        assert!(
            rejection
                .reject_message
                .contains("M0 injected batch failure")
        );
        assert!(before == pic.get_stable_memory(canister).await);
        check(&[1, 2], &[(1, 1, 11), (2, 2, 22)]).await;
    }
    assert_eq!(call(swap).await, Ok(0));
    check(&[1, 2], &[(1, 1, 22), (2, 2, 11)]).await;
    // Release/acquire a principal, then repoint the surviving reference while deleting its user.
    assert_eq!(call(vec![(2, 3, 2, 22), (3, 1, 0, 0)]).await, Ok(0));
    check(&[1, 2], &[(2, 2, 11), (3, 2, 22)]).await;
    assert_eq!(
        call(vec![(1, 2, 0, 0), (2, 2, 1, 11), (3, 3, 0, 0)]).await,
        Ok(2)
    );
    check(&[1], &[(2, 1, 11)]).await;
    assert_eq!(call(vec![(1, 1, 0, 0), (3, 2, 0, 0)]).await, Ok(1));
    check(&[], &[]).await;

    // Explicit present-only uniqueness: absence is not a shared unique value,
    // and does not exempt the mapping from its separate reference constraint.
    refuse(vec![(2, 1, 1, 0)], 5).await;
    call(vec![(0, 1, 0, 0), (2, 1, 1, 0), (2, 2, 1, 0)])
        .await
        .unwrap();
    check(&[1], &[(1, 1, 0), (2, 1, 0)]).await;
    refuse(vec![(2, 1, 1, 101)], 1).await;
    refuse(vec![(2, 1, 1, 33), (2, 2, 1, 33)], 4).await;
    call(vec![(2, 1, 1, 33)]).await.unwrap();
    check(&[1], &[(1, 1, 33), (2, 1, 0)]).await;
    refuse(vec![(2, 2, 1, 33)], 4).await;
    let transfer = vec![(2, 1, 1, 0), (2, 2, 1, 33)];
    for boundary in 1..=5_u8 {
        let before = pic.get_stable_memory(canister).await;
        assert!(
            pic.update_call(
                canister,
                canister,
                "batch",
                ic_wire::encode(
                    &(transfer.clone(), boundary),
                    Encoding::Frame {
                        method: 3,
                        direction: Direction::Request
                    },
                    LIMITS
                )
                .unwrap(),
            )
            .await
            .is_err()
        );
        assert!(before == pic.get_stable_memory(canister).await);
        check(&[1], &[(1, 1, 33), (2, 1, 0)]).await;
    }
    call(transfer).await.unwrap();
    check(&[1], &[(1, 1, 0), (2, 1, 33)]).await;
    call(vec![(2, 2, 1, 0)]).await.unwrap();
    check(&[1], &[(1, 1, 0), (2, 1, 0)]).await;
    refuse(vec![(1, 1, 0, 0)], 5).await;
    assert_eq!(
        call(vec![(1, 1, 0, 0), (3, 1, 0, 0), (3, 2, 0, 0)]).await,
        Ok(2)
    );
    check(&[], &[]).await;

    // Existence of a mapping's user does not imply a mandatory mapping for every user.
    assert_eq!(call(vec![(0, 1, 0, 0), (0, 2, 0, 0)]).await, Ok(0));
    let first: Vec<_> = (1..=8).map(|id| (2, id, 1, id)).collect();
    call(first).await.unwrap();
    call(vec![(2, 9, 1, 9)]).await.unwrap();
    let mut deletion = vec![(1, 1, 0, 0)];
    deletion.extend((1..=7).map(|id| (3, id, 0, 0)));
    refuse(deletion, 5).await; // An untouched reference beyond the changed prefix survives.
    // An unrelated user's deletion examines no reverse entries, despite nine mappings.
    assert_eq!(call(vec![(1, 2, 0, 0)]).await, Ok(0));
    call(vec![(2, 10, 1, 0)]).await.unwrap();
    let mut mappings: Vec<_> = (1..=9).map(|id| (id, 1, id)).collect();
    mappings.push((10, 1, 0));
    check(&[1], &mappings).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    check(&[1], &mappings).await;
    println!(
        "batch constraints: unique exchange/release, present-only optional uniqueness, final-state references, indexed restricted deletion, pre-write refusals and five commit trap boundaries passed"
    );
    owned.close().await;
}
