#!/usr/bin/env bash
# Disposable source copy: measure the proposed format without adopting it in crates/.
set -euo pipefail
cd "$(dirname "$0")/.."
root="$PWD"
export CARGO_NET_OFFLINE=true
unset IC_DB_FLUENT_COMPARISON IC_DB_AGGREGATE_COMPARISON IC_DB_COUNT_PROFILE IC_DB_PREFIX_CANDIDATE
out="$root/qualification/target/evidence/prefix-count-candidate"
mkdir -p "$out"
source_copy="$(mktemp -d "$out/source.XXXXXX")"
git ls-files -z --cached --others --exclude-standard | tar --null -T - -cf "$out/source.tar"
tar -xf "$out/source.tar" -C "$source_copy"
patch --batch --fuzz=0 -d "$source_copy" -p1 < qualification/experiments/prefix-counts.patch
printf '%s\n' "$source_copy" > "$out/source-directory.txt"
git rev-parse HEAD > "$out/source-head.txt"
git diff --binary HEAD > "$out/working.patch"
cp qualification/experiments/prefix-counts.patch "$out/candidate.patch"
cp qualification/Cargo.lock "$out/Cargo.lock"
rustc -Vv > "$out/compiler.txt"
build() {
    local label="$1" manifest="$2" package="$3" features="$4"
    cargo tree --manifest-path "$manifest" --locked -p "$package" --features "$features" \
        --target wasm32-unknown-unknown -e normal,build --format '{p} features=[{f}]' > "$out/$label-features.txt"
    cargo build --manifest-path "$manifest" --locked -p "$package" --features "$features" \
        --target wasm32-unknown-unknown --release --target-dir "$root/qualification/target" \
        --message-format=json-render-diagnostics > "$out/$label-build.jsonl"
    local artifact
    artifact="$(jq -ers --arg name "${package//-/_}" '[.[] | select(.reason == "compiler-artifact" and .target.name == $name) | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one artifact") end' "$out/$label-build.jsonl")"
    cp "$artifact" "$out/$label.wasm"
}
cargo test --manifest-path "$source_copy/Cargo.toml" --locked -p ic-db --test prefix_counts \
    --target-dir "$root/target" -- --nocapture 2>&1 | tee "$out/native.log"
cargo clippy --manifest-path "$source_copy/Cargo.toml" --locked -p ic-db --test prefix_counts \
    --target-dir "$root/target" -- -D warnings
build control "$root/qualification/Cargo.toml" ic-db-records-benchmark-canister count-comparison
build candidate "$source_copy/qualification/Cargo.toml" ic-db-records-benchmark-canister count-comparison
build legacy "$root/qualification/Cargo.toml" ic-db-icydb-baseline-canister count-comparison
export IC_DB_COUNT_COMPARISON=1 IC_DB_PREFIX_WRITE_PROFILE=1 IC_DB_ICYDB_BASELINE_WASM="$out/legacy.wasm"
for label in control candidate; do
    if [[ "$label" == candidate ]]; then export IC_DB_PREFIX_CANDIDATE=1; fi
    IC_DB_RECORDS_BENCHMARK_WASM="$out/$label.wasm" cargo test --manifest-path qualification/Cargo.toml \
        --locked -p ic-db-m0-rollback-driver --test records_comparison -- --nocapture \
        2>&1 | tee "$out/$label.log"
done
build fault "$source_copy/qualification/Cargo.toml" ic-db-m1-records-canister fault-injection
IC_DB_M1_FAULT_WASM="$out/fault.wasm" cargo test --manifest-path qualification/Cargo.toml \
    --locked -p ic-db-m0-rollback-driver --test commit_failures -- --nocapture \
    2>&1 | tee "$out/fault.log"
