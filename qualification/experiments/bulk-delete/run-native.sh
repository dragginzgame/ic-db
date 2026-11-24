#!/usr/bin/env bash
# Apply the prototype only inside a disposable copy of the locked tree.
set -euo pipefail
source_tree=${1:?Supply the source directory of ic-stable-structures 0.7.2}
experiment="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir "$work/src"
cp -R "$source_tree" "$work/tree"
patch --batch --fuzz=0 -d "$work/tree" -p1 < "$experiment/leaf-batch.patch"
cp "$experiment/main.rs" "$work/src/main.rs"
cat > "$work/Cargo.toml" <<'TOML'
[workspace]
exclude = ["tree"]

[package]
name = "ic-db-bulk-delete-probe"
edition = "2024"
version = "0.0.0"
publish = false

[dependencies]
ic-stable-structures = { version = "=0.7.2", path = "tree" }
TOML
cargo run --release --offline --manifest-path "$work/Cargo.toml"
