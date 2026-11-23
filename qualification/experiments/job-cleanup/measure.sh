#!/usr/bin/env bash
# Build each exact supplier against the qualification lock in a disposable crate.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir "$work/src"
cp "$root/experiments/job-cleanup/main.rs" "$work/src/main.rs"
for version in 0.74.1 0.78.8; do
    cp "$root/Cargo.lock" "$work/Cargo.lock"
    cat > "$work/Cargo.toml" <<EOF
[package]
name = "ic-db-job-cleanup-probe"
version = "0.0.0"
edition = "2024"
publish = false

[features]
historical = []

[dependencies]
ic-storage = "=$version"
EOF
    features=()
    if [[ "$version" == 0.74.1 ]]; then features=(--features historical); fi
    echo "ic-storage $version"
    CARGO_TARGET_DIR="$root/target" cargo run --manifest-path "$work/Cargo.toml" --offline --quiet "${features[@]}"
done
