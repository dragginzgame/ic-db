# IC DB local instructions

Apply the reviewed [Toko Tooling baseline](TOKO-TOOLING.md).
This overlay owns local authority, product boundaries and qualification choices.

## Authority and scope

- IC DB is an independent experiment, not an IcyDB replacement deployment.
  IcyDB's checkout and private `dragginzgame/icydb-legacy` repository are frozen:
  preserve committed and uncommitted work, releases and data. Research is read-only;
  edits, commits, pushes, merges, releases or deployments require an explicit exception or lift.
  Canic is also a read-only research input unless separately authorized.
- Private source releases to `dragginzgame/ic-db`, branch `main`, are authorized
  when requested. Keep packages private and unpublished. Crates.io publication,
  live-canister deployment, IcyDB feature retirement and runtime migration remain
  excluded. A hard cut never authorizes deleting retained data.
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
- IC Storage owns allocation/collections; Testkit owns PocketIC lifecycle;
  Canic's adapter owns endpoints, guards and composition; applications own
  authorization. Standalone qualification may use Platform hooks and bounded
  Wire transport outside product crates, never a second executor/endpoint registry.
- Keep two-stage schema expansion: Rust resolves paths, host-only registration
  collects declarations, then validation seals the graph and emits target data.
  Registration and `ctor` stay out of deployed Wasm.
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
  native wall-clock timings are not IC performance proof. Keep matched workloads,
  complete write journeys, cold/post-upgrade and warm costs separate.
- The maintainer's entity-growth objective is roughly 2 KB per added entity;
  roughly 4 KB from IcyDB is the working comparison ceiling. Reachable entity
  growth remains above that ceiling; the design owns current measurements. Do not
  weaken reachable behavior or increase the budget to pass. Common read/write
  regressions require an explicit tradeoff even when Wasm shrinks.
- IC Storage is expected to change. Do not shape IC DB around its current
  allocation/reclamation quirks; retain version-bound probes and requalify after
  the overhaul while preserving correctness, bounded execution and data safety.
- After each patch, review its responsibility and callers for drift; remove new
  sediment in the same scope without broadening tests. Report responsibility,
  durable-state and representation changes, with LOC only when requested.

## IcyDB research and explanations

Before substantive subsystem work, inspect the relevant frozen implementation,
regressions and explanatory history. Identify the original failure, solution and
changed assumptions. Reuse sound code/algorithms when they fit; independent
reimplementation is not itself a benefit. Carry applicable regressions into IC DB
and qualify them here. Explain concrete replacements, exclusions, lost guarantees
and moved responsibilities; cite source/revision and distinguish adapted code
from retained behavior or independently implemented lessons. Keep enduring
rationale in the existing system design, not another comparison ledger. Historical
passes, feature deferrals and design hypotheses do not establish parity, savings
or full Toko adoption.

## Local commands and presentation

`make clippy`, `make check`, `make test` and `make doc` select the native library;
they are aggregate library checks, not automatically focused. `make docs-check`
checks local links; `make toko-tooling-check` checks both workspaces' selection.
Prepare tools explicitly with `make install-tools` and `make tools-check`; hooks
use `make install-hooks`. Run `make fmt` after code/manifest edits and
`make fmt-check` for verification. Use `toko-cloc` for source counts.

Present local HTML through a verified, reachable local HTTP URL, not a filesystem
or editor link. Keep examples executable and claims bounded to actual evidence.
