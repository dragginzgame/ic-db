//! Fluent query pages remain bounded and resume across upgrades in real Wasm.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::native;
type Page = (Vec<(u64, u64)>, Option<Vec<u8>>, u32, u32, u32, u64);
type Request = (Option<Vec<u8>>, bool, Option<u64>, u64, u32, u32, u32);

#[tokio::test]
async fn sparse_fluent_pages_retry_and_reopen() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let query = async |request: Request| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_query",
                native::encode_request(95, &request, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Page, u8>>(95, &reply, LIMITS).unwrap()
    };
    let before = pic.get_stable_memory(canister).await;
    let ordered = async |request: Request| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_ordered",
                native::encode_request(98, &request, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Page, u8>>(98, &reply, LIMITS).unwrap()
    };
    let order_start = ordered((None, false, Some(1), 40, 2, 1, 4096))
        .await
        .unwrap();
    assert!(order_start.0.is_empty());
    assert_eq!(order_start.2, 1);
    assert!(order_start.1.is_some());
    let all = ordered((None, false, None, 0, 32, 128, 4096))
        .await
        .unwrap();
    assert_eq!(
        all.0,
        [(2, 20), (4, 40), (6, 60), (1, 11), (3, 31), (5, 51)]
    );
    assert_eq!(all.2, 6);
    let reverse = ordered((None, true, None, 0, 32, 128, 4096)).await.unwrap();
    assert_eq!(reverse.0, all.0.into_iter().rev().collect::<Vec<_>>());
    let project = async |request: Request| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_projection",
                native::encode_request(96, &request, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Page, u8>>(96, &reply, LIMITS).unwrap()
    };
    let projected = project((None, false, None, 0, 32, 128, 31)).await.unwrap();
    assert_eq!(projected.0, [(1, 11)]);
    assert_eq!((projected.2, projected.4), (2, 16));
    assert_eq!(
        query((projected.1.clone(), false, None, 0, 32, 128, 4096))
            .await
            .unwrap_err(),
        1
    );
    let first = query((None, false, Some(1), 40, 2, 1, 4096)).await.unwrap();
    assert!(first.0.is_empty());
    assert!(first.1.is_some());
    assert_eq!((first.2, first.3, first.4), (1, 1, 0));
    println!("fluent_sparse_first_instructions={}", first.5);
    assert_eq!(
        query((first.1.clone(), true, Some(1), 40, 2, 1, 4096))
            .await
            .unwrap_err(),
        1
    );
    assert_eq!(
        query((first.1.clone(), false, Some(1), 41, 2, 1, 4096))
            .await
            .unwrap_err(),
        1
    );
    // Only queries ran since install, so no update rounds drained the IC's
    // install-code rate limiter. Advance virtual time, not wall-clock time.
    pic.advance_time(std::time::Duration::from_secs(600)).await;
    pic.tick().await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    let after_upgrade = ordered((order_start.1.clone(), false, Some(1), 40, 2, 128, 4096))
        .await
        .unwrap();
    assert_eq!(after_upgrade.0, [(5, 51)]);
    assert_eq!(after_upgrade.2, 2); // only the two remaining category=1 entries
    assert_eq!(
        ordered((order_start.1.clone(), true, Some(1), 40, 2, 128, 4096))
            .await
            .unwrap_err(),
        1
    );
    assert_eq!(
        ordered((order_start.1, false, Some(0), 40, 2, 128, 4096))
            .await
            .unwrap_err(),
        1
    );
    let resumed = project((projected.1, false, None, 0, 32, 128, 4096))
        .await
        .unwrap();
    assert_eq!(resumed.0, [(2, 20), (3, 31), (4, 40), (5, 51), (6, 60)]);
    assert_eq!(resumed.4, 80);
    let second = query((first.1, false, Some(1), 40, 2, 1, 4096))
        .await
        .unwrap();
    assert!(second.0.is_empty());
    assert!(second.1.is_some());
    let third = query((second.1, false, Some(1), 40, 2, 1, 4096))
        .await
        .unwrap();
    assert_eq!(third.0, [(5, 51)]);
    assert_eq!(third.4, 24);
    assert!(
        query((third.1, false, Some(1), 40, 2, 1, 4096))
            .await
            .unwrap()
            .1
            .is_none()
    );
    let bytes = query((None, false, None, 0, 32, 128, 47)).await.unwrap();
    assert_eq!(bytes.0, [(1, 11)]);
    assert_eq!((bytes.2, bytes.3, bytes.4), (2, 2, 24));
    assert_eq!(
        query((bytes.1, false, None, 0, 32, 128, 4096))
            .await
            .unwrap()
            .0,
        [(2, 20), (3, 31), (4, 40), (5, 51), (6, 60)]
    );
    assert_eq!(
        query((None, false, None, 0, 32, 128, 23))
            .await
            .unwrap_err(),
        2
    );
    let descending = query((None, true, Some(1), 0, 32, 128, 4096))
        .await
        .unwrap();
    assert_eq!(descending.0, [(5, 51), (3, 31), (1, 11)]);
    assert!(descending.1.is_none());
    println!("fluent_descending_three_instructions={}", descending.5);
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let reply = pic
        .update_call(
            canister,
            canister,
            "replace_binary_object",
            native::encode_request(
                14,
                &(
                    (
                        ic_wire::ByteBuf::from(vec![7; 1024]),
                        None::<ic_wire::ByteBuf>,
                    ),
                    false,
                ),
                LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    native::decode_response::<Result<(), u8>>(14, &reply, LIMITS)
        .unwrap()
        .unwrap();
    let before = pic.get_stable_memory(canister).await;
    let binary = async |projected: bool| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_binary_projection",
                native::encode_request(97, &projected, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<u64>, u32, u64)>(97, &reply, LIMITS).unwrap()
    };
    let full = binary(false).await;
    let projected = binary(true).await;
    assert_eq!(full.0, [1]);
    assert_eq!(full.0, projected.0);
    assert_eq!((full.1, projected.1), (1037, 8));
    println!("fluent_blob_full_instructions={}", full.2);
    println!("fluent_blob_projected_instructions={}", projected.2);
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.close().await;
}
