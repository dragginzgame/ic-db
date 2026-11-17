# ic-db

IC DB uses Storage 0.78.0 named stable-memory allocations and upstream stable collections. Storage owns its allocation directory codec. Version 0.78 rejects 0.77 backing. Existing data requires an export with the previous owner. IC DB owns its row, index and job codecs.

A fresh design workspace for a smaller embedded Internet Computer database.
This repository explores replacing IcyDB's accumulated mechanisms with a system
built around broad bounded fluent queries and familiar attribute-based schemas.
SQL, joins and window functions are excluded. This is a multipurpose IC database;
application-defined schemas and broad capabilities drive its design. The private
Toko GitHub repository is a secondary compatibility target, not the architectural
guide. The obsolete, deleted local checkout is not a design input.
It does not replace IcyDB yet.
`db inspect --provider URL --canister PRINCIPAL --identity KEY.pem` reads an
application's Canic inspection v1 catalogue. Add `--entity NAME --count` for its
schema and permitted maintained count, or `--json` for structured output.
Use `--anonymous` instead of a PEM only when intentionally selecting that caller.
`--timeout-secs` bounds each complete query, including verification reads (default
30, range 1–300). Busy providers refuse without automatic retries.
Loopback development providers need explicit `--local-root-key` trust. Remote
inspection never falls back to the temporary fixture. `db witness-list` reads one
bounded page from Canic's specific qualification application; other `query`
commands still use the local fixture. `make m4-inspection` qualifies the client through an
installed Canic witness and local HTTP gateway.
Ordinary Rust iteration handles result mapping; the Canic adapter owns product
endpoints. Broad schema types and constrained resumable aggregation are required.
Shared execution and measured monomorphization growth are explicit design concerns.

Start with the [proposed system design](docs/design/ic-db-system-design.md):
capability contracts, authoritative state, schema evolution,
query/write paths, experiments and conditions for proceeding.

The [IcyDB comparison matrix](qualification/README.md#comparison-with-frozen-icydb)
collects measured costs, crate/functionality differences and unmeasured gaps.
Recorded benchmark wins are scoped to their workloads, not whole-product parity.

IcyDB's local checkout and private [icydb-legacy](https://github.com/dragginzgame/icydb-legacy)
repository are frozen by maintainer decision pending this experiment.
Read-only research remains allowed; development and
delivery require an explicit decision to resume. Existing work and data are retained.

The first M1 product slice now lives in `crates/ic-db` (shared runtime) and
`crates/ic-db-macros` (attribute authoring and typed codecs). It creates and reopens
exactly admitted schemas, persists signed/unsigned 8/16/32/64/128-bit integers,
U256, Ulid, Date, Timestamp, Duration, Decimal, finite floats, Bool, Principal,
Account/Subaccount and bounded Text/Blob with optional
values, maintains indexes and entity revisions, and applies bounded final-state
batches with required/optional integer, Ulid, Principal, Account or Subaccount references and indexed restricted deletion. This is
experimental: there is no production compatibility promise or deployment.

M3 now has a first product maintenance boundary. Construct a
`SchemaTransition::new(&source, &target, "widen-amount", 1, fixed_inputs)`, then
consume the live handle with `db.begin_maintenance(&transition)`. Ordinary access
closes durably. `Maintenance::resume(memory, &transition)` requires the exact
identity; dropping its handle leaves the source frozen. Its pre-copy `cancel()`
restores the original manifest so source-compatible code can reopen.
`maintenance.source_page::<Source>(&source, after, 32, 128 * 1024)` reads frozen
entities through their explicit source decoder, in primary-key order. Continue
until `next` is absent. Its bounded cursors survive compatible resume but refuse
changed transitions or source revisions; they are read positions, not copy checkpoints.
`begin_copy` now binds a fresh host-provided target allocation and returns
`SchemaCopy`. Its `copy_next::<Source, Target>` transforms bounded pages, rebuilds
indexes through batch commits and saves progress atomically with each chunk.
Copy the whole database, preserving the named entity set and every primary key.
Use `next_entity()` to select the next typed transform in schema-name order;
`copy_unchanged::<E>` explicitly preserves an unchanged entity. Even empty entities
must be visited. `CopyStep.complete` means every entity is copied. References,
including cycles, validate against the complete target before publication; ordinary
writes retain their existing checks. Managed timestamps retain their values by
role, including explicit field renames; adding/removing roles or changing their
values refuses. Both allocations stay closed to ordinary access.
`cancel_step` removes bounded target entries before reopening source.
`validate_next` checks both collections in bounded passes; only after it completes
can `publish` atomically open target and retire source. It returns the target
database handle, and the host selects that allocation for subsequent opens.
Publication preserves source rows, indexes and revisions while closing its admission.
Once host retention obligations permit deletion, `RetiredSource::resume` requires
the exact transition and both incarnations; `cleanup_step` removes at most 32
retired entries per call and preserves the retired manifest. It never opens target.
Allocation reuse and physical shrink are not provided by cleanup.
Host adapters own access control. Run
`make m3-maintenance` for the focused native/Wasm probe.
`make m3-references` separately qualifies a 256-row migration with cross-chunk
reference pairs, delayed validation refusal and safe cancellation.
`make m3-jobs` checks retained aggregate access and independent job cleanup while
maintenance closes the source, then resumption after supported cancellation.
For a visible local example, `make db` then `db migration-demo` runs a fixed
six-row transformation, upgrades the temporary canister during copying, resumes,
validates, publishes and cleans the retired source. It widens `Item.amount` from
Nat64 to Nat128 and adds an indexed label; final rows come from that new index.
The existing query fixture stays separate. No deployed database is accessed.

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
clauses over top-level ordered scalar/unit-enum comparisons. `make m2-queries` checks its native/Wasm
boundaries. The local CLI constructs its documented subset of fluent operations.

Use `filter_any` for a bounded OR clause, combined with other filters using AND:

```rust
let page = db.query::<Entry>()
    .filter("category", ic_db::Comparison::Eq, 1_u64)
    .filter_any(&[
        ("amount", ic_db::Comparison::Lt, 20_u64.into()),
        ("amount", ic_db::Comparison::Ge, 50_u64.into()),
    ])
    .fetch()?;
```

Every comparison across `filter` and `filter_any` counts toward the same eight-term
bound. Empty OR clauses and oversized requests return `Error::Limit` at execution.
All operands are validated even when another alternative matches. One ordered scan
returns each matching row once; OR alternatives cannot establish fixed index
prefixes. Clauses also work with aggregates and with DISTINCT when its existing
whole-class predicate proof holds. Clause boundaries and operands bind cursors and
retained jobs. Nested boolean trees and index unions are not introduced by this API.

Presence filters also work on optional blobs, lists, records, enums, sets and maps:

```rust
let page = db.query::<ListRow>()
    .filter("labels", ic_db::Comparison::Ne, ic_db::IndexValue::Absent)
    .fetch()?;
```

`Ne(Absent)` selects present values, including empty containers; `Eq(Absent)`
selects absence. Required fields reject absent operands. This does not enable
ordering or value comparisons on otherwise unsupported types.

For membership on one field, use the equivalent equality shorthand:

```rust
let page = db.query::<Entry>()
    .filter_in("id", &[1_u64.into(), 3_u64.into()])
    .fetch()?;
```

The list must be nonempty and shares the same eight-comparison budget. Duplicate
members count toward that bound but never duplicate results. Optional absence
matches only an explicit `IndexValue::Absent` member. No member is coerced or
discarded before validation. The supplied order binds cursors/jobs, and the same
ordered equality clause can resume work started through `filter_in`.

Use `filter_not_in("id", &[1_u64.into(), 3_u64.into()])` to exclude listed values.
It is equivalent to consecutive `Ne` filters, including their cursor/job identity.
Absent fields remain eligible unless the list includes `IndexValue::Absent`.
The list must be nonempty; every member consumes the same comparison budget and
undergoes exact admission. Reordering it changes continuation identity.

Text fields also accept exact prefixes, suffixes and substrings:

```rust
let page = db.query::<Document>()
    .filter("note", ic_db::Comparison::StartsWith, "alpha")
    .filter("note", ic_db::Comparison::Contains, "preview")
    .filter("note", ic_db::Comparison::EndsWith, ".png")
    .fetch()?;
```

These comparisons are case-sensitive and perform no Unicode normalization. An empty
operand matches every present text value, including empty text; absence never
matches. The operand must fit the declared text byte limit. Other field types
refuse; `Contains` is substring search, not collection membership. They work with `filter_any` and with `filter_groups` on
completed text min/max results, including retained jobs. A mandatory `StartsWith`
can seek on the first index field after its fixed prefix, using exact escaped
UTF-8 bytes. Empty prefixes include present empty text and exclude absence.
`EndsWith`, `Contains`, OR alternatives and unseekable fields remain residual
filters under the chosen scan's work budget.

Mandatory primary-key `Eq`, `Lt`, `Le`, `Gt` and `Ge` filters seek directly to
their intersected range on primary traversal. For example:

```rust
let page = db.query::<Entry>()
    .filter("id", ic_db::Comparison::Ge, 100_u64)
    .filter("id", ic_db::Comparison::Lt, 200_u64)
    .fetch()?;
```

Rows outside that range consume no candidate budget and are not inspected for
corruption. Contradictory bounds return an empty result after validating all
operands. Selected secondary-index paths apply the same comparisons to the first
field after their fixed prefix. For index `(category, id)` with category fixed,
an `id` range therefore skips unrelated IDs. Equal values include every trailing
component and owner; optional ranges exclude absence. OR alternatives, later
fields and derived bounds exceeding 512 bytes remain residual filters.
Continuations intersect the range in either direction, including output-byte
retries. Counts and retained aggregates use the same traversal.

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
can fix index components; the engine prefers a path that proves the complete
order. Without one, it scans the complete input, retains the best requested
1–32 rows, then sorts them. Work exhaustion returns `Limit` even for `limit(1)`;
the maximum work allowance permits 511 candidates plus observing exhaustion.
Sort keys are capped at 4096 bytes and retained key/output
buffers at 128 KiB. This fallback returns no continuation and refuses `after`;
an explicit `using_index` still has to supply the requested order.
Both entity and projected results are supported, including ordering by fields
outside the projection. Byte limits apply to the selected output after sorting;
overflow refuses rather than returning a partial result. Without `order_by`,
the existing primary/explicit-index order remains unchanged.

For individual directions, use `order_by_each` with `(field, SortDirection)` pairs.
Its implicit primary key is ascending; `.descending()` reverses the complete
order. Constant fields may have different directions without preventing index
traversal. Otherwise mixed directions use the same bounded complete sorter.
Sorted `distinct()` accepts inherently unique results or an explicit order covering
exactly the projected fields. It removes duplicates before applying `limit` and
output bytes. Retain at most 32 distinct tuples within 128 KiB, using the existing
sort-key prefixes for equality. Duplicate input still consumes scan work; a 33rd
distinct tuple refuses even with `limit(1)`. No additional set or cursor is stored.

Sparse indexes (`present_only`) can supply order when filters exclude absence
for every optional indexed field. For example, `Ne` with `IndexValue::Absent`,
a range comparison or a text match proves presence; `Ne` with a present value
does not. Every alternative in a proving OR clause must exclude absence on the
same field. This also applies to `using_index`; incomplete paths still refuse.

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

Live DISTINCT pages require a leading index projection after any fixed index prefix.
Filters may read projected fields or fields fixed by that prefix, so each duplicate
group has one filter result. For example, add
`.filter("category", ic_db::Comparison::Ge, 1_u64)` to the query above.
Full entities and projections containing the primary
key are already unique and allow ordinary filters. Duplicate candidates
consume work, so an empty page can still continue.
Continuation preserves duplicate suppression; live pages do not provide a frozen
input for exact aggregates.

Other projections of ordered scalar/unit-enum fields use one bounded complete
scan, including nonadjacent duplicates and filters varying within a class:

```rust
let categories = db.query::<Entry>().distinct().fetch_as::<CategoryOnly>()?;
```

This returns the first matching occurrence of each tuple in traversal order,
with `next: None`. It retains at most 32 tuples, comparison keys of at most 4096
bytes and 128 KiB of key/row buffers. The query's `limit` is a result capacity for
this path: exceeding it, the combined output-byte budget, or the work budget
returns `Limit`, never a partial result. Even a scan ending exactly at the work
budget can refuse until exhaustion is observed. An individually oversized row
returns `RowTooLarge`. Cursors refuse; larger paginated DISTINCT remains later work.
Try `db query Entry --select category --distinct` in the fixture CLI.

Check existence without fetching an entity or counting all matches:

```rust
let result = db.query::<Entry>()
    .filter("amount", ic_db::Comparison::Ge, 40_u64)
    .exists()?;
```

`result.value` is true after the first validated match, or false after observing
the end of the input. Work exhaustion without a match returns `Limit`, including
an exact-boundary scan whose end has not yet been observed. The result occupies
one byte; `limit` does not truncate input. Like scalar counts, this terminal
refuses cursors, DISTINCT, input ordering and group modifiers. It validates every
examined row, but stops before inspecting rows beyond the first match.
Try `db query Entry --select exists --work 1 --bytes 1` in the local fixture.

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
using a bounded set of comparison keys. Its count is unaffected by `limit`.
`group_count_distinct::<KeyRecord, Record>()` applies the same rule within each
group. An all-absent group has one distinct tuple; repeated tuples count once.
Key and operand projections may overlap. Group filters/order apply to completed
Nat64 counts, before output limits. Each set retains at most 32 distinct keys;
all groups share the existing 128-KiB state allowance. Output filtering cannot
hide cardinality or state exhaustion. Scalar distinct counts retain no projected
records, and both forms use the same tuple codec and distinct reducer.

These operations finish synchronously or refuse. Completion must be observed
before the work cap, so a scan examining exactly the allowed number can still
return `Error::Limit`. Group state has separate caps of 32 groups, 4096 bytes per
comparison key and 128 KiB retained key/output bytes. Output byte exhaustion also
refuses instead of returning partial counts. Cursors, DISTINCT and explicit input order
are not aggregate compositions. Retained grouped operations and completed-output
pagination use the separate job contract below. Float arithmetic remains work.

Count and integer/Decimal sum and average also have retained variants. The host creates or
opens an `AggregateJobs` store in a separate governed allocation. For example,
given that store and host-captured timestamps:

```rust,ignore
let handle = db.query::<Entry>().start_count(&mut jobs, now, expires)?;
let step = db.query::<Entry>().work_limit(64)
    .resume_count(&mut jobs, &handle, 0, now)?;
// On the next call, reconstruct the query and supply step.sequence.
```

`start_sum::<Record>`/`resume_sum::<Record>` and
`start_avg::<Record>`/`resume_avg::<Record>` use the same result shapes as their
synchronous counterparts. Average retains the exact total and present count,
then divides and rounds once at completion to the declared Decimal scale with
ties to even. Both operation and result scale are bound to the job; a sum cannot
resume as an average. `JobOutcome` is `Running`, `Complete(value)`, `Invalidated` or `Expired`;
partial totals are never results. Older sequence numbers return saved progress
without advancing again. Any real source-entity change invalidates unfinished
work; completed results retain their original revision. Same-code reopen/upgrade
preserves jobs. Database/schema/query/algorithm/result mismatches refuse intact.
Handles carry identity, not authorization; the host owns caller checks and time.

`start_min::<Record>`/`resume_min::<Record>` and
`start_max::<Record>`/`resume_max::<Record>` retain scalar extrema using the same
exact single-field projection and ordering as synchronous `min`/`max`. Absent
operands are ignored; empty/all-absent input completes with `None`. Persist only
the winning record, reconstructing its comparison key on resume. Minimum and
maximum have separate identities. A provisional winner must fit the whole-job
state bound, but output bytes apply only to the completed winner.
`start_group_min::<KeyRecord, Record>`/`resume_group_min::<KeyRecord, Record>`
and the matching `group_max` start/resume methods retain one optional winner per
group. All-absent groups remain present with `None`; grouping and operand fields
may overlap. Finish all input before group filtering, ordering and the total
limit. These terminals return `GroupPage` and support `group_page` below, within
the same 32-group and 4096-byte whole-job limits.

Initial limits are 16 jobs per store, 4096 encoded bytes per job, a one-hour
maximum lifetime and one million examined candidates per job. Each advance uses
the query's 1–512 candidate limit and at most 64 KiB of source row/key bytes.
Output bytes bound the completed result. Expired jobs still occupy a slot until
`cleanup_expired(now, limit)` (1–16 removals) or `cancel(&handle)` releases it;
neither promises that underlying stable memory shrinks. These operations run
synchronously in an update message. A later trap rolls back job changes too.
`make m2-jobs` checks the native and Wasm contract. `db query` uses synchronous
aggregates. `db aggregate Entry --select category,min:amount --work 2 --page-size 1`
uses retained jobs, displays each call's progress and completed result pages, then
cancels the temporary job. Count, field/DISTINCT count and min/max/sum/avg of amount
are supported, optionally grouped by category. Document supports row/note/priority
counts, DISTINCT notes, note/priority min/max and priority/score sums, optionally
grouped by `entry_id`. DecimalRow supports row/field/DISTINCT counts and
min/max/sum/average of `value` or
optional `maybe`, optionally grouped by `zone`, using the existing scale-2 result
records. An all-absent field has present count zero and DISTINCT count one;
count thresholds are integers. All-absent scalar extrema/sums/averages return no value; grouped results retain
the group with `—`. `--min` and `--category` filter Entry rows; `--entry-id`
filters Document rows. `--group-min`, `--group-max` and `--group-order` select
completed groups. Either bound is optional; together they form an inclusive
range applied before ordering, total output limits and pagination.
Thresholds use the exact result type: signed sums accept negative values and
Decimal results accept fractions at the declared scale. Grouped note min/max
bounds are literal text, including empty strings, ordered by UTF-8 bytes without
normalization. They exclude absent results; `None` is ordinary text, not an absence
marker. Quote empty or whitespace-containing bounds in shell commands and at
`db>` (for example, `query Document --select entry_id,min:note --group-min ''`).
The console accepts single/double quotes and backslash escapes; it performs no
variable, wildcard or command expansion. Incomplete quoting refuses before any
database call and returns to the interactive prompt.
Integer fractions,
overflow and threshold text above 64 bytes per bound refuse. A valid inverted
range selects no groups. `--limit`
caps the total selected groups across pages, independently of `--page-size`;
it never truncates aggregate input. The CLI follows server cursors
without accumulating values itself. Each invocation uses the local disposable
fixture; this command does not connect to a deployed database. See `db aggregate
--help` for work, response-byte and group-page bounds.

`start_count_distinct::<Record>`/`resume_count_distinct::<Record>` retain an exact
count of projected tuples using the same equality as synchronous `count_distinct`.
Duplicates across steps count once, including an absent tuple; empty input counts
zero. At most 32 distinct comparison keys fit inside the existing 4096-byte whole
job frame. Saved keys are validated on resume. Cardinality/frame/output refusal
preserves progress; `limit` never truncates input or relaxes the retained-set bounds.

`start_group_count_distinct::<KeyRecord, Record>` and
`resume_group_count_distinct::<KeyRecord, Record>` retain those sets per group,
returning `Complete(GroupPage<GroupCount<K>>)`. The projections may overlap, and an
all-absent tuple contributes one to its group. Admit at most 32 groups and 32 keys
per group, with all sets and progress fitting the same 4096-byte job frame.
Group/operand projections and completed-group selection bind the job. Finish
input before filtering/order/limit; output selection never relaxes state bounds.

`start_group_count::<KeyRecord>`/`resume_group_count::<KeyRecord>` retain row counts
per exact grouping key, including absent keys. They share synchronous grouping's
equality, group predicates and ordering. `limit` selects completed groups and is
bound to the job along with `filter_groups` and `order_groups`; work and output-byte
allowances can change. Finish all input before returning `Complete(GroupPage<GroupCount<K>>)`.
Empty input completes with empty `rows` and no `next` cursor.

`start_count_field`/`resume_count_field` and
`start_group_count_field::<KeyRecord>`/`resume_group_count_field::<KeyRecord>`
take a field name before the job-store argument. They count present values of
any top-level stored type, including empty text, blobs and collections. All-absent
groups retain count zero. The field name binds the job even when required; switching
fields or using a row-count terminal refuses. Limits and checkpoint formats are
shared with row counts.

`start_group_sum::<KeyRecord, Record>`/`resume_group_sum::<KeyRecord, Record>` and
`start_group_avg::<KeyRecord, Record>`/`resume_group_avg::<KeyRecord, Record>` use
the same lifecycle and return `Complete(GroupPage<GroupValue<K, R>>)`. Each group keeps
an exact total and present count; absent operands do not enter either. All-absent
groups return `None`, distinct from a present zero. Averages round once at the
declared result scale. Finalize every group before filtering/order/limit, so an
overflow in an excluded group still refuses. Key/result types, operation and
result scale bind the job; output/state refusal preserves the saved checkpoint.

All retained grouped resume methods now return `GroupPage { rows, next }` rather
than a vector. Use `.rows` for ordinary Rust mapping while keeping `.next`.
Without `group_page`, the complete selected result must still fit the response.
On resume, `.group_page(size, after)` returns at most 1–32 groups; use `None` first
and then `page.next.as_ref()`. This setting is separate from the query's bound
total `limit`, and applies only to retained grouped resume terminals.

Pages enumerate the immutable completed result, so later source writes cannot
move groups across pages. Page size and byte allowance may change. Byte exhaustion
leaves the unreturned group for the next page; an oversized first group returns
`RowTooLarge`. The 28-byte `GroupCursor` is bound to the job handle and a validated
result position. Retrying it reads the same position, until expiry/cancellation.
It is a logical position, not an authorization token or proof of prior enumeration.

Retain at most 32 groups inside the existing 4096-byte **whole job frame**, including
progress, key-record bytes and reducer state. This is a tighter retained-state envelope than the
synchronous grouping budget; large keys/groups can therefore refuse with `Limit`.
Filtering or limiting output cannot relax it. Refused advancement leaves the prior
checkpoint intact. Groups use the same job collection, revision gate and cleanup;
comparison keys are reconstructed with the owned codec rather than stored twice.

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

`.filter_groups(comparison, operand)` adds an AND term to the completed aggregate
value. `.filter_groups_any(&[(comparison, operand), ...])` adds a nonempty OR
clause, ANDed with other group filters. All group comparisons share an eight-term
budget, separate from input filters; every operand is admitted even if another
alternative matches. Counts require `u64`; extrema require the exact source
type; sums require the declared result type. Absent extrema support equality/inequality; range comparisons require
presence. `.order_groups(GroupOrder::AggregateAscending)` or
`AggregateDescending` sorts completed groups, breaking ties by ascending group key.
Absent extrema sort first ascending, last descending. `GroupOrder::Key` is the
default ascending canonical tuple order; `KeyDescending` reverses that tuple,
including absence ordering, before output limits or completed-output pages.
The CLI exposes these as `--group-order key` and `--group-order key-desc`.
These modifiers refuse on row queries and scalar aggregates.

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

For example, retain groups with at most one or at least five rows:

```rust
let groups = db.query::<Entry>()
    .filter_groups_any(&[
        (ic_db::Comparison::Le, 1_u64.into()),
        (ic_db::Comparison::Ge, 5_u64.into()),
    ])
    .group_count::<CategoryOnly>()?;
```

The same filters work on retained grouped jobs. Clause boundaries bind the job;
a singleton OR clause has the same identity as an ordinary group filter.

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
That workload does not qualify rich types or broader reference/query combinations,
and does not complete M1. Secondary indexes now
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
[shared schema](qualification/ic-db-m1-records-schema/src/lib.rs) and its
[build script](qualification/ic-db-m1-records-canister/build.rs).
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
remains separate work; no clock or RNG is introduced into the database.

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
Decimal sums and averages use the exact arithmetic and result-scale contracts
described above; storage admission never applies aggregate rounding.
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
The public source repository is [dragginzgame/ic-db](https://github.com/dragginzgame/ic-db).
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

The [current delivery status](docs/design/ic-db-system-design.md#9-delivery-and-deletion-gates)
separates delivered M1–M3 capabilities from remaining qualification and integration
work. `make m0-icydb-baseline` provides the first
SQL-disabled comparison workload: persistent writes, indexed reads and unchanged
upgrade/reopen. It measures the frozen engine only; the separate
`make m1-records-comparison` provides the matched scalar results.
`make m2-aggregate-comparison` compares fluent category counts, sums and extrema;
the [receipt](qualification/README.md#matched-grouped-aggregates-019) records its
32-row scope and result-type differences. `make m2-count-comparison` compares
maintained whole-entity counts and indexed-prefix counts as rows grow; see
[the adopted count receipt](qualification/README.md#maintained-count-adoption-019).
The earlier scan-only comparison is retained separately; legacy still has the
measured populated-prefix count advantage.
Broader workload budgets remain outstanding.

The authoring comparison is complete. M1 keeps the familiar attribute-owned
entity/record declarations with empty struct bodies; alternatives may be explored
later. For a real executable declaration and write/reopen journey, see
[the record tests](crates/ic-db/tests/records.rs) and
[the CLI fixture](qualification/ic-db-m1-records-canister/src/lib.rs).
Run `make m1-records` for their focused native and Wasm checks.

## Database console

```sh
make m0-setup   # once: explicitly prepare the local PocketIC server
make db        # install/update the binary; does not start the console
db             # open the console
```

At the `db>` prompt, use `entities`, `schema`, `status`, `query Entry --limit 3`,
`query Entry --min 40 --select id,amount`, `help` or `quit`.
For one command, try:

```sh
db entities
db entities --json
db schema Document --json
db schema --json > schema.json
db query Entry --category 1 --order-by id --desc --select id,amount
db query Entry --min 100 --work 2
db query Document --entry-id 1 --order-by score
db query Document --select id
db query Entry --order-by amount --desc --limit 2 --select id,amount
db query Entry --order-by category:asc,amount:desc --limit 3 --select id,amount
db query Entry --order-by category:desc --select category --distinct --limit 1
db migration-demo
db query --help
```

`schema --json` writes one JSON document to stdout for a single invocation;
failures use stderr and a nonzero exit status. Entity selection includes its
reachable named types, while `schema_fingerprint` identifies the whole schema.
The `source` is `local_fixture`. Types and `info` contain the same display text
as the tables; this output is not a schema import format or remote API contract.

Each database call reports query/update kind, local call duration to two decimal
places and cycles charged below the command's results. JSON and failed-command
statistics use stderr. Cycles are the canister's observed balance
decrease, not an instruction-based estimate; compact millions also show the exact
integer. Queries in this fixture can charge zero cycles while still doing work.
Durations exclude simulator startup, fixture installation/seeding and balance
sampling. Start `db` once and enter several commands to reuse that setup.
Aggregates and migrations show each constituent database call separately.

For repeated commands, launch `db` without arguments. Wait for `db>`, then enter
`count Document`, `query Entry` or `aggregate Entry --select count --work 2`.
The same temporary canister stays open until `quit` or end of input, with a
one-hour total command limit for a server started by the console.
Use ↑/↓ to recall commands and ←/→ to edit them. Ctrl-C clears the current
input; Ctrl-D on an empty line exits. History retains the last 100 commands in
this session only. Interactive input accepts one command at a time; piped stdin
continues to accept multiple command lines.

`Entry` is the default entity. Copy the returned hexadecimal cursor into `--after`
with the same entity, filters, order and projection. The old `--after ID` syntax
is retired. Limits may change: `--limit` bounds returned rows (1–32), `--work`
bounds examined candidates (1–512), and `--bytes` bounds encoded results
(1–131072). An empty page can still have a next cursor. `--order-by` uses a complete
ordered path or the bounded complete sort above; `--desc` reverses the whole order.
Each field accepts `:asc` or `:desc`; omitted directions and the implicit primary
tie-breaker are ascending. The fluent equivalent is
`.order_by_each(&[("category", SortDirection::Ascending), ("amount", SortDirection::Descending)])`.
Installation defaults to `~/.local/bin/db`; keep `~/.local/bin` on `PATH`.
Override the prefix with `make db DB_INSTALL_PREFIX=/your/prefix`.

The current console reads an explicitly labelled, temporary M1 database with six
`Entry` rows, three `DecimalRow` samples, one record each for `Document`, `WideNumbers`, `BinaryObject`, `NarrowNumbers` and `IdentityRow`,
and an empty `OptionalLink` entity showing an optional self-reference,
running in local PocketIC. Each session starts fresh and is discarded on exit.
`db entities` lists declared entity names alphabetically with a **Columns** count.
The count includes the primary key and optional fields; nested fields are not
expanded. `--json` emits an array of `{ "name": "Entry", "columns": 3 }` objects
to stdout, with call statistics on stderr.
It uses the current local fixture's schema, including entities beyond the demo
query roster. `db count ENTITY` supports every listed entity and reads its
maintained metadata; an empty entity returns zero. For example, `db count SetRow`.
Main help alphabetizes commands, uses terminal colours (respecting `NO_COLOR`) and keeps
redirected output plain; detailed options remain under each command's `--help`.
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
The fixture uses no production credentials. Broader
entity coverage remains required in the eventual product CLI;
Canic will own its endpoint integration. See [probe details](qualification/README.md).
The installed fixture lives under the prefix's `share/ic-db/m1`; PocketIC still
uses this checkout's explicitly prepared `.tools/testkit-server` server.

## Development

Rust 1.100.0 and published Toko Tooling 0.45.0 are pinned. Read [AGENTS.md](AGENTS.md)
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

CI uses explicit per-job and per-step time budgets. Five successful main runs
reviewed on 2026-11-17 establish the current baseline (queue time excluded):

| Job | Observed maximum | Job budget | Tool setup / qualification budgets |
| --- | --- | --- | --- |
| Secrets | 7 seconds | 10 minutes | Scanner install 2 minutes; scan 2 minutes |
| Linux | 2m10s | 20 minutes | 5 / 5 minutes |
| Apple Silicon macOS | 3m18s | 25 minutes | 8 / 6 minutes |
| Intel macOS | 9m23s | 35 minutes | 14 / 8 minutes |

Checkout is bounded to 2 minutes, compiler preparation to 3 minutes. Shorter
work-step limits leave time for failure status, artifact upload and cleanup before
the job deadline. Status records list step outcomes, their budgets and whether
sanitized setup excerpts are available; an interrupted setup process may not
finish collecting its excerpt. Upload/report steps are bounded too. Hard job
termination or runner loss cannot guarantee an artifact; GitHub's runner log
remains the fallback. These limits bound hangs, not successful-run performance.
The workflow runs native scaffold checks, not the separately selected PocketIC
release qualification. Preserve this distinction when revisiting the budgets.

Timing sources: [run 48](https://github.com/dragginzgame/ic-db/actions/runs/43975586559),
[run 46](https://github.com/dragginzgame/ic-db/actions/runs/43946175012),
[run 44](https://github.com/dragginzgame/ic-db/actions/runs/43933647974),
[run 41](https://github.com/dragginzgame/ic-db/actions/runs/43909172411),
[run 39](https://github.com/dragginzgame/ic-db/actions/runs/43893625554).
These predate the pending setup-diagnostic wrapper; retain headroom and reassess
after hosted runs of the changed workflow. GitHub documents
[step and job timeouts](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idstepstimeout-minutes).

Compiler, tool and scanner setup failures retain sanitized diagnostics for seven
days, linked from the job summary. The wrapper uses core Perl before tool setup;
it preserves the original exit status and reports collection/upload failures
separately. It keeps a bounded output tail and at most ten selected attempt logs,
including hidden attempt directories, from the job-local tool/asset roots.
Symlinks, configs, binaries and environment dumps are excluded. Each excerpt is
limited to 64 KiB/300 lines, with oversized lines/logs explicitly omitted; total
retained text stays below 768 KiB. Known credential fields, token patterns, private
key blocks and sensitive environment values are removed before printing/upload.
Raw logs are never uploaded. No general guarantee can be made for arbitrary
unlabelled secrets in third-party output.

Run `perl scripts/tests/ci-setup.t` for the offline failure-path regression and
`actionlint .github/workflows/ci.yml` for workflow validation. These checks prove
local collection/status behavior, not hosted artifact upload or retrieval.

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

## Public source releases

`make release-patch`, `make release-minor` and `make release-major` use the same
shared runner and local adapter. They require clean committed source on `main`,
prepare the locked dependencies/tools and PocketIC server once, run the selected
native and installed contracts below, finalize version/changelog metadata, then atomically push `main`
and the annotated version tag to the public repository. `make release-resume
VERSION=X.Y.Z` recovers a specific interrupted attempt. There is no crates.io
upload or live-canister deployment. Creating the GitHub prerelease is a separate
delivery step after tag verification.

The release metadata editor is the explicitly prepared IC Build 0.84.1 tool.
Validation logs and the shared runner's recovery state remain local; generated
Wasm/server assets are not shipped as a portable production binary. Build the
fixture CLI from the source checkout with `make m0-setup` and `make db`.

### Validation scope and recovery

Development selects the affected check: docs use `make docs-check`, native engine
changes use the relevant Cargo test, and storage/upgrade changes select their
installed probe from the qualification guide. Release orchestration changes run
`make release-adapter-test`; CI includes it alongside the native checks. This is
consumer wiring qualification, not the upstream tooling test suite.

Tooling adoption checks the published snapshot against both Cargo locks with
`make toko-tooling-check`, admits prepared tools and exercises changed local
callers. Testkit adoption additionally runs the driver's focused Clippy and
server/terminal checks after explicit `make m0-setup`; launcher lifetime changes
select the ignored idle regression. These are development checks, not a release
or publication request. Generic Tooling and Testkit regression suites stay upstream.

The single product release includes the engine and macros. Their generated code,
fixture schemas, two Cargo locks and native/Wasm feature combinations make a
file-extension-based reduced release unsafe. Keep this complete selected roster
until narrower input/guarantee mappings are qualified. CLI-only development can
use focused checks; that does not make the release gate optional. Independent
performance/comparison workloads remain explicitly selected.

| Phase / owner | Actual checks and guarantees | Inputs / trigger |
| --- | --- | --- |
| Fresh admission / shared runner + local adapter | Clean source, branch/remote/public visibility, changelog and exact saved release intent | Every release and relevant recovery hook; no setup effects in preflight |
| Preparation / `release-setup` | Locked fetch for both workspaces; `install-tools`, `release-tools`, `m0-setup` | Once per validation attempt; failure withholds the offline gate |
| Native/target lint / `release-lint` | Root `clippy`; native driver/schema all targets; non-baseline Wasm libraries; fault, maintenance-v2, maintenance-faults and retained-console features | Product, macro/generated schema and qualification source; both locks, compiler, flags and selected toolchain |
| Native contracts / shared logger | `tools-check`, `fmt-check`, `test` (engine/macros with build feature), `doc`, `docs-check`, `m0-contracts` (numeric/schema/row/nested/query examples) | Each release; tools/source/docs and both workspace inputs |
| Installed M0 / fixture owners | `m0-core`: one artifact/session, entrypoints + rollback + batch + reads; separate `m0-aggregation` and `m0-maintenance` variants | Message boundaries, final-state constraints, pages, revision invalidation and interrupted migration; fixture source/features, server and compiler |
| Installed product / fixture owners | `m1-records-wasm`, `m1-commit-failures`, `m3-maintenance-wasm` (including `m3-jobs`), `m3-cli` | Generated types, product rollback, copy/cancel/publish/reclaim, retained jobs and CLI/terminal composition; exact built artifacts and native callers |
| Version/delivery / shared runner + IC Build | Two-lock metadata transaction, exact source diff, commit/tag/remote verification | After successful validation, or the recorded metadata/delivery recovery phase; no crates.io upload |
| Hosted CI | Secret scan once; `make ci` on Linux and both supported macOS architectures | Push/PR; independent hosted evidence, not a reason to repeat that whole matrix locally |
| Frozen comparison / explicit consumer selection | Pinned IcyDB schema/canister and comparison targets remain isolated | A changed comparison question or explicit benchmark request; excluded from routine release lint and gates |

Required stages execute once per attempt, fail fast, and retain raw logs plus
`timings.tsv` under `target/release-validation/<source>/validation.*`. Preparation
and offline validation have separate directories. An earlier pass is invalidated
before a new verification attempt; only complete success creates its source receipt.
Missing, changed or symlinked receipts refuse version preparation. Source and
destination checks still run after validation and at delivery boundaries.

If validation or evidence finalization fails, repair that failure first. This
adapter does not admit partial-stage reuse: retrying validation reruns preparation
and the complete roster, with a new log directory. Old logs are diagnostic evidence,
not permission to skip a stage. If no saved post-validation plan exists, retry the
original release command. After validation saved a plan, use `make release-resume
VERSION=X.Y.Z` for the exact attempt; metadata/delivery recovery repeats fresh
admission without running setup or database validation again. Changed source,
toolchain or validation configuration requires a fresh validation attempt, not
manual receipt editing. Uncertain pushes retain the shared runner's reconciliation
and remote identity checks.

`scripts/tests/release.t` has 63 passing assertions using the actual adapter,
shared logger, phase runner and Make recipes in a disposable repository. It checks
fail-fast, failure-log/timing retention, evidence-finalization failure, complete
retry, receipt refusal, source drift, all four M0 binaries, failed artifact builds
and baseline lint exclusions. The runner witness reaches a deliberately refusing
metadata owner, retains the `prepare` phase, and resumes it without setup or
validation. Changed visibility or a tampered receipt refuses that resume. Remote
observations, version selection, expensive leaves and metadata effects are
substituted; it creates no release commit/tag and forbids pushing. Generic runner
and metadata-transaction qualification remain with their shared owners.

Local Linux qualification on 2026-11-17 also ran all five installed tests from the
four M0 binaries through one real Testkit 0.109.0/PocketIC session, then through
four separate sessions. Both passed with the same retained pre-cutover Wasm
(`7c683c4f258c81e788399b9c8d01359d29e8c84b290d481a1af1f519cee12fe0`).

| Observation | Elapsed wall time | Scope |
| --- | ---: | --- |
| One server session / all four binaries | 8.71 s | Five installed tests; prepared tools and compiled driver |
| Four server sessions / one binary each | 20.49 s | Same five tests and retained Wasm; prepared tools and compiled driver |
| Runner fixture / complete validation through metadata refusal | 0.675 s | Three phases; expensive work and remote effects substituted |
| Runner fixture / metadata resume | 0.197 s | One phase; fresh admission, no setup or validation |

These are single local orchestration observations, not IC instruction/cycle
savings, cold-install measurements or whole-release speedups. Logs, timings,
artifact/lock identities and source observations are retained under
`qualification/target/evidence/release-flow/`; launcher evidence is under
`qualification/target/evidence/testkit/`. The initial sandbox startup refusal is
retained separately from the successful loopback-server runs. Hosted macOS
qualification remains CI-owned.

This closes the release-wiring qualification independently of the Storage port.
Release qualification builds fresh Wasm with the selected Storage supplier.
