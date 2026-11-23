#!/usr/bin/env bash
# Explicit record comparisons or entity growth; no implicit release/CI selection.
set -euo pipefail
cd "$(dirname "$0")/.."
workload="${1:-query-matrix}"
unset IC_DB_KEYSET_COMPARISON IC_DB_EXISTS_COMPARISON IC_DB_COUNT_PATHS IC_DB_FLUENT_COMPARISON IC_DB_AGGREGATE_COMPARISON IC_DB_COUNT_COMPARISON IC_DB_COUNT_PROFILE IC_DB_AGGREGATE_PATHS IC_DB_PREFIX_CANDIDATE IC_DB_PREFIX_WRITE_PROFILE
case "$workload" in
    fluent)
        evidence="$PWD/qualification/target/evidence/fluent-comparison"
        engines=(records-benchmark icydb-baseline)
        test_name=records_comparison
        export IC_DB_FLUENT_COMPARISON=1
        ;;
    keyset)
        evidence="$PWD/qualification/target/evidence/keyset-comparison-02313"
        engines=(records-benchmark icydb-baseline)
        test_name=records_comparison
        export IC_DB_KEYSET_COMPARISON=1
        ;;
    aggregate)
        evidence="$PWD/qualification/target/evidence/aggregate-comparison"
        engines=(records-benchmark icydb-baseline)
        test_name=records_comparison
        export IC_DB_AGGREGATE_COMPARISON=1
        ;;
    ranked-listing)
        evidence="$PWD/qualification/target/evidence/ranked-listing"
        engines=(records-benchmark)
        test_name=ranked_listing
        ;;
    query-matrix)
        evidence="${IC_DB_QUERY_MATRIX_DIR:-$PWD/qualification/target/evidence/query-matrix}"
        engines=(records-benchmark icydb-baseline)
        test_name=query_matrix
        export IC_DB_QUERY_MATRIX_DIR="$evidence"
        ;;
    exists)
        evidence="${IC_DB_QUERY_MATRIX_DIR:-$PWD/qualification/target/evidence/exists-comparison}"
        engines=(records-benchmark icydb-baseline)
        test_name=query_matrix
        export IC_DB_QUERY_MATRIX_DIR="$evidence" IC_DB_EXISTS_COMPARISON=1
        ;;
    count-paths)
        evidence="${IC_DB_COUNT_EVIDENCE_DIR:-$PWD/qualification/target/evidence/count-paths}"
        engines=(records-benchmark icydb-baseline)
        test_name=query_matrix
        export IC_DB_QUERY_MATRIX_DIR="$evidence" IC_DB_COUNT_PATHS=1
        ;;
    aggregate-paths)
        evidence="${IC_DB_QUERY_MATRIX_DIR:-$PWD/qualification/target/evidence/aggregate-paths}"
        engines=(records-benchmark icydb-baseline)
        test_name=query_matrix
        export IC_DB_QUERY_MATRIX_DIR="$evidence" IC_DB_AGGREGATE_PATHS=1
        ;;
    count)
        evidence="${IC_DB_COUNT_EVIDENCE_DIR:-$PWD/qualification/target/evidence/count-comparison}"
        engines=(records-benchmark icydb-baseline)
        test_name=records_comparison
        export IC_DB_COUNT_COMPARISON=1
        ;;
    count-profile)
        evidence="${IC_DB_COUNT_EVIDENCE_DIR:-$PWD/qualification/target/evidence/count-profile}"
        engines=(records-benchmark)
        test_name=records_comparison
        export IC_DB_COUNT_COMPARISON=1 IC_DB_COUNT_PROFILE=1
        ;;
    growth)
        evidence="$PWD/qualification/target/evidence/entity-growth"
        engines=(entities-1 entities-8 entities-32 retained-1 retained-8 retained-32)
        test_name=entity_growth
        ;;
    queries)
        evidence="${IC_DB_QUERY_GROWTH_DIR:-$PWD/qualification/target/evidence/query-growth}"
        engines=(direct two-indexes direct-entities one shapes maps entities)
        test_name=query_growth
        ;;
    *) printf 'Expected ranked-listing, fluent, keyset, aggregate, aggregate-paths, query-matrix, exists, count-paths, count, count-profile, growth or queries workload\n' >&2; exit 2 ;;
esac
mkdir -p "$evidence"
if [[ "$test_name" == query_matrix ]]; then
    # Preserve the driver even while this new benchmark is still untracked.
    cp qualification/ic-db-m0-rollback-driver/tests/query_matrix.rs "$evidence/query-matrix.rs"
    cp qualification/Cargo.lock "$evidence/Cargo.lock"
fi
rustc -Vv > "$evidence/compiler.txt"
git rev-parse HEAD > "$evidence/source-commit.txt"
git status --short > "$evidence/source-status.txt"
git diff --binary HEAD > "$evidence/source.diff"
git ls-files -z --cached --others --exclude-standard -- \
    Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates qualification scripts Makefile |
    while IFS= read -r -d '' input; do
        # An intentional tracked deletion is captured by source.diff, not hashed
        # as if the old path still existed. Other unreadable inputs must fail.
        if [[ ! -e "$input" && ! -L "$input" ]] &&
            [[ "$(git diff --name-only --diff-filter=D HEAD -- "$input")" == "$input" ]]; then
            printf 'Recorded deletion in source.diff: %s\n' "$input" >&2
        else
            shasum -a 256 -- "$input"
        fi
    done > "$evidence/source-sha256.txt"
printf 'RUSTFLAGS=%s\nCARGO_ENCODED_RUSTFLAGS=%s\n' \
    "${RUSTFLAGS-}" "${CARGO_ENCODED_RUSTFLAGS-}" > "$evidence/build-environment.txt"

for engine in "${engines[@]}"; do
    package="ic-db-$engine-canister"
    build_features=""
    if [[ "$workload" == query-matrix || "$workload" == aggregate-paths ]]; then
        build_features=query-matrix
    fi
    if [[ "$workload" == ranked-listing ]]; then
        build_features=ranked-listing
    fi
    if [[ "$workload" == exists ]]; then
        build_features=exists-comparison
    fi
    if [[ "$workload" == keyset ]]; then
        build_features=keyset-comparison
    fi
    if [[ "$workload" == fluent ]]; then
        build_features=fluent-comparison
    fi
    if [[ "$workload" == aggregate ]]; then
        build_features=aggregate-comparison
    fi
    if [[ "$workload" == count || "$workload" == count-paths ]]; then
        build_features="count-comparison"
    fi
    if [[ "$workload" == count-profile || ( "$workload" == count-paths && "$engine" == records-benchmark ) ]]; then
        build_features="count-profile"
    fi
    if [[ "$workload" == growth ]]; then
        package=ic-db-entity-growth-canister
        count="${engine##*-}"
        if [[ "$count" != 1 ]]; then
            build_features="entities-$count"
        fi
        if [[ "$engine" == retained-* ]]; then
            build_features="${build_features:+$build_features,}retained-host"
        fi
    fi
    if [[ "$workload" == queries ]]; then
        package=ic-db-entity-growth-canister
        build_features=retained-host
        case "$engine" in
            two-indexes) build_features+=,second-index ;;
            direct-entities) build_features+=,entities-8 ;;
            one) build_features+=,fluent ;;
            shapes) build_features+=,fluent-shapes ;;
            maps) build_features+=,fluent-maps ;;
            entities) build_features+=,fluent-shapes,entities-8 ;;
        esac
    fi
    cargo tree --manifest-path qualification/Cargo.toml --locked -p "$package" \
        --features "$build_features" \
        --target wasm32-unknown-unknown -e normal,build --format '{p} features=[{f}]' \
        > "$evidence/$engine-features.txt"
    cargo build --manifest-path qualification/Cargo.toml --locked -p "$package" \
        --features "$build_features" \
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
export IC_DB_ENTITY_GROWTH_DIR="$evidence"
export IC_DB_QUERY_GROWTH_DIR="$evidence"
bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver \
    --test "$test_name" -- --nocapture 2>&1 | tee "$evidence/results.log"
