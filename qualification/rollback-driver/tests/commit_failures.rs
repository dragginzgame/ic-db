//! Failures inside the delivered commit path, using the real IC rollback boundary.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_testkit::pocket_ic::ErrorCode;
use ic_wire::native;

type Status = ([u8; 32], [u8; 16], u64, u64);

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
                "inspect",
                native::encode_request(1, &(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Status>(1, &reply, LIMITS).unwrap()
    };
    let category = async |category: u64| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "category",
                native::encode_request(5, &category, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Vec<(u64, u64)>>(5, &reply, LIMITS).unwrap()
    };
    let batch = async |first: u64, stop_after: u64, deny_growth: bool, exhaust: bool| {
        pic.update_call(
            canister,
            canister,
            "fault_batch",
            native::encode_request(6, &(first, stop_after, deny_growth, exhaust), LIMITS).unwrap(),
        )
        .await
    };

    let original = status().await;
    let reference = async || {
        let reply = pic
            .query_call(
                canister,
                canister,
                "document_reference",
                native::encode_request(9, &1_u64, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Option<u64>>(9, &reply, LIMITS).unwrap()
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
                let (writes, grows): (u64, u64) =
                    native::decode_response(6, &reply, LIMITS).unwrap();
                assert_eq!(writes + 1, stop_after);
                assert!(writes > 1);
                assert_eq!(status().await.2, original.2 + 1);
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
                let (_, grows): (u64, u64) = native::decode_response(6, &reply, LIMITS).unwrap();
                assert_eq!(grows, 0, "grow refusal must not be ignored");
                assert_eq!(status().await.2, prior.2 + 1);
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
                let (writes, grows): (u64, u64) =
                    native::decode_response(6, &reply, LIMITS).unwrap();
                assert!(
                    grows > 0,
                    "failed batch must really cross a growth boundary"
                );
                assert!(status().await.3 > prior.3);
                assert_eq!(status().await.2, prior.2 + 1);
                let reply = pic
                    .query_call(
                        canister,
                        canister,
                        "fluent_query",
                        native::encode_request(
                            95,
                            &(
                                None::<Vec<u8>>,
                                true,
                                Some(9_u64),
                                u64::MAX,
                                3_u32,
                                3_u32,
                                4096_u32,
                            ),
                            LIMITS,
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap();
                type Page = (Vec<(u64, u64)>, Option<Vec<u8>>, u32, u32, u32, u64);
                let page = native::decode_response::<Result<Page, u8>>(95, &reply, LIMITS)
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
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert!(pic.get_stable_memory(canister).await == before);
    assert_eq!(status().await, prior);
    assert_eq!(reference().await, Some(2));
    assert_eq!(&category(9).await[..2], &[(1, 901), (2, 902)]);
    owned.close().await;
}
