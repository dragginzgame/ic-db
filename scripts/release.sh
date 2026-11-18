#!/usr/bin/env bash
# Public source releases. Shared tooling owns version edits, Git effects and recovery.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$PWD/.tools/host/bin:$PATH"
shared="$PWD/vendor/toko-tooling/scripts/ci"
installer="$PWD/vendor/toko-tooling/scripts/dev/install-rust-tools.sh"
export CARGO_RELEASE_METADATA_MAX_BYTES=8388608
source scripts/release-metadata.sh

metadata_check() {
    bash "$shared/cargo-release-metadata.sh" check ic-db || return
    select_metadata_editor || return
    check_qualification_lock
}

verified_source() {
    local receipt
    [[ "${RELEASE_SOURCE:-}" =~ ^[0-9a-f]{40}$ ]]
    receipt="$(git rev-parse --git-path "release-state/ic-db/$RELEASE_SOURCE.verified")"
    [[ -f "$receipt" && ! -L "$receipt" ]]
    [[ "$(cat "$receipt")" == "$RELEASE_SOURCE" ]]
}

case "${1:-}" in
    release-version)
        bash "$shared/read-cargo-workspace-version.sh" --stable Cargo.toml
        ;;
    release-files)
        printf '%s\0' Cargo.toml Cargo.lock qualification/Cargo.lock CHANGELOG.md
        ;;
    release-preflight)
        bash "$shared/check-release-source.sh"
        [[ "${RELEASE_REMOTE:?}" == origin && "${RELEASE_BRANCH:?}" == main ]]
        [[ "$(git remote get-url --push origin)" == https://github.com/dragginzgame/ic-db.git ]]
        [[ "$(gh repo view dragginzgame/ic-db --json visibility --jq .visibility)" == PUBLIC ]]
        bash "$shared/check-release-changelog.sh" "$RELEASE_VERSION" "$RELEASE_PREVIOUS" "$RELEASE_DATE" CHANGELOG.md
        ;;
    release-verify)
        [[ "${RELEASE_SOURCE:?}" =~ ^[0-9a-f]{40}$ ]]
        receipt="$(git rev-parse --git-path "release-state/ic-db/$RELEASE_SOURCE.verified")"
        # A failed recheck must not leave an earlier whole-gate pass admissible.
        rm -f -- "$receipt"
        [[ "$(git rev-parse HEAD)" == "${RELEASE_SOURCE:?}" ]]
        bash "$shared/check-release-source.sh"
        printf '%s\n' 'Running the complete IC DB gate; partial prior passes are diagnostics, not reusable proof.'
        printf '%s\n' 'Validation failure reruns this gate; saved post-validation attempts resume their recorded phase.'
        # Preflight is repeated by the shared runner before metadata preparation.
        # Prepare once here; a later metadata retry only repeats fresh admission.
        VALIDATION_REPOSITORY_ROOT="$PWD" \
          VALIDATION_LOG_DIR="$PWD/target/release-validation/$RELEASE_SOURCE" \
          bash "$shared/run-validation-targets.sh" --fail-fast release-setup
        # The shared logger retains failure logs; each gate executes once, in order.
        VALIDATION_REPOSITORY_ROOT="$PWD" \
          VALIDATION_LOG_DIR="$PWD/target/release-validation/$RELEASE_SOURCE" \
          bash "$shared/run-validation-targets.sh" --fail-fast \
          tools-check release-lint fmt-check test doc docs-check m0-contracts \
          m0-core m0-aggregation m0-maintenance \
          m1-records-wasm m1-commit-failures m3-maintenance-wasm m3-cli
        bash "$shared/check-release-source.sh"
        [[ "$(git rev-parse HEAD)" == "$RELEASE_SOURCE" ]]
        receipt="$(git rev-parse --git-path "release-state/ic-db/$RELEASE_SOURCE.verified")"
        mkdir -p "$(dirname "$receipt")"
        printf '%s\n' "$RELEASE_SOURCE" > "$receipt"
        ;;
    release-prepare-version)
        verified_source
        select_metadata_editor
        # shellcheck source=vendor/toko-tooling/scripts/ci/release-metadata-transaction.sh
        source "$shared/release-metadata-transaction.sh"
        release_metadata_prepare "${TMPDIR:-/tmp}/ic-db-release-metadata.XXXXXX" \
            render_release_metadata metadata_check Cargo.lock qualification/Cargo.lock CHANGELOG.md Cargo.toml
        make fmt-check
        ;;
    release-prepared-check|release-commit-check)
        verified_source
        metadata_check
        bash "$shared/check-release-source.sh" --allow Cargo.toml --allow Cargo.lock --allow qualification/Cargo.lock --allow CHANGELOG.md
        git diff --exit-code "$RELEASE_SOURCE" -- . ':!Cargo.toml' ':!Cargo.lock' ':!qualification/Cargo.lock' ':!CHANGELOG.md'
        if [[ "$1" == release-commit-check ]]; then
            git diff --exit-code
        fi
        ;;
    release-committed-check|release-tagged-check|release-push-check)
        verified_source
        [[ "$(git rev-parse "${RELEASE_COMMIT:?}^")" == "$RELEASE_SOURCE" ]]
        git diff --exit-code "$RELEASE_SOURCE" "$RELEASE_COMMIT" -- . ':!Cargo.toml' ':!Cargo.lock' ':!qualification/Cargo.lock' ':!CHANGELOG.md'
        metadata_check
        if [[ "$1" == release-push-check ]]; then
            [[ "$(gh repo view dragginzgame/ic-db --json visibility --jq .visibility)" == PUBLIC ]]
        fi
        ;;
    *) echo 'unknown release hook' >&2; exit 2 ;;
esac
