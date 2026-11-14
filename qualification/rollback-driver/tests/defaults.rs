//! Installed omission/default behavior, index maintenance, rollback and unchanged upgrade.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::native;
type Row = (u64, u16, bool, Option<String>);
type State = (Vec<Row>, Vec<u64>, u64);

#[tokio::test]
async fn literal_defaults_remain_insert_only_across_real_message_boundaries() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records-wasm"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |id: u64, mode: u8, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "default_write",
            native::encode_request(80, &(id, mode, trap), LIMITS).unwrap(),
        )
        .await
    };
    let decode =
        |bytes: Vec<u8>| native::decode_response::<Result<Row, u8>>(80, &bytes, LIMITS).unwrap();
    let state = async || {
        let bytes = pic
            .query_call(
                canister,
                canister,
                "default_rows",
                native::encode_request(81, &(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<State>(81, &bytes, LIMITS).unwrap()
    };
    assert_eq!(
        decode(call(1, 0, false).await.unwrap()).unwrap(),
        (1, 7, true, Some("draft".into()))
    );
    assert_eq!(
        decode(call(2, 1, false).await.unwrap()).unwrap(),
        (2, 0, false, None)
    );
    let before = pic.get_stable_memory(canister).await;
    let current = state().await;
    assert_eq!(current.1, vec![1]);
    for (mode, error) in [(0, 3), (3, 1), (4, 2)] {
        assert_eq!(decode(call(3, mode, false).await.unwrap()), Err(error));
        assert!(pic.get_stable_memory(canister).await == before);
    }
    let error = call(3, 1, true).await.unwrap_err();
    assert!(error.reject_message.contains("after default commit"));
    assert!(pic.get_stable_memory(canister).await == before);
    assert_eq!(state().await, current);
    assert_eq!(
        decode(call(1, 2, false).await.unwrap()).unwrap(),
        (1, 0, false, None)
    );
    let revision = state().await.2;
    decode(call(1, 2, false).await.unwrap()).unwrap();
    assert_eq!(state().await.2, revision);
    assert!(state().await.1.is_empty());
    decode(call(3, 0, false).await.unwrap()).unwrap();
    assert_eq!(state().await.1, vec![3]);
    let before = state().await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(state().await, before);
    owned.close().await;
}
