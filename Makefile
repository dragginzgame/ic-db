.DEFAULT_GOAL := help
export CARGO_BUILD_WARNINGS := deny
FORMAT_RUST_ARGS := --extra-workspace qualification
TOKO_TOOLING_RELEASE := 1
RELEASE_REMOTE := origin
RELEASE_BRANCH := main
RELEASE_DELIVERY_POLICY := scripts/release-delivery.txt
include vendor/toko-tooling/make/consumer.mk
include vendor/toko-tooling/make/cargo-dependencies.mk

.PHONY: help check test clippy doc docs-check ci db m0-setup m0-contracts m0-rollback m0-batch m0-reads m0-aggregation m0-maintenance m0-maintenance-artifacts
DB_INSTALL_PREFIX ?= $(HOME)/.local
help:
	@printf '%s\n' \
	  'make check / test / clippy / doc  Check the empty library scaffold' \
	  'make docs-check                  Check local documentation links' \
	  'make m0-setup                    Explicitly prepare M0 PocketIC server' \
	  'make m0-contracts                Check native numeric/schema/row/query contracts' \
	  'make m0-rollback                 Run only the Wasm write-rollback probe' \
	  'make m0-batch                    Run final-state batch constraint probe' \
	  'make m0-reads                    Run bounded ordering/page/DISTINCT probe' \
	  'make m0-aggregation              Run only the resumable grouped-sum probe' \
	  'make m0-maintenance              Run interrupted schema-maintenance probe' \
	  'make db                         Install db; run db to open the console' \
	  'make release-patch/minor/major   Qualify and deliver a private source release' \
	  'make fmt / fmt-check             Format / check Rust and manifests' \
	  'make deps-list / deps-update     Inspect / update Cargo dependencies' \
	  'make install-tools / tools-check Prepare / admit development tools' \
	  'make toko-tooling-check          Verify Cargo/snapshot agreement' \
	  'make install-hooks              Install the stock formatting hook' \
	  'toko-cloc                       Report source counts (PATH launcher)' \
	  'make ci                         Aggregate scaffold checks; no database qualification'

check:
	cargo check --locked -p ic-db

test:
	cargo test --locked -p ic-db

clippy:
	cargo clippy --locked -p ic-db --all-targets -- -D warnings

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --locked -p ic-db --no-deps

docs-check:
	perl vendor/toko-tooling/scripts/ci/check-documentation-links.pl --root . README.md AGENTS.md CHANGELOG.md docs/design/ic-db-system-design.md qualification/README.md

m0-setup:
	cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test rollback prepare_server -- --ignored --nocapture

m0-contracts:
	cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test numeric_contract --test schema_identity --test row_codec --test query_composition -- --nocapture

m0-rollback:
	@mkdir -p qualification/target/evidence/m0
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-canister --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m0/rollback-build.jsonl
	@IC_DB_M0_WASM="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m0_rollback_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/m0/rollback-build.jsonl)" && \
	  export IC_DB_M0_WASM && \
	  cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test rollback message_rollback -- --nocapture

m0-aggregation:
	@mkdir -p qualification/target/evidence/m0
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-aggregation-canister --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m0/aggregation-build.jsonl
	@IC_DB_M0_AGGREGATION_WASM="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m0_aggregation_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/m0/aggregation-build.jsonl)" && \
	  export IC_DB_M0_AGGREGATION_WASM && \
	  cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test aggregation revision_aggregation -- --nocapture

m0-batch:
	@mkdir -p qualification/target/evidence/m0
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-canister --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m0/batch-build.jsonl
	@IC_DB_M0_WASM="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m0_rollback_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/m0/batch-build.jsonl)" && \
	  export IC_DB_M0_WASM && \
	  cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test batch final_state_batches -- --nocapture

m0-maintenance-artifacts:
	@set -eu; out=qualification/target/evidence/m0; mkdir -p "$$out"; \
	  for variant in a b bprime bad; do \
	    case "$$variant" in a) features='' ;; b) features=target ;; bprime) features=changed-transform ;; bad) features=bad-upgrade ;; esac; \
	    cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-maintenance-canister --target wasm32-unknown-unknown --release --no-default-features --features "$$features" --message-format=json-render-diagnostics > "$$out/maintenance-$$variant.jsonl"; \
	    artifact="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m0_maintenance_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' "$$out/maintenance-$$variant.jsonl")"; \
	    cp "$$artifact" "$$out/maintenance-$$variant.wasm"; \
	  done

m0-reads:
	@mkdir -p qualification/target/evidence/m0
	cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-canister --target wasm32-unknown-unknown --release --message-format=json-render-diagnostics > qualification/target/evidence/m0/reads-build.jsonl
	@IC_DB_M0_WASM="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m0_rollback_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' qualification/target/evidence/m0/reads-build.jsonl)" && \
	  export IC_DB_M0_WASM && \
	  cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test reads bounded_reads -- --nocapture

m0-maintenance: m0-maintenance-artifacts
	IC_DB_M0_MAINTENANCE_DIR="$(CURDIR)/qualification/target/evidence/m0" cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test maintenance -- --nocapture

db:
	@set -eu; out=qualification/target/evidence/m0; mkdir -p "$$out"; \
	  cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-maintenance-canister --target wasm32-unknown-unknown --release --no-default-features --message-format=json-render-diagnostics > "$$out/db-fixture.jsonl"; \
	  fixture="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "ic_db_m0_maintenance_canister") | .filenames[] | select(endswith(".wasm"))] | if length == 1 then .[0] else error("expected one Wasm artifact") end' "$$out/db-fixture.jsonl")"; \
	  cargo build --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --bin db --release --message-format=json-render-diagnostics > "$$out/db-build.jsonl"; \
	  binary="$$(jq -ers '[.[] | select(.reason == "compiler-artifact" and .target.name == "db") | .executable | select(. != null)] | if length == 1 then .[0] else error("expected one db binary") end' "$$out/db-build.jsonl")"; \
	  mkdir -p "$(DB_INSTALL_PREFIX)/bin" "$(DB_INSTALL_PREFIX)/share/ic-db/m0"; \
	  install -m 644 "$$fixture" "$(DB_INSTALL_PREFIX)/share/ic-db/m0/maintenance-a.wasm"; \
	  install -m 755 "$$binary" "$(DB_INSTALL_PREFIX)/bin/db"; \
	  printf 'Installed %s/bin/db. Run db (ensure %s/bin is on PATH).\n' "$(DB_INSTALL_PREFIX)" "$(DB_INSTALL_PREFIX)"

ci: fmt-check clippy test doc docs-check

.PHONY: release-version release-preflight release-verify release-prepare-version release-prepared-check release-files release-commit-check release-committed-check release-tagged-check release-push-check release-lint release-tools
release-version release-preflight release-verify release-prepare-version release-prepared-check release-files release-commit-check release-committed-check release-tagged-check release-push-check:
	@bash scripts/release.sh "$@"

release-tools:
	bash vendor/toko-tooling/scripts/dev/install-rust-tools.sh --consumer "$(CURDIR)" --package ic-build --version 0.84.1 --bin ic-build-release-metadata --profile debug --features cli

release-lint:
	$(MAKE) clippy
	cargo clippy --manifest-path qualification/Cargo.toml --locked --workspace --all-targets -- -D warnings
