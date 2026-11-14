# IC DB local instructions

Apply the reviewed [Toko Tooling baseline](TOKO-TOOLING.md). The root
`.toko-tooling.snapshot` owns installed bytes; update through its published
installer. Do not edit vendor files or copy upstream reference documents.

## Product direction

- This is a separate design-stage restart, not a renamed IcyDB or a replacement
  deployment. IcyDB and Canic are read-only research inputs unless separately
  authorized. Preserve their source, history, releases and data.
- Maintainer decision: IcyDB's local checkout and GitHub repository are frozen
  pending the IC DB experiment. Keep existing uncommitted work intact. Read-only
  research is allowed; do not edit, commit, push, merge, release or deploy IcyDB
  unless the maintainer explicitly lifts the freeze or authorizes an exception.
- Read [the proposed system design](docs/design/ic-db-system-design.md). The
  maintainer authorized repository setup, design, M0 investigation/probes and
  the local `make db` inspection/query CLI. On 2026-11-14 the maintainer also
  authorized the defined M1 product implementation once the remaining M0 gates
  are satisfied. The requested authoring comparison is complete; the maintainer
  selected the current attribute-owned declarations with empty struct bodies.
  Feature retirement and runtime migration remain excluded.
  Private source releases to `dragginzgame/ic-db` are now authorized when requested;
  crates.io publication and live-canister deployment remain excluded.
- Build a multipurpose database engine specialized for the IC. Derive mechanisms
  from generic database capabilities, IC constraints and applicable IcyDB lessons.
  The private `dragginzgame/toko` GitHub repository is a tier-2 compatibility
  target, not an architectural guide, feature ceiling or milestone dependency.
  Inspect it read-only at recorded revisions when checking application coverage;
  translate gaps into generic requirements instead of adding Toko-specific behavior.
  The deleted local Toko checkout was obsolete: do not use or restore it as evidence.
  Toko Tooling remains the independently selected engineering baseline.
- Existing IcyDB modules, APIs, tests and planned features have no automatic
  right to reappear.
  Preserve the underlying required invariants; rederive their implementation.
- Target broad bounded fluent queries and familiar attribute-based schemas.
  Keep the current attribute-owned entity/record declarations with empty struct
  bodies: the maintainer finds them easier to read than annotated actual fields.
  Alternative authoring styles are future optional work, not an M1 prerequisite
  or a second supported interface. Blanket source compatibility with IcyDB is
  still not required; avoid mechanisms added only for hypothetical flexibility.
  Field declarations must not require numeric IDs. Use field names as their
  contract identity; renames require an explicit supported schema transition,
  not inferred matching or a hidden ID registry. Latest maintainer direction also
  removes mandatory numeric entity/index/key identifiers from schema authoring.
  Use compiler-resolved types, field paths and unambiguous derived or explicit
  schema/index names; keep any numeric storage handles internal. Select an existing
  bounded reverse index automatically when unambiguous, with a named selector
  when needed; missing access paths still refuse. The scalar macros now use logical names, optional `name` overrides and typed
  reference paths. Internal generated handles are collision-checked, never schema
  authority; the full named contract is versioned. Earlier numbered data refuses
  intact until a supported transition exists. Record primary-key
  values remain application data, distinct from schema identifiers.
  Nested types and enum variants also use declared schema names;
  compact stored enum ordinals come from canonical variant-name order under the
  admitted contract. Renames require transitions; declaration reorderings do not.
  Attribute removals are permitted with a concrete reason: report
  each removed attribute and the changed behavior or replacement to the maintainer.
  SQL, joins and window functions are excluded. Application usage does not define
  product breadth; distinguish SQL-only machinery from shared fluent behavior.
- Keep `make db` as the CLI installation command; it must not launch the console.
  The installed `db` command provides schema inspection and bounded queries.
  It currently opens an explicitly labelled local M1 record fixture. Future product
  access uses the same admitted fluent semantics through Canic-owned endpoints;
  the CLI must not introduce SQL or a second query engine.
- Retain broad schema types, reference checks/restricted deletion and constrained
  resumable aggregation. Ordinary result mapping uses Rust iterators, not engine
  callback hooks. Hours-long maintenance and source-plus-target space are accepted.
- Accepted 2026-11-14: exact aggregates use conservative per-entity revision
  invalidation and bounded server-retained jobs; completion during continuous
  writes is not promised. Jobs may expire and incompatible upgrades invalidate
  them. Capacity exhaustion refuses with bounded cleanup, without automatic spill.
  Decimal averages use a documented precision/rounding contract and overflow
  errors. Earlier acceptance of explicit entity/index IDs is superseded by the
  subsequent name/path-based authoring decision above. Common read/write performance
  regressions require an explicit tradeoff decision even when Wasm is smaller.
- Use small typed boundaries over shared execution machinery. Measure
  monomorphization growth across entities, fluent chains and result mapping.
  All product endpoints belong to the Canic adapter, not database macros.
- Retain IcyDB's two-stage schema path expansion: Rust resolves typed paths,
  host-only `ctor` registration collects declarations, then a second stage
  validates/seals the graph and emits target artifacts. Keep collection and ctor
  out of deployed Wasm. The scalar entity pipeline is implemented through the
  optional `ic-db/build` feature; named/nested schema types remain work. Build-time
  Rust paths do not silently replace declared durable names.
- IC DB owns its durable row codec. Keep `ic-wire` and transport serialization
  out of product dependencies and persisted encoding. Transport adapters may
  convert at their boundary; wire-format optimization must not alter stored
  bytes or decoding. Version durable codec changes explicitly and refuse
  unsupported old contracts intact until a supported transition exists.
  Start codec work from IcyDB's database-owned implementations and regression
  tests; adapt them to this compiled-schema contract instead of using its Wire
  paths. Explain departures and qualify the resulting bytes independently.
- Feature deferrals in the design are proposals. They do not authorize removing
  capabilities from IcyDB or claiming full Toko adoption.
- Keep one current system design and executable examples as implementation grows.
  When a release changes or adds CLI behavior, include a concise `▶️ Try it`
  section in its changelog with copyable fenced `sh` blocks using real `db`
  commands, following Canic's presentation. State setup and fixture/deployment
  scope; do not imply proposed commands or product capabilities are implemented.
  Use GitHub issues for actionable follow-ups once a remote exists; do not invent
  a parallel local issue queue. The design's open decisions are design questions.
- Product packages live under `crates/`. Add an independent `qualification/`
  workspace only when real native/Wasm experiments need it. Do not add dummy
  packages, runtime dependencies, tests or general frameworks to fill the scaffold.
- Keep `ic-db` private and unpublished while the experiment proceeds. The source release destination
  is `https://github.com/dragginzgame/ic-db.git`, branch `main`. Use the shared
  release commands and local adapter; no public licence or live deployment target
  has been selected. Qualification packages retain independent `0.0.0` versions.

The independent `qualification/` workspace owns its lockfile, M0 canisters and
native driver, plus the M1 host consuming the actual product library.
`make m1-records` selects native generated records and their Wasm/reopen/message
boundary checks. This first scalar slice does not complete M1's type/reference,
resource-failure or matched-performance gates.
`make m1-commit-failures` selects real instruction exhaustion after a backing write,
backing-write traps and physical growth refusal
inside real product commits. Its fault feature belongs only to qualification;
the normal CLI host and product library do not expose injection controls.
`make m0-setup` explicitly prepares Testkit's server;
`make m0-contracts` checks native numeric/schema/row/query semantics without a server.
`make m0-rollback`, `make m0-batch`, `make m0-reads`, `make m0-aggregation` and
`make m0-maintenance` select the focused experiments separately. `make db` installs
to `~/.local` by default (`DB_INSTALL_PREFIX` overrides it). Run `db` for the console
or `db schema` / `db query --limit 2` for one noninteractive command.
`make m0-icydb-baseline` separately measures the immutable SQL-disabled IcyDB
comparison host. Its source comes from the pinned Git dependency, with suppliers
selected by the qualification lockfile. It does not read or modify the frozen
sibling checkout, establish IC DB savings, or run implicitly in scaffold CI.
`make m1-records-comparison` explicitly selects the same scalar workload for a
one-entity IC DB qualification host and that frozen IcyDB host. Keep matched
input/result checks, feature/build evidence and complete write journeys including
IcyDB settling. Direct IC DB index reads are not yet a fluent-planner comparison;
do not extrapolate this slice to the full retained product scope.

## Qualification and simplicity

- After every patch, review the changed responsibility and its affected callers
  for design drift and architectural sediment: duplicated policy or state,
  unnecessary abstractions, obsolete paths, misleading documentation and
  misplaced ownership. Remove newly introduced sediment within the same change;
  do not defer it to a future cleanup. Keep the review proportional to the patch,
  explain material findings and fixes, and do not add a general audit framework
  or broaden testing merely to perform this review.
- Run focused checks for changed behavior. Aggregate `make ci`, full product
  suites and release qualification require explicit selection or configured CI.
- Use `make fmt` after code/manifest edits and `make fmt-check` for verification.
- Test externally observable invariants and failure boundaries, not retired names
  or the exact current module structure. Keep independent oracles independent.
- Report net responsibility, durable-state and representation changes; measure
  non-test/test LOC separately when requested. Do not trade a deletion for a
  more general framework without a demonstrated consumer need.
- Performance measures are total uncompressed Wasm bytes, code-section bytes,
  IC cycles and instructions. Native/wall-clock timings are not performance proof.
- IC Storage owns backing allocation and collection mechanics. IC Testkit owns
  PocketIC lifecycle. The Canic adapter owns endpoints, guards and composition;
  the application owns authorization policy.
- Maintainer direction (2026-11-14): IC Storage is expected to receive an overhaul
  when Rust OP is available. Do not optimize IC DB's architecture around the
  current supplier's allocation layout, reclamation limits or performance quirks.
  Keep current probes as version-specific evidence; defer supplier-specific
  optimization and requalify after the overhaul. Preserve required correctness,
  bounded execution and data safety with whichever supplier version is selected.
- Never assume a hard cut authorizes deleting retained data. Reject unsupported
  formats intact; same-contract recovery and future supported upgrades need proof.

## Explaining IcyDB carryover and design decisions

- Before implementing or substantially changing a subsystem, inspect the relevant
  frozen IcyDB implementation and regression tests; consult its change history or
  investigations where they explain a fix or tradeoff. Identify the original
  failure, established solution and which assumptions change under IC DB. This
  is targeted research, not a full IcyDB audit for every routine edit.
- Reuse or adapt sound IcyDB code and algorithms when they fit the chosen contract;
  independent reimplementation is not itself a benefit. Carry applicable failure
  cases into IC DB qualification and prove they pass here. Explain concrete reasons
  for replacing a solved mechanism; repository size or age alone is not a reason.
- When relevant to the work, explain what is being carried over from IcyDB and
  why. Distinguish copied/adapted code from retained behavior, API ergonomics,
  algorithms or lessons reimplemented independently; cite the inspected source
  or revision when making a concrete comparison.
- Explain what is not being retained and why, distinguishing excluded features,
  replaced mechanisms and deferred work. Identify any lost capability, changed
  guarantee or responsibility moved to an application, adapter or supplier.
- Connect those choices to accepted design decisions and evidence, including
  tradeoffs and unresolved assumptions. Do not imply measured savings or parity
  where only a design hypothesis exists.
- Include concise comparisons in appropriate progress updates and final reports;
  do not repeat the full comparison for routine edits. Keep enduring rationale
  in the current system design rather than a separate comparison ledger.

## Local HTML links

- Present local HTML previews, reports and documentation with browser-capable
  HTTP URLs served locally. Verify the URL is reachable before sharing it.
  Do not link HTML files using workspace/file paths or VS Code/editor links.

## Commands

`make clippy`, `make check`, `make test` and `make doc` select the current library.
These qualify the current native library slice. `make docs-check` checks local links;
`make toko-tooling-check` verifies tooling selection. Tool preparation is explicit:
`make install-tools`, then `make tools-check`. Hooks use `make install-hooks`.
Use `toko-cloc` for source counts; `make cloc` and `CLOC_*` settings are retired.
