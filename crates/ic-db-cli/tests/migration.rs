//! Installed migration demo and subsequent console queries.

use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn installed_demo_prints_migrated_rows_and_keeps_the_query_fixture() {
    let prefix =
        std::env::temp_dir().join(format!("ic-db-migration-console-{}", std::process::id()));
    std::fs::create_dir(&prefix).unwrap();
    std::fs::create_dir(prefix.join("bin")).unwrap();
    std::fs::create_dir_all(prefix.join("share/ic-db/m1")).unwrap();
    let binary = prefix.join("bin/db");
    std::fs::copy(env!("CARGO_BIN_EXE_db"), &binary).unwrap();
    std::fs::copy(
        std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m3-cli"),
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
        .write_all(b"schema Entry\nstatus\ncount Document\naggregate Entry --select count --work 2\nmigration-demo\nquery Entry --select count\nquit\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.is_empty(), "{stderr}");
    let lines: Vec<_> = stdout
        .lines()
        .filter(|line| line.starts_with("query call") || line.starts_with("update call"))
        .collect();
    for method in ["cardinality", "aggregate_start", "migration_demo"] {
        let position = stdout.find(&format!("call ({method}): ")).unwrap();
        assert!(stdout[..position].contains("+-------+"));
    }
    let count_footer = stdout.find("query call (cardinality): ").unwrap();
    assert!(stdout[..count_footer].ends_with("| 1     |\n+-------+\n"));
    let aggregate_footer = stdout.find("update call (aggregate_start): ").unwrap();
    assert!(stdout[..aggregate_footer].ends_with("| 6     |\n+-------+\n"));
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("query call"))
            .count(),
        4,
        "{stderr}"
    );
    for method in ["schema", "inspect", "cardinality", "query"] {
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with(&format!("query call ({method}): "))),
            "{stderr}"
        );
    }
    for method in [
        "aggregate_start",
        "aggregate_resume",
        "aggregate_cancel",
        "migration_demo",
    ] {
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with(&format!("update call ({method}): "))),
            "{stderr}"
        );
    }
    let resumed = lines
        .iter()
        .filter(|line| line.starts_with("update call (aggregate_resume)"))
        .count();
    assert_eq!(
        resumed,
        stdout
            .lines()
            .filter(|line| line.starts_with("Call "))
            .count()
    );
    for line in lines {
        let (_, stats) = line.split_once(": ").unwrap();
        let (seconds, charge) = stats.split_once(" sec, ").unwrap();
        assert_eq!(seconds.split_once('.').unwrap().1.len(), 2);
        assert!(seconds.parse::<f64>().unwrap().is_finite());

        if line.starts_with("query call") {
            assert_eq!(charge, "0 cycles charged");
        } else {
            assert!(line.starts_with("update call"), "{line}");
            let cycles = if let Some((_, exact)) = charge.split_once("cycles charged (") {
                exact.strip_suffix(')').unwrap().parse::<u128>().unwrap()
            } else {
                charge
                    .strip_suffix(" cycles charged")
                    .unwrap()
                    .parse::<u128>()
                    .unwrap()
            };
            assert!(cycles > 0, "{line}");
        }
    }
    assert!(stdout.contains("LOCAL M1 FIXTURE"));
    assert!(stdout.contains("Freeze 1: complete"));
    assert!(stdout.contains("Copy 1: in progress"));
    assert!(stdout.contains("Upgrade: complete"));
    assert!(stdout.contains("Publish 1: complete"));
    let target = stdout.split("Item — target (label index)").nth(1).unwrap();
    for id in 1..=6 {
        assert!(
            target.lines().any(|line| {
                let cells: Vec<_> = line.split(['│', '|']).map(str::trim).collect();
                cells == ["", &id.to_string(), &(id * 10).to_string(), "migrated", ""]
            }),
            "missing target row {id}: {target}"
        );
    }
    assert!(target.contains("\nEntry\n"));
    assert!(
        target
            .lines()
            .any(|line| line.split(['│', '|']).map(str::trim).collect::<Vec<_>>() == ["", "6", ""])
    );
    std::fs::remove_dir_all(prefix).unwrap();
}
