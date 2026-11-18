//! Generate the target assembly from the shared schema crate.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    ic_db_m1_records_schema::write_schema("crate::schema")
}
