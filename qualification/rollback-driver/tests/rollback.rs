//! Actual Wasm rollback checks; raw stable bytes provide an oracle independent of row lookup.

use ic_db_m0_rollback_driver as support;
use ic_testkit::pic::server_assets;
use ic_wire::native;
use support::{LIMITS, server_directory};
type Entries = Vec<(u64, u64)>;
type Snapshot = (Entries, Entries, Entries, [u64; 4]);

#[test]
#[ignore = "explicit provisioning only: make m0-setup"]
fn prepare_server() {
    let path = server_assets::setup(&server_directory(), || false).unwrap();
    println!("Prepared {}", path.display());
}

#[tokio::test]
async fn message_rollback() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M0_WASM").expect("run make m0-rollback")).unwrap();
    let owned = support::instance().await.unwrap();
    let pic = owned.instance();
    for mode in 0_u8..=8 {
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 2_000_000_000_000).await;
        pic.install_canister(canister, wasm.clone(), vec![], None)
            .await;
        let before = pic.get_stable_memory(canister).await;
        let request = native::encode_request(1, &(mode, 7_u64, 11_u64), LIMITS).unwrap();
        let response = pic.update_call(canister, canister, "mutate", request).await;
        let after = pic.get_stable_memory(canister).await;
        if matches!(mode, 2..=5 | 7) {
            let rejection = response.expect_err("injected boundary must trap");
            assert!(
                rejection
                    .reject_message
                    .contains("M0 injected commit failure"),
                "mode {mode}: unexpected failure: {rejection:?}"
            );
            assert!(
                before == after,
                "mode {mode}: raw backing, including allocator controls, must revert"
            );
        } else {
            let result: Result<(), u8> =
                native::decode_response(1, &response.unwrap(), LIMITS).unwrap();
            assert_eq!(
                result,
                match mode {
                    0 => Ok(()),
                    1 => Err(1),
                    6 => Err(6),
                    8 => Err(8),
                    _ => unreachable!(),
                }
            );
            assert_eq!(
                before == after,
                mode == 1,
                "normal Err after mutation must retain effects"
            );
        }
        let reply = pic
            .query_call(
                canister,
                canister,
                "inspect",
                native::encode_request(2, &(), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        let snapshot: Snapshot = native::decode_response(2, &reply, LIMITS).unwrap();
        let committed = matches!(mode, 0 | 6 | 8);
        if committed {
            assert!(after.len() > before.len(), "reservation must grow backing");
        }
        assert_eq!(snapshot.0, if committed { vec![(7, 11)] } else { vec![] });
        assert_eq!(
            snapshot.1,
            if matches!(mode, 0 | 8) {
                vec![(11, 7)]
            } else {
                vec![]
            }
        );
        assert_eq!(
            snapshot.2,
            if matches!(mode, 0 | 8) {
                vec![(7, 11)]
            } else {
                vec![]
            }
        );
        println!(
            "mode={mode} retained_bytes={} generation={} data_pages={} physical_pages={} extents={}",
            after.len(),
            snapshot.3[0],
            snapshot.3[1],
            snapshot.3[2],
            snapshot.3[3]
        );
        if mode == 0 {
            // Uniqueness refusal must not change existing data or allocation controls.
            let duplicate = native::encode_request(1, &(0_u8, 8_u64, 11_u64), LIMITS).unwrap();
            let reply = pic
                .update_call(canister, canister, "mutate", duplicate)
                .await
                .unwrap();
            assert_eq!(
                native::decode_response::<Result<(), u8>>(1, &reply, LIMITS).unwrap(),
                Err(1)
            );
            assert!(after == pic.get_stable_memory(canister).await);
            pic.upgrade_canister(canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
            let reply = pic
                .query_call(
                    canister,
                    canister,
                    "inspect",
                    native::encode_request(2, &(), LIMITS).unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                native::decode_response::<Snapshot>(2, &reply, LIMITS).unwrap(),
                snapshot
            );
            assert!(after == pic.get_stable_memory(canister).await);
        }
    }
    owned.close().await;
}
