//! Failures inside the delivered commit path, using the real IC rollback boundary.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_testkit::pocket_ic::ErrorCode;
use ic_wire::{Direction, Encoding};

type Status = ([u8; 32], [u8; 16], u64, u64, u64, u64);

#[tokio::test]
async fn backing_writes_and_growth_failures_roll_back() {
    let wasm = std::fs::read(
        std::env::var_os("IC_DB_M1_FAULT_WASM").expect("run make m1-commit-failures"),
    )
    .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let status = async || {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fault_status",
                ic_wire::encode(
                    &(),
                    Encoding::Frame {
                        method: 240,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Status>(
            &reply,
            Encoding::Frame {
                method: 240,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let category = async |category: u64| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "category",
                ic_wire::encode(
                    &category,
                    Encoding::Frame {
                        method: 5,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Vec<(u64, u64)>>(
            &reply,
            Encoding::Frame {
                method: 5,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let batch = async |first: u64, stop_after: u64, deny_growth: bool, exhaust: bool| {
        pic.update_call(
            canister,
            canister,
            "fault_batch",
            ic_wire::encode(
                &(first, stop_after, deny_growth, exhaust),
                Encoding::Frame {
                    method: 6,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };

    let original = status().await;
    assert_eq!(original.4, 6);
    let reference = async || {
        let reply = pic
            .query_call(
                canister,
                canister,
                "document_reference",
                ic_wire::encode(
                    &1_u64,
                    Encoding::Frame {
                        method: 9,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Option<u64>>(
            &reply,
            Encoding::Frame {
                method: 9,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let before = pic.get_stable_memory(canister).await;
    // Exercise the real IC meter, not a synthetic counter or an explicit trap.
    // The host burns instructions only after commit's first backing write.
    let error = batch(0, 1, false, true).await.unwrap_err();
    assert_eq!(
        error.error_code,
        ErrorCode::CanisterInstructionLimitExceeded,
        "{error:?}"
    );
    assert!(pic.get_stable_memory(canister).await == before);
    assert_eq!(status().await, original);
    assert_eq!(reference().await, Some(1));
    assert_eq!(category(1).await, vec![(1, 11), (3, 31), (5, 51)]);
    assert_eq!(category(0).await, vec![(2, 20), (4, 40), (6, 60)]);
    assert!(category(9).await.is_empty());
    eprintln!(
        "instruction exhaustion after a backing write: IC limit rejection and complete rollback"
    );

    let mut completed = false;
    // Discover the actual number of backing writes; never bind a test to the
    // supplier's current node layout or prescribe an optimized allocation shape.
    for stop_after in 1..=1024 {
        match batch(0, stop_after, false, false).await {
            Err(error) => {
                assert!(
                    error
                        .reject_message
                        .contains("M1 injected trap after backing write"),
                    "{error:?}"
                );
                assert!(
                    pic.get_stable_memory(canister).await == before,
                    "backing, extent or allocator metadata changed after write {stop_after}"
                );
                assert_eq!(status().await, original);
                assert_eq!(reference().await, Some(1));
                assert_eq!(category(1).await, vec![(1, 11), (3, 31), (5, 51)]);
                assert_eq!(category(0).await, vec![(2, 20), (4, 40), (6, 60)]);
                assert!(category(9).await.is_empty());
            }
            Ok(reply) => {
                let (writes, grows): (u64, u64) = ic_wire::decode(
                    &reply,
                    Encoding::Frame {
                        method: 6,
                        direction: Direction::Response,
                    },
                    LIMITS,
                )
                .unwrap();
                assert_eq!(writes + 1, stop_after);
                assert!(writes > 1);
                assert_eq!(status().await.2, original.2 + 1);
                assert_eq!(
                    (status().await.4, status().await.5),
                    (original.4, original.5)
                );
                assert_eq!(reference().await, Some(2));
                assert_eq!(category(9).await, vec![(1, 901), (2, 902)]);
                assert_eq!(category(1).await, vec![(3, 31), (5, 51)]);
                assert_eq!(category(0).await, vec![(4, 40), (6, 60)]);
                eprintln!(
                    "replacement: rollback after every one of {writes} backing writes; successful retry grows={grows}"
                );
                completed = true;
                break;
            }
        }
    }
    assert!(
        completed,
        "replacement did not complete within probe's write-count bound"
    );

    // Fill through ordinary bounded product batches until the supplier requests
    // physical growth. Reject grow() with -1, then retry the identical batch with
    // growth enabled. This distinguishes an actual growth boundary from a trap
    // injected by the host after a successful database function return.
    let mut growth_checked = false;
    for first in (100..100 + 256 * 32).step_by(32) {
        let before = pic.get_stable_memory(canister).await;
        let prior = status().await;
        match batch(first, 0, true, false).await {
            Ok(reply) => {
                let (_, grows): (u64, u64) = ic_wire::decode(
                    &reply,
                    Encoding::Frame {
                        method: 6,
                        direction: Direction::Response,
                    },
                    LIMITS,
                )
                .unwrap();
                assert_eq!(grows, 0, "grow refusal must not be ignored");
                assert_eq!(status().await.2, prior.2 + 1);
                assert_eq!(
                    (status().await.4, status().await.5),
                    (prior.4 + 32, prior.5)
                );
            }
            Err(error) => {
                assert!(
                    !error.reject_message.contains("M1 injected trap"),
                    "{error:?}"
                );
                assert!(
                    pic.get_stable_memory(canister).await == before,
                    "growth refusal changed rows, indexes, revisions or allocator state"
                );
                assert_eq!(status().await, prior);
                let reply = batch(first, 0, false, false)
                    .await
                    .expect("same batch must complete when growth is allowed");
                let (writes, grows): (u64, u64) = ic_wire::decode(
                    &reply,
                    Encoding::Frame {
                        method: 6,
                        direction: Direction::Response,
                    },
                    LIMITS,
                )
                .unwrap();
                assert!(
                    grows > 0,
                    "failed batch must really cross a growth boundary"
                );
                assert!(status().await.3 > prior.3);
                assert_eq!(status().await.2, prior.2 + 1);
                assert_eq!(
                    (status().await.4, status().await.5),
                    (prior.4 + 32, prior.5)
                );
                let reply = pic
                    .query_call(
                        canister,
                        canister,
                        "fluent_query",
                        ic_wire::encode(
                            &(
                                None::<Vec<u8>>,
                                true,
                                Some(9_u64),
                                u64::MAX,
                                3_u32,
                                3_u32,
                                4096_u32,
                            ),
                            Encoding::Frame {
                                method: 95,
                                direction: Direction::Request,
                            },
                            LIMITS,
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap();
                type Page = (Vec<(u64, u64)>, Option<Vec<u8>>, u32, u32, u32, u64);
                let page = ic_wire::decode::<Result<Page, u8>>(
                    &reply,
                    Encoding::Frame {
                        method: 95,
                        direction: Direction::Response,
                    },
                    LIMITS,
                )
                .unwrap()
                .unwrap();
                assert_eq!(
                    page.0,
                    vec![
                        (first + 31, u64::MAX),
                        (first + 30, u64::MAX),
                        (first + 29, u64::MAX)
                    ]
                );
                eprintln!(
                    "growth refusal: batch starting {first} rolled back; successful retry writes={writes} grows={grows}; pages {} -> {}",
                    prior.3,
                    status().await.3
                );
                growth_checked = true;
                break;
            }
        }
    }
    assert!(
        growth_checked,
        "probe never reached a physical growth boundary"
    );
    let before = pic.get_stable_memory(canister).await;
    let prior = status().await;
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert!(pic.get_stable_memory(canister).await == before);
    assert_eq!(status().await, prior);
    assert_eq!(reference().await, Some(2));
    assert_eq!(&category(9).await[..2], &[(1, 901), (2, 902)]);
    owned.close().await;
}
