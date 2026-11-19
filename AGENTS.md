# IC DB local instructions

Apply the reviewed [Toko Tooling baseline](TOKO-TOOLING.md).
This overlay owns local authority, product boundaries and qualification choices.

## Authority and scope

- IC DB is an independent experiment, not an IcyDB replacement deployment.
  IcyDB's checkout and private `dragginzgame/icydb-legacy` repository are frozen:
  preserve committed and uncommitted work, releases and data. Research is read-only;
  edits, commits, pushes, merges, releases or deployments require an explicit exception or lift.
  Canic is also a read-only research input unless separately authorized.
- **Deploy, ship, or release** means deliver the verified source and tag to
  `dragginzgame/ic-db` on `main`, then publish every crate selected for that
  release to crates.io. The product crates use `publish = ["crates-io"]`.
  The maintainer retired the old blanket `publish = false` rule, including
  copies in chat-provided AGENTS snapshots. A deploy request authorizes both
  destinations without a separate publication confirmation. Use the
  Makefile’s `CARGO_PUBLISH_PACKAGES` as the package list so future changes to
  the release group apply automatically. Run `make publish` from the verified
  tag and confirm each selected crate is visible at the release version.
  A source-only delivery requires an explicit source-only request. The CLI and
  qualification packages stay private. Live-canister deployment, IcyDB feature
  retirement and runtime migration require separate authorization.
- Read the opening status and affected sections of the
  [system design](docs/design/ic-db-system-design.md), not its entire historical
  evidence inventory on every task. Its delivery section owns implemented scope,
  remaining milestone gates and current qualification limits. M1 implementation
  was authorized after its M0 prerequisites; this is no longer design-only work.
- Build a multipurpose IC database. Toko is a tier-2 compatibility target, not
  the architecture, feature ceiling or milestone dependency. Authorized read-only
  research uses recorded GitHub revisions; do not restore its obsolete checkout.
  Existing IcyDB modules, APIs and plans have no automatic right to reappear.

## Product boundaries and contract owners

- Use attribute-owned declarations with empty struct bodies and logical schema
  names/typed paths. Numeric storage handles stay internal. Renames require
  explicit transitions; do not infer identity from declaration order. Alternative
  authoring styles and blanket IcyDB source compatibility are not prerequisites.
  Explain each removed attribute and its replacement or changed behavior.
- Retain broad bounded fluent queries, schema types, reference integrity and
  constrained resumable aggregation. SQL, joins and window functions are excluded.
  Ordinary result mapping uses Rust iterators, not engine callback hooks.
- IC DB owns its durable codec. Keep Wire and transport serialization out of
  product dependencies and stored bytes. Version durable changes; unsupported
  formats refuse intact until a supported transition exists.
- IC Storage owns allocation/collections. Qualification borrows PocketIC's
  original async client through Testkit's default-feature-free export; a prepared
  command-scoped launcher owns the server.
  Keep setup explicit and attempt instance deletion explicitly; no replacement
  lifecycle framework belongs in IC DB. The released 0.110.0 launcher remains
  an external bootstrap tool, independent of the lightweight Testkit library;
  retain per-attempt diagnostics and its explicit one-hour command bound.
  Canic's adapter owns endpoints, guards and composition; applications own
  authorization. Standalone qualification may use Platform hooks and bounded
  Wire transport outside product crates, never a second executor/endpoint registry.
- The maintainer authorized 0.22.1 source delivery after the direct Agent finding.
  This scopes an exception to the external-IC dependency ownership rule to the
  native inspection/listing client and its qualification. Retain Agent 0.49.3
  until Platform supplies the supported secp256k1 query path tracked in
  [Platform #4](https://github.com/dragginzgame/ic-platform/issues/4), then migrate
  and remove this exception. It grants no advisory exemption, live deployment,
  product-engine dependency or sibling implementation authority.
- Keep two-stage schema expansion: Rust resolves paths, host-only registration
  collects declarations, then validation seals the graph and emits target data.
  Emit target-independent schema Rust. A generated `cfg` or `cfg_attr` needs a
  specific target-exclusive operation. IC DB owns the host registration gate
  so registration and `ctor` stay out of deployed Wasm.
- Hours-long maintenance and source-plus-target space are accepted. Preserve
  source/target identity, closed ordinary access during maintenance, rollback,
  supported cancellation, validated publication and retired-source custody.
  Do not add an inferred DDL planner, ordinary-write WAL or reset/reuse authority.
- Exact aggregates use per-entity revision invalidation and bounded retained
  jobs; continuous-write completion is not promised. Expiry/incompatible upgrades
  must retain supported cleanup. Capacity exhaustion refuses without automatic spill.

Read the owning design contract before changing its behavior:

| Work | Existing design owner |
| --- | --- |
| Authoring, names, types and durable admission | [Schema and identity](docs/design/ic-db-system-design.md#4-schema-and-identity-challenge-the-persisted-catalog) |
| Copy, cancellation, publication and source cleanup | [Schema maintenance](docs/design/ic-db-system-design.md#schema-maintenance-and-the-ddl-boundary) |
| Ordering, filtering, projection and aggregates | [Reads](docs/design/ic-db-system-design.md#5-reads-and-query-semantics) |
| Batches, defaults, managed time and recovery | [Writes](docs/design/ic-db-system-design.md#6-writes-commit-and-recovery) |
| Implemented CLI/record/query slices and open gates | [Delivery](docs/design/ic-db-system-design.md#9-delivery-and-deletion-gates) |
| Matched experiments and growth limits | [Measurement](docs/design/ic-db-system-design.md#10-measurement-and-qualification) |

## CLI and qualification

- `make db` installs the CLI to `~/.local` (`DB_INSTALL_PREFIX` overrides it);
  it must not launch the console. Put tutorials in `--help`, not normal output.
  Use actual implemented `db` commands for quick starts and identify fixture scope.
  The CLI uses admitted engine semantics; no SQL, client reducer, hidden restart,
  migration controller or second query engine. Production endpoints stay in Canic.
- Product, normal CLI and measurement builds must not expose qualification fault
  controls. Keep raw-byte/tuple oracles independent of product codecs. Failed
  resource checks must prove backing preservation and supported later progress.
- Use the [qualification guide](qualification/README.md) and owning design slice
  to select focused native/Wasm checks. `make m0-setup` explicitly prepares the
  simulator; `make m0-contracts` needs no server. The immutable IcyDB baseline is
  an explicit comparison, never an implicit CI run or mutation of its checkout.
- Measures are total uncompressed Wasm/code bytes, IC cycles and instructions;
  native time is not IC performance proof. Keep complete, matched write journeys
  and cold/post-upgrade/warm costs separate. The [comparison matrix](qualification/README.md#comparison-with-frozen-icydb)
  owns receipts, historical baselines and unmeasured gaps. Retain the isolated
  baseline until the maintainer revisits retirement; it is not a release gate.
- Entity-growth objective remains roughly 2 KB per entity, with roughly 4 KB as
  the IcyDB comparison ceiling. The design owns current measurements. Do not
  weaken reachable behavior or increase budgets to pass; read/write regressions
  require an explicit tradeoff even when Wasm shrinks.
- Use Storage’s named stable-memory allocations. Keep Wire out of the product dependency graph. Preserve the DB-owned codecs and reject incompatible backing intact.
- Storage source changes needed for this experimental integration are authorized.
  Preserve concurrent sibling work and follow Storage's local instructions; this
  permission does not authorize sibling publication or deployment.
- After each patch, review its responsibility and callers for drift; remove new
  sediment in the same scope without broadening tests. Report responsibility,
  durable-state and representation changes, with LOC only when requested.

## IcyDB research

Before substantive subsystem work, inspect the relevant frozen predecessor,
regressions and explanatory history read-only. Identify the solved invariant and
changed assumptions; existing APIs and plans do not establish current requirements.
Reuse sound solutions when they fit, bring applicable regressions into the new
owner and qualify them here. Cite exact source/revisions and explain exclusions,
lost guarantees and moved responsibilities. Keep enduring rationale in the
system design; historical passes do not establish parity, savings or Toko adoption.

## Local commands and presentation

The [release scope and recovery contract](README.md#validation-scope-and-recovery)
owns gate selection. Run `make release-adapter-test` after orchestration changes;
use the shared runner's retained evidence and saved phases. Partial failed-gate
logs are not reusable proof; do not add local skip flags or another receipt framework.

`make clippy`, `make check`, `make test` and `make doc` select the native library;
they are aggregate library checks, not automatically focused. `make docs-check`
checks local links; `make toko-tooling-check` checks both workspaces' selection.
Prepare tools explicitly with `make install-tools` and `make tools-check`; hooks
use `make install-hooks`. Run `make fmt` after code/manifest edits and
`make fmt-check` for verification. Use `toko-cloc` for source counts.

Present local HTML through a verified, reachable local HTTP URL, not a filesystem
or editor link. Keep examples executable and claims bounded to actual evidence.
