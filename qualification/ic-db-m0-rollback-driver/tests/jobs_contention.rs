//! Controlled write schedules against product retained sums, without hidden retries.
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

type Step = (u64, u64, u64, u64, u8, Option<String>, u64);

#[tokio::test]
async fn retained_sum_requires_a_quiet_source_boundary_not_a_quiet_database() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-contention"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = owned.instance();
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
                ic_wire::encode(
                    &(id, id % 2, amount, mode),
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
        ic_wire::decode::<Result<(), u8>>(
            &reply,
            Encoding::Frame {
                method: 4,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let start = async || {
        let reply = pic
            .update_call(
                canister,
                canister,
                "job_start",
                ic_wire::encode(
                    &(1_u8, 0_i64, 100_i64, 0_u64),
                    Encoding::Frame {
                        method: 110,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
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
    let advance = async |handle: &[u8], sequence: u64| {
        let reply = pic
            .update_call(
                canister,
                canister,
                "job_advance",
                ic_wire::encode(
                    &(handle, 1_u8, sequence, 12_u32, 1_i64, 0_u64, false),
                    Encoding::Frame {
                        method: 111,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
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
    let control = async |operation: u8, handle: &[u8]| {
        pic.update_call(
            canister,
            canister,
            "job_control",
            ic_wire::encode(
                &(operation, handle, 0_i64, 0_u32),
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
    let controlled = |reply: Vec<u8>| {
        ic_wire::decode::<Result<u64, String>>(
            &reply,
            Encoding::Frame {
                method: 112,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
        .unwrap()
    };
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
        for call in 1..=9 {
            let step = advance(&handle, sequence).await.unwrap();
            assert_eq!(step.1, *revision.get_or_insert(step.1));
            assert_eq!(step.2, (call * 12).min(96));
            sequence = step.0;
            if call == 9 {
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
        assert!(advance(&handle, sequence).await.is_err());
        eprintln!("schedule={name} rows=96 work=12 advances=9 outcome=complete sum={base}");
    }

    // Three caller-chosen restarts under writes all invalidate. Never report a
    // prefix sum or silently restart the existing handle; explicitly cancel it.
    // A later quiet interval allows a fresh attempt to finish.
    let mut changes = 0;
    let mut previous_revision = None;
    for attempt in 0..3 {
        let handle = start().await;
        let first = advance(&handle, 0).await.unwrap();
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
            pic.upgrade_canister(canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
        }
        let invalid = advance(&handle, first.0).await.unwrap();
        assert_eq!(
            (invalid.1, invalid.2, invalid.4, &invalid.5),
            (first.1, 12, 2, &None)
        );
        let retry = advance(&handle, 0).await.unwrap();
        assert_eq!(
            (retry.0, retry.1, retry.2, retry.4, retry.5),
            (invalid.0, invalid.1, invalid.2, 2, None)
        );
        controlled(control(0, &handle).await.unwrap());
        assert!(advance(&handle, 0).await.is_err());
        eprintln!(
            "schedule=source-write attempt={} examined=12 outcome=invalidated",
            attempt + 1
        );
    }
    let handle = start().await;
    let mut sequence = 0;
    let mut captured_revision = 0;
    for call in 1..=9 {
        let step = advance(&handle, sequence).await.unwrap();
        sequence = step.0;
        captured_revision = step.1;
        if call == 9 {
            assert_eq!(
                (step.2, step.4, step.5),
                (96, 1, Some((base + changes).to_string()))
            );
        } else {
            assert_eq!((step.4, step.5), (0, None));
        }
    }
    controlled(control(2, &[]).await.unwrap());
    let historical = advance(&handle, sequence).await.unwrap();
    assert_eq!(
        (historical.1, historical.4, historical.5),
        (captured_revision, 1, Some((base + changes).to_string()))
    );
    controlled(control(0, &handle).await.unwrap());
    eprintln!(
        "schedule=quiet-after-writes advances=9 outcome=complete sum={}",
        base + changes
    );
}
