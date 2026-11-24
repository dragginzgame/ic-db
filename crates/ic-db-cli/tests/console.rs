//! Installed command admission, output and interactive queries against the local fixture.

use std::{
    io::Write,
    process::{Command, Stdio},
};

type ConsoleCase<'a> = (&'a str, &'a [&'a [&'a str]]);

fn result_table_rows(text: &str) -> Vec<Vec<&str>> {
    text.lines()
        .filter(|line| line.contains(['│', '|']))
        .map(|line| {
            line.split(['│', '|'])
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|row| {
            row.first().is_some_and(|v| {
                matches!(*v, "true" | "false" | "—")
                    || v.chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_digit() || c == '-')
            })
        })
        .collect()
}

fn run_console(binary: &std::path::Path, commands: &str) -> String {
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(commands.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn console_counts_use_entity_metadata() {
    // Admission/help must work without loading the adjacent fixture.
    for args in [
        vec!["count"],
        vec!["count", "Entry", "--category", "1"],
        vec!["count", "Entry", "--limit", "1"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_db"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_db"))
        .args(["count", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("atomic entity metadata")
    );

    let prefix = std::env::temp_dir().join(format!("ic-db-count-{}", std::process::id()));
    std::fs::create_dir(&prefix).unwrap();
    std::fs::create_dir(prefix.join("bin")).unwrap();
    std::fs::create_dir_all(prefix.join("share/ic-db/m1")).unwrap();
    let binary = prefix.join("bin/db");
    std::fs::copy(env!("CARGO_BIN_EXE_db"), &binary).unwrap();
    std::fs::copy(
        std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("build the console fixture"),
        prefix.join("share/ic-db/m1/records.wasm"),
    )
    .unwrap();
    let output = run_console(
        &binary,
        "count Entry\nquery Entry --min 0 --select count --work 1\ncount Document\ncount DecimalRow\nquery Entry --select count --category 1\ncount Entry\ncount SetRow\ncount IdentityRow\nquit\n",
    );
    assert_eq!(
        result_table_rows(&output),
        [
            vec!["6"],
            vec!["6"],
            vec!["1"],
            vec!["3"],
            vec!["3"],
            vec!["6"],
            vec!["0"],
            vec!["1"]
        ]
    );
    assert!(!output.contains("atomic entity metadata"));
    let refused = Command::new(&binary)
        .args(["count", "Unknown"])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("Unknown entity"));
    // Standalone db preserves its normal launch behavior through the prepared
    // command owner, without requiring the caller to supply an endpoint.
    let standalone = Command::new(&binary)
        .args(["count", "Entry"])
        .env_remove("IC_TESTKIT_POCKET_IC_URL")
        .env_remove("POCKET_IC_BIN")
        .output()
        .unwrap();
    assert!(
        standalone.status.success(),
        "{}",
        String::from_utf8_lossy(&standalone.stderr)
    );
    assert_eq!(
        result_table_rows(&String::from_utf8(standalone.stdout).unwrap()),
        [vec!["6"]]
    );
    std::fs::remove_dir_all(prefix).unwrap();
}

#[test]
fn console_schema_json_is_complete_selected_and_pipeable() {
    let help = Command::new(env!("CARGO_BIN_EXE_db"))
        .args(["schema", "--help"])
        .env_remove("IC_TESTKIT_POCKET_IC_URL")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--json"));

    let prefix = std::env::temp_dir().join(format!("ic-db-schema-json-{}", std::process::id()));
    std::fs::create_dir(&prefix).unwrap();
    std::fs::create_dir(prefix.join("bin")).unwrap();
    std::fs::create_dir_all(prefix.join("share/ic-db/m1")).unwrap();
    let binary = prefix.join("bin/db");
    std::fs::copy(env!("CARGO_BIN_EXE_db"), &binary).unwrap();
    std::fs::copy(
        std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("build the console fixture"),
        prefix.join("share/ic-db/m1/records.wasm"),
    )
    .unwrap();
    let inspect = |args: &[&str]| -> serde_json::Value {
        let output = Command::new(&binary).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // Parsing the entire stdout rejects banners, tables and trailing help.
        let stats = String::from_utf8_lossy(&output.stderr);
        assert_eq!(stats.lines().count(), 1, "{stats}");
        assert!(stats.starts_with("query call (schema): "), "{stats}");
        assert!(stats.ends_with("sec, 0 cycles charged\n"), "{stats}");

        serde_json::from_slice(&output.stdout).unwrap()
    };
    let all = inspect(&["schema", "--json"]);
    assert_eq!(all["source"], "local_fixture");
    let fingerprint = all["schema_fingerprint"].as_str().unwrap();
    assert_eq!(fingerprint.len(), 64);
    assert!(fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let selected = inspect(&["schema", "Document", "--json"]);
    assert_eq!(selected["schema_fingerprint"], all["schema_fingerprint"]);
    let entities = selected["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 1);
    let document = &entities[0];
    assert_eq!(document["name"], "Document");
    assert_eq!(document["primary_key"], "id");
    assert_eq!(
        all["entities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entity| entity["name"] == "Document"),
        Some(document)
    );
    let fields = document["fields"].as_array().unwrap();
    let note = fields.iter().find(|field| field["name"] == "note").unwrap();
    assert_eq!(note["type"], "Text");
    assert_eq!(note["optional"], true);
    assert_eq!(note["info"], "max 64 bytes");
    assert!(note["reference"].is_null());
    let reference = fields
        .iter()
        .find(|field| field["name"] == "entry_id")
        .unwrap();
    assert_eq!(
        reference["reference"],
        serde_json::json!({"target": "Entry.id", "index": "entry_id,score"})
    );
    assert!(
        document["indexes"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |index| index["fields"] == serde_json::json!(["entry_id", "score"])
                    && index["unique"] == false
                    && index["present_only"] == false
            )
    );

    let recursive = inspect(&["schema", "ListRow", "--json"]);
    let types = recursive["types"].as_array().unwrap();
    assert_eq!(
        types.iter().filter(|ty| ty["name"] == "ListNode").count(),
        1
    );
    assert!(!types.iter().any(|ty| ty["name"] == "Lifecycle"));

    let refused = Command::new(&binary)
        .args(["schema", "Missing", "--json"])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(refused.stdout.is_empty());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("Unknown entity"));

    let tables = Command::new(&binary)
        .args(["schema", "Document"])
        .output()
        .unwrap();
    assert!(tables.status.success());
    let table = String::from_utf8_lossy(&tables.stdout);
    assert!(table.contains("Schema: 1 entity"));
    assert!(table.contains("Reference index"));
    assert!(
        table
            .lines()
            .any(|line| line.contains("Entry.id") && line.contains("(entry_id, score)"))
    );
    std::fs::remove_dir_all(prefix).unwrap();
}

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
        .write_all(b"\n\nschema\nstatus\nquery Entry --limit 32\nquery Entry --select category,distinct-count:category\nquery Entry --select category,distinct-count:amount --group-min 3\nquery Document --select entry_id,distinct-count:note\nquery Entry --select avg:amount\nquery Entry --select category,avg:amount --group-order desc\nquery DecimalRow --select avg:value\nquery DecimalRow --select zone,avg:maybe\nquery DecimalRow\nquery DecimalRow --select sum:value\nquery DecimalRow --select zone,sum:value --group-min 3\nquery DecimalRow --select zone,sum:maybe\nquery Entry --select count\nquery Entry --select sum:amount --work 6\nquery Entry --select category,sum:amount --work 6 --group-order desc\nquery Entry --select category,sum:amount --group-min 100 --limit 1\nquery Document --select entry_id,sum:score\nquery Document --select sum:priority\nquery Document --select sum:score\nquery Entry --select min:amount --limit 1\nquery Entry --select category,min:amount\nquery Entry --select category,max:amount\nquery Entry --select category,min:amount --group-order asc --limit 1\nquery Entry --select category,count --min 40 --group-min 2 --group-order desc\nquery Document --select entry_id,min:note\nquery Document --select entry_id,max:priority\nquery Entry --select max:amount --limit 1\nquery Document --select min:note\nquery Document --select min:priority\nquery Entry --select distinct-count:category --limit 1\nquery Document --select count:note\nquery Entry --select category,count:amount --min 40\nquery Entry --select category,count --min 40\nquery Entry --order-by category --select category --distinct\nquery --min 100 --work 3\nquery Entry --min 40 --limit 2 --select id,amount\nquit\n")
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
    assert!(stdout.contains("ICDBMF02"));
    assert!(stdout.contains("Exact match"));
    assert!(stdout.contains("Membership"));
    assert!(stdout.contains("Enum: Lifecycle"));
    assert!(stdout.contains("state,id"));
    assert!(stdout.contains("Record: Details"));
    assert!(stdout.contains("List: Samples"));
    assert!(stdout.contains("Newtype: Sample"));
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
    assert!(
        stdout
            .lines()
            .any(|line| line.contains("WideKey.id") && line.contains("(peer, id)"))
    );
    assert!(
        stdout
            .lines()
            .any(|line| line.contains("KeyRow.id") && line.contains("(target, id)"))
    );
    assert!(stdout.contains("17 bytes; scale 2; exact"));
    assert!(stdout.contains("Unix milliseconds"));
    assert!(stdout.contains("Subaccount"));
    assert!(stdout.contains("Float32"));
    assert!(stdout.contains("Float64"));
    assert!(stdout.contains("Enum: NestedPayload"));
    assert!(
        stdout
            .lines()
            .any(|line| line.contains("OptionalLink.id") && line.contains("(next, id)"))
    );
    assert!(stdout.contains("All optional keys present"));
    assert!(stdout.contains("0 rows; 3 examined; 3 fetched; 0 encoded bytes."));
    assert!(stdout.contains("6 rows; 6 examined; 6 fetched; 144 encoded bytes."));
    assert!(stdout.contains("2 rows; 6 examined; 6 fetched; 16 encoded bytes."));
    assert!(stdout.contains("1 row; 6 examined; 6 fetched; 8 encoded bytes."));
    assert!(stdout.contains("1 row; 1 examined; 1 fetched; 8 encoded bytes."));
    assert!(stdout.contains("0 rows; 1 examined; 1 fetched; 0 encoded bytes."));
    assert!(stdout.contains("1 row; 1 examined; 1 fetched; 9 encoded bytes."));
    assert!(stdout.contains("1 row; 1 examined; 1 fetched; 17 encoded bytes."));
    assert!(stdout.contains("2 rows; 6 examined; 6 fetched; 32 encoded bytes."));
    assert!(stdout.contains("1 row; 6 examined; 6 fetched; 16 encoded bytes."));
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
    let existence = run_console(
        &binary,
        "query Entry --select exists --work 1\nquery Entry --select exists --min 50 --work 5\nquery Entry --select exists --min 999 --work 6\nquery Document --select exists --entry-id 999 --work 1\nquit\n",
    );
    let blobs = run_console(
        &binary,
        "count BinaryObject\nquery BinaryObject\nquery BinaryObject --select payload --distinct\nquery BinaryObject --payload-min 0x00FF80 --payload-max 0x00ff80 --select min:payload\nquery BinaryObject --payload-min 0x --select max:payload\nquery BinaryObject --select payload,count --group-min 1\nquery BinaryObject --select payload,count:optional\nquery BinaryObject --payload-min 0x01 --select exists\nquery BinaryObject --payload-max 0x --select exists\nquery BinaryObject --order-by payload:desc --select payload\nquit\n",
    );
    assert_eq!(
        result_table_rows(&blobs),
        vec![
            vec!["1"],
            vec!["1", "0x00ff80", "—"],
            vec!["0x00ff80"],
            vec!["0x00ff80"],
            vec!["0x00ff80"],
            vec!["0x00ff80", "1"],
            vec!["0x00ff80", "0"],
            vec!["false"],
            vec!["false"],
            vec!["0x00ff80"],
        ]
    );
    assert_eq!(blobs.matches("query call (query):").count(), 9);
    assert_eq!(blobs.matches("query call (cardinality):").count(), 1);
    let prefixes = run_console(
        &binary,
        "query BinaryObject --payload-prefix 0x00FF --select payload\nquery BinaryObject --payload-prefix 0xff --select count\nquery BinaryObject --payload-prefix 0x00 --payload-max 0x00ff --select count\nquery BinaryObject --payload-suffix 0xfF80 --select payload\nquery BinaryObject --payload-suffix 0xff --select count\nquery BinaryObject --payload-prefix 0x00 --payload-suffix 0x80 --select count\nquery BinaryObject --payload-contains 0xfF --select payload\nquery BinaryObject --payload-contains 0xffff --select count\nquit\n",
    );
    assert_eq!(
        result_table_rows(&prefixes),
        [
            vec!["0x00ff80"],
            vec!["0"],
            vec!["0"],
            vec!["0x00ff80"],
            vec!["0"],
            vec!["1"],
            vec!["0x00ff80"],
            vec!["0"]
        ]
    );
    assert_eq!(prefixes.matches("query call (query):").count(), 8);
    assert_eq!(
        result_table_rows(&existence),
        vec![vec!["true"], vec!["true"], vec!["false"], vec!["false"]]
    );
    assert!(existence.contains("1 row; 1 examined; 1 fetched; 1 encoded bytes."));
    assert!(existence.contains("1 row; 0 examined; 0 fetched; 1 encoded bytes."));
    let distinct = run_console(&binary, "query Entry --select category --distinct\nquit\n");
    assert_eq!(result_table_rows(&distinct), [vec!["1"], vec!["0"]]);
    assert!(distinct.contains("2 rows; 6 examined; 6 fetched; 16 encoded bytes."));
    assert!(distinct.contains("Next cursor: none"));
    let sorted = run_console(
        &binary,
        "query Entry --order-by amount --desc --limit 2 --select id,amount\nquit\n",
    );
    assert_eq!(
        result_table_rows(&sorted),
        [vec!["6", "60"], vec!["5", "51"]]
    );
    assert!(sorted.contains("Next cursor: none"));
    let sorted = run_console(
        &binary,
        "query Entry --order-by category:asc,amount:desc --limit 3 --select id,amount\nquit\n",
    );
    assert_eq!(
        result_table_rows(&sorted),
        [vec!["6", "60"], vec!["4", "40"], vec!["2", "20"]]
    );
    assert!(sorted.contains("Next cursor: none"));
    let filtered_distinct = run_console(
        &binary,
        "query Entry --select category --distinct --min 0 --order-by category\nquery Entry --category 1 --order-by amount:desc --select category --distinct\nquery Entry --category 1 --order-by category --select category --distinct\nquit\n",
    );
    assert_eq!(
        result_table_rows(&filtered_distinct),
        [vec!["0"], vec!["1"], vec!["1"], vec!["1"]]
    );
    assert!(
        filtered_distinct
            .contains("1 row; 1 examined; 1 fetched; 8 encoded bytes.\nNext cursor: none")
    );
    assert!(contains_row(&stdout, &["0", "120"]));
    assert!(contains_row(&stdout, &["1", "93"]));
    assert!(contains_row(&stdout, &["1", "-10"]));
    assert!(contains_row(&stdout, &["213"]));
    assert!(contains_row(&stdout, &["0", "1"]));
    assert!(contains_row(&stdout, &["1", "1"]));
    assert!(contains_row(&stdout, &["35.5"]));
    assert!(contains_row(&stdout, &["0", "40"]));
    assert!(contains_row(&stdout, &["1", "31"]));
    assert!(contains_row(&stdout, &["1", "true", "1.25", "—"]));
    assert!(contains_row(&stdout, &["true", "3"]));
    assert!(contains_row(&stdout, &["true", "—"]));
    assert!(stdout.contains("1 row; 3 examined; 3 fetched; 17 encoded bytes."));
    assert!(contains_row(&stdout, &["-10"]));
    assert!(contains_row(&stdout, &["0"]));
    assert!(contains_row(&stdout, &["2"]));
    assert!(contains_row(&stdout, &["11"]));
    assert!(contains_row(&stdout, &["60"]));
    assert!(contains_row(&stdout, &["0", "20"]));
    assert!(contains_row(&stdout, &["1", "51"]));
    assert!(contains_row(&stdout, &["1", "—"]));
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
        ("Document", "(entry_id, score)", "Enum: Lifecycle"),
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
        "query Entry --order-by amount --work 1",
        "query Document --length title eq 4294967296",
        "query Document --length title contains 1",
        "query Entry --length id eq 1",
        "query Entry --present id",
        "query Document --absent missing",
        "query Entry --between-fields id amount",
        "query Entry --not-between-fields id amount missing",
        "query Entry --compare amount contains category",
        "query Document --compare id eq title",
        "query Entry --text id eq 1",
        "query Document --bool published yes",
        "query Document --bool title true",
        "query Document --text title unknown needle",
        "query Entry --select note",
        "query Entry --select category --distinct --limit 1",
        "query Entry --min 40 --select count --work 1",
        "query Entry --select exists --min 999 --work 5",
        "query Entry --select exists --group-min 0",
        "query Entry --select exists --distinct",
        "query Entry --select category,sum:amount --work 5",
        "query Entry --select sum:amount --work 5",
        "query Entry --select sum:amount --group-min 0",
        "query Entry --select max:amount --work 5",
        "query Entry --select category,max:amount --work 5",
        "query Document --select min:amount",
        "query Entry --select category,count --order-by category",
        "query Entry --group-order key",
        "query Entry --select count --group-min 0",
        "SELECT * FROM rows",
    ] {
        let output = Command::new(&binary)
            .args(command.split_whitespace())
            .output()
            .unwrap();
        assert!(!output.status.success(), "{command}");
    }
    let compared = run_console(
        &binary,
        "query Document --compare title contains title --select count\nquery Document --compare title ends-with note --select count\nquery Entry --compare id eq category --select id\nquery Document --length title eq 14 --absent note --select id\nquery Entry --compare id gt category --select count\nquery Document --present priority --absent note --select id\nquery Document --present note --select count\nquery DecimalRow --compare value gt maybe --order-by maybe --work 1 --select id\nquit\n",
    );
    assert!(compared.contains("0 rows; 0 examined; 0 fetched; 0 encoded bytes."));
    assert_eq!(
        result_table_rows(&compared),
        [
            vec!["1"],
            vec!["0"],
            vec!["1"],
            vec!["1"],
            vec!["5"],
            vec!["1"],
            vec!["0"]
        ]
    );
    let ranges = run_console(
        &binary,
        concat!(
            "query Entry --between-fields category id amount --select id\n",
            "query Entry --not-between-fields category id amount --select count\nquit\n",
        ),
    );
    assert_eq!(result_table_rows(&ranges), [vec!["1"], vec!["5"]]);
    let prefix_output = run_console(
        &binary,
        concat!(
            "query Document --bool published false --text title starts-with First --order-by title --select id\n",
            "query Document --bool published false --text title starts-with Missing --order-by title --select id\n",
            "query Document --bool published true --text title starts-with First --order-by title --select id\nquit\n",
        ),
    );
    assert_eq!(result_table_rows(&prefix_output), [vec!["1"]]);
    assert_eq!(
        prefix_output
            .matches("1 row; 1 examined; 1 fetched;")
            .count(),
        1
    );
    assert_eq!(
        prefix_output
            .matches("0 rows; 0 examined; 0 fetched;")
            .count(),
        2
    );
    let projected = run_console(
        &binary,
        "query Document --text title contains doc --select title\nquery Document --select note --distinct\nquit\n",
    );
    assert!(projected.contains("First document"), "{projected}");
    assert!(
        projected.contains("1 fetched; 18 encoded bytes."),
        "{projected}"
    );
    assert_eq!(result_table_rows(&projected), [vec!["—"]]);
    assert!(
        projected.contains("1 fetched; 1 encoded bytes."),
        "{projected}"
    );
    let literal = run_console(
        &binary,
        concat!(
            "query Document --text title eq 'First document' --select id\n",
            "query Document --text title contains Document --select count\n",
            "query Document --text title starts-with '' --select count\n",
            "query Document --text note contains '' --select count\n",
            "query Document --text title eq --limit --select count\nquit\n",
        ),
    );
    assert_eq!(
        result_table_rows(&literal),
        [vec!["1"], vec!["0"], vec!["1"], vec!["0"], vec!["0"]]
    );
    if std::env::var_os("IC_DB_CLI_RETAINED").is_some() {
        let literal = run_console(
            &binary,
            "aggregate Document --bool published false --text title ends-with document --select count --work 1\naggregate Document --text title ends-with Document --select count --work 1\naggregate Document --bool published true --select count --work 1\nquit\n",
        );
        assert_eq!(
            result_table_rows(&literal),
            [vec!["1"], vec!["0"], vec!["0"]]
        );
        // One console session exercises every retained selector. More than 16 jobs
        // proves successful commands release slots instead of leaking capacity.
        let cases: [ConsoleCase<'_>; 16] = [
            ("count", &[&["6"]]),
            ("count:amount", &[&["6"]]),
            ("distinct-count:amount", &[&["6"]]),
            (
                "distinct:amount",
                &[&["11"], &["20"], &["31"], &["40"], &["51"], &["60"]],
            ),
            ("sum:amount", &[&["213"]]),
            ("avg:amount", &[&["35.5"]]),
            ("min:amount", &[&["11"]]),
            ("max:amount", &[&["60"]]),
            ("category,count", &[&["0", "3"], &["1", "3"]]),
            ("category,count:amount", &[&["0", "3"], &["1", "3"]]),
            (
                "category,distinct-count:amount",
                &[&["0", "3"], &["1", "3"]],
            ),
            (
                "category,summary:amount",
                &[&["0", "3", "120", "40"], &["1", "3", "93", "31"]],
            ),
            ("category,sum:amount", &[&["0", "120"], &["1", "93"]]),
            ("category,avg:amount", &[&["0", "40"], &["1", "31"]]),
            ("category,min:amount", &[&["0", "20"], &["1", "11"]]),
            ("category,max:amount", &[&["0", "60"], &["1", "51"]]),
        ];
        let mut commands = cases
            .iter()
            .map(|(select, _)| {
                format!("aggregate Entry --select {select} --work 2 --page-size 1\n")
            })
            .collect::<String>();
        commands.push_str("aggregate Entry\naggregate Entry\naggregate Entry\n");
        let controls: [ConsoleCase<'_>; 18] = [
            (
                "distinct:amount --group-order key-desc",
                &[&["60"], &["51"], &["40"], &["31"], &["20"], &["11"]],
            ),
            (
                "distinct:amount --group-order first",
                &[&["11"], &["20"], &["31"], &["40"], &["51"], &["60"]],
            ),
            (
                "distinct:amount --group-order first --order-by category --desc",
                &[&["51"], &["31"], &["11"], &["60"], &["40"], &["20"]],
            ),
            ("count --between-fields category id amount", &[&["1"]]),
            ("count --not-between-fields category id amount", &[&["5"]]),
            ("count --compare id gt category", &[&["5"]]),
            ("count --min 40", &[&["3"]]),
            ("sum:amount --min 40 --limit 1", &[&["151"]]),
            ("distinct-count:amount --category 1 --min 30", &[&["2"]]),
            (
                "category,sum:amount --min 30 --group-min 50 --group-order asc --limit 1",
                &[&["1", "82"]],
            ),
            (
                "category,avg:amount --min 30 --group-min 45 --group-order desc",
                &[&["0", "50"]],
            ),
            (
                "category,min:amount --min 30 --group-order asc",
                &[&["1", "31"], &["0", "40"]],
            ),
            ("category,max:amount --category 1", &[&["1", "51"]]),
            ("category,count --min 40 --group-min 2", &[&["0", "2"]]),
            (
                "category,count --group-order desc --limit 1",
                &[&["0", "3"]],
            ),
            ("count --min 100", &[&["0"]]),
            ("min:amount --min 100", &[]),
            ("category,count --group-min 4", &[]),
        ];
        for (options, _) in controls {
            commands.push_str(&format!(
                "aggregate Entry --select {options} --work 1 --page-size 1\n"
            ));
        }
        commands.push_str("quit\n");
        let stdout = run_console(&binary, &commands);
        let sections = stdout.split("Entry — ").skip(1).collect::<Vec<_>>();
        let control_start = cases.len() + 3;
        assert_eq!(sections.len(), control_start + controls.len());
        for ((select, expected), section) in cases.iter().zip(&sections) {
            assert!(section.starts_with(&format!("{select}\n")));
            assert!(section.contains("Running; sequence 1; 2 examined"));
            let complete = section.find("Complete;").unwrap();
            assert!(
                !section[..complete].contains(['│', '|']),
                "running progress must not contain a result table"
            );
            let actual = result_table_rows(section);
            assert_eq!(&actual, expected, "{select}: {section}");
            assert_eq!(section.matches("Complete;").count(), expected.len().max(1));
        }
        for ((options, expected), section) in controls.iter().zip(&sections[control_start..]) {
            assert_eq!(
                &result_table_rows(section),
                expected,
                "{options}: {section}"
            );
            assert!(
                section.contains("Running; sequence 1; 1 examined"),
                "{options}: {section}"
            );
            let complete = section.find("Complete;").unwrap();
            assert!(!section[..complete].contains(['│', '|']));
            assert_eq!(section.matches("Complete;").count(), expected.len().max(1));
        }
        for command in [
            "aggregate Missing",
            "aggregate Entry --select invalid",
            "aggregate Entry --work 0",
            "aggregate Entry --page-size 33",
            "aggregate Entry --limit 0",
            "aggregate Entry --limit 33",
            "aggregate Entry --select count --group-min 1",
            "aggregate Entry --select count --group-order key",
            "aggregate Entry --select distinct:amount --group-order desc",
            "aggregate Entry --select category,count --group-order first",
            "aggregate Entry --select distinct:amount --group-order first --order-by amount",
            "aggregate Entry --select distinct:amount --group-order first --order-by category:unknown",
            "aggregate Entry --select distinct:amount --group-order key --desc",
            "aggregate Entry --group-order unsupported",
            "aggregate Entry --entry-id 1",
            "aggregate Entry --select count:note",
            "aggregate DecimalRow --min 0",
            "aggregate DecimalRow --category 1",
            "aggregate DecimalRow --entry-id 1",
            "aggregate DecimalRow --select sum:amount",
            "aggregate BinaryObject --min 0",
            "aggregate BinaryObject --select sum:payload",
            "aggregate Entry --select category,count --group-min 1.5",
            "aggregate Entry --select category,count --group-min 4 --group-max -1",
            "aggregate DecimalRow --select zone,avg:value --group-max 0.001",
            "aggregate DecimalRow --select zone,distinct-count:value --group-min 1.5",
            "aggregate Entry --select count --group-max 1",
            "aggregate Document --select entry_id,sum:score --group-min -170141183460469231731687303715884105729",
            "aggregate DecimalRow --select zone,avg:value --group-min 0.001",
            "aggregate Document --min 0",
            "aggregate Document --category 1",
            "aggregate Document --select min:amount",
        ] {
            assert!(
                !Command::new(&binary)
                    .args(command.split_whitespace())
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "{command}"
            );
        }
        let documents: [ConsoleCase<'_>; 28] = [
            ("count --absent note --present priority", &[&["1"]]),
            ("count --present note", &[&["0"]]),
            ("count", &[&["1"]]),
            ("count:note", &[&["0"]]),
            ("count:priority", &[&["1"]]),
            ("distinct-count:note", &[&["1"]]),
            ("min:note", &[]),
            ("max:note", &[]),
            ("min:priority", &[&["0"]]),
            ("max:priority", &[&["0"]]),
            ("sum:priority", &[&["0"]]),
            ("sum:score", &[&["-10"]]),
            ("entry_id,count", &[&["1", "1"]]),
            ("entry_id,count:note", &[&["1", "0"]]),
            ("entry_id,count:priority", &[&["1", "1"]]),
            ("entry_id,distinct-count:note", &[&["1", "1"]]),
            ("entry_id,min:note", &[&["1", "—"]]),
            ("entry_id,max:note", &[&["1", "—"]]),
            ("entry_id,min:priority", &[&["1", "0"]]),
            ("entry_id,max:priority", &[&["1", "0"]]),
            ("entry_id,sum:priority", &[&["1", "0"]]),
            ("entry_id,sum:score", &[&["1", "-10"]]),
            ("count --entry-id 2", &[&["0"]]),
            ("distinct-count:note --entry-id 2", &[&["0"]]),
            ("entry_id,count --entry-id 2", &[]),
            (
                "entry_id,count:priority --entry-id 1 --group-min 1 --group-order desc --limit 1",
                &[&["1", "1"]],
            ),
            ("entry_id,sum:score --group-min 0", &[]),
            (
                "entry_id,min:priority --group-min 0 --group-order asc",
                &[&["1", "0"]],
            ),
        ];
        let mut commands = documents
            .iter()
            .map(|(options, _)| {
                format!("aggregate Document --select {options} --work 1 --page-size 1\n")
            })
            .collect::<String>();
        commands.push_str("quit\n");
        let stdout = run_console(&binary, &commands);
        let sections = stdout.split("Document — ").skip(1).collect::<Vec<_>>();
        assert_eq!(sections.len(), documents.len());
        for ((options, expected), section) in documents.iter().zip(sections) {
            let actual = result_table_rows(section);
            assert_eq!(&actual, expected, "{options}: {section}");
            assert_eq!(section.matches("Complete;").count(), 1);
            let complete = section.find("Complete;").unwrap();
            assert!(!section[..complete].contains(['│', '|']));
            if !options.contains("--entry-id 2") {
                assert!(section.contains("Complete; sequence 1; 1 examined"));
            }
        }
        let decimals: [ConsoleCase<'_>; 14] = [
            ("count", &[&["3"]]),
            ("zone,count", &[&["true", "3"]]),
            ("sum:value", &[&["3"]]),
            ("avg:value", &[&["1"]]),
            ("sum:maybe", &[]),
            ("avg:maybe", &[]),
            ("zone,sum:value", &[&["true", "3"]]),
            ("zone,avg:value", &[&["true", "1"]]),
            ("zone,sum:maybe", &[&["true", "—"]]),
            ("zone,avg:maybe", &[&["true", "—"]]),
            ("zone,sum:value --group-min 3", &[&["true", "3"]]),
            ("zone,sum:value --group-min 4", &[]),
            (
                "zone,avg:value --group-order desc --limit 1",
                &[&["true", "1"]],
            ),
            ("zone,avg:maybe --group-min 0", &[]),
        ];
        let mut commands = decimals
            .iter()
            .map(|(options, _)| {
                format!("aggregate DecimalRow --select {options} --work 1 --page-size 1\n")
            })
            .collect::<String>();
        commands.push_str("quit\n");
        let stdout = run_console(&binary, &commands);
        let sections = stdout.split("DecimalRow — ").skip(1).collect::<Vec<_>>();
        assert_eq!(sections.len(), decimals.len());
        for ((options, expected), section) in decimals.iter().zip(sections) {
            assert_eq!(
                &result_table_rows(section),
                expected,
                "{options}: {section}"
            );
            assert_eq!(section.matches("Complete;").count(), 1);
            assert!(section.contains("Running; sequence 1; 1 examined"));
            assert!(section.contains("Complete; sequence 3; 3 examined"));
            let complete = section.find("Complete;").unwrap();
            assert!(!section[..complete].contains(['│', '|']));
        }
        let reversed_bounds = "count --payload-min 0xff --payload-max 0x00";
        let blobs: &[ConsoleCase<'_>] = &[
            (
                "count --length payload eq 3 --payload-prefix 0x00",
                &[&["1"]],
            ),
            ("count --length optional eq 0", &[&["0"]]),
            ("count", &[&["1"]]),
            ("count:payload", &[&["1"]]),
            ("count:optional", &[&["0"]]),
            ("distinct-count:payload", &[&["1"]]),
            ("distinct-count:optional", &[&["1"]]),
            ("optional,count", &[&["—", "1"]]),
            ("optional,count:optional", &[&["—", "0"]]),
            ("optional,count:optional --group-min 1", &[]),
            ("optional,count --payload-max 0x00ff", &[]),
            ("distinct-count:optional --payload-max 0x00ff", &[&["0"]]),
            ("min:payload", &[&["0x00ff80"]]),
            ("max:payload", &[&["0x00ff80"]]),
            ("min:optional", &[]),
            ("max:optional", &[]),
            ("payload,count", &[&["0x00ff80", "1"]]),
            ("payload,count:optional", &[&["0x00ff80", "0"]]),
            ("payload,count --group-min 2", &[]),
            (
                "count --payload-min 0x00ff80 --payload-max 0x00ff80",
                &[&["1"]],
            ),
            ("max:payload --payload-max 0x00ff", &[]),
            ("payload,count --payload-min 0x00ff81", &[]),
            (reversed_bounds, &[&["0"]]),
            ("count --payload-prefix 0x00ff", &[&["1"]]),
            ("count --payload-prefix 0xff", &[&["0"]]),
            ("count --payload-prefix 0x", &[&["1"]]),
            (
                "count --payload-contains 0xff --payload-prefix 0x00",
                &[&["1"]],
            ),
            ("count --payload-contains 0xffff", &[&["0"]]),
            ("count --payload-suffix 0x80", &[&["1"]]),
            ("count --payload-suffix 0xff", &[&["0"]]),
            (
                "max:payload --payload-prefix 0x00 --payload-suffix 0xff80",
                &[&["0x00ff80"]],
            ),
            (
                "max:payload --payload-prefix 0x00 --payload-max 0x00ff",
                &[],
            ),
        ];
        let mut commands = blobs
            .iter()
            .map(|(options, _)| {
                format!("aggregate BinaryObject --select {options} --work 1 --page-size 1\n")
            })
            .collect::<String>();
        commands.push_str("quit\n");
        let stdout = run_console(&binary, &commands);
        let sections = stdout.split("BinaryObject — ").skip(1).collect::<Vec<_>>();
        assert_eq!(sections.len(), blobs.len());
        for ((options, expected), section) in blobs.iter().zip(sections) {
            assert_eq!(
                &result_table_rows(section),
                expected,
                "{options}: {section}"
            );
            assert_eq!(section.matches("Complete;").count(), 1);
            let first_step = if *options == reversed_bounds {
                "Complete; sequence 1; 0 examined; 0 input bytes"
            } else {
                "Complete; sequence 1; 1 examined"
            };
            assert!(section.contains(first_step), "{options}: {section}");
            assert!(!section[..section.find("Complete;").unwrap()].contains(['│', '|']));
        }
        assert_eq!(
            stdout.matches("update call (aggregate_start):").count(),
            blobs.len()
        );
        assert_eq!(
            stdout.matches("update call (aggregate_resume):").count(),
            blobs.len()
        );
        assert_eq!(
            stdout.matches("update call (aggregate_cancel):").count(),
            blobs.len()
        );
        let paired_queries: &[(&str, &[ConsoleCase<'_>])] = &[
            (
                "Document",
                &[
                    ("entry_id,min:note --group-min 0", &[]),
                    ("entry_id,min:note --group-min alpha", &[]),
                    ("entry_id,max:note --group-max zèbre", &[]),
                    ("entry_id,max:note --group-min alpha --group-max zèbre", &[]),
                    ("entry_id,sum:score --group-min -10", &[&["1", "-10"]]),
                    ("entry_id,sum:score --group-min -9", &[]),
                    ("entry_id,sum:score --group-max -10", &[&["1", "-10"]]),
                    ("entry_id,sum:score --group-max -11", &[]),
                    (
                        "entry_id,sum:score --group-min -11 --group-max -9",
                        &[&["1", "-10"]],
                    ),
                    ("entry_id,min:priority --group-max 0", &[&["1", "0"]]),
                    (
                        "entry_id,sum:score --group-min -170141183460469231731687303715884105728",
                        &[&["1", "-10"]],
                    ),
                ],
            ),
            (
                "DecimalRow",
                &[
                    ("count:value", &[&["3"]]),
                    ("count:maybe", &[&["0"]]),
                    ("distinct-count:value", &[&["3"]]),
                    ("distinct-count:maybe", &[&["1"]]),
                    ("zone,count", &[&["true", "3"]]),
                    ("zone,count:value", &[&["true", "3"]]),
                    ("zone,count:maybe", &[&["true", "0"]]),
                    ("zone,distinct-count:value", &[&["true", "3"]]),
                    ("zone,distinct-count:maybe", &[&["true", "1"]]),
                    (
                        "zone,summary:value --group-min sum:2.99 --group-max avg:1.00",
                        &[&["true", "3", "3", "1"]],
                    ),
                    ("zone,summary:value --group-min sum:3.01", &[]),
                    (
                        "zone,summary:maybe --group-max count:0 --group-order avg:desc",
                        &[&["true", "0", "—", "—"]],
                    ),
                    ("zone,summary:maybe --group-min avg:0", &[]),
                    ("zone,count:maybe --group-max 0", &[&["true", "0"]]),
                    (
                        "zone,distinct-count:maybe --group-min 1 --group-max 1",
                        &[&["true", "1"]],
                    ),
                    ("zone,distinct-count:value --group-min 4", &[]),
                    ("min:value", &[&["-0.75"]]),
                    ("max:value", &[&["2.5"]]),
                    ("min:maybe", &[]),
                    ("max:maybe", &[]),
                    ("zone,min:value", &[&["true", "-0.75"]]),
                    ("zone,max:value", &[&["true", "2.5"]]),
                    ("zone,min:maybe", &[&["true", "—"]]),
                    ("zone,max:maybe", &[&["true", "—"]]),
                    (
                        "zone,min:value --group-min -0.75 --group-max -0.75",
                        &[&["true", "-0.75"]],
                    ),
                    (
                        "zone,max:value --group-min 2.50 --group-max 2.50",
                        &[&["true", "2.5"]],
                    ),
                    ("zone,max:value --group-max 2.49", &[]),
                    ("zone,min:value --group-min -0.74", &[]),
                    ("zone,avg:value --group-min 0.75", &[&["true", "1"]]),
                    ("zone,avg:value --group-min 1.01", &[]),
                    (
                        "zone,avg:value --group-min 0.75 --group-max 1.00",
                        &[&["true", "1"]],
                    ),
                    ("zone,avg:value --group-max 0.99", &[]),
                    ("zone,avg:maybe --group-max 0", &[]),
                    ("zone,sum:value --group-min 3.01 --group-max 3", &[]),
                    ("zone,sum:value --group-min 3.00", &[&["true", "3"]]),
                    ("zone,sum:value --group-min 3.01", &[]),
                    ("zone,sum:value --group-min -3.01", &[&["true", "3"]]),
                ],
            ),
            (
                "Entry",
                &[
                    (
                        "category,summary:amount --group-min count:3 --group-order avg:asc",
                        &[&["1", "3", "93", "31"], &["0", "3", "120", "40"]],
                    ),
                    (
                        "category,summary:amount --group-min avg:31.5 --group-max sum:120",
                        &[&["0", "3", "120", "40"]],
                    ),
                    ("category,avg:amount --group-min 31.50", &[&["0", "40"]]),
                    ("category,sum:amount --group-min 18446744073709551616", &[]),
                    (
                        "category,sum:amount --group-min 340282366920938463463374607431768211455",
                        &[],
                    ),
                    ("category,count --group-min 18446744073709551615", &[]),
                    (
                        "category,sum:amount --group-min 93 --group-max 120",
                        &[&["0", "120"], &["1", "93"]],
                    ),
                    (
                        "category,sum:amount --group-min 93 --group-max 120 --group-order asc --limit 1",
                        &[&["1", "93"]],
                    ),
                    ("category,min:amount --group-max 11", &[&["1", "11"]]),
                    ("category,count --group-max 3", &[&["0", "3"], &["1", "3"]]),
                ],
            ),
        ];
        for (entity, cases) in paired_queries {
            let mut commands = cases
                .iter()
                .flat_map(|(options, _)| {
                    [
                        format!("query {entity} --select {options}\n"),
                        format!("aggregate {entity} --select {options} --work 1 --page-size 1\n"),
                    ]
                })
                .collect::<String>();
            commands.push_str("quit\n");
            let stdout = run_console(&binary, &commands);
            let separator = format!("\n{entity}");
            let sections = stdout.split(&separator).skip(1).collect::<Vec<_>>();
            assert_eq!(sections.len(), cases.len() * 2);
            for ((options, expected), pair) in cases.iter().zip(sections.as_chunks::<2>().0) {
                for section in pair {
                    assert_eq!(
                        &result_table_rows(section),
                        expected,
                        "{entity} {options}: {section}"
                    );
                }
                assert_eq!(pair[1].matches("Complete;").count(), expected.len().max(1));
                let complete = pair[1].find("Complete;").unwrap();
                assert!(!pair[1][..complete].contains(['│', '|']));
            }
        }
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
