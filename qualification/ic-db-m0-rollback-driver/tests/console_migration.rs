//! Local demo uses actual maintenance gates and survives a real executable upgrade.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};
use std::{
    io::Write,
    process::{Command, Stdio},
};

type Reply = (bool, Vec<Vec<String>>);

fn wasm() -> Vec<u8> {
    std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m3-cli")).unwrap()
}

#[tokio::test]
async fn demo_refuses_wrong_phases_and_reopens_exact_progress() {
    let wasm = wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 2_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |action: u8| {
        let reply = pic
            .update_call(
                canister,
                canister,
                "migration_demo",
                ic_wire::encode(
                    &action,
                    Encoding::Frame {
                        method: 142,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<Reply, String>>(
            &reply,
            Encoding::Frame {
                method: 142,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let expected: Vec<Vec<String>> = (1..=6)
        .map(|id| vec![id.to_string(), (id * 10).to_string()])
        .collect();
    assert_eq!(call(0).await.unwrap(), (true, expected.clone()));
    assert_eq!(call(7).await.unwrap().1, expected);
    // A frame for the existing fluent-any endpoint must not authorize this control.
    let backing = pic.get_stable_memory(canister).await;
    assert!(
        pic.update_call(
            canister,
            canister,
            "migration_demo",
            ic_wire::encode(
                &7_u8,
                Encoding::Frame {
                    method: 123,
                    direction: Direction::Request
                },
                LIMITS
            )
            .unwrap()
        )
        .await
        .is_err()
    );
    assert_eq!(pic.get_stable_memory(canister).await, backing);
    call(1).await.unwrap();
    for action in [7, 6, 3, 4, 5, 255] {
        let backing = pic.get_stable_memory(canister).await;
        assert!(call(action).await.is_err(), "action {action}");
        assert_eq!(pic.get_stable_memory(canister).await, backing);
    }
    assert_eq!(call(2).await.unwrap(), (false, vec![vec!["2".into()]]));
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
        .await
        .unwrap();
    let mut copied = 2;
    let mut complete = false;
    for _ in 0..8 {
        let (done, rows) = call(2).await.unwrap();
        copied += rows[0][0].parse::<u64>().unwrap();
        if done {
            complete = true;
            break;
        }
    }
    assert!(complete);
    assert_eq!(copied, 6, "upgrade must not restart the input");
    let backing = pic.get_stable_memory(canister).await;
    assert!(call(4).await.is_err(), "copy completion is not validation");
    assert_eq!(pic.get_stable_memory(canister).await, backing);
    complete = false;
    for _ in 0..32 {
        if call(3).await.unwrap().0 {
            complete = true;
            break;
        }
    }
    assert!(complete);
    call(4).await.unwrap();
    let target: Vec<Vec<String>> = expected
        .into_iter()
        .map(|mut row| {
            row.push("migrated".into());
            row
        })
        .collect();
    assert_eq!(call(6).await.unwrap().1, target);
    for action in [0, 1, 7, 2, 4] {
        let backing = pic.get_stable_memory(canister).await;
        assert!(call(action).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, backing);
    }
    complete = false;
    for _ in 0..32 {
        if call(5).await.unwrap().0 {
            complete = true;
            break;
        }
    }
    assert!(complete);
    let backing = pic.get_stable_memory(canister).await;
    assert!(call(5).await.unwrap().0);
    assert!(call(7).await.is_err());
    assert!(
        call(0).await.is_err(),
        "cleanup does not authorize resetting the source"
    );
    assert_eq!(pic.get_stable_memory(canister).await, backing);
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(call(6).await.unwrap().1, target);
    owned.drop().await;
}

#[test]
fn installed_demo_prints_migrated_rows_and_keeps_the_query_fixture() {
    let prefix =
        std::env::temp_dir().join(format!("ic-db-migration-console-{}", std::process::id()));
    std::fs::create_dir(&prefix).unwrap();
    std::fs::create_dir(prefix.join("bin")).unwrap();
    std::fs::create_dir_all(prefix.join("share/ic-db/m1")).unwrap();
    let binary = prefix.join("bin/db");
    std::fs::copy(env!("CARGO_BIN_EXE_db"), &binary).unwrap();
    std::fs::write(prefix.join("share/ic-db/m1/records.wasm"), wasm()).unwrap();
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
