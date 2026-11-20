//! Execute the producer-owned schema for each separately built contract.

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
            let request = ic_wire::encode(
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
                let result: u64 = ic_wire::decode(
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
