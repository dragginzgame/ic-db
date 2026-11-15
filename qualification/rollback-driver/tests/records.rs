//! Real product records in Wasm, with raw stable backing as the rollback oracle.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::native;
use std::{
    io::Write,
    process::{Command, Stdio},
};

type Status = ([u8; 32], [u8; 16], u64, u64);
type Page = (Vec<(u64, u64)>, Option<Vec<u8>>, u32, u32, u32, u64);

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
        .write_all(b"\n\nschema\nstatus\nquery Entry --limit 32\nquery --min 100 --work 3\nquery Entry --min 40 --limit 2 --select id,amount\nquit\n")
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
        "OptionalLink",
        "EnumRow",
    ] {
        assert!(stdout.contains(entity));
    }
    assert!(stdout.contains("ICDBMF01"));
    assert!(stdout.contains("Exact match"));
    assert!(stdout.contains("Membership"));
    assert!(stdout.contains("Enum: Lifecycle"));
    assert!(stdout.contains("state,id"));
    assert!(stdout.contains("Record: Details"));
    assert!(stdout.contains("List<Nat16>"));
    assert!(stdout.contains("Info"));
    assert!(stdout.contains("Record: ListNode"));
    assert!(stdout.contains("Set: Scores"));
    assert!(stdout.contains("Map: Rates"));
    assert!(stdout.contains("FloatRow"));
    assert!(stdout.contains("AccountRow"));
    assert!(stdout.contains("UlidRow"));
    assert!(stdout.contains("DateRow"));
    assert!(stdout.contains("TimeRow"));
    assert!(stdout.contains("DecimalRow"));
    assert!(stdout.contains("KeyRow"));
    assert!(stdout.contains("SignedKey"));
    assert!(stdout.contains("WideKey.id (via peer,id)"));
    assert!(stdout.contains("KeyRow.id (via target,id)"));
    assert!(stdout.contains("17 bytes; scale 2; exact"));
    assert!(stdout.contains("Unix milliseconds"));
    assert!(stdout.contains("Subaccount"));
    assert!(stdout.contains("Float32"));
    assert!(stdout.contains("Float64"));
    assert!(stdout.contains("Enum: NestedPayload"));
    assert!(stdout.contains("OptionalLink.id (via next,id)"));
    assert!(stdout.contains("All optional keys present"));
    assert!(stdout.contains("0 rows; 3 examined; 3 fetched; 0 encoded bytes."));
    assert!(stdout.contains("6 rows; 6 examined; 6 fetched; 144 encoded bytes."));
    assert!(!stdout.contains("Next page:"));
    let cursor = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("Next cursor: "))
        .next_back()
        .unwrap();
    assert_ne!(cursor, "none");
    let contains_row = |text: &str, expected: &[&str]| {
        text.lines().any(|line| {
            line.split(['|', '│'])
                .map(str::trim)
                .filter(|cell| !cell.is_empty())
                .eq(expected.iter().copied())
        })
    };
    assert!(contains_row(
        &stdout,
        &[
            "labels",
            "List<Text>",
            "Yes",
            "-",
            "max 3 items; item max 4 bytes"
        ]
    ));
    assert!(contains_row(
        &stdout,
        &["label", "Text", "No", "max 8 bytes"]
    ));
    assert!(contains_row(
        &stdout,
        &["value", "Text", "Yes", "max 4 bytes"]
    ));
    assert!(contains_row(
        &stdout,
        &["element", "Int16", "No", "max 3 items"]
    ));
    assert!(contains_row(
        &stdout,
        &["key", "Int16", "No", "map max 3 entries"]
    ));
    assert!(contains_row(&stdout, &["4", "40"]));
    assert!(contains_row(&stdout, &["5", "51"]));
    let output = Command::new(&binary)
        .current_dir(std::env::temp_dir())
        .args([
            "query",
            "Entry",
            "--after",
            cursor,
            "--min",
            "40",
            "--limit",
            "2",
            "--select",
            "id,amount",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(contains_row(&stdout, &["6", "60"]));
    assert!(stdout.contains("Next cursor: none"));
    let output = Command::new(&binary)
        .args([
            "query",
            "Entry",
            "--category",
            "1",
            "--order-by",
            "id",
            "--desc",
            "--select",
            "id,amount",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ordered = String::from_utf8(output.stdout).unwrap();
    let positions: Vec<_> = ["51", "31", "11"]
        .iter()
        .map(|v| ordered.find(v).unwrap())
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]));
    assert!(ordered.contains("3 rows; 3 examined; 3 fetched; 48 encoded bytes."));
    let output = Command::new(&binary)
        .args([
            "query",
            "Document",
            "--entry-id",
            "1",
            "--order-by",
            "score",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("First document")
    );
    let output = Command::new(&binary)
        .args(["query", "Document", "--select", "id"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("1 row; 1 examined; 1 fetched; 8 encoded bytes.")
    );
    let output = Command::new(&binary)
        .args([
            "query",
            "Entry",
            "--after",
            cursor,
            "--min",
            "41",
            "--select",
            "id,amount",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid cursor"));
    for (entity, expected, excluded) in [
        ("Entry", "category", "Enum: Lifecycle"),
        ("EnumRow", "Enum: Lifecycle", "Record: Details"),
        ("NestedRow", "Record: Details", "Enum: Lifecycle"),
        ("ListRow", "Record: ListNode", "Enum: Lifecycle"),
        ("MapTreeRow", "Map: MapTree", "Enum: Lifecycle"),
        (
            "Document",
            "Entry.id (via entry_id,score)",
            "Enum: Lifecycle",
        ),
    ] {
        let output = Command::new(&binary)
            .args(["schema", entity])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("Schema: 1 entity"));
        assert!(stdout.contains(expected), "{entity}: {stdout}");
        assert!(!stdout.contains(excluded), "{entity}: {stdout}");
        assert!(!stdout.lines().any(|line| line == "WideNumbers"));
        assert_eq!(
            stdout.matches("Record: ListNode").count(),
            usize::from(entity == "ListRow")
        );
    }
    let output = Command::new(&binary)
        .args(["schema", "Missing"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown entity 'Missing'"));
    for command in [
        "query --limit 0",
        "query --limit 1 --limit 2",
        "query --after 3",
        "query --work 513",
        "query --bytes 0",
        "query Missing",
        "query Document --min 0",
        "query Entry --order-by amount",
        "query Entry --select id --bytes 7",
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
    let output = Command::new(env!("CARGO_BIN_EXE_db"))
        .args(["query", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--order-by"));
    assert!(help.contains("An empty page may still continue"));
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
    let page = async |after: Option<Vec<u8>>, limit: u32, min: u64| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_query",
                native::encode_request(
                    95,
                    &(after, false, None::<u64>, min, limit, limit, 4096_u32),
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Page, u8>>(95, &reply, LIMITS).unwrap()
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
        // Independently assembled with Node byte framing and SHA-256.
        "3d9eff928eeb4e73eceeec428f447f05512e428f53f51fb9a878dbbb195260f8"
    );
    assert_eq!(original.1, [1; 16]);
    assert_eq!(original.2, 1);
    let reply = pic
        .query_call(
            canister,
            canister,
            "schema",
            native::encode_request(2, &None::<String>, LIMITS).unwrap(),
        )
        .await
        .unwrap();
    type FieldView = (String, String, bool, String, Option<(String, String)>);
    type IndexView = (String, Vec<String>, bool, bool);
    type EntityView = (String, u32, String, Vec<FieldView>, Vec<IndexView>);
    type TypeView = (String, String, Vec<(String, String, bool, String)>);
    let (admitted, entities, types): (Status, Vec<EntityView>, Vec<TypeView>) =
        native::decode_response(2, &reply, LIMITS).unwrap();
    assert_eq!(admitted, original);
    let entity = |name: &str| entities.iter().find(|entity| entity.0 == name).unwrap();
    assert_eq!(entities.len(), 31);
    assert_eq!(types.len(), 10);
    assert!(entity("ManagedRow").3.contains(&(
        "updated".into(),
        "Timestamp".into(),
        false,
        "8 bytes; Unix milliseconds; managed updated_at".into(),
        None,
    )));
    assert!(entity("DefaultRow").3.contains(&(
        "label".into(),
        "Text".into(),
        true,
        "max 8 bytes; default \"draft\"".into(),
        None,
    )));
    assert!(entity("UlidRow").3.contains(&(
        "value".into(),
        "Ulid".into(),
        false,
        "16 bytes".into(),
        None
    )));
    assert!(entity("AccountRow").3.contains(&(
        "sub".into(),
        "Subaccount".into(),
        false,
        "32 bytes".into(),
        None
    )));
    assert!(entity("AccountRow").3.contains(&(
        "value".into(),
        "Account".into(),
        false,
        "owner max 29 bytes; subaccount 32 bytes".into(),
        None
    )));
    assert!(entity("FloatRow").3.contains(&(
        "small".into(),
        "Float32".into(),
        false,
        String::new(),
        None
    )));
    assert!(entity("FloatRow").3.contains(&(
        "value".into(),
        "Float64".into(),
        false,
        String::new(),
        None
    )));
    assert!(entity("BigNumbers").3.contains(&(
        "value".into(),
        "U256".into(),
        false,
        String::new(),
        None
    )));
    assert!(types.iter().any(|(name, record, fields)| name == "Details"
        && record == "Record"
        && fields.contains(&("x".into(), "Nat16".into(), false, String::new()))));
    assert_eq!(entity("OptionalLink").0, "OptionalLink");
    assert!(entity("OptionalLink").3.contains(&(
        "next".into(),
        "Nat64".into(),
        true,
        String::new(),
        Some(("OptionalLink.id".into(), "next,id".into()))
    )));
    assert_eq!(entity("IdentityRow").0, "IdentityRow");
    assert!(
        entity("IdentityRow")
            .3
            .iter()
            .any(|field| field.0 == "owner" && field.1 == "Principal" && field.3 == "max 29 bytes")
    );
    assert_eq!(entity("NarrowNumbers").0, "NarrowNumbers");
    assert!(entity("NarrowNumbers").4.is_empty());
    for (name, kind) in [
        ("signed8", "Int8"),
        ("signed16", "Int16"),
        ("signed32", "Int32"),
        ("unsigned8", "Nat8"),
        ("unsigned16", "Nat16"),
        ("unsigned32", "Nat32"),
    ] {
        assert!(entity("NarrowNumbers").3.contains(&(
            name.into(),
            kind.into(),
            false,
            String::new(),
            None
        )));
        assert!(entity("NarrowNumbers").3.contains(&(
            format!("optional_{name}"),
            kind.into(),
            true,
            String::new(),
            None
        )));
    }
    assert_eq!(entity("BinaryObject").0, "BinaryObject");
    assert_eq!(
        entity("BinaryObject").3,
        vec![
            ("id".into(), "Nat64".into(), false, String::new(), None),
            (
                "optional".into(),
                "Blob".into(),
                true,
                "max 32 bytes".into(),
                None
            ),
            (
                "payload".into(),
                "Blob".into(),
                false,
                "max 1024 bytes".into(),
                None
            ),
        ]
    );
    assert!(entity("BinaryObject").4.is_empty());
    assert_eq!(entity("WideNumbers").0, "WideNumbers");
    assert_eq!(
        entity("WideNumbers").3,
        vec![
            ("id".into(), "Nat64".into(), false, String::new(), None),
            (
                "optional_signed".into(),
                "Int128".into(),
                true,
                String::new(),
                None
            ),
            (
                "optional_unsigned".into(),
                "Nat128".into(),
                true,
                String::new(),
                None
            ),
            ("signed".into(), "Int128".into(), false, String::new(), None),
            (
                "unsigned".into(),
                "Nat128".into(),
                false,
                String::new(),
                None
            ),
        ]
    );
    assert!(entity("WideNumbers").4.is_empty());
    assert_eq!(entity("Entry").0, "Entry");
    assert_eq!(entity("Entry").2, "id");
    assert_eq!(
        entity("Entry").4,
        vec![(
            "category,id".into(),
            vec!["category".into(), "id".into()],
            false,
            false
        )]
    );
    assert_eq!(entity("Document").0, "Document");
    assert_eq!(
        entity("Document").3,
        vec![
            (
                "entry_id".into(),
                "Nat64".into(),
                false,
                String::new(),
                Some(("Entry.id".into(), "entry_id,score".into()))
            ),
            ("id".into(), "Nat64".into(), false, String::new(), None),
            (
                "note".into(),
                "Text".into(),
                true,
                "max 64 bytes".into(),
                None
            ),
            (
                "previous_score".into(),
                "Int64".into(),
                true,
                String::new(),
                None
            ),
            ("priority".into(), "Nat64".into(), true, String::new(), None),
            (
                "published".into(),
                "Bool".into(),
                false,
                String::new(),
                None
            ),
            ("score".into(), "Int64".into(), false, String::new(), None),
            (
                "title".into(),
                "Text".into(),
                false,
                "max 32 bytes".into(),
                None
            ),
        ]
    );
    assert_eq!(
        entity("Document").4,
        vec![
            (
                "entry_id,priority".into(),
                vec!["entry_id".into(), "priority".into()],
                false,
                false
            ),
            (
                "entry_id,score".into(),
                vec!["entry_id".into(), "score".into()],
                false,
                false
            ),
            (
                "published,title".into(),
                vec!["published".into(), "title".into()],
                false,
                false
            ),
        ]
    );
    let first = page(None, 3, 0).await.unwrap();
    assert_eq!(first.0, [(1, 11), (2, 20), (3, 31)]);
    assert_eq!(first.2, 3);
    assert!(first.1.is_some());
    let sparse = page(None, 3, 40).await.unwrap();
    assert!(sparse.0.is_empty());
    assert_eq!(sparse.2, 3);
    let last = page(sparse.1, 3, 40).await.unwrap();
    assert_eq!(last.0, [(4, 40), (5, 51), (6, 60)]);
    assert_eq!(last.2, 3);
    let done = page(last.1, 3, 40).await.unwrap();
    assert!(done.0.is_empty());
    assert_eq!((done.1, done.2), (None, 0));
    assert_eq!(page(None, 0, 0).await, Err(3));
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
    let first = page(None, 1, 0).await.unwrap();
    assert_eq!(first.0, [(1, 900)]);
    assert_eq!(first.2, 1);
    assert!(first.1.is_some());
    let after_upgrade = (valid.0, valid.1, valid.2, valid.3, 0, None);
    assert_eq!(document().await, (after_upgrade.clone(), 5));
    // Cold query initialization after upgrade must not leave stale leases or
    // admission in the subsequent replicated update and query.
    let mut next = after_upgrade;
    next.4 = 1;
    assert_eq!(replace_document(next.clone()).await, Ok(()));
    assert_eq!(document().await, (next, 6));
    assert_eq!(status().await.2, 2);
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

#[tokio::test]
async fn text_index_boundaries_and_boolean_prefixes_survive_wasm_updates() {
    type Rows = Vec<(String, u64)>;
    type Writes = Vec<(u64, String, bool)>;
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |rows: Writes, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "text_index_batch",
            native::encode_request(23, &(rows, trap), LIMITS).unwrap(),
        )
        .await
    };
    let read = async |published: bool, title: Option<String>| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "text_index",
                native::encode_request(24, &(published, title), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Rows, u8>>(24, &reply, LIMITS).unwrap()
    };
    let values = ["aa", "a\0", "a", "", "é", "e\u{301}", "a", "\0", "😀"];
    let rows: Writes = values
        .iter()
        .enumerate()
        .map(|(i, s)| (i as u64 + 1, (*s).into(), false))
        .collect();
    let reply = write(rows, false).await.unwrap();
    assert_eq!(
        native::decode_response::<Result<(), u8>>(23, &reply, LIMITS).unwrap(),
        Ok(())
    );
    let mut expected: Rows = values
        .iter()
        .enumerate()
        .map(|(i, s)| ((*s).into(), i as u64 + 1))
        .collect();
    expected.sort();
    assert_eq!(read(false, None).await, Ok(expected.clone()));
    assert_eq!(
        read(false, Some("a".into())).await,
        Ok(vec![("a".into(), 3), ("a".into(), 7)])
    );
    assert_eq!(read(true, None).await, Ok(vec![]));
    let before = pic.get_stable_memory(canister).await;
    // A later oversized row must not leave the earlier intended change applied.
    let reply = write(
        vec![(1, "changed".into(), true), (2, "é".repeat(17), false)],
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        native::decode_response::<Result<(), u8>>(23, &reply, LIMITS).unwrap(),
        Err(1)
    );
    assert_eq!(read(false, Some("é".repeat(17))).await, Err(1));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let change = vec![(3, "a\0b".into(), true)];
    assert!(write(change.clone(), true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let reply = write(change, false).await.unwrap();
    assert_eq!(
        native::decode_response::<Result<(), u8>>(23, &reply, LIMITS).unwrap(),
        Ok(())
    );
    expected.retain(|(_, id)| *id != 3);
    assert_eq!(read(false, None).await, Ok(expected.clone()));
    assert_eq!(
        read(true, Some("a\0b".into())).await,
        Ok(vec![("a\0b".into(), 3)])
    );
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(read(false, None).await, Ok(expected));
    assert_eq!(
        read(false, Some("a".into())).await,
        Ok(vec![("a".into(), 7)])
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.close().await;
}

#[tokio::test]
async fn optional_membership_and_unique_transfer_survive_wasm_rollback() {
    use ic_wire::ByteBuf;
    type Writes = Vec<(u64, Option<ByteBuf>, Option<u64>)>;
    type Read = (Result<Vec<u64>, u8>, Vec<(Option<u64>, u64)>);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |rows: Writes, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "optional_index_batch",
            native::encode_request(25, &(rows, trap), LIMITS).unwrap(),
        )
        .await
    };
    let read = async |value: Option<ByteBuf>| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "optional_indexes",
                native::encode_request(26, &value, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Read>(26, &reply, LIMITS).unwrap()
    };
    let rows = vec![
        (1, None, None),
        (2, None, None),
        (3, Some(ByteBuf::new()), Some(0)),
        (4, Some(ByteBuf::from(vec![4])), Some(1)),
    ];
    let reply = write(rows, false).await.unwrap();
    assert_eq!(
        native::decode_response::<Result<(), u8>>(25, &reply, LIMITS).unwrap(),
        Ok(())
    );
    let order = vec![(None, 1), (None, 2), (Some(0), 3), (Some(1), 4)];
    assert_eq!(read(None).await, (Err(2), order.clone()));
    assert_eq!(
        read(Some(ByteBuf::new())).await,
        (Ok(vec![3]), order.clone())
    );
    let before = pic.get_stable_memory(canister).await;
    for rows in [
        vec![(5, Some(ByteBuf::new()), None)],
        vec![
            (5, Some(ByteBuf::from(vec![9])), None),
            (6, Some(ByteBuf::from(vec![9])), None),
        ],
    ] {
        let reply = write(rows, false).await.unwrap();
        assert_eq!(
            native::decode_response::<Result<(), u8>>(25, &reply, LIMITS).unwrap(),
            Err(3)
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    let swap = vec![
        (3, Some(ByteBuf::from(vec![4])), Some(1)),
        (4, Some(ByteBuf::new()), Some(0)),
    ];
    let reply = write(swap, false).await.unwrap();
    assert_eq!(
        native::decode_response::<Result<(), u8>>(25, &reply, LIMITS).unwrap(),
        Ok(())
    );
    assert_eq!(read(Some(ByteBuf::new())).await.0, Ok(vec![4]));
    let before = pic.get_stable_memory(canister).await;
    let transfer = vec![(4, None, None), (5, Some(ByteBuf::new()), Some(0))];
    assert!(write(transfer.clone(), true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let reply = write(transfer, false).await.unwrap();
    assert_eq!(
        native::decode_response::<Result<(), u8>>(25, &reply, LIMITS).unwrap(),
        Ok(())
    );
    let expected = (
        Ok(vec![5]),
        vec![(None, 1), (None, 2), (None, 4), (Some(0), 5), (Some(1), 3)],
    );
    assert_eq!(read(Some(ByteBuf::new())).await, expected);
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(read(Some(ByteBuf::new())).await, expected);
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.close().await;
}

#[tokio::test]
async fn optional_references_preserve_final_state_rollback_and_upgrade() {
    type Links = (Vec<(u64, Option<u64>)>, Vec<u64>, u64);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let read = async |target: Option<u64>| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "optional_references",
                native::encode_request(28, &target, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Links>(28, &reply, LIMITS).unwrap()
    };
    let write = async |rows: Vec<(u64, Option<u64>)>, deletes: Vec<u64>| {
        let reply = pic
            .update_call(
                canister,
                canister,
                "optional_reference_batch",
                native::encode_request(27, &(rows, deletes, false), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<(), u8>>(27, &reply, LIMITS).unwrap()
    };
    assert_eq!(write(vec![(1, None)], vec![]).await, Ok(()));
    assert_eq!(read(None).await, (vec![(1, None)], vec![1], 1));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(write(vec![(1, Some(0))], vec![]).await, Err(2));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(write(vec![(1, Some(0)), (0, None)], vec![]).await, Ok(()));
    assert_eq!(
        read(Some(0)).await,
        (vec![(0, None), (1, Some(0))], vec![1], 2)
    );
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(write(vec![], vec![0]).await, Err(2));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    // A late application trap must restore the row, both index memberships and revision.
    pic.update_call(
        canister,
        canister,
        "optional_reference_batch",
        native::encode_request(27, &(vec![(1_u64, None::<u64>)], vec![0_u64], true), LIMITS)
            .unwrap(),
    )
    .await
    .expect_err("successful unlink/delete must roll back with the message");
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        read(Some(0)).await,
        (vec![(0, None), (1, Some(0))], vec![1], 2)
    );
    assert_eq!(write(vec![(1, None)], vec![0]).await, Ok(()));
    assert_eq!(read(Some(0)).await, (vec![(1, None)], vec![], 3));
    assert_eq!(read(None).await, (vec![(1, None)], vec![1], 3));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(write(vec![(1, None)], vec![]).await, Ok(()));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        write(vec![(u64::MAX, Some(u64::MAX))], vec![]).await,
        Ok(())
    );
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        read(Some(u64::MAX)).await,
        (
            vec![(1, None), (u64::MAX, Some(u64::MAX))],
            vec![u64::MAX],
            4
        )
    );
    assert_eq!(write(vec![], vec![u64::MAX]).await, Ok(()));
    owned.close().await;
}

#[tokio::test]
async fn named_enums_preserve_ordinals_indexes_and_message_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |mode: u8, state: u8, previous: Option<u8>| {
        pic.update_call(
            canister,
            canister,
            "enum_write",
            native::encode_request(29, &(mode, state, previous), LIMITS).unwrap(),
        )
        .await
    };
    let decode = |reply: &[u8]| {
        native::decode_response::<Result<(Vec<u8>, u64), u8>>(29, reply, LIMITS).unwrap()
    };
    let golden = vec![1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2];
    assert_eq!(
        decode(&write(1, 2, Some(1)).await.unwrap()),
        Ok((golden.clone(), 1))
    );
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        decode(&write(2, 2, Some(1)).await.unwrap()),
        Ok((golden.clone(), 1))
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(decode(&write(2, 255, None).await.unwrap()), Err(1));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(write(3, 0, None).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for (bytes, valid) in [
        (golden.clone(), true),
        (vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0], true),
        (vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 3], false),
        (vec![1, 0, 0, 0, 0, 0, 0, 0, 1], false),
        (vec![1, 0, 0, 0, 0, 0, 0, 0, 2, 0], false),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "enum_decode",
                native::encode_request(30, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(30, &reply, LIMITS).unwrap(),
            valid
        );
    }
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(decode(&write(0, 0, None).await.unwrap()), Ok((golden, 1)));
    assert_eq!(
        decode(&write(2, 0, None).await.unwrap()),
        Ok((vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0], 2))
    );
    owned.close().await;
}

#[tokio::test]
async fn nested_records_payloads_and_shared_bounds_survive_wasm_reopen() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |mode: u8, shape: u8, size: u8| {
        pic.update_call(
            canister,
            canister,
            "nested_write",
            native::encode_request(31, &(mode, shape, size), LIMITS).unwrap(),
        )
        .await
    };
    let decode = |reply: &[u8]| {
        native::decode_response::<Result<(Vec<u8>, u64), u8>>(31, reply, LIMITS).unwrap()
    };
    let golden = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, b'x', 0x34, 0x12];
    assert_eq!(
        decode(&write(1, 0, 1).await.unwrap()),
        Ok((golden.clone(), 1))
    );
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        decode(&write(2, 0, 1).await.unwrap()),
        Ok((golden.clone(), 1))
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for (shape, size) in [(0, 9), (1, 32), (2, 7)] {
        assert_eq!(decode(&write(2, shape, size).await.unwrap()), Err(1));
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    assert!(write(3, 1, 31).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let mut deep = vec![1, 0, 0, 0, 0, 0, 0, 0, 0];
    deep.extend([2; 32]);
    deep.push(1);
    let mut invalid = golden.clone();
    invalid[14] = 255;
    for (bytes, valid) in [(golden.clone(), true), (deep, false), (invalid, false)] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "nested_decodes",
                native::encode_request(32, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(32, &reply, LIMITS).unwrap(),
            valid
        );
    }
    assert!(decode(&write(2, 2, 6).await.unwrap()).is_ok());
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let (_, revision) = decode(&write(0, 0, 0).await.unwrap()).unwrap();
    assert_eq!(revision, 2);
    owned.close().await;
}

#[tokio::test]
async fn ordered_lists_preserve_bounds_and_message_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |mode: u8, shape: u8, size: u8| {
        pic.update_call(
            canister,
            canister,
            "list_write",
            native::encode_request(33, &(mode, shape, size), LIMITS).unwrap(),
        )
        .await
    };
    let decode = |reply: &[u8]| {
        native::decode_response::<Result<(Vec<u8>, u64), u8>>(33, reply, LIMITS).unwrap()
    };
    let golden = vec![
        1, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 2, 1, 2, 1, 2, 1, 0, 0, 0, 0,
    ];
    assert_eq!(
        decode(&write(1, 0, 3).await.unwrap()),
        Ok((golden.clone(), 1))
    );
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        decode(&write(2, 0, 3).await.unwrap()),
        Ok((golden.clone(), 1))
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for (shape, size) in [(0, 4), (1, 15), (2, 5), (3, 7)] {
        assert_eq!(decode(&write(2, shape, size).await.unwrap()), Err(1));
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    assert!(write(3, 1, 14).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let mut huge = golden.clone();
    huge[9..13].copy_from_slice(&u32::MAX.to_le_bytes());
    let invalid = vec![
        1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    for (bytes, valid) in [
        (golden.clone(), true),
        (huge, false),
        (invalid, false),
        (golden[..18].to_vec(), false),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "list_decodes",
                native::encode_request(34, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(34, &reply, LIMITS).unwrap(),
            valid
        );
    }
    assert!(decode(&write(2, 3, 6).await.unwrap()).is_ok());
    let saved = decode(&write(0, 0, 0).await.unwrap());
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(decode(&write(0, 0, 0).await.unwrap()), saved);
    owned.close().await;
}

#[tokio::test]
async fn canonical_sets_preserve_order_and_message_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |mode: u8, reverse: bool, size: u8| {
        pic.update_call(
            canister,
            canister,
            "set_write",
            native::encode_request(35, &(mode, reverse, size), LIMITS).unwrap(),
        )
        .await
    };
    let decode = |reply: &[u8]| {
        native::decode_response::<Result<(Vec<u8>, u64), u8>>(35, reply, LIMITS).unwrap()
    };
    let golden = vec![1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 255, 255, 0, 0, 0, 1];
    assert_eq!(
        decode(&write(1, true, 3).await.unwrap()),
        Ok((golden.clone(), 1))
    );
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        decode(&write(2, false, 3).await.unwrap()),
        Ok((golden.clone(), 1))
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(decode(&write(2, false, 4).await.unwrap()), Err(1));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(write(3, false, 2).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let mut duplicate = golden.clone();
    duplicate[14..16].copy_from_slice(&[255, 255]);
    let mut descending = golden.clone();
    descending[12..16].copy_from_slice(&[0, 0, 255, 255]);
    let mut huge = golden.clone();
    huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    for (bytes, valid) in [
        (golden.clone(), true),
        (duplicate, false),
        (descending, false),
        (huge, false),
        (golden[..17].to_vec(), false),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "set_decodes",
                native::encode_request(36, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(36, &reply, LIMITS).unwrap(),
            valid
        );
    }
    assert_eq!(decode(&write(2, false, 0).await.unwrap()).unwrap().1, 2);
    let saved = decode(&write(0, false, 0).await.unwrap());
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(decode(&write(0, false, 0).await.unwrap()), saved);
    owned.close().await;
}

#[tokio::test]
async fn canonical_maps_preserve_entries_and_message_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |mode: u8, shape: u8| {
        pic.update_call(
            canister,
            canister,
            "map_write",
            native::encode_request(37, &(mode, shape), LIMITS).unwrap(),
        )
        .await
    };
    let decode = |reply: &[u8]| {
        native::decode_response::<Result<(Vec<u8>, u64), u8>>(37, reply, LIMITS).unwrap()
    };
    let golden = vec![
        1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 255, 255, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 2, 0, 0, 0,
        195, 169,
    ];
    assert_eq!(decode(&write(1, 0).await.unwrap()), Ok((golden.clone(), 1)));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(decode(&write(2, 1).await.unwrap()), Ok((golden.clone(), 1)));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for shape in [2, 3] {
        assert_eq!(decode(&write(2, shape).await.unwrap()), Err(1));
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    assert!(write(3, 4).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let mut duplicate = golden.clone();
    duplicate[15..17].copy_from_slice(&[255, 255]);
    let mut descending = golden.clone();
    descending[12..14].copy_from_slice(&[0, 0]);
    descending[15..17].copy_from_slice(&[255, 255]);
    let mut huge = golden.clone();
    huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut invalid = golden.clone();
    invalid[30] = 255;
    for (bytes, valid) in [
        (golden.clone(), true),
        (duplicate, false),
        (descending, false),
        (huge, false),
        (invalid, false),
        (golden[..30].to_vec(), false),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "map_decodes",
                native::encode_request(38, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(38, &reply, LIMITS).unwrap(),
            valid
        );
    }
    assert_eq!(decode(&write(2, 4).await.unwrap()).unwrap().1, 2);
    let saved = decode(&write(0, 0).await.unwrap());
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(decode(&write(0, 0).await.unwrap()), saved);
    for (star, size, valid) in [
        (false, 31_u16, true),
        (false, 32, false),
        (true, 255, true),
        (true, 256, false),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "map_tree_encode",
                native::encode_request(39, &(star, size), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        let result = native::decode_response::<Result<Vec<u8>, u8>>(39, &reply, LIMITS).unwrap();
        assert_eq!(result.is_ok(), valid);
        let mut bytes = 1_u64.to_le_bytes().to_vec();
        if star {
            bytes.extend(u32::from(size).to_le_bytes());
            for key in 0..size {
                bytes.extend(key.to_le_bytes());
                bytes.extend([0; 4]);
            }
        } else {
            for _ in 0..size {
                bytes.extend([1, 0, 0, 0, 0, 0]);
            }
            bytes.extend([0; 4]);
        }
        if valid {
            assert_eq!(result.unwrap(), bytes);
        }
        let reply = pic
            .query_call(
                canister,
                canister,
                "map_tree_decodes",
                native::encode_request(40, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(40, &reply, LIMITS).unwrap(),
            valid
        );
    }
    owned.close().await;
}

#[tokio::test]
async fn u256_exact_order_uniqueness_and_message_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    type Row = (u64, Option<[u8; 32]>, [u8; 32]);
    let write = async |rows: Vec<Row>, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "u256_batch",
            native::encode_request(41, &(rows, trap), LIMITS).unwrap(),
        )
        .await
    };
    let result =
        |reply: &[u8]| native::decode_response::<Result<(), u8>>(41, reply, LIMITS).unwrap();
    let read = async || {
        let reply = pic
            .query_call(
                canister,
                canister,
                "u256_rows",
                native::encode_request(42, &(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<(u64, Vec<u8>)>, u64)>(42, &reply, LIMITS).unwrap()
    };
    // Independent byte vectors: zero, one, 2^128-1, 2^128, 2^255, maximum.
    let zero = [0; 32];
    let mut one = zero;
    one[31] = 1;
    let mut low = zero;
    low[16..].fill(255);
    let mut high = zero;
    high[15] = 1;
    let mut top = zero;
    top[0] = 128;
    let maximum = [255; 32];
    let mut rows: Vec<Row> = [zero, one, low, high, top, maximum]
        .into_iter()
        .enumerate()
        .map(|(n, v)| (n as u64 + 1, Some(v), v))
        .collect();
    rows.push((7, None, high));
    let expected = |rows: &[Row]| {
        let mut rows = rows.to_vec();
        rows.sort_by_key(|(id, _, value)| (*value, *id));
        rows.into_iter()
            .map(|(id, maybe, value)| {
                let mut bytes = id.to_le_bytes().to_vec();
                bytes.push(u8::from(maybe.is_some()));
                if let Some(maybe) = maybe {
                    bytes.extend(maybe);
                }
                bytes.extend(value);
                bytes.push(1);
                (id, bytes)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(result(&write(rows.clone(), false).await.unwrap()), Ok(()));
    assert_eq!(read().await, (expected(&rows), 1));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(result(&write(rows.clone(), false).await.unwrap()), Ok(()));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        result(&write(vec![(1, Some(maximum), zero)], false).await.unwrap()),
        Err(2)
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let mut swapped = rows.clone();
    swapped[0].1 = Some(maximum);
    swapped[5].1 = Some(zero);
    assert!(write(vec![swapped[0], swapped[5]], true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (expected(&rows), 1));
    assert_eq!(
        result(&write(vec![swapped[0], swapped[5]], false).await.unwrap()),
        Ok(())
    );
    assert_eq!(read().await, (expected(&swapped), 2));
    let reply = pic
        .query_call(
            canister,
            canister,
            "u256_lookup",
            native::encode_request(43, &high, LIMITS).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        native::decode_response::<Vec<u64>>(43, &reply, LIMITS).unwrap(),
        vec![4, 7]
    );
    let golden = expected(&rows)
        .into_iter()
        .find(|(id, _)| *id == 7)
        .unwrap()
        .1;
    let mut truncated = golden.clone();
    truncated.remove(9);
    let mut surplus = golden.clone();
    surplus.insert(9, 0);
    let mut invalid = golden.clone();
    invalid[8] = 2;
    for (bytes, valid) in [
        (golden, true),
        (truncated, false),
        (surplus, false),
        (invalid, false),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "u256_decodes",
                native::encode_request(44, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(44, &reply, LIMITS).unwrap(),
            valid
        );
    }
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (expected(&swapped), 2));
    owned.close().await;
}

#[tokio::test]
async fn finite_floats_preserve_domain_zero_uniqueness_and_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    type Row = (u64, Option<u64>, u32, u64);
    let write = async |rows: Vec<Row>, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "float_batch",
            native::encode_request(45, &(rows, trap), LIMITS).unwrap(),
        )
        .await
    };
    let result =
        |reply: &[u8]| native::decode_response::<Result<(), u8>>(45, reply, LIMITS).unwrap();
    let read = async |small: bool| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "float_rows",
                native::encode_request(46, &small, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<(u64, Vec<u8>)>, u64)>(46, &reply, LIMITS).unwrap()
    };
    // Literal IEEE patterns: finite extremes, +/-1, normals, subnormals and zero.
    let small = [
        0xff7fffff_u32,
        0xbf800000,
        0x80800000,
        0x80000001,
        0,
        1,
        0x00800000,
        0x3f800000,
        0x7f7fffff,
    ];
    let wide = [
        0xffefffffffffffff_u64,
        0xbff0000000000000,
        0x8010000000000000,
        0x8000000000000001,
        0,
        1,
        0x0010000000000000,
        0x3ff0000000000000,
        0x7fefffffffffffff,
    ];
    let rows: Vec<Row> = small
        .into_iter()
        .zip(wide)
        .enumerate()
        .map(|(n, (a, b))| (n as u64 + 1, Some(b), a, b))
        .collect();
    let expected = |rows: &[Row]| {
        rows.iter()
            .map(|&(id, maybe, small, value)| {
                let mut bytes = id.to_le_bytes().to_vec();
                bytes.push(u8::from(maybe.is_some()));
                if let Some(bits) = maybe {
                    bytes.extend(bits.to_le_bytes());
                }
                bytes.extend(small.to_le_bytes());
                bytes.extend(value.to_le_bytes());
                bytes.push(1);
                (id, bytes)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        result(
            &write(rows.iter().rev().copied().collect(), false)
                .await
                .unwrap()
        ),
        Ok(())
    );
    for small in [false, true] {
        assert_eq!(read(small).await, (expected(&rows), 1));
    }
    let before = pic.get_stable_memory(canister).await;
    let negative_zero = (5, Some(0x8000000000000000), 0x80000000, 0x8000000000000000);
    assert_eq!(
        result(&write(vec![negative_zero], false).await.unwrap()),
        Ok(())
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let duplicate = (10, negative_zero.1, negative_zero.2, negative_zero.3);
    assert_eq!(
        result(&write(vec![duplicate], false).await.unwrap()),
        Err(2)
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    // Refusal after an earlier row was prepared still performs no writes.
    for bits in [
        0x7ff0000000000000,
        0xfff0000000000000,
        0x7ff8000000000000,
        0x7ff0000000000001,
    ] {
        assert_eq!(
            result(
                &write(vec![(11, None, 0, 0), (12, None, 0, bits)], false)
                    .await
                    .unwrap()
            ),
            Err(1)
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert_eq!(
            result(&write(vec![(12, Some(bits), 0, 0)], false).await.unwrap()),
            Err(1)
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    for bits in [0x7f800000, 0xff800000, 0x7fc00000, 0x7f800001] {
        assert_eq!(
            result(&write(vec![(11, None, bits, 0)], false).await.unwrap()),
            Err(1)
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    let mut swapped = rows.clone();
    swapped[4].1 = Some(wide[8]);
    swapped[8].1 = Some(0);
    assert!(write(vec![swapped[4], swapped[8]], true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        result(&write(vec![swapped[4], swapped[8]], false).await.unwrap()),
        Ok(())
    );
    for bits in [0_u64, 0x8000000000000000] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "float_lookup",
                native::encode_request(47, &bits, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<Result<Vec<u64>, u8>>(47, &reply, LIMITS).unwrap(),
            Ok(vec![9])
        );
    }
    let golden = vec![
        7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0x3f, 0, 0, 0, 0, 0, 0, 2, 0xc0, 1,
    ];
    let mut cases = vec![(golden.clone(), true), (golden[..21].to_vec(), false)];
    for bits in [
        0x80000000_u32,
        0x7f800000,
        0xff800000,
        0x7fc00000,
        0x7f800001,
    ] {
        let mut bytes = golden.clone();
        bytes[9..13].copy_from_slice(&bits.to_le_bytes());
        cases.push((bytes, false));
    }
    for bits in [
        0x8000000000000000_u64,
        0x7ff0000000000000,
        0xfff0000000000000,
        0x7ff8000000000000,
        0x7ff0000000000001,
    ] {
        let mut bytes = golden.clone();
        bytes[13..21].copy_from_slice(&bits.to_le_bytes());
        cases.push((bytes, false));
    }
    let mut surplus = golden;
    surplus.push(0);
    cases.push((surplus, false));
    for (bytes, valid) in cases {
        let reply = pic
            .query_call(
                canister,
                canister,
                "float_decodes",
                native::encode_request(48, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(48, &reply, LIMITS).unwrap(),
            valid
        );
    }
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for small in [false, true] {
        assert_eq!(read(small).await, (expected(&swapped), 2));
    }
    owned.close().await;
}

#[tokio::test]
async fn accounts_preserve_default_identity_order_and_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    type AccountInput = (Vec<u8>, Option<[u8; 32]>);
    type Row = (u64, Option<AccountInput>, [u8; 32], AccountInput);
    let write = async |rows: Vec<Row>, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "account_batch",
            native::encode_request(49, &(rows, trap), LIMITS).unwrap(),
        )
        .await
    };
    let result = |r: &[u8]| native::decode_response::<Result<(), u8>>(49, r, LIMITS).unwrap();
    let read = async |sub: bool| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "account_rows",
                native::encode_request(50, &sub, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<(u64, Vec<u8>)>, u64)>(50, &reply, LIMITS).unwrap()
    };
    let values: Vec<AccountInput> = vec![
        (vec![], None),
        (vec![], Some([255; 32])),
        (vec![4], None),
        (vec![4], Some([1; 32])),
        (vec![4], Some([255; 32])),
        (vec![0, 0], None),
        (vec![0, 0], Some([1; 32])),
        (vec![255; 29], Some([255; 32])),
    ];
    let rows: Vec<Row> = values
        .into_iter()
        .enumerate()
        .map(|(n, value)| (n as u64 + 1, Some(value.clone()), [n as u8; 32], value))
        .collect();
    // Independent row oracle: u32 owner length, owner bytes, effective 32-byte subaccount.
    let account_bytes = |value: &AccountInput| {
        let mut bytes = (value.0.len() as u32).to_le_bytes().to_vec();
        bytes.extend(&value.0);
        bytes.extend(value.1.unwrap_or([0; 32]));
        bytes
    };
    let expected = |rows: &[Row]| {
        rows.iter()
            .map(|(id, maybe, sub, value)| {
                let mut bytes = id.to_le_bytes().to_vec();
                bytes.push(u8::from(maybe.is_some()));
                if let Some(maybe) = maybe {
                    bytes.extend(account_bytes(maybe));
                }
                bytes.extend(sub);
                bytes.extend(account_bytes(value));
                bytes.push(1);
                (*id, bytes)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        result(
            &write(rows.iter().rev().cloned().collect(), false)
                .await
                .unwrap()
        ),
        Ok(())
    );
    for sub in [false, true] {
        assert_eq!(read(sub).await, (expected(&rows), 1));
    }
    let before = pic.get_stable_memory(canister).await;
    let zero = (vec![4], Some([0; 32]));
    let mut same = rows[2].clone();
    same.1 = Some(zero.clone());
    same.3 = zero.clone();
    assert_eq!(
        result(&write(vec![same.clone()], false).await.unwrap()),
        Ok(())
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    same.0 = 9;
    assert_eq!(result(&write(vec![same], false).await.unwrap()), Err(2));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for size in [30, 100] {
        let bad = (vec![255; size], None);
        assert_eq!(
            result(
                &write(
                    vec![(9, None, [0; 32], zero.clone()), (10, None, [0; 32], bad)],
                    false
                )
                .await
                .unwrap()
            ),
            Err(1)
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    let mut swapped = rows.clone();
    let first = swapped[2].1.clone();
    swapped[2].1 = swapped[7].1.clone();
    swapped[7].1 = first;
    assert!(
        write(vec![swapped[2].clone(), swapped[7].clone()], true)
            .await
            .is_err()
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        result(
            &write(vec![swapped[2].clone(), swapped[7].clone()], false)
                .await
                .unwrap()
        ),
        Ok(())
    );
    for value in [(vec![4], None), zero] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "account_lookup",
                native::encode_request(51, &value, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<Result<Vec<u64>, u8>>(51, &reply, LIMITS).unwrap(),
            Ok(vec![8])
        );
    }
    let mut golden = vec![7, 0, 0, 0, 0, 0, 0, 0, 0];
    golden.extend(1_u8..=32);
    golden.extend([1, 0, 0, 0, 4]);
    golden.extend([0; 32]);
    golden.push(1);
    let mut bad_owner = golden.clone();
    bad_owner[41..45].copy_from_slice(&30_u32.to_le_bytes());
    let mut bad_tag = golden.clone();
    bad_tag[8] = 2;
    let mut short = golden.clone();
    short.remove(9);
    let mut excess = golden.clone();
    excess.push(0);
    for (bytes, valid) in [
        (golden, true),
        (bad_owner, false),
        (bad_tag, false),
        (short, false),
        (excess, false),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "account_decodes",
                native::encode_request(52, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(52, &reply, LIMITS).unwrap(),
            valid
        );
    }
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for sub in [false, true] {
        assert_eq!(read(sub).await, (expected(&swapped), 2));
    }
    owned.close().await;
}

#[tokio::test]
async fn ulid_exact_order_uniqueness_and_message_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    for (text, accepted) in [
        ("01ARZ3NDEKTSV4RRFFQ69G5FAV", true),
        ("01arz3ndektsv4rrffq69g5fav", false),
        ("80000000000000000000000000", false),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "ulid_text",
                native::encode_request(57, &text.to_string(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        let result =
            native::decode_response::<Result<(String, [u8; 16]), u8>>(57, &reply, LIMITS).unwrap();
        if accepted {
            assert_eq!(
                result,
                Ok((
                    text.into(),
                    [
                        1, 86, 62, 58, 181, 211, 214, 118, 76, 97, 239, 185, 147, 2, 189, 91
                    ]
                ))
            );
        } else {
            assert_eq!(result, Err(1));
        }
    }
    type Row = (u64, Option<[u8; 16]>, [u8; 16]);
    let write = async |rows: Vec<Row>, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "ulid_batch",
            native::encode_request(53, &(rows, trap), LIMITS).unwrap(),
        )
        .await
    };
    let result =
        |reply: &[u8]| native::decode_response::<Result<(), u8>>(53, reply, LIMITS).unwrap();
    let read = async || {
        let reply = pic
            .query_call(
                canister,
                canister,
                "ulid_rows",
                native::encode_request(54, &(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<(u64, Vec<u8>)>, u64)>(54, &reply, LIMITS).unwrap()
    };
    // Independent byte vectors: zero, one, 2^80-1, 2^80, 2^127, maximum.
    let zero = [0; 16];
    let mut one = zero;
    one[15] = 1;
    let mut low = zero;
    low[6..].fill(255);
    let mut high = zero;
    high[5] = 1;
    let mut top = zero;
    top[0] = 128;
    let maximum = [255; 16];
    let mut rows: Vec<Row> = [zero, one, low, high, top, maximum]
        .into_iter()
        .enumerate()
        .map(|(n, v)| (n as u64 + 1, Some(v), v))
        .collect();
    rows.push((7, None, high));
    let expected = |rows: &[Row]| {
        let mut rows = rows.to_vec();
        rows.sort_by_key(|(id, _, value)| (*value, *id));
        rows.into_iter()
            .map(|(id, maybe, value)| {
                let mut bytes = id.to_le_bytes().to_vec();
                bytes.push(u8::from(maybe.is_some()));
                if let Some(maybe) = maybe {
                    bytes.extend(maybe);
                }
                bytes.extend(value);
                bytes.push(1);
                (id, bytes)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(result(&write(rows.clone(), false).await.unwrap()), Ok(()));
    assert_eq!(read().await, (expected(&rows), 1));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(result(&write(rows.clone(), false).await.unwrap()), Ok(()));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        result(&write(vec![(1, Some(maximum), zero)], false).await.unwrap()),
        Err(2)
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let mut swapped = rows.clone();
    swapped[0].1 = Some(maximum);
    swapped[5].1 = Some(zero);
    assert!(write(vec![swapped[0], swapped[5]], true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (expected(&rows), 1));
    assert_eq!(
        result(&write(vec![swapped[0], swapped[5]], false).await.unwrap()),
        Ok(())
    );
    assert_eq!(read().await, (expected(&swapped), 2));
    let reply = pic
        .query_call(
            canister,
            canister,
            "ulid_lookup",
            native::encode_request(55, &high, LIMITS).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        native::decode_response::<Vec<u64>>(55, &reply, LIMITS).unwrap(),
        vec![4, 7]
    );
    let golden = expected(&rows)
        .into_iter()
        .find(|(id, _)| *id == 7)
        .unwrap()
        .1;
    let mut truncated = golden.clone();
    truncated.remove(9);
    let mut surplus = golden.clone();
    surplus.insert(9, 0);
    let mut invalid = golden.clone();
    invalid[8] = 2;
    for (bytes, valid) in [
        (golden, true),
        (truncated, false),
        (surplus, false),
        (invalid, false),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "ulid_decodes",
                native::encode_request(56, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(56, &reply, LIMITS).unwrap(),
            valid
        );
    }
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (expected(&swapped), 2));
    owned.close().await;
}

#[tokio::test]
async fn dates_preserve_domain_order_and_message_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    type Row = (u64, Option<i32>, i32);
    let write = async |rows: Vec<Row>, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "date_batch",
            native::encode_request(58, &(rows, trap), LIMITS).unwrap(),
        )
        .await
    };
    let result =
        |reply: &[u8]| native::decode_response::<Result<(), u8>>(58, reply, LIMITS).unwrap();
    let read = async || {
        let reply = pic
            .query_call(
                canister,
                canister,
                "date_rows",
                native::encode_request(59, &(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<(u64, Vec<u8>)>, u64)>(59, &reply, LIMITS).unwrap()
    };
    let zero = -719528_i32;
    let one = -365_i32;
    let low = -1_i32;
    let high = 0_i32;
    let top = 1_i32;
    let maximum = 2932896_i32;
    let mut rows: Vec<Row> = [zero, one, low, high, top, maximum]
        .into_iter()
        .enumerate()
        .map(|(n, v)| (n as u64 + 1, Some(v), v))
        .collect();
    rows.push((7, None, high));
    let expected = |rows: &[Row]| {
        let mut rows = rows.to_vec();
        rows.sort_by_key(|(id, _, value)| (*value, *id));
        rows.into_iter()
            .map(|(id, maybe, value)| {
                let mut bytes = id.to_le_bytes().to_vec();
                bytes.push(u8::from(maybe.is_some()));
                if let Some(maybe) = maybe {
                    bytes.extend(maybe.to_le_bytes());
                }
                bytes.extend(value.to_le_bytes());
                bytes.push(1);
                (id, bytes)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(result(&write(rows.clone(), false).await.unwrap()), Ok(()));
    assert_eq!(read().await, (expected(&rows), 1));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(result(&write(rows.clone(), false).await.unwrap()), Ok(()));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        result(&write(vec![(1, Some(maximum), zero)], false).await.unwrap()),
        Err(2)
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        result(&write(vec![(1, Some(zero), 2932897)], false).await.unwrap()),
        Err(1)
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let mut swapped = rows.clone();
    swapped[0].1 = Some(maximum);
    swapped[5].1 = Some(zero);
    assert!(write(vec![swapped[0], swapped[5]], true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (expected(&rows), 1));
    assert_eq!(
        result(&write(vec![swapped[0], swapped[5]], false).await.unwrap()),
        Ok(())
    );
    assert_eq!(read().await, (expected(&swapped), 2));
    let reply = pic
        .query_call(
            canister,
            canister,
            "date_lookup",
            native::encode_request(60, &high, LIMITS).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        native::decode_response::<Result<Vec<u64>, u8>>(60, &reply, LIMITS).unwrap(),
        Ok(vec![4, 7])
    );
    let golden = expected(&rows)
        .into_iter()
        .find(|(id, _)| *id == 7)
        .unwrap()
        .1;
    let mut truncated = golden.clone();
    truncated.remove(9);
    let mut surplus = golden.clone();
    surplus.insert(9, 0);
    let mut invalid = golden.clone();
    invalid[8] = 2;
    let mut out_of_range = golden.clone();
    out_of_range[9..13].copy_from_slice(&i32::MAX.to_le_bytes());
    for (bytes, valid) in [
        (golden, true),
        (out_of_range, false),
        (truncated, false),
        (surplus, false),
        (invalid, false),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "date_decodes",
                native::encode_request(61, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(61, &reply, LIMITS).unwrap(),
            valid
        );
    }
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (expected(&swapped), 2));
    owned.close().await;
}

#[tokio::test]
async fn time_values_preserve_units_order_and_message_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    type Row = (u64, Option<u64>, u64, Option<i64>, i64);
    let write = async |rows: Vec<Row>, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "time_batch",
            native::encode_request(62, &(rows, trap), LIMITS).unwrap(),
        )
        .await
    };
    let result =
        |reply: &[u8]| native::decode_response::<Result<(), u8>>(62, reply, LIMITS).unwrap();
    let read = async |duration: bool| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "time_rows",
                native::encode_request(63, &duration, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<(u64, Vec<u8>)>, u64)>(63, &reply, LIMITS).unwrap()
    };
    let mut rows: Vec<Row> = [
        (0, i64::MIN),
        (1, -1),
        (i64::MAX as u64, 0),
        (1_u64 << 63, 1),
        (u64::MAX, i64::MAX),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, (d, t))| (n as u64 + 1, Some(d), d, Some(t), t))
    .collect();
    rows.push((6, None, 0, None, 0));
    let expected = |rows: &[Row], duration: bool| {
        let mut rows = rows.to_vec();
        if duration {
            rows.sort_by_key(|r| (r.2, r.0));
        } else {
            rows.sort_by_key(|r| (r.4, r.0));
        }
        rows.into_iter()
            .map(|(id, delay, duration, maybe, timestamp)| {
                let mut bytes = vec![u8::from(delay.is_some())];
                if let Some(d) = delay {
                    bytes.extend(d.to_le_bytes());
                }
                bytes.extend(duration.to_le_bytes());
                bytes.extend(id.to_le_bytes());
                bytes.push(u8::from(maybe.is_some()));
                if let Some(t) = maybe {
                    bytes.extend(t.to_le_bytes());
                }
                bytes.extend(timestamp.to_le_bytes());
                bytes.push(1);
                (id, bytes)
            })
            .collect::<Vec<_>>()
    };
    let mut reversed = rows.clone();
    reversed.reverse();
    assert_eq!(result(&write(reversed, false).await.unwrap()), Ok(()));
    for duration in [false, true] {
        assert_eq!(read(duration).await, (expected(&rows, duration), 1));
    }
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(result(&write(rows.clone(), false).await.unwrap()), Ok(()));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for duplicate in [
        (1, Some(u64::MAX), 0, Some(i64::MIN), i64::MIN),
        (1, Some(0), 0, Some(i64::MAX), i64::MIN),
    ] {
        assert_eq!(
            result(&write(vec![duplicate], false).await.unwrap()),
            Err(2)
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    let mut swapped = rows.clone();
    swapped[0].1 = Some(u64::MAX);
    swapped[0].3 = Some(i64::MAX);
    swapped[4].1 = Some(0);
    swapped[4].3 = Some(i64::MIN);
    assert!(write(vec![swapped[0], swapped[4]], true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read(false).await, (expected(&rows, false), 1));
    assert_eq!(
        result(&write(vec![swapped[0], swapped[4]], false).await.unwrap()),
        Ok(())
    );
    for duration in [false, true] {
        assert_eq!(read(duration).await, (expected(&swapped, duration), 2));
    }
    let golden = expected(&rows, false)
        .into_iter()
        .find(|r| r.0 == 6)
        .unwrap()
        .1;
    let mut truncated = golden.clone();
    truncated.pop();
    let mut excess = golden.clone();
    excess.push(0);
    let mut bad = golden.clone();
    bad[0] = 2;
    for (bytes, valid) in [
        (golden, true),
        (truncated, false),
        (excess, false),
        (bad, false),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "time_decodes",
                native::encode_request(64, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(64, &reply, LIMITS).unwrap(),
            valid
        );
    }
    for (d, t, expected) in [
        ("2s", "1969-12-31T23:59:59.999999999Z", Ok((2000, -1))),
        (
            "18446744073709551615ms",
            "-9223372036854775808",
            Ok((u64::MAX, i64::MIN)),
        ),
        ("18446744073709552s", "0", Err(1)),
        ("1ms", "1900-02-29T00:00:00Z", Err(2)),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "time_text",
                native::encode_request(65, &(d.to_string(), t.to_string()), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<Result<(u64, i64), u8>>(65, &reply, LIMITS).unwrap(),
            expected
        );
    }
    let reply = pic
        .query_call(
            canister,
            canister,
            "time_math",
            native::encode_request(66, &(-1001_i64, -1_i64, u64::MAX), LIMITS).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        native::decode_response::<(i64, i64, i64, i64)>(66, &reply, LIMITS).unwrap(),
        (-2, -1, i64::MAX, i64::MIN)
    );
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for duration in [false, true] {
        assert_eq!(read(duration).await, (expected(&swapped, duration), 2));
    }
    owned.close().await;
}

#[tokio::test]
async fn decimals_preserve_exact_scale_uniqueness_and_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    type Row = (u64, Option<String>, String);
    let write = async |rows: Vec<Row>, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "decimal_batch",
            native::encode_request(67, &(rows, trap), LIMITS).unwrap(),
        )
        .await
    };
    let result =
        |reply: &[u8]| native::decode_response::<Result<(), u8>>(67, reply, LIMITS).unwrap();
    let read = async || {
        let reply = pic
            .query_call(
                canister,
                canister,
                "decimal_rows",
                native::encode_request(68, &(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<(u64, Vec<u8>)>, u64)>(68, &reply, LIMITS).unwrap()
    };
    // Independent literal fixed-scale coefficients: no database Decimal parser
    // or encoder in the oracle, including both extreme signed coefficients.
    let entries = [
        ("-1701411834604692317316873037158841057.28", i128::MIN),
        ("-1.00", -100),
        ("-0.01", -1),
        ("0", 0),
        ("0.01", 1),
        ("1.0", 100),
        ("1701411834604692317316873037158841057.27", i128::MAX),
    ];
    let rows: Vec<Row> = entries
        .iter()
        .enumerate()
        .map(|(n, (s, _))| (n as u64 + 1, Some((*s).into()), (*s).into()))
        .collect();
    let expected = |swap: bool| {
        entries
            .iter()
            .enumerate()
            .map(|(n, (_, coefficient))| {
                let id = n as u64 + 1;
                let mut b = id.to_le_bytes().to_vec();
                b.push(1);
                let maybe = if swap && n == 0 {
                    i128::MAX
                } else if swap && n == 6 {
                    i128::MIN
                } else {
                    *coefficient
                };
                b.extend(maybe.to_be_bytes());
                b.push(2);
                b.extend(coefficient.to_be_bytes());
                b.extend([2, 1]);
                (id, b)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        result(
            &write(rows.iter().rev().cloned().collect(), false)
                .await
                .unwrap()
        ),
        Ok(())
    );
    assert_eq!(read().await, (expected(false), 1));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        result(
            &write(vec![(6, Some("1.000".into()), "1.00".into())], false)
                .await
                .unwrap()
        ),
        Ok(())
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for (value, error) in [
        ("1.000", 2),
        ("0.001", 1),
        ("170141183460469231731687303715884105727", 1),
    ] {
        assert_eq!(
            result(
                &write(vec![(1, Some(value.into()), rows[0].2.clone())], false)
                    .await
                    .unwrap()
            ),
            Err(error)
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    let swapped = vec![
        (1, rows[6].1.clone(), rows[0].2.clone()),
        (7, rows[0].1.clone(), rows[6].2.clone()),
    ];
    assert!(write(swapped.clone(), true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (expected(false), 1));
    assert_eq!(result(&write(swapped, false).await.unwrap()), Ok(()));
    assert_eq!(read().await, (expected(true), 2));
    let golden = expected(false)[5].1.clone();
    let mut equivalent = golden.clone();
    equivalent[9..25].copy_from_slice(&10_i128.to_be_bytes());
    equivalent[25] = 1;
    let mut bad = golden.clone();
    bad[25] = 29;
    let mut short = golden.clone();
    short.pop();
    for (bytes, valid) in [
        (golden, true),
        (equivalent, false),
        (bad, false),
        (short, false),
    ] {
        let reply = pic
            .query_call(
                canister,
                canister,
                "decimal_decodes",
                native::encode_request(69, &bytes, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<bool>(69, &reply, LIMITS).unwrap(),
            valid
        );
    }
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(read().await, (expected(true), 2));
    owned.close().await;
}

#[tokio::test]
async fn ulid_primary_keys_keep_paging_references_and_message_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    type Row = ([u8; 16], Option<u64>, u64);
    type Request = (
        Vec<Row>,
        Vec<(u64, Option<[u8; 16]>)>,
        Vec<[u8; 16]>,
        Vec<u64>,
        bool,
    );
    let write = async |request: Request| {
        pic.update_call(
            canister,
            canister,
            "key_batch",
            native::encode_request(70, &request, LIMITS).unwrap(),
        )
        .await
    };
    let result =
        |reply: &[u8]| native::decode_response::<Result<(), u8>>(70, reply, LIMITS).unwrap();
    let page = async |after: Option<[u8; 16]>, limit: u8| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "key_page",
                native::encode_request(71, &(after, limit), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<Row>, Option<[u8; 16]>, u64)>(71, &reply, LIMITS).unwrap()
    };
    let lookup = async |id: [u8; 16], value: u64| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "key_lookup",
                native::encode_request(72, &(id, value), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Option<Row>, Vec<Row>)>(72, &reply, LIMITS).unwrap()
    };
    let mut rows: Vec<Row> = [
        0,
        u64::MAX as u128,
        1_u128 << 64,
        (1_u128 << 80) - 1,
        1_u128 << 80,
        u128::MAX,
    ]
    .into_iter()
    .enumerate()
    .map(|(n, id)| (id.to_be_bytes(), (n == 0).then_some(1), n as u64 + 1))
    .collect();
    assert_eq!(
        result(
            &write((
                rows.iter().rev().copied().collect(),
                vec![(1, Some([0; 16])), (2, None)],
                vec![],
                vec![],
                false
            ))
            .await
            .unwrap()
        ),
        Ok(())
    );
    let first = page(None, 3).await;
    assert_eq!(first, (rows[..3].to_vec(), Some(rows[2].0), 1));
    let last = page(first.1, 3).await;
    assert_eq!(last, (rows[3..].to_vec(), Some([255; 16]), 1));
    assert_eq!(page(last.1, 3).await, (vec![], None, 1));
    assert_eq!(lookup([255; 16], 6).await, (Some(rows[5]), vec![rows[5]]));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        result(
            &write((vec![rows[0]], vec![], vec![], vec![], false))
                .await
                .unwrap()
        ),
        Ok(())
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for request in [
        (vec![], vec![], vec![[0; 16]], vec![], false),
        (vec![], vec![], vec![], vec![1], false),
        (
            vec![],
            vec![(3, Some(7_u128.to_be_bytes()))],
            vec![],
            vec![],
            false,
        ),
        (vec![([0; 16], Some(1), 6)], vec![], vec![], vec![], false),
        (vec![rows[0]], vec![], vec![[0; 16]], vec![], false),
    ] {
        assert_eq!(result(&write(request).await.unwrap()), Err(2));
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    rows[0].2 = 6;
    rows[5].2 = 1;
    assert!(
        write((vec![rows[0], rows[5]], vec![], vec![], vec![], true))
            .await
            .is_err()
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        result(
            &write((vec![rows[0], rows[5]], vec![], vec![], vec![], false))
                .await
                .unwrap()
        ),
        Ok(())
    );
    assert_eq!(lookup([255; 16], 1).await, (Some(rows[5]), vec![rows[5]]));
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(page(None, 32).await, (rows.clone(), None, 2));
    assert!(
        write((vec![], vec![], vec![[0; 16]], vec![1], true))
            .await
            .is_err()
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        result(
            &write((vec![], vec![], vec![[0; 16]], vec![1], false))
                .await
                .unwrap()
        ),
        Ok(())
    );
    assert_eq!(page(None, 32).await, (rows[1..].to_vec(), None, 3));
    assert_eq!(lookup([0; 16], 6).await, (None, vec![]));
    owned.close().await;
}

#[tokio::test]
async fn integer_primary_keys_preserve_signed_order_full_width_and_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    type Signed = (i128, Option<[u8; 32]>, u64);
    type Wide = ([u8; 32], Option<i128>, u64);
    type Request = (Vec<Signed>, Vec<Wide>, Vec<i128>, Vec<[u8; 32]>, bool);
    let write = async |request: Request| {
        pic.update_call(
            canister,
            canister,
            "integer_key_batch",
            native::encode_request(73, &request, LIMITS).unwrap(),
        )
        .await
    };
    let result =
        |bytes: &[u8]| native::decode_response::<Result<(), u8>>(73, bytes, LIMITS).unwrap();
    let signed_page = async |after: Option<i128>, limit: u8| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "signed_key_page",
                native::encode_request(74, &(after, limit), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<Signed>, Option<i128>, u64)>(74, &reply, LIMITS).unwrap()
    };
    let wide_page = async |after: Option<[u8; 32]>, limit: u8| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "wide_key_page",
                native::encode_request(75, &(after, limit), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<Wide>, Option<[u8; 32]>, u64)>(75, &reply, LIMITS).unwrap()
    };
    let lookup = async |rank: u64| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "integer_key_lookup",
                native::encode_request(76, &rank, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<Signed>, Vec<Wide>)>(76, &reply, LIMITS).unwrap()
    };
    let signed_ids = [i128::MIN, -1, 0, 1, i128::MAX];
    let mut one = [0; 32];
    one[31] = 1;
    let mut high = [0; 32];
    high[15] = 1;
    let mut penultimate = [255; 32];
    penultimate[31] = 254;
    let wide_ids = [[0; 32], one, high, penultimate, [255; 32]];
    let mut signed: Vec<Signed> = signed_ids
        .iter()
        .enumerate()
        .map(|(n, &id)| (id, Some(wide_ids[n]), n as u64))
        .collect();
    let mut wide: Vec<Wide> = wide_ids
        .iter()
        .enumerate()
        .map(|(n, &id)| (id, Some(signed_ids[n]), n as u64))
        .collect();
    let empty = pic.get_stable_memory(canister).await;
    assert_eq!(
        result(
            &write((vec![signed[0]], vec![], vec![], vec![], false))
                .await
                .unwrap()
        ),
        Err(2)
    );
    assert_eq!(pic.get_stable_memory(canister).await, empty);
    assert_eq!(
        result(
            &write((
                signed.iter().rev().copied().collect(),
                wide.iter().rev().copied().collect(),
                vec![],
                vec![],
                false
            ))
            .await
            .unwrap()
        ),
        Ok(())
    );
    let first = signed_page(None, 2).await;
    assert_eq!(first, (signed[..2].to_vec(), Some(-1), 1));
    assert_eq!(
        signed_page(first.1, 32).await,
        (signed[2..].to_vec(), None, 1)
    );
    assert_eq!(signed_page(Some(i128::MAX), 2).await, (vec![], None, 1));
    let first = wide_page(None, 3).await;
    assert_eq!(first, (wide[..3].to_vec(), Some(high), 1));
    assert_eq!(
        wide_page(first.1, 2).await,
        (wide[3..].to_vec(), Some([255; 32]), 1)
    );
    assert_eq!(wide_page(Some([255; 32]), 2).await, (vec![], None, 1));
    assert_eq!(lookup(4).await, (vec![signed[4]], vec![wide[4]]));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        result(
            &write((vec![signed[0]], vec![wide[0]], vec![], vec![], false))
                .await
                .unwrap()
        ),
        Ok(())
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for request in [
        (vec![], vec![], vec![i128::MIN], vec![], false),
        (vec![], vec![], vec![], vec![[0; 32]], false),
        (
            vec![(i128::MIN, Some([0; 32]), 4)],
            vec![],
            vec![],
            vec![],
            false,
        ),
    ] {
        assert_eq!(result(&write(request).await.unwrap()), Err(2));
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    signed[0].2 = 4;
    signed[4].2 = 0;
    wide[0].2 = 4;
    wide[4].2 = 0;
    assert!(
        write((
            vec![signed[0], signed[4]],
            vec![wide[0], wide[4]],
            vec![],
            vec![],
            true
        ))
        .await
        .is_err()
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        result(
            &write((
                vec![signed[0], signed[4]],
                vec![wide[0], wide[4]],
                vec![],
                vec![],
                false
            ))
            .await
            .unwrap()
        ),
        Ok(())
    );
    assert_eq!(lookup(0).await, (vec![signed[4]], vec![wide[4]]));
    let before = pic.get_stable_memory(canister).await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(signed_page(None, 32).await, (signed.clone(), None, 2));
    assert_eq!(wide_page(None, 32).await, (wide.clone(), None, 2));
    assert!(
        write((vec![], vec![], vec![i128::MIN], vec![[0; 32]], true))
            .await
            .is_err()
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        result(
            &write((vec![], vec![], vec![i128::MIN], vec![[0; 32]], false))
                .await
                .unwrap()
        ),
        Ok(())
    );
    assert_eq!(signed_page(None, 32).await, (signed[1..].to_vec(), None, 3));
    assert_eq!(wide_page(None, 32).await, (wide[1..].to_vec(), None, 3));
    owned.close().await;
}
