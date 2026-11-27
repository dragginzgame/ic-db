#[path = "mod.rs"]
mod artifact;

pub fn read(path: &std::path::Path, label: std::fmt::Arguments<'_>) -> Vec<u8> {
    let (wasm, facts) = artifact::read(std::fs::File::open(path).unwrap()).unwrap();
    let exports: Vec<_> = facts
        .exports
        .iter()
        .map(String::as_str)
        .filter(|name| name.starts_with("canister_"))
        .collect();
    assert_eq!(exports, ["canister_init", "canister_update exercise"]);
    eprintln!(
        "{label} wasm_bytes={} code_section_bytes={} sha256={}",
        facts.identity.bytes,
        facts.code_section_bytes,
        facts
            .identity
            .sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    wasm
}
