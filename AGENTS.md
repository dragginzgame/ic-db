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
  the local `make db` inspection/query CLI, not
  product database implementation, feature retirement or runtime migration.
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
  SQL, joins and window functions are excluded. Application usage does not define
  product breadth; distinguish SQL-only machinery from shared fluent behavior.
- Keep `make db` as the CLI installation command; it must not launch the console.
  The installed `db` command provides schema inspection and bounded queries.
  During M0 it opens an explicitly labelled local fixture. Future product
  access uses the same admitted fluent semantics through Canic-owned endpoints;
  the CLI must not introduce SQL or a second query engine.
- Retain broad schema types, reference checks/restricted deletion and constrained
  resumable aggregation. Ordinary result mapping uses Rust iterators, not engine
  callback hooks. Hours-long maintenance and source-plus-target space are accepted.
- Use small typed boundaries over shared execution machinery. Measure
  monomorphization growth across entities, fluent chains and result mapping.
  All product endpoints belong to the Canic adapter, not database macros.
- Feature deferrals in the design are proposals. They do not authorize removing
  capabilities from IcyDB or claiming full Toko adoption.
- Keep one current system design and executable examples as implementation grows.
  Use GitHub issues for actionable follow-ups once a remote exists; do not invent
  a parallel local issue queue. The design's open decisions are design questions.
- Product packages live under `crates/`. Add an independent `qualification/`
  workspace only when real native/Wasm experiments need it. Do not add dummy
  packages, runtime dependencies, tests or general frameworks to fill the scaffold.
- Keep `ic-db` as a private unpublished scaffold. The source release destination
  is `https://github.com/dragginzgame/ic-db.git`, branch `main`. Use the shared
  release commands and local adapter; no public licence or live deployment target
  has been selected. Qualification packages retain independent `0.0.0` versions.

The independent `qualification/` workspace owns its lockfile, M0 canisters and
native driver. `make m0-setup` explicitly prepares Testkit's server;
`make m0-contracts` checks native numeric/schema/query semantics without a server.
`make m0-rollback`, `make m0-batch`, `make m0-reads`, `make m0-aggregation` and
`make m0-maintenance` select the focused experiments separately. `make db` installs
to `~/.local` by default (`DB_INSTALL_PREFIX` overrides it). Run `db` for the console
or `db schema` / `db query --limit 2` for one noninteractive command.

## Qualification and simplicity

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
- Never assume a hard cut authorizes deleting retained data. Reject unsupported
  formats intact; same-contract recovery and future supported upgrades need proof.

## Local HTML links

- Present local HTML previews, reports and documentation with browser-capable
  HTTP URLs served locally. Verify the URL is reachable before sharing it.
  Do not link HTML files using workspace/file paths or VS Code/editor links.

## Commands

`make clippy`, `make check`, `make test` and `make doc` select the current library.
At this stage these qualify only a scaffold. `make docs-check` checks local links;
`make toko-tooling-check` verifies tooling selection. Tool preparation is explicit:
`make install-tools`, then `make tools-check`. Hooks use `make install-hooks`.
