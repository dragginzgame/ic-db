#!/usr/bin/env bash
# Explicit scalar comparison; no release/CI selection or product optimization.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_NET_OFFLINE=true
evidence="$PWD/qualification/target/evidence/records-comparison"
mkdir -p "$evidence"
rustc -Vv > "$evidence/compiler.txt"
git rev-parse HEAD > "$evidence/source-commit.txt"
git status --short > "$evidence/source-status.txt"
git diff --binary HEAD > "$evidence/source.diff"
git ls-files -z --cached --others --exclude-standard -- \
    Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates qualification scripts Makefile |
    xargs -0 shasum -a 256 > "$evidence/source-sha256.txt"
printf 'RUSTFLAGS=%s\nCARGO_ENCODED_RUSTFLAGS=%s\n' \
    "${RUSTFLAGS-}" "${CARGO_ENCODED_RUSTFLAGS-}" > "$evidence/build-environment.txt"

for engine in records-benchmark icydb-baseline; do
    package="ic-db-$engine-canister"
    cargo tree --manifest-path qualification/Cargo.toml --locked -p "$package" \
        --target wasm32-unknown-unknown -e normal,build --format '{p} features=[{f}]' \
        > "$evidence/$engine-features.txt"
    cargo build --manifest-path qualification/Cargo.toml --locked -p "$package" \
        --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics \
        > "$evidence/$engine-build.jsonl"
    artifact="$(jq -ers --arg target "${package//-/_}" \
        '[.[] | select(.reason == "compiler-artifact" and .target.name == $target) |
          .filenames[] | select(endswith(".wasm"))] |
         if length == 1 then .[0] else error("expected one Wasm artifact") end' \
        "$evidence/$engine-build.jsonl")"
    cp "$artifact" "$evidence/$engine.wasm"
done

export IC_DB_RECORDS_BENCHMARK_WASM="$evidence/records-benchmark.wasm"
export IC_DB_ICYDB_BASELINE_WASM="$evidence/icydb-baseline.wasm"
cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver \
    --test records_comparison -- --nocapture 2>&1 | tee "$evidence/results.log"
