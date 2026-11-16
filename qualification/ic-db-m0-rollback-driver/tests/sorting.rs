//! Complete unindexed sorting through the same console/fluent boundary in Wasm.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

type Page = (
    Vec<String>,
    Vec<Vec<String>>,
    Option<Vec<u8>>,
    u32,
    u32,
    u32,
);

#[tokio::test]
async fn sorting_reads_complete_input_and_refuses_partial_results_across_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let query = async |order: &str,
                       descending: bool,
                       minimum: Option<u64>,
                       limit: u32,
                       work: u32,
                       bytes: u32,
                       after: Option<Vec<u8>>,
                       distinct: bool| {
        let request = (
            "Entry",
            after,
            limit,
            minimum,
            None::<u64>,
            None::<u64>,
            vec![order],
            descending,
            work,
            bytes,
            "id,amount",
            (distinct, (None::<String>, None::<String>), None::<String>),
        );
        let reply = pic
            .query_call(
                canister,
                canister,
                "query",
                ic_wire::encode(
                    &request,
                    Encoding::Frame {
                        method: 3,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<Page, String>>(
            &reply,
            Encoding::Frame {
                method: 3,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let backing = pic.get_stable_memory(canister).await;
    let page = query("amount", true, None, 2, 128, 32, None, false)
        .await
        .unwrap();
    assert_eq!(page.0, ["id", "amount"]);
    assert_eq!(page.1, [vec!["6", "60"], vec!["5", "51"]]);
    assert!(page.2.is_none());
    assert_eq!((page.3, page.4, page.5), (6, 6, 32));
    let page = query("amount", false, Some(20), 2, 128, 32, None, false)
        .await
        .unwrap();
    assert_eq!(page.1, [vec!["2", "20"], vec!["3", "31"]]);
    for (work, bytes) in [(1, 32), (6, 32), (128, 31), (128, 15)] {
        assert!(
            query("amount", true, None, 2, work, bytes, None, false)
                .await
                .is_err()
        );
    }
    let cursor = query("id", false, None, 1, 128, 16, None, false)
        .await
        .unwrap()
        .2
        .unwrap();
    assert!(
        query(
            "amount",
            true,
            None,
            2,
            128,
            32,
            Some(cursor.clone()),
            false
        )
        .await
        .is_err()
    );
    assert!(
        query("amount", true, None, 2, 128, 32, None, true)
            .await
            .is_err()
    );
    assert_eq!(pic.get_stable_memory(canister).await, backing);
    pic.advance_time(std::time::Duration::from_secs(600)).await;
    pic.tick().await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(
        query("amount", true, None, 2, 128, 32, None, false)
            .await
            .unwrap()
            .1,
        [vec!["6", "60"], vec!["5", "51"]]
    );
    // Ordered-path cursors retain their meaning across the same executable upgrade.
    assert_eq!(
        query("id", false, None, 1, 128, 16, Some(cursor), false)
            .await
            .unwrap()
            .1,
        [vec!["2", "20"]]
    );
    for id in 7_u64..=33 {
        let reply = pic
            .update_call(
                canister,
                canister,
                "write",
                ic_wire::encode(
                    &(id, id % 2, id * 10, 1_u8),
                    Encoding::Frame {
                        method: 4,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            ic_wire::decode::<Result<(), u8>>(
                &reply,
                Encoding::Frame {
                    method: 4,
                    direction: Direction::Response
                },
                LIMITS
            )
            .unwrap(),
            Ok(())
        );
    }
    let backing = pic.get_stable_memory(canister).await;
    assert!(
        query("amount", true, None, 1, 128, 16, None, false)
            .await
            .is_err()
    );
    let page = query("amount", true, Some(20), 1, 128, 16, None, false)
        .await
        .unwrap();
    assert_eq!(page.1, [vec!["33", "330"]]);
    assert_eq!(page.3, 33);
    assert!(page.2.is_none());
    assert_eq!(pic.get_stable_memory(canister).await, backing);
    owned.close().await;
}
