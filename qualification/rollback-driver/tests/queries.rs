//! Fluent query pages remain bounded and resume across upgrades in real Wasm.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::native;
type Page = (Vec<(u64, u64)>, Option<Vec<u8>>, u32, u32, u32, u64);
type Request = (Option<Vec<u8>>, bool, Option<u64>, u64, u32, u32, u32);
type ConsoleRequest<'a> = (
    &'a str,
    Option<Vec<u8>>,
    u32,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Vec<&'a str>,
    bool,
    u32,
    u32,
    &'a str,
    bool,
);
type DisplayPage = (
    Vec<String>,
    Vec<Vec<String>>,
    Option<Vec<u8>>,
    u32,
    u32,
    u32,
);

#[tokio::test]
async fn sparse_fluent_pages_retry_and_reopen() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let query = async |request: Request| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_query",
                native::encode_request(95, &request, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Page, u8>>(95, &reply, LIMITS).unwrap()
    };
    let before = pic.get_stable_memory(canister).await;
    let selected =
        async |request: ConsoleRequest<'_>, group_min: Option<u64>, group_order: Option<&str>| {
            let (
                entity,
                cursor,
                limit,
                min,
                category,
                entry_id,
                order,
                desc,
                work,
                bytes,
                select,
                distinct,
            ) = request;
            let request = (
                entity,
                cursor,
                limit,
                min,
                category,
                entry_id,
                order,
                desc,
                work,
                bytes,
                select,
                (distinct, group_min, group_order),
            );
            let reply = pic
                .query_call(
                    canister,
                    canister,
                    "query",
                    native::encode_request(3, &request, LIMITS).unwrap(),
                )
                .await
                .unwrap();
            native::decode_response::<Result<DisplayPage, String>>(3, &reply, LIMITS).unwrap()
        };
    let console = async |request: ConsoleRequest<'_>| selected(request, None, None).await;
    let ordered_request = (
        "Entry",
        None,
        1,
        None,
        None,
        None,
        vec![],
        false,
        7,
        16,
        "category,min:amount",
        false,
    );
    let selected_groups = selected(ordered_request.clone(), Some(15), Some("desc"))
        .await
        .unwrap();
    assert_eq!(selected_groups.1, [vec!["0", "20"]]);
    assert_eq!((selected_groups.3, selected_groups.5), (6, 16));
    let ascending = selected(ordered_request.clone(), None, Some("asc"))
        .await
        .unwrap();
    assert_eq!(ascending.1, [vec!["1", "11"]]);
    let mut row_request = ordered_request.clone();
    row_request.10 = "full";
    assert!(selected(row_request, None, Some("key")).await.is_err());
    let distinct = async |cursor: Option<Vec<u8>>,
                          work: u32,
                          bytes: u32,
                          descending: bool,
                          category: Option<u64>| {
        console((
            "Entry",
            cursor,
            1,
            None,
            category,
            None,
            vec!["category"],
            descending,
            work,
            bytes,
            "category",
            true,
        ))
        .await
        .unwrap()
    };
    let aggregate = async |select: &str, limit: u32, work: u32, bytes: u32| {
        console((
            "Entry",
            None,
            limit,
            None,
            None,
            None,
            vec![],
            false,
            work,
            bytes,
            select,
            false,
        ))
        .await
    };
    let sum = aggregate("sum:amount", 1, 7, 16).await.unwrap();
    assert_eq!(sum.1, [vec!["213"]]);
    assert_eq!((sum.3, sum.4, sum.5), (6, 6, 16));
    assert!(sum.2.is_none());
    assert!(aggregate("sum:amount", 1, 6, 16).await.is_err());
    assert!(aggregate("sum:amount", 1, 7, 15).await.is_err());
    let count = aggregate("count", 1, 7, 8).await.unwrap();
    assert_eq!(count.1, [vec!["6"]]);
    assert_eq!((count.3, count.4, count.5), (6, 6, 8));
    let grouped = aggregate("category,count", 1, 7, 16).await.unwrap();
    assert_eq!(grouped.1, [vec!["0", "3"]]);
    assert_eq!((grouped.3, grouped.5), (6, 16));
    assert!(grouped.2.is_none());
    assert!(aggregate("count", 1, 6, 8).await.is_err());
    assert!(aggregate("category,count", 2, 7, 31).await.is_err());
    let distinct_count = aggregate("distinct-count:category", 1, 7, 8).await.unwrap();
    assert_eq!(distinct_count.1, [vec!["2"]]);
    assert_eq!(
        (distinct_count.3, distinct_count.4, distinct_count.5),
        (6, 6, 8)
    );
    assert!(distinct_count.2.is_none());
    assert_eq!(aggregate("count:amount", 1, 7, 8).await.unwrap(), count);
    assert_eq!(
        aggregate("category,count:amount", 1, 7, 16).await.unwrap(),
        grouped
    );
    assert!(aggregate("distinct-count:category", 1, 6, 8).await.is_err());
    assert!(aggregate("count:missing", 1, 7, 8).await.is_err());
    let notes = console((
        "Document",
        None,
        1,
        None,
        None,
        None,
        vec![],
        false,
        128,
        8,
        "count:note",
        false,
    ))
    .await
    .unwrap();
    assert_eq!(notes.1, [vec!["0"]]);
    assert_eq!((notes.3, notes.4, notes.5), (1, 1, 8));
    let minimum = aggregate("min:amount", 1, 7, 8).await.unwrap();
    let maximum = aggregate("max:amount", 1, 7, 8).await.unwrap();
    assert_eq!(minimum.1, [vec!["11"]]);
    assert_eq!(maximum.1, [vec!["60"]]);
    assert_eq!((maximum.3, maximum.4, maximum.5), (6, 6, 8));
    assert!(maximum.2.is_none());
    assert!(aggregate("max:amount", 1, 6, 8).await.is_err());
    assert!(aggregate("min:amount", 1, 7, 7).await.is_err());
    let grouped_sum = aggregate("category,sum:amount", 2, 7, 48).await.unwrap();
    assert_eq!(grouped_sum.1, [vec!["0", "120"], vec!["1", "93"]]);
    assert_eq!((grouped_sum.3, grouped_sum.5), (6, 48));
    let sum_request = (
        "Entry",
        None,
        1,
        None,
        None,
        None,
        vec![],
        false,
        7,
        24,
        "category,sum:amount",
        false,
    );
    let selected_sum = selected(sum_request.clone(), Some(100), Some("desc"))
        .await
        .unwrap();
    assert_eq!(selected_sum.1, [vec!["0", "120"]]);
    assert_eq!(
        selected(sum_request.clone(), None, Some("asc"))
            .await
            .unwrap()
            .1,
        [vec!["1", "93"]]
    );
    assert!(aggregate("category,sum:amount", 2, 6, 48).await.is_err());
    assert!(aggregate("category,sum:amount", 2, 7, 47).await.is_err());
    let group_min = aggregate("category,min:amount", 2, 7, 32).await.unwrap();
    let group_max = aggregate("category,max:amount", 2, 7, 32).await.unwrap();
    assert_eq!(group_min.1, [vec!["0", "20"], vec!["1", "11"]]);
    assert_eq!(group_max.1, [vec!["0", "60"], vec!["1", "51"]]);
    assert_eq!((group_max.3, group_max.4, group_max.5), (6, 6, 32));
    assert!(group_max.2.is_none());
    assert_eq!(
        aggregate("category,max:amount", 1, 7, 16).await.unwrap().1,
        [vec!["0", "60"]]
    );
    assert!(aggregate("category,min:amount", 1, 6, 16).await.is_err());
    assert!(aggregate("category,max:amount", 2, 7, 31).await.is_err());
    for (select, expected, bytes) in [
        ("entry_id,min:note", "—", 8),
        ("entry_id,max:priority", "0", 17),
        ("entry_id,sum:priority", "0", 25),
        ("entry_id,sum:score", "-10", 24),
    ] {
        let result = console((
            "Document",
            None,
            1,
            None,
            None,
            None,
            vec![],
            false,
            128,
            bytes,
            select,
            false,
        ))
        .await
        .unwrap();
        assert_eq!(result.1, [vec!["1", expected]]);
        assert_eq!((result.3, result.4, result.5), (1, 1, bytes));
        assert!(result.2.is_none());
    }
    for select in ["min:note", "max:note", "min:priority", "max:priority"] {
        let result = console((
            "Document",
            None,
            1,
            None,
            None,
            None,
            vec![],
            false,
            128,
            9,
            select,
            false,
        ))
        .await
        .unwrap();
        if select.ends_with("note") {
            assert!(result.1.is_empty());
            assert_eq!(result.5, 0);
        } else {
            assert_eq!(result.1, [vec!["0"]]);
            assert_eq!(result.5, 9);
        }
        assert_eq!((result.3, result.4), (1, 1));
        assert!(result.2.is_none());
    }
    let unique_first = distinct(None, 1, 8, false, None).await;
    assert_eq!(unique_first.1, [vec!["0"]]);
    assert_eq!((unique_first.3, unique_first.5), (1, 8));
    let filtered_unique = distinct(None, 1, 8, false, Some(1)).await;
    assert_eq!(filtered_unique.1, [vec!["1"]]);
    assert_eq!(filtered_unique.3, 1);
    let ordered = async |request: Request| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_ordered",
                native::encode_request(98, &request, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Page, u8>>(98, &reply, LIMITS).unwrap()
    };
    let order_start = ordered((None, false, Some(1), 40, 2, 1, 4096))
        .await
        .unwrap();
    assert!(order_start.0.is_empty());
    assert_eq!(order_start.2, 1);
    assert!(order_start.1.is_some());
    let all = ordered((None, false, None, 0, 32, 128, 4096))
        .await
        .unwrap();
    assert_eq!(
        all.0,
        [(2, 20), (4, 40), (6, 60), (1, 11), (3, 31), (5, 51)]
    );
    assert_eq!(all.2, 6);
    let reverse = ordered((None, true, None, 0, 32, 128, 4096)).await.unwrap();
    assert_eq!(reverse.0, all.0.into_iter().rev().collect::<Vec<_>>());
    let project = async |request: Request| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_projection",
                native::encode_request(96, &request, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Page, u8>>(96, &reply, LIMITS).unwrap()
    };
    let projected = project((None, false, None, 0, 32, 128, 31)).await.unwrap();
    assert_eq!(projected.0, [(1, 11)]);
    assert_eq!((projected.2, projected.4), (2, 16));
    assert_eq!(
        query((projected.1.clone(), false, None, 0, 32, 128, 4096))
            .await
            .unwrap_err(),
        1
    );
    let first = query((None, false, Some(1), 40, 2, 1, 4096)).await.unwrap();
    assert!(first.0.is_empty());
    assert!(first.1.is_some());
    assert_eq!((first.2, first.3, first.4), (1, 1, 0));
    println!("fluent_sparse_first_instructions={}", first.5);
    assert_eq!(
        query((first.1.clone(), true, Some(1), 40, 2, 1, 4096))
            .await
            .unwrap_err(),
        1
    );
    assert_eq!(
        query((first.1.clone(), false, Some(1), 41, 2, 1, 4096))
            .await
            .unwrap_err(),
        1
    );
    // Only queries ran since install, so no update rounds drained the IC's
    // install-code rate limiter. Advance virtual time, not wall-clock time.
    pic.advance_time(std::time::Duration::from_secs(600)).await;
    pic.tick().await;
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(
        aggregate("category,count", 1, 7, 16).await.unwrap(),
        grouped
    );
    assert_eq!(
        aggregate("distinct-count:category", 1, 7, 8).await.unwrap(),
        distinct_count
    );
    assert_eq!(
        selected(ordered_request, Some(15), Some("desc"))
            .await
            .unwrap(),
        selected_groups
    );
    assert_eq!(
        aggregate("category,sum:amount", 2, 7, 48).await.unwrap(),
        grouped_sum
    );
    assert_eq!(
        selected(sum_request, Some(100), Some("desc"))
            .await
            .unwrap(),
        selected_sum
    );
    assert_eq!(aggregate("sum:amount", 1, 7, 16).await.unwrap(), sum);
    assert_eq!(aggregate("max:amount", 1, 7, 8).await.unwrap(), maximum);
    assert_eq!(
        aggregate("category,max:amount", 2, 7, 32).await.unwrap(),
        group_max
    );
    let filtered_done = distinct(filtered_unique.2, 4, 8, false, Some(1)).await;
    assert!(filtered_done.1.is_empty());
    assert!(filtered_done.2.is_none());
    assert_eq!(filtered_done.3, 2);
    let empty_unique = distinct(unique_first.2, 2, 8, false, None).await;
    assert!(empty_unique.1.is_empty());
    assert_eq!(empty_unique.3, 2);
    assert!(empty_unique.2.is_some());
    let second_unique = distinct(empty_unique.2, 4, 8, false, None).await;
    assert_eq!(second_unique.1, [vec!["1"]]);
    let done_unique = distinct(second_unique.2, 4, 8, false, None).await;
    assert!(done_unique.1.is_empty());
    assert!(done_unique.2.is_none());
    assert_eq!(distinct(None, 4, 8, true, None).await.1, [vec!["1"]]);
    let after_upgrade = ordered((order_start.1.clone(), false, Some(1), 40, 2, 128, 4096))
        .await
        .unwrap();
    assert_eq!(after_upgrade.0, [(5, 51)]);
    assert_eq!(after_upgrade.2, 2); // only the two remaining category=1 entries
    assert_eq!(
        ordered((order_start.1.clone(), true, Some(1), 40, 2, 128, 4096))
            .await
            .unwrap_err(),
        1
    );
    assert_eq!(
        ordered((order_start.1, false, Some(0), 40, 2, 128, 4096))
            .await
            .unwrap_err(),
        1
    );
    let resumed = project((projected.1, false, None, 0, 32, 128, 4096))
        .await
        .unwrap();
    assert_eq!(resumed.0, [(2, 20), (3, 31), (4, 40), (5, 51), (6, 60)]);
    assert_eq!(resumed.4, 80);
    let second = query((first.1, false, Some(1), 40, 2, 1, 4096))
        .await
        .unwrap();
    assert!(second.0.is_empty());
    assert!(second.1.is_some());
    let third = query((second.1, false, Some(1), 40, 2, 1, 4096))
        .await
        .unwrap();
    assert_eq!(third.0, [(5, 51)]);
    assert_eq!(third.4, 24);
    assert!(
        query((third.1, false, Some(1), 40, 2, 1, 4096))
            .await
            .unwrap()
            .1
            .is_none()
    );
    let bytes = query((None, false, None, 0, 32, 128, 47)).await.unwrap();
    assert_eq!(bytes.0, [(1, 11)]);
    assert_eq!((bytes.2, bytes.3, bytes.4), (2, 2, 24));
    assert_eq!(
        query((bytes.1, false, None, 0, 32, 128, 4096))
            .await
            .unwrap()
            .0,
        [(2, 20), (3, 31), (4, 40), (5, 51), (6, 60)]
    );
    assert_eq!(
        query((None, false, None, 0, 32, 128, 23))
            .await
            .unwrap_err(),
        2
    );
    let descending = query((None, true, Some(1), 0, 32, 128, 4096))
        .await
        .unwrap();
    assert_eq!(descending.0, [(5, 51), (3, 31), (1, 11)]);
    assert!(descending.1.is_none());
    println!("fluent_descending_three_instructions={}", descending.5);
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let reply = pic
        .update_call(
            canister,
            canister,
            "replace_binary_object",
            native::encode_request(
                14,
                &(
                    (
                        ic_wire::ByteBuf::from(vec![7; 1024]),
                        None::<ic_wire::ByteBuf>,
                    ),
                    false,
                ),
                LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    native::decode_response::<Result<(), u8>>(14, &reply, LIMITS)
        .unwrap()
        .unwrap();
    let before = pic.get_stable_memory(canister).await;
    let binary = async |projected: bool| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "fluent_binary_projection",
                native::encode_request(97, &projected, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<(Vec<u64>, u32, u64)>(97, &reply, LIMITS).unwrap()
    };
    let full = binary(false).await;
    let projected = binary(true).await;
    assert_eq!(full.0, [1]);
    assert_eq!(full.0, projected.0);
    assert_eq!((full.1, projected.1), (1037, 8));
    println!("fluent_blob_full_instructions={}", full.2);
    println!("fluent_blob_projected_instructions={}", projected.2);
    assert_eq!(pic.get_stable_memory(canister).await, before);
    // Explicit result widening stays exact beyond the source's 64-bit range in Wasm.
    for id in 1_u64..=2 {
        let reply = pic
            .update_call(
                canister,
                canister,
                "write",
                native::encode_request(4, &(id, id % 2, u64::MAX, 0_u8), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<(), u8>>(4, &reply, LIMITS)
            .unwrap()
            .unwrap();
    }
    let before = pic.get_stable_memory(canister).await;
    let widened = aggregate("sum:amount", 1, 7, 16).await.unwrap();
    assert_eq!(
        widened.1,
        [vec![(u128::from(u64::MAX) * 2 + 182).to_string()]]
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let widened_groups = aggregate("category,sum:amount", 2, 7, 48).await.unwrap();
    assert_eq!(
        widened_groups.1,
        [
            vec!["0".to_string(), (u128::from(u64::MAX) + 100).to_string()],
            vec!["1".to_string(), (u128::from(u64::MAX) + 82).to_string()]
        ]
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    owned.close().await;
}

#[tokio::test]
async fn decimal_aggregates_preserve_scale_rounding_and_upgrade_behavior() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m1-records"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |values: Vec<String>| {
        let rows: Vec<_> = values
            .into_iter()
            .enumerate()
            .map(|(i, value)| (i as u64 + 1, None::<String>, value))
            .collect();
        let reply = pic
            .update_call(
                canister,
                canister,
                "decimal_batch",
                native::encode_request(67, &(rows, false), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            native::decode_response::<Result<(), u8>>(67, &reply, LIMITS).unwrap(),
            Ok(())
        );
    };
    let query = async |select: &str, minimum: Option<u64>| {
        let request = (
            "DecimalRow",
            None::<Vec<u8>>,
            1_u32,
            None::<u64>,
            None::<u64>,
            None::<u64>,
            Vec::<String>::new(),
            false,
            128_u32,
            4096_u32,
            select,
            (
                false,
                minimum,
                select.starts_with("zone,").then_some("desc"),
            ),
        );
        let reply = pic
            .query_call(
                canister,
                canister,
                "query",
                native::encode_request(3, &request, LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<DisplayPage, String>>(3, &reply, LIMITS).unwrap()
    };
    write(vec!["1.25".into(), "2.50".into(), "-0.75".into()]).await;
    for upgraded in [false, true] {
        if upgraded {
            pic.upgrade_canister(canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
        }
        let before = pic.get_stable_memory(canister).await;
        let sum = query("sum:value", None).await.unwrap();
        assert_eq!(sum.1, vec![vec!["3".to_owned()]]);
        assert_eq!(sum.5, 17);
        assert_eq!(
            query("avg:value", None).await.unwrap().1,
            vec![vec!["1".to_owned()]]
        );
        assert!(query("avg:maybe", None).await.unwrap().1.is_empty());
        assert_eq!(
            query("zone,avg:value", Some(1)).await.unwrap().1,
            vec![vec!["true".to_owned(), "1".to_owned()]]
        );
        assert!(query("zone,avg:value", Some(2)).await.unwrap().1.is_empty());
        assert_eq!(
            query("zone,avg:maybe", None).await.unwrap().1,
            vec![vec!["true".to_owned(), "—".to_owned()]]
        );
        assert!(query("sum:maybe", None).await.unwrap().1.is_empty());
        assert_eq!(
            query("zone,sum:value", Some(3)).await.unwrap().1,
            vec![vec!["true".to_owned(), "3".to_owned()]]
        );
        assert!(query("zone,sum:value", Some(4)).await.unwrap().1.is_empty());
        assert_eq!(
            query("zone,sum:maybe", None).await.unwrap().1,
            vec![vec!["true".to_owned(), "—".to_owned()]]
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    // Fixed scale 2: max coefficient + one cent - one cent cancels exactly.
    let maximum = "1701411834604692317316873037158841057.27";
    write(vec![maximum.into(), "0.01".into(), "-0.01".into()]).await;
    assert_eq!(
        query("sum:value", None).await.unwrap().1,
        vec![vec![maximum.to_owned()]]
    );
    write(vec![maximum.into(), "0.01".into(), "0".into()]).await;
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        query("sum:value", None).await.unwrap_err(),
        "aggregate exceeds its declared result range"
    );
    assert_eq!(
        query("zone,sum:value", None).await.unwrap_err(),
        "aggregate exceeds its declared result range"
    );
    assert_eq!(
        query("avg:value", None).await.unwrap().1,
        vec![vec!["567137278201564105772291012386280352.43".to_owned()]]
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for (values, expected) in [
        (["1.24", "1.25", "1.24", "1.25"], "1.24"),
        (["-1.25", "-1.26", "-1.25", "-1.26"], "-1.26"),
    ] {
        write(values.into_iter().map(str::to_owned).collect()).await;
        let before = pic.get_stable_memory(canister).await;
        assert_eq!(
            query("avg:value", None).await.unwrap().1,
            vec![vec![expected.to_owned()]]
        );
        assert_eq!(
            query("zone,avg:value", None).await.unwrap().1,
            vec![vec!["true".to_owned(), expected.to_owned()]]
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    owned.close().await;
}
