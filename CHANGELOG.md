# Changelog

## [0.18.0] - Unreleased

### ▶️ Try it

From this checkout, prepare PocketIC once if needed:

```sh
make m0-setup
```

Install the updated console and run the disposable local demonstration:

```sh
make db
db migration-demo
db query Entry --order-by amount --desc --limit 2 --work 512 --select id,amount
db query Entry --order-by category:asc,amount:desc --limit 3 --select id,amount
db query Entry --order-by category:desc --select category --distinct --limit 1
db query Entry --select category,count --group-order key-desc --limit 1
db aggregate Entry --select category,count --group-order key-desc --page-size 1
```

Expect six source rows, bounded copy/validation/cleanup progress, a real local
canister upgrade during copying, and six target rows read through the new label
index. All data is temporary; no deployed canister or GitHub credentials are used.
The query scans all six entries and returns IDs 6 and 5 by descending amount,
with no continuation, using the engine's bounded complete sort.
The mixed-direction query returns IDs 6, 4 and 2, also with no continuation.
The sorted DISTINCT query removes duplicate categories before selecting category 1.
Descending group keys select category 1 first (count 3); the retained command
then returns category 0 (count 3) on its next completed-output page.

Decimal projections are also available. The CLI seeds values 1.25, 2.50 and
-0.75, with all optional `maybe` values absent:

```sh
db query DecimalRow --order-by value:desc --select value --distinct
db query DecimalRow --order-by maybe:desc --select maybe --distinct
```

### ➕ Added

- Add `GroupOrder::KeyDescending` and CLI `--group-order key-desc` for synchronous
  and retained grouped results. Reverse completed canonical keys before output
  selection, sharing the existing bounded buffer. Existing order bindings stay
  unchanged; native and Wasm checks cover resume refusal and upgrade pagination.
  Exhaustive Rust matches on `GroupOrder` must add the new variant.

- Let unindexed ordered queries scan beyond 32 matches while retaining only the
  best requested rows. Keep the 512-candidate work cap, 128 KiB retained-buffer
  cap and complete-input requirement.

- Expose DecimalRow's `value` and optional `maybe` projections in the console,
  including sorting and DISTINCT through the existing fluent engine. Qualify
  exact decimal extremes, equivalent scales, absence and bounded refusal across upgrade.

- Compose bounded sorting with DISTINCT. Admit identity results or an order
  covering exactly the projected fields. Deduplicate during scanning using the
  existing sort keys, then sort and apply output limits. Non-identity projections
  retain at most 32 distinct tuples, so repeated input can exceed 32 rows within
  the work budget. A 33rd distinct tuple refuses even with `LIMIT 1`; no second
  key set, cursor or durable state is added.

- Add fluent `order_by_each` with per-field `SortDirection` and CLI `:asc`/`:desc`
  suffixes. Keep the implicit primary tie-breaker ascending; `.descending()` or
  `--desc` reverses the whole order. Prove one physical traversal direction after
  removing fixed fields, or reuse the bounded complete sorter. Preserve ordinary
  cursor identities and bind new directions explicitly; no stored-format changes.

- Support `order_by` without a matching ordered path by scanning the
  complete input result with 4096-byte keys and 128 KiB retained
  key/output buffers. Apply result limits after sorting; work or resource exhaustion
  refuses without partial output. Share the existing scanner, key codec and typed
  projection. No cursor or explicit-index fallback; indexed pages retain
  their current behavior and bindings.
- Add `db migration-demo`: a fixed compiled `Item` transformation widens `amount`
  from Nat64 to Nat128 and adds an indexed label. Resume from product checkpoints
  after upgrade, validate before publication and clean the retired source. Keep
  the usual query fixture separate. The console adds no product API, durable
  progress, runtime DDL, planner or fault controls.
- Add `make m3-cli` for the installed command and its real Wasm admission/recovery
  checks, including premature publication, source closure and cleanup retries.

### ⚡ Performance

- Add `make m2-fluent-comparison`: both engines now execute typed fluent queries
  over the same 32-row fixture. Six successful first-page cases use 64.8–73.7%
  fewer IC DB instructions; tuple/order and unchanged-backing checks pass across
  upgrade. Record legacy public-lane sort refusal and differing pagination limits
  separately, without counting refused work as a speed comparison. Update the
  [comparison matrix](qualification/README.md#comparison-with-frozen-icydb).

- Refresh the matched scalar comparison against frozen, SQL-disabled IcyDB.
  Current IC DB uses 233,238 versus 3,087,364 raw Wasm bytes in this direct-record
  fixture, 57.0% fewer hit-lookup instructions and 63.0% fewer complete replacement
  cycles including legacy settling. All row/index and upgrade checks pass.
  Record IC DB's +8,690 raw bytes and roughly 0.1% lookup/replacement instruction
  increase since the prior run; this is not a whole-product size comparison.

- Add `make m2-sort-costs` using the existing independent sorting oracle and a
  measurement-only host wrapper. Qualify 511-row sorting, DISTINCT, sparse/empty
  matches, resource refusals and upgrades with unchanged backing. Mixed-order
  selection of 32 rows uses about 13.86M instructions in this scalar fixture;
  this has no matched IcyDB result. The
  [comparison matrix](qualification/README.md#comparison-with-frozen-icydb)
  separates recorded legacy comparisons, current capability and remaining gaps.

- Avoid redundant equality checks during order admission. The matched fluent
  benchmark recovers 913–1,073 instructions on affected ordered reads for 21
  additional shared Wasm/code bytes. Recent query capabilities add about 2.2 KB
  overall while per-entity growth remains about 4.30 KB; see the
  [measured costs and remaining tradeoffs](qualification/README.md#expanded-sorting-costs-and-direction-admission-018).

- Share projection decoding by output record type. The matched eight-entity
  projection build loses 3,688 raw / 3,686 code bytes; projected growth falls
  from 5.14 KB to 4.61 KB per entity. Eight-entity projected calls improve;
  one-entity calls add 20–203 instructions/cycles. Reuse the admitted entity ID
  for revision reads, saving a duplicate membership lookup at a 35-byte raw/code
  cost in the eight-entity build. Result metadata, codecs and public APIs stay
  unchanged; ordinary entity-query growth remains about 4.30 KB.
- Reuse the descriptor returned by schema admission at typed direct-read
  boundaries. Combined reachable growth falls from 5.79 KB to 4.29 KB per added
  entity; direct growth falls to 2.79 KB. The eight-entity build loses 9,984 raw /
  9,988 code bytes, while the one-entity eight-shape build adds 491 / 489 bytes.
  Sampled reads use fewer work instructions; retain up to 98 extra instructions
  on first inserts and 44 on a read/replace pair. Total cycles do not improve
  for every subject. No new cache, schema representation, codec or public API.
- Move fluent predicate/index/order construction onto the existing concrete request
  and share fetch/projection admission, execution and cleanup before typed decoding.
  The eight-entity composition build loses 3,961 raw / 3,948 code bytes; incremental
  entity growth falls from 7.77 KB to 7.13 KB. Sampled fluent operation instructions
  and cycles improve. The one-entity eight-shape build adds 509 raw / 517 code bytes
  for the shared boundary. The roughly 2 KB objective / 4 KB working ceiling remains
  unmet. No API, schema, persisted-state or continuation-format changes.
- Share direct primary/index traversal and index-prefix admission across entities,
  resolving key types once per operation. Combined growth falls further to
  6.31 KB per entity; the direct-path control falls to 4.33 KB. The eight-entity
  fluent build loses another 4,989 raw / 4,992 code bytes, with a 741 / 738-byte
  fixed-cost increase in the one-entity eight-shape build. Sampled direct reads
  use fewer instructions/cycles. Keep one-row traversal, typed decoding and all
  ownership/index checks; no cache, extra page buffer or stored-format change.
- Share the existing typed page-conversion loop across callers. Combined growth
  falls further to 5.79 KB per entity; the eight-entity fluent build loses another
  3,642 raw / 3,643 code bytes. Accept 18 extra instructions in sampled multi-query
  reads (201 in the single-query build) for that reduction. Measured direct reads,
  inserts and read/replace costs stay unchanged. Keep codecs, page semantics and
  maintenance source decoding intact; discard ineffective outlining probes.

### 🔧 Qualification

- Adopt Toko Tooling 0.44.5 in both workspaces through its published installer.
  Synchronize exact pins, lockfiles and the authenticated snapshot receipt;
  installed tooling payloads and database behavior remain unchanged.
- Point the frozen comparison dependency at private `dragginzgame/icydb-legacy`
  after its repository rename. Retain revision `6d421e3` and the same baseline
  source; repository access now requires authentication.
- Qualify cursor/job refusal after publication for an unchanged entity with the
  same rows and revision. Native and Wasm checks cover running/completed jobs, unchanged
  backing on refusal, retired-source closure, independent cleanup and fresh target
  jobs. The Wasm case adds publication-trap rollback and upgrades across copying,
  publication and cleanup. Reuse existing identity checks; add no product state
  or coordinator.
- Adopt direct `ic-host-artifacts` 0.80.1 bounded reads and byte identities.
  Replace its removed Wasm-inspection feature with a small benchmark-only
  `wasmparser` reader, preserving measurement definitions and limits and adding
  complete-module validation. Keep Testkit's transitive 0.79 dependencies until
  its separate migration; product dependencies and durable formats stay unchanged.
- Add `make m3-jobs`: combine existing retained-aggregate and maintenance fixtures.
  Check frozen read refusal, independent cancellation/expiry cleanup under an
  incompatible maintenance executable, rollback of maintenance cancellation,
  unchanged-source resumption and stale-handle refusal after slot reuse. No new
  product API or durable state; cutover qualification adds one private fixture endpoint.
- Add `make m3-references`: migrate 256 rows with 128 reference pairs across
  copy chunks and upgrades. Check literal row/index bytes, checkpoint rollback,
  delayed dangling-reference refusal, cancellation, publication and final-batch
  restricted deletion. After publication, check 254 references to one target,
  refusal beyond a deleted prefix and bounded explicit deletion across upgrade.
  Record phase instructions using existing product mechanisms; keep fixture
  controls out of normal builds.
- Add `make m2-query-growth`: measure the first fluent query, eight query shapes,
  eight application mapping branches and eight equivalent entities with one fixed
  endpoint. Check actual results and upgrade behavior, retain Wasm/code bytes and
  IC instructions/cycles, and keep application growth separate from engine claims.
- Record first/eighth insert costs and a read followed by replacement in that same
  workload, after query samples, to compare write costs against preserved artifacts.
- Add the same-build eight-entity direct-path control: approximately 5.16 KB per
  added entity before fluent queries, plus 2.61 KB associated with fluent
  reachability before the boundary reduction above. Record that initial 7.77 KB
  as an unresolved gap against the
  maintainer's roughly 2 KB objective / 4 KB working comparison ceiling.
- Add `make m2-contention`: exercise the product retained sum over 96 rows with
  controlled writes between advances. Cover unrelated writes, no-ops, trapped and
  refused writes, repeated source invalidation, value restoration across upgrade,
  explicit cancellation and completion after writes stop. Reuse existing host
  endpoints; add no product state or automatic retry policy. These deterministic
  schedules do not establish completion rates under production traffic.

### 🧹 Cleanup

- Reconcile the system design with delivered M1–M3 capabilities and explicit gaps.
  Separate current contracts from dated probes/benchmarks; remove stale prerequisite
  wording and numeric index examples without changing runtime behavior.
- Put the query test entrypoint beside its child modules, using ordinary Rust
  discovery instead of a source-path override. Keep the `queries` test target.

## [0.17.0] - 2026-11-15

### ➕ Added

- Extend maintenance to whole-database copy and cutover. Preserve the named entity
  set and primary keys; select typed transforms through `next_entity()` and copy
  unchanged entities explicitly with `copy_unchanged::<E>`. Checkpoint progress
  across entities, rebuild all indexes and validate references against the complete
  target before publication, including cycles. Ordinary writes retain their
  reference checks. Entity additions/removals, key remapping and selective copying
  remain unsupported. No DDL or runtime migration planner; no new CLI command.
- Preserve managed creation/update timestamps during bounded schema copying,
  including explicit field renames and rebuilt timestamp indexes. Refuse changed
  values or added/removed managed roles before mutation; final validation also
  checks preservation. Normal write policies remain active after publication.
  No new schema attributes, persisted format or CLI command.
- Add bounded `RetiredSource` cleanup after cutover, admitted against the exact
  transition and source/target incarnations. Preserve the retired manifest and
  keep source access closed; cleanup never opens or modifies the live target.
  Interrupted deletion resumes without a new cursor, phase or stored format.
  Completed cleanup is a no-op on retry; physical shrink and allocation reuse
  remain outside this contract. No new CLI command yet.

### 🔧 Qualification

- Add a migration capacity probe with 128 four-KiB rows, eight target indexes and
  512-byte index keys. Reject actual supplier growth, compare complete backing and
  retry the identical chunk after upgrade with growth enabled. Check structural
  refusals, independent row/index bytes and completion of bounded validation and
  cleanup. Reuse the existing fault wrapper and write-envelope oracle; no product
  fault controls or resource framework. Included in `make m3-maintenance`.
- Exercise two successive whole-database migrations in Wasm, with ordinary writes
  between them and a new field/index in the second target. Check interruption,
  changed-executable refusal, stale migration operations, cancellation back to the
  latest source and cleanup of both retired sources. Record physical backing at
  publication and cleanup. This uses fresh allocations and adds no product state,
  allocation reuse or migration planner.

## [0.16.0] - 2026-11-15

### ➕ Added

- Begin M3 with a durable source freeze: `SchemaTransition` binds source/target
  contracts, transformation name/version and fixed inputs. `begin_maintenance`
  consumes the live handle; `Maintenance::resume` requires the exact identity;
  pre-copy `cancel` restores source admission. A bounded manifest record closes
  ordinary access under current and 0.15 executables. Rows, indexes and revisions
  stay intact; message traps roll back freeze and cancellation.
- Add bounded typed `Maintenance::source_page` reads through an explicit compiled
  source schema. Reuse query traversal and byte-limit retries; bind continuations
  to the transition, database, entity and revision. Upgrades can resume paging,
  while cancellation followed by source edits invalidates old cursors. No new
  durable state or copy checkpoint is introduced.
- Add `make m3-maintenance` with two executable transition versions and optional
  recorded-predecessor qualification. No new CLI command yet.
- Add `SchemaCopy` for a single key-preserving entity transformation into a fresh
  host allocation. Bind source and target ownership, rebuild indexes through ordinary
  batch commits and checkpoint each bounded chunk in the same IC message. Refuse
  changed transformations and cross-chunk uniqueness collisions without advancing.
  Bounded cancellation removes target entries before restoring source service;
  keep a closed ownership tombstone. Multi-entity/reference/managed-time transforms
  and allocation reuse remain later work.
- Add resumable validation of source/target key coverage, complete rows, revisions
  and both expected and stored index entries. `SchemaCopy::publish` requires both
  passes to finish, then atomically opens the target and retires source admission.
  Target writes and reopen work normally afterward; cancellation cannot revive the
  old source. Retired-source cleanup remains later work.

### 🧹 Cleanup

- Reconcile six directory-owning modules with the adopted `name/mod.rs` layout.
  Preserve their Rust paths, public APIs and stored contracts.

### 🔧 Qualification

- Add the actual M3 migration, validation, cutover and rollback probes to the
  source-release gate. Keep all crates unpublished.
- Registry review confirms the selected Platform, Storage, Host, Testkit, Wire
  and Toko Tooling versions remain current. Keep the frozen IcyDB comparison
  unchanged. Reconfirm the optional IC Build 0.84.1 → 0.86.0 editor refresh for
  the next dependency-maintenance batch; it is not required by this release.

## [0.15.0] - 2026-11-15

### ▶️ Try it

```sh
make db
db schema Entry
db query Entry --limit 2
db query Entry --select category --distinct
db query Entry --select category --distinct --min 40 --order-by category
db query Entry --select exists --work 1 --bytes 1
db query Entry --select exists --min 999 --work 7
db aggregate Entry --select category,count \
  --work 1 --page-size 1
```

These commands use the disposable local fixture with the updated Wire transport.
The existence examples return `true` after one candidate and `false` after a
completed six-row scan. A work budget of six for the latter query refuses with
`Limit`, because completion has not yet been observed.
The DISTINCT examples return complete category sets without a continuation;
the first preserves first-match ID order (`1`, `0`), the second category order
(`0`, `1`). A fallback query with `--limit 1` refuses rather than truncating.

### 🔧 Dependencies

- Release review confirms the locked Platform, Storage, Host, Testkit, Wire
  0.24.1 and Toko Tooling 0.44.4 selections match current stable registry versions.
  Defer the optional release-editor IC Build 0.84.1 → 0.86.0 refresh to the next
  dependency-maintenance pass. The frozen IcyDB comparison retains its transitive
  IC Entropy 0.2.0 and Wire 0.22.4; revisiting those belongs to a separately reviewed
  baseline refresh, not a change to the comparison during this release.
- Adopt Toko Tooling 0.44.4 through its published snapshot installer in both
  workspaces. Rename qualification directories to their existing Cargo package
  names and update workspace paths and documentation; package identities stay
  unchanged.
- Upgrade qualification transport to IC Wire 0.24.0 and migrate callers to its
  root-level APIs with explicit payload/request/response encoding. Preserve
  fixture method IDs and resource bounds without a legacy API wrapper. The
  frozen IcyDB comparison still brings Wire 0.22.4 transitively; the IC DB product
  remains independent of Wire.

### ➕ Added

- Add bounded complete DISTINCT projections when leading-index adjacency cannot
  be proven. Reuse the scanner, exact aggregate tuple equality and projection
  copying; preserve first-match traversal order and validate examined duplicates.
  Return one complete page without a cursor, capped at 32 tuples, 4096-byte keys
  and 128 KiB of key/row buffers. Work, result-count and combined byte exhaustion
  return `Limit`; an oversized single row returns `RowTooLarge`. Existing live
  indexed DISTINCT stays unchanged. No durable state, replay or new CLI flag.
- Seek mandatory `StartsWith` filters on the first secondary-index field after
  its fixed prefix. Reuse byte-range bounds for exact UTF-8, embedded NULs, empty
  prefixes and optional presence. Preserve DISTINCT, retained aggregation and
  upgrade continuation without changing stored bytes or query identities.
  Oversized derived bounds stay residual; `EndsWith` and `Contains` still scan.
  Available through the existing fluent API; no new CLI flag.
- Extend range seeks to the first secondary-index field after its admitted fixed
  prefix. Bound complete equal-value classes, exclude optional absence from range
  comparisons and retain forward/reverse byte retries. Oversized derived bounds
  remain residual filters instead of introducing new refusals. Pages, DISTINCT and
  aggregates share execution; cursor/job identities and stored bytes are unchanged.
  Available through existing fluent filters and index/order selection; no new CLI flag.
- Seek directly to the intersection of mandatory primary-key equality/range
  filters on primary traversal. Pages and aggregates share inclusive/exclusive
  bounds, empty intersections and cursor clamping; rows outside the range no
  longer consume candidate budget. Preserve existing cursor/job identities and
  byte-limited retries. No new schema attribute, durable format or CLI flag;
  use the fluent `filter` API. OR alternatives remain residual.
- Add synchronous fluent `exists()` and `db query ... --select exists`. Return
  true after the first validated match, without entity materialization; return
  false only after exhausting the selected input. Work exhaustion without a match
  refuses with `Limit`. Reuse ordinary query admission and traversal, report one
  output byte, and keep grouped/retained state formats unchanged.
- Add `filter_groups_any` for nonempty OR clauses over completed aggregate values,
  ANDed with other group filters under their existing eight-comparison budget.
  Validate all operands and finalize every group before selection, so OR cannot
  hide overflow or relax resource bounds. Synchronous and retained groups share
  execution; real OR clauses bind retained jobs while singleton clauses preserve
  existing conjunction identity. No query AST or durable format is added.
- Extend `Eq`/`Ne` with `IndexValue::Absent` to every optional stored field,
  including blobs, lists, records, payload enums, sets and maps. Empty values
  remain present. Row filters, OR clauses and retained aggregates reuse validated
  decoder spans and existing comparison bindings; no new predicate or stored format.
  Required fields still reject absent operands, and unsupported value comparisons
  remain refused. Available through the fluent API.
- Allow sparse indexes to serve fluent queries when filters prove every optional
  indexed field present, including ranges, text matches and non-null membership
  lists. Every OR alternative in a proving clause must reject absence on the
  same field. Automatic and explicit paths share this check; presence never
  substitutes for equality in ordering or DISTINCT proofs. No storage changes.
  A newly selected physical path invalidates continuations bound to the old path.
- Add `Comparison::Contains` for literal text substring matching in fluent row
  and completed text min/max group filters, including retained jobs. It shares
  prefix/suffix admission, absence rules and scan bounds; collection membership,
  case folding and pattern syntax are not implied. Feature-free `memchr` 2.8.3
  supplies linear-time, constant-space search over existing comparison keys.
  No durable format changes. Exhaustive comparison matches need the new variant.
- Add `Comparison::EndsWith` for exact, case-sensitive text suffix filters on
  rows and completed text min/max groups, including retained jobs. Empty suffixes
  match all present text; absence never matches. Matching shares text admission,
  existing keys and bounded scans, with no new stored format or index-seek path.
  Exhaustive matches on `Comparison` must handle the new variant. Available
  through the fluent Rust API; no new CLI flag.
- Add fluent `filter_not_in(field, values)` for bounded exact-value exclusion.
  It expands to ordinary inequality filters and shares their cursor/job identity,
  admission and execution. Absent fields match unless absence is explicitly
  excluded. Lists must be nonempty; duplicates count toward the shared
  eight-comparison budget. No new executor, stored format or CLI flag is added.

## [0.14.0] - 2026-11-15

### ▶️ Try it

```sh
make db
db query Document --select entry_id,min:note
db query Document --select entry_id,min:note --group-min alpha
db aggregate Document --select entry_id,max:note --group-max zèbre --work 1
```

The local fixture's note is absent: the first command shows its group, while
the text bounds exclude it. Each invocation opens a fresh disposable fixture.

### ⚠️ Breaking

- Schema declarations now consistently reject `where` clauses. Record, enum,
  set and map macros previously discarded these constraints silently; entity
  declarations already refused them. Remove such clauses from declarations
  and enforce any intended constraint in ordinary Rust code. Supported empty
  struct declarations and durable schema identities are unchanged.

### ➕ Added

- Add `Comparison::StartsWith` to fluent row filters and completed text min/max
  group filters, including retained jobs. Matching is literal and case-sensitive;
  empty prefixes match present text, while absent values never match. Reuse the
  existing comparison keys and bounds without changing stored formats or adding
  prefix index seeks. Consumers exhaustively matching `Comparison` must handle
  the new variant. This increment is available through Rust, not a new CLI flag.
- Add fluent `filter_any` for OR clauses and `filter_in` for exact-value membership,
  combined with ordinary AND filters. Membership uses the same equality clause;
  duplicate members consume budget but never duplicate rows, and optional absence
  requires an explicit absent member. Lists must be nonempty.
  All comparisons share the eight-term budget and exact type admission, including
  alternatives that would otherwise short-circuit. Reads and aggregates use one
  scan; existing conjunction cursors retain their identity. No nested expression
  tree, index union or additional stored query state is introduced.
- Both CLI query modes accept literal text bounds for grouped note min/max.
  Comparisons preserve empty strings and UTF-8 byte order, without normalization
  or special absence strings. Bounds remain limited to 64 bytes and use the
  product's existing group selection and retained-job identity. Empty or
  whitespace-containing bounds use one-shot shell commands.

### 🧹 Cleanup

- Reconcile the README, design and agent instructions with delivered retained
  grouping, completed-output pagination and Decimal arithmetic. Clarify the
  local console's entity selection and separate its fixture selectors from the
  product's fluent capabilities. Preserve historical qualification limits.
- Share declaration-shape validation across the five schema macros and correct
  the qualification guide's installed-console command list and transport reference.

### 🔧 Dependencies

- Release review confirms the selected Platform Types 0.20.4, Platform Runtime
  0.22.5, Storage 0.74.1, Host Artifacts 0.79.0, Host Text 0.76.0, Testkit 0.106.0
  and Toko Tooling 0.44.3 are current stable selections. Defer optional Wire
  0.22.4 → 0.24.0 and release-tool IC Build 0.84.1 → 0.86.0 updates to the next
  dependency-maintenance pass. Wire remains confined to qualification transport;
  the product dependency graph and durable codecs do not use it.

## [0.13.0] - 2026-11-15 (development checkpoint)

### ▶️ Try it

```sh
make db
db aggregate Document --select entry_id,sum:score --group-min -10 --work 1
db aggregate DecimalRow --select zone,avg:value --group-min 0.75 --work 1
db query Entry --select category,avg:amount --group-min 31.50
db aggregate Entry --select category,sum:amount --group-min 93 --group-max 120 --page-size 1
db query DecimalRow --select zone,avg:value --group-min 0.75 --group-max 1.00
db aggregate DecimalRow --select min:value --work 1
db aggregate DecimalRow --select zone,max:value --group-min 2.50 --group-max 2.50 --work 1
db query DecimalRow --select zone,min:maybe
db aggregate DecimalRow --select count:maybe --work 1
db aggregate DecimalRow --select zone,distinct-count:maybe --work 1
db query DecimalRow --select distinct-count:value
```

### 🛠️ Changes

- Parse CLI group thresholds in the selected aggregate's exact result type,
  shared by synchronous and retained queries. Signed sums accept negative
  thresholds; Decimal results accept fractions within their declared scale;
  widened integer sums accept their full result domain. Integer fractions,
  overflow, excess Decimal precision and threshold text above 64 bytes refuse.
  Reuse existing parsers and engine admission without changing durable formats.
- Add `--group-max` to synchronous and retained CLI queries. Either bound may
  be used alone; together they form an inclusive range on completed aggregate
  values before ordering, total limits and paging. Share typed parsing for both
  bounds and compose existing engine predicates. Valid inverted ranges return
  no groups; malformed operands still refuse. No local result filtering is added.
- Expose Decimal min/max in both CLI query modes for required `value` and
  optional `maybe`, with optional `zone` grouping. Keep exact Decimal operands
  for group bounds, empty scalar results for all-absent input, and absent grouped
  results. Share existing result records and formatting with sum/average;
  product reducers, codecs and job formats remain unchanged.
- Add Decimal field and DISTINCT counts to both CLI modes, optionally grouped
  by `zone`. Retain zero-count groups and one DISTINCT absent value; count
  thresholds remain Nat64 rather than Decimal. Reuse existing projection records,
  presence checks and bounded key sets. Qualification checks equivalent Decimal
  spellings and signed zero across upgrade, without changing storage semantics.

## [0.12.0] - 2026-11-15 (development checkpoint)

### ▶️ Try it

```sh
make db
db aggregate Entry --select category,min:amount --work 2 --page-size 1
db aggregate Entry --select category,max:amount --work 2 --page-size 1
db aggregate Entry --select avg:amount --work 2
db aggregate Entry --select category,sum:amount --min 30 --group-order asc --limit 1 --page-size 1
db aggregate Entry --select category,avg:amount --group-min 35 --group-order desc
db aggregate Document --select count:note --work 1
db aggregate Document --select distinct-count:note --work 1
db aggregate Document --select entry_id,min:note --work 1
db aggregate Document --select entry_id,sum:score --entry-id 1 --work 1
db aggregate DecimalRow --select sum:value --work 1
db aggregate DecimalRow --select zone,avg:value --work 1 --page-size 1
db aggregate DecimalRow --select zone,avg:maybe --work 1
```

### 🛠️ Changes

- Add a local retained-aggregate CLI journey: show progress and completed pages,
  then cancel the temporary job. The fixture uses fluent product jobs; no client
  reducer, fault controls or production endpoint generation is added.
- Add retained CLI input filters (`--min`, `--category`) and completed-group
  filtering/order/total limits (`--group-min`, `--group-order`, `--limit`). Keep
  total selection separate from per-response `--page-size`. Reconstruct the same
  admitted query on every advance and reuse synchronous CLI type conversion;
  no client filtering, sorting or partial aggregation.
- Extend retained CLI queries to Document: row/present-field counts, DISTINCT
  notes, note/priority extrema and priority/score sums, optionally grouped by
  `entry_id`. Share input-filter validation with synchronous queries. The fixture's
  absent note counts as zero present values and one DISTINCT value; grouped
  extrema display absence as `—`, while present zero remains `0`. Reuse existing
  product terminals and result records without changing engine or stored formats.
- Add retained DecimalRow counts and sum/average selectors for `value` and
  optional `maybe`, optionally grouped by `zone`. Reuse fixed-scale Decimal
  result records, exact product arithmetic and completed-group selection. The
  fixture contains 1.25, 2.50 and -0.75, with all `maybe` values absent. No new
  codec, accumulator or job format is introduced.

## [0.11.0] - 2026-11-15 (development checkpoint)

### 📝 Summary

Complete retained field/DISTINCT counts and scalar/grouped extrema, with bounded
pagination over completed grouped results. Share checkpoint validation, reducers
and finalization; retain existing schema/row formats and earlier job identities.

### ▶️ Try it

```sh
db query Document --select count:note
db query Entry --select category,distinct-count:category
db query Entry --select min:amount
db query Entry --select max:amount
db query Entry --select category,min:amount
db query Entry --select category,max:amount
make m2-jobs
```

The CLI demonstrates the existing synchronous count and extrema semantics; the
qualification command exercises the new retained APIs and upgrade recovery.

### 🛠️ Changes

- Add retained grouped `start_group_min`/`resume_group_min` and matching maximum
  terminals. Preserve all-absent groups, exact types and overlapping key/operand
  projections. Share winner validation with scalar extrema and start/resume/output
  paging with grouped numeric queries. Apply filtering/order/limit only after full
  accumulation; group/frame refusals preserve progress even for excluded output.
  Qualify rollback, upgrade, invalidation and historical completed-page retries.
- Add `start_min::<Record>`/`resume_min::<Record>` and matching maximum terminals.
  Retain one exact scalar winner with the existing comparator, scanner, revision
  invalidation and retry lifecycle. Persist only winner bytes; validate them and
  reconstruct comparison keys on resume. Empty/all-absent input returns `None`.
  State/output refusals preserve progress; qualify Wasm rollback and upgrade.
  Existing job identities/payloads and row/schema formats stay unchanged.
- Add retained scalar/grouped present-field counts through the existing job
  lifecycle. Count present values of any stored top-level type, including empty
  values, and retain groups whose field count is zero. Bind the selected field
  without changing existing row-count or numeric job identities and payloads.
- Add `start_count_distinct::<Record>`/`resume_count_distinct::<Record>` with
  exact tuple equality across calls, including absent tuples. Persist at most 32
  comparison keys within the existing 4-KiB job frame; validate saved keys and
  reject duplicates/malformed frames intact. Reuse DISTINCT admission, reducer,
  scanner and job lifecycle. Oversized state refuses without losing progress.
  CLI selectors remain synchronous.
- Add `start_group_count_distinct::<KeyRecord, Record>` and its resume terminal.
  Keep distinct tuples per group across calls, with absent-tuple inclusion and
  overlapping projections. Finish input before group selection. Enforce 32 groups,
  32 keys per group and the shared 4-KiB job frame independently. Scalar/grouped
  checkpoints share key-set validation; existing payloads and identities remain
  valid. Qualify independent-set results, refusal preservation and Wasm recovery.
- Return `GroupPage { rows, next }` from retained grouped resume terminals, replacing
  their vector result. Add `.group_page(size, after)` for bounded pages over completed
  counts, field/DISTINCT counts, sums, averages and extrema. Keep total `limit` and group
  selection bound to the job; page budgets may change. Retry unreturned groups on
  byte exhaustion. Validate cursors against job identity and completed output;
  expiry/cancellation still apply. No new durable fields, collections or formats.

## [0.10.0] - 2026-11-15 (development checkpoint)

### 📝 Summary

Add bounded retained scalar/grouped counts, sums and averages with atomic progress,
revision invalidation, retries and upgrade recovery. Extend synchronous grouped
DISTINCT counts and CLI examples. Retain one scanner, exact reducers and grouped
finalization; no schema, row-format or endpoint-generation changes.

### ▶️ Try it

```sh
make db
db query Entry --select category,distinct-count:amount
db query Entry --select category,distinct-count:category
db query Document --select entry_id,distinct-count:note
make m2-jobs
```

Each category has three distinct amounts and one distinct category value. The
document's absent note counts as one distinct tuple, unlike present-field count.
`make m2-jobs` qualifies retained aggregate APIs; CLI selectors remain synchronous.

### 🛠️ Changes

- Add `.group_count_distinct::<KeyRecord, Record>()` with absent-tuple inclusion,
  overlapping projections, completed-count filtering/order and independent bounds
  on groups, distinct tuples and retained bytes. Scalar and grouped distinct
  counts share one comparison-key set reducer; scalar counts no longer retain
  duplicate projected-record bytes. No durable state or schema changes.
- Add retained `start_count`/`resume_count` and `start_sum`/`resume_sum` with
  explicit progress/completion, per-entity revision invalidation, sequence-safe
  retries and same-code upgrade recovery. Reuse the existing scanner, exact
  integer/Decimal reducers and owned codec. One separate host-owned allocation
  holds at most 16 jobs; candidate, byte, lifetime and cleanup limits are explicit.
  Add `make m2-jobs` for native and Wasm rollback qualification. This adds job
  state without changing the database manifest, row codec or schema attributes;
  retained jobs are not yet exposed by the CLI.
- Add `start_avg::<Record>`/`resume_avg::<Record>` over the same retained total
  and present count. Integer/Decimal averages round once at completion to the
  declared Decimal scale with ties to even. Bind average separately from sum and
  preserve existing count/sum job identities. Reuse numeric admission, job
  preparation, result conversion and storage without a new format or allocation.
  Qualify uneven chunks, absence, large cancelling totals, overflow, retry and
  upgrade recovery. CLI average commands remain synchronous.
- Consolidate numeric query admission across synchronous, grouped and retained
  terminals. Reuse the cursor codec's frame bound for job records, remove repeated
  test-store setup and consolidate duplicated job guidance. Public behavior and
  stored formats are unchanged by this cleanup.
- Add `start_group_count::<KeyRecord>`/`resume_group_count::<KeyRecord>` with
  exact multi-field/absent keys, completed-group filtering/order and atomic retries.
  Reuse the group table, scanner, output conversion and job lifecycle. Retain at
  most 32 groups within the existing 4-KiB whole-job cap; oversized state refuses
  without losing progress, regardless of output filters/limits. Store key records
  and counts, reconstruct comparison keys, and preserve scalar checkpoint bytes.
  Qualify prior-scalar-executable upgrades and grouped rollback/reopening.
- Add `start_group_sum`/`resume_group_sum` and `start_group_avg`/`resume_group_avg`
  with exact per-group totals/present counts and completed `GroupValue` results.
  Preserve absent groups, round averages once, and refuse overflow even in groups
  excluded by output selection. Share numeric checkpoints and grouped result
  conversion; retain existing count-job bytes and identities. The same 32-group/
  4-KiB frame limits apply, without new collections, dependencies or schema changes.
  Qualify arithmetic, retries, refusal preservation and Wasm rollback/upgrades.
  Retained DISTINCT jobs and completed-group paging remain later work.

## [0.9.0] - 2026-11-15 (development checkpoint)

### 📝 Summary

Add bounded scalar/grouped extrema, sums and averages, with completed-group
filtering and ordering. Integer and Decimal arithmetic share exact accumulation;
averages use the declared result scale and ties-to-even rounding. Extend the
local CLI demos and keep the execution, grouping and codec paths shared.

### ▶️ Try it

```sh
make db
db query Entry --select avg:amount
db query Entry --select category,avg:amount --group-order desc
db query DecimalRow --select avg:value
db query DecimalRow --select zone,avg:maybe
db query DecimalRow
db query DecimalRow --select sum:value
db query DecimalRow --select zone,sum:value --group-min 3
db query DecimalRow --select zone,sum:maybe
db query Entry --select sum:amount
db query Entry --select category,sum:amount --group-order desc
db query Entry --select category,sum:amount --group-min 100 --limit 1
db query Document --select entry_id,sum:score
db query Document --select sum:priority
db query Document --select sum:score
db query Entry --select min:amount
db query Entry --select max:amount --min 40
db query Document --select min:note
db query Document --select min:priority
db query Entry --select category,min:amount
db query Entry --select category,max:amount --limit 1
db query Document --select entry_id,min:note
db query Document --select entry_id,max:priority
db query Entry --select category,min:amount --group-order asc --limit 1
db query Entry --select category,count --min 40 --group-min 2 --group-order desc
```

The local fixture returns total amount 213, priority sum 0 and score sum -10.
Average amount is 35.5; category averages are 0 = 40 and 1 = 31. The decimal
average is 1. Average demos select Decimal scale 2 and round ties to even.
The CLI's decimal rows are 1.25, 2.50 and -0.75, summing exactly to 3 at declared
scale 2. Their optional `maybe` values are absent, so the grouped sum displays `—`.
These sums explicitly select 128-bit results. It returns minimum amount 11 and maximum amount 60. Its absent
note has no extremum; present priority zero returns 0. Aggregates finish within
one execution or refuse, without partial answers or continuation.
Grouped extrema retain the group when every operand is absent, displaying `—`.
`--limit 1` selects the first completed group; it does not shorten the input scan.
Aggregate ascending order puts category 1’s minimum (11) before category 0’s (20).
The filtered count command returns category 0 with count 2. Grouped amount sums
are category 0 = 120 and category 1 = 93; `--group-min 100` keeps category 0.

```sh
toko-cloc
toko-cloc -d 1
toko-cloc -d 2
```

### 🛠️ Changes

- Add `.avg::<Record>()` and `.group_avg::<KeyRecord, Record>()` for integer and
  Decimal inputs. The result record declares Decimal scale; round the complete
  total/count once, to nearest with ties to even. Check result overflow after
  division, preserve absence, and filter/order completed rounded group values.
  Share the existing numeric accumulator, group table and codecs.
- Extend scalar/grouped sums to Decimal at the exact declared scale. Reuse I256
  coefficient accumulation and the owned codec, with final i128 overflow checks
  and no rounding or numeric conversion. Qualify cancellation, absent/zero groups,
  28 fractional places, excluded-group overflow and Wasm upgrade/reopen.
- Add DecimalRow inspection/query demos to the local CLI; its sample rows are
  seeded through the private qualification write endpoint.
- Add `.min::<Record>()` and `.max::<Record>()` over exact single-field
  projections. Preserve ordered scalar/unit-enum types, ignore absent operands
  and distinguish no result from a present zero. Reuse the scanner and ordering
  codec; retain only the winning key/value. Work and comparison-key exhaustion
  refuse, while output bytes apply to the completed winner.
- Add `.group_min::<KeyRecord, ValueRecord>()` and `.group_max::<KeyRecord, ValueRecord>()`.
  Preserve absent grouping keys and all-absent groups; allow overlapping key/value
  projections. Counts and extrema now share one bounded group table and reducer
  implementation. Account for winner growth and release replaced winner storage;
  keep output limits separate from complete input and retained-state bounds.
- Add `.filter_groups(comparison, operand)` and `.order_groups(GroupOrder)` for
  completed counts/extrema/sums/averages. Filter, sort, then limit output; aggregate ties retain
  ascending group keys. Reject these modifiers on scalar and row terminals.
  The local CLI exposes `--group-min` and `--group-order key|asc|desc`.
- Add scalar integer `.sum::<Record>()` with explicit same-family result widening,
  no implicit narrowing or float/decimal conversion. Preserve absence versus zero;
  checked 256-bit accumulation precedes final result-range validation. Signed
  cancellation can recover an out-of-range prefix; overflow reports
  `ArithmeticOverflow`. Reuse the scanner and owned result codecs.
- Add `.group_sum::<KeyRecord, ResultRecord>()` through the existing group table.
  Validate every completed total before group filtering/order/limits, and charge
  final result encoding to the retained-state budget. All-absent groups remain.
  Rename `GroupExtremum` to `GroupValue` for the shared min/max/sum result shape;
  callers naming the old type must update their import. No schema/format change.
- Adopt Toko Tooling 0.44.3 in both workspaces through its published installer.
  Default LOC output matches `-d 0` compact totals; `-d 1` expands crates and
  `-d 2` expands modules. Executable snapshot ownership remains unchanged.

## [0.8.0] - 2026-11-15 (development checkpoint)

### 📝 Summary

Add ordered DISTINCT and exact bounded row/group counts through the shared fluent
executor.

### ▶️ Try it

```sh
make db
db query Entry --order-by category --select category --distinct
db query Entry --category 1 --order-by category --select category --distinct
db query Entry --order-by category --select category --distinct --work 1 --limit 1
db query Entry --select count
db query Entry --select category,count
db query Entry --select category,count --min 40 --limit 1
db query Entry --select distinct-count:category --limit 1
db query Document --select count:note
db query Entry --select category,count:amount --min 40
```

The fixture has two category values. Work-limited continuation may return empty
pages while passing duplicates; use the printed cursor with the same query.
Counts complete in one execution or refuse; they do not have continuations.
The distinct category count is 2; the fixture Document has no note, so its
present-note count is 0.

Source reports now default to crate rows; request module detail explicitly:

```sh
toko-cloc
toko-cloc -d 1
```

### 🛠️ Changes

- Adopt Toko Tooling 0.44.2 in both workspaces and the authenticated snapshot.
  Default `toko-cloc` depth now matches `-d 0`; release timing reports reject
  malformed finish records instead of treating missing status as success.
  `make toko-tooling-check` now also admits the qualification workspace and
  verifies that its selection matches the product workspace.
- Add `.count()` and `.group_count::<Record>()`. Group keys use ordered scalar
  semantics and include absent values; no index is required. Output limits select
  completed groups only. Work, group or retained-state exhaustion refuses instead
  of returning prefix counts. Resumable jobs and other aggregates remain planned.
- Add `.count_field("field")`, `.count_distinct::<Record>()` and
  `.group_count_field::<Record>("field")` through that same scanner and group map.
  Field counts exclude absence but include present empty values of all stored
  types. Distinct tuple counts include absence; all-absent operand groups survive
  with count zero. No new durable state, codec or schema attributes.
- Add `.distinct()` for complete entities, projections containing the primary
  key, and projections forming a leading index tuple after any fixed prefix.
  Filters on projected or fixed-prefix fields are supported: each duplicate group
  is either wholly rejected or emitted once. Automatic and explicit paths share
  the same proof. Filters varying within a group and other shapes refuse for now;
  broader DISTINCT remains planned.
- Suppress duplicates across pages in both directions, retry an unreturned value
  after byte exhaustion, and continue after deletion of the cursor row or an
  unchanged-schema upgrade. Every examined row still undergoes integrity checks.
  This is live enumeration, not an exact aggregate snapshot.
- Reuse index equality and cursor framing with one bounded previous-key buffer;
  no durable seen-set, new cursor fields or additional executor. Native and Wasm
  coverage is selected by `make m2-queries`; CLI coverage by `make m2-cli`.

## [0.7.0] - 2026-11-15 (development checkpoint)

### ⚠️ Breaking

- Reinstall the CLI with `make db`. `query --after` now accepts the printed
  hexadecimal cursor, not a numeric row ID. Repeat the same entity, filters,
  order and projection when continuing.
- The 512-byte complete-key contract uses semantics version 2. Older stored
  contracts refuse intact; no automatic transformation or data deletion is provided.

### ▶️ Try it

Inspect `AccountKey` / `SubaccountKey` for typed primary keys and mutual references.
`EnvelopeRow` is the synthetic maximum-row/index-fan-out fixture.
`EnumRow` now shows the `state,id` and sparse unique `previous` indexes over `Lifecycle`.
The CLI uses a fresh local fixture; `schema` shows all entities and `query` supports
`Entry` (default) and `Document`.

```sh
make db
db schema
db schema EnumRow
db schema EnumCollectionsRow
db schema Document
db query --limit 2
db query Entry --category 1 --order-by id --desc --select id,amount
db query Entry --min 100 --work 2
db query Document --entry-id 1 --order-by score
db query Document --select id
db query --help
```

Measure controlled entity growth separately from the local CLI fixture:

```sh
make m1-entity-growth
```

### 🛠️ Changes

- Start M2 with bounded fluent reads: exact AND comparisons, primary or explicit
  index-prefix order, reverse traversal, work/row/byte limits and query-bound
  continuations. Sparse pages advance over rejected candidates; output limits
  retry rather than skip a matching row. Shared borrowed field inspection avoids
  constructing rejected entities. `make m2-queries` selects the native/Wasm probe.
  The local CLI now uses these same admitted operations.

- Connect `db query [Entry|Document]` to fluent reads, with field ordering,
  descending traversal, typed projections and independent row/work/byte bounds.
  Replace numeric `--after ID` with an opaque hexadecimal `--after CURSOR`;
  reuse it with the same query shape. Normal output shows tables, work counts
  and continuation data; guidance stays in `--help`. Remove the old host-side
  post-page filter. `make m2-cli` checks the installed-layout console and migrated
  query caller. This remains a temporary local fixture, not a remote adapter.

- Add `order_by(&[...])` with an implicit primary-key tie-breaker and automatic
  ordered-path selection. Equality prefixes narrow compatible indexes; incomplete
  ordering and unsafe sparse paths refuse. Selection is deterministic and bounded
  by schema metadata, with no sorting, plan cache or statistics. Native full-sort
  oracles and the `make m2-queries` Wasm probe cover continuation and upgrade.

- Add typed selective projection through `fetch_as::<Record>()`, reusing ordinary
  `#[record]` declarations and their codecs. Exact field/type admission includes
  transitive named definitions. Byte limits count projected output; cursors bind
  its shape, and unselected corruption still refuses. No macro syntax, durable
  schema or dependency changes. `make m2-queries` also checks projection and its
  upgrade/byte-retry boundaries; CLI `--select` exposes the fixed fixture shapes.

- Refresh qualification's Wire transport lock to 0.22.3. Product codecs and
  dependencies remain independent of Wire.

- Keep CLI output focused on results: move schema explanations to
  `db schema --help` and remove the startup command tutorial.

- Support unit enums as set elements and map keys, with canonical ordering shared
  by indexes and codecs. Retain count/node bounds and reject corrupt duplicates,
  reversed order and invalid variants before materialization or mutation.
  `EnumCollectionsRow` demonstrates the declarations; payload enums remain excluded.

- Add `db schema ENTITY` for one declared entity, with its fields, indexes,
  references and reachable nested types. Recursive definitions appear once;
  unknown names refuse clearly. The same command works inside the console.
  `db schema` retains the whole-schema view; this remains the local fixture.

- Support unit-enum secondary indexes, including compound, optional and sparse
  unique keys. Canonical variant-name order agrees with row encoding; query
  operands must match the whole declared enum. Payload enums remain unsupported
  as keys. Native byte/corruption checks and Wasm rollback/upgrade qualification
  cover the existing index path; no new durable catalog or executor.

- Keep opening focused on the manifest. Revision reads and write preparation
  validate only the needed entities; no-ops validate without advancing, while
  malformed/overflowed counters refuse the complete affected batch. Generated
  schemas reuse immutable metadata and the local M1 host retains storage-runtime
  custody. No revision/admission cache or format change. Warm 32-entity opening
  falls from about 755k to 113k instructions after removing unrelated checks.

- Add `make m1-entity-growth` to measure 1/8/32 equivalent entities with a fixed
  endpoint count. Check typed row/index isolation, replacement, deletion and
  upgrade; separate host bootstrap, lease, schema construction and database-open
  instructions. Compare fresh and retained hosts, including cold upgrade/trap
  recovery. With operation-specific revision validation, current fresh/retained
  hosts measure about 960k/113k opening instructions at 32 entities. Retaining
  host setup adds about 1.4 KB Wasm; no durable metadata or executor is added.

- Enforce IcyDB's 512-byte complete index-key budget in the existing write/read
  paths, including framing and owner suffixes. Check new keys before no-op
  shortcuts, reserve query-prefix completion space, and bound supplier key reads
  before payload allocation. Semantics version 2 refuses older contracts intact;
  row/component bytes and schema attributes are unchanged. Keep 4 KiB rows with
  unindexed padding in the maximum-batch fixture; independently audit 512-byte
  keys and 513-byte refusal. No extra map, executor, WAL or dependency.

- Add `make m1-envelope-comparison` for matched 32-row, eight-index writes
  against frozen IcyDB, including its journal settling. Retain the original
  large-key refusal separately: IcyDB's 512-byte key bound excludes that shape.
  The common workload uses four 48-byte Text values; product code is unchanged.

- Add `make m1-write-envelope` for real Wasm qualification of 32 × 4 KiB rows
  and eight compound indexes per row. Independently check every stored row/index
  byte, preparation refusals, no-ops, rollback, upgrade and deletion. Keep the
  probe and bounded raw inspection in qualification; product execution is unchanged.

- Support supplied Account/Subaccount primary keys, typed lookup/delete/page
  cursors and exact references. Absent and explicit-zero subaccounts identify
  the same Account across primary uniqueness, batch targets and references.
  Reuse the owned key codec and existing batch executor; malformed stored owners
  refuse intact. Existing stored contracts remain unchanged; no attribute is removed.
  Native and Wasm tests cover aliases, cross-entity references, rollback and upgrade.
  The unchanged scalar benchmark adds 1.2 KB Wasm and under 0.03% query/write
  instructions; Account-specific workload costs remain separate qualification.

## [0.6.0] - 2026-11-14 (development checkpoint)

### ▶️ Try it

The CLI still inspects a fresh local fixture and queries `Entry`. Look for
`DefaultRow`: its **Info** column now shows literal defaults beside field bounds.
`ManagedRow` shows database-owned creation/update timestamps in the same column.
`PrincipalKey` demonstrates a Principal primary key and optional self-reference.
ULID issuance remains an explicit Rust API.

```sh
make db
db schema
db query --limit 2
```

### 🛠️ Changes

- Support supplied Principal primary keys and exact typed references through the
  existing batch, lookup and pagination paths. Empty and full-width principals
  retain length-first ordering; invalid stored lengths/owners refuse intact.
  Reuse the shared index codec and remove the fixed-width reference assumption;
  no new executor, durable metadata or authored attribute is needed.

- Add `managed = "created_at"` / `"updated_at"` for Timestamp fields and
  `Batch::at(time)`. Inserts share one captured time; replacements preserve
  creation and update time only for real changes. No-op timestamps/revisions
  remain unchanged, while authored/stale managed values and backward real-change
  clocks refuse before mutation. No hidden clock, callback or durable journal.
  Managed policy enters schema identity; no existing attribute is removed.
  Native and Wasm regressions retain IcyDB's no-op and corrupt-order guarantees.
  Shared overhead adds 3.9 KB of Wasm and less than 0.1% to measured scalar
  query/write instructions; managed-write costs remain a separate workload.

- Adopt Toko Tooling 0.43.2 through its published snapshot installer. Refresh
  qualification to Testkit 0.106.0, Host Artifacts/FS/Process 0.79.0 and Host Text
  0.76.0, plus the newly selected Platform Runtime/macros/guard 0.22.5 patch.
  Storage, Wire, Platform Types and the native Platform facade are already current.
  Remove shared-source path overrides and use directory modules where children
  exist. The private record-schema fixture becomes a normal Cargo dependency
  for host collection and target compilation; product crates and schema identity
  stay unchanged.
  The matched scalar benchmark preserves IC DB's query/write execution costs;
  the design records the small artifact changes from the Runtime patch separately.

- Add typed `Entity::insert()` inputs and `Batch::insert_input`. Omitted fields
  use declared integer, Bool or bounded Text defaults; explicit values, including
  `None`, remain unchanged. Missing required fields refuse. Use
  `field(..., default = 7)` in the existing attribute syntax; no attribute is
  removed. Replacements and decoding never reapply defaults.
- Include defaults in schema identity and inspection. Changed default policy
  refuses incompatible stored contracts intact; contracts without defaults keep
  their fingerprints. Native and installed Wasm checks cover default collisions,
  exact integer limits, refusal/trap rollback and upgrade preservation. Richer
  defaults and primary-key defaults remain separate work.

- Add `UlidGenerator` with an explicit operation timestamp and host-provided
  randomness. Preserve monotonic ordering at equal/backward times and refuse
  invalid timestamps or suffix exhaustion without changing state. Hosts own
  issuance checkpoints; ordinary refusals may leave gaps, while IC traps roll
  back the current execution. No clock, RNG dependency or stored-format change.

- Give the shared fixture transport an ordinary private qualification crate,
  replacing source-path sharing. Keep the same bounded Platform hooks; product
  crates acquire no transport dependency. Benchmark source capture now records
  intentional tracked deletions through its existing source diff.

## [0.5.0] - 2026-11-14 (development checkpoint)

### ▶️ Try it

Inspect `SignedKey` / `WideKey` for Int128/U256 primary keys and mutual references,
and `KeyRow` for ULID keys, alongside
`DecimalRow`, `DateRow` and `TimeRow`, with `db schema`. Decimal fields
show their declared scale in **Info**; time fields show their millisecond units. This remains a fresh local fixture;
`query` reads Entry rows.

```sh
make db
db schema
db query --limit 2
```

### 🛠️ Changes

- Move all six qualification hosts from Wire Endpoint to Platform's generic
  guard/decode/encode hooks. Preserve method names, Wire identities, directional
  limits and trap behavior. Standalone hosts remain usable without Canic;
  `make m0-endpoints` checks malformed requests and reply-failure rollback across
  message/callback boundaries. No product schema, stored bytes or query change.
  [#1 — Migrate qualification canisters off retiring Wire Endpoint](https://github.com/dragginzgame/ic-db/issues/1).

- Extend typed primary keys and exact references to signed/unsigned 8–128-bit
  integers and U256. Preserve numeric order, full-width owners and typed cursors.
  Primary and secondary keys share ordered encoding; no implicit widening,
  signedness conversion, new dependency or persistent metadata.

- Admit supplied ULID primary keys with typed lookup, delete and page cursors.
  References use the target’s exact Nat64/Ulid key type, including final-state
  cycles and restricted deletion across both types. Preserve Nat64 bytes and
  schema identities. Reuse the existing ULID codec and shared commit path; no
  new authoring attributes, ID generator, clock, RNG or durable metadata.

- Add `prim = "Decimal", scale = N` (0–28) for exact decimal fields, optional
  values, collections and secondary indexes. Adapt IcyDB’s owned coefficient/scale
  codec and numeric ordering. Equivalent values share equality, hashing and keys;
  excess precision or coefficient overflow refuses without rounding. Persisted
  scales must match the schema. No dependency or aggregate arithmetic is added.

- Add signed Unix-millisecond `Timestamp` and unsigned-millisecond `Duration`
  values, optional fields, collections and exact secondary indexes. Adapt
  IcyDB's owned eight-byte codecs, pre-epoch flooring, text parsing and saturating
  value arithmetic, including full-width duration operations. No clock, timer,
  managed-field state or dependency is added. Aggregate arithmetic remains separate.

- Add bounded Gregorian `Date` values, optional fields, collections and secondary
  indexes. Adapt IcyDB's epoch-day codec and calendar algorithm; reject invalid
  dates and out-of-range stored/index bytes. No calendar dependency, clock read
  or managed timestamp policy is introduced.

## [0.4.0] - 2026-11-14 (development checkpoint)

### ▶️ Try it

`db schema` now includes `UlidRow` with supplied ULID values and ordered/unique
indexes, plus `AccountRow` with Account/Subaccount fields, alongside
`FloatRow` with finite Float32/Float64 fields and compound/optional unique indexes.
Reinstall with `make db` to refresh the fixture.

Type bounds now appear in a separate **Info** column. This console still uses
a fresh local fixture; `query` reads Entry rows.

```sh
make db
db schema
db query --limit 2
```

### 🛠️ Changes

- Add `prim = "Ulid"` for supplied values, optional fields, collection elements/
  keys and secondary indexes. Adapt IcyDB's 16-byte codec and canonical uppercase
  text wrapper. Keep `ulid` 3.0.0 default features disabled: no native RNG or
  automatic generator. Native/Wasm witnesses cover full-width values, timestamp
  boundaries, malformed widths/text, unique exchanges, rollback and reopen.
  Document IcyDB's separate timestamp/entropy/monotonicity lessons for generated
  IDs; primary keys remain Nat64 in this increment.

- Adopt published Platform Runtime/macros/guard 0.22.4 and set the product's
  Types minimum to 0.20.4. Platform itself remains transitively selected at
  0.33.2: announced 0.33.3 was unavailable from crates.io at this update.
  Keep the coordinated runtime singleton and database-owned identity codecs.

- Adopt IC Testkit 0.105.0 for qualification and the local `db` host. Keep fresh
  test instances; no retired pooling API is needed. Its host helpers now share
  Artifacts/FS/Process 0.78.0, removing the duplicate Artifacts version and obsolete
  process dependencies. PocketIC remains 16.1.0; product codecs are unchanged.

- Add `prim = "Account"` and `prim = "Subaccount"`, using the re-exported
  Platform Types values with database-owned codecs. Account identity treats an
  absent subaccount and an explicit zero subaccount identically. Support optional
  values, collections and exact secondary indexes; ordering follows owner then
  effective subaccount. Native/Wasm checks cover independent bytes, all owner
  lengths, duplicate keys, uniqueness swaps, bounds, rollback and reopen. No new
  dependency, account wrapper or durable metadata is introduced. Primary keys
  and references remain Nat64.

- Move schema table bounds into a separate **Info** column for entity fields,
  records, enum payloads, lists, sets and maps. Type names no longer contain
  parenthesized limits. Run `make db`, then `db schema`, to refresh the console.

- Add finite `Float32`/`Float64` fields, collection elements/keys and exact
  secondary-index operands. Adapt IcyDB’s finite wrappers and owned IEEE-byte
  codec: constructors normalize signed zero, while non-finite inputs refuse.
  Persisted NaN, infinities and noncanonical negative-zero bytes refuse as corrupt.
  Equality, hashing, ordering and unique ownership agree across the finite domain,
  including subnormal values. No new dependency or durable state is added;
  aggregate arithmetic and cross-type coercion remain separate work.

## [0.3.0] - 2026-11-14 (development checkpoint)

### ▶️ Try it

`db schema` now also shows `BigNumbers`, its required/optional U256 fields and
compound/unique indexes. U256 preserves the full unsigned 256-bit domain.

`db schema` now shows `MapRow`/`Rates` and recursive `MapTree`,
`SetRow` and its bounded scalar `Scores` set,
`ListRow` with bounded ordered lists and recursive `ListNode`,
alongside `NestedRow`, its `Details` record, and unit/payload/recursive
`NestedPayload` variants. Named definitions appear once, with fields, payloads
and optionality. No numeric field or variant IDs are authored.

From the repository root, reinstall the local fixture console, then inspect `IdentityRow`'s required and
optional `Principal` fields and unique owner index. Document's reverse index now
orders signed scores within each entry, and its new `(published, title)` index
combines boolean and exact text ordering. This remains a fresh local database;
`query` still reads Entry rows.
`db schema` now shows Membership: IdentityRow's optional identity index requires
all optional keys present; Document's priority index includes absent values.
The empty `OptionalLink` entity shows an optional Nat64 self-reference and its
ordinary reverse index; no target is required for `None`.
The fixture now builds these declarations through host-only `ctor` collection
and second-stage schema generation. Inspection now shows logical entity/index
names, with no author-assigned schema numbers. Reinstall both the console and its
fixture together; this is a new contract, not a migration.

```sh
make db
db schema
db status
```

### 🛠️ Changes

- Adopt published `ic-host-artifacts` 0.78.0 for benchmark Wasm inspection.
  `ic-host-text` 0.75.0 remains current. Testkit 0.104.1 still selects
  Artifacts/FS/Process 0.77.3; moving those helpers awaits publication of the
  announced Testkit 0.105.0. Product codecs, schema identities and Wasm
  dependencies are unchanged.

- Add `prim = "U256"` using the feature-free `ethnum` integer type re-exported
  as `ic_db::U256`. Adapt IcyDB’s owned 32-byte big-endian payload and boundary
  cases, without its wrapper, numeric conversion engine or Wire serialization.
  Support exact secondary-index operands, optional uniqueness and collection
  elements/keys/values. Native/Wasm checks cover high bits, independent bytes and
  fingerprints, malformed data, row limits, uniqueness swaps, no-op revisions,
  rollback and unchanged upgrade. Existing contracts without U256 are unchanged;
  aggregate arithmetic and non-Nat64 primary keys remain separate work.

- Add named maps with scalar keys and ordinary optional/list/named values through
  `#[map(key(...), value(...), max_items = N)]`. Retain IcyDB's `BTreeMap` wrapper
  and immutable-key rule. Shared ordered traversal rejects stored duplicate or
  descending keys before insertion can overwrite a value. Missing keys, absent
  values and empty values remain distinct. Recursive maps have an empty base and
  use the existing depth, node, byte and collection-entry budgets. Native/Wasm
  checks cover exact bytes/fingerprints, malformed keys/payloads, shared limits,
  refused writes, no-op revisions, late traps and upgrade/reopen. No new crate,
  dependency, durable state or value engine. Existing contracts without maps
  remain unchanged; non-scalar keys and member indexes/references remain work.

- Add named scalar sets using IcyDB's `#[set(item(...))]` wrapper shape with
  explicit `max_items`. Preserve logical ordering, construction-order independence
  and absence versus empty. Decode refuses duplicate or descending stored elements
  before insertion can erase the evidence. Typed decoding and borrowed inspection
  share the order check; lists and sets share the existing 256-item row budget.
  Native/Wasm checks cover independent bytes/fingerprints, numeric and identity
  order, bounds, corruption refusal, no-op revisions, traps and upgrade/reopen.
  Existing contracts without sets remain unchanged. No new crate, dependency,
  durable catalog or generic value representation. Non-scalar elements, member
  indexes and normalization policies remain implementation work.

- Adopt Toko Tooling 0.43.1 in both workspaces and refresh the authenticated vendor
  snapshot. Frozen release tooling now retains phase timings across failures and
  resumes; database dependencies and behavior are unchanged by this refresh.

- Add ordered lists to entity/record fields and enum payloads using
  `value(many, max_items = N, item(...))`. Retain IcyDB's `many` spelling with an
  explicit count bound; preserve order, duplicates and absence versus empty.
  One codec budget checks counts before allocation/traversal: at most 256 list
  items and 256 structured values per row, depth 32 and 4 KiB encoded bytes.
  Empty lists give recursive types a finite constructor. Shared stored inspection
  validates every element without constructing a list. Native/Wasm checks cover
  independent bytes/fingerprints, malformed counts/elements, sibling and recursive
  limits, no-op writes, refusals, late traps and upgrade/reopen. New list metadata
  binds these limits; contracts without lists remain unchanged. No new dependency,
  durable state or generic value tree. List indexes/references and optional
  list elements remain future work; no existing attribute is removed.

- Support named records and unit/payload enums through the familiar attribute-owned
  `record`, `enum_`, `variant(value(...))` and `item(is = "...")` spelling.
  `item(indirect, is = "...")` gives explicit Box indirection. Recursive graphs
  require a finite constructor; required cycles without one refuse admission.
  One finite compiled graph replaces the unit-only descriptor. The generated
  codec boundary is now `Value` for both records and enums; the provisional
  `Enum` trait/`EnumSchema` are removed. All typed reads/writes and shared stored
  inspection enforce the same 32-level/256-structured-value/4 KiB row envelope.
  Scalar-only contracts remain unchanged. Named metadata now uses `ICDBTY02`;
  earlier unit-enum manifests refuse intact. No persisted catalog, Wire codec,
  materialized generic value tree, additional product crate or journal is added.
  Native/Wasm checks cover exact bytes, transitive schema changes, reorderings,
  malformed payloads, boundary limits, no-op writes, late traps and upgrade/reopen.
  Nested indexes/references remain implementation work.

- Remove mandatory numeric entity/index IDs from scalar authoring. Entity names
  default to the Rust type name; index names default to ordered comma-separated
  fields. Optional `name` overrides preserve explicit identity. References select
  the sole suitable reverse index, with `index = "name"` for ambiguity. Runtime
  index lookup accepts the name and revision lookup uses the entity type. Remove
  entity/index `id` attributes and numeric reference selectors. Full names define
  contract grammar 2; collision-checked internal handles require no durable
  assignment table. Old numbered manifests refuse unchanged. Native/Wasm checks
  cover name collisions, reference resolution, old-contract refusal, persistence
  and rollback. `db schema` shows names instead of schema ID numbers.

- Implement the first two-stage schema pipeline using IcyDB's constructor/path
  approach. `references(entity = "path::Target", ...)` resolves through Rust,
  including imports and `Self`. Native-only
  `ic-db/build` collects declarations, validates through the existing admission
  owner and emits the target's schema assembly/fingerprint. The M1 fixture uses
  the same authored module for host and Wasm, replacing its manual entity list.
  Qualify duplicate/late registration, order independence, missing/unresolved
  targets and unchanged Wasm persistence/rollback/reopen. No persisted catalog,
  endpoint generation, new product crate or Wasm constructor dependency is added.
  Named types use the same pipeline. Runtime and macro source retain unsafe-code
  prohibition; native declaration builds permit ctor's generated registration.
- Support optional Nat64 references through existing ordinary reverse indexes.
  Absence has no target; present zero is a real key. Validate existence and
  restricted deletion against the final batch, allowing explicit unlink/delete
  and repoint/delete together. Adapt IcyDB's absent-relationship invariant without
  adding relationship catalogs, cascades or nested-reference machinery. Native/Wasm
  checks cover zero/max keys, self-references, independent edges, maximum bounded
  unlinking, malformed data, refusal, rollback and reopen. Existing contracts and
  stored bytes remain unchanged; changing a field's optionality changes its identity.
  No attribute, crate, dependency or durable state is added.
- Add optional secondary-index components. Ordinary indexes retain absence and
  order it before presence; use `IndexValue::Absent` for an exact absent lookup.
  Optional unique indexes require `unique, present_only`, guarding every optional
  component of a compound key. Missing guards and filtered reverse-reference
  indexes refuse. Keep row validation and references independent of membership.
  Adapt IcyDB's explicit nullable-uniqueness rule without its predicate/compiler
  machinery. Native/Wasm checks cover omission, empty values, swaps, release/acquire,
  corruption refusal, rollback and reopen. No new crate, dependency or journal.
  `IndexSchema` now carries `present_only`; inspection shows its Membership column.
  Existing admitted fingerprints/key bytes remain unchanged. New index policy
  changes the contract and cannot silently reopen existing data.
  Scalar comparison grows by 855 Wasm bytes to 172,539; complete read/modify/replace
  uses 835,747 instructions (+1,236). The recorded IcyDB scalar ceilings still hold;
  optional-heavy workloads and broad fluent-query parity remain unmeasured.
- Breaking API: `index_prefix` now accepts exact `IndexValue` operands.
  Convert numeric arguments with their declared type, such as `7_u64.into()`;
  previously accepted integer slices no longer compile directly. Existing
  Nat64 persisted contracts are unchanged.

- Add required Bool and bounded Text secondary indexes. Adapt IcyDB's
  zero-byte escaping and component terminators; preserve byte-exact UTF-8 order,
  distinct normalization forms, empty strings and embedded NULs. Text operands
  borrow `&str`; explicit stored operand types now use `IndexValue<'a>`.
  Lookup prefixes match complete leading fields, not partial strings. Native and
  Wasm checks cover bounds, ordering, tuple boundaries, writes, refusal and reopen;
  the full 4 KiB row/escaped-key boundary is additionally qualified natively.
  Keep one component encoder/admission owner and remove the obsolete eight-byte
  allocation assumption. No new crate, dependency, durable metadata or attribute.
  Scalar comparison: 171,684 Wasm bytes (+1,033); listing 780,555 instructions
  (+607); complete read/modify/replace 834,511 instructions (+977).
  All measured scalar operations remain below the frozen-IcyDB comparison
  ceilings; this does not measure text-heavy queries.

- Extend secondary indexes to required Nat/Int8/16/32/64/128 and Principal fields.
  Adapt IcyDB's numeric ordering and length-first Principal rules into one shared
  component encoder. Typed lookup operands must match their declared fields;
  compound boundaries, duplicate ordering, unique swaps, reverse references,
  corruption refusal and Wasm rollback/reopen are qualified. Preserve existing
  Nat64 key bytes and fingerprints. Primary keys/references remain Nat64;
  fluent queries remain work. No attribute is removed,
  new durable metadata added, or supplier/transport dependency introduced.
  The matched scalar host grows by 3,620 bytes to 170,651 Wasm bytes; indexed
  listing uses 779,948 instructions and complete read/modify/replace uses 833,534,
  increases of about 0.8% and 0.25% over the preceding Principal increment.
  Both remain below the frozen-IcyDB workload ceilings; broader parity is unproven.

- Add required/optional `Principal` fields using the canonical Platform Types
  identity. Adapt IcyDB's raw-byte admission and rejection cases to IC DB's owned
  u32-length framing. Distinguish absent, management-canister, anonymous and
  maximum-length identities; reject malformed stored values, including during
  key-only mutation inspection. Native/Wasm checks cover literal bytes, schema
  identity, no-ops, reopen and late-message rollback. Tag 14 extends codec-v3
  schemas without changing contracts that omit the new type. Principal primary
  keys and predicates remain unimplemented; no existing attribute is removed.
  Platform Types is a direct product dependency; its identity/text helpers are
  reused without introducing Wire persistence, an IcyDB wrapper or a value engine.
  The existing scalar host grows by 410 Wasm bytes; measured scalar operations
  rise by 6–10 instructions and remain below the frozen-IcyDB comparison ceilings.

- Add `make m1-records-comparison`: one independent scalar workload and tuple
  oracle for IC DB and frozen SQL-disabled IcyDB, with matched schema/index,
  supplier graph, transport and build profile. Record Wasm/code-section bytes,
  instruction counts, full update cycles and IcyDB's settling work. The initial
  slice passes its ceilings: 166,621 versus 3,085,668 Wasm bytes; complete
  read/modify/replace uses 831,463 versus 7,022,250 measured instructions and
  7,845,420 versus 21,196,570 cycles. Preserve scope limits in the system design:
  direct IC DB index reads are not a completed fluent engine, and broader product
  parity/savings remain unmeasured. One private qualification host is added;
  no product code or runtime dependency changes.
- Qualify actual IC instruction exhaustion inside a product commit, after its
  first backing write. Require PocketIC's instruction-limit error and unchanged
  raw backing, revisions, indexes and references; retry completes successfully.
  The existing write-trap and physical-growth refusal cases still pass.
  Carry over IcyDB's resource-refusal preservation invariant without importing
  its budget framework or journal. Only the qualification host and driver change;
  product code, dependencies, persisted bytes and CLI behavior remain unchanged.
  This proves rollback at the selected failure boundary, not that all accepted
  workload shapes complete within the instruction limit.

Run the focused checks with the already prepared local PocketIC server:

```sh
make m1-commit-failures
make m1-records-comparison
```

## [0.2.0] - 2026-11-14

### ▶️ Try it

Install the console using the real first M1 record library, then inspect its
generated schema and two bounded pages. Each session has six Entry rows, one
mixed-type Document referencing Entry 1, and one record each for WideNumbers, BinaryObject and NarrowNumbers; this does not connect to a deployed
database. `db schema` shows the target and reverse index. Restart an existing console.

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
`db schema` also shows Document's bounded Text, Bool, signed Int64 scores and optional fields. The
current `query` command still reads Entry; Document writes/reopen are covered by
`make m1-records` through private qualification controls.

### 🛠️ Product slice

- Use a minor experimental release for the changed public API and durable codec
  contract. Extend release qualification to the actual M1 Wasm records and
  supplier-failure probes; update both product and qualification lockfiles in
  one shared metadata transaction. Qualification packages remain at `0.0.0`.
  Supplier review on 2026-11-14 found the selected Storage, Platform Runtime/Types,
  Testkit, Host, Wire/Endpoint, IC Build and Toko Tooling versions current.
  Defer qualification-only Platform's optional `0.32.3` → `0.33.0` update to the
  next dependency-maintenance pass; retain this release's tested graph.
  Product crates remain unpublished and do not depend on Wire.
  Match the comparison host's wakeup stub to IcyDB's generated native-test
  configuration so full-workspace linting passes without relaxing lints.
  Move the installed-console smoke test from the historical maintenance fixture
  to M1 records; check the current tables, continuation and silent blank input.
- Record the accepted IcyDB carryover sequence in the system design: borrowed
  access and selective reads, shared equality/ordering/hashing, and owned rich-type
  codecs; offset tables and caches remain evidence-gated choices.
  Implement the first borrowed-read use in shared stored-key inspection for
  writes and reference checks. Remove per-entity decoder callbacks and avoid
  constructing owned text/blob fields when only keys are needed. Preserve full
  field/row validation, physical key/index checks, existing bytes and fingerprints.
  Public typed reads still return complete entities; selective fluent queries
  remain implementation work. No new dependency, durable state or cache framework.
- Add required/optional `Nat8/16/32` and `Int8/16/32` fields with native Rust
  integer types and exact 1/2/4-byte little-endian payloads. Adapt IcyDB's owned
  scalar width and boundary cases; verify absence, malformed rows, schema
  incompatibility, no-ops, reopen and message rollback. Existing codec-v3 schemas
  retain their identities and bytes. No write-engine or dependency change.
  `db schema` shows all six widths in NarrowNumbers; `db query` still reads Entry.
  Keys, indexes and references still require Nat64; storage support does not
  imply query arithmetic or coercion support.
- Remove IC Wire from product dependencies and stored-row serialization. Adapt
  IcyDB's owned fixed-width scalar routines and exact-length regression cases:
  little-endian integers, strict boolean/presence tags and checked u32
  byte lengths. `Blob` is an alias for `Vec<u8>`. Row codec identity advances to 3;
  both previous experimental manifests refuse without mutation. No migration is
  implied. Wire remains only in separate qualification transport/historical probes.
  Small integers take more stored bytes than the provisional varint codec;
  decoding has no varint/zigzag path. No performance improvement is claimed.
- Add required/optional `Blob(max_bytes)` fields exposed as `ic_db::Blob`.
  Preserve arbitrary bytes, empty versus absent values,
  zero-length bounds and length framing. Qualify the relevant frozen IcyDB
  regression cases, plus a 1,024-byte bulk value,
  oversized stored/input values, incompatible schemas, no-ops and Wasm rollback/
  reopen. The commit engine is unchanged; Blob indexes and
  predicates remain later work. `db schema` shows BinaryObject's byte limits;
  `db query` continues to read Entry only.
  Also admit `Text(max_bytes=0)` as an empty-only contract, following the same
  IcyDB regression; remove the unnecessary Text-specific zero-bound restriction.
- Require targeted inspection of frozen IcyDB implementations, regression tests
  and relevant history before subsystem work. Reuse/adapt established solutions
  where the new contract permits, and qualify their failure cases in IC DB.
  Record concrete read/write-boundary precedents in the system design; a fresh
  implementation is not evidence of a better mechanism.
- Remove redundant decoding at the typed/runtime boundary. Reads decode once;
  batch construction extracts keys from the already validated typed value instead
  of decoding newly encoded bytes. Primary-key reads avoid constructing index
  keys. Share physical-index membership checks with restricted deletion and retain
  corruption refusals. Stored bytes, schema identity and message atomicity are
  unchanged. Consolidate the design's implementation status and keep golden
  fingerprint inventories in executable tests. Wasm growth/performance remains
  a measurement gate, not a claimed improvement.
- Add required/optional `Nat128` and `Int128` fields with exact Rust `u128`/`i128`
  values. Native/Wasm checks cover extrema, values beyond 64 bits, malformed
  encodings, absence, no-op writes, schema incompatibility, message rollback and
  reopen. Earlier schema tags/fingerprints are unchanged. No executor, numeric
  conversion layer or dependency is added; wide keys/indexes and arithmetic are
  still unimplemented. `db schema` now includes a separate WideNumbers fixture
  entity; `db query` remains Entry-only.
- Add required/optional `Int64` stored fields using the full Rust `i64` range.
  Preserve negative values and absence versus zero without numeric coercion.
  The new schema tag leaves earlier contracts unchanged; native/Wasm checks cover
  extrema, malformed encodings, schema mismatch and persistence. No commit-engine
  specialization, dependency or crate is added. Signed indexes and arithmetic
  remain later work. `db schema` shows `score` and `previous_score`; `db query`
  continues to read Entry only. Multiple-entity querying will require an explicit
  entity selector, rather than choosing a default from schema order.
- Add required Nat64 references with `references(entity = ..., index = ...)`.
  The named ordinary index must lead with that field. Prepare target-existence
  and restricted-deletion checks against the complete batch: paired insertion,
  repointing, cycles and explicit dependent deletion work without cascades.
  Reference contracts participate in schema identity; no new durable catalog,
  journal or runtime crate is added. Optional/nested references remain future work.
  `db schema` shows the reference target and the dependent's reverse index.
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
  journal code is copied. Richer types, broader references and fluent queries remain
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
  real three-row replacement (including a dependent repoint) and refuse a physical growth request during a 32-row
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
