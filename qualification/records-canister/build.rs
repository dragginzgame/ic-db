//! Compile declarations with Rust, then validate and emit the target schema assembly.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = ic_db_m1_records_schema::generate("crate::schema")?;
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("schema.rs"),
        source,
    )?;
    Ok(())
}
