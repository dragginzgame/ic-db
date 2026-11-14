# Changelog

## [0.1.2]

### ▶️ Try it

Install the console using the real first M1 record library, then inspect its
generated schema and two bounded pages. Each session has six Entry rows and one
mixed-type Document; this does not connect to a deployed database. Restart an existing console.

```sh
make m0-setup
make db
db schema
db status
db query --limit 3 --min 40
db query --after 3 --limit 3 --min 40
```

The first filtered page is empty but prints a next-page command with `--after 3`. The next returns
IDs 4–6. A full page can require one final empty request to establish completion.
The displayed value column is now named `amount`, matching the authored schema.
`db schema` also shows Document's bounded Text, Bool and optional fields. The
current `query` command still reads Entry; Document writes/reopen are covered by
`make m1-records` through private qualification controls.

### 🛠️ Product slice

- Restore IcyDB-style bordered schema/query tables using `ic-host-text` 0.75.0
  in the native CLI. Show primary keys first and separate field type, optionality
  and byte limits; keep full storage identities in `db status`. Print usable
  continuation commands instead of raw cursor/debug fields. The fixture returns
  structured schema metadata; remove its terminal layout and the obsolete runtime
  field formatter. No SQL frontend, stored-format or query-semantics change.
- Extend stored fields to Bool, Text with an explicit `max_bytes`, and optional
  Nat64/Bool/Text. Preserve absence versus empty/zero/false; enforce UTF-8 byte
  limits before writes and on decode. Types, bounds and optionality participate
  in schema identity; existing Nat64 bytes/fingerprints remain unchanged. Schema
  inspection reads typed metadata instead of hardcoding Nat64. Primary/index keys
  remain non-optional Nat64 for now; other key types are not retired. Generated
  codecs and validation change, while the shared commit/traversal engine does not.
- Add the first `#[entity]` macro and shared record runtime: externally keyed
  Nat64 fields, compiled schema fingerprints, exact manifest admission,
  create/reopen, primary-key pages, bounded index-prefix reads and cross-entity
  batches with final-state uniqueness and atomic entity revisions. Field names
  are identities; numeric field IDs are not introduced. No IcyDB executor or
  journal code is copied. Richer types, references and fluent queries remain
  required follow-on implementation; this does not complete M1.
- `db schema` now inspects generated metadata from this library, rather than
  the M0 maintenance fixture. Clap owns console/argv command validation and help;
  blank lines remain silent. `make db` still installs without launching.
- Add `make m1-records` for native codec/identity/conflict/reopen checks and the
  product's Wasm message-boundary witness. Full backing comparisons distinguish
  late application traps from normal errors; unchanged executable upgrades
  preserve rows, indexes and revisions. Instruction exhaustion and matched
  performance remain unqualified.

### 🧪 Qualification

- Add `make m1-commit-failures`: trap after every observed backing write in a
  real two-row replacement and refuse a physical growth request during a 32-row
  insert. Full stable-memory rollback, indexes, revisions, successful retry and
  unchanged-upgrade reopen pass. Fault injection is restricted to a dedicated
  qualification feature; the runtime, stored format and installed CLI commands
  remain unchanged. Review each patch for drift and architectural sediment under
  the new `AGENTS.md` instruction, fixing introduced duplication and ownership drift
  within the same change.

```sh
make m1-commit-failures
```

- Specify a finite compiled type graph with bounded recursive values. Extend
  name-based identity to nested types and enum variants; compact enum tags derive
  from canonical name order, without author-supplied field/type/variant numbers.
  Replace the probe-only numeric enum identity candidate and update its independent
  SHA-256 vector. Entity/index IDs remain unchanged; no product attributes or data
  are removed.
- Add native recursive-row codec checks for canonical field order, enum payloads,
  map/set equality, list ordering, absent versus empty values, and separate
  database/Wire bounds. Malformed map/set entries refuse before Rust collection
  construction can discard duplicates. Serde 1.0.229 is an explicit test-only
  dependency, already present transitively and registry-verified. Generated
  rich-type product codecs and Wasm cost remain unqualified.

```sh
make m0-contracts
```

- Add `make m0-icydb-baseline`: build the frozen IcyDB revision without SQL and
  measure a synthetic 32-row persistent workload in local PocketIC. Check typed
  inserts, duplicate refusal, primary-key reads, ordered category lookup,
  read/modify/replace with index maintenance and unchanged-upgrade persistence.
  Report uncompressed Wasm/code-section bytes, instructions, complete update
  cycle deltas and separate post-write settling work. This establishes an IcyDB
  reference, not IC DB parity or a measured improvement.

```sh
cargo fetch --manifest-path qualification/Cargo.toml --locked
make m0-setup
make m0-icydb-baseline
```

The fetch needs read access to the private IcyDB repository. The benchmark uses
an immutable Git dependency and this qualification lockfile, leaving local IcyDB
files untouched. Recorded baseline numbers used Storage 0.74.0/Runtime 0.22.2/
Types 0.20.2; the M1 lockfile selects 0.74.1/0.22.3/0.20.3 respectively. Refresh
the baseline before claiming a matched product comparison.

### 📚 Documentation

- Narrow the IcyDB baseline to its resolving SQL-disabled product graph. Record
  the executor conflict in the separately deferred historical Canic audit
  workspace; keep the frozen source unchanged and avoid treating that workspace
  as a valid benchmark or a product-engine failure.

## [0.1.1] - 2026-11-14

### ▶️ Try it

From this checkout, prepare the local fixture server once and install the CLI:

```sh
make m0-setup
make db
```

With `~/.local/bin` on `PATH`, inspect the fixture and read its first page:

```sh
db schema
db status
db query --limit 3
```

Continue after row 3, or filter by the fixture's value field:

```sh
db query --after 3 --limit 3
db query --after 3 --min 40 --limit 3
```

Open the console, press Return to get another quiet prompt, then use `help` or `quit`:

```sh
db
```

Each invocation starts a fresh, temporary six-row M0 fixture. These commands do
not connect to a deployed database; `schema` still describes the fixture.

### 🐛 Fixes

- Ignore blank and whitespace-only console input instead of displaying help.

### 🔧 Tooling

- Adopt IC Testkit 0.104.0 and its required native host helpers 0.77.2 in the
  qualification workspace. Retain PocketIC 16.1.0 and caller-owned Wire encoding;
  the extracted typed Wire adapter is not needed by these raw-call probes.
  Requalify explicit server setup, rollback, batches, reads, aggregation,
  maintenance/reuse and console behavior; refresh the installed local `db` demo.
- Adopt published Toko Tooling 0.43.0 in both workspaces and the authenticated
  installed snapshot. Use the `toko-cloc` PATH launcher for source reporting;
  include the hook path-resolution fixes. Runtime supplier selections are unchanged.
- Release review (2026-11-14): defer optional Storage 0.74.1, Platform 0.32.3,
  Types 0.20.3 and Runtime/macros/guard 0.22.3 refreshes to the next dependency
  maintenance pass after 0.1.1. Keep this release's tested locked graph. Tooling,
  Wire/Endpoint, Testkit, Host helpers and the release editor match the reviewed
  current stable selections.

### 📝 Design

- Select a domain-separated SHA-256 schema fingerprint and qualify bounded Wire
  payloads as the initial row-codec candidate. Keep named-field identities in the
  contract and positional row payloads under explicit admission; complete product
  grammar, generated traversal and Wasm budgets remain unqualified.
- Record accepted aggregate invalidation, job expiry/upgrade/capacity policies,
  decimal-average rounding direction, explicit IDs and performance tradeoff gate.
  Complete the pre-M1 authoring comparison and retain attribute-owned declarations
  with empty struct bodies by maintainer preference. Alternative forms are future
  optional work; blanket source compatibility is not required.
- Remove numeric field IDs from the proposed authoring contract. Field names
  identify fields; renames require explicit schema transitions. Update the native
  identity witness and retire the disposable authoring comparison and command.
  Attribute removals must be explained with their behavior consequences.
- Consolidate M0 readiness, remaining contract recommendations and M1–M4 delivery
  gates. Define the first connected typed-schema/inspection/persistence increment;
  pin the frozen IcyDB comparison candidate and explain why historical measurements
  do not establish a matched SQL-disabled baseline. M0 remains open.
- Specify the proposed M1 entity authoring and schema-inspection contract:
  explicit stable identities, named typed fields, bounds with units and shared
  compiled descriptors. Record IcyDB ergonomics retained, generated machinery
  not automatically retained, and macro/runtime acceptance cases. Product schema
  macros remain unimplemented.
- Define candidate omission/default, generated-key and managed-timestamp policies;
  retain explicit present-only optional uniqueness without SQL predicate parsing.
- Record the planned IC Storage overhaul when Rust OP is available. Defer
  supplier-specific architectural optimization; preserve current probe evidence
  and requalify correctness and capacity against the revised supplier.

### 🧪 Qualification

- Include primary-key membership/order in schema identity, with duplicate/dangling
  refusal and an independent SHA-256 golden vector. Probe compact row framing,
  optional/empty values, integer extremes, malformed/trailing data, schema-first
  admission, Wire work limits and database field byte bounds in the existing
  native driver; add no runtime package or durable state.
- Extend optional aggregate contracts to absent grouping keys, all-absent groups
  and present-zero groups through completed-output pagination and group filtering.
- Add supplier-independent optional aggregate witnesses: row/field/distinct
  counts, empty versus absent versus zero results, exact totals and weighted
  average continuation. Retain IcyDB's useful absence semantics without copying
  its reducer or selecting production numeric result types.
- Extend the existing Wasm batch probe with absent unique values, membership
  changes, same-batch ownership transfer and rollback. Verify complete row/index
  contents, unchanged backing on refusal/trap, independent reference enforcement
  and same-code reopen. Add no package, dependency or durable state.
- Probe supplier-owned reset/reuse of a fully retired migration allocation with
  larger, repeated and smaller candidate rows. Check physical growth/reuse, reset
  refusal before retirement, chunk rollback, upgrade resume and intact live data.
  Add three fixture control entries; no allocation, package or dependency. This
  proves a reuse mechanism, not a second published schema migration.

## [0.1.0] - 2026-11-13

### 📝 Summary

- Establish an unpublished Rust scaffold and proposed database restart design.
- Refine M0 around generic capability contracts, explicit write and pagination
  contracts, identified maintenance recovery and comparable resource evidence.
  Keep probes small; specify aggregate invalidation/finalization, fluent composition,
  typed result shapes and bounded reverse-reference checks before implementation.
- Target broad fluent Rust queries and attribute-based schemas, including grouping,
  distinct, projection and aggregates independently of application usage. Exclude
  SQL, a separate dynamic-query frontend, joins and window functions.
- Specify ordinary iterator mapping, Canic-owned endpoints and Wasm measurements
  for monomorphization growth; retain broad types, reference enforcement and
  constrained resumable aggregation in the proposed architecture.
- Prefer ordinary writes without a database WAL, subject to Wasm proof; retain
  versioned, resumable schema maintenance, accept hours-long outages and copy
  space, and exclude general runtime DDL.
- Add an isolated M0 Wasm rollback probe and explicit PocketIC setup/run commands.
  Verify row/index/allocation rollback, error-return controls and same-code reopen;
  record the supplier's cross-allocation reclamation limitation for maintenance.
- Probe bounded resumable grouping with atomic revision/progress state, exact
  completed results and explicit source-change invalidation.
- Add `make db` to install the `db` binary and local fixture. Run `db` for the
  interactive console or `db schema` / `db query` for scripts. Make the no-SQL CLI
  a product requirement; installation does not launch the console.
- Qualify one identified encoding/index transformation through interrupted copy,
  changed-transform refusal, cutover, cancellation, source-gate enforcement and
  upgrade-hook rollback. Logical cleanup leaves physical allocation unchanged.
- Add `make m0-batch` for final-state uniqueness, reference existence and indexed
  restricted deletion, including swaps and commit traps. Record the proposed
  rich-type operation matrix separately from implemented probe coverage.
- Add `make m0-reads` for index-order admission, sparse-page progress, encoded
  reply-byte limits and retry, ordered DISTINCT including absence, cursor binding
  and live insert/delete/update behavior. Reuse the existing qualification canister.
- Add native `make m0-contracts` witnesses for exact decimal/composite ordering,
  uniqueness/grouping equality, finite-float zero normalization, full-width U256
  keys and canonical schema sensitivity. No production codec or fingerprint is selected.
- Extend the native contract examples with projection/tuple DISTINCT, distinct
  counts, row versus group filtering, complete-result ordering/pagination and
  explicit retained-key/group capacity refusals across different work budgets.
- Consolidate M0 closeout and application-defined typed schemas for M1. Use generic
  capability requirements and applicable IcyDB lessons as primary design inputs.
  Retire obsolete local Toko observations and the Toko-derived schema/milestone
  proposal; use the private GitHub repository only as a tier-2 compatibility target.
- Adopt published Toko Tooling 0.42.5 with vendor-owned tooling and focused
  development commands. No database behavior is implemented.
- Establish private source releases to `dragginzgame/ic-db` using the shared
  version/commit/tag/push workflow and a gate covering the scaffold and M0 probes.
  This first checkpoint does not qualify a production database or complete M0.
- Review first-party suppliers for this checkpoint: retain Storage 0.74.0,
  Platform Runtime 0.22.2 and Wire 0.22.1. Defer optional Testkit 0.103.0
  (selected 0.102.0) and Toko Tooling 0.42.8 (selected 0.42.5) refreshes to M1
  preparation, where their compatibility and probe results will be reviewed.
  Select IC Build 0.84.1 solely as the release metadata editor.
