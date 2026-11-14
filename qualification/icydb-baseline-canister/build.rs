//! Generate the frozen engine's ordinary actor wiring from the benchmark schema.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    icydb::build_canister!(ic_db_icydb_baseline_schema::BaselineCanister)
}
