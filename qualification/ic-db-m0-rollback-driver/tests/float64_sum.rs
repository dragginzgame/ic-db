//! Exact Float64 sums using the product scanner, retained job codec and stable store.
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
    for width in [2_u16, 256] {
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 2_000_000_000_000).await;
        pic.install_canister(canister, wasm.clone(), vec![], None)
            .await;
        for round in 0..5_u8 {
            wire_call::call::<()>(pic, canister, "seed", 1, &(width, round)).await;
        }
        let start = async |grouped| -> Vec<u8> {
            wire_call::call(pic, canister, "start", 2, &grouped).await
        };
        let advance = async |grouped: bool, handle: &Vec<u8>, seq: u64, work: u16| -> Reply {
            wire_call::call(pic, canister, "advance", 3, &(grouped, handle, seq, work)).await
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
            let expected = (0..32).map(|i| (format!("{i:02}"), i + 1)).collect();
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
            (2, 160, Some(vec![(String::new(), 528)]))
        );
    }
    owned.drop().await;
}
