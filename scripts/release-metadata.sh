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
    local -a names
    for lock in Cargo.lock qualification/Cargo.lock; do
        names=(ic-db ic-db-macros)
        if [[ "$lock" == Cargo.lock ]]; then names+=(ic-db-cli); fi
        "$CARGO_RELEASE_METADATA_EDITOR" --max-input-bytes "$CARGO_RELEASE_METADATA_MAX_BYTES" \
            lock "$original/$lock" "$RELEASE_PREVIOUS" "$RELEASE_VERSION" \
            "${names[@]}" > "$candidate/$lock" || return
    done
    awk -v version="$RELEASE_VERSION" -v previous="$RELEASE_PREVIOUS" -v date="$RELEASE_DATE" \
        -f "$shared/finalize-release-changelog.awk" "$original/CHANGELOG.md" > "$candidate/CHANGELOG.md"
}

check_qualification_lock() (
    set -euo pipefail
    local evidence status=0
    evidence="$(mktemp -d "${TMPDIR:-/tmp}/ic-db-release-lock.XXXXXX")"
    trap 'status=$?; if [[ "$status" == 0 ]]; then rm -rf -- "$evidence"; else printf "Qualification lock evidence: %s\n" "$evidence" >&2; fi' EXIT
    git show "$RELEASE_SOURCE:qualification/Cargo.lock" > "$evidence/original.lock"
    "$CARGO_RELEASE_METADATA_EDITOR" --max-input-bytes "$CARGO_RELEASE_METADATA_MAX_BYTES" \
        lock "$evidence/original.lock" "$RELEASE_PREVIOUS" "$RELEASE_VERSION" \
        ic-db ic-db-macros > "$evidence/expected.lock"
    if [[ -n "${RELEASE_COMMIT:-}" ]]; then
        git show "$RELEASE_COMMIT:qualification/Cargo.lock" > "$evidence/actual.lock"
    else
        cp qualification/Cargo.lock "$evidence/actual.lock"
        cargo metadata --manifest-path qualification/Cargo.toml --locked \
            --format-version 1 > "$evidence/metadata.json"
    fi
    cmp "$evidence/expected.lock" "$evidence/actual.lock"
)
