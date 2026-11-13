//! Executable admission and interrupted copy/cutover/cancellation against real Wasm.

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::native;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

type Page = (Vec<(u64, u64)>, Option<u64>, u64);

fn artifacts() -> PathBuf {
    PathBuf::from(std::env::var_os("IC_DB_M0_MAINTENANCE_DIR").expect("run make m0-maintenance"))
}

#[tokio::test]
async fn identified_maintenance() {
    let root = artifacts();
    let a = std::fs::read(root.join("maintenance-a.wasm")).unwrap();
    let b = std::fs::read(root.join("maintenance-b.wasm")).unwrap();
    let bprime = std::fs::read(root.join("maintenance-bprime.wasm")).unwrap();
    let bad = std::fs::read(root.join("maintenance-bad.wasm")).unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    for scenario in ["success", "cancel", "collision", "missing", "unsupported"] {
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 3_000_000_000_000).await;
        pic.install_canister(canister, a.clone(), vec![], None)
            .await;
        let call = async |input: (u8, u64, u64, u8)| -> Result<[u64; 9], u8> {
            let reply = pic
                .update_call(
                    canister,
                    canister,
                    "command",
                    native::encode_request(4, &input, LIMITS).unwrap(),
                )
                .await
                .unwrap();
            native::decode_response(4, &reply, LIMITS).unwrap()
        };
        let read = async |after: u64, minimum: u64| -> Result<Page, u8> {
            let reply = pic
                .query_call(
                    canister,
                    canister,
                    "query",
                    native::encode_request(3, &(after, 3_u64, minimum), LIMITS).unwrap(),
                )
                .await
                .unwrap();
            native::decode_response(3, &reply, LIMITS).unwrap()
        };
        let trap = async |operation: u8, boundary: u8| {
            let before = pic.get_stable_memory(canister).await;
            let rejection = pic
                .update_call(
                    canister,
                    canister,
                    "command",
                    native::encode_request(4, &(operation, 0_u64, 0_u64, boundary), LIMITS)
                        .unwrap(),
                )
                .await
                .unwrap_err();
            assert!(
                rejection
                    .reject_message
                    .contains("M0 injected maintenance failure")
            );
            assert!(
                before == pic.get_stable_memory(canister).await,
                "{scenario}: boundary {boundary}"
            );
        };
        assert_eq!(
            read(0, 0).await.unwrap(),
            (vec![(1, 11), (2, 20), (3, 31)], Some(3), 3)
        );
        assert_eq!(read(0, 100).await.unwrap(), (vec![], Some(3), 3));
        assert_eq!(read(3, 100).await.unwrap(), (vec![], Some(6), 3));
        assert_eq!(read(6, 100).await.unwrap(), (vec![], None, 0));
        if matches!(scenario, "missing" | "unsupported") {
            call((7, if scenario == "missing" { 0 } else { 99 }, 0, 0))
                .await
                .unwrap();
            let before = pic.get_stable_memory(canister).await;
            let error = if scenario == "missing" { 1 } else { 2 };
            assert_eq!(read(0, 0).await, Err(error));
            pic.upgrade_canister(canister, b.clone(), vec![], None)
                .await
                .unwrap();
            assert_eq!(call((0, 0, 0, 0)).await, Err(error));
            assert!(before == pic.get_stable_memory(canister).await);
            continue;
        }
        if scenario == "collision" {
            // Distinct old values 11 and 12 collide only after normalization, across chunks.
            call((6, 6, 12, 0)).await.unwrap();
        }
        let before_upgrade = pic.get_stable_memory(canister).await;
        let rejection = pic
            .upgrade_canister(canister, bad.clone(), vec![], None)
            .await
            .unwrap_err();
        assert!(
            rejection
                .reject_message
                .contains("M0 injected upgrade failure")
        );
        assert!(before_upgrade == pic.get_stable_memory(canister).await);
        assert!(
            read(0, 0).await.is_ok(),
            "failed upgrade must leave A installed"
        );
        pic.upgrade_canister(canister, b.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(read(0, 0).await, Err(2));
        trap(0, 1).await;
        let frozen = call((0, 0, 0, 0)).await.unwrap();
        assert_eq!((frozen[1], frozen[2], frozen[3]), (1, 1, 7));
        pic.upgrade_canister(canister, a.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(read(0, 0).await, Err(3));
        let frozen_backing = pic.get_stable_memory(canister).await;
        assert_eq!(call((6, 1, 90, 0)).await, Err(3));
        assert!(frozen_backing == pic.get_stable_memory(canister).await);
        pic.upgrade_canister(canister, b.clone(), vec![], None)
            .await
            .unwrap();
        for boundary in 2..=4 {
            trap(1, boundary).await;
        }
        let partial = call((1, 0, 0, 0)).await.unwrap();
        assert_eq!((partial[4], partial[6], partial[7]), (2, 2, 2));
        pic.upgrade_canister(canister, bprime.clone(), vec![], None)
            .await
            .unwrap();
        let before = pic.get_stable_memory(canister).await;
        assert_eq!(call((1, 0, 0, 0)).await, Err(4));
        assert!(before == pic.get_stable_memory(canister).await);
        pic.upgrade_canister(canister, b.clone(), vec![], None)
            .await
            .unwrap();
        if scenario != "success" {
            if scenario == "collision" {
                call((1, 0, 0, 0)).await.unwrap();
                let before = pic.get_stable_memory(canister).await;
                assert_eq!(call((1, 0, 0, 0)).await, Err(6));
                assert_eq!(call((2, 0, 0, 0)).await, Err(3));
                assert!(before == pic.get_stable_memory(canister).await);
            }
            call((4, 0, 0, 0)).await.unwrap();
            trap(5, 6).await;
            for _ in 0..3 {
                let status = call((5, 0, 0, 0)).await.unwrap();
                if status[2] == 0 {
                    assert_eq!((status[1], status[5], status[6], status[7]), (1, 6, 0, 0));
                    break;
                }
            }
            assert_eq!(
                read(0, 0).await,
                Err(2),
                "B's maintenance decoder cannot serve source"
            );
            pic.upgrade_canister(canister, a.clone(), vec![], None)
                .await
                .unwrap();
            assert_eq!(read(0, 0).await.unwrap().0, vec![(1, 11), (2, 20), (3, 31)]);
            assert_eq!(
                read(3, 0).await.unwrap().0,
                vec![
                    (4, 40),
                    (5, 51),
                    (6, if scenario == "collision" { 12 } else { 60 })
                ]
            );
            println!("{scenario}: source preserved; target cleaned; A reopens");
            continue;
        }
        call((1, 0, 0, 0)).await.unwrap();
        let ready = call((1, 0, 0, 0)).await.unwrap();
        assert_eq!(
            (ready[2], ready[4], ready[5], ready[6], ready[7]),
            (2, 6, 6, 6, 6)
        );
        assert_eq!(read(0, 0).await, Err(3));
        trap(2, 5).await;
        let published = call((2, 0, 0, 0)).await.unwrap();
        assert_eq!((published[1], published[2]), (2, 4));
        assert_eq!(read(0, 0).await.unwrap().0, vec![(1, 1), (2, 2), (3, 3)]);
        pic.upgrade_canister(canister, a.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(read(0, 0).await, Err(2));
        assert_eq!(call((6, 1, 90, 0)).await, Err(2));
        pic.upgrade_canister(canister, b.clone(), vec![], None)
            .await
            .unwrap();
        trap(3, 6).await;
        call((3, 0, 0, 0)).await.unwrap();
        call((3, 0, 0, 0)).await.unwrap();
        trap(3, 7).await;
        let cleaned = call((3, 0, 0, 0)).await.unwrap();
        assert_eq!(
            (cleaned[1], cleaned[2], cleaned[5], cleaned[6], cleaned[7]),
            (2, 0, 0, 6, 6)
        );
        assert_eq!(
            cleaned[8], published[8],
            "logical cleanup does not shrink stable memory"
        );
        pic.upgrade_canister(canister, b.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(read(3, 0).await.unwrap().0, vec![(4, 4), (5, 5), (6, 6)]);
        println!(
            "success: identified copy/cutover/reopen; physical pages before/after cleanup={}/{}",
            published[8], cleaned[8]
        );
    }
    owned.close().await;
}

#[test]
fn console_inspects_and_queries() {
    let prefix = std::env::temp_dir().join(format!("ic-db-console-{}", std::process::id()));
    std::fs::create_dir(&prefix).unwrap();
    std::fs::create_dir(prefix.join("bin")).unwrap();
    std::fs::create_dir_all(prefix.join("share/ic-db/m0")).unwrap();
    let binary = prefix.join("bin/db");
    std::fs::copy(env!("CARGO_BIN_EXE_db"), &binary).unwrap();
    std::fs::copy(
        artifacts().join("maintenance-a.wasm"),
        prefix.join("share/ic-db/m0/maintenance-a.wasm"),
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
        .write_all(b"schema\nstatus\nquery --min 100\nquery --after 3 --limit 2\nquit\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("LOCAL M0 FIXTURE"));
    assert!(stdout.contains("SourceRow v1"));
    assert!(stdout.contains("stored_schema=1 phase=live"));
    assert!(stdout.contains("scanned=3 next_after=Some(3) complete=false"));
    assert!(stdout.contains("4\t40\n5\t51"));
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
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("4\t40\n5\t51")
    );
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
