//! Real product records in Wasm, with raw stable backing as the rollback oracle.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::native;

type Status = ([u8; 32], [u8; 16], u64, u64);
type Page = (Vec<(u64, u64)>, Option<u64>, u64);

#[tokio::test]
async fn references_follow_final_batch_state_in_wasm() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let reference = async |id: u64| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "document_reference",
                native::encode_request(9, &id, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Option<u64>>(9, &reply, LIMITS).unwrap()
    };
    let batch = async |mode: u8| {
        let reply = pic
            .update_call(
                canister,
                canister,
                "reference_batch",
                native::encode_request(10, &mode, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<(), u8>>(10, &reply, LIMITS).unwrap()
    };
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(batch(0).await, Err(1)); // missing target
    assert_eq!(batch(1).await, Err(1)); // surviving dependent
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(reference(1).await, Some(1));
    assert_eq!(batch(4).await, Ok(())); // dependent is added before its target
    assert_eq!(reference(2).await, Some(7));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(batch(5).await, Err(1));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(batch(6).await, Ok(())); // delete target and dependent together
    assert_eq!(reference(2).await, None);
    assert_eq!(batch(3).await, Ok(())); // repoint dependent and delete old target
    assert_eq!(reference(1).await, Some(2));
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(reference(1).await, Some(2));
    owned.close().await;
}

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
        // Independently assembled with Perl pack(V/C) and OpenSSL SHA-256.
        "75edf28841acea2e80a386f6655120ea9bcb2aa628c9950e5670514c82b78ee1"
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
    type FieldView = (String, String, bool, Option<u32>, Option<(u32, u32)>);
    type IndexView = (u32, Vec<String>, bool);
    type EntityView = (String, u32, String, Vec<FieldView>, Vec<IndexView>);
    let (admitted, entities): (Status, Vec<EntityView>) =
        native::decode_response(2, &reply, LIMITS).unwrap();
    assert_eq!(admitted, original);
    assert_eq!(entities.len(), 3);
    assert_eq!(entities[2].0, "WideNumbers");
    assert_eq!(
        entities[2].3,
        vec![
            ("id".into(), "Nat64".into(), false, None, None),
            ("optional_signed".into(), "Int128".into(), true, None, None),
            (
                "optional_unsigned".into(),
                "Nat128".into(),
                true,
                None,
                None
            ),
            ("signed".into(), "Int128".into(), false, None, None),
            ("unsigned".into(), "Nat128".into(), false, None, None),
        ]
    );
    assert!(entities[2].4.is_empty());
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
            ("entry_id".into(), "Nat64".into(), false, None, Some((1, 1))),
            ("id".into(), "Nat64".into(), false, None, None),
            ("note".into(), "Text".into(), true, Some(64), None),
            ("previous_score".into(), "Int64".into(), true, None, None),
            ("priority".into(), "Nat64".into(), true, None, None),
            ("published".into(), "Bool".into(), false, None, None),
            ("score".into(), "Int64".into(), false, None, None),
            ("title".into(), "Text".into(), false, Some(32), None),
        ]
    );
    assert_eq!(
        entities[1].4,
        vec![(1, vec!["entry_id".into(), "id".into()], false)]
    );
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
    type DocumentValue = (String, Option<String>, bool, Option<u64>, i64, Option<i64>);
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
        (
            ("First document".into(), None, false, Some(0), -10, None),
            1
        )
    );
    let valid = (
        "é".repeat(16),
        Some(String::new()),
        true,
        None,
        i64::MIN,
        Some(i64::MAX),
    );
    assert_eq!(replace_document(valid.clone()).await, Ok(()));
    assert_eq!(document().await, (valid.clone(), 2));
    assert_eq!(status().await.2, 2); // only the Document entity revision advances
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        replace_document(("é".repeat(17), None, false, Some(1), -1, None)).await,
        Err(1)
    );
    assert_eq!(
        replace_document((String::new(), Some("x".repeat(65)), false, None, 0, Some(0))).await,
        Err(1)
    );
    assert!(pic.get_stable_memory(canister).await == before);
    assert_eq!(document().await, (valid.clone(), 2));
    assert_eq!(replace_document(valid.clone()).await, Ok(()));
    assert!(pic.get_stable_memory(canister).await == before); // semantic no-op
    // Signed extrema, zero and absence retain their exact values in real Wasm.
    for (offset, (score, previous)) in [(i64::MAX, Some(i64::MIN)), (-1, Some(0)), (0, None)]
        .into_iter()
        .enumerate()
    {
        let next = (
            valid.0.clone(),
            valid.1.clone(),
            valid.2,
            valid.3,
            score,
            previous,
        );
        assert_eq!(replace_document(next.clone()).await, Ok(()));
        assert_eq!(document().await, (next, 3 + offset as u64));
    }
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(status().await.2, 2);
    assert_eq!(category(9).await, vec![(1, 900)]);
    assert_eq!(page(0, 1, 0).await, Ok((vec![(1, 900)], Some(1), 1)));
    assert_eq!(
        document().await,
        ((valid.0, valid.1, valid.2, valid.3, 0, None), 5)
    );
    owned.close().await;
}

#[tokio::test]
async fn wide_numbers_are_exact_in_wasm_and_rollback_with_the_message() {
    type Value = (i128, u128, Option<i128>, Option<u128>);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let read = async || {
        let reply = pic
            .query_call(
                canister,
                canister,
                "wide_numbers",
                native::encode_request(11, &(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Value, u64)>(11, &reply, LIMITS).unwrap()
    };
    let write = async |value: Value, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "replace_wide_numbers",
            native::encode_request(12, &(value, trap), LIMITS).unwrap(),
        )
        .await
    };
    assert_eq!(read().await, ((i128::MIN, u128::MAX, None, Some(0)), 1));
    let cases = [
        (i128::MAX, 1_u128 << 127, Some(i128::MIN), Some(u128::MAX)),
        (-(1_i128 << 100) - 1, (1_u128 << 100) + 1, None, None),
        (
            i128::from(i64::MAX) + 1,
            u128::from(u64::MAX) + 1,
            Some(-1),
            Some(1),
        ),
        (0, 0, Some(0), Some(0)),
        (i128::MIN, u128::MAX, Some(i128::MAX), None),
    ];
    for (offset, value) in cases.into_iter().enumerate() {
        write(value, false).await.unwrap();
        assert_eq!(read().await, (value, 2 + offset as u64));
    }
    let before = pic.get_stable_memory(canister).await;
    let final_value = *cases.last().unwrap();
    write(final_value, false).await.unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before); // no-op
    let error = write((0, 0, None, None), true)
        .await
        .expect_err("late trap");
    assert!(
        error
            .reject_message
            .contains("M1 wide-number application trap")
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (final_value, 6));
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (final_value, 6));
    owned.close().await;
}
