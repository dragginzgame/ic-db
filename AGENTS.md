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
  Unit enums support secondary indexes through the existing component codec;
  operands must match the complete declared enum. Any payload variant excludes
  the whole enum from indexes. No persisted enum catalog or numeric authoring IDs.
  Attribute removals are permitted with a concrete reason: report
  each removed attribute and the changed behavior or replacement to the maintainer.
  SQL, joins and window functions are excluded. Application usage does not define
  product breadth; distinguish SQL-only machinery from shared fluent behavior.
- Keep `make db` as the CLI installation command; it must not launch the console.
  Normal CLI output should contain requested information, not explanatory help
  footers or command tutorials. Put usage guidance and semantic explanations in
  `--help`; retain result data, continuation information and clear errors.
  The installed `db` command provides schema inspection and bounded queries.
  `db aggregate Entry` demonstrates retained scalar/category-grouped counts,
  DISTINCT counts and min/max/sum/avg through typed fluent terminals. Document
  adds optional note/priority counts and extrema, DISTINCT notes and priority/score
  sums, optionally grouped by entry_id. DecimalRow exposes row/field/DISTINCT counts and required/
  optional Decimal min/max/sums/averages, optionally grouped by zone, through the same
  fixed-scale result records as synchronous queries. Show progress
  and completed pages, follow server continuations, then cancel the temporary job
  on success or refusal. Keep its 32-call fixture bound and 60-second job expiry;
  no hidden restart or client reducer. `retained-console` enables its host store
  with the same entity-specific filters and exact group-threshold parsing as synchronous
  query. Reconstruct/bind input filters, group selection and total limit on every
  call; keep total limit separate from response page size. Do not filter or sort
  returned groups in the CLI. This feature runs
  without qualification fault controls. Keep normal product/measurement builds
  independent of this optional fixture feature.
  Parse bounded CLI group-threshold text (at most 64 bytes per bound) directly into the
  selected result type: u64 counts/extrema, u128 unsigned sums, i128 signed sums
  or Decimal. Reuse existing parsers and engine scale/range admission; never
  round thresholds or convert through floating point. `--group-min` and
  `--group-max` compose inclusive engine predicates before output selection;
  validate both operands even when the range would select no groups.
  It currently opens an explicitly labelled local M1 record fixture. Future product
  access uses the same admitted fluent semantics through Canic-owned endpoints;
  the CLI must not introduce SQL or a second query engine.
- Retain broad schema types, reference checks/restricted deletion and constrained
  resumable aggregation. Ordinary result mapping uses Rust iterators, not engine
  callback hooks. Hours-long maintenance and source-plus-target space are accepted.
- Accepted 2026-11-14: exact aggregates use conservative per-entity revision
  invalidation and bounded server-retained jobs; completion during continuous
  writes is not promised. Jobs may expire; incompatible upgrades must refuse their
  resumption and preserve a supported cleanup path. Capacity exhaustion refuses
  with bounded cleanup, without automatic spill.
  Accepted 2026-11-15: integer/Decimal averages use the result record's declared
  Decimal scale, rounding once to nearest with ties to even, with overflow errors.
  Earlier acceptance of explicit entity/index IDs is superseded by the
  subsequent name/path-based authoring decision above. Common read/write performance
  regressions require an explicit tradeoff decision even when Wasm is smaller.
- Use small typed boundaries over shared execution machinery.
  Fluent reads use exact top-level comparisons and proven ordered paths;
  no predicate callbacks. `order_by` adds a primary-key tie-breaker, uses one
  direction and selects bounded equality-prefix paths from compiled metadata.
  Explicit paths undergo the same order proof. Refuse incomplete order rather
  than sorting a bounded input prefix. Keep work limits separate from result limits.
  Empty pages may continue; byte exhaustion must retry the unreturned candidate.
  Cursor frames carry bounded logical positions, not trusted progress or access
  claims; validate database/schema/query/direction and complete key framing.
  Borrowed field inspection is shared with mutation integrity; decode full entities
  only for returned rows. `make m2-queries` selects this slice's native/Wasm probe.
  `fetch_as::<Record>()` uses existing named-record metadata and codecs for exact
  top-level projection. Validate the full transitive named definitions, retain
  whole-source corruption checks and count projected bytes before materialization.
  DISTINCT proves adjacent equality for a leading-index projection after fixed
  prefix components, or identity through the primary key. Nonidentity filters
  may read projected fields or fixed prefix fields: every duplicate class must
  have one predicate result. Carry the actual selected prefix count from ordering;
  do not reconstruct planner choices in DISTINCT admission.
  Reuse physical-key equality and bounded logical cursors; do not substitute
  page-local deduplication or trust cursor claims about emitted groups. Filters
  varying within a duplicate class need a separate progress proof and currently
  refuse. Broader DISTINCT and aggregation remain required implementation work.
  Synchronous `count`/`group_count::<Record>` reuse the scanner and exact ordered
  scalar keys. Absent keys form a group; row counts include those rows. Scan all
  matching input before applying output limits; never return prefix counts after
  work exhaustion. Keep group/state/output bounds separate. Grouped count output
  uses canonical field-name tuple order, without a required index. No durable
  jobs or extra cursor state are implied by those synchronous terminals.
  Follow the [retained aggregation contract](docs/design/ic-db-system-design.md#constrained-resumable-aggregation)
  for job quotas, identities, revision invalidation, retry, expiry and cleanup.
  Keep one host-governed job allocation and atomically save accumulator/cursor/
  sequence. Re-admit fluent queries; persist no second query AST, scheduler,
  snapshots, WAL or closures. Synchronous, grouped and retained numeric terminals
  share numeric admission and exact reducers. Average retains total/present count
  and rounds once at completion; keep operation/result-scale bindings explicit
  and existing job identities stable. `make m2-jobs` selects native/Wasm recovery
  qualification. Retained grouped counts/sums/averages share the grouping table and
  complete output selection; persist key records, counts and numeric totals,
  reconstructing comparison keys. Numeric groups retain their own present counts,
  including zero for all-absent groups; validate every completed group before selection.
  Group predicates, order and output row limit bind the job. Keep the existing
  job-frame cap: resource refusal must preserve the checkpoint even when output
  filters hide the oversized state. Retained grouped resumes return GroupPage;
  retain its continuation when mapping rows. `group_page` changes only response
  budgets/position, never the bound total limit or input semantics. Finalize all
  groups before paging, retry byte-limited unreturned groups, and validate positions
  against the completed job. Keep cursors client-owned, without a durable output cache.
  Keep detailed limits in the design
  and public API documentation.
  `count_field` counts presence for any top-level stored type; empty values count.
  `group_count_field` retains zero-count groups. Their retained start/resume
  terminals share count checkpoints and bind the selected field, even when required.
  No field-specific job state or second presence rule. Scalar `count_distinct::<Record>`
  and `group_count_distinct::<Key, Record>` share a bounded comparison-key set and
  include absent tuples; an all-absent group therefore has distinct count one.
  Cap distinct tuples at 32 per set, independently of the 32-group cap, and charge
  all retained key capacities to the shared byte budget before output selection.
  Scalar distinct counts retain no projected records. Keep these semantics
  distinct from present-field counts, row counts and output limits.
  Retained scalar/grouped DISTINCT counts persist the same canonical key sets within
  one job frame; restore shares full key-framing and strict-order validation.
  Each group has its own bounded set; group/operand projections bind the job.
  Preserve absent tuples and refuse oversized state without losing progress.
  `min`/`max` use exact single-field record projections and the same scanner.
  Ignore absent operands, preserve stored types and retain only the winning
  key/value. Finish input before applying the output-byte budget; no prefix
  extrema or implicit numeric coercion. Grouped min/max use separate exact key
  and single-field operand records, retaining all-absent groups as `None`.
  Counts and extrema share one bounded grouping table. Charge changing winners
  and release replaced allocations; an output limit never relaxes group/state caps.
  Retained scalar min/max reuse that comparator and the existing job lifecycle.
  Store only winner record bytes; validate presence and full framing, then rebuild
  the comparison key on resume. Operation/projection identities remain distinct.
  Keep whole-job state limits separate from completed output limits; no prefix
  winner is an exact result. Retained grouped min/max store key records and optional
  winners under the same 32-group/4096-byte job bounds. Scalar/grouped winner
  restoration shares presence/framing validation. Keep all-absent groups, bind
  key/operand/operation/selection, and reuse completed group pagination. Output
  selection cannot relax state bounds; no separate extrema executor or job store.
  `filter_groups` and `order_groups` act only on completed grouped values before
  output limits. Exact count operands are Nat64; extrema retain source types.
  Aggregate ties use ascending canonical keys; absent extrema sort first ascending
  and last descending. Scalar/row terminals refuse group modifiers. Keep the
  bounded 32-group sort concrete; output filters cannot relax scan/state caps.
  Scalar integer `sum::<Record>` selects one named field and explicit same-family
  result widening while preserving optionality. Use checked I256/U256 totals;
  validate the final result range after accumulation so signed prefixes can cancel.
  Distinguish arithmetic overflow from resource refusal, absence from present zero.
  Grouped sums use the same reducer/table and predicates use their declared result
  type. Finalize every group before filtering or limiting: excluded overflow still
  refuses. Charge final encoded keys/results to the retained-state budget. Min/max/
  sum share GroupValue; remove the narrower GroupExtremum name. Reuse the scanner
  and owned codec. Decimal sums use the same I256 accumulator at the exact declared
  scale; reject rescaling/conversion and check the final i128 coefficient, including
  groups later excluded. `avg`/`group_avg` share the exact numeric accumulator,
  divide by the present count before result conversion, and retain absent groups.
  Group filters/order compare the rounded result. No precision fallback; U256
  totals too large for the bounded Decimal average refuse. Count the average's
  denominator in retained state. Float arithmetic remains separate work.
  No implicit numeric coercion or general reducer framework.
  Output records are not durable schema additions. The local CLI constructs
  fluent queries for Entry/Document/DecimalRow, uses opaque continuation and display-only
  result mapping; no client-side filtering or paging engine. `make m2-cli` checks
  installed-layout use and the migrated query caller. Broader entity adapters,
  optimized range seeks and richer sorting remain subsequent work.
  Measure monomorphization growth across entities, fluent chains and result mapping.
  All product endpoints belong to the Canic adapter, not database macros.
  Standalone hosts may use Platform's generic entrypoint/guard/codec hooks without
  Canic. Qualification uses those hooks with bounded Wire frames; keep its small
  transport helpers outside product crates. Do not restore Wire Endpoint, a second
  executor or an endpoint registry. `make m0-endpoints` checks admission and reply
  failure across synchronous and callback execution boundaries.
  Shared fixture codecs live in the private `qualification/transport` crate,
  with ordinary Cargo dependencies rather than shared-source path modules.
- Retain IcyDB's two-stage schema path expansion: Rust resolves typed paths,
  host-only `ctor` registration collects declarations, then a second stage
  validates/seals the graph and emits target artifacts. Keep collection and ctor
  out of deployed Wasm. Qualification's `records-schema` crate supplies the same
  declarations to host and target through ordinary Cargo dependencies; keep it
  private and outside the product workspace. The scalar entity pipeline is implemented through the
  optional `ic-db/build` feature. Named records and unit/payload enums resolve through entity field
  type paths and enter one finite compiled graph. Recursive values use explicit
  `item(indirect, is = "...")` boxing and need a finite constructor. All nested
  encoding, decoding and stored inspection share row byte/depth/node bounds.
  Ordered lists use `value(many, max_items = N, item(...))`, optionally `opt`.
  Count admission precedes allocation/traversal; per-row limits also cover total
  collection items and structured values. Recursive list edges provide an empty base
  without Box. Scalar/unit-enum sets use `#[set(item(...), max_items = N)]` and
  `BTreeSet`; stored duplicates or non-increasing elements refuse before
  materialization can erase them. Lists and sets share one row item budget.
  Named maps use `#[map(key(...), value(...), max_items = N)]` over `BTreeMap`:
  scalar/unit-enum keys have the set ordering rules; values use ordinary field shapes,
  including optional, list and recursive named values. Stored duplicate keys
  refuse before insertion can overwrite them. A map entry counts once in the
  shared collection budget, with nested collection contents counted additionally.
  Unit enums share canonical variant-name order across indexes, sets and map keys;
  each stored enum key counts toward the structured-value budget. Payload enums
  and named records/collections remain unsupported as set elements or map keys.
  Nested indexes/references and collection indexes remain work. Build-time
  Rust paths do not silently replace declared durable names.
- IC DB owns its durable row codec. Keep `ic-wire` and transport serialization
  out of product dependencies and persisted encoding. Transport adapters may
  convert at their boundary; wire-format optimization must not alter stored
  bytes or decoding. Version durable codec changes explicitly and refuse
  unsupported old contracts intact until a supported transition exists.
  Complete secondary-index keys have one fixed 512-byte cap, including namespace,
  components and any primary-key suffix. Keep the bounded key codec on the existing
  map; query prefixes reserve minimum completion space. Admit after-images before
  no-op shortcuts and preserve sparse omission. No truncation, hashing or per-index
  escape hatch. Semantics version 2 refuses prior wider-key contracts intact.
  Opening checks the current manifest, not every entity's revision. Validate
  counters on explicit revision reads and once per touched entity during batch
  preparation, including no-ops; advance only entities with real changes.
  Missing/malformed unrelated counters do not block ordinary row reads or other
  entities' writes. Never retain revision values or durable admission across calls.
  Non-indexed row values and future grouping/DISTINCT retain separate bounds.
  U256 uses the re-exported feature-free `ethnum::U256` with an explicitly owned
  32-byte big-endian row/index codec, adapted from IcyDB. Do not introduce Wire
  serialization or a numeric conversion engine for that storage boundary.
  Float32/Float64 use finite wrappers with one positive-zero representation.
  Constructors normalize signed zero; stored non-finite/negative-zero bytes refuse.
  Equality, hashing, collections and ordered keys must share this contract.
  Account/Subaccount reuse Platform Types values with database-owned codecs.
  Account identity and bytes use the effective 32-byte subaccount: absent and
  explicit zero agree. Ordering compares owner then effective subaccount;
  optional account absence remains distinct from a present default account.
  Ulid retains IcyDB's exact 16-byte atom and canonical uppercase text wrapper.
  Keep the ulid crate's default std/rand features disabled. Storage support does
  not authorize an OS RNG, a hidden clock read or automatic generation. Before
  generated-ID work, carry forward the operation-timestamp, same/backward-time,
  overflow, entropy and upgrade/reset lessons recorded in the current design.
  The explicit `UlidGenerator` now accepts an operation Timestamp and host-provided
  80-bit randomness. The host owns its state/checkpoint; errors preserve state,
  expected post-issuance refusals may consume IDs, and an IC trap rolls back the
  current execution. Do not infer automatic generation attributes or durable
  issuer recovery from this utility. Never substitute a timestamp/sequence for
  independently provisioned randomness across separate/reset streams.
  Supplied integer/ULID/Principal/Account/Subaccount primary keys keep lookups, deletes and page cursors typed
  to the entity, including signed/unsigned 8–128-bit integers and U256. References
  must match the target key type exactly; other identity/composite keys
  and automatic generation remain separate increments. Principal keys reuse the
  length-first index encoding, including the empty management principal. Consume
  framed reference components instead of assuming every target has a fixed width.
  Account keys use owner then effective subaccount; absent and explicit-zero
  subaccounts are one identity across primary uniqueness, batch targets and references.
  Typed `Entity::insert()` inputs distinguish omission from explicit values and
  enter the ordinary batch through `insert_input`. Integer, Bool and bounded Text
  literals may declare `field(..., default = ...)`; optional defaults supply a
  present value, while explicit `None` remains absent. Required fields without a
  default refuse omission. Defaults belong to schema identity and never run on
  replacement, decode or reopen. Primary-key/nested-record defaults, versioned
  producers remain separate work; no SQL mutation lane. Nonoptional Timestamp
  fields may use `managed = "created_at"` or `"updated_at"`, once per role.
  `Batch::at(time)` carries one host-captured time, without reading a clock.
  Managed insert setters are absent; fully authored inserts refuse. Replacements
  must echo stored timestamps, preserve creation, and update time only for real
  changes. No-ops retain time even with an earlier clock, but inverted stored
  timestamp order refuses. Changed policies enter schema identity; decode/reopen
  never assign timestamps. Keep this in the existing bounded commit path.
  Decimal fields retain an explicit declared scale (0–28), as confirmed by the
  maintainer. Excess precision and fixed-scale coefficient overflow refuse;
  storage never silently rounds. Aggregate arithmetic has its own contract.
  Start codec work from IcyDB's database-owned implementations and regression
  tests; adapt them to this compiled-schema contract instead of using its Wire
  paths. Explain departures and qualify the resulting bytes independently.
- Feature deferrals in the design are proposals. They do not authorize removing
  capabilities from IcyDB or claiming full Toko adoption.
- Keep one current system design and executable examples as implementation grows.
  For the baseline's changelog quick starts, use real `db` commands and Canic's
  presentation. State fixture/deployment scope; do not imply proposed commands
  or product capabilities are implemented.
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
`make m1-write-envelope` separately selects maximum row bytes, batch cardinality
and text-index fan-out in real Wasm. Keep its raw-byte inspection and synthetic
schema in qualification; it does not add a product query API or establish every
valid workload's instruction/memory envelope.
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
`make m1-entity-growth` separately measures 1/8/32 equivalent scalar entities
with one fixed endpoint and reachable typed writes/lookups/pages/index reads.
It compares fresh construction with a retained host/runtime and cloned immutable
Schema. Every Database open still checks current durable authority; revision
validation is operation-specific. Generated schema assembly retains only immutable
executable metadata, and the local M1 host retains supplier runtime custody.
Retain cold/post-upgrade and warm costs in the report. The growth fixture is
update-only; the normal record witness separately checks cold queries after
upgrade followed by updates. Heterogeneous growth, fluent chains and Canic
integration remain separate qualification.

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
`make toko-tooling-check` verifies the snapshot and matching tooling declarations
and locks in both product and qualification workspaces. Tool preparation is explicit:
`make install-tools`, then `make tools-check`. Hooks use `make install-hooks`.
Use `toko-cloc` for source counts; `make cloc` and `CLOC_*` settings are retired.
