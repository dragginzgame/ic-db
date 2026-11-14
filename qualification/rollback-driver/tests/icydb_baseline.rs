//! Standalone measurement of frozen SQL-disabled IcyDB using the shared workload.
#[path = "support/scalar_workload.rs"]
mod scalar_workload;

#[tokio::test]
async fn frozen_icydb_baseline() {
    let path = std::env::var_os("IC_DB_ICYDB_BASELINE_WASM").expect("run make m0-icydb-baseline");
    scalar_workload::run("icydb", &path, true).await;
}
