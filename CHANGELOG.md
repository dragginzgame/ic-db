# Changelog

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
