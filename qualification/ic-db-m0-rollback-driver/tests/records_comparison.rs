//! Same scalar workload, transport and supplier graph; different engine contracts.
mod artifact;
mod support;
use support::scalar_workload;

#[tokio::test]
async fn matched_scalar_records() {
    let fluent = std::env::var_os("IC_DB_FLUENT_COMPARISON").is_some();
    for (label, variable, needs_settling) in [
        ("ic-db", "IC_DB_RECORDS_BENCHMARK_WASM", false),
        ("icydb", "IC_DB_ICYDB_BASELINE_WASM", true),
    ] {
        let path = std::env::var_os(variable).expect("run make m1-records-comparison");
        scalar_workload::run(label, &path, needs_settling, fluent).await;
    }
}
