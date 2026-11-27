//! Maintenance gates aggregate reads; independent job custody still permits cleanup.

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

mod job_step;
mod wire_frame;
use job_step::Step;

#[tokio::test]
async fn frozen_jobs_remain_cleanable_and_resume_only_after_supported_cancellation() {
    let directory =
        std::path::PathBuf::from(std::env::var_os("IC_DB_M3_DIR").expect("run make m3-jobs"));
    let wasm = std::fs::read(directory.join("jobs-maintenance.wasm")).unwrap();
    let changed = std::fs::read(directory.join("jobs-maintenance-v2.wasm")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let upgrade = async |bytes: &Vec<u8>| {
        pic.advance_time(std::time::Duration::from_secs(600)).await;
        pic.tick().await;
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, bytes.clone(), vec![], None)
            .await
            .unwrap();
    };
    let start = async |expires: i64| {
        pic.update_call(
            canister,
            canister,
            "job_start",
            wire_frame::request(110, &(1_u8, 0_i64, expires, 0_u64)),
        )
        .await
    };
    let handle =
        |reply: Vec<u8>| wire_frame::response::<Result<Vec<u8>, String>>(110, &reply).unwrap();
    let advance = async |handle: &[u8], sequence: u64, work: u32, now: i64| {
        pic.update_call(
            canister,
            canister,
            "job_advance",
            wire_frame::request(111, &(handle, 1_u8, sequence, work, now, 0_u64, false)),
        )
        .await
    };
    let step = |reply: Vec<u8>| wire_frame::response::<Result<Step, String>>(111, &reply);
    let control = async |operation: u8, handle: &[u8], now: i64, limit: u32| {
        let reply = pic
            .update_call(
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
            .unwrap();

        ic_wire::decode::<Result<u64, String>>(
            &reply,
            Encoding::Frame {
                method: 112,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let maintenance = async |operation: u8, trap: bool| {
        pic.update_call(
            canister,
            canister,
            "maintenance_control",
            ic_wire::encode(
                &(operation, vec![9_u8], trap),
                Encoding::Frame {
                    method: 133,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let admitted = |reply: Vec<u8>| {
        ic_wire::decode::<Result<(), String>>(
            &reply,
            Encoding::Frame {
                method: 133,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };

    let running = handle(start(1000).await.unwrap());
    let cancelled = handle(start(1000).await.unwrap());
    let expired = [
        handle(start(20).await.unwrap()),
        handle(start(20).await.unwrap()),
    ];
    let completed = handle(start(1000).await.unwrap());
    let first = step(advance(&running, 0, 2, 1).await.unwrap()).unwrap();
    assert_eq!(
        (first.sequence, first.examined, first.phase, first.value),
        (1, 2, 0, None)
    );
    let total = (1_u64..=6)
        .map(|id| id * 10 + id % 2)
        .sum::<u64>()
        .to_string();
    let done = step(advance(&completed, 0, 128, 1).await.unwrap()).unwrap();
    assert_eq!(
        (done.examined, done.phase, done.value),
        (6, 1, Some(total.clone()))
    );
    admitted(maintenance(0, false).await.unwrap()).unwrap();

    for executable in [&wasm, &changed] {
        upgrade(executable).await;
        let frozen = pic.get_stable_memory(canister).await;
        // These fixture adapters unwrap ordinary Database::open; the expected
        // Maintenance refusal therefore reaches the caller as a message trap.
        let error = start(1000).await.unwrap_err();
        assert!(error.to_string().contains("Maintenance"), "{error}");
        for (job, sequence) in [(&running, first.sequence), (&completed, done.sequence)] {
            let error = advance(job, sequence, 2, 21).await.unwrap_err();
            assert!(error.to_string().contains("Maintenance"), "{error}");
        }
        assert_eq!(pic.get_stable_memory(canister).await, frozen);
    }
    let frozen = pic.get_stable_memory(canister).await;
    assert_eq!(
        admitted(maintenance(2, false).await.unwrap()),
        Err("InvalidTransition".into())
    );
    assert_eq!(pic.get_stable_memory(canister).await, frozen);

    // Version two cannot interpret this active maintenance transition, but
    // supported job-frame cleanup needs neither its schema nor source admission.
    assert_eq!(control(0, &cancelled, 0, 0).await, Ok(0));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(control(0, &cancelled, 0, 0).await, Err("InvalidJob".into()));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(control(1, &[], 20, 1).await, Ok(1));
    assert_eq!(control(1, &[], 20, 1).await, Ok(1));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(control(1, &[], 20, 1).await, Ok(0));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(advance(&running, first.sequence, 2, 21).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);

    upgrade(&wasm).await;
    let before = pic.get_stable_memory(canister).await;
    assert!(maintenance(2, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    admitted(maintenance(2, false).await.unwrap()).unwrap();
    let second = step(advance(&running, first.sequence, 2, 21).await.unwrap()).unwrap();
    assert_eq!(
        (
            second.sequence,
            second.source_revision,
            second.examined,
            second.phase,
            second.value
        ),
        (2, first.source_revision, 4, 0, None)
    );
    let retry = step(advance(&running, first.sequence, 2, 21).await.unwrap()).unwrap();
    assert_eq!(
        (
            retry.sequence,
            retry.source_revision,
            retry.examined,
            retry.phase
        ),
        (2, first.source_revision, 4, 0)
    );
    let third = step(advance(&running, second.sequence, 2, 22).await.unwrap()).unwrap();
    let final_step = step(advance(&running, third.sequence, 2, 23).await.unwrap()).unwrap();
    assert_eq!(
        (
            final_step.source_revision,
            final_step.examined,
            final_step.phase,
            final_step.value
        ),
        (first.source_revision, 6, 1, Some(total.clone()))
    );
    let retained = step(advance(&completed, done.sequence, 2, 23).await.unwrap()).unwrap();
    assert_eq!(
        (
            retained.sequence,
            retained.source_revision,
            retained.examined,
            retained.phase,
            retained.value
        ),
        (done.sequence, done.source_revision, 6, 1, Some(total))
    );

    let fresh = handle(start(1000).await.unwrap());
    assert_ne!(fresh, cancelled);
    let before = pic.get_stable_memory(canister).await;
    for old in [&cancelled, &expired[0], &expired[1]] {
        assert_eq!(
            step(advance(old, 0, 2, 23).await.unwrap()),
            Err("InvalidJob".into())
        );
        assert_eq!(control(0, old, 23, 0).await, Err("InvalidJob".into()));
    }
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for job in [&running, &completed, &fresh] {
        assert_eq!(control(0, job, 23, 0).await, Ok(0));
    }
    owned.drop().await;
}

#[tokio::test]
async fn publication_invalidates_positions_but_preserves_independent_job_cleanup() {
    let directory =
        std::path::PathBuf::from(std::env::var_os("IC_DB_M3_DIR").expect("run make m3-jobs"));
    let wasm = std::fs::read(directory.join("jobs-maintenance.wasm")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let upgrade = async || {
        pic.advance_time(std::time::Duration::from_secs(600)).await;
        pic.tick().await;
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
    };
    let copy = async |operation: u8, trap: bool| {
        let reply = pic
            .update_call(
                canister,
                canister,
                "maintenance_whole_copy",
                ic_wire::encode(
                    &(operation, trap),
                    Encoding::Frame {
                        method: 138,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await?;

        Ok::<_, ic_testkit::pocket_ic::RejectResponse>(
            ic_wire::decode::<Result<(bool, Option<String>, Vec<(u64, u64, u128)>), String>>(
                &reply,
                Encoding::Frame {
                    method: 138,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap(),
        )
    };
    let job = async |operation: u8, target: bool, bytes: &[u8], sequence: u64, work: u32| {
        let reply = pic
            .update_call(
                canister,
                canister,
                "maintenance_job",
                ic_wire::encode(
                    &(operation, target, bytes, sequence, work),
                    Encoding::Frame {
                        method: 146,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();

        ic_wire::decode::<Result<(Vec<u8>, u64, u64, u64, Option<u64>), String>>(
            &reply,
            Encoding::Frame {
                method: 146,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let cleanup = async |operation: u8, bytes: &[u8], now: i64| {
        let reply = pic
            .update_call(
                canister,
                canister,
                "job_control",
                ic_wire::encode(
                    &(operation, bytes, now, 1_u32),
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
            &reply,
            Encoding::Frame {
                method: 112,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };

    copy(10, false).await.unwrap().unwrap();
    let running = job(0, false, &[], 0, 32).await.unwrap().0;
    let completed = job(0, false, &[], 0, 32).await.unwrap().0;
    let first = job(1, false, &running, 0, 1).await.unwrap();
    assert_eq!((first.1, first.2, first.3, first.4), (1, 1, 1, None));
    let done = job(1, false, &completed, 0, 32).await.unwrap();
    assert_eq!((done.2, done.3, done.4), (1, 2, Some(2)));
    let cursor = job(2, false, &[], 0, 32).await.unwrap().0;
    assert!(!cursor.is_empty());
    copy(11, false).await.unwrap().unwrap();
    assert_eq!(
        copy(1, false).await.unwrap().unwrap().1.as_deref(),
        Some("A_Mapping")
    );
    upgrade().await;
    assert_eq!(
        copy(1, false).await.unwrap().unwrap().1.as_deref(),
        Some("Entry")
    );
    assert_eq!(
        copy(2, false).await.unwrap().unwrap().1.as_deref(),
        Some("Entry")
    );
    assert_eq!(
        copy(2, false).await.unwrap().unwrap().1.as_deref(),
        Some("Z_Empty")
    );
    assert!(copy(3, false).await.unwrap().unwrap().0);
    let mut ready = false;
    for _ in 0..32 {
        if copy(4, false).await.unwrap().unwrap().0 {
            ready = true;
            break;
        }
    }
    assert!(ready);
    let before = pic.get_stable_memory(canister).await;
    assert!(copy(5, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    copy(5, false).await.unwrap().unwrap();
    upgrade().await;

    let before = pic.get_stable_memory(canister).await;
    // The unchanged entity and its numeric revision match; the new database's
    // schema/incarnation still forbids both unfinished and completed old jobs.
    let page = job(2, true, &[], 0, 32).await.unwrap();
    assert_eq!((page.2, page.3), (first.2, 1));
    assert_eq!(
        job(2, true, &cursor, 0, 32).await,
        Err("InvalidCursor".into())
    );
    for (handle, sequence) in [(&running, first.1), (&completed, done.1)] {
        assert_eq!(
            job(1, true, handle, sequence, 32).await,
            Err("InvalidJob".into())
        );
        assert_eq!(
            job(1, false, handle, sequence, 32).await,
            Err("Maintenance".into())
        );
    }
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(cleanup(0, &running, 1).await, Ok(0));
    assert_eq!(cleanup(1, &[], 100).await, Ok(1));
    assert_eq!(cleanup(1, &[], 100).await, Ok(0));
    upgrade().await;
    let fresh = job(0, true, &[], 0, 32).await.unwrap().0;
    assert_ne!(fresh, running);
    assert_ne!(fresh, completed);
    let result = job(1, true, &fresh, 0, 32).await.unwrap();
    assert_eq!((result.2, result.3, result.4), (1, 2, Some(2)));
    let before = pic.get_stable_memory(canister).await;
    for old in [&running, &completed] {
        assert_eq!(cleanup(0, old, 1).await, Err("InvalidJob".into()));
    }
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(cleanup(0, &fresh, 1).await, Ok(0));
    owned.drop().await;
}
