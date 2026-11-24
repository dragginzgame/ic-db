#!/usr/bin/env bash
# Compare point and bulk removal in disposable dependency and workspace copies.
set -euo pipefail
tree=${1:?Supply the ic-stable-structures 0.7.2 source directory}
storage=${2:?Supply the ic-storage 0.78.9 source directory}
experiment="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$experiment/../../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir "$work/db"
cp -R "$tree" "$work/tree"
cp -R "$storage" "$work/storage"
patch --batch --fuzz=0 -d "$work/tree" -p1 < "$experiment/leaf-batch.patch"
cd "$root"
git ls-files -co --exclude-standard -z -- Cargo.toml Cargo.lock LICENSE.md README.md rust-toolchain.toml crates qualification |
    tar -cf - --null -T - | tar -xf - -C "$work/db"
cd "$work/db"
patch --batch --fuzz=0 -p1 < "$experiment/fixture.patch"
# The expanded envelope belongs only to this comparison.
sed -i 's/^const MAX_BATCH: usize = 32;$/const MAX_BATCH: usize = 256;/' crates/ic-db/src/store/mod.rs
cat >> qualification/Cargo.toml <<'TOML'

[patch.crates-io]
ic-storage = { path = "../../storage" }
ic-stable-structures = { path = "../../tree" }
TOML
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/qualification/target}"
export IC_DB_DELETE_EXPERIMENT_256=1
cargo metadata --manifest-path qualification/Cargo.toml --offline --format-version 1 --filter-platform wasm32-unknown-unknown >/dev/null
for mode in point bulk; do
    if [[ "$mode" == bulk ]]; then
        patch --batch --fuzz=0 -d "$work/storage" -p1 < "$experiment/storage-adapter.patch"
    fi
    echo "Installed delete workload: $mode"
    cargo build --offline --locked --release --manifest-path qualification/Cargo.toml \
        -p ic-db-m1-records-canister --features query-measurements --target wasm32-unknown-unknown
    IC_DB_M1_RECORDS_WASM="$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/ic_db_m1_records_canister.wasm" \
        bash "$root/scripts/with-pocketic.sh" cargo test --offline --locked --manifest-path qualification/Cargo.toml \
        -p ic-db-m0-rollback-driver --test delete_costs -- --ignored --nocapture
done
