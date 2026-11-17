#!/usr/bin/env bash
# Build the committed producer witness selected by the client's Canic dependency.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
out="$root/qualification/target/evidence/canic-inspection"
mkdir -p "$out"
cargo metadata --manifest-path qualification/Cargo.toml --locked --format-version 1 > "$out/metadata.json"
manifest=$(jq -er '.packages[] | select(.name == "canic") | .manifest_path' "$out/metadata.json")
source_id=$(jq -er '.packages[] | select(.name == "canic") | .source' "$out/metadata.json")
revision=${source_id##*#}
source_root=$(cd "$(dirname "$manifest")/../.." && pwd)
[[ "$revision" =~ ^[0-9a-f]{40}$ && "$(git -C "$source_root" rev-parse HEAD)" == "$revision" ]]
[[ -z "$(git -C "$source_root" status --porcelain --untracked-files=no)" ]]
printf '%s\n' "$source_id" > "$out/source.txt"
for label in normal fault incompatible; do
    features=()
    if [[ "$label" == fault ]]; then features=(--features fault-injection); fi
    if [[ "$label" == incompatible ]]; then features=(--features incompatible-schema); fi
    cargo build --manifest-path "$source_root/qualification/Cargo.toml" --locked \
        -p canic-db-witness "${features[@]}" --target wasm32-unknown-unknown --release \
        --target-dir "$root/qualification/target/canic-inspection-build" \
        --message-format=json-render-diagnostics > "$out/$label-build.jsonl"
    artifact=$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "canic_db_witness") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Canic witness") end' "$out/$label-build.jsonl")
    cp "$artifact" "$out/$label.wasm"
done
sha256sum "$out"/*.wasm > "$out/wasm.sha256"
