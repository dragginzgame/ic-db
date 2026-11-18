.DEFAULT_GOAL := help
export CARGO_BUILD_WARNINGS := deny
FORMAT_RUST_ARGS := --extra-workspace qualification
TOKO_TOOLING_RELEASE := 1
RELEASE_REMOTE := origin
RELEASE_BRANCH := main
RELEASE_DELIVERY_POLICY := scripts/release-delivery.txt
CARGO_PUBLISH_PACKAGES := ic-db-macros ic-db
CARGO_PUBLISH_REGISTRY := crates-io
CARGO_PUBLISH_ALL_FEATURES := 1
override CARGO_PUBLISH_VERIFY_TARGETS := publication-tag
publish-check: override CARGO_PUBLISH_VERIFY_TARGETS := check
include vendor/toko-tooling/make/consumer.mk
include vendor/toko-tooling/make/cargo-dependencies.mk

.PHONY: help check test clippy doc docs-check ci db m0-setup m0-contracts m0-rollback m0-batch m0-reads m0-aggregation m0-maintenance m0-maintenance-artifacts m0-icydb-baseline
DB_INSTALL_PREFIX ?= $(HOME)/.local
help:
	@printf '%s\n' \
	  'make check / test / clippy / doc  Check the runtime and schema macro' \
	  'make docs-check                  Check local documentation links' \
	  'make m0-setup                    Explicitly prepare M0 PocketIC server' \
	  'make m0-contracts                Check native numeric/schema/row/query contracts' \
	  'make m0-rollback                 Run only the Wasm write-rollback probe' \
	  'make m0-endpoints                Check standalone hooks and reply rollback' \
	  'make m0-batch                    Run final-state batch constraint probe' \
	  'make m0-reads                    Run bounded ordering/page/DISTINCT probe' \
	  'make m0-aggregation              Run only the resumable grouped-sum probe' \
	  'make m0-maintenance              Run interrupted schema-maintenance probe' \
	  'make m0-icydb-baseline           Measure the frozen SQL-disabled engine' \
	  'make m1-records                 Check generated records natively and in Wasm' \
	  'make m2-queries                 Check bounded fluent query pages' \
	  'make m2-jobs                    Check retained aggregates and rollback' \
	  'make m2-contention              Check retained aggregates and shared job capacity' \
	  'make m2-cli                     Check the fluent CLI and migrated query caller' \
	  'make m3-maintenance             Check product migration, cutover and rollback' \
	  'make m3-references              Check cross-chunk migration references' \
	  'make m3-jobs                    Check aggregate custody during maintenance' \
	  'make m1-commit-failures         Check product rollback inside supplier writes' \
	  'make m4-lifecycle               Check mixed writes, queries, jobs and migration' \
	  'make m1-write-envelope         Measure maximal rows, batches and index fan-out' \
	  'make m1-records-comparison      Measure matched scalar IC DB/IcyDB workloads' \
	  'make m1-envelope-comparison     Compare high-fan-out IC DB/IcyDB writes' \
	  'make m1-entity-growth           Measure 1/8/32-entity Wasm and execution growth' \
	  'make m2-query-growth            Measure fluent shapes, maps and entity growth' \
	  'make m2-count-profile           Separate count opening, scan and row-fetch costs' \
	  'make m2-count-comparison        Compare entity and indexed-prefix count costs' \
	  'make m2-aggregate-comparison    Compare grouped IC DB/IcyDB aggregate costs' \
	  'make m2-fluent-comparison       Compare fluent IC DB/IcyDB listings and sorting' \
	  'make m2-query-matrix            Compare broad query/selectivity/population matrix' \
	  'make m2-aggregate-paths         Compare primary and explicit-index aggregates' \
	  'make m2-prefix-candidate        Measure isolated maintained-prefix experiment' \
	  'make db                         Install db; run db to open the console' \
	  'make publish / publish-check     Upload tagged product crates / rehearse publication' \
	  'make release-patch/minor/major   Qualify and deliver a public source release' \
	  'make fmt / fmt-check             Format / check Rust and manifests' \
	  'make deps-list / deps-update     Inspect / update Cargo dependencies' \
	  'make install-tools / tools-check Prepare / admit development tools' \
	  'make toko-tooling-check          Verify Cargo/snapshot agreement' \
	  'make install-hooks              Install the stock formatting hook' \
	  'toko-cloc                       Report source counts (PATH launcher)' \
	  'make ci                         Aggregate native checks; no full database qualification'

check:
	cargo check --locked -p ic-db -p ic-db-macros

test:
	cargo test --locked -p ic-db -p ic-db-macros --features ic-db/build

clippy:
	cargo clippy --locked -p ic-db -p ic-db-macros --features ic-db/build --all-targets -- -D warnings

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --locked -p ic-db -p ic-db-macros --features ic-db/build --no-deps

docs-check:
	perl vendor/toko-tooling/scripts/ci/check-documentation-links.pl --root . README.md AGENTS.md CHANGELOG.md docs/design/ic-db-system-design.md qualification/README.md

.PHONY: qualification-tooling-check
toko-tooling-check: qualification-tooling-check
qualification-tooling-check:
	@set -eu; export PATH="$(CURDIR)/.tools/host/bin:$$PATH"; \
	  root_selection="$$(perl "$(TOKO_TOOLING_ROOT)/scripts/dev/tooling-lock.pl" "$(CURDIR)")"; \
	  qualification_selection="$$(perl "$(TOKO_TOOLING_ROOT)/scripts/dev/tooling-lock.pl" "$(CURDIR)/qualification")"; \
	  test "$$root_selection" = "$$qualification_selection" || { \
	    printf '%s\n' 'Product and qualification tooling selections differ.' >&2; exit 1; \
	  }; \
	  printf '%s\n' 'Qualification tooling declarations and lock match the product selection.'

m0-setup:
	@umask 077; launcher="$$(bash vendor/toko-tooling/scripts/dev/install-rust-tools.sh --consumer "$(CURDIR)" --package ic-testkit --version 0.110.0 --bin ic-testkit-server --profile release --features cli)" && \
	  mkdir -p "$(CURDIR)/qualification/target/evidence/testkit" && \
	  attempt="$$(mktemp -d "$(CURDIR)/qualification/target/evidence/testkit/setup.XXXXXX")" && \
	  "$$launcher" setup "$(CURDIR)/.tools/testkit-server" "$$attempt/evidence" "$$(command -v curl)" "$$(command -v gzip)"

m0-contracts:
	cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test numeric_contract --test schema_identity --test row_codec --test nested_row_codec --test query_composition -- --nocapture

# A benchmark selection, not part of aggregate scaffold CI or routine release.
# Cargo owns the immutable Git source; no local sibling paths or edits are used.
m0-icydb-baseline:
	@mkdir -p qualification/target/evidence/icydb-baseline
	rustc -Vv > qualification/target/evidence/icydb-baseline/compiler.txt
	cargo tree --manifest-path qualification/Cargo.toml --locked --offline -p ic-db-icydb-baseline-canister --target wasm32-unknown-unknown -e normal,build --format '{p} features=[{f}]' > qualification/target/evidence/icydb-baseline/features.txt
	cargo build --manifest-path qualification/Cargo.toml --locked --offline -p ic-db-icydb-baseline-canister --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/icydb-baseline/build.jsonl
	@IC_DB_ICYDB_BASELINE_WASM="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_icydb_baseline_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/icydb-baseline/build.jsonl)" && \
	  export IC_DB_ICYDB_BASELINE_WASM && \
	  bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked --offline -p ic-db-m0-rollback-driver --test icydb_baseline -- --nocapture

.PHONY: m1-records-comparison m2-fluent-comparison m2-aggregate-comparison m2-count-comparison m2-count-profile
m1-records-comparison:
	bash scripts/measure-records.sh

m2-fluent-comparison:
	bash scripts/measure-records.sh fluent

.PHONY: m2-query-matrix
m2-query-matrix:
	bash scripts/measure-records.sh query-matrix

.PHONY: m2-aggregate-paths
m2-aggregate-paths:
	bash scripts/measure-records.sh aggregate-paths

.PHONY: m2-prefix-candidate
m2-prefix-candidate:
	bash scripts/measure-prefix-counts.sh

m2-aggregate-comparison:
	bash scripts/measure-records.sh aggregate

m2-count-comparison:
	bash scripts/measure-records.sh count

m2-count-profile:
	bash scripts/measure-records.sh count-profile

.PHONY: m1-envelope-comparison
m1-envelope-comparison:
	bash scripts/measure-records.sh envelope

.PHONY: m1-entity-growth
m1-entity-growth:
	bash scripts/measure-records.sh growth

.PHONY: m2-query-growth
m2-query-growth:
	bash scripts/measure-records.sh queries

.PHONY: m0-core-artifact m0-core
m0-core-artifact:
	@mkdir -p qualification/target/evidence/m0
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-canister --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m0/core-build.jsonl
	@artifact="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m0_rollback_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/m0/core-build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/m0/core.wasm

# One artifact and server session, with the same four independent test binaries.
m0-core: m0-core-artifact
	IC_DB_M0_WASM="$(CURDIR)/qualification/target/evidence/m0/core.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test entrypoints --test rollback --test batch --test reads -- --nocapture

m0-rollback: m0-core-artifact
	IC_DB_M0_WASM="$(CURDIR)/qualification/target/evidence/m0/core.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test rollback message_rollback -- --nocapture

.PHONY: m0-endpoints
m0-endpoints: m0-core-artifact
	IC_DB_M0_WASM="$(CURDIR)/qualification/target/evidence/m0/core.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test entrypoints -- --nocapture

m0-aggregation:
	@mkdir -p qualification/target/evidence/m0
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-aggregation-canister --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m0/aggregation-build.jsonl
	@IC_DB_M0_AGGREGATION_WASM="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m0_aggregation_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/m0/aggregation-build.jsonl)" && \
	  export IC_DB_M0_AGGREGATION_WASM && \
	  bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test aggregation revision_aggregation -- --nocapture

m0-batch: m0-core-artifact
	IC_DB_M0_WASM="$(CURDIR)/qualification/target/evidence/m0/core.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test batch final_state_batches -- --nocapture

m0-maintenance-artifacts:
	@set -eu; out=qualification/target/evidence/m0; mkdir -p "$$out"; \
	  for variant in a b bprime bad; do \
	    case "$$variant" in a) features='' ;; b) features=target ;; bprime) features=changed-transform ;; bad) features=bad-upgrade ;; esac; \
	    cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-maintenance-canister --target wasm32-unknown-unknown --release --no-default-features --features "$$features" --message-format=json-render-diagnostics > "$$out/maintenance-$$variant.jsonl"; \
	    artifact="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m0_maintenance_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' "$$out/maintenance-$$variant.jsonl")"; \
	    cp "$$artifact" "$$out/maintenance-$$variant.wasm"; \
	  done

m0-reads: m0-core-artifact
	IC_DB_M0_WASM="$(CURDIR)/qualification/target/evidence/m0/core.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test reads bounded_reads -- --nocapture

m0-maintenance: m0-maintenance-artifacts
	IC_DB_M0_MAINTENANCE_DIR="$(CURDIR)/qualification/target/evidence/m0" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test maintenance -- --nocapture

.PHONY: m1-records m1-records-artifact m1-records-wasm
m1-records-artifact:
	@mkdir -p qualification/target/evidence/m1
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m1/records-build.jsonl
	@artifact="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m1_records_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/m1/records-build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/m1/records.wasm

m1-records:
	cargo test --locked -p ic-db --test enum_collections
	cargo test --locked -p ic-db -p ic-db-macros --features ic-db/build --test records --test cardinality --test enum_indexes --test schema_reuse --test key_bounds --test defaults --test managed --test nested --test lists --test sets --test maps --test u256 --test floats --test accounts --test ulids --test ulid_generation --test dates --test times --test time_values --test decimals --test primary_keys --test scalar_keys --test schema_build --lib
	$(MAKE) m1-records-wasm

m1-records-wasm: m1-records-artifact
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m1/records.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test records --test enum_collections --test defaults --test managed --test generation --test principal_keys --test account_keys --test sorting -- --nocapture

.PHONY: m1-commit-failures
.PHONY: m2-queries m2-sort-costs m2-sort-costs-artifact
m2-queries: m1-records-artifact
	cargo test --locked -p ic-db --test queries
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m1/records.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test queries --test sorting --test live_pages -- --nocapture

m2-sort-costs-artifact:
	@mkdir -p qualification/target/evidence/m2
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features query-measurements --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m2/sort-costs-build.jsonl
	@artifact="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m1_records_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/m2/sort-costs-build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/m2/sort-costs.wasm

m2-sort-costs: m2-sort-costs-artifact
	IC_DB_QUERY_COSTS=1 IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/sort-costs.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test sorting sorting_reads_complete_input -- --nocapture

.PHONY: m2-jobs m2-jobs-artifact m2-contention m2-job-envelope
m2-jobs: m2-jobs-artifact
	cargo test --locked -p ic-db --test jobs
	IC_DB_M2_JOBS_WASM="$(CURDIR)/qualification/target/evidence/m2/jobs.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test jobs --test jobs_contention --test jobs_envelope -- --nocapture

m2-job-envelope: m2-jobs-artifact
	IC_DB_M2_JOBS_WASM="$(CURDIR)/qualification/target/evidence/m2/jobs.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test jobs_envelope -- --nocapture

m2-contention: m2-jobs-artifact
	IC_DB_M2_JOBS_WASM="$(CURDIR)/qualification/target/evidence/m2/jobs.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test jobs_contention -- --nocapture

m2-jobs-artifact:
	@mkdir -p qualification/target/evidence/m2
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features aggregate-jobs --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m2/jobs-build.jsonl
	@artifact="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m1_records_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/m2/jobs-build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/m2/jobs.wasm

.PHONY: m3-maintenance m3-maintenance-wasm
m3-maintenance:
	cargo test --locked -p ic-db --test maintenance --test schema_copy
	$(MAKE) m3-maintenance-wasm

m3-maintenance-wasm:
	@mkdir -p qualification/target/evidence/m3
	@set -eu; for feature in maintenance maintenance-v2 maintenance-faults; do \
	  cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features "$$feature" --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > "qualification/target/evidence/m3/$$feature-build.jsonl"; \
	  artifact="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m1_records_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' "qualification/target/evidence/m3/$$feature-build.jsonl")"; \
	  cp "$$artifact" "qualification/target/evidence/m3/$$feature.wasm"; \
	done
	IC_DB_M3_DIR="$(CURDIR)/qualification/target/evidence/m3" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test product_maintenance --test product_copy --test product_capacity --test product_references -- --nocapture
	$(MAKE) m3-jobs

.PHONY: m3-references
m3-references:
	@mkdir -p qualification/target/evidence/m3
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features maintenance-faults --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m3/references-build.jsonl
	@artifact="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m1_records_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/m3/references-build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/m3/maintenance-faults.wasm
	IC_DB_M3_DIR="$(CURDIR)/qualification/target/evidence/m3" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test product_references -- --nocapture

.PHONY: m3-jobs
m3-jobs:
	@mkdir -p qualification/target/evidence/m3
	@set -eu; for feature in maintenance maintenance-v2; do \
	  cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features "aggregate-jobs,$$feature" --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > "qualification/target/evidence/m3/jobs-$$feature-build.jsonl"; \
	  artifact="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m1_records_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' "qualification/target/evidence/m3/jobs-$$feature-build.jsonl")"; \
	  cp "$$artifact" "qualification/target/evidence/m3/jobs-$$feature.wasm"; \
	done
	IC_DB_M3_DIR="$(CURDIR)/qualification/target/evidence/m3" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test jobs_maintenance -- --nocapture

.PHONY: m2-cli m2-cli-artifact
m2-cli-artifact:
	@mkdir -p qualification/target/evidence/m2
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features retained-console --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m2/console-build.jsonl
	@artifact="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m1_records_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/m2/console-build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/m2/console.wasm

m2-cli: m2-cli-artifact
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/console.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --bin db --test console_text --test console_aggregates -- --nocapture
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/console.wasm" IC_DB_CLI_RETAINED=1 bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test records console_inspects_and_queries -- --nocapture
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/console.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test records product_records_and_message_boundary -- --nocapture

.PHONY: m3-cli
m3-cli: m2-cli-artifact
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/console.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --bin db --test console_migration --test console_text --test console_aggregates -- --nocapture
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/console.wasm" cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test console_terminal -- --nocapture

.PHONY: m1-write-envelope
m1-write-envelope: m1-records-artifact
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m1/records.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test write_envelope -- --nocapture

m1-commit-failures:
	@mkdir -p qualification/target/evidence/m1
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features fault-injection --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m1/fault-build.jsonl
	@artifact="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m1_records_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/m1/fault-build.jsonl)" && \
	  IC_DB_M1_FAULT_WASM="$$artifact" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test commit_failures -- --nocapture

.PHONY: m4-lifecycle
m4-lifecycle:
	@mkdir -p qualification/target/evidence/mixed-lifecycle
	@git rev-parse HEAD > qualification/target/evidence/mixed-lifecycle/source-head.txt
	@git diff --binary > qualification/target/evidence/mixed-lifecycle/working.patch
	@rustc -Vv > qualification/target/evidence/mixed-lifecycle/compiler.txt
	@cp qualification/Cargo.lock qualification/target/evidence/mixed-lifecycle/Cargo.lock
	@cp qualification/ic-db-m1-records-canister/src/mixed_lifecycle.rs qualification/target/evidence/mixed-lifecycle/host.rs
	@cp qualification/ic-db-m0-rollback-driver/tests/mixed_lifecycle.rs qualification/target/evidence/mixed-lifecycle/driver.rs
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features mixed-lifecycle --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/mixed-lifecycle/build.jsonl
	@artifact="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m1_records_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/mixed-lifecycle/build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/mixed-lifecycle/host.wasm
	@if IC_DB_M4_WASM="$$PWD/qualification/target/evidence/mixed-lifecycle/host.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test mixed_lifecycle -- --nocapture > qualification/target/evidence/mixed-lifecycle/run.log 2>&1; then \
	  cat qualification/target/evidence/mixed-lifecycle/run.log; \
	else cat qualification/target/evidence/mixed-lifecycle/run.log; exit 1; fi

db: m2-cli-artifact
	@set -eu; out=qualification/target/evidence/m1; \
	  mkdir -p "$$out"; \
	  fixture=qualification/target/evidence/m2/console.wasm; \
	  cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --bin db --release --message-format=json-render-diagnostics > "$$out/db-build.jsonl"; \
	  binary="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "db") | .executable | select(. != null)] | if length == 1 then .[0] else error("expected one db binary") end' "$$out/db-build.jsonl")"; \
	  mkdir -p "$(DB_INSTALL_PREFIX)/bin" "$(DB_INSTALL_PREFIX)/share/ic-db/m1"; \
	  install -m 644 "$$fixture" "$(DB_INSTALL_PREFIX)/share/ic-db/m1/records.wasm"; \
	  install -m 755 "$$binary" "$(DB_INSTALL_PREFIX)/bin/db"; \
	  printf 'Installed %s/bin/db. Run db (ensure %s/bin is on PATH).\n' "$(DB_INSTALL_PREFIX)" "$(DB_INSTALL_PREFIX)"

.PHONY: publication-tag
publication-tag:
	@bash vendor/toko-tooling/scripts/ci/check-release-tag.sh "$$(git rev-parse HEAD)" "$$($(MAKE) --no-print-directory -s release-version)"

.PHONY: release-adapter-test release-setup
release-adapter-test:
	perl scripts/tests/release.t

release-setup:
	cargo fetch --locked
	cargo fetch --manifest-path qualification/Cargo.toml --locked
	$(MAKE) install-tools
	$(MAKE) release-tools
	$(MAKE) m0-setup

ci: fmt-check clippy test doc docs-check release-adapter-test

.PHONY: release-version release-preflight release-verify release-prepare-version release-prepared-check release-files release-commit-check release-committed-check release-tagged-check release-push-check release-lint release-tools
release-version release-preflight release-verify release-prepare-version release-prepared-check release-files release-commit-check release-committed-check release-tagged-check release-push-check:
	@bash scripts/release.sh "$@"

release-tools:
	bash vendor/toko-tooling/scripts/dev/install-rust-tools.sh --consumer "$(CURDIR)" --package ic-build --version 0.84.1 --bin ic-build-release-metadata --profile debug --features cli

release-lint:
	$(MAKE) clippy
	# Native callers/schema builds and IC-only fixtures have different target APIs.
	cargo clippy --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver -p ic-db-m1-records-schema --all-targets -- -D warnings
	cargo clippy --manifest-path qualification/Cargo.toml --locked --workspace --exclude ic-db-m0-rollback-driver --exclude ic-db-icydb-baseline-schema --exclude ic-db-icydb-baseline-canister --lib --target wasm32-unknown-unknown -- -D warnings
	cargo clippy --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features fault-injection --target wasm32-unknown-unknown -- -D warnings
	cargo clippy --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features maintenance-v2 --target wasm32-unknown-unknown -- -D warnings
	cargo clippy --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features maintenance-faults --target wasm32-unknown-unknown -- -D warnings
	cargo clippy --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features retained-console --target wasm32-unknown-unknown -- -D warnings

.PHONY: m4-inspection-artifact m4-inspection
m4-inspection-artifact:
	bash scripts/build-canic-inspection.sh

m4-inspection: m4-inspection-artifact
	IC_DB_CANIC_WASM_DIR="$(CURDIR)/qualification/target/evidence/canic-inspection" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test inspection -- --nocapture
