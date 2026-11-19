//! Actual native CLI -> verified HTTP query -> committed Canic application witness.

use ic_agent::{Identity, identity::BasicIdentity};
use ic_db_m0_rollback_driver::{instance, upgrade_stopped};
use std::{path::PathBuf, process::Command};

async fn command(args: Vec<String>) -> std::process::Output {
    tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_db"))
            .env_remove("IC_TESTKIT_POCKET_IC_URL")
            .args(args)
            .output()
            .unwrap()
    })
    .await
    .unwrap()
}

async fn insert(
    pic: &ic_testkit::pocket_ic::nonblocking::PocketIc,
    canister: ic_testkit::pocket_ic::CanisterId,
    owner: ic_testkit::pocket_ic::CanisterId,
    rows: &[(u64, u64)],
) {
    let reply = pic
        .update_call(
            canister,
            owner,
            "db_write",
            canic_wire::encode(
                &rows,
                canic_wire::Encoding::Frame {
                    method: canic_wire::method_id("canic-db-witness.write.v1"),
                    direction: canic_wire::Direction::Request,
                },
                canic::db::RPC_LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let expected = Ok::<_, ()>((u32::try_from(rows.len()).unwrap(), String::new()));
    assert_eq!(
        reply,
        canic_wire::encode(
            &expected,
            canic_wire::Encoding::Frame {
                method: canic_wire::method_id("canic-db-witness.write.v1"),
                direction: canic_wire::Direction::Response,
            },
            canic::db::RPC_LIMITS
        )
        .unwrap()
    );
}

#[tokio::test]
async fn explicit_application_inspection_preserves_authority_and_storage() {
    let artifacts =
        PathBuf::from(std::env::var_os("IC_DB_CANIC_WASM_DIR").expect("run make m4-inspection"));
    let normal = std::fs::read(artifacts.join("normal.wasm")).unwrap();
    let fault = std::fs::read(artifacts.join("fault.wasm")).unwrap();
    let incompatible = std::fs::read(artifacts.join("incompatible.wasm")).unwrap();
    // Public synthetic test identity, never an application credential.
    let key = artifacts.join("test-identity.pem");
    std::fs::write(
        &key,
        // Literal PKCS#8 encoding of the public synthetic [1; 32] seed below.
        "-----BEGIN PRIVATE KEY-----\n\
         MC4CAQAwBQYDK2VwBCIEIAEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEB\n\
         -----END PRIVATE KEY-----\n",
    )
    .unwrap();
    let owner = BasicIdentity::from_raw_key(&[1; 32]).sender().unwrap();
    assert_eq!(
        BasicIdentity::from_pem_file(&key)
            .unwrap()
            .sender()
            .unwrap(),
        owner
    );
    let mut pic = instance().await.unwrap();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, normal.clone(), owner.as_slice().to_vec(), None)
        .await;
    insert(
        &pic,
        canister,
        owner,
        &[(10, 1), (20, 2), (30, 30), (40, 40)],
    )
    .await;
    insert(&pic, canister, owner, &[(50, 5), (60, 60)]).await;
    let url = pic.make_live(None).await;
    let base = vec![
        "inspect".into(),
        "--provider".into(),
        url.to_string(),
        "--canister".into(),
        canister.to_text(),
        "--local-root-key".into(),
    ];
    let invoke_command = async |command_name: &str, extra: &[&str], anonymous: bool| {
        let mut args = base.clone();
        args[0] = command_name.into();
        if anonymous {
            args.push("--anonymous".into());
        } else {
            args.extend(["--identity".into(), key.to_string_lossy().into_owned()]);
        }
        args.extend(extra.iter().map(|s| s.to_string()));

        command(args).await
    };
    let invoke =
        async |extra: &[&str], anonymous: bool| invoke_command("inspect", extra, anonymous).await;
    let before = pic.get_stable_memory(canister).await;
    // Selecting a local provider does not implicitly trust its root key.
    for command_name in ["inspect", "witness-list"] {
        let mut args = base.clone();
        args[0] = command_name.into();
        args.retain(|arg| arg != "--local-root-key");
        args.extend([
            "--identity".into(),
            key.to_string_lossy().into_owned(),
            "--json".into(),
        ]);
        let refused = command(args).await;
        assert!(!refused.status.success());
        assert!(
            refused.stdout.is_empty(),
            "untrusted replies must not render"
        );
        let error = String::from_utf8(refused.stderr).unwrap();
        assert!(error.contains("Certificate verification failed"), "{error}");
        assert!(error.contains("cycles unavailable"), "{error}");
    }
    let catalogue = invoke(&["--json"], false).await;
    assert!(
        catalogue.status.success(),
        "{}",
        String::from_utf8_lossy(&catalogue.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&catalogue.stdout).unwrap();
    assert_eq!(value["caller"], owner.to_text());
    assert_eq!(
        value["inspection"]["selection"]["Catalogue"],
        serde_json::json!(["Item"])
    );
    assert!(String::from_utf8_lossy(&catalogue.stderr).contains("cycles unavailable"));
    assert!(!String::from_utf8_lossy(&catalogue.stderr).contains("0 cycles"));
    let selected = invoke(&["--entity", "Item", "--count"], false).await;
    assert!(
        selected.status.success(),
        "{}",
        String::from_utf8_lossy(&selected.stderr)
    );
    let text = String::from_utf8(selected.stdout).unwrap();
    for expected in [
        "APPLICATION INSPECTION",
        "Readable",
        "Maintained count",
        "score",
        "Nat64",
        "Info",
        "query call (db_inspect)",
    ] {
        assert!(text.contains(expected), "{text}");
    }
    assert_eq!(text.matches("query call (db_inspect)").count(), 2);
    assert!(!text.contains("LOCAL M1 FIXTURE"));
    for (extra, anonymous) in [(&[][..], true), (&["--entity", "Missing"][..], false)] {
        let denied = invoke(extra, anonymous).await;
        assert!(!denied.status.success());
        assert!(String::from_utf8_lossy(&denied.stderr).contains("cycles unavailable"));
        assert!(!String::from_utf8_lossy(&denied.stdout).contains("LOCAL M1 FIXTURE"));
    }
    let first = invoke_command(
        "witness-list",
        &["--minimum", "30", "--limit", "1", "--work", "1", "--json"],
        false,
    )
    .await;
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["page"]["rows"], serde_json::json!([]));
    assert_eq!(first["page"]["examined"], 1);
    let first_cursor = first["page"]["next"].as_str().unwrap().to_owned();
    let corrupt = format!(
        "{}{}",
        if first_cursor.starts_with("ff") {
            "00"
        } else {
            "ff"
        },
        &first_cursor[2..]
    );
    for (minimum, after, anonymous) in [
        ("31", first_cursor.as_str(), false),
        ("30", corrupt.as_str(), false),
        ("30", first_cursor.as_str(), true),
    ] {
        let refused = invoke_command(
            "witness-list",
            &[
                "--minimum",
                minimum,
                "--limit",
                "1",
                "--work",
                "1",
                "--after",
                after,
                "--json",
            ],
            anonymous,
        )
        .await;
        assert!(!refused.status.success());
        assert!(refused.stdout.is_empty());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("cycles unavailable"));
        if !anonymous {
            assert!(String::from_utf8_lossy(&refused.stderr).contains("listing refused: Cursor"));
        }
    }
    assert_eq!(pic.get_stable_memory(canister).await, before);
    // The sparse page stopped at key 10. Both inserts match the filter.
    insert(&pic, canister, owner, &[(5, 50), (15, 35)]).await;
    let fresh = invoke_command(
        "witness-list",
        &["--minimum", "30", "--limit", "1", "--work", "1", "--json"],
        false,
    )
    .await;
    assert!(
        fresh.status.success(),
        "{}",
        String::from_utf8_lossy(&fresh.stderr)
    );
    let fresh: serde_json::Value = serde_json::from_slice(&fresh.stdout).unwrap();
    assert_eq!(
        fresh["page"]["rows"],
        serde_json::json!([{"id":5,"score":50}])
    );
    pic.stop_live().await;
    for (wasm, admission) in [
        (normal, "Readable"),
        (fault, "Maintenance"),
        (incompatible, "Incompatible"),
    ] {
        upgrade_stopped(&pic, canister, wasm, vec![], None)
            .await
            .unwrap();
        if admission == "Maintenance" {
            pic.update_call(
                canister,
                owner,
                "db_test_control",
                canic_wire::encode(
                    &2u8,
                    canic_wire::Encoding::Frame {
                        method: 991,
                        direction: canic_wire::Direction::Request,
                    },
                    canic::db::RPC_LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        }
        let url = pic.make_live(Some(url.port().unwrap())).await;
        assert_eq!(url.as_str(), base[2]);
        let before = pic.get_stable_memory(canister).await;
        let reply = invoke(&["--entity", "Item", "--count", "--json"], false).await;
        assert!(
            reply.status.success(),
            "{}",
            String::from_utf8_lossy(&reply.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&reply.stdout).unwrap();
        if admission == "Readable" {
            assert!(value["inspection"]["admission"].get("Readable").is_some());
            assert_eq!(
                value["inspection"]["selection"]["Entity"]["cardinality"]["Available"],
                8
            );
        } else {
            assert_eq!(value["inspection"]["admission"], admission);
            assert_eq!(
                value["inspection"]["selection"]["Entity"]["cardinality"],
                "Unavailable"
            );
        }
        if admission == "Readable" {
            let mut after = Some(first_cursor.clone());
            let mut rows = Vec::new();
            let mut pages = 0;
            while let Some(cursor) = after {
                pages += 1;
                assert!(pages <= 8);
                let reply = invoke_command(
                    "witness-list",
                    &[
                        "--minimum",
                        "30",
                        "--limit",
                        "1",
                        "--work",
                        "1",
                        "--after",
                        &cursor,
                        "--json",
                    ],
                    false,
                )
                .await;
                assert!(
                    reply.status.success(),
                    "{}",
                    String::from_utf8_lossy(&reply.stderr)
                );
                assert_eq!(
                    String::from_utf8_lossy(&reply.stderr)
                        .matches("query call (db_query)")
                        .count(),
                    1
                );
                let value: serde_json::Value = serde_json::from_slice(&reply.stdout).unwrap();
                rows.extend(value["page"]["rows"].as_array().unwrap().iter().cloned());
                after = value["page"]["next"].as_str().map(str::to_owned);
            }
            assert_eq!(
                rows,
                serde_json::json!([{"id":15,"score":35},{"id":30,"score":30},{"id":40,"score":40},{"id":60,"score":60}])
                    .as_array()
                    .unwrap()
                    .clone()
            );
            let table = invoke_command(
                "witness-list",
                &["--minimum", "30", "--limit", "1", "--work", "4"],
                false,
            )
            .await;
            assert!(table.status.success());
            let table = String::from_utf8(table.stdout).unwrap();
            for expected in [
                "CANIC WITNESS LISTING",
                "score",
                "Next cursor:",
                "cycles unavailable",
            ] {
                assert!(table.contains(expected));
            }
        } else {
            let refused = invoke_command("witness-list", &["--json"], false).await;
            assert!(!refused.status.success());
            assert!(refused.stdout.is_empty());
        }
        assert_eq!(pic.get_stable_memory(canister).await, before);
        pic.stop_live().await;
        if admission == "Maintenance" {
            pic.update_call(
                canister,
                owner,
                "db_test_control",
                canic_wire::encode(
                    &3_u8,
                    canic_wire::Encoding::Frame {
                        method: 991,
                        direction: canic_wire::Direction::Request,
                    },
                    canic::db::RPC_LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        }
    }
    pic.drop().await;
    let failed = invoke(&[], false).await;
    assert!(!failed.status.success());
    assert!(!String::from_utf8_lossy(&failed.stdout).contains("LOCAL M1 FIXTURE"));
    std::fs::remove_file(key).unwrap();
}
