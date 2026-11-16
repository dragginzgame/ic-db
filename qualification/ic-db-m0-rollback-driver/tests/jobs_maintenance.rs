//! Maintenance gates aggregate reads; independent job custody still permits cleanup.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

type Step = (u64, u64, u64, u64, u8, Option<String>, u64);

#[tokio::test]
async fn frozen_jobs_remain_cleanable_and_resume_only_after_supported_cancellation() {
    let directory =
        std::path::PathBuf::from(std::env::var_os("IC_DB_M3_DIR").expect("run make m3-jobs"));
    let wasm = std::fs::read(directory.join("jobs-maintenance.wasm")).unwrap();
    let changed = std::fs::read(directory.join("jobs-maintenance-v2.wasm")).unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let upgrade = async |bytes: &Vec<u8>| {
        pic.advance_time(std::time::Duration::from_secs(600)).await;
        pic.tick().await;
        pic.upgrade_canister(canister, bytes.clone(), vec![], None)
            .await
            .unwrap();
    };
    let start = async |expires: i64| {
        pic.update_call(
            canister,
            canister,
            "job_start",
            ic_wire::encode(
                &(1_u8, 0_i64, expires, 0_u64),
                Encoding::Frame {
                    method: 110,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let handle = |reply: Vec<u8>| {
        ic_wire::decode::<Result<Vec<u8>, String>>(
            &reply,
            Encoding::Frame {
                method: 110,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
        .unwrap()
    };
    let advance = async |handle: &[u8], sequence: u64, work: u32, now: i64| {
        pic.update_call(
            canister,
            canister,
            "job_advance",
            ic_wire::encode(
                &(handle, 1_u8, sequence, work, now, 0_u64, false),
                Encoding::Frame {
                    method: 111,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    let step = |reply: Vec<u8>| {
        ic_wire::decode::<Result<Step, String>>(
            &reply,
            Encoding::Frame {
                method: 111,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
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
    assert_eq!((first.0, first.2, first.4, first.5), (1, 2, 0, None));
    let total = (1_u64..=6)
        .map(|id| id * 10 + id % 2)
        .sum::<u64>()
        .to_string();
    let done = step(advance(&completed, 0, 128, 1).await.unwrap()).unwrap();
    assert_eq!((done.2, done.4, done.5), (6, 1, Some(total.clone())));
    admitted(maintenance(0, false).await.unwrap()).unwrap();

    for executable in [&wasm, &changed] {
        upgrade(executable).await;
        let frozen = pic.get_stable_memory(canister).await;
        // These fixture adapters unwrap ordinary Database::open; the expected
        // Maintenance refusal therefore reaches the caller as a message trap.
        let error = start(1000).await.unwrap_err();
        assert!(error.to_string().contains("Maintenance"), "{error}");
        for (job, sequence) in [(&running, first.0), (&completed, done.0)] {
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
    assert!(advance(&running, first.0, 2, 21).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);

    upgrade(&wasm).await;
    let before = pic.get_stable_memory(canister).await;
    assert!(maintenance(2, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    admitted(maintenance(2, false).await.unwrap()).unwrap();
    let second = step(advance(&running, first.0, 2, 21).await.unwrap()).unwrap();
    assert_eq!(
        (second.0, second.1, second.2, second.4, second.5),
        (2, first.1, 4, 0, None)
    );
    let retry = step(advance(&running, first.0, 2, 21).await.unwrap()).unwrap();
    assert_eq!((retry.0, retry.1, retry.2, retry.4), (2, first.1, 4, 0));
    let third = step(advance(&running, second.0, 2, 22).await.unwrap()).unwrap();
    let final_step = step(advance(&running, third.0, 2, 23).await.unwrap()).unwrap();
    assert_eq!(
        (final_step.1, final_step.2, final_step.4, final_step.5),
        (first.1, 6, 1, Some(total.clone()))
    );
    let retained = step(advance(&completed, done.0, 2, 23).await.unwrap()).unwrap();
    assert_eq!(
        (retained.0, retained.1, retained.2, retained.4, retained.5),
        (done.0, done.1, 6, 1, Some(total))
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
    owned.close().await;
}
