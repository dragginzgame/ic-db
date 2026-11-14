//! Compile declarations with Rust, then validate and emit the target schema assembly.
#[path = "src/schema.rs"]
mod schema;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=src/schema.rs");
    let source = ic_db::build::generate(concat!(module_path!(), "::schema"), "crate::schema")?;
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("schema.rs"),
        source,
    )?;
    Ok(())
}
