//! Execute the producer-owned schema for each separately built contract.

use ic_db_cli::fixture_wire;
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

#[tokio::test]
async fn selected_schema_on_replica() {
    let pic = instance().await.unwrap();
    for (artifact, expected) in [("baseline", Some(3)), ("drift", None)] {
        let path =
            std::path::PathBuf::from(std::env::var_os("IC_DB_SCHEMA_OWNER_ARTIFACTS").unwrap())
                .join(format!("{artifact}.wasm"));
        let id = pic.create_canister().await;
        pic.add_cycles(id, 2_000_000_000_000).await;
        pic.install_canister(id, std::fs::read(path).unwrap(), vec![], None)
            .await;
        for (hub, size) in [(true, expected), (false, Some(1))] {
            let request = fixture_wire::encode(
                &hub,
                Encoding::Frame {
                    method: 1,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap();
            let reply = pic.query_call(id, id, "schema_size", request).await;

            if let Some(size) = size {
                let result: u64 = fixture_wire::decode(
                    &reply.unwrap(),
                    Encoding::Frame {
                        method: 1,
                        direction: Direction::Response,
                    },
                    LIMITS,
                )
                .unwrap();
                assert_eq!(result, size, "{artifact}");
            } else {
                assert!(
                    reply
                        .unwrap_err()
                        .reject_message
                        .contains("host/target schema mismatch")
                );
            }
        }
    }
    pic.drop().await;
}

/// Attribute cold admission and individual operations on one named stable store.
#[tokio::test]
#[ignore = "requires the separately built single-store width artifact"]
async fn wide_store_on_replica() {
    let width: u64 = std::env::var("IC_DB_STORE_WIDTH").unwrap().parse().unwrap();
    let path = std::path::PathBuf::from(std::env::var_os("IC_DB_SCHEMA_OWNER_ARTIFACTS").unwrap())
        .join(format!("width-{width}.wasm"));
    let wasm = std::fs::read(path).unwrap();
    let pic = instance().await.unwrap();
    let id = pic.create_canister().await;
    pic.add_cycles(id, 10_000_000_000_000).await;
    pic.install_canister(id, wasm.clone(), vec![], None).await;
    for create in [true, false] {
        if !create {
            ic_db_m0_rollback_driver::upgrade_stopped(&pic, id, wasm.clone(), vec![], None)
                .await
                .unwrap();
        }
        let request = fixture_wire::encode(
            &create,
            Encoding::Frame {
                method: 1,
                direction: Direction::Request,
            },
            LIMITS,
        )
        .unwrap();
        let before = pic.cycle_balance(id).await;
        let reply = pic.update_call(id, id, "exercise", request).await.unwrap();
        let cycles = before - pic.cycle_balance(id).await;
        let (observed, work): (u64, [u64; 6]) = fixture_wire::decode(
            &reply,
            Encoding::Frame {
                method: 1,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap();
        assert_eq!(observed, width);
        eprintln!("width={width} create={create} instructions={work:?} update_cycles={cycles}");
    }
    pic.drop().await;
}
