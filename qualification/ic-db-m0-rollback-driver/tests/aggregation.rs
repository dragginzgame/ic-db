//! Bounded aggregation with an independent full-input oracle and raw rollback checks.

use ic_db_m0_rollback_driver as support;
use ic_wire::{Direction, Encoding};

use support::LIMITS;

type Response = (u64, u64, u64, u64, u64, Vec<(u64, u64)>);

#[tokio::test]
async fn revision_aggregation() {
    let wasm = std::fs::read(
        std::env::var_os("IC_DB_M0_AGGREGATION_WASM").expect("run make m0-aggregation"),
    )
    .unwrap();
    let owned = support::instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 2_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;

    let call = async |input: (u8, u64, u64, u8)| -> Result<Response, u8> {
        let reply = pic
            .update_call(
                canister,
                canister,
                "command",
                ic_wire::encode(
                    &input,
                    Encoding::Frame {
                        method: 1,
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
                method: 1,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let inspect = async || -> Response {
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

        ic_wire::decode(
            &reply,
            Encoding::Frame {
                method: 2,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let finish = async |job: u64, mut sequence: u64| -> Response {
        for _ in 0..5 {
            let state = call((1, job, sequence, 0)).await.unwrap();
            if state.0 == 2 {
                return state;
            }
            assert_eq!(state.0, 1);
            assert!(state.5.is_empty(), "partial sums must never be published");
            assert_eq!(state.2, sequence + 1);
            sequence = state.2;
        }
        panic!("twelve-row input must finish within four three-row advances");
    };

    // Independent expected sums from the complete input, not the fixture's cursor path.
    let mut expected: Vec<_> = (0..3_u64)
        .map(|group| {
            (
                group,
                (1..=12_u64)
                    .filter(|id| id % 3 == group)
                    .map(|id| id * 10)
                    .sum::<u64>(),
            )
        })
        .filter(|(_, sum)| *sum > 150)
        .collect();
    expected.sort_by_key(|&(group, sum)| (std::cmp::Reverse(sum), group));
    assert_eq!(expected, vec![(0, 300), (2, 260), (1, 220)]);

    let start = call((0, 0, 0, 0)).await.unwrap();
    let job = start.1;
    assert_eq!(start.0, 1);
    assert_eq!(call((0, 0, 0, 0)).await, Err(5), "one retained job bound");
    assert_eq!(
        call((1, job, 1, 0)).await,
        Err(3),
        "future sequence refused"
    );
    let first = call((1, job, 0, 0)).await.unwrap();
    assert_eq!(first.2, 1);
    assert!(first.5.is_empty());
    let backing = pic.get_stable_memory(canister).await;
    assert_eq!(
        call((1, job, 0, 0)).await.unwrap(),
        first,
        "lost response replay"
    );
    assert!(backing == pic.get_stable_memory(canister).await);

    // No persistent changes survive any accumulator/progress boundary failure.
    for boundary in 1..=3_u8 {
        let request = ic_wire::encode(
            &(1_u8, job, 1_u64, boundary),
            Encoding::Frame {
                method: 1,
                direction: Direction::Request,
            },
            LIMITS,
        )
        .unwrap();
        let rejection = pic
            .update_call(canister, canister, "command", request)
            .await
            .unwrap_err();
        assert!(
            rejection
                .reject_message
                .contains("M0 injected aggregation failure")
        );
        assert!(backing == pic.get_stable_memory(canister).await);
    }
    // Refused writes and trapped row/revision writes must not invalidate a running job.
    assert_eq!(call((2, 13, 1, 0)).await, Err(1));
    assert_eq!(call((2, 1, 1001, 0)).await, Err(1));
    for boundary in 1..=2_u8 {
        let request = ic_wire::encode(
            &(2_u8, 1_u64, 99_u64, boundary),
            Encoding::Frame {
                method: 1,
                direction: Direction::Request,
            },
            LIMITS,
        )
        .unwrap();
        let rejection = pic
            .update_call(canister, canister, "command", request)
            .await
            .unwrap_err();
        assert!(
            rejection
                .reject_message
                .contains("M0 injected aggregation failure")
        );
        assert!(backing == pic.get_stable_memory(canister).await);
    }
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
        .await
        .unwrap();
    assert_eq!(inspect().await, first);
    let complete = finish(job, 1).await;
    assert_eq!(complete.2, 4);
    assert_eq!(complete.3, 0);
    assert_eq!(
        complete.4, 0,
        "intermediate writes do not advance source revision"
    );
    assert_eq!(complete.5, expected);

    // Restoration of the original value does not restore the old input revision.
    let next = call((0, 0, 0, 0)).await.unwrap();
    call((1, next.1, 0, 0)).await.unwrap();
    assert_eq!(call((2, 1, 99, 0)).await.unwrap().4, 1);
    assert_eq!(call((2, 1, 10, 0)).await.unwrap().4, 2);
    let invalid = call((1, next.1, 1, 0)).await.unwrap();
    assert_eq!(invalid.0, 3);
    assert!(invalid.5.is_empty());
    assert_eq!(call((1, next.1, 1, 0)).await.unwrap(), invalid);

    let restarted = call((0, 0, 0, 0)).await.unwrap();
    assert_eq!(
        call((1, next.1, 1, 0)).await,
        Err(2),
        "old job cannot advance replacement"
    );
    assert_eq!(finish(restarted.1, 0).await.5, expected);
    // A completed result remains tied to its captured revision after later writes.
    let changed = call((2, 1, 10, 0)).await.unwrap();
    assert_eq!(changed.5, expected);
    assert_ne!(changed.3, changed.4);

    // Deterministic contention schedule, not an asserted production write rate.
    let mut invalidations = 0;
    for value in 20..25 {
        let job = call((0, 0, 0, 0)).await.unwrap().1;
        call((1, job, 0, 0)).await.unwrap();
        call((2, 1, value, 0)).await.unwrap();
        let state = call((1, job, 1, 0)).await.unwrap();
        assert_eq!(state.0, 3);
        assert!(state.5.is_empty());
        invalidations += 1;
    }
    assert_eq!(invalidations, 5);
    call((2, 1, 10, 0)).await.unwrap();
    let quiet = call((0, 0, 0, 0)).await.unwrap();
    assert_eq!(finish(quiet.1, 0).await.5, expected);

    let cancelling = call((0, 0, 0, 0)).await.unwrap();
    call((1, cancelling.1, 0, 0)).await.unwrap();
    let cancelled = call((3, cancelling.1, 0, 0)).await.unwrap();
    assert_eq!(cancelled.0, 0);
    assert_eq!(cancelled.4, cancelling.4);
    assert!(cancelled.5.is_empty());
    assert_eq!(call((1, cancelling.1, 0, 0)).await, Err(4));

    // Deletion invalidates; sparse/empty inputs terminate without invented groups.
    let deleting = call((0, 0, 0, 0)).await.unwrap();
    call((4, 1, 0, 0)).await.unwrap();
    assert_eq!(call((1, deleting.1, 0, 0)).await.unwrap().0, 3);
    for id in 2..=12 {
        call((4, id, 0, 0)).await.unwrap();
    }
    let empty = call((0, 0, 0, 0)).await.unwrap();
    let result = finish(empty.1, 0).await;
    assert_eq!(result.2, 1);
    assert!(result.5.is_empty());
    call((2, 2, 200, 0)).await.unwrap();
    call((2, 11, 900, 0)).await.unwrap();
    let sparse = call((0, 0, 0, 0)).await.unwrap();
    let result = finish(sparse.1, 0).await;
    assert_eq!(result.2, 1);
    assert_eq!(result.5, vec![(2, 1100)]);
    println!(
        "exact sums={expected:?}; quiet completion=4 advances; contention=5 explicit invalidations/5 attempts; empty completion=1 advance"
    );
    owned.drop().await;
}
