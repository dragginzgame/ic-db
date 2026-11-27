//! Product retained jobs: real Wasm rollback, retry, upgrade and explicit expiry.

mod wire_call;
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

use wire_call::wire_frame;
mod job_step;
use job_step::Step as Response;

#[tokio::test]
async fn grouped_or_selection_preserves_rollback_binding_and_upgrade() {
    check_job_binding_across_upgrade(14, 15, "0:3,1:3").await;
}

#[tokio::test]
async fn retained_field_comparison_binds_the_right_field_across_upgrade() {
    // Entry ids are 1..=6, category is id % 2. Only id 1 fails id > category.
    check_job_binding_across_upgrade(18, 19, "5").await;
}

#[tokio::test]
async fn retained_mixed_or_rejects_conjunction_and_resumes_across_upgrade() {
    // id == category selects 1; category == 0 selects 2, 4 and 6.
    check_job_binding_across_upgrade(20, 21, "4").await;
}

async fn check_job_binding_across_upgrade(operation: u8, changed: u8, expected: &str) {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-jobs")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let reply = pic
        .update_call(
            canister,
            canister,
            "job_start",
            wire_frame::request(110, &(operation, 0_i64, 100_i64, 0_u64)),
        )
        .await
        .unwrap();
    let handle = wire_frame::response::<Result<Vec<u8>, String>>(110, &reply).unwrap();
    let advance = async |operation: u8, sequence: u64, fault: u64| {
        pic.update_call(
            canister,
            canister,
            "job_advance",
            wire_frame::request(
                111,
                &(&handle, operation, sequence, 2_u32, 1_i64, fault, false),
            ),
        )
        .await
    };
    let decode = |bytes: Vec<u8>| wire_frame::response::<Result<Response, String>>(111, &bytes);
    let first = decode(advance(operation, 0, 0).await.unwrap()).unwrap();
    assert_eq!((first.sequence, first.examined, first.phase), (1, 2, 0));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        decode(advance(changed, first.sequence, 0).await.unwrap()).unwrap_err(),
        "InvalidJob"
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(advance(operation, first.sequence, 1).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    let mut sequence = first.sequence;
    for _ in 0..4 {
        let step = decode(advance(operation, sequence, 0).await.unwrap()).unwrap();
        sequence = step.sequence;
        if step.phase == 1 {
            assert_eq!(step.examined, 6);
            assert_eq!(step.value.as_deref(), Some(expected));
            owned.drop().await;

            return;
        }
        assert_eq!(step.phase, 0);
    }
    panic!("retained operation {operation} did not finish within its work bound");
}

#[tokio::test]
async fn retained_aggregates_rollback_and_resume_across_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-jobs")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let start = async |operation: u8, fault: u64| {
        pic.update_call(
            canister,
            canister,
            "job_start",
            wire_frame::request(110, &(operation, 0_i64, 100_i64, fault)),
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
            wire_frame::request(
                111,
                &(handle, operation, sequence, 2_u32, now, fault, trap_after),
            ),
        )
        .await
    };
    let control = async |operation: u8, handle: &Vec<u8>, now: i64, limit: u32| {
        pic.update_call(
            canister,
            canister,
            "job_control",
            ic_wire::encode(
                &(operation, handle, now, limit),
                Encoding::Frame {
                    method: 112,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let decode =
        |bytes: Vec<u8>| wire_frame::response::<Result<Response, String>>(111, &bytes).unwrap();
    let handle =
        |bytes: Vec<u8>| wire_frame::response::<Result<Vec<u8>, String>>(110, &bytes).unwrap();
    let controlled = |bytes: Vec<u8>| {
        ic_wire::decode::<Result<u64, String>>(
            &bytes,
            Encoding::Frame {
                method: 112,
                direction: Direction::Response,
            },
            LIMITS,
        )
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
    assert_eq!(
        (first.sequence, first.examined, first.phase, first.value),
        (1, 2, 0, None)
    );
    let first_sum = decode(advance(&sum, 1, 0, 1, 0, false).await.unwrap());
    assert_eq!(
        (
            first_sum.sequence,
            first_sum.examined,
            first_sum.phase,
            first_sum.value.as_deref()
        ),
        (1, 2, 0, None)
    );
    assert!(first.writes > 0);
    let first_average = decode(advance(&average, 2, 0, 1, 0, false).await.unwrap());
    assert_eq!(
        (
            first_average.sequence,
            first_average.examined,
            first_average.phase,
            first_average.value
        ),
        (1, 2, 0, None)
    );

    // Interrupt successive writes on the same slot until its forward step completes.
    let probe = handle(start(1, 0).await.unwrap());
    decode(advance(&probe, 1, 0, 1, 0, false).await.unwrap());
    let before = pic.get_stable_memory(canister).await;
    for fault in 1.. {
        match advance(&probe, 1, 1, 2, fault, false).await {
            Ok(reply) => {
                let writes = decode(reply).writes;
                assert!(writes > 0);
                assert_eq!(writes + 1, fault);
                break;
            }
            Err(error) => {
                assert!(fault < 256, "checkpoint never completed: {error:?}");
                assert_eq!(pic.get_stable_memory(canister).await, before);
            }
        }
    }
    controlled(control(0, &probe, 2, 0).await.unwrap());
    let before = pic.get_stable_memory(canister).await;
    assert!(advance(&sum, 1, 1, 2, 0, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(advance(&average, 2, 1, 2, 1, false).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(advance(&average, 2, 1, 2, 0, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    // A failed source update must not advance the source revision either.
    assert!(control(5, &vec![], 0, 0).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);

    let grouped = handle(start(3, 0).await.unwrap());
    let first_grouped = decode(advance(&grouped, 3, 0, 1, 0, false).await.unwrap());
    assert_eq!(
        (
            first_grouped.sequence,
            first_grouped.examined,
            first_grouped.phase,
            first_grouped.value
        ),
        (1, 2, 0, None)
    );
    let before = pic.get_stable_memory(canister).await;
    assert!(advance(&grouped, 3, 1, 2, 1, false).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(advance(&grouped, 3, 1, 2, 0, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let grouped_sum = handle(start(4, 0).await.unwrap());
    let grouped_average = handle(start(5, 0).await.unwrap());
    let distinct = handle(start(8, 0).await.unwrap());
    for (job, operation) in [(&grouped_sum, 4), (&grouped_average, 5), (&distinct, 8)] {
        let first = decode(advance(job, operation, 0, 1, 0, false).await.unwrap());
        assert_eq!(
            (first.sequence, first.examined, first.phase, first.value),
            (1, 2, 0, None)
        );
        let before = pic.get_stable_memory(canister).await;
        assert!(advance(job, operation, 1, 2, 1, false).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert!(advance(job, operation, 1, 2, 0, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
        .await
        .unwrap();
    let before = pic.get_stable_memory(canister).await;
    assert!(advance(&sum, 1, 1, 2, 1, false).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(advance(&sum, 1, 1, 2, 0, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    // Replaying the prior sequence must not write a checkpoint.
    let replay = decode(advance(&sum, 1, 0, 1, 0, false).await.unwrap());
    assert_eq!((replay.sequence, replay.examined, replay.phase), (1, 2, 0));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let group_distinct = handle(start(9, 0).await.unwrap());
    let first = decode(advance(&group_distinct, 9, 0, 1, 0, false).await.unwrap());
    assert_eq!(
        (first.sequence, first.examined, first.phase, first.value),
        (1, 2, 0, None)
    );
    let before = pic.get_stable_memory(canister).await;
    assert!(advance(&group_distinct, 9, 1, 2, 1, false).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(advance(&group_distinct, 9, 1, 2, 0, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    for (job, operation) in [
        (&grouped_sum, 4),
        (&grouped_average, 5),
        (&distinct, 8),
        (&group_distinct, 9),
    ] {
        let retry = decode(advance(job, operation, 0, 1, 0, false).await.unwrap());
        assert_eq!(
            (
                retry.sequence,
                retry.examined,
                retry.phase,
                retry.value,
                retry.writes
            ),
            (1, 2, 0, None, 0)
        );
    }
    let retry = decode(advance(&sum, 1, 0, 1, 0, false).await.unwrap());
    let retry_average = decode(advance(&average, 2, 0, 1, 0, false).await.unwrap());
    let retry_grouped = decode(advance(&grouped, 3, 0, 1, 0, false).await.unwrap());
    assert_eq!(
        (
            retry_grouped.sequence,
            retry_grouped.examined,
            retry_grouped.phase,
            retry_grouped.value
        ),
        (1, 2, 0, None)
    );
    assert_eq!(
        (
            retry_average.sequence,
            retry_average.examined,
            retry_average.phase,
            retry_average.value
        ),
        (1, 2, 0, None)
    );
    assert_eq!(
        (
            retry.sequence,
            retry.source_revision,
            retry.examined,
            retry.input_bytes,
            retry.phase,
            retry.value
        ),
        (
            first_sum.sequence,
            first_sum.source_revision,
            first_sum.examined,
            first_sum.input_bytes,
            first_sum.phase,
            first_sum.value
        )
    );
    controlled(control(3, &vec![], 0, 0).await.unwrap()); // Noop source replacement.
    controlled(control(4, &vec![], 0, 0).await.unwrap()); // Another entity may change.
    for sequence in 1..=2 {
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

        if sequence < 2 {
            assert_eq!((c.phase, s.phase, a.phase, g.phase), (0, 0, 0, 0));
            assert_eq!((gs.phase, ga.phase, d.phase, gd.phase), (0, 0, 0, 0));
        } else {
            assert_eq!((c.examined, c.phase, c.value), (6, 1, Some("6".into())));
            let expected: u64 = (1..=6).map(|id| id * 10 + id % 2).sum();
            assert_eq!(
                (s.examined, s.phase, s.value),
                (6, 1, Some(expected.to_string()))
            );
            assert_eq!((a.examined, a.phase, a.value), (6, 1, Some("35.5".into())));
            assert_eq!(
                (g.examined, g.phase, g.value),
                (6, 1, Some("0:3,1:3".into()))
            );
            assert_eq!(
                (gs.examined, gs.phase, gs.value),
                (6, 1, Some("0:120,1:93".into()))
            );
            assert_eq!(
                (ga.examined, ga.phase, ga.value),
                (6, 1, Some("0:40,1:31".into()))
            );
            assert_eq!((d.examined, d.phase, d.value), (6, 1, Some("2".into())));
            assert_eq!(
                (gd.examined, gd.phase, gd.value),
                (6, 1, Some("0:1,1:1".into()))
            );
        }
    }
    let live = handle(start(2, 0).await.unwrap());
    decode(advance(&live, 2, 0, 1, 0, false).await.unwrap());
    controlled(control(2, &vec![], 0, 0).await.unwrap());
    let invalidated = decode(advance(&live, 2, 0, 2, 0, false).await.unwrap());
    assert_eq!((invalidated.phase, invalidated.value), (2, None));
    // A completed result keeps its captured boundary after later source writes.
    assert_eq!(
        decode(advance(&sum, 1, 0, 2, 0, false).await.unwrap()).value,
        Some("213".into())
    );
    assert_eq!(
        decode(advance(&average, 2, 0, 2, 0, false).await.unwrap()).value,
        Some("35.5".into())
    );
    let expired = decode(advance(&sum, 1, 0, 100, 0, false).await.unwrap());
    assert_eq!((expired.phase, expired.value), (3, None));
    assert_eq!(controlled(control(1, &vec![], 100, 1).await.unwrap()), 1);
    assert_eq!(
        decode(advance(&grouped, 3, 0, 2, 0, false).await.unwrap()).value,
        Some("0:3,1:3".into())
    );
    assert_eq!(controlled(control(1, &vec![], 100, 16).await.unwrap()), 8);
    let replacement = handle(start(0, 0).await.unwrap());
    assert_ne!(replacement, count);
    let stale = control(0, &count, 0, 0).await.unwrap();
    assert!(
        ic_wire::decode::<Result<u64, String>>(
            &stale,
            Encoding::Frame {
                method: 112,
                direction: Direction::Response
            },
            LIMITS
        )
        .unwrap()
        .is_err()
    );
    controlled(control(0, &replacement, 0, 0).await.unwrap());
    owned.drop().await;
}

#[tokio::test]
async fn present_field_jobs_preserve_zero_counts_across_rollback_and_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-jobs")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let advance =
        async |handle: &Vec<u8>, operation: u8, sequence: u64, fault: u64, trap_after: bool| {
            pic.update_call(
                canister,
                canister,
                "job_advance",
                wire_frame::request(
                    111,
                    &(handle, operation, sequence, 1_u32, 1_i64, fault, trap_after),
                ),
            )
            .await
        };
    let decode =
        |bytes: Vec<u8>| wire_frame::response::<Result<Response, String>>(111, &bytes).unwrap();
    let mut jobs = Vec::new();
    for operation in [6_u8, 7] {
        let response = pic
            .update_call(
                canister,
                canister,
                "job_start",
                wire_frame::request(110, &(operation, 0_i64, 100_i64, 0_u64)),
            )
            .await
            .unwrap();
        let handle = wire_frame::response::<Result<Vec<u8>, String>>(110, &response).unwrap();
        let before = pic.get_stable_memory(canister).await;
        assert!(advance(&handle, operation, 0, 1, false).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert!(advance(&handle, operation, 0, 0, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let step = decode(advance(&handle, operation, 0, 0, false).await.unwrap());
        assert_eq!(
            (
                step.sequence,
                step.examined,
                step.phase,
                step.value.as_deref()
            ),
            (1, 1, 1, Some(if operation == 6 { "0" } else { "1:0" }))
        );
        jobs.push((operation, handle, step));
    }
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    for (operation, handle, saved) in jobs {
        let retry = decode(advance(&handle, operation, 0, 0, false).await.unwrap());
        assert_eq!(
            (
                retry.sequence,
                retry.examined,
                retry.phase,
                retry.value.as_deref()
            ),
            (
                saved.sequence,
                saved.examined,
                saved.phase,
                saved.value.as_deref()
            )
        );
        let complete = decode(advance(&handle, operation, 1, 0, false).await.unwrap());
        assert_eq!(
            (complete.examined, complete.phase, complete.value.as_deref()),
            (1, 1, Some(if operation == 6 { "0" } else { "1:0" }))
        );
    }
    owned.drop().await;
}

#[tokio::test]
async fn completed_group_pages_survive_traps_upgrade_and_source_changes() {
    type PageResponse = (u64, u8, Vec<(u64, u64)>, Option<Vec<u8>>, u64);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-jobs")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let start = async || {
        let response = pic
            .update_call(
                canister,
                canister,
                "job_start",
                wire_frame::request(110, &(3_u8, 0_i64, 100_i64, 0_u64)),
            )
            .await
            .unwrap();

        wire_frame::response::<Result<Vec<u8>, String>>(110, &response).unwrap()
    };
    let handle = start().await;
    let response = pic
        .update_call(
            canister,
            canister,
            "job_advance",
            wire_frame::request(111, &(&handle, 3_u8, 0_u64, 128_u32, 1_i64, 0_u64, false)),
        )
        .await
        .unwrap();
    let old = wire_frame::response::<Result<Response, String>>(111, &response).unwrap();
    assert_eq!((old.phase, old.value.as_deref()), (1, Some("0:3,1:3")));
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
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
            ic_wire::encode(
                &(
                    handle, sequence, cursor, 1_u32, 100_u32, now, fault, trap_after,
                ),
                Encoding::Frame {
                    method: 113,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let decode = |bytes: Vec<u8>| {
        ic_wire::decode::<Result<PageResponse, String>>(
            &bytes,
            Encoding::Frame {
                method: 113,
                direction: Direction::Response,
            },
            LIMITS,
        )
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
    // A job completed before upgrade can be paged without rewriting its format.
    let first = decode(
        page(&handle, old.sequence, None, 2, 0, false)
            .await
            .unwrap(),
    );
    assert_eq!(
        (first.0, first.1, first.2, first.4),
        (old.sequence, 1, vec![(0, 3)], old.source_revision)
    );
    let cursor = first.3.unwrap();
    // A different output page can share the same checkpoint and observed time.
    // Arm the first supplier write: this replay must finish without hitting it.
    let replay = decode(
        page(&handle, old.sequence, Some(cursor.clone()), 2, 1, false)
            .await
            .unwrap(),
    );
    assert_eq!(
        replay,
        (old.sequence, 1, vec![(1, 3)], None, old.source_revision)
    );
    let before = pic.get_stable_memory(canister).await;
    // A later clock observation must still be saved, even on this completed job.
    assert!(
        page(&handle, old.sequence, Some(cursor.clone()), 3, 1, false)
            .await
            .is_err()
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(
        page(&handle, old.sequence, Some(cursor.clone()), 3, 0, true)
            .await
            .is_err()
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    pic.update_call(
        canister,
        canister,
        "job_control",
        ic_wire::encode(
            &(2_u8, Vec::<u8>::new(), 0_i64, 0_u32),
            Encoding::Frame {
                method: 112,
                direction: Direction::Request,
            },
            LIMITS,
        )
        .unwrap(),
    )
    .await
    .unwrap();
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    for _ in 0..2 {
        let last = decode(
            page(&handle, old.sequence, Some(cursor.clone()), 3, 0, false)
                .await
                .unwrap(),
        );
        assert_eq!(
            last,
            (old.sequence, 1, vec![(1, 3)], None, old.source_revision)
        );
    }
    let expired = decode(
        page(&handle, old.sequence, Some(cursor), 100, 0, false)
            .await
            .unwrap(),
    );
    assert_eq!((expired.1, expired.2, expired.3), (3, vec![], None));
    owned.drop().await;
}

#[tokio::test]
async fn retained_extrema_rollback_and_preserve_winners_across_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-jobs")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
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
                wire_frame::request(110, &(operation, 0_i64, 100_i64, 0_u64)),
            )
            .await
            .unwrap();

        wire_frame::response::<Result<Vec<u8>, String>>(110, &bytes).unwrap()
    };
    let advance =
        async |handle: &Vec<u8>, operation: u8, sequence: u64, fault: u64, trap_after: bool| {
            pic.update_call(
                canister,
                canister,
                "job_advance",
                wire_frame::request(
                    111,
                    &(handle, operation, sequence, 2_u32, 1_i64, fault, trap_after),
                ),
            )
            .await
        };
    let decode =
        |bytes: Vec<u8>| wire_frame::response::<Result<Response, String>>(111, &bytes).unwrap();
    let min = start(10).await;
    let max = start(11).await;
    for (handle, op) in [(&min, 10), (&max, 11)] {
        let first = decode(advance(handle, op, 0, 0, false).await.unwrap());
        assert_eq!(
            (first.sequence, first.examined, first.phase, first.value),
            (1, 2, 0, None)
        );
        let before = pic.get_stable_memory(canister).await;
        assert!(advance(handle, op, 1, 1, false).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        assert!(advance(handle, op, 1, 0, true).await.is_err());
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let retry = decode(advance(handle, op, 0, 0, false).await.unwrap());
        assert_eq!((retry.sequence, retry.examined, retry.phase), (1, 2, 0));
    }
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    let mut completed = Vec::new();
    for (handle, op, expected) in [(&min, 10, "11"), (&max, 11, "60")] {
        let mut sequence = 1;

        loop {
            let step = decode(advance(handle, op, sequence, 0, false).await.unwrap());
            sequence = step.sequence;
            if step.phase == 1 {
                assert_eq!((step.examined, step.value.as_deref()), (6, Some(expected)));
                completed.push((handle, op, step.sequence, step.source_revision, expected));
                break;
            }
            assert_eq!(step.phase, 0);
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
            ic_wire::encode(
                &(2_u8, Vec::<u8>::new(), 0_i64, 0_u32),
                Encoding::Frame {
                    method: 112,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    ic_wire::decode::<Result<u64, String>>(
        &bytes,
        Encoding::Frame {
            method: 112,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        decode(advance(&running, 10, 1, 0, false).await.unwrap()).phase,
        2
    );
    for (handle, op, sequence, revision, expected) in completed {
        let step = decode(advance(handle, op, sequence, 0, false).await.unwrap());
        assert_eq!(
            (
                step.sequence,
                step.source_revision,
                step.phase,
                step.value.as_deref()
            ),
            (sequence, revision, 1, Some(expected))
        );
    }
    owned.drop().await;
}

#[tokio::test]
async fn retained_group_extrema_rollback_upgrade_and_page_completed_winners() {
    type Response = (u64, u8, Vec<(u64, Option<u64>)>, Option<Vec<u8>>, u64, u64);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-jobs")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
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
                wire_frame::request(110, &(operation, 0_i64, 100_i64, 0_u64)),
            )
            .await
            .unwrap();

        wire_frame::response::<Result<Vec<u8>, String>>(110, &bytes).unwrap()
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
            ic_wire::encode(
                &(
                    handle, op, sequence, 2_u32, after, 1_u32, now, fault, trap_after,
                ),
                Encoding::Frame {
                    method: 114,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let decode = |bytes: Vec<u8>| {
        ic_wire::decode::<Result<Response, String>>(
            &bytes,
            Encoding::Frame {
                method: 114,
                direction: Direction::Response,
            },
            LIMITS,
        )
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
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
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
            ic_wire::encode(
                &(2_u8, Vec::<u8>::new(), 0_i64, 0_u32),
                Encoding::Frame {
                    method: 112,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    ic_wire::decode::<Result<u64, String>>(
        &bytes,
        Encoding::Frame {
            method: 112,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        decode(advance(&running, 12, 1, None, 3, 0, false).await.unwrap()).1,
        2
    );
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
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
    owned.drop().await;
}

#[tokio::test]
async fn duration_groups_resume_with_exact_totals_across_upgrade() {
    use wire_call::call;
    let wasm = std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").unwrap()).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let rows: Vec<_> = (0..16_u64)
        .map(|id| {
            (
                id,
                None::<u64>,
                [1_u64, 2, 2, 3][id as usize % 4],
                None::<i64>,
                (id % 2) as i64,
            )
        })
        .collect();
    for chunk in rows.chunks(8) {
        call::<Result<(), u8>>(pic, canister, "time_batch", 62, &(chunk, false))
            .await
            .unwrap();
    }
    for work in [32_u32, 6] {
        let handle = call::<Result<Vec<u8>, String>>(
            pic,
            canister,
            "job_start",
            110,
            &(23_u8, 0_i64, 100_i64, 0_u64),
        )
        .await
        .unwrap();
        let mut sequence = 0;
        for examined in (0..16)
            .step_by(work as usize)
            .map(|start| (start + work).min(16))
        {
            let before = pic.cycle_balance(canister).await;
            let (reply, instructions): (Result<Response, String>, u64) = call(
                pic,
                canister,
                "job_advance_cost",
                115,
                &(&handle, 23_u8, sequence, work, 1_i64, 0_u64, false),
            )
            .await;
            let cycles = before - pic.cycle_balance(canister).await;
            let job_step::Step {
                sequence: next,
                examined: seen,
                phase,
                value: result,
                ..
            } = reply.unwrap();
            assert_eq!(seen, u64::from(examined));
            assert_eq!(next, sequence + 1);
            sequence = next;
            println!(
                "duration-job,work={work},examined={seen},instructions={instructions},cycles={cycles}"
            );
            if examined == 16 {
                // Both 1.5 ms and 2.5 ms round to 2 ms, with distinct group totals.
                assert_eq!((phase, result.as_deref()), (1, Some("0:8:12:2,1:8:20:2")));
            } else {
                assert_eq!((phase, result), (0, None));
                if sequence == 1 {
                    ic_db_m0_rollback_driver::upgrade_stopped(
                        pic,
                        canister,
                        wasm.clone(),
                        vec![],
                        None,
                    )
                    .await
                    .unwrap();
                }
            }
        }
    }
    owned.drop().await;
}

#[tokio::test]
async fn duration_summary_overflow_preserves_checkpoint_and_allows_cancellation() {
    use wire_call::call;
    let wasm = std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").unwrap()).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;

    let row = |id: u64, duration: u64, timestamp: i64| {
        (id, None::<u64>, duration, None::<i64>, timestamp)
    };
    let rows = [row(1, 7, 0), row(2, u64::MAX, 1), row(3, 1, 1)];
    let write = async |rows: &[_]| {
        call::<Result<(), u8>>(pic, canister, "time_batch", 62, &(rows, false))
            .await
            .unwrap();
    };
    let start = async || {
        call::<Result<Vec<u8>, String>>(
            pic,
            canister,
            "job_start",
            110,
            &(23_u8, 0_i64, 100_i64, 0_u64),
        )
        .await
        .unwrap()
    };
    let advance = async |handle: &[u8], sequence: u64, work: u32| {
        call::<Result<Response, String>>(
            pic,
            canister,
            "job_advance",
            111,
            &(handle, 23_u8, sequence, work, 1_i64, 0_u64, false),
        )
        .await
    };
    write(&rows).await;
    let handle = start().await;
    let first = advance(&handle, 0, 2).await.unwrap();
    assert_eq!(
        (first.sequence, first.examined, first.phase, first.value),
        (1, 2, 0, None)
    );
    let saved = pic.get_stable_memory(canister).await;
    for _ in 0..2 {
        assert_eq!(
            advance(&handle, 1, 8).await.unwrap_err(),
            "ArithmeticOverflow"
        );
        assert_eq!(pic.get_stable_memory(canister).await, saved);
    }

    call::<Result<u64, String>>(
        pic,
        canister,
        "job_control",
        112,
        &(0_u8, &handle, 2_i64, 1_u32),
    )
    .await
    .unwrap();
    // The repaired group sums to exactly MAX and its half-millisecond tie rounds up.
    write(&[row(3, 0, 1)]).await;
    let replacement = start().await;
    assert_eq!(advance(&handle, 1, 8).await.unwrap_err(), "InvalidJob");
    let done = advance(&replacement, 0, 8).await.unwrap();
    assert_eq!((done.examined, done.phase), (3, 1));
    assert_eq!(
        done.value.as_deref(),
        Some("0:1:7:7,1:2:18446744073709551615:9223372036854775808")
    );
    owned.drop().await;
}
