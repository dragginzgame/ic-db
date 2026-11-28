//! Execute the producer-owned schema for each separately built contract.

mod wire_frame;
use ic_db_m0_rollback_driver::instance;

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
            let request = wire_frame::request(1, &hub);
            let reply = pic.query_call(id, id, "schema_size", request).await;

            if let Some(size) = size {
                let result: u64 = wire_frame::response(1, &reply.unwrap());
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
        let request = wire_frame::request(1, &create);
        let before = pic.cycle_balance(id).await;
        let reply = pic.update_call(id, id, "exercise", request).await.unwrap();
        let cycles = before - pic.cycle_balance(id).await;
        let (observed, work): (u64, [u64; 6]) = wire_frame::response(1, &reply);
        assert_eq!(observed, width);
        eprintln!("width={width} create={create} instructions={work:?} update_cycles={cycles}");
    }
    pic.drop().await;
}
