//! Exact finite float aggregates using the product scanner, retained job codec and stable store.
mod wire_call;
use ic_db_m0_rollback_driver::{instance, upgrade_stopped};
type Reply = Result<(u64, u64, Option<Vec<(String, u64)>>), String>;
#[tokio::test]
async fn product_float_sums_resume_and_refuse_intact() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_FLOAT64_SUM_WASM").expect("run make m2-float64-sum"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    for (width, average, small) in [
        (2_u16, false, false),
        (256, false, false),
        (2, true, false),
        (2, false, true),
        (2, true, true),
    ] {
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 2_000_000_000_000).await;
        pic.install_canister(canister, wasm.clone(), vec![], None)
            .await;
        for round in 0..5_u8 {
            wire_call::call::<()>(pic, canister, "seed", 1, &(width, round)).await;
        }
        let start = async |grouped| -> Vec<u8> {
            wire_call::call(pic, canister, "start", 2, &(grouped, average, small)).await
        };
        let advance = async |grouped: bool, handle: &Vec<u8>, seq: u64, work: u16| -> Reply {
            wire_call::call(
                pic,
                canister,
                "advance",
                3,
                &(grouped, average, small, handle, seq, work),
            )
            .await
        };
        let scalar = start(false).await;
        let grouped = start(true).await;
        assert_eq!(advance(false, &scalar, 0, 96).await.unwrap(), (1, 96, None));
        if width == 2 {
            assert_eq!(advance(true, &grouped, 0, 96).await.unwrap(), (1, 96, None));
        } else {
            assert_eq!(advance(true, &grouped, 0, 16).await.unwrap(), (1, 16, None));
        }
        upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        let before = pic.get_stable_memory(canister).await;
        assert_eq!(
            advance(true, &scalar, 1, 256).await.unwrap_err(),
            "InvalidJob"
        );
        if width == 2 {
            let expected = (0..32)
                .map(|i| {
                    let units = if average { (i + 3) / 5 } else { i + 1 };
                    (format!("{i:02}"), units)
                })
                .collect();
            assert_eq!(
                advance(true, &grouped, 1, 256).await.unwrap(),
                (2, 160, Some(expected))
            );
        } else {
            assert_eq!(advance(true, &grouped, 1, 32).await.unwrap_err(), "Limit");
            assert!(pic.get_stable_memory(canister).await == before);
            assert_eq!(advance(true, &grouped, 1, 1).await.unwrap(), (2, 17, None));
        }
        assert_eq!(
            advance(false, &scalar, 1, 256).await.unwrap(),
            (
                2,
                160,
                Some(vec![(String::new(), if average { 3 } else { 528 })])
            )
        );
    }
    owned.drop().await;
}

#[tokio::test]
async fn committed_writes_invalidate_running_averages_and_preserve_completed_results() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_FLOAT64_SUM_WASM").expect("run make m2-float64-sum"))
            .unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 2_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let seed = async |round: u8| {
        wire_call::call::<()>(pic, canister, "seed", 1, &(2_u16, round)).await;
    };
    let start = async |grouped: bool| -> Vec<u8> {
        wire_call::call(pic, canister, "start", 2, &(grouped, true, true)).await
    };
    let advance = async |grouped: bool, handle: &[u8], sequence: u64, work: u16| -> Reply {
        wire_call::call(
            pic,
            canister,
            "advance",
            3,
            &(grouped, true, true, handle, sequence, work),
        )
        .await
    };

    seed(0).await;
    let completed = start(false).await;
    let original = advance(false, &completed, 0, 64).await.unwrap();
    assert_eq!(
        original,
        (
            1,
            32,
            Some(vec![(String::new(), u64::from(f32::MAX.to_bits()))])
        )
    );

    for (grouped, round) in [(false, 1), (true, 2)] {
        let running = start(grouped).await;
        assert_eq!(
            advance(grouped, &running, 0, 16).await.unwrap(),
            (1, 16, None)
        );
        // A retry before the write returns the checkpoint without scanning again.
        assert_eq!(
            advance(grouped, &running, 0, 16).await.unwrap(),
            (1, 16, None)
        );
        seed(round).await;
        assert_eq!(
            advance(grouped, &running, 1, 16).await.unwrap_err(),
            "Invalidated"
        );
        assert_eq!(
            advance(grouped, &running, 1, 16).await.unwrap_err(),
            "Invalidated"
        );
    }
    seed(3).await;
    seed(4).await;
    upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(advance(false, &completed, 0, 32).await.unwrap(), original);

    let fresh = start(false).await;
    for sequence in 0..5 {
        assert_eq!(
            advance(false, &fresh, sequence, 32).await.unwrap(),
            (sequence + 1, (sequence + 1) * 32, None)
        );
    }
    assert_eq!(
        advance(false, &fresh, 5, 32).await.unwrap(),
        (6, 160, Some(vec![(String::new(), 3)]))
    );
    owned.drop().await;
}

#[tokio::test]
async fn retained_summary_pages_survive_interrupted_scan_and_completed_upgrade() {
    type Page = (Vec<(String, u64, u64, u64)>, Option<Vec<u8>>);
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_FLOAT64_SUM_WASM").expect("run make m2-float64-sum"))
            .unwrap();
    let pic = instance().await.unwrap();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 2_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    for round in 0..5_u8 {
        wire_call::call::<()>(&pic, canister, "seed", 1, &(2_u16, round)).await;
    }
    let handle: Vec<u8> = wire_call::call(&pic, canister, "start_summary", 4, &()).await;
    let advance =
        async |sequence: u64, work: u16, cursor: Option<Vec<u8>>| -> (u64, u64, Option<Page>) {
            wire_call::call(
                &pic,
                canister,
                "advance_summary",
                5,
                &(&handle, sequence, work, cursor),
            )
            .await
        };
    // Each group has accumulated two maxima and a tiny term, before cancellation.
    let checkpoint = advance(0, 96, None).await;
    assert_eq!(checkpoint, (1, 96, None));
    upgrade_stopped(&pic, canister, wasm.clone(), vec![], None)
        .await
        .unwrap();
    assert_eq!(advance(0, 1, None).await, checkpoint);

    let mut expected: Vec<_> = (0..32)
        .map(|i| (format!("{i:02}"), 5, i + 1, (i + 3) / 5))
        .collect();
    // The stable sort preserves ascending group keys when rounded averages tie.
    expected.sort_by_key(|row| std::cmp::Reverse(row.3));
    let mut cursor = None;
    for page in 0..5 {
        let sequence = if page == 0 { 1 } else { 2 };
        let (seq, examined, result) = advance(sequence, 128, cursor.take()).await;
        assert_eq!((seq, examined), (2, 160));
        let (rows, next) = result.expect("completed summary");
        // The large terms cancel. The sum is (i+1) subnormal units and the
        // average is that integer divided by five, rounded to the nearest unit.
        assert_eq!(rows, expected[page * 7..((page + 1) * 7).min(32)]);
        assert_eq!(next.is_some(), page < 4);
        cursor = next;
        if page == 0 {
            upgrade_stopped(&pic, canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
        }
    }
    pic.drop().await;
}
