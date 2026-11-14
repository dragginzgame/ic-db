//! Managed field/index/revision behavior across real IC execution and upgrades.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::native;
type State = (Vec<(u64, String, i64, i64)>, Vec<u64>, u64);

#[tokio::test]
async fn managed_times_follow_batch_change_and_message_boundaries() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records-wasm"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |id: u64, body: &str, time: i64, mode: u8, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "managed_write",
            native::encode_request(82, &(id, body.to_string(), time, mode, trap), LIMITS).unwrap(),
        )
        .await
    };
    let decode =
        |bytes: Vec<u8>| native::decode_response::<Result<(), u8>>(82, &bytes, LIMITS).unwrap();
    let state = async |time: i64| {
        let bytes = pic
            .query_call(
                canister,
                canister,
                "managed_rows",
                native::encode_request(83, &time, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<State>(83, &bytes, LIMITS).unwrap()
    };
    decode(call(1, "first", 100, 5, false).await.unwrap()).unwrap();
    assert_eq!(
        state(100).await,
        (
            vec![
                (1, "first".into(), 100, 100),
                (2, "second".into(), 100, 100)
            ],
            vec![1, 2],
            1
        )
    );
    decode(
        call(1, "a longer replacement", 200, 1, false)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        state(200).await,
        (
            vec![
                (1, "a longer replacement".into(), 100, 200),
                (2, "second".into(), 100, 100)
            ],
            vec![1],
            2
        )
    );
    assert_eq!(state(100).await.1, vec![2]);
    let before = pic.get_stable_memory(canister).await;
    for now in [0, 200, 300] {
        decode(
            call(1, "a longer replacement", now, 1, false)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(pic.get_stable_memory(canister).await == before);
    }
    for (id, body, now, mode, expected) in [
        (3, "new", 300, 4, 1),
        (1, "changed", 150, 2, 2),
        (1, "changed", 300, 3, 2),
        (1, "changed", 199, 1, 3),
        (1, "second", 300, 1, 4),
    ] {
        assert_eq!(
            decode(call(id, body, now, mode, false).await.unwrap()),
            Err(expected)
        );
        assert!(pic.get_stable_memory(canister).await == before);
    }
    let error = call(1, "trapped", 300, 1, true).await.unwrap_err();
    assert!(error.reject_message.contains("after managed commit"));
    assert!(pic.get_stable_memory(canister).await == before);
    let before = state(200).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(state(200).await, before);
    owned.close().await;
}
