//! Real product records in Wasm, with raw stable backing as the rollback oracle.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::native;

type Status = ([u8; 32], [u8; 16], u64, u64);
type Page = (Vec<(u64, u64)>, Option<u64>, u64);

#[tokio::test]
async fn product_records_and_message_boundary() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    // The normal CLI host must not acquire qualification fault controls.
    let absent = pic
        .update_call(
            canister,
            canister,
            "fault_batch",
            native::encode_request(6, &(0_u64, 1_u64, false), LIMITS).unwrap(),
        )
        .await
        .expect_err("normal host must exclude fault endpoint");
    assert!(
        absent.reject_message.contains("has no update method"),
        "{absent:?}"
    );
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
    let page = async |after: u64, limit: u64, min: u64| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "query",
                native::encode_request(3, &(after, limit, min), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Page, u8>>(3, &reply, LIMITS).unwrap()
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
    let write = async |id: u64, category: u64, amount: u64, mode: u8| {
        pic.update_call(
            canister,
            canister,
            "write",
            native::encode_request(4, &(id, category, amount, mode), LIMITS).unwrap(),
        )
        .await
    };
    let original = status().await;
    assert_eq!(
        original.0.map(|b| format!("{b:02x}")).concat(),
        "80625391c9330ef8b4379a91b5e93663acd40384608a1a74197813267f2b2b3a"
    );
    assert_eq!(original.1, [1; 16]);
    assert_eq!(original.2, 1);
    let reply = pic
        .query_call(
            canister,
            canister,
            "schema",
            native::encode_request(2, &(), LIMITS).unwrap(),
        )
        .await
        .unwrap();
    type FieldView = (String, String, bool, Option<u32>);
    type IndexView = (u32, Vec<String>, bool);
    type EntityView = (String, u32, String, Vec<FieldView>, Vec<IndexView>);
    let (admitted, entities): (Status, Vec<EntityView>) =
        native::decode_response(2, &reply, LIMITS).unwrap();
    assert_eq!(admitted, original);
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[0].0, "Entry");
    assert_eq!(entities[0].2, "id");
    assert_eq!(
        entities[0].4,
        vec![(1, vec!["category".into(), "id".into()], false)]
    );
    assert_eq!(entities[1].0, "Document");
    assert_eq!(
        entities[1].3,
        vec![
            ("id".into(), "Nat64".into(), false, None),
            ("note".into(), "Text".into(), true, Some(64)),
            ("priority".into(), "Nat64".into(), true, None),
            ("published".into(), "Bool".into(), false, None),
            ("title".into(), "Text".into(), false, Some(32)),
        ]
    );
    assert!(entities[1].4.is_empty());
    assert_eq!(
        page(0, 3, 0).await,
        Ok((vec![(1, 11), (2, 20), (3, 31)], Some(3), 3))
    );
    assert_eq!(page(0, 3, 40).await, Ok((vec![], Some(3), 3)));
    assert_eq!(
        page(3, 3, 40).await,
        Ok((vec![(4, 40), (5, 51), (6, 60)], Some(6), 3))
    );
    assert_eq!(page(6, 3, 0).await, Ok((vec![], None, 0)));
    assert_eq!(page(0, 0, 0).await, Err(1));
    assert_eq!(category(1).await, vec![(1, 11), (3, 31), (5, 51)]);

    let before = pic.get_stable_memory(canister).await;
    let reply = write(1, 9, 900, 1).await.unwrap(); // duplicate insert: ordinary refusal
    assert_eq!(
        native::decode_response::<Result<(), u8>>(4, &reply, LIMITS).unwrap(),
        Err(3)
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let error = write(1, 9, 900, 2)
        .await
        .expect_err("late application trap");
    assert!(error.reject_message.contains("M1 application trap"));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(status().await, original);
    assert!(category(9).await.is_empty());
    assert_eq!(category(1).await, vec![(1, 11), (3, 31), (5, 51)]);

    // Returning an application error after a successful batch does not roll it back.
    let reply = write(1, 9, 900, 3).await.unwrap();
    assert_eq!(
        native::decode_response::<Result<(), u8>>(4, &reply, LIMITS).unwrap(),
        Err(4)
    );
    assert_eq!(status().await.2, 2);
    assert_eq!(category(9).await, vec![(1, 900)]);
    assert_eq!(category(1).await, vec![(3, 31), (5, 51)]);
    let before = pic.get_stable_memory(canister).await;
    let reply = write(1, 9, 900, 0).await.unwrap();
    assert_eq!(
        native::decode_response::<Result<(), u8>>(4, &reply, LIMITS).unwrap(),
        Ok(())
    );
    assert_eq!(pic.get_stable_memory(canister).await, before); // no-op preserves revision
    type DocumentValue = (String, Option<String>, bool, Option<u64>);
    let document = async || {
        let reply = pic
            .query_call(
                canister,
                canister,
                "document",
                native::encode_request(7, &(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(DocumentValue, u64)>(7, &reply, LIMITS).unwrap()
    };
    let replace_document = async |value: DocumentValue| {
        let reply = pic
            .update_call(
                canister,
                canister,
                "replace_document",
                native::encode_request(8, &value, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<(), u8>>(8, &reply, LIMITS).unwrap()
    };
    assert_eq!(
        document().await,
        (("First document".into(), None, false, Some(0)), 1)
    );
    let valid = ("é".repeat(16), Some(String::new()), true, None);
    assert_eq!(replace_document(valid.clone()).await, Ok(()));
    assert_eq!(document().await, (valid.clone(), 2));
    assert_eq!(status().await.2, 2); // only the Document entity revision advances
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        replace_document(("é".repeat(17), None, false, Some(1))).await,
        Err(1)
    );
    assert_eq!(
        replace_document((String::new(), Some("x".repeat(65)), false, None)).await,
        Err(1)
    );
    assert!(pic.get_stable_memory(canister).await == before);
    assert_eq!(document().await, (valid.clone(), 2));
    assert_eq!(replace_document(valid.clone()).await, Ok(()));
    assert!(pic.get_stable_memory(canister).await == before); // semantic no-op
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(status().await.2, 2);
    assert_eq!(category(9).await, vec![(1, 900)]);
    assert_eq!(page(0, 1, 0).await, Ok((vec![(1, 900)], Some(1), 1)));
    assert_eq!(document().await, (valid, 2));
    owned.close().await;
}
