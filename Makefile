.DEFAULT_GOAL := help
export CARGO_BUILD_WARNINGS := deny
FORMAT_RUST_ARGS := --extra-workspace qualification
TOKO_TOOLING_RELEASE := 1
RELEASE_REMOTE := origin
RELEASE_BRANCH := main
RELEASE_DELIVERY_POLICY := scripts/release-delivery.txt
CARGO_PUBLISH_PACKAGES := ic-db-macros ic-db ic-db-cli
CARGO_PUBLISH_REGISTRY := crates-io
CARGO_PUBLISH_ALL_FEATURES := 1
override CARGO_PUBLISH_VERIFY_TARGETS := publication-tag
publish-check: override CARGO_PUBLISH_VERIFY_TARGETS := check
include vendor/toko-tooling/make/consumer.mk
include vendor/toko-tooling/make/cargo-dependencies.mk

.PHONY: help check test clippy doc docs-check ci db m0-setup m0-contracts m0-rollback m0-batch m0-reads m0-aggregation m0-maintenance m0-maintenance-artifacts
DB_INSTALL_PREFIX ?= $(HOME)/.local
help:
	@printf '%s\n' \
	  'make check / test / clippy / doc  Check the runtime and schema macro' \
	  'make schema-build-check        Check schema producer and consumer ownership' \
	  'make docs-check                  Check local documentation links' \
	  'make m0-setup                    Explicitly prepare M0 PocketIC server' \
	  'make m0-contracts                Check native numeric, schema and Wire contracts' \
	  'make m0-rollback                 Run only the Wasm write-rollback probe' \
	  'make m0-endpoints                Check standalone hooks and reply rollback' \
	  'make m0-batch                    Run final-state batch constraint probe' \
	  'make m0-reads                    Run bounded ordering/page/DISTINCT probe' \
	  'make m0-aggregation              Run only the resumable grouped-sum probe' \
	  'make m2-float64-sum              Verify exact Float64 queries and retained jobs' \
	  'make m0-maintenance              Run interrupted schema-maintenance probe' \
	  'make m1-records                 Check generated records natively and in Wasm' \
	  'make m2-queries                 Check bounded fluent query pages' \
	  'make m2-primary-pages           Measure primary pages and verify continuations' \
	  'make m2-bulk-selection          Compare projected bulk primary-key reads' \
	  'make m2-sqlite-reference        Check native fluent results against SQLite' \
	  'make m2-jobs                    Check retained aggregates and rollback' \
	  'make m2-contention              Check retained aggregates and shared job capacity' \
	  'make m2-cli                     Check the fluent CLI and migrated query caller' \
	  'make m3-maintenance             Check product migration, cutover and rollback' \
	  'make m3-references              Check cross-chunk migration references' \
	  'make m3-jobs                    Check aggregate custody during maintenance' \
	  'make m1-commit-failures         Check product rollback inside supplier writes' \
	  'make m4-lifecycle               Check mixed writes, queries, jobs and migration' \
	  'make m4-matched-growth          Compare installed IC DB and frozen IcyDB entity growth' \
	  'make m1-write-envelope         Measure maximal rows, batches and index fan-out' \
	  'make m1-store-width            Exercise one store (STORE_WIDTH=32 by default)' \
	  'make m1-entity-growth           Measure 1/8/32-entity Wasm and execution growth' \
	  'make m2-query-growth            Measure fluent shapes, maps and entity growth' \
	  'make m2-count-paths            Compare validated, index-entry and maintained prefix counts' \
	  'make m2-count-profile           Separate count opening, scan and row-fetch costs' \
	  'make m2-count-comparison        Compare entity and indexed-prefix count costs' \
	  'make m2-aggregate-comparison    Compare grouped IC DB/IcyDB aggregate costs' \
	  'make m2-fluent-comparison       Compare fluent IC DB/IcyDB listings and sorting' \
	  'make m2-keyset-comparison       Compare complete two-row listings with frozen IcyDB' \
	  'make m2-ranked-listing          Check ordered project listings against SQLite' \
	  'make m2-query-matrix            Compare broad query/selectivity/population matrix' \
	  'make m2-aggregate-paths         Compare primary and explicit-index aggregates' \
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
	cargo test --locked -p ic-db -p ic-db-macros --features ic-db/build,ic-db/test-support
	cargo test --locked -p ic-db --features metrics --test queries --test jobs metrics_
	cargo test --locked -p ic-db --features logging --test queries logging_

clippy:
	cargo clippy --locked -p ic-db -p ic-db-macros --features ic-db/build,ic-db/test-support --all-targets -- -D warnings

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --locked -p ic-db -p ic-db-macros --features ic-db/build,ic-db/test-support --no-deps

docs-check:
	perl vendor/toko-tooling/scripts/ci/check-documentation-links.pl --root . README.md AGENTS.md CHANGELOG.md docs/design/ic-db-system-design.md qualification/README.md

.PHONY: schema-build-check
schema-build-check:
	perl scripts/check-schema-builds.pl
	python3 qualification/experiments/schema-owner/probe.py --admission

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
	  if test -e "$(CURDIR)/.tools/testkit-server/pocket-ic"; then \
	    "$$launcher" check "$(CURDIR)/.tools/testkit-server" > "$$attempt/check.log" 2>&1 || { cat "$$attempt/check.log" >&2; exit 1; }; \
	    cat "$$attempt/check.log"; \
	  else \
	    "$$launcher" setup "$(CURDIR)/.tools/testkit-server" "$$attempt/evidence" "$$(command -v curl)" "$$(command -v gzip)"; \
	  fi

.PHONY: m2-sqlite-reference
m2-sqlite-reference:
	cargo test --locked --manifest-path qualification/Cargo.toml -p ic-db-sqlite-reference

m0-contracts:
	cargo run --manifest-path qualification/Cargo.toml --locked -p ic-db-wire-schema-probe
	cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-wire-schema-probe --features inspection
	cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-qualification-transport --test schema_serde
	cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test numeric_contract --test schema_identity -- --nocapture

.PHONY: m2-fluent-comparison m2-aggregate-comparison m2-count-comparison m2-count-profile
m2-fluent-comparison:
	bash scripts/measure-records.sh fluent

.PHONY: m2-keyset-comparison
m2-keyset-comparison:
	bash scripts/measure-records.sh keyset

.PHONY: m2-ranked-listing
m2-ranked-listing:
	bash scripts/measure-records.sh ranked-listing

.PHONY: m2-query-matrix
m2-query-matrix:
	bash scripts/measure-records.sh query-matrix

.PHONY: m2-count-paths
m2-count-paths:
	bash scripts/measure-records.sh count-paths

.PHONY: m2-aggregate-paths
m2-aggregate-paths:
	bash scripts/measure-records.sh aggregate-paths

m2-aggregate-comparison:
	bash scripts/measure-records.sh aggregate

m2-count-comparison:
	bash scripts/measure-records.sh count

m2-count-profile:
	bash scripts/measure-records.sh count-profile

.PHONY: m4-matched-growth
m4-matched-growth:
	python3 qualification/experiments/matched-growth/measure.py qualification/target/evidence/matched-growth
	IC_DB_MATCHED_GROWTH_DIR="$(CURDIR)/qualification/target/evidence/matched-growth" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test query_matrix paired_entity_growth -- --ignored --nocapture

.PHONY: m1-entity-growth
m1-entity-growth:
	bash scripts/measure-records.sh growth

STORE_WIDTH ?= 32
.PHONY: m1-store-width
m1-store-width:
	python3 qualification/experiments/schema-owner/probe.py --store-width "$(STORE_WIDTH)"
	IC_DB_STORE_WIDTH="$(STORE_WIDTH)" IC_DB_SCHEMA_OWNER_ARTIFACTS="$(CURDIR)/qualification/target/schema-owner" \
	  bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml \
	    -p ic-db-m0-rollback-driver --test schema_owner wide_store_on_replica -- --ignored --exact --nocapture

NAMED_GROWTH_DIR ?= $(CURDIR)/qualification/target/evidence/named-growth
.PHONY: m2-named-growth
m2-named-growth:
	python3 qualification/experiments/named-growth/measure.py "$(NAMED_GROWTH_DIR)"
	IC_DB_QUERY_GROWTH_DIR="$(NAMED_GROWTH_DIR)" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test query_growth named_growth -- --ignored --exact --nocapture

.PHONY: m2-query-growth
m2-query-growth:
	bash scripts/measure-records.sh queries

.PHONY: m0-core-artifact m0-core
m0-core-artifact:
	@mkdir -p qualification/target/evidence/m0
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-canister --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m0/core-build.jsonl
	@artifact="$$(jq -ers --arg target ic_db_m0_rollback_canister -f scripts/wasm-artifact.jq qualification/target/evidence/m0/core-build.jsonl)" && \
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
	@IC_DB_M0_AGGREGATION_WASM="$$(jq -ers --arg target ic_db_m0_aggregation_canister -f scripts/wasm-artifact.jq qualification/target/evidence/m0/aggregation-build.jsonl)" && \
	  export IC_DB_M0_AGGREGATION_WASM && \
	  bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test aggregation revision_aggregation -- --nocapture

.PHONY: m2-float64-sum
m2-float64-sum:
	cargo test --locked -p ic-db --lib float_sum
	@mkdir -p qualification/target/evidence/m2
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-aggregation-canister --example float64_sum --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m2/float64-build.jsonl
	@artifact="$$(jq -ers --arg target float64_sum -f scripts/wasm-artifact.jq qualification/target/evidence/m2/float64-build.jsonl)" && \
	  IC_DB_FLOAT64_SUM_WASM="$$artifact" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test float64_sum -- --nocapture

m0-batch: m0-core-artifact
	IC_DB_M0_WASM="$(CURDIR)/qualification/target/evidence/m0/core.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test batch final_state_batches -- --nocapture

m0-maintenance-artifacts:
	@set -eu; out=qualification/target/evidence/m0; mkdir -p "$$out"; \
	  for variant in a b bprime bad; do \
	    case "$$variant" in a) features='' ;; b) features=target ;; bprime) features=changed-transform ;; bad) features=bad-upgrade ;; esac; \
	    cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-maintenance-canister --target wasm32-unknown-unknown --release --no-default-features --features "$$features" --message-format=json-render-diagnostics > "$$out/maintenance-$$variant.jsonl"; \
	    artifact="$$(jq -ers --arg target ic_db_m0_maintenance_canister -f scripts/wasm-artifact.jq "$$out/maintenance-$$variant.jsonl")"; \
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
	@artifact="$$(jq -ers --arg target ic_db_m1_records_canister -f scripts/wasm-artifact.jq qualification/target/evidence/m1/records-build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/m1/records.wasm

m1-records: schema-build-check
	cargo test --locked --manifest-path qualification/Cargo.toml -p ic-db-m1-records-canister --lib
	cargo test --locked -p ic-db --test enum_collections
	cargo test --locked -p ic-db -p ic-db-macros --features ic-db/build --test records --test write_contracts --test cardinality --test enum_indexes --test schema_reuse --test key_bounds --test defaults --test managed --test nested --test lists --test sets --test maps --test u256 --test floats --test accounts --test ulids --test ulid_generation --test dates --test times --test time_values --test decimals --test primary_keys --test scalar_keys --lib
	$(MAKE) m1-records-wasm

m1-records-wasm: m1-records-artifact
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m1/records.wasm" bash scripts/with-pocketic.sh cargo test --locked -p ic-db-cli --test console -- --nocapture
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m1/records.wasm" bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml -p ic-db-m0-rollback-driver --test records --test enum_collections --test defaults --test managed --test generation --test principal_keys --test account_keys --test sorting -- --nocapture

.PHONY: m1-commit-failures
.PHONY: m2-queries m2-sort-costs m2-sort-costs-artifact
m2-queries: m1-records-artifact m2-sqlite-reference
	cargo test --locked -p ic-db --test queries
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m1/records.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test queries --test sorting --test live_pages -- --nocapture

m2-sort-costs-artifact:
	@mkdir -p qualification/target/evidence/m2
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features query-measurements --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m2/sort-costs-build.jsonl
	@artifact="$$(jq -ers --arg target ic_db_m1_records_canister -f scripts/wasm-artifact.jq qualification/target/evidence/m2/sort-costs-build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/m2/sort-costs.wasm

m2-sort-costs: m2-sort-costs-artifact
	IC_DB_QUERY_COSTS=1 IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/sort-costs.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test sorting sorting_reads_complete_input -- --nocapture
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/sort-costs.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test sorting unique_decimal_order_matches_sqlite -- --ignored --nocapture

.PHONY: m2-primary-pages
m2-primary-pages: m2-sort-costs-artifact
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/sort-costs.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test listing_costs primary_page_lookahead -- --exact --ignored --nocapture

.PHONY: m2-bulk-selection
m2-bulk-selection: m2-sort-costs-artifact
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/sort-costs.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test listing_costs projected_bulk_lookup_matches_full_and_fluent_reads -- --ignored --nocapture

.PHONY: m2-listing-costs
m2-listing-costs: m2-sort-costs-artifact m2-sqlite-reference
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/sort-costs.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test listing_costs ordered_listing_paths_match_sqlite_page_by_page -- --ignored --nocapture
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/sort-costs.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test listing_costs ulid_time_listing_resumes_inside_a_millisecond -- --ignored --nocapture

.PHONY: m2-duration-costs
m2-duration-costs: m2-sort-costs-artifact
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/sort-costs.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test duration_costs -- --ignored --nocapture

.PHONY: m2-filter-costs
m2-filter-costs: m2-sort-costs-artifact
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/sort-costs.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test listing_costs measured_membership_and_empty_text_return_expected_counts -- --ignored --nocapture

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
	@artifact="$$(jq -ers --arg target ic_db_m1_records_canister -f scripts/wasm-artifact.jq qualification/target/evidence/m2/jobs-build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/m2/jobs.wasm

.PHONY: m3-maintenance m3-maintenance-wasm
m3-maintenance:
	cargo test --locked -p ic-db --test maintenance --test schema_copy
	$(MAKE) m3-maintenance-wasm

M3_ARTIFACTS := m3-artifact-maintenance m3-artifact-maintenance-v2 m3-artifact-maintenance-faults
.PHONY: $(M3_ARTIFACTS)
$(M3_ARTIFACTS): m3-artifact-%:
	@mkdir -p qualification/target/evidence/m3
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features $* --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m3/$*-build.jsonl
	@artifact="$$(jq -ers --arg target ic_db_m1_records_canister -f scripts/wasm-artifact.jq qualification/target/evidence/m3/$*-build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/m3/$*.wasm

m3-maintenance-wasm: $(M3_ARTIFACTS)
	IC_DB_M3_DIR="$(CURDIR)/qualification/target/evidence/m3" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test product_maintenance --test product_copy --test product_capacity --test product_references -- --nocapture
	$(MAKE) m3-jobs

.PHONY: m3-references
m3-references: m3-artifact-maintenance-faults
	IC_DB_M3_DIR="$(CURDIR)/qualification/target/evidence/m3" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test product_references -- --nocapture

.PHONY: m3-jobs
m3-jobs:
	@mkdir -p qualification/target/evidence/m3
	@set -eu; for feature in maintenance maintenance-v2; do \
	  cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features "aggregate-jobs,$$feature" --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > "qualification/target/evidence/m3/jobs-$$feature-build.jsonl"; \
	  artifact="$$(jq -ers --arg target ic_db_m1_records_canister -f scripts/wasm-artifact.jq "qualification/target/evidence/m3/jobs-$$feature-build.jsonl")"; \
	  cp "$$artifact" "qualification/target/evidence/m3/jobs-$$feature.wasm"; \
	done
	IC_DB_M3_DIR="$(CURDIR)/qualification/target/evidence/m3" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test jobs_maintenance -- --nocapture

.PHONY: m2-cli m2-cli-artifact
m2-cli-artifact:
	@mkdir -p qualification/target/evidence/m2
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features retained-console --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m2/console-build.jsonl
	@artifact="$$(jq -ers --arg target ic_db_m1_records_canister -f scripts/wasm-artifact.jq qualification/target/evidence/m2/console-build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/m2/console.wasm

m2-cli: m2-cli-artifact
	cargo test --locked -p ic-db-cli --bin db
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/console.wasm" bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml -p ic-db-m0-rollback-driver --test console_text --test console_aggregates -- --nocapture
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/console.wasm" IC_DB_CLI_RETAINED=1 bash scripts/with-pocketic.sh cargo test --locked -p ic-db-cli --test console console_inspects_and_queries -- --nocapture
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/console.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test records product_records_and_message_boundary -- --nocapture

.PHONY: m3-cli
m3-cli: m2-cli-artifact
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/console.wasm" bash scripts/with-pocketic.sh cargo test --locked -p ic-db-cli --bin db --test migration -- --nocapture
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/console.wasm" bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml -p ic-db-m0-rollback-driver --test console_migration --test console_text --test console_aggregates -- --nocapture
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m2/console.wasm" cargo test --locked -p ic-db-cli --test console_terminal -- --nocapture

.PHONY: m1-write-envelope
m1-write-envelope: m1-records-artifact
	IC_DB_M1_RECORDS_WASM="$(CURDIR)/qualification/target/evidence/m1/records.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test write_envelope -- --nocapture

m1-commit-failures:
	@mkdir -p qualification/target/evidence/m1
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features fault-injection --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m1/fault-build.jsonl
	@artifact="$$(jq -ers --arg target ic_db_m1_records_canister -f scripts/wasm-artifact.jq qualification/target/evidence/m1/fault-build.jsonl)" && \
	  IC_DB_M1_FAULT_WASM="$$artifact" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test commit_failures -- --nocapture

.PHONY: m4-lifecycle
m4-lifecycle:
	@mkdir -p qualification/target/evidence/mixed-lifecycle
	@git rev-parse HEAD > qualification/target/evidence/mixed-lifecycle/source-head.txt
	@git diff --binary > qualification/target/evidence/mixed-lifecycle/working.patch
	@rustc -Vv > qualification/target/evidence/mixed-lifecycle/compiler.txt
	@cp qualification/Cargo.lock qualification/target/evidence/mixed-lifecycle/Cargo.lock
	@cp qualification/ic-db-qualification-transport/src/mixed.rs qualification/target/evidence/mixed-lifecycle/protocol.rs
	@cp qualification/ic-db-m1-records-canister/src/mixed_lifecycle.rs qualification/target/evidence/mixed-lifecycle/host.rs
	@cp qualification/ic-db-m0-rollback-driver/tests/mixed_lifecycle.rs qualification/target/evidence/mixed-lifecycle/driver.rs
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features mixed-lifecycle --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/mixed-lifecycle/build.jsonl
	@artifact="$$(jq -ers --arg target ic_db_m1_records_canister -f scripts/wasm-artifact.jq qualification/target/evidence/mixed-lifecycle/build.jsonl)" && \
	  cp "$$artifact" qualification/target/evidence/mixed-lifecycle/host.wasm
	@if IC_DB_M4_WASM="$$PWD/qualification/target/evidence/mixed-lifecycle/host.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test mixed_lifecycle -- --nocapture > qualification/target/evidence/mixed-lifecycle/run.log 2>&1; then \
	  cat qualification/target/evidence/mixed-lifecycle/run.log; \
	else cat qualification/target/evidence/mixed-lifecycle/run.log; exit 1; fi

db: m2-cli-artifact
	@set -eu; out=qualification/target/evidence/m1; \
	  mkdir -p "$$out"; \
	  fixture=qualification/target/evidence/m2/console.wasm; \
	  cargo build --locked -p ic-db-cli --bin db --release --message-format=json-render-diagnostics > "$$out/db-build.jsonl"; \
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

ci: fmt-check clippy test doc docs-check schema-build-check release-adapter-test

.PHONY: release-version release-preflight release-verify release-prepare-version release-prepared-check release-files release-commit-check release-committed-check release-tagged-check release-push-check release-lint release-tools
release-version release-preflight release-verify release-prepare-version release-prepared-check release-files release-commit-check release-committed-check release-tagged-check release-push-check:
	@bash scripts/release.sh "$@"

release-tools:
	bash vendor/toko-tooling/scripts/dev/install-rust-tools.sh --consumer "$(CURDIR)" --package ic-build --version 0.84.1 --bin ic-build-release-metadata --profile debug --features cli

release-lint:
	cargo clippy --locked -p ic-db-cli --all-targets -- -D warnings
	$(MAKE) clippy
	# Native callers/schema builds and IC-only fixtures have different target APIs.
	cargo clippy --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver -p ic-db-m1-records-schema -p ic-db-sqlite-reference --all-targets -- -D warnings
	cargo clippy --manifest-path qualification/Cargo.toml --locked --workspace --exclude ic-db-m0-rollback-driver --exclude ic-db-icydb-baseline-schema --exclude ic-db-icydb-baseline-canister --lib --target wasm32-unknown-unknown -- -D warnings
	cargo clippy --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features fault-injection --target wasm32-unknown-unknown -- -D warnings
	cargo clippy --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features maintenance-v2 --target wasm32-unknown-unknown -- -D warnings
	cargo clippy --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features maintenance-faults --target wasm32-unknown-unknown -- -D warnings
	cargo clippy --manifest-path qualification/Cargo.toml --locked -p ic-db-m1-records-canister --features retained-console --target wasm32-unknown-unknown -- -D warnings

.PHONY: m2-nested-path-costs
m2-nested-path-costs:
	@set -eu; out="$(CURDIR)/qualification/target/evidence/m2"; mkdir -p "$$out"; \
	for tier in 1 8 32 indexed member optional; do \
	  count=$$tier; features=""; case "$$tier" in indexed) count=32; features=flat-index-control;; member) count=32; features=member-index-control;; optional) count=32; features=optional-member-index-control;; esac; \
	  IC_DB_PATH_COUNT=$$count cargo build --features "$$features" --locked --manifest-path qualification/Cargo.toml -p ic-db-m0-aggregation-canister --example nested_paths --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > "$$out/nested-paths-build.jsonl"; \
	  artifact=$$(jq -ers --arg target nested_paths -f scripts/wasm-artifact.jq "$$out/nested-paths-build.jsonl"); \
	  cp "$$artifact" "$$out/nested-paths-$$tier.wasm"; \
	done; \
	IC_DB_NESTED_PATH_DIR="$$out" bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml -p ic-db-m0-rollback-driver --test nested_paths -- --ignored --nocapture

.PHONY: m2-column-listing
m2-column-listing:
	@set -eu; out="$(CURDIR)/qualification/target/evidence/m2"; mkdir -p "$$out"; \
	for shape in entity named scalar tuple records with-scalar all; do \
	  IC_DB_LISTING_SHAPE=$$shape cargo build --locked --manifest-path qualification/Cargo.toml -p ic-db-m0-aggregation-canister --example column_listing --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > "$$out/column-listing-build.jsonl"; \
	  artifact=$$(jq -ers --arg target column_listing -f scripts/wasm-artifact.jq "$$out/column-listing-build.jsonl"); \
	  cp "$$artifact" "$$out/column-listing-$$shape.wasm"; \
	done; \
	IC_DB_LISTING_DIR="$$out" bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml -p ic-db-m0-rollback-driver --test column_listing -- --ignored --nocapture
