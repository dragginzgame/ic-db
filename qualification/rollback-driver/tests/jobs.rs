//! Product retained jobs: real Wasm rollback, retry, upgrade and explicit expiry.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::native;
type Response = (u64, u64, u64, u64, u8, Option<String>, u64);

#[tokio::test]
async fn retained_aggregates_rollback_and_resume_across_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-jobs")).unwrap();
    // Optional recorded predecessor qualifies old scalar checkpoints against this
    // executable. Without it, the same test proves same-code reopening.
    let initial = std::env::var_os("IC_DB_M2_SCALAR_JOBS_WASM")
        .map(|path| std::fs::read(path).unwrap())
        .unwrap_or_else(|| wasm.clone());
    let grouped_predecessor = std::env::var_os("IC_DB_M2_GROUP_COUNT_JOBS_WASM")
        .map(|path| std::fs::read(path).unwrap())
        .unwrap_or_else(|| wasm.clone());
    let distinct_predecessor = std::env::var_os("IC_DB_M2_DISTINCT_JOBS_WASM")
        .map(|path| std::fs::read(path).unwrap())
        .unwrap_or_else(|| wasm.clone());
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, initial, vec![], None).await;
    let start = async |operation: u8, fault: u64| {
        pic.update_call(
            canister,
            canister,
            "job_start",
            native::encode_request(110, &(operation, 0_i64, 100_i64, fault), LIMITS).unwrap(),
        )
        .await
    };
    let advance = async |handle: &Vec<u8>,
                         operation: u8,
                         sequence: u64,
                         now: i64,
                         fault: u64,
                         trap_after: bool| {
        pic.update_call(
            canister,
            canister,
            "job_advance",
            native::encode_request(
                111,
                &(handle, operation, sequence, 2_u32, now, fault, trap_after),
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let control = async |operation: u8, handle: &Vec<u8>, now: i64, limit: u32| {
        pic.update_call(
            canister,
            canister,
            "job_control",
            native::encode_request(112, &(operation, handle, now, limit), LIMITS).unwrap(),
        )
        .await
    };
    let decode = |bytes: Vec<u8>| {
        native::decode_response::<Result<Response, String>>(111, &bytes, LIMITS)
            .unwrap()
            .unwrap()
    };
    let handle = |bytes: Vec<u8>| {
        native::decode_response::<Result<Vec<u8>, String>>(110, &bytes, LIMITS)
            .unwrap()
            .unwrap()
    };
    let controlled = |bytes: Vec<u8>| {
        native::decode_response::<Result<u64, String>>(112, &bytes, LIMITS)
            .unwrap()
            .unwrap()
    };

    // Creation has both a generation/header write and a slot write. Traps restore
    // the complete backing allocation, including supplier/allocator metadata.
    let before = pic.get_stable_memory(canister).await;
    for fault in [1, 2, 3] {
        assert!(start(0, fault).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    let count = handle(start(0, 0).await.unwrap());
    let sum = handle(start(1, 0).await.unwrap());
    let average = handle(start(2, 0).await.unwrap());
    let first = decode(advance(&count, 0, 0, 1, 0, false).await.unwrap());
    assert_eq!((first.0, first.2, first.4, first.5), (1, 2, 0, None));
    let first_sum = decode(advance(&sum, 1, 0, 1, 0, false).await.unwrap());
    assert_eq!(
        (
            first_sum.0,
            first_sum.2,
            first_sum.4,
            first_sum.5.as_deref()
        ),
        (1, 2, 0, None)
    );
    assert!(first.6 > 0);
    let first_average = decode(advance(&average, 2, 0, 1, 0, false).await.unwrap());
    assert_eq!(
        (
            first_average.0,
            first_average.2,
            first_average.4,
            first_average.5
        ),
        (1, 2, 0, None)
    );

    // A replay rewrites the same-sized checkpoint; use its observed supplier
    // write count rather than assuming how many backing writes an update needs.
    let checkpoint_writes = decode(advance(&sum, 1, 0, 1, 0, false).await.unwrap()).6;
    assert!(checkpoint_writes > 0);
    let before = pic.get_stable_memory(canister).await;
    for fault in 1..=checkpoint_writes {
        assert!(advance(&sum, 1, 1, 2, fault, false).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    assert!(advance(&sum, 1, 1, 2, 0, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(advance(&average, 2, 1, 2, 1, false).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(advance(&average, 2, 1, 2, 0, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    // A failed source update must not advance the source revision either.
    assert!(control(5, &vec![], 0, 0).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);

    pic.upgrade_canister(canister, grouped_predecessor, vec![], None)
        .await
        .unwrap();
    let grouped = handle(start(3, 0).await.unwrap());
    let first_grouped = decode(advance(&grouped, 3, 0, 1, 0, false).await.unwrap());
    assert_eq!(
        (
            first_grouped.0,
            first_grouped.2,
            first_grouped.4,
            first_grouped.5
        ),
        (1, 2, 0, None)
    );
    let before = pic.get_stable_memory(canister).await;
    assert!(advance(&grouped, 3, 1, 2, 1, false).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(advance(&grouped, 3, 1, 2, 0, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    pic.upgrade_canister(canister, distinct_predecessor, vec![], None)
        .await
        .unwrap();
    let grouped_sum = handle(start(4, 0).await.unwrap());
    let grouped_average = handle(start(5, 0).await.unwrap());
    let distinct = handle(start(8, 0).await.unwrap());
    for (job, operation) in [(&grouped_sum, 4), (&grouped_average, 5), (&distinct, 8)] {
        let first = decode(advance(job, operation, 0, 1, 0, false).await.unwrap());
        assert_eq!((first.0, first.2, first.4, first.5), (1, 2, 0, None));
        let before = pic.get_stable_memory(canister).await;
        assert!(advance(job, operation, 1, 2, 1, false).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert!(advance(job, operation, 1, 2, 0, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    pic.upgrade_canister(canister, wasm.clone(), vec![], None)
        .await
        .unwrap();
    let group_distinct = handle(start(9, 0).await.unwrap());
    let first = decode(advance(&group_distinct, 9, 0, 1, 0, false).await.unwrap());
    assert_eq!((first.0, first.2, first.4, first.5), (1, 2, 0, None));
    let before = pic.get_stable_memory(canister).await;
    assert!(advance(&group_distinct, 9, 1, 2, 1, false).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(advance(&group_distinct, 9, 1, 2, 0, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    for (job, operation) in [
        (&grouped_sum, 4),
        (&grouped_average, 5),
        (&distinct, 8),
        (&group_distinct, 9),
    ] {
        let retry = decode(advance(job, operation, 0, 1, 0, false).await.unwrap());
        assert_eq!((retry.0, retry.2, retry.4, retry.5), (1, 2, 0, None));
    }
    let retry = decode(advance(&sum, 1, 0, 1, 0, false).await.unwrap());
    let retry_average = decode(advance(&average, 2, 0, 1, 0, false).await.unwrap());
    let retry_grouped = decode(advance(&grouped, 3, 0, 1, 0, false).await.unwrap());
    assert_eq!(
        (
            retry_grouped.0,
            retry_grouped.2,
            retry_grouped.4,
            retry_grouped.5
        ),
        (1, 2, 0, None)
    );
    assert_eq!(
        (
            retry_average.0,
            retry_average.2,
            retry_average.4,
            retry_average.5
        ),
        (1, 2, 0, None)
    );
    assert_eq!(
        (retry.0, retry.1, retry.2, retry.3, retry.4, retry.5),
        (
            first_sum.0,
            first_sum.1,
            first_sum.2,
            first_sum.3,
            first_sum.4,
            first_sum.5
        )
    );
    controlled(control(3, &vec![], 0, 0).await.unwrap()); // Noop source replacement.
    controlled(control(4, &vec![], 0, 0).await.unwrap()); // Another entity may change.
    for sequence in 1..=3 {
        let c = decode(advance(&count, 0, sequence, 2, 0, false).await.unwrap());
        let s = decode(advance(&sum, 1, sequence, 2, 0, false).await.unwrap());
        let a = decode(advance(&average, 2, sequence, 2, 0, false).await.unwrap());
        let g = decode(advance(&grouped, 3, sequence, 2, 0, false).await.unwrap());
        let gs = decode(
            advance(&grouped_sum, 4, sequence, 2, 0, false)
                .await
                .unwrap(),
        );
        let ga = decode(
            advance(&grouped_average, 5, sequence, 2, 0, false)
                .await
                .unwrap(),
        );
        let d = decode(advance(&distinct, 8, sequence, 2, 0, false).await.unwrap());
        let gd = decode(
            advance(&group_distinct, 9, sequence, 2, 0, false)
                .await
                .unwrap(),
        );
        if sequence < 3 {
            assert_eq!((c.4, s.4, a.4, g.4), (0, 0, 0, 0));
            assert_eq!((gs.4, ga.4, d.4, gd.4), (0, 0, 0, 0));
        } else {
            assert_eq!((c.2, c.4, c.5), (6, 1, Some("6".into())));
            let expected: u64 = (1..=6).map(|id| id * 10 + id % 2).sum();
            assert_eq!((s.2, s.4, s.5), (6, 1, Some(expected.to_string())));
            assert_eq!((a.2, a.4, a.5), (6, 1, Some("35.5".into())));
            assert_eq!((g.2, g.4, g.5), (6, 1, Some("0:3,1:3".into())));
            assert_eq!((gs.2, gs.4, gs.5), (6, 1, Some("0:120,1:93".into())));
            assert_eq!((ga.2, ga.4, ga.5), (6, 1, Some("0:40,1:31".into())));
            assert_eq!((d.2, d.4, d.5), (6, 1, Some("2".into())));
            assert_eq!((gd.2, gd.4, gd.5), (6, 1, Some("0:1,1:1".into())));
        }
    }
    let live = handle(start(2, 0).await.unwrap());
    decode(advance(&live, 2, 0, 1, 0, false).await.unwrap());
    controlled(control(2, &vec![], 0, 0).await.unwrap());
    let invalidated = decode(advance(&live, 2, 0, 2, 0, false).await.unwrap());
    assert_eq!((invalidated.4, invalidated.5), (2, None));
    // A completed result keeps its captured boundary after later source writes.
    assert_eq!(
        decode(advance(&sum, 1, 0, 2, 0, false).await.unwrap()).5,
        Some("213".into())
    );
    assert_eq!(
        decode(advance(&average, 2, 0, 2, 0, false).await.unwrap()).5,
        Some("35.5".into())
    );
    let expired = decode(advance(&sum, 1, 0, 100, 0, false).await.unwrap());
    assert_eq!((expired.4, expired.5), (3, None));
    assert_eq!(controlled(control(1, &vec![], 100, 1).await.unwrap()), 1);
    assert_eq!(
        decode(advance(&grouped, 3, 0, 2, 0, false).await.unwrap()).5,
        Some("0:3,1:3".into())
    );
    assert_eq!(controlled(control(1, &vec![], 100, 16).await.unwrap()), 8);
    let replacement = handle(start(0, 0).await.unwrap());
    assert_ne!(replacement, count);
    let stale = control(0, &count, 0, 0).await.unwrap();
    assert!(
        native::decode_response::<Result<u64, String>>(112, &stale, LIMITS)
            .unwrap()
            .is_err()
    );
    controlled(control(0, &replacement, 0, 0).await.unwrap());
}

#[tokio::test]
async fn present_field_jobs_preserve_zero_counts_across_rollback_and_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-jobs")).unwrap();
    let initial = std::env::var_os("IC_DB_M2_FIELD_COUNT_JOBS_WASM")
        .map(|path| std::fs::read(path).unwrap())
        .unwrap_or_else(|| wasm.clone());
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, initial, vec![], None).await;
    let advance =
        async |handle: &Vec<u8>, operation: u8, sequence: u64, fault: u64, trap_after: bool| {
            pic.update_call(
                canister,
                canister,
                "job_advance",
                native::encode_request(
                    111,
                    &(handle, operation, sequence, 1_u32, 1_i64, fault, trap_after),
                    LIMITS,
                )
                .unwrap(),
            )
            .await
        };
    let decode = |bytes: Vec<u8>| {
        native::decode_response::<Result<Response, String>>(111, &bytes, LIMITS)
            .unwrap()
            .unwrap()
    };
    let mut jobs = Vec::new();
    for operation in [6_u8, 7] {
        let response = pic
            .update_call(
                canister,
                canister,
                "job_start",
                native::encode_request(110, &(operation, 0_i64, 100_i64, 0_u64), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        let handle = native::decode_response::<Result<Vec<u8>, String>>(110, &response, LIMITS)
            .unwrap()
            .unwrap();
        let before = pic.get_stable_memory(canister).await;
        assert!(advance(&handle, operation, 0, 1, false).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert!(advance(&handle, operation, 0, 0, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let step = decode(advance(&handle, operation, 0, 0, false).await.unwrap());
        assert_eq!((step.0, step.2, step.4, step.5), (1, 1, 0, None));
        jobs.push((operation, handle));
    }
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    for (operation, handle) in jobs {
        let retry = decode(advance(&handle, operation, 0, 0, false).await.unwrap());
        assert_eq!((retry.0, retry.2, retry.4, retry.5), (1, 1, 0, None));
        let complete = decode(advance(&handle, operation, 1, 0, false).await.unwrap());
        assert_eq!(
            (complete.2, complete.4, complete.5.as_deref()),
            (1, 1, Some(if operation == 6 { "0" } else { "1:0" }))
        );
    }
}

#[tokio::test]
async fn completed_group_pages_survive_traps_upgrade_and_source_changes() {
    type PageResponse = (u64, u8, Vec<(u64, u64)>, Option<Vec<u8>>, u64);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-jobs")).unwrap();
    let predecessor = std::env::var_os("IC_DB_M2_GROUP_PAGES_PREDECESSOR_WASM")
        .map(|path| std::fs::read(path).unwrap())
        .unwrap_or_else(|| wasm.clone());
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, predecessor, vec![], None)
        .await;
    let start = async || {
        let response = pic
            .update_call(
                canister,
                canister,
                "job_start",
                native::encode_request(110, &(3_u8, 0_i64, 100_i64, 0_u64), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Vec<u8>, String>>(110, &response, LIMITS)
            .unwrap()
            .unwrap()
    };
    let handle = start().await;
    let response = pic
        .update_call(
            canister,
            canister,
            "job_advance",
            native::encode_request(
                111,
                &(&handle, 3_u8, 0_u64, 128_u32, 1_i64, 0_u64, false),
                LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let old = native::decode_response::<Result<Response, String>>(111, &response, LIMITS)
        .unwrap()
        .unwrap();
    assert_eq!((old.4, old.5.as_deref()), (1, Some("0:3,1:3")));
    pic.upgrade_canister(canister, wasm.clone(), vec![], None)
        .await
        .unwrap();
    let page = async |handle: &Vec<u8>,
                      sequence: u64,
                      cursor: Option<Vec<u8>>,
                      now: i64,
                      fault: u64,
                      trap_after: bool| {
        pic.update_call(
            canister,
            canister,
            "job_page",
            native::encode_request(
                113,
                &(
                    handle, sequence, cursor, 1_u32, 100_u32, now, fault, trap_after,
                ),
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let decode = |bytes: Vec<u8>| {
        native::decode_response::<Result<PageResponse, String>>(113, &bytes, LIMITS)
            .unwrap()
            .unwrap()
    };
    // A new job's final input progress and first output page share rollback.
    let pending = start().await;
    let before = pic.get_stable_memory(canister).await;
    assert!(page(&pending, 0, None, 1, 1, false).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(page(&pending, 0, None, 1, 0, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let fresh = decode(page(&pending, 0, None, 1, 0, false).await.unwrap());
    assert_eq!((fresh.1, fresh.2), (1, vec![(0, 3)]));
    // A job completed by the predecessor can be paged without rewriting its format.
    let first = decode(page(&handle, old.0, None, 2, 0, false).await.unwrap());
    assert_eq!(
        (first.0, first.1, first.2, first.4),
        (old.0, 1, vec![(0, 3)], old.1)
    );
    let cursor = first.3.unwrap();
    let before = pic.get_stable_memory(canister).await;
    assert!(
        page(&handle, old.0, Some(cursor.clone()), 3, 0, true)
            .await
            .is_err()
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    pic.update_call(
        canister,
        canister,
        "job_control",
        native::encode_request(112, &(2_u8, Vec::<u8>::new(), 0_i64, 0_u32), LIMITS).unwrap(),
    )
    .await
    .unwrap();
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    for _ in 0..2 {
        let last = decode(
            page(&handle, old.0, Some(cursor.clone()), 3, 0, false)
                .await
                .unwrap(),
        );
        assert_eq!(last, (old.0, 1, vec![(1, 3)], None, old.1));
    }
    let expired = decode(
        page(&handle, old.0, Some(cursor), 100, 0, false)
            .await
            .unwrap(),
    );
    assert_eq!((expired.1, expired.2, expired.3), (3, vec![], None));
}

#[tokio::test]
async fn retained_extrema_rollback_and_preserve_winners_across_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-jobs")).unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let start = async |operation: u8| {
        let bytes = pic
            .update_call(
                canister,
                canister,
                "job_start",
                native::encode_request(110, &(operation, 0_i64, 100_i64, 0_u64), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Vec<u8>, String>>(110, &bytes, LIMITS)
            .unwrap()
            .unwrap()
    };
    let advance =
        async |handle: &Vec<u8>, operation: u8, sequence: u64, fault: u64, trap_after: bool| {
            pic.update_call(
                canister,
                canister,
                "job_advance",
                native::encode_request(
                    111,
                    &(handle, operation, sequence, 2_u32, 1_i64, fault, trap_after),
                    LIMITS,
                )
                .unwrap(),
            )
            .await
        };
    let decode = |bytes: Vec<u8>| {
        native::decode_response::<Result<Response, String>>(111, &bytes, LIMITS)
            .unwrap()
            .unwrap()
    };
    let min = start(10).await;
    let max = start(11).await;
    for (handle, op) in [(&min, 10), (&max, 11)] {
        let first = decode(advance(handle, op, 0, 0, false).await.unwrap());
        assert_eq!((first.0, first.2, first.4, first.5), (1, 2, 0, None));
        let before = pic.get_stable_memory(canister).await;
        assert!(advance(handle, op, 1, 1, false).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert!(advance(handle, op, 1, 0, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let retry = decode(advance(handle, op, 0, 0, false).await.unwrap());
        assert_eq!((retry.0, retry.2, retry.4), (1, 2, 0));
    }
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    let mut completed = Vec::new();
    for (handle, op, expected) in [(&min, 10, "11"), (&max, 11, "60")] {
        let mut sequence = 1;
        loop {
            let step = decode(advance(handle, op, sequence, 0, false).await.unwrap());
            sequence = step.0;
            if step.4 == 1 {
                assert_eq!((step.2, step.5.as_deref()), (6, Some(expected)));
                completed.push((handle, op, step.0, step.1, expected));
                break;
            }
            assert_eq!(step.4, 0);
            assert!(sequence < 6);
        }
    }
    let running = start(10).await;
    decode(advance(&running, 10, 0, 0, false).await.unwrap());
    let bytes = pic
        .update_call(
            canister,
            canister,
            "job_control",
            native::encode_request(112, &(2_u8, Vec::<u8>::new(), 0_i64, 0_u32), LIMITS).unwrap(),
        )
        .await
        .unwrap();
    native::decode_response::<Result<u64, String>>(112, &bytes, LIMITS)
        .unwrap()
        .unwrap();
    assert_eq!(
        decode(advance(&running, 10, 1, 0, false).await.unwrap()).4,
        2
    );
    for (handle, op, sequence, revision, expected) in completed {
        let step = decode(advance(handle, op, sequence, 0, false).await.unwrap());
        assert_eq!(
            (step.0, step.1, step.4, step.5.as_deref()),
            (sequence, revision, 1, Some(expected))
        );
    }
}

#[tokio::test]
async fn retained_group_extrema_rollback_upgrade_and_page_completed_winners() {
    type Response = (u64, u8, Vec<(u64, Option<u64>)>, Option<Vec<u8>>, u64, u64);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-jobs")).unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let start = async |operation: u8| {
        let bytes = pic
            .update_call(
                canister,
                canister,
                "job_start",
                native::encode_request(110, &(operation, 0_i64, 100_i64, 0_u64), LIMITS).unwrap(),
            )
            .await
            .unwrap();
        native::decode_response::<Result<Vec<u8>, String>>(110, &bytes, LIMITS)
            .unwrap()
            .unwrap()
    };
    let advance = async |handle: &Vec<u8>,
                         op: u8,
                         sequence: u64,
                         after: Option<Vec<u8>>,
                         now: i64,
                         fault: u64,
                         trap_after: bool| {
        pic.update_call(
            canister,
            canister,
            "job_group_extremum",
            native::encode_request(
                114,
                &(
                    handle, op, sequence, 2_u32, after, 1_u32, now, fault, trap_after,
                ),
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let decode = |bytes: Vec<u8>| {
        native::decode_response::<Result<Response, String>>(114, &bytes, LIMITS)
            .unwrap()
            .unwrap()
    };
    let min = start(12).await;
    let max = start(13).await;
    for (handle, op) in [(&min, 12), (&max, 13)] {
        let step = decode(advance(handle, op, 0, None, 1, 0, false).await.unwrap());
        assert_eq!((step.0, step.1, step.5), (1, 0, 2));
        assert!(step.2.is_empty());
        let before = pic.get_stable_memory(canister).await;
        assert!(advance(handle, op, 1, None, 2, 1, false).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert!(advance(handle, op, 1, None, 2, 0, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let retry = decode(advance(handle, op, 0, None, 1, 0, false).await.unwrap());
        assert_eq!((retry.0, retry.1, retry.5), (1, 0, 2));
    }
    pic.upgrade_canister(canister, wasm.clone(), vec![], None)
        .await
        .unwrap();
    let mut completed = Vec::new();
    for (handle, op, first, second) in [(&min, 12, 20, 11), (&max, 13, 60, 51)] {
        let mut sequence = 1;
        loop {
            let step = decode(
                advance(handle, op, sequence, None, 2, 0, false)
                    .await
                    .unwrap(),
            );
            sequence = step.0;
            if step.1 == 1 {
                assert_eq!(step.2, vec![(0, Some(first))]);
                assert_eq!(step.5, 6);
                completed.push((handle, op, sequence, step.3.unwrap(), step.4, second));
                break;
            }
            assert_eq!(step.1, 0);
            assert!(sequence < 6);
        }
    }
    let running = start(12).await;
    decode(advance(&running, 12, 0, None, 2, 0, false).await.unwrap());
    let bytes = pic
        .update_call(
            canister,
            canister,
            "job_control",
            native::encode_request(112, &(2_u8, Vec::<u8>::new(), 0_i64, 0_u32), LIMITS).unwrap(),
        )
        .await
        .unwrap();
    native::decode_response::<Result<u64, String>>(112, &bytes, LIMITS)
        .unwrap()
        .unwrap();
    assert_eq!(
        decode(advance(&running, 12, 1, None, 3, 0, false).await.unwrap()).1,
        2
    );
    pic.upgrade_canister(canister, wasm, vec![], None)
        .await
        .unwrap();
    for (handle, op, sequence, cursor, revision, expected) in completed {
        let before = pic.get_stable_memory(canister).await;
        assert!(
            advance(handle, op, sequence, Some(cursor.clone()), 3, 0, true)
                .await
                .is_err()
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
        for _ in 0..2 {
            let step = decode(
                advance(handle, op, sequence, Some(cursor.clone()), 3, 0, false)
                    .await
                    .unwrap(),
            );
            assert_eq!((step.0, step.1, step.4, step.5), (sequence, 1, revision, 6));
            assert_eq!(step.2, vec![(1, Some(expected))]);
            assert!(step.3.is_none());
        }
        assert_eq!(
            decode(
                advance(handle, op, sequence, Some(cursor), 100, 0, false)
                    .await
                    .unwrap()
            )
            .1,
            3
        );
    }
}
