//! Same scalar workload, transport and supplier graph; different engine contracts.

mod artifact;
mod support;

use support::scalar_workload;

#[tokio::test]
async fn matched_scalar_records() {
    for (label, variable, needs_settling) in [
        ("ic-db", "IC_DB_RECORDS_BENCHMARK_WASM", false),
        ("icydb", "IC_DB_ICYDB_BASELINE_WASM", true),
    ] {
        if needs_settling && std::env::var_os("IC_DB_COUNT_PROFILE").is_some() {
            continue;
        }
        let path = std::env::var_os(variable).expect("run make m1-records-comparison");
        scalar_workload::run(label, &path, needs_settling, true).await;
    }
}
