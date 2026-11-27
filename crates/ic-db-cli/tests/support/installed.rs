use std::path::{Path, PathBuf};

pub fn install(prefix: &Path) -> PathBuf {
    std::fs::create_dir(prefix).unwrap();
    std::fs::create_dir(prefix.join("bin")).unwrap();
    std::fs::create_dir_all(prefix.join("share/ic-db/m1")).unwrap();
    let binary = prefix.join("bin/db");
    std::fs::copy(env!("CARGO_BIN_EXE_db"), &binary).unwrap();
    std::fs::copy(
        std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("build the console fixture"),
        prefix.join("share/ic-db/m1/records.wasm"),
    )
    .unwrap();
    binary
}
