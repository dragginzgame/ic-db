//! Controlled write schedules and shared retained-job capacity, without hidden retries.

mod wire_frame;
use ic_db_m0_rollback_driver::instance;
use wire_frame::{request, response};

type Step = (u64, u64, u64, u64, u8, Option<String>, u64);

#[tokio::test]
async fn shared_jobs_preserve_isolation_capacity_and_stale_handle_refusal() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-contention"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let start = async |operation: u8| {
        let bytes = pic
            .update_call(
                canister,
                canister,
                "job_start",
                request(110, &(operation, 0_i64, 100_i64, 0_u64)),
            )
            .await
            .unwrap();

        response::<Result<Vec<u8>, String>>(110, &bytes)
    };
    let advance = async |handle: &[u8], operation: u8, sequence: u64, work: u32| {
        let bytes = pic
            .update_call(
                canister,
                canister,
                "job_advance",
                request(
                    111,
                    &(handle, operation, sequence, work, 1_i64, 0_u64, false),
                ),
            )
            .await
            .unwrap();

        response::<Result<Step, String>>(111, &bytes)
    };
    let control = async |operation: u8, handle: &[u8], now: i64, limit: u32| {
        let bytes = pic
            .update_call(
                canister,
                canister,
                "job_control",
                request(112, &(operation, handle, now, limit)),
            )
            .await
            .unwrap();

        response::<Result<u64, String>>(112, &bytes)
    };
    let upgrade = async || {
        let before = pic.get_stable_memory(canister).await;
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(pic.get_stable_memory(canister).await, before);
    };

    // Four query shapes over two source entities share all 16 logical slots.
    // Keep most jobs unfinished so one entity mutation can distinguish isolation
    // from a global "anything changed" counter, including after heap loss.
    let mut jobs = Vec::new();
    for operation in [4_u8, 6, 3, 9].into_iter().cycle().take(16) {
        jobs.push((operation, start(operation).await.unwrap(), 0_u64));
    }
    let full = pic.get_stable_memory(canister).await;
    assert_eq!(start(4).await, Err("JobCapacity".into()));
    assert_eq!(pic.get_stable_memory(canister).await, full);
    for (operation, handle, sequence) in &mut jobs {
        let step = advance(handle, *operation, *sequence, 1).await.unwrap();
        let expected = if *operation == 6 {
            (1, 1, Some("0"))
        } else {
            (1, 0, None)
        };
        assert_eq!((step.2, step.4, step.5.as_deref()), expected);
        *sequence = step.0;
    }
    // Complete one grouped sum and retain its historical result. Expected values
    // come from fixture tuples, not the database's reducer or row decoder.
    let expected = format!(
        "0:{},1:{}",
        [20_u64, 40, 60].iter().sum::<u64>(),
        [11_u64, 31, 51].iter().sum::<u64>()
    );
    let complete = advance(&jobs[0].1, 4, jobs[0].2, 32).await.unwrap();
    assert_eq!(
        (complete.2, complete.4, complete.5.as_deref()),
        (6, 1, Some(expected.as_str()))
    );
    jobs[0].2 = complete.0;
    upgrade().await;
    let full = pic.get_stable_memory(canister).await;
    assert_eq!(start(6).await, Err("JobCapacity".into()));
    assert_eq!(pic.get_stable_memory(canister).await, full);
    assert_eq!(
        advance(&jobs[0].1, 6, jobs[0].2, 1).await.unwrap_err(),
        "InvalidJob"
    );
    assert_eq!(pic.get_stable_memory(canister).await, full);

    // Release one occupied slot, refill it, then try both operations with the old
    // handle. A stale cancel must never remove the replacement occupant.
    let stale = jobs[3].clone();
    assert_eq!(control(0, &stale.1, 0, 0).await, Ok(0));
    jobs[3] = (6, start(6).await.unwrap(), 0);
    assert_ne!(jobs[3].1, stale.1);
    let replaced = pic.get_stable_memory(canister).await;
    assert_eq!(
        advance(&stale.1, stale.0, stale.2, 1).await.unwrap_err(),
        "InvalidJob"
    );
    assert_eq!(pic.get_stable_memory(canister).await, replaced);
    assert_eq!(control(0, &stale.1, 0, 0).await, Err("InvalidJob".into()));
    assert_eq!(pic.get_stable_memory(canister).await, replaced);

    // A committed Entry change invalidates unfinished Entry jobs only.
    assert_eq!(control(2, &[], 0, 0).await, Ok(complete.1 + 1));
    upgrade().await;
    for (index, (operation, handle, sequence)) in jobs.iter_mut().enumerate() {
        let step = advance(handle, *operation, *sequence, 32).await.unwrap();
        if index == 0 {
            assert_eq!(
                (step.1, step.4, step.5.as_deref()),
                (complete.1, 1, Some(expected.as_str()))
            );
        } else if *operation == 6 {
            // The fixture's one Document has note=None, so its present-value count is zero.
            assert_eq!((step.2, step.4, step.5.as_deref()), (1, 1, Some("0")));
        } else {
            assert_eq!((step.1, step.2, step.4, step.5), (complete.1, 1, 2, None));
        }
        *sequence = step.0;
    }
    // Completed and invalidated jobs retain custody until explicit cancellation.
    let full = pic.get_stable_memory(canister).await;
    assert_eq!(start(4).await, Err("JobCapacity".into()));
    assert_eq!(pic.get_stable_memory(canister).await, full);
    for (_, handle, _) in &jobs {
        assert_eq!(control(0, handle, 0, 0).await, Ok(0));
    }

    // Refill every slot with current-source groups. Old generations cannot alias
    // any new occupant, and every new job completes with the changed exact sum.
    let mut fresh = Vec::new();
    for _ in 0..16 {
        fresh.push(start(4).await.unwrap());
    }
    let refilled = pic.get_stable_memory(canister).await;
    for (operation, handle, sequence) in &jobs {
        assert!(advance(handle, *operation, *sequence, 32).await.is_err());
        assert!(control(0, handle, 0, 0).await.is_err());
    }
    assert_eq!(pic.get_stable_memory(canister).await, refilled);
    for handle in &fresh {
        let step = advance(handle, 4, 0, 32).await.unwrap();
        assert_eq!(
            (step.1, step.2, step.4, step.5.as_deref()),
            (complete.1 + 1, 6, 1, Some("0:120,1:94"))
        );
    }
    // Expiry cleanup has a per-call removal bound, also after an upgrade. It
    // releases logical capacity; no claim about physical shrinking is made.
    upgrade().await;
    let mut removed = 0;
    for expected in [3, 3, 3, 3, 3, 1, 0] {
        let count = control(1, &[], 100, 3).await.unwrap();
        assert_eq!(count, expected);
        removed += count;
    }
    assert_eq!(removed, 16);
    assert!(advance(&fresh[0], 4, 0, 32).await.is_err());
    let next = start(4).await.unwrap();
    assert!(!fresh.contains(&next));
    assert_eq!(control(0, &next, 0, 0).await, Ok(0));
    eprintln!(
        "shared jobs: 16 slots, four query shapes, two entities, three upgrades; isolated invalidation, stale handles and bounded cleanup passed"
    );
    owned.drop().await;
}

#[tokio::test]
async fn retained_sum_requires_a_quiet_source_boundary_not_a_quiet_database() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-contention"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let write = async |id: u64, amount: u64, mode: u8| {
        let reply = pic
            .update_call(
                canister,
                canister,
                "write",
                request(4, &(id, id % 2, amount, mode)),
            )
            .await
            .unwrap();

        response::<Result<(), u8>>(4, &reply)
    };
    let start = async || {
        let reply = pic
            .update_call(
                canister,
                canister,
                "job_start",
                request(110, &(1_u8, 0_i64, 100_i64, 0_u64)),
            )
            .await
            .unwrap();

        response::<Result<Vec<u8>, String>>(110, &reply).unwrap()
    };
    let advance = async |handle: &[u8], sequence: u64, work: u32| {
        let reply = pic
            .update_call(
                canister,
                canister,
                "job_advance",
                request(111, &(handle, 1_u8, sequence, work, 1_i64, 0_u64, false)),
            )
            .await
            .unwrap();

        response::<Result<Step, String>>(111, &reply)
    };
    let control = async |operation: u8, handle: &[u8]| {
        pic.update_call(
            canister,
            canister,
            "job_control",
            request(112, &(operation, handle, 0_i64, 0_u32)),
        )
        .await
    };
    let controlled = |reply: Vec<u8>| response::<Result<u64, String>>(112, &reply).unwrap();
    for id in 7..=96 {
        assert_eq!(write(id, id * 10 + id % 2, 1).await, Ok(()));
    }
    let base: u64 = (1..=96).map(|id| id * 10 + id % 2).sum();

    // An independent tuple sum must complete despite every intervening message
    // being a write attempt. Neither job checkpoints nor other entities may
    // invalidate the source. Traps and refused writes preserve complete backing.
    for (name, operation) in [
        ("quiet", 0),
        ("unrelated", 4),
        ("noop", 3),
        ("trapped", 5),
        ("refused", 6),
    ] {
        let handle = start().await;
        let mut sequence = 0;
        let mut revision = None;
        for call in 1..=8 {
            let step = advance(&handle, sequence, 12).await.unwrap();
            assert_eq!(step.1, *revision.get_or_insert(step.1));
            assert_eq!(step.2, (call * 12).min(96));
            sequence = step.0;
            if call == 8 {
                assert_eq!((step.4, step.5), (1, Some(base.to_string())));
                break;
            }
            assert_eq!((step.4, step.5), (0, None));

            match operation {
                0 => {}
                5 | 6 => {
                    let before = pic.get_stable_memory(canister).await;
                    if operation == 5 {
                        assert!(control(5, &[]).await.is_err());
                    } else {
                        assert!(write(1, 999, 1).await.is_err());
                    }
                    assert_eq!(pic.get_stable_memory(canister).await, before);
                }
                _ => assert_eq!(controlled(control(operation, &[]).await.unwrap()), step.1),
            }
        }
        controlled(control(0, &handle).await.unwrap());
        assert!(advance(&handle, sequence, 12).await.is_err());
        eprintln!("schedule={name} rows=96 work=12 advances=8 outcome=complete sum={base}");
    }

    // Start at each of four offsets before the next committed source write.
    for work in [12_u32, 48] {
        let mut completions = 0;
        for calls_before_write in 1..=4 {
            let handle = start().await;
            let mut sequence = 0;
            for _ in 0..calls_before_write {
                let step = advance(&handle, sequence, work).await.unwrap();
                sequence = step.0;
                if step.4 == 1 {
                    assert_eq!((step.2, step.5), (96, Some(base.to_string())));
                    break;
                }
                assert_eq!((step.4, step.5), (0, None));
            }
            assert_eq!(write(1, 12, 0).await, Ok(()));
            let outcome = advance(&handle, sequence, work).await.unwrap();
            if calls_before_write * work >= 96 {
                assert_eq!(
                    (outcome.2, outcome.4, outcome.5),
                    (96, 1, Some(base.to_string()))
                );
                completions += 1;
            } else {
                assert_eq!(
                    (outcome.2, outcome.4, outcome.5),
                    (u64::from(calls_before_write * work), 2, None)
                );
            }
            controlled(control(0, &handle).await.unwrap());
            assert_eq!(write(1, 11, 0).await, Ok(()));
        }
        eprintln!(
            "schedule=four-source-write-offsets rows=96 work={work} completed={completions}/4"
        );
    }

    // Three caller-chosen restarts under writes all invalidate. Never report a
    // prefix sum or silently restart the existing handle; explicitly cancel it.
    // A later quiet interval allows a fresh attempt to finish.
    let mut changes = 0;
    let mut previous_revision = None;
    for attempt in 0..3 {
        let handle = start().await;
        let first = advance(&handle, 0, 12).await.unwrap();
        assert_eq!((first.2, first.4, first.5), (12, 0, None));
        if let Some(previous) = previous_revision {
            assert!(first.1 > previous);
        }
        previous_revision = Some(first.1);
        assert_eq!(controlled(control(2, &[]).await.unwrap()), first.1 + 1);
        changes += 1;
        if attempt == 1 {
            // Restoration is another committed change, not an unchanged boundary.
            assert_eq!(write(1, 11 + changes - 1, 0).await, Ok(()));
            changes -= 1;
            pic.advance_time(std::time::Duration::from_secs(600)).await;
            pic.tick().await;
            ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
        }
        let invalid = advance(&handle, first.0, 12).await.unwrap();
        assert_eq!(
            (invalid.1, invalid.2, invalid.4, &invalid.5),
            (first.1, 12, 2, &None)
        );
        let retry = advance(&handle, 0, 12).await.unwrap();
        assert_eq!(
            (retry.0, retry.1, retry.2, retry.4, retry.5),
            (invalid.0, invalid.1, invalid.2, 2, None)
        );
        controlled(control(0, &handle).await.unwrap());
        assert!(advance(&handle, 0, 12).await.is_err());
        eprintln!(
            "schedule=source-write attempt={} examined=12 outcome=invalidated",
            attempt + 1
        );
    }
    let handle = start().await;
    let mut sequence = 0;
    let mut captured_revision = 0;
    for call in 1..=8 {
        let step = advance(&handle, sequence, 12).await.unwrap();
        sequence = step.0;
        captured_revision = step.1;

        if call == 8 {
            assert_eq!(
                (step.2, step.4, step.5),
                (96, 1, Some((base + changes).to_string()))
            );
        } else {
            assert_eq!((step.4, step.5), (0, None));
        }
    }
    controlled(control(2, &[]).await.unwrap());
    let historical = advance(&handle, sequence, 12).await.unwrap();
    assert_eq!(
        (historical.1, historical.4, historical.5),
        (captured_revision, 1, Some((base + changes).to_string()))
    );
    controlled(control(0, &handle).await.unwrap());
    eprintln!(
        "schedule=quiet-after-writes advances=8 outcome=complete sum={}",
        base + changes
    );
    owned.drop().await;
}
