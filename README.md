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
exactly admitted schemas, persists signed/unsigned 8/16/32/64/128-bit integers,
U256, Ulid, Date, Timestamp, Duration, Decimal, finite floats, Bool, Principal,
Account/Subaccount and bounded Text/Blob with optional
values, maintains indexes and entity revisions, and applies bounded final-state
batches with required/optional integer, Ulid, Principal, Account or Subaccount references and indexed restricted deletion. This is
experimental: there is no production compatibility promise or deployment.
The first bounded fluent read slice is also available:

```rust
let page = db.query::<Entry>()
    .using_index("category,id", &[1_u64.into()])
    .filter("amount", ic_db::Comparison::Ge, 40_u64)
    .descending()
    .limit(20)
    .work_limit(100)
    .byte_limit(4096)
    .fetch()?;
```

Omit `using_index` for primary-key order. Filter operands must match the exact
declared type. `page.next` resumes the same query through `.after(&cursor)`;
an empty page can still carry a continuation. Output budgets count encoded row
bytes; transport framing remains host-owned. This first slice supports AND
comparisons over top-level ordered scalar/unit-enum fields. `make m2-queries` checks its native/Wasm
boundaries. The local CLI now constructs these same fluent operations.

Request field ordering without naming an index:

```rust
let page = db.query::<Entry>()
    .filter("category", ic_db::Comparison::Eq, 1_u64)
    .order_by(&["id"])
    .descending()
    .fetch()?;
```

`order_by` accepts up to four distinct ordered fields and adds the primary key as
the final tie-breaker. All fields share the selected direction. Equality filters
can fix index components; the engine chooses a path that proves the complete
order or returns `InvalidQuery`. No bounded-prefix sorting occurs. Without
`order_by`, the existing primary/explicit-index order remains unchanged.

Use an ordinary named record for selective output, with exact matching field
names, types, optionality and bounds:

```rust
#[ic_db::record(fields(
    field(name="id", value(item(prim="Nat64"))),
    field(name="amount", value(item(prim="Nat64")))
))]
struct Summary {}

let page = db.query::<Entry>()
    .filter("amount", ic_db::Comparison::Ge, 40_u64)
    .fetch_as::<Summary>()?;
```

This returns `QueryPage<Summary>` without decoding a full `Entry`. Byte limits
count the projected record, while the complete source row is still validated.
Output records need no stored schema registration. Projection cursors require the
same selected fields; aliases, coercions and nested field paths are not implemented.

Add `.distinct()` to remove duplicate projected tuples across pages:

```rust
#[ic_db::record(fields(field(name="category", value(item(prim="Nat64")))))]
struct CategoryOnly {}

let page = db.query::<Entry>()
    .order_by(&["category"])
    .distinct()
    .fetch_as::<CategoryOnly>()?;
```

This slice requires a leading index projection after any fixed index prefix.
Filters may read projected fields or fields fixed by that prefix, so each duplicate
group has one filter result. For example, add
`.filter("category", ic_db::Comparison::Ge, 1_u64)` to the query above.
Full entities and projections containing the primary
key are already unique and allow ordinary filters. Filters varying within a
duplicate group and other DISTINCT shapes refuse for now. Duplicate candidates
consume work, so an empty page can still continue.
Continuation preserves duplicate suppression; live pages do not provide a frozen
input for exact aggregates.

Exact bounded counts use the same scanner:

```rust
let count = db.query::<Entry>().filter("amount", ic_db::Comparison::Ge, 40_u64).count()?;
let groups = db.query::<Entry>().limit(1).group_count::<CategoryOnly>()?;
let categories = db.query::<Entry>().count_distinct::<CategoryOnly>()?;
let notes = db.query::<Document>().count_field("note")?;
```

`count.value` is the number of matching rows. `groups.value` contains records with
`key` and `count`; the limit selects completed groups, never input rows. Grouping
supports ordered scalars/unit enums, including optional absence, in an exact named
record. No index is required; output is ascending by the tuple's canonical field
name order. Results include examined/fetched and encoded-byte counts.

`count_field("field")` counts present values of any top-level stored type; present
empty text, blobs and collections count. `group_count_field::<Record>("field")`
applies that rule within each group and retains all-absent groups with count zero.
`count_distinct::<Record>()` counts projected tuples, including absent values,
using the same bounded group state. Its count is unaffected by `limit`.

These operations finish synchronously or refuse. Completion must be observed
before the work cap, so a scan examining exactly the allowed number can still
return `Error::Limit`. Group state has separate caps of 32 groups, 4096 bytes per
comparison key and 128 KiB retained key/output bytes. Output byte exhaustion also
refuses instead of returning partial counts. Cursors, DISTINCT and explicit input order
are not yet aggregate compositions. Float arithmetic, resumable jobs and
completed-output pagination remain required work.

`.min::<Record>()` and `.max::<Record>()` accept an exact **single-field** record
projection and return `Aggregate<Option<Record>>`. They preserve ordered scalar
and unit-enum types, ignore absent operands, and return `None` for empty/all-absent
input. For optional fields, a present result retains the record's `Some(value)`.
No index is required; the scanner retains only the current winner. The comparison
key is bounded to 4096 bytes; the retained record uses the existing row bound.
All matching input must complete within the work budget, regardless of `limit`.
Only the final record's bytes count toward `byte_limit` (zero when no result).
An oversized provisional winner does not reject a smaller final answer.

```rust
let minimum = db.query::<Entry>().min::<AmountOnly>()?;
let maximum = db.query::<Entry>().max::<AmountOnly>()?;
let grouped = db.query::<Entry>().group_max::<CategoryOnly, AmountOnly>()?;
```

`group_min::<KeyRecord, ValueRecord>()` / `group_max::<KeyRecord, ValueRecord>()`
return `Aggregate<Vec<GroupValue<KeyRecord, ValueRecord>>>`. Each result has
`key` and `value: Option<ValueRecord>`. An absent key still forms a group, and an
all-absent group retains `None`; empty input has no groups. Key and operand
projections may overlap. Group counts and extrema share the 32-group/128-KiB
retained-state bounds, including winner growth and replacement. Output follows
canonical key order and is limited only after the full input completes. Encoded
output bytes count both records, excluding transport option framing.

`.filter_groups(comparison, operand)` applies up to eight AND terms to the
completed aggregate value. Counts require `u64`; extrema require the exact source
type; sums require the declared result type. Absent extrema support equality/inequality; range comparisons require
presence. `.order_groups(GroupOrder::AggregateAscending)` or
`AggregateDescending` sorts completed groups, breaking ties by ascending group key.
Absent extrema sort first ascending, last descending. `GroupOrder::Key` is the
default. These modifiers refuse on row queries and scalar aggregates.

```rust
let groups = db.query::<Entry>()
    .filter("amount", ic_db::Comparison::Ge, 40_u64)
    .filter_groups(ic_db::Comparison::Ge, 2_u64)
    .order_groups(ic_db::GroupOrder::AggregateDescending)
    .limit(1)
    .group_count::<CategoryOnly>()?;
```

All input and retained-state bounds apply before group filtering. Output limits
and bytes apply after filtering and ordering; an excluded group uses no output
bytes. Final sorting is bounded by the existing 32-group cap.

Scalar integer `.sum::<Record>()` returns `Aggregate<Option<Record>>`. The record
selects one field by name, preserving optionality; its integer type may match or
widen the source within the same signed/unsigned family. Unsigned results may
widen to U256. For example, a Nat64 amount can explicitly produce a Nat128 total:

```rust
#[ic_db::record(fields(field(name = "amount", value(item(prim = "Nat128")))))]
struct TotalAmount {}
let total = db.query::<Entry>().sum::<TotalAmount>()?;
```

Absent operands are ignored; empty/all-absent input returns `None`, while present
zero returns a record. I256/U256 accumulators retain exact totals before checking
the declared result range. Signed cancelling prefixes can exceed that result range;
final overflow and U256 accumulator overflow return `ArithmeticOverflow`.
No decimal or float conversion occurs. Work exhaustion refuses without a partial
sum. Output limits never truncate input; final result bytes use the ordinary codec.
Decimal sums retain the source field's declared scale and optionality. Coefficients
accumulate exactly in I256 and must fit i128 at that scale on completion, even if
normalizing trailing zeros could fit a different representation. Rescaling and
integer/Decimal conversion refuse; there is no rounding. Group filters accept
Decimal operands under the same declared-scale rules as ordinary comparisons.

`.group_sum::<KeyRecord, ResultRecord>()` uses the same integer/Decimal rules and returns
`Aggregate<Vec<GroupValue<KeyRecord, ResultRecord>>>`. It retains absent keys
and all-absent groups. Group predicates must use the declared result type, so a
Nat128 total requires a `u128` operand:

```rust
let groups = db.query::<Entry>()
    .filter_groups(ic_db::Comparison::Ge, 100_u128)
    .order_groups(ic_db::GroupOrder::AggregateDescending)
    .group_sum::<CategoryOnly, TotalAmount>()?;
```

Every group must finalize successfully before selection. Overflow in an excluded
group still refuses the query. Final result encoding counts toward the existing
state cap, even when an output filter would remove every group. Min, max and sum
share `GroupValue` (renamed from `GroupExtremum`); no schema attributes change.

`.avg::<Record>()` and `.group_avg::<KeyRecord, Record>()` accept all integer
widths (including U256) and Decimal inputs. The result record preserves the field
name and optionality and declares a Decimal scale from 0 to 28:

```rust
#[ic_db::record(fields(field(name = "amount", value(item(prim = "Decimal", scale = 2)))))]
struct AverageAmount {}
let average = db.query::<Entry>().avg::<AverageAmount>()?;
```

Accumulate an exact total and present count, then divide and round once to nearest
with ties to even: 1.245 becomes 1.24, and 1.255 becomes 1.26 at scale 2, with the
same rule for negative values. Empty/all-absent input returns `None`; a rounded
zero is present. The result must fit an i128 coefficient at the declared scale;
overflow refuses instead of reducing precision. Group filters and ordering use
the rounded Decimal values, with the usual ascending key tie-breaker. All groups
must finish successfully before output selection. No floating-point conversion,
per-row rounding or persisted job state is introduced.

The [first matched scalar comparison](docs/design/ic-db-system-design.md#first-matched-scalar-comparison)
measures a smaller host and lower operation costs than frozen SQL-disabled IcyDB
for one 32-row workload; it does not establish full-product parity or savings.
Rich types, broader references and the broad fluent builder remain required
implementation work; this increment does not complete M1. Secondary indexes now
support Nat/Int8/16/32/64/128, U256, Ulid, Date, Timestamp, Duration, Decimal,
finite Float32/Float64, Account/Subaccount,
Principal, Bool, bounded Text and unit-enum fields, including
optional values, compound keys and uniqueness. Optional unique indexes explicitly
require `present_only`; ordinary indexes retain absent values. Primary keys support supplied signed/unsigned 8–128-bit integers, U256, Ulid, Principal, Account and Subaccount;
references must match the target’s exact key type.
For Principal identities, declare `pk(field = "id")` and an `id` field with
`value(item(prim = "Principal"))`. Lookup, delete and page cursors take
`ic_db::Principal` directly. Ordering is length first, then bytes; the empty
management principal is a valid identity, distinct from an absent reference.
`db schema` shows the `PrincipalKey` example and its indexed self-reference.
Account/Subaccount primary keys use the same field declarations with
`prim = "Account"` / `"Subaccount"`. Account ordering is owner then effective
subaccount. An absent and explicit-zero subaccount always name the same key;
the database does not treat that alias as a second row or an update.
`db schema` shows `AccountKey` / `SubaccountKey` with mutual references.
An optional reference's `None` has no target; `Some(id)` requires a surviving
target. Unlinking the last dependant and deleting its target can share one batch.
Schemas no longer require numeric entity or index IDs. Entity names default to
the Rust type name; index names default to ordered fields such as `"category,id"`.
Optional `name` overrides provide stable explicit identities. References use
`references(entity = "Entry")`, with `index = "entry_id,score"` only when reverse
selection is ambiguous. Rust resolves imports and module paths; full logical names
define durable identity. The fixture now uses
IcyDB's two-stage approach: host-only `ctor` registration, followed by validated
schema assembly generated for the target. This is implemented for scalar entities;
named records and payload enums participate through typed fields. See the
[shared schema](qualification/records-schema/src/lib.rs) and its
[build script](qualification/records-canister/build.rs).
Records use `#[record(fields(...))]` and enums use
`#[enum_(variant(name = "Ready"), variant(name = "Data", value(item(is = "Details"))))]`.
Both retain the empty-struct authoring style. Fields use `value(item(is = "Type"))`;
optional values add `opt`. Recursive values can use `item(indirect, is = "Self")`
and need a finite base case. Encoding, decoding and stored-row inspection share
32-level, 256-structured-value and 4 KiB row bounds. Complete encoded secondary-index
keys are limited to 512 bytes, including namespace and any primary-key suffix.
Oversized keys refuse before mutation; non-indexed values retain their row/field
bounds. Semantics version 2 refuses older wider-key contracts intact. Lists use
`value(many, max_items = 3, item(prim = "Nat16"))`, or a named item type;
adding `opt` gives `Option<Vec<T>>`. Order and duplicates are preserved, with
at most 256 collection items across the entire row and an explicit per-list maximum.
Named scalar sets use `#[set(item(prim = "Int16"), max_items = 3)]` on an empty
struct, producing a tuple wrapper over `BTreeSet<i16>`. Fields refer to that type
with `item(is = "Scores")`. Stored elements must be strictly increasing in logical
order; duplicates and out-of-order data refuse. Sets share the collection budget.
`db schema` shows `ListRow`, recursive `ListNode`, and each reachable
record/enum/set/map definition once, including payload types and recursive links.
`SetRow` and its `Scores` definition are also available for inspection.
Maps use `#[map(key(prim = "Int16"), value(opt, item(prim = "Text", max_bytes = 4)), max_items = 3)]`
on an empty struct, producing a tuple wrapper over `BTreeMap<i16, Option<String>>`.
Keys are ordered scalars or unit enums; values may also be lists or named types. Stored duplicate
or descending keys refuse without overwriting values. Map entries share the row's
collection budget, and their nested contents consume the same budget. Inspect
`MapRow`/`Rates` and recursive `MapTree` with `db schema`.
`value(item(prim = "U256"))` uses `ic_db::U256` (ethnum), with exact 32-byte
big-endian storage and secondary-index keys. Optional values and collection
elements/keys retain all 256 bits. Inspect `BigNumbers` with `db schema`.
Scalar sums preserve U256 and explicitly refuse accumulator overflow.
`prim = "Float32"` / `prim = "Float64"` use finite `ic_db::Float32` /
`ic_db::Float64` wrappers. Construct with `try_new(value)`; NaN/infinities refuse,
while either signed zero becomes positive zero. Row bytes, ordering, uniqueness
and collection keys agree on that canonical value. Inspect `FloatRow` with
`db schema`. Aggregate arithmetic and cross-type coercion remain separate work.
`prim = "Account"` / `prim = "Subaccount"` use the re-exported Platform Types
`ic_db::Account` and `ic_db::Subaccount` (32 bytes). An account's absent and
explicit-zero subaccounts share one identity and encoding; decoding returns
`None` for that default. Account ordering compares the owner first, then the
effective subaccount. Optional fields and collections use the same rules.
Inspect `AccountRow` with `db schema`; the Info column shows byte widths.
`prim = "Ulid"` uses `ic_db::Ulid`, with exact 16-byte storage and ordered keys.
Construct supplied values with `Ulid::from_bytes`, `Ulid::from_u128`, or parse
canonical uppercase 26-character text. `db schema` shows `UlidRow`. Automatic
generation remains separate work; this value does not read
a clock or acquire entropy. The `ulid` dependency's default RNG feature is disabled.
Declare `pk(field = "id")` with `id` of `prim = "Ulid"` to use ULID primary keys.
`get`, `delete` and `page` then accept ULIDs, including typed continuation keys.
Inspect `KeyRow` and its mixed-key references with `db schema`.
`UlidGenerator::generate(operation_timestamp, randomness)` now offers explicit
issuance from a `Timestamp` and 10 host-provided random bytes. Equal/backward time
increments the preceding suffix; invalid time and exhaustion refuse unchanged.
The host owns the stream and any upgrade checkpoint via `last()` / `resume_after()`.
Resetting state can repeat IDs, and issuance before a normally refused batch can
leave gaps. Schema-owned automatic identity generation
remain separate work; no clock or RNG is introduced into the database.

Entity fields can now declare literal defaults, for example
`field(name = "count", value(item(prim = "Nat16")), default = 7)`.
The first supported defaults are integers, Bool and bounded Text. Construct a
typed insert with `batch.insert_input(MyEntity::insert().id(1))?`, then commit the
batch normally. The returned row is prepared, not yet persistent. Setters supply
explicit values; `.label(None)` keeps optional absence even if `label` has a
default. Omitted optional fields without defaults become `None`; missing required
fields refuse. Fully specified `batch.insert(&row)` remains available.
Defaults never run on replacement, decode or reopen. Changing a declared default
changes the schema contract; it does not silently backfill stored rows. Inspect
`DefaultRow` with `db schema`. Nested-record and primary-key defaults are not yet
supported; no existing authoring attributes were removed.

For database-managed time, declare nonoptional Timestamp fields with
`managed = "created_at"` or `managed = "updated_at"`. Each role may appear once,
independently of the other. Begin writes with `Batch::at(operation_timestamp)`;
the host supplies one captured time for the whole batch. The insert builder omits
managed setters, and direct fully authored inserts refuse for these entities.
For replacement, read the entity, change authorable fields and call `replace`.
Commit rejects changed or stale managed values, preserves creation and sets update
time only on a real change. Read after commit when the assigned update time is
needed. No-op replacements preserve timestamps and revisions; real changes with
a clock earlier than stored time refuse. Decode/reopen never assign time. Inspect
`ManagedRow` with `db schema` for the roles beside their units in **Info**.

Integer primary keys use the same `pk(field = "id")` declaration. Lookups, deletes
and page cursors use the exact Rust field type; references do not implicitly
widen or change signedness. `SignedKey` and `WideKey` demonstrate Int128/U256
primary keys and mutual references. Composite primary keys remain separate work.
`prim = "Date"` uses `ic_db::Date`, a Gregorian day from `0000-01-01` through
`9999-12-31`. Use `Date::try_new(year, month, day)`, `Date::parse("2024-02-29")`
or checked epoch-day construction. Four-byte storage and signed ordered keys
preserve dates before 1970. Inspect `DateRow` with `db schema`.
`prim = "Timestamp"` and `prim = "Duration"` use `ic_db::Timestamp` and
`ic_db::Duration`. Both store exact eight-byte millisecond values: signed Unix
milliseconds for timestamps, nonnegative milliseconds for durations. Use
`from_millis` for exact values or their explicit unit/text constructors.
Timestamp sub-millisecond conversion floors before 1970; duration sub-millisecond
conversion explicitly truncates. Value arithmetic saturates at the type boundary;
text parsing refuses overflow. Inspect `TimeRow` with `db schema`. Managed time
policies use `Batch::at` as described above; Timestamp/Duration aggregate
arithmetic remains separate work.
`value(item(prim = "Decimal", scale = 2))` stores exact decimal values at the
declared scale (0–28). Parse `"1.00".parse::<ic_db::Decimal>()` or use
`Decimal::try_new(mantissa, scale)`. Equivalent values such as 1.0 and 1.00 have
the same equality, hash and index key. Writes refuse excess precision or
coefficient overflow; they never round. Inspect `DecimalRow` with `db schema`.
Decimal aggregate arithmetic remains separate work.
Unit enums also support `#[set(item(is = "Status"), max_items = 3)]` and
`#[map(key(is = "Status"), value(item(prim = "Nat64")), max_items = 3)]`.
Their canonical variant-name order agrees with secondary indexes. Stored duplicate,
reversed or invalid variants refuse before collection construction. Inspect
`db schema EnumCollectionsRow` for an example. Payload-enum/nested/collection
indexes, nested references and other named collection keys/elements remain work.
IC DB owns its durable row codec and has no IC Wire dependency. Transport format
changes cannot change persisted decoding. The current experimental codec identity
is version 3, adapting IcyDB's owned fixed-width scalar approach; opening a
version 1 or 2 codec contract refuses without changing its data. The named schema
grammar is version 2; earlier numbered contracts also refuse intact.
Generated schemas reuse immutable executable metadata. Opening still checks the
current manifest; explicit revision reads and write preparation validate only
the counters they need. Unrelated broken counters do not block ordinary row
reads or other entities' writes. No revision values are cached across calls.
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
upgrade/reopen. It measures the frozen engine only; the separate
`make m1-records-comparison` provides the matched scalar results. Broader workload
budgets remain outstanding.

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

At the `db>` prompt, use `schema`, `status`, `query Entry --limit 3`,
`query Entry --min 40 --select id,amount`, `help` or `quit`.
For one command, try:

```sh
db query Entry --category 1 --order-by id --desc --select id,amount
db query Entry --min 100 --work 2
db query Document --entry-id 1 --order-by score
db query Document --select id
db query --help
```

`Entry` is the default entity. Copy the returned hexadecimal cursor into `--after`
with the same entity, filters, order and projection. The old `--after ID` syntax
is retired. Limits may change: `--limit` bounds returned rows (1–32), `--work`
bounds examined candidates (1–512), and `--bytes` bounds encoded results
(1–131072). An empty page can still have a next cursor. `--order-by` admits only
complete supported orderings; `--desc` reverses the whole order.
Installation defaults to `~/.local/bin/db`; keep `~/.local/bin` on `PATH`.
Override the prefix with `make db DB_INSTALL_PREFIX=/your/prefix`.

The current console reads an explicitly labelled, temporary M1 database with six
`Entry` rows, three `DecimalRow` samples, one record each for `Document`, `WideNumbers`, `BinaryObject`, `NarrowNumbers` and `IdentityRow`,
and an empty `OptionalLink` entity showing an optional self-reference,
running in local PocketIC. Each session starts fresh and is discarded on exit.
`db schema` presents generated fields and indexes in bordered tables, with the
primary key first and separate Type, Optional and Info columns. Info contains
byte/item/entry limits, including list-element bounds; type names stay uncluttered. The CLI
uses `ic-host-text`, as IcyDB's describe renderer did. `db status` contains the
full manifest fingerprint, incarnation, Entry revision and physical pages.
Use `db schema EnumRow` or `db schema Document` to select one exact declared
entity name. Only its reachable named types are included; recursive definitions
appear once and references retain their target names. Inside the console, use
`schema EnumRow`. Unknown names report an error; omit the name to see all entities.
`query` reads Entry, Document or DecimalRow through the fluent engine and displays the chosen
record shape, work counts and continuation cursor. Terminal layout belongs to
the CLI, not the runtime.
It has no SQL, remote-canister connection or production credentials. Broader
entity coverage remains required in the eventual product CLI;
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
