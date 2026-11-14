//! Real product records in Wasm, with raw stable backing as the rollback oracle.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::native;
use std::{
    io::Write,
    process::{Command, Stdio},
};

type Status = ([u8; 32], [u8; 16], u64, u64);
type Page = (Vec<(u64, u64)>, Option<u64>, u64);

#[test]
fn console_inspects_and_queries() {
    let prefix = std::env::temp_dir().join(format!("ic-db-console-{}", std::process::id()));
    std::fs::create_dir(&prefix).unwrap();
    std::fs::create_dir(prefix.join("bin")).unwrap();
    std::fs::create_dir_all(prefix.join("share/ic-db/m1")).unwrap();
    let binary = prefix.join("bin/db");
    std::fs::copy(env!("CARGO_BIN_EXE_db"), &binary).unwrap();
    std::fs::copy(
        std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"),
        prefix.join("share/ic-db/m1/records.wasm"),
    )
    .unwrap();
    let mut child = Command::new(&binary)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"\n\nschema\nstatus\nquery --min 100\nquery --after 3 --limit 2\nquit\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("LOCAL M1 FIXTURE"));
    assert!(
        !stdout.contains("Usage: db"),
        "blank input must not print help"
    );
    for entity in [
        "Entry",
        "Document",
        "WideNumbers",
        "BinaryObject",
        "NarrowNumbers",
        "IdentityRow",
    ] {
        assert!(stdout.contains(entity));
    }
    assert!(stdout.contains("ICDBMF01"));
    assert!(stdout.contains("Exact match"));
    assert!(stdout.contains("0 rows returned; 3 candidates examined."));
    assert!(stdout.contains("Next page: db query --after 3 --limit 3 --min 100"));
    let contains_row = |text: &str, id: &str, amount: &str| {
        text.lines().any(|line| {
            line.split(['|', '│'])
                .map(str::trim)
                .filter(|cell| !cell.is_empty())
                .eq([id, amount])
        })
    };
    assert!(contains_row(&stdout, "4", "40"));
    assert!(contains_row(&stdout, "5", "51"));
    let output = Command::new(&binary)
        .current_dir(std::env::temp_dir())
        .args(["query", "--after", "3", "--limit", "2"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(contains_row(&stdout, "4", "40"));
    assert!(contains_row(&stdout, "5", "51"));
    for command in [
        "query --limit 0",
        "query --limit 1 --limit 2",
        "SELECT * FROM rows",
    ] {
        let output = Command::new(&binary)
            .args(command.split_whitespace())
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
    std::fs::remove_dir_all(prefix).unwrap();
    // Help must work even for the uninstalled build artifact with no adjacent fixture.
    let output = Command::new(env!("CARGO_BIN_EXE_db"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("Usage: db [COMMAND]")
    );
}

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
            native::encode_request(6, &(0_u64, 1_u64, false, false), LIMITS).unwrap(),
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
        // Independently assembled with Perl pack(V/C) and SHA-256.
        "c0eb1f78d335c405a703e8e6f7f4838add36835385760d519e18291ffd527d51"
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
    assert_eq!(entities.len(), 6);
    assert_eq!(entities[5].0, "IdentityRow");
    assert!(
        entities[5]
            .3
            .iter()
            .any(|field| field.0 == "owner" && field.1 == "Principal" && field.3 == Some(29))
    );
    assert_eq!(entities[4].0, "NarrowNumbers");
    assert!(entities[4].4.is_empty());
    for (name, kind) in [
        ("signed8", "Int8"),
        ("signed16", "Int16"),
        ("signed32", "Int32"),
        ("unsigned8", "Nat8"),
        ("unsigned16", "Nat16"),
        ("unsigned32", "Nat32"),
    ] {
        assert!(
            entities[4]
                .3
                .contains(&(name.into(), kind.into(), false, None, None))
        );
        assert!(entities[4].3.contains(&(
            format!("optional_{name}"),
            kind.into(),
            true,
            None,
            None
        )));
    }
    assert_eq!(entities[3].0, "BinaryObject");
    assert_eq!(
        entities[3].3,
        vec![
            ("id".into(), "Nat64".into(), false, None, None),
            ("optional".into(), "Blob".into(), true, Some(32), None),
            ("payload".into(), "Blob".into(), false, Some(1024), None),
        ]
    );
    assert!(entities[3].4.is_empty());
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
        vec![(1, vec!["entry_id".into(), "score".into()], false)]
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

#[tokio::test]
async fn binary_values_preserve_bounds_and_bytes_in_wasm() {
    use ic_wire::ByteBuf;
    type Value = (ByteBuf, Option<ByteBuf>);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let decodes = async |bytes: Vec<u8>| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "binary_decodes",
                native::encode_request(15, &ByteBuf::from(bytes), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<bool>(15, &reply, LIMITS).unwrap()
    };
    // Independent persisted bytes, transported opaquely: id, absent option,
    // little-endian u32 length, payload. Exercise decoding on 32-bit Wasm.
    let valid = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 255, 128];
    assert!(decodes(valid.clone()).await);
    let mut bad_option = valid.clone();
    bad_option[8] = 2;
    for invalid in [
        valid[..15].to_vec(),
        [valid.as_slice(), &[0]].concat(),
        bad_option,
        vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255],
        vec![0; 4097],
    ] {
        assert!(!decodes(invalid).await);
    }
    let read = async || {
        let reply = pic
            .query_call(
                canister,
                canister,
                "binary_object",
                native::encode_request(13, &(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Value, u64)>(13, &reply, LIMITS).unwrap()
    };
    let write = async |value: Value, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "replace_binary_object",
            native::encode_request(14, &(value, trap), LIMITS).unwrap(),
        )
        .await
    };
    assert_eq!(read().await, ((ByteBuf::from(vec![0, 255, 128]), None), 1));
    let full = (
        ByteBuf::from(vec![255; 1024]),
        Some(ByteBuf::from(vec![128; 32])),
    );
    for (offset, value) in [(ByteBuf::new(), Some(ByteBuf::new())), full.clone()]
        .into_iter()
        .enumerate()
    {
        let reply = write(value.clone(), false).await.unwrap();
        assert_eq!(
            native::decode_response::<Result<(), u8>>(14, &reply, LIMITS).unwrap(),
            Ok(())
        );
        assert_eq!(read().await, (value, 2 + offset as u64));
    }
    let before = pic.get_stable_memory(canister).await;
    for invalid in [
        (ByteBuf::from(vec![0; 1025]), None),
        (ByteBuf::new(), Some(ByteBuf::from(vec![0; 33]))),
    ] {
        let reply = write(invalid, false).await.unwrap();
        assert_eq!(
            native::decode_response::<Result<(), u8>>(14, &reply, LIMITS).unwrap(),
            Err(1)
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    let reply = write(full.clone(), false).await.unwrap();
    assert_eq!(
        native::decode_response::<Result<(), u8>>(14, &reply, LIMITS).unwrap(),
        Ok(())
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let error = write((ByteBuf::new(), None), true)
        .await
        .expect_err("late trap");
    assert!(error.reject_message.contains("M1 binary application trap"));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (full, 3));
    owned.close().await;
}

#[tokio::test]
async fn narrow_numbers_preserve_widths_absence_and_message_rollback() {
    type Required = (i8, i16, i32, u8, u16, u32);
    type Optional = (
        Option<i8>,
        Option<i16>,
        Option<i32>,
        Option<u8>,
        Option<u16>,
        Option<u32>,
    );
    type Value = (Required, Optional);
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
                "narrow_numbers",
                native::encode_request(16, &(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Value, u64)>(16, &reply, LIMITS).unwrap()
    };
    let write = async |value: Value, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "replace_narrow_numbers",
            native::encode_request(17, &(value, trap), LIMITS).unwrap(),
        )
        .await
    };
    let minimum = (i8::MIN, i16::MIN, i32::MIN, u8::MAX, u16::MAX, u32::MAX);
    let absent = (None, None, None, None, None, None);
    assert_eq!(read().await, ((minimum, absent), 1));
    let values = [
        minimum,
        (-1, -1, -1, 0, 0, 0),
        (0, 0, 0, 0, 0, 0),
        (1, 1, 1, 1, 1, 1),
        (i8::MAX, i16::MAX, i32::MAX, 128, 32768, 2147483648),
    ];
    for (offset, required) in values.into_iter().enumerate() {
        let optional = (
            Some(required.0),
            Some(required.1),
            Some(required.2),
            Some(required.3),
            Some(required.4),
            Some(required.5),
        );
        write((required, optional), false).await.unwrap();
        assert_eq!(read().await, ((required, optional), 2 + offset as u64));
    }
    let (final_value, revision) = read().await;
    let before = pic.get_stable_memory(canister).await;
    write(final_value, false).await.unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let error = write((minimum, absent), true).await.expect_err("late trap");
    assert!(
        error
            .reject_message
            .contains("M1 narrow-number application trap")
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (final_value, revision));
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (final_value, revision));
    owned.close().await;
}

#[tokio::test]
async fn principal_identity_bytes_survive_wasm_reopen_and_rollback() {
    use ic_wire::ByteBuf;
    type Value = (ByteBuf, Option<ByteBuf>);
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
                "principal_record",
                native::encode_request(18, &(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Value, u64)>(18, &reply, LIMITS).unwrap()
    };
    let write = async |value: Value, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "replace_principal_record",
            native::encode_request(19, &(value, trap), LIMITS).unwrap(),
        )
        .await
    };
    let decodes = async |bytes: Vec<u8>| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "principal_decodes",
                native::encode_request(20, &ByteBuf::from(bytes), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<bool>(20, &reply, LIMITS).unwrap()
    };
    assert_eq!(read().await, ((ByteBuf::from(vec![4]), None), 1));
    let golden = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 4];
    assert!(decodes(golden.clone()).await);
    for end in 0..golden.len() {
        assert!(!decodes(golden[..end].to_vec()).await);
    }
    let mut invalid_tag = golden.clone();
    invalid_tag[8] = 2;
    assert!(!decodes(invalid_tag).await);
    let mut trailing = golden.clone();
    trailing.push(0);
    assert!(!decodes(trailing).await);
    for length in [30_u32, u32::MAX] {
        let mut oversized = golden[..9].to_vec();
        oversized.extend_from_slice(&length.to_le_bytes());
        oversized.extend_from_slice(&[0; 30]);
        assert!(!decodes(oversized).await);
    }
    for (index, bytes) in [vec![], vec![4], vec![2, 7], vec![0, 0], vec![0xAB; 29]]
        .into_iter()
        .enumerate()
    {
        let value = (ByteBuf::from(bytes.clone()), Some(ByteBuf::from(bytes)));
        let reply = write(value.clone(), false).await.unwrap();
        assert_eq!(
            native::decode_response::<Result<(), u8>>(19, &reply, LIMITS).unwrap(),
            Ok(())
        );
        assert_eq!(read().await, (value, index as u64 + 2));
    }
    let (final_value, revision) = read().await;
    let before = pic.get_stable_memory(canister).await;
    for invalid in [
        (ByteBuf::from(vec![0; 30]), None),
        (ByteBuf::new(), Some(ByteBuf::from(vec![0; 30]))),
    ] {
        let reply = write(invalid, false).await.unwrap();
        assert_eq!(
            native::decode_response::<Result<(), u8>>(19, &reply, LIMITS).unwrap(),
            Err(1)
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    write(final_value.clone(), false).await.unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let error = write((ByteBuf::new(), None), true)
        .await
        .expect_err("late trap");
    assert!(
        error
            .reject_message
            .contains("M1 principal application trap")
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(read().await, (final_value, revision));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.close().await;
}

#[tokio::test]
async fn typed_index_order_uniqueness_and_rollback_survive_wasm_reopen() {
    use ic_wire::ByteBuf;
    type Read = (Vec<u64>, Vec<(i64, u64)>);
    type Write = Vec<(u64, ByteBuf, i64)>;
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |rows: Write, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "indexed_batch",
            native::encode_request(21, &(rows, trap), LIMITS).unwrap(),
        )
        .await
    };
    let read = async |owner: Vec<u8>| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "typed_indexes",
                native::encode_request(22, &ByteBuf::from(owner), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Read, u8>>(22, &reply, LIMITS)
            .unwrap()
            .unwrap()
    };
    let values = vec![
        (1, ByteBuf::from(vec![4]), i64::MIN),
        (2, ByteBuf::new(), i64::MAX),
        (3, ByteBuf::from(vec![0]), 0),
        (4, ByteBuf::from(vec![0, 0]), -1),
        (5, ByteBuf::from(vec![255; 29]), 0),
    ];
    let reply = write(values.clone(), false).await.unwrap();
    assert_eq!(
        native::decode_response::<Result<(), u8>>(21, &reply, LIMITS).unwrap(),
        Ok(())
    );
    let expected = vec![(i64::MIN, 1), (-1, 4), (0, 3), (0, 5), (i64::MAX, 2)];
    for (id, owner, _) in values {
        assert_eq!(read(owner.into_vec()).await, (vec![id], expected.clone()));
    }
    assert_eq!(read(vec![1]).await, (vec![], expected));
    let before = pic.get_stable_memory(canister).await;
    for duplicate in [
        vec![(6, ByteBuf::new(), 1)],
        vec![
            (6, ByteBuf::from(vec![8]), 1),
            (7, ByteBuf::from(vec![8]), 2),
        ],
    ] {
        let reply = write(duplicate, false).await.unwrap();
        assert_eq!(
            native::decode_response::<Result<(), u8>>(21, &reply, LIMITS).unwrap(),
            Err(3)
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    let swap = vec![
        (1, ByteBuf::new(), i64::MAX),
        (2, ByteBuf::from(vec![4]), i64::MIN),
    ];
    assert!(write(swap.clone(), true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let reply = write(swap, false).await.unwrap();
    assert_eq!(
        native::decode_response::<Result<(), u8>>(21, &reply, LIMITS).unwrap(),
        Ok(())
    );
    let expected = vec![(i64::MIN, 2), (-1, 4), (0, 3), (0, 5), (i64::MAX, 1)];
    assert_eq!(read(vec![]).await, (vec![1], expected.clone()));
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(read(vec![4]).await, (vec![2], expected));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.close().await;
}
