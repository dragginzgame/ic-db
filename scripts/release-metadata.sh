#!/usr/bin/env bash
# Release metadata adapter for the product workspace and its qualification lockfile.
# Shared tooling owns transactional replacement and Cargo/changelog editing.
# shellcheck disable=SC2154 # shared and installer are supplied by release.sh.

select_metadata_editor() {
    CARGO_RELEASE_METADATA_EDITOR="$(bash "$installer" --consumer "$PWD" \
        --package ic-build --version 0.84.1 --bin ic-build-release-metadata \
        --profile debug --features cli --check --wait-seconds 30)" || return
    export CARGO_RELEASE_METADATA_EDITOR
}

pin_file=ci/dependency-pinning-exceptions.json

render_macro_pin() {
    local pin
    pin="$(yq -er '.workspace.dependencies.ic-db-macros.version' "$2")" || return
    [[ "$pin" == "=$RELEASE_VERSION" ]] || return 1
    jq -e --arg previous "=$RELEASE_PREVIOUS" --arg pin "$pin" '
        def macro: .rule == "cargo-exact" and .file == "Cargo.toml" and .subject == "ic-db-macros";
        if ([.[] | select(macro) | .value] == [$previous])
        then map(if macro then .value = $pin else . end)
        else error("expected one macro pin matching the previous release") end' "$1"
}

render_release_lock() {
    "$CARGO_RELEASE_METADATA_EDITOR" --max-input-bytes "$CARGO_RELEASE_METADATA_MAX_BYTES" \
        lock "$1" "$RELEASE_PREVIOUS" "$RELEASE_VERSION" ic-db ic-db-macros ic-db-cli
}

render_release_metadata() {
    local original="$1" candidate="$2" evidence="${2%/*}" lock
    cargo metadata --locked --format-version 1 --no-deps > "$evidence/metadata.json" || return
    yq -p toml -o json '.' "$original/Cargo.toml" > "$evidence/manifest.json" || return
    jq -n --slurpfile manifest "$evidence/manifest.json" --slurpfile metadata "$evidence/metadata.json" '
        {manifest: $manifest[0], members: ($metadata[0] | . as $m |
            [.packages[] | select(.id as $id | $m.workspace_members | index($id)) |
                {name, manifest_path}])}' > "$evidence/workspace-input.json" || return
    "$CARGO_RELEASE_METADATA_EDITOR" --max-input-bytes "$CARGO_RELEASE_METADATA_MAX_BYTES" \
        manifest Cargo.toml "$RELEASE_PREVIOUS" "$RELEASE_VERSION" \
        "$evidence/workspace-input.json" > "$candidate/Cargo.toml" || return
    render_macro_pin "$original/$pin_file" "$candidate/Cargo.toml" > "$candidate/$pin_file" || return
    for lock in Cargo.lock qualification/Cargo.lock; do
        render_release_lock "$original/$lock" > "$candidate/$lock" || return
    done
    awk -v version="$RELEASE_VERSION" -v previous="$RELEASE_PREVIOUS" -v date="$RELEASE_DATE" \
        -f "$shared/finalize-release-changelog.awk" "$original/CHANGELOG.md" > "$candidate/CHANGELOG.md"
}

check_local_release_metadata() (
    set -euo pipefail
    local evidence status=0
    evidence="$(mktemp -d "${TMPDIR:-/tmp}/ic-db-release-check.XXXXXX")"
    trap 'status=$?; if [[ "$status" == 0 ]]; then rm -rf -- "$evidence"; else printf "Release metadata evidence: %s\n" "$evidence" >&2; fi' EXIT
    git show "$RELEASE_SOURCE:qualification/Cargo.lock" > "$evidence/original.lock" || return
    render_release_lock "$evidence/original.lock" > "$evidence/expected.lock" || return
    if [[ -n "${RELEASE_COMMIT:-}" ]]; then
        git show "$RELEASE_COMMIT:qualification/Cargo.lock" > "$evidence/actual.lock" || return
    else
        cp qualification/Cargo.lock "$evidence/actual.lock" || return
        cargo metadata --manifest-path qualification/Cargo.toml --locked \
            --format-version 1 > "$evidence/metadata.json" || return
    fi
    cmp "$evidence/expected.lock" "$evidence/actual.lock" || return
    check_macro_pin "$evidence"
)

check_macro_pin() {
    local evidence="$1"
    git show "$RELEASE_SOURCE:$pin_file" > "$evidence/original-pin.json" || return
    if [[ -n "${RELEASE_COMMIT:-}" ]]; then
        git show "$RELEASE_COMMIT:Cargo.toml" > "$evidence/manifest.toml" || return
        git show "$RELEASE_COMMIT:$pin_file" > "$evidence/actual-pin.json" || return
    else
        cp Cargo.toml "$evidence/manifest.toml" || return
        cp "$pin_file" "$evidence/actual-pin.json" || return
    fi
    render_macro_pin "$evidence/original-pin.json" "$evidence/manifest.toml" > "$evidence/expected-pin.json" || return
    cmp "$evidence/expected-pin.json" "$evidence/actual-pin.json"
}
