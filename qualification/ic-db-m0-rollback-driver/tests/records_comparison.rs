//! Same scalar workload, transport and supplier graph; different engine contracts.

mod artifact;
mod support;

use support::scalar_workload;

#[tokio::test]
async fn matched_query_workload() {
    let workload = scalar_workload::Workload::selected();
    for (label, variable, needs_settling) in [
        ("ic-db", "IC_DB_RECORDS_BENCHMARK_WASM", false),
        ("icydb", "IC_DB_ICYDB_BASELINE_WASM", true),
    ] {
        if needs_settling && workload == scalar_workload::Workload::CountProfile {
            continue;
        }
        let path = std::env::var_os(variable).expect("run a matched query comparison target");
        scalar_workload::run(label, &path, needs_settling, workload).await;
    }
}
