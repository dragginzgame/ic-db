# ic-db

A fresh design workspace for a smaller embedded Internet Computer database.
This repository explores replacing IcyDB's accumulated mechanisms with a system
built around broad bounded fluent queries and familiar attribute-based schemas.
SQL, joins and window functions are excluded. This is a multipurpose IC database;
application-defined schemas and broad capabilities drive its design. The private
Toko GitHub repository is a secondary compatibility target, not the architectural
guide. The obsolete, deleted local checkout is not a design input.
It does not replace IcyDB yet.
Ordinary Rust iteration handles result mapping; the Canic adapter owns product
endpoints. Broad schema types and constrained resumable aggregation are required.
Shared execution and measured monomorphization growth are explicit design concerns.

Start with the [proposed system design](docs/design/ic-db-system-design.md):
capability contracts, authoritative state, schema evolution,
query/write paths, experiments and conditions for proceeding.

IcyDB's local checkout and GitHub repository are frozen by maintainer decision
pending this experiment. Read-only research remains allowed; development and
delivery require an explicit decision to resume. Existing work and data are retained.

The first M1 product slice now lives in `crates/ic-db` (shared runtime) and
`crates/ic-db-macros` (attribute authoring and typed codecs). It creates and reopens
exactly admitted schemas, persists Nat64/Int64, Nat128/Int128, Bool and bounded Text with optional
values, maintains indexes and entity revisions, and applies bounded final-state
batches with required Nat64 references and indexed restricted deletion. This is
experimental: there is no production compatibility promise, measured improvement
or deployment. Rich types, broader references and the broad fluent builder remain required
implementation work; this increment does not complete M1. Primary and index keys
currently require non-optional Nat64 fields.
The private source repository is [dragginzgame/ic-db](https://github.com/dragginzgame/ic-db).
No public licence or crates.io publication has been selected; packages are unpublished.

M0 investigation has begun in the independent [qualification workspace](qualification/README.md).
Its synchronous write-rollback probe passes in PocketIC with published IC Storage.
This supports the no-WAL candidate; it does not qualify the complete product.
The resumable grouped-sum probe also passes: it completes on unchanged input and
explicitly invalidates under intervening writes. Completion under real traffic
is not guaranteed under the accepted initial contract.
The identified maintenance probe now covers interrupted copying, changed-code
refusal, cutover and cancellation. These remain small fixtures, not a product engine.
The final-state batch probe covers unique-value exchanges, reference existence
and indexed restricted deletion without a durable staging layer.
The bounded-read probe covers order admission, sparse pages, actual reply-byte
limits, ordered DISTINCT and the accepted live-pagination duplicate/omission behavior.
`make m0-contracts` checks candidate exact numeric keys, schema identity sensitivity,
SHA-256 fingerprints, bounded recursive row decoding and projection/DISTINCT/grouping
composition natively. It does not qualify product codecs or measure Wasm cost.

The [M0 readiness review](docs/design/ic-db-system-design.md#next-delivery-close-m0-then-typed-schemas-in-m1)
separates accepted entry contracts from M1–M3 implementation checks. The M0 entry
review permits this first typed-schema increment. `make m0-icydb-baseline` provides the first
SQL-disabled comparison workload: persistent writes, indexed reads and unchanged
upgrade/reopen. It measures the frozen engine only; matched IC DB results and
broader workload budgets remain outstanding.

The authoring comparison is complete. M1 keeps the familiar attribute-owned
entity/record declarations with empty struct bodies; alternatives may be explored
later. For a real executable declaration and write/reopen journey, see
[the record tests](crates/ic-db/tests/records.rs) and
[the CLI fixture](qualification/records-canister/src/lib.rs).
Run `make m1-records` for their focused native and Wasm checks.

## Database console

```sh
make m0-setup   # once: explicitly prepare the local PocketIC server
make db        # install/update the binary; does not start the console
db             # open the console
```

At the `db>` prompt, use `schema`, `status`, `query --limit 3`,
`query --after 3 --limit 2 --min 40`, `help` or `quit`.
For one command: `db schema` or `db query --limit 2`. Use `db --help` for help.
Installation defaults to `~/.local/bin/db`; keep `~/.local/bin` on `PATH`.
Override the prefix with `make db DB_INSTALL_PREFIX=/your/prefix`.

The current console reads an explicitly labelled, temporary M1 database with six
`Entry` rows, one mixed-type `Document` and one `WideNumbers` record
running in local PocketIC. Each session starts fresh and is discarded on exit.
`db schema` presents generated fields and indexes in bordered tables, with the
primary key first and separate type, optionality and byte-limit columns. The CLI
uses `ic-host-text`, as IcyDB's describe renderer did. `db status` contains the
full manifest fingerprint, incarnation, Entry revision and physical pages.
`query` currently reads Entry and shows `id` and `amount`; its `--min` filter runs over a
bounded primary-key page, so an empty page can still have a continuation.
The output supplies a next-page command, retaining the limit and filter, or an
explicit completion message. Terminal layout belongs to the CLI, not the runtime.
It has no SQL, remote-canister connection or production credentials. Broader
query support remains required in the eventual product CLI, with explicit entity
selection once more than Entry can be queried;
Canic will own its endpoint integration. See [probe details](qualification/README.md).
The installed fixture lives under the prefix's `share/ic-db/m1`; PocketIC still
uses this checkout's explicitly prepared `.tools/m0-pocketic` server.

## Development

Rust 1.100.0 and published Toko Tooling 0.43.0 are pinned. Read [AGENTS.md](AGENTS.md)
and [TOKO-TOOLING.md](TOKO-TOOLING.md). Common scripts live in `vendor/toko-tooling/`;
upstream reference documentation stays upstream.

```sh
make install-tools
make tools-check
make toko-tooling-check
make clippy
make docs-check
```

`make help` lists the focused checks. `make ci` is the aggregate native library gate;
passing it does not qualify a database. The configured Linux and macOS CI jobs
run it on GitHub. These checks do not certify a production database.

For tooling updates, change the exact Cargo pin, run
`make deps-update DEPENDENCY_PACKAGE=toko-tooling`, then
`make toko-tooling-preview` and `make toko-tooling-update`. Check the resulting
lock and snapshot together. `make install-hooks` enables the stock format hook.

Use `toko-cloc` for source counts, or `toko-cloc --manifest qualification/Cargo.toml`
for the experiments alone. Toko Tooling 0.43 replaces `make cloc`. Install its
checkout-selecting launcher once on your user PATH:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 vendor/toko-tooling/scripts/dev/toko-cloc "$HOME/.local/bin/toko-cloc"
```

Keep `~/.local/bin` on `PATH`; the launcher uses the current checkout's installed reporter.

IcyDB and Canic remain separate; there is no path dependency on either repository.

## Private source releases

`make release-patch`, `make release-minor` and `make release-major` use the same
shared runner and local adapter. They require clean committed source on `main`,
prepare the locked dependencies/tools and PocketIC server, run the scaffold and
all M0 gates, finalize version/changelog metadata, then atomically push `main`
and the annotated version tag to the private repository. `make release-resume
VERSION=X.Y.Z` recovers a specific interrupted attempt. There is no crates.io
upload or live-canister deployment. Creating the GitHub prerelease is a separate
delivery step after tag verification.

The release metadata editor is the explicitly prepared IC Build 0.84.1 tool.
Validation logs and the shared runner's recovery state remain local; generated
Wasm/server assets are not shipped as a portable production binary. Build the
fixture CLI from the source checkout with `make m0-setup` and `make db`.
