//! Installed open, scan and arithmetic costs for grouped Duration summaries.

mod wire_call;

type Sample = (u64, u64, u64, u32, u32, u64, u64);

use ic_db_m0_rollback_driver::instance;
use wire_call::call;

#[tokio::test]
#[ignore = "requires query-measurements Wasm; run make m2-duration-costs"]
async fn grouped_duration_summary_costs() {
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    let mut previous = 0;
    for (population, count, sum, average) in [(2_u32, 2, 3, 2), (16, 12, 96, 8), (96, 72, 3456, 48)]
    {
        let rows: Vec<_> = (previous + 1..=population)
            .map(|id| {
                let id = u64::from(id);
                (id, (id % 4 != 0).then_some(id), id, None::<i64>, 0_i64)
            })
            .collect();
        for chunk in rows.chunks(8) {
            call::<Result<(), u8>>(pic, canister, "time_batch", 62, &(chunk, false))
                .await
                .unwrap();
        }
        previous = population;
        let before_control = pic.cycle_balance(canister).await;
        let control: Sample = call(pic, canister, "measured_duration_summary", 245, &false).await;
        assert_eq!(control, (0, 0, 0, 0, 0, 0, 0));
        let before = pic.cycle_balance(canister).await;
        let control_cycles = before_control - before;
        let (actual_count, actual_sum, actual_average, examined, fetched, open, query): Sample =
            call(pic, canister, "measured_duration_summary", 245, &true).await;
        let cycles = before - pic.cycle_balance(canister).await;
        assert_eq!(
            (actual_count, actual_sum, actual_average),
            (count, sum, average)
        );
        assert_eq!((examined, fetched), (population, population));
        println!(
            "duration-summary,rows={population},open={open},query={query},update_cycles={cycles},control_cycles={control_cycles}"
        );
    }
    owned.drop().await;
}
