//! Complete unindexed sorting through the same console/fluent boundary in Wasm.
mod artifact;
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
async fn decimal_sorting_and_distinct_preserve_exact_values_across_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    // Literal expectations avoid the product Decimal comparator, codec and formatter.
    let values = [
        "1",
        "-0.01",
        "1.00",
        "-0.00",
        "1.0",
        "0.01",
        "-1701411834604692317316873037158841057.28",
        "1701411834604692317316873037158841057.27",
    ];
    let rows: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(i, value)| (i as u64 + 1, None::<String>, *value))
        .collect();
    let reply = pic
        .update_call(
            canister,
            canister,
            "decimal_batch",
            ic_wire::encode(
                &(rows, false),
                Encoding::Frame {
                    method: 67,
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
                method: 67,
                direction: Direction::Response,
            },
            LIMITS
        )
        .unwrap(),
        Ok(())
    );
    let query = async |select: &str, order: &str, distinct: bool, work: u32, bytes: u32| {
        let request = (
            "DecimalRow",
            None::<Vec<u8>>,
            32_u32,
            None::<u64>,
            None::<u64>,
            None::<u64>,
            vec![order],
            false,
            work,
            bytes,
            select,
            (
                distinct,
                (None::<String>, None::<String>),
                None::<String>,
                [None::<Vec<u8>>, None, None, None, None],
            ),
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
    let ascending = [values[6], "-0.01", "0", "0.01", "1", values[7]];
    for upgraded in [false, true] {
        if upgraded {
            ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
        }
        let before = pic.get_stable_memory(canister).await;
        for descending in [false, true] {
            let order = if descending {
                "value:desc"
            } else {
                "value:asc"
            };
            let mut expected = ascending.to_vec();
            if descending {
                expected.reverse();
            }
            let page = query("value", order, true, 9, 102).await.unwrap();
            assert_eq!(
                page.1,
                expected.iter().map(|v| vec![*v]).collect::<Vec<_>>()
            );
            assert_eq!((page.3, page.4, page.5), (8, 8, 102));
            assert!(page.2.is_none());
            // Byte admission follows deduplication, but input work still covers all rows.
            assert!(query("value", order, true, 9, 101).await.is_err());
            assert!(query("value", order, true, 8, 102).await.is_err());
            let page = query("value", order, false, 9, 136).await.unwrap();
            let expected = if descending {
                vec![values[7], "1", "1", "1", "0.01", "0", "-0.01", values[6]]
            } else {
                vec![values[6], "-0.01", "0", "0.01", "1", "1", "1", values[7]]
            };
            assert_eq!(
                page.1,
                expected.into_iter().map(|v| vec![v]).collect::<Vec<_>>()
            );
            assert!(page.2.is_none());
        }
        // Sparse unique indexes cannot supply absent rows; all eight collapse to one absent value.
        let absent = query("maybe", "maybe:desc", true, 9, 1).await.unwrap();
        assert_eq!(absent.1, [vec!["—"]]);
        assert_eq!((absent.3, absent.4, absent.5), (8, 8, 1));
        assert!(absent.2.is_none());
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    owned.drop().await;
}

#[tokio::test]
async fn sorting_reads_complete_input_and_refuses_partial_results_across_upgrade() {
    let measured = std::env::var_os("IC_DB_QUERY_COSTS").is_some();
    let (wasm, facts) = artifact::read(
        std::fs::File::open(
            std::env::var_os("IC_DB_M1_RECORDS_WASM")
                .expect("run make m2-queries or m2-sort-costs"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        facts
            .exports
            .iter()
            .any(|name| name == "canister_update measured_query"),
        measured
    );
    if measured {
        eprintln!(
            "sort-cost wasm_bytes={} code_section_bytes={} sha256={}",
            facts.identity.bytes,
            facts.code_section_bytes,
            facts
                .identity
                .sha256
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
    }
    let owned = instance().await.unwrap();
    let pic = &owned;
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
            order.split(',').collect::<Vec<_>>(),
            descending,
            work,
            bytes,
            if distinct { "category" } else { "id,amount" },
            (
                distinct,
                (None::<String>, None::<String>),
                None::<String>,
                [None::<Vec<u8>>, None, None, None, None],
            ),
        );
        if measured {
            let backing = pic.get_stable_memory(canister).await;
            let before = pic.cycle_balance(canister).await;
            let reply = pic
                .update_call(
                    canister,
                    canister,
                    "measured_query",
                    ic_wire::encode(
                        &request,
                        Encoding::Frame {
                            method: 147,
                            direction: Direction::Request,
                        },
                        LIMITS,
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            let cycles = before
                .checked_sub(pic.cycle_balance(canister).await)
                .unwrap();
            let (result, instructions): (Result<Page, String>, u64) = ic_wire::decode(
                &reply,
                Encoding::Frame {
                    method: 147,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap();
            assert_eq!(pic.get_stable_memory(canister).await, backing);
            eprintln!(
                "sort-cost order={order} reverse={descending} min={minimum:?} limit={limit} work={work} bytes={bytes} distinct={distinct} result={:?} instructions={instructions} cycles={cycles}",
                result
                    .as_ref()
                    .map(|page| (page.1.len(), page.3, page.4, page.5))
            );
            return result;
        }
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
    let page = query(
        "category:asc,amount:desc",
        false,
        None,
        3,
        128,
        48,
        None,
        false,
    )
    .await
    .unwrap();
    assert_eq!(page.1, [vec!["6", "60"], vec!["4", "40"], vec!["2", "20"]]);
    assert!(page.2.is_none());
    for order in [
        "category:asc,amount:desc",
        "amount:sideways",
        "amount:desc:asc",
    ] {
        assert!(
            query(order, false, None, 1, 6, 16, None, false)
                .await
                .is_err()
        );
    }
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
    let reverse_cursor = query("id:desc", false, None, 1, 128, 16, None, false)
        .await
        .unwrap()
        .2
        .unwrap();
    assert!(
        query(
            "id:asc",
            false,
            None,
            1,
            128,
            16,
            Some(reverse_cursor.clone()),
            false
        )
        .await
        .is_err()
    );
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
    let distinct = query("category:desc", false, None, 1, 128, 8, None, true)
        .await
        .unwrap();
    assert_eq!(distinct.1, [vec!["1"]]);
    assert!(distinct.2.is_none());
    assert_eq!((distinct.3, distinct.4, distinct.5), (6, 6, 8));
    assert!(
        query("category:desc", false, None, 1, 6, 8, None, true)
            .await
            .is_err()
    );
    pic.advance_time(std::time::Duration::from_secs(600)).await;
    pic.tick().await;
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
        .await
        .unwrap();
    assert_eq!(
        query("amount", true, None, 2, 128, 32, None, false)
            .await
            .unwrap()
            .1,
        [vec!["6", "60"], vec!["5", "51"]]
    );
    let page = query(
        "category:asc,amount:desc",
        true,
        None,
        3,
        128,
        48,
        None,
        false,
    )
    .await
    .unwrap();
    assert_eq!(page.1, [vec!["1", "11"], vec!["3", "31"], vec!["5", "51"]]);
    assert!(page.2.is_none());
    // Ordered-path cursors retain their meaning across the same executable upgrade.
    assert_eq!(
        query("category:desc", false, None, 2, 128, 16, None, true)
            .await
            .unwrap()
            .1,
        [vec!["1"], vec!["0"]]
    );
    assert_eq!(
        query(
            "id:desc",
            false,
            None,
            1,
            128,
            16,
            Some(reverse_cursor),
            false
        )
        .await
        .unwrap()
        .1,
        [vec!["5", "51"]]
    );
    assert_eq!(
        query("id", false, None, 1, 128, 16, Some(cursor), false)
            .await
            .unwrap()
            .1,
        [vec!["2", "20"]]
    );
    let write = async |id: u64, category: u64, amount: u64, mode: u8| {
        let reply = pic
            .update_call(
                canister,
                canister,
                "write",
                ic_wire::encode(
                    &(id, category, amount, mode),
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
    };
    for id in 7_u64..=511 {
        write(id, id % 2, id * 10, 1).await;
    }
    for upgraded in [false, true] {
        if measured {
            eprintln!("sort-cost phase=511-rows upgraded={upgraded}");
        }
        if upgraded {
            ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
        }
        let backing = pic.get_stable_memory(canister).await;
        // Duplicate rows consume scan work but only two retained tuple slots.
        for descending in [false, true] {
            let page = query("category:desc", descending, None, 2, 512, 16, None, true)
                .await
                .unwrap();
            assert_eq!(
                page.1,
                if descending {
                    vec![vec!["0"], vec!["1"]]
                } else {
                    vec![vec!["1"], vec!["0"]]
                }
            );
            assert_eq!((page.3, page.4, page.5), (511, 511, 16));
            assert!(page.2.is_none());
        }
        let first = query("category:desc", false, None, 1, 512, 8, None, true)
            .await
            .unwrap();
        assert_eq!(first.1, [vec!["1"]]);
        assert!(
            query("category:desc", false, None, 1, 511, 8, None, true)
                .await
                .is_err()
        );
        for limit in [1, 32] {
            for descending in [false, true] {
                let mut expected: Vec<_> = (1_u64..=511)
                    .map(|id| (id, if id <= 6 { id * 10 + id % 2 } else { id * 10 }))
                    .collect();
                expected.sort_by_key(|&(id, amount)| (id % 2, std::cmp::Reverse(amount), id));
                if descending {
                    expected.reverse();
                }
                let page = query(
                    "category:asc,amount:desc",
                    descending,
                    None,
                    limit,
                    512,
                    limit * 16,
                    None,
                    false,
                )
                .await
                .unwrap();
                assert_eq!(
                    page.1,
                    expected
                        .iter()
                        .take(limit as usize)
                        .map(|(id, amount)| vec![id.to_string(), amount.to_string()])
                        .collect::<Vec<_>>()
                );
                assert_eq!((page.3, page.4, page.5), (511, 511, limit * 16));
                assert!(page.2.is_none());
            }
        }
        assert!(
            query("amount", true, None, 1, 511, 16, None, false)
                .await
                .is_err()
        );
        assert!(
            query("amount", true, None, 32, 512, 511, None, false)
                .await
                .is_err()
        );
        let page = query("amount", true, Some(20), 1, 512, 16, None, false)
            .await
            .unwrap();
        assert_eq!(page.1, [vec!["511", "5110"]]);
        assert_eq!(page.3, 511);
        assert!(page.2.is_none());
        // A sparse or empty match still proves complete input before returning.
        for minimum in [5110, 6000] {
            let page = query("amount", true, Some(minimum), 1, 512, 16, None, false)
                .await
                .unwrap();
            assert_eq!(
                page.1,
                if minimum == 5110 {
                    vec![vec!["511", "5110"]]
                } else {
                    vec![]
                }
            );
            assert_eq!((page.3, page.4), (511, 511));
            assert!(page.2.is_none());
        }
        assert_eq!(pic.get_stable_memory(canister).await, backing);
    }
    // 0 and 1 still occur elsewhere. Adding categories 2..=31 fills the 32-key
    // envelope; a later category 32 must refuse even when only one result is asked for.
    for id in 2_u64..=31 {
        write(id, id, id * 10, 0).await;
    }
    let complete = query("category:desc", false, None, 32, 512, 256, None, true)
        .await
        .unwrap();
    assert_eq!(
        complete.1,
        (0..32)
            .rev()
            .map(|n| vec![n.to_string()])
            .collect::<Vec<_>>()
    );
    assert!(complete.2.is_none());
    write(32, 32, 320, 0).await;
    for upgraded in [false, true] {
        if upgraded {
            ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
        }
        let backing = pic.get_stable_memory(canister).await;
        assert!(
            query("category:desc", false, None, 1, 512, 8, None, true)
                .await
                .is_err()
        );
        assert_eq!(pic.get_stable_memory(canister).await, backing);
    }
    owned.drop().await;
}
