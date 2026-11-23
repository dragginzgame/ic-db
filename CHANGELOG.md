# Changelog

## [0.28.0] - 2026-11-23

### ✨ Queries

- Fluent listings can pass an `Option<&QueryCursor>` directly to `.after_optional(cursor)`. Use `None` for the first page or the previous page’s cursor to resume. [Issue #28](https://github.com/dragginzgame/ic-db/issues/28)

### ✨ Schema

- Hard cut to compiled schema v1. Every store gets a new fingerprint, including scalar-only schemas. Opening 0.27.8 backing returns `IncompatibleSchema` before reading rows, and pre-cut cursors refuse. Export data with the old executable before switching if it must be retained.
- Typed operations reuse the exact immutable entity descriptor already admitted by the store. Independently supplied descriptors still undergo structural and transitive named-type checks.
- Schema admission checks recursive dependencies between records and enums. Scalar wrappers and collections provide base cases directly, reducing [cold schema construction](qualification/README.md#cold-schema-construction-on-compiled-schema-v1) on the 64-payload fixture by 44%. A checked Text type requiring nonblank content now refuses a zero-byte or zero-character limit.
- Cloning a compiled schema shares its entity roster and named definitions. Opening an allocation still checks its current durable manifest.

## [0.27.8] - 2026-11-23

### ✨ Schema

- A store can use more than 32 named value types. Admission visits reachable declarations and checks that each type can describe a finite value. Existing schema fingerprints and per-row limits are preserved. [Issue #27](https://github.com/dragginzgame/ic-db/issues/27)

### 🧪 Qualification

- Updated the mixed-lifecycle canister and its driver to Canic 0.259.1.

## [0.27.7] - 2026-11-23

### ✨ Schema

- `Schema::describe(Some("Document"))` exports an entity and its reachable types through Serde, including checked newtype rules and recursive links. Pass `None` for the whole store. [Inspection API](README.md#inspect-a-compiled-store-schema)

### 🔍 Queries

- Synchronous aggregates now finish when their final candidate uses the last unit of work. If another candidate remains, the query returns `Limit` without exposing a partial sum, count or group.
- Bounded sorting and collected DISTINCT also accept an exact fit, up to 512 examined candidates. A complete sort still refuses larger input with `limit(1)`.
- Indexed queries now assemble the owner lookup key in one allocation and check the row identity once. [PocketIC comparison](qualification/README.md#indexed-owner-lookup-in-the-0277-worktree)

## [0.27.6] - 2026-11-23

### 🛠️ CLI

- Releases now include the `ic-db-cli` crate. It installs the `db` binary. `make db` also installs the local fixture Wasm.

### 🔍 Queries

- `.exists()` returns false when every candidate fails its filters and the final candidate uses the last unit of work. If more input remains, it returns `Limit`.
- A primary-key or index range counted entirely from keys now succeeds when its entry count exactly equals `work_limit`. A larger range still returns `Limit`.

### 🎯 Qualification

- A migration copying 4 KiB rows across a canister upgrade now has a [recovery check](qualification/README.md#byte-limited-migration-recovery-0276-worktree) for a byte-limited checkpoint, a refused retry and an interrupted write. The same fixture measures stable memory through a second migration and verifies every row and index entry after each cleanup.
- The bounded fluent-query milestone now has a [closeout matrix](docs/design/ic-db-system-design.md#m2-bounded-query-closeout) covering projection, ordering, DISTINCT, grouping and retained jobs, with measured sort limits and per-entity Wasm growth.

### 🛠️ Tooling

- The product and qualification workspaces now use Toko Tooling 0.46.3 and its compact changelog layout.

## [0.27.5] - 2026-11-22

### ✨ Schema

- Text newtypes accept `validate(non_blank, max_chars = 50)`. Construction, Wire and stored reads enforce the same limits; text keeps its original spacing and Unicode encoding.
- Reusable schema values are available from `ic_db::types`, starting with `NonAnonymousPrincipal`. The `#[newtype]` macro supplies `#[serde(transparent)]` by default.
- Declare an owner as `item(is = "NonAnonymousPrincipal")` and construct it with `NonAnonymousPrincipal::try_from(principal)?`. Stored reads and Wire decoding reject anonymous owners. Adopting the checked type on an existing field requires a schema transition.

### 🔍 Queries

- `.first()` and `.try_one()` can finish when the final candidate uses the last unit of work. An incomplete search still returns `Limit`.

## [0.27.4] - 2026-11-22

### ✍️ Writes

- Use `session.insert_if_absent::<DiscoveryByWalletAndProject>(input)?` to insert on the first report and preserve the existing row on replay. The outcome is `Inserted(row)` or `AlreadyPresent`. Conflicts on other keys still fail.

### 🔍 Queries

- Ordered index and primary-key scans now finish `.first()` directly, without copying the scan position into a continuation that the caller cannot use.
- IDs from `row.id()` can be sorted or used directly as map keys and set members when the primary key supports the operation.
- `get_many::<E>(&ids)` accepts a slice of entity-typed IDs from `row.id()` as well as raw keys. For a bare empty slice, specify the element type.
- Use `db.get_unique(&key)?` with a standalone database or a session read guard. It accepts the same generated keys as the session lookup.
- `Database::page()` omits the continuation when the final row exactly fills a primary-key page, saving an empty follow-up call.
- `get_many()` looks up each distinct primary key once per call, including missing keys. Repeated IDs still return a result at every requested position.
- Ordinary ordered `.try_one()` validates the requested order and skips a fallback sort. A large field can now exceed the sort-key limit and still allow a sole matching row to be returned.
- DISTINCT can page a selected column in the order of an indexed record member when a mandatory equality ties them together. For example, with `bucket == rating.score`, ordering by `rating.score` groups duplicate buckets. The same equality can justify a bounded sort.
- `.try_one()` reports `MultipleMatches` when it proves a second result, even at the work boundary or when only the first result fits in the response. This applies to bounded sorts too. With DISTINCT, the two projected values must differ.
- Use `.select(|e| e.id).first()` for the first matching ID or `.try_one()` to require at most one match. Both also support selected tuples.
- An ordered DISTINCT lookup can finish at its first validated match when every selected field is fixed by the query. This also works when ordering all matching rows would require a sort.
- DISTINCT can return a record-valued column when a primary or unique key proves the result is already distinct. Partial keys still require comparable projected fields.
- A paginated DISTINCT scan finishes as soon as it returns the only class its index prefix or filters allow. Sparse scans still return a continuation while searching for the first match.
- DISTINCT recognizes a selected column fixed through a mandatory equality with another column. With a fixed index prefix, sparse matches can now be paged one candidate at a time.
- When a filter requires two columns to be equal, ordering by either can page their DISTINCT pairs on a matching index. Both selected values are returned. A selected column can also stand in for an equal primary-key or unique-index column. Compound keys require every component, and sparse indexes require proof that every matching row is indexed.

## [0.27.3] - 2026-11-22

### 🔍 Queries

- Select a single field with `.select(|e| e.id).fetch()` or a tuple with `.select(|e| (e.id, e.amount, e.category)).fetch()`. Both return bounded pages with cursors and infer the selected Rust types.

### ✨ Schema

- A nonunique index can cover a scalar inside a record, even when the record, the scalar or both are optional, for example `index(field = "price.amount")`. A missing price and a price with no amount sort together before present amounts. Assigning or clearing a price updates the index in the same batch as the row.

## [0.27.2] - 2026-11-22

### 🔍 Queries

- Conflicting prefix or suffix filters on Text and Blob can finish with zero candidates examined when another filter fixes the comparison field. For example, if `code = "ab"`, a name cannot start with both `code` and `"é"`.
- Counts can use maintained cardinality when an integer or unit-enum filter covers every value of a required field. For example, `.gte(Item::LANE, 0_u8).count()` examines zero candidates for a required Nat8 lane. A strict bound beyond either endpoint finishes empty, including on optional fields.
- Two bounds can fix a compound-index component to one unit-enum variant. With variants `Archived`, `Draft`, and `Ready`, a query for `state > Archived` and `state < Ready` can page in index order within `Draft`.
- Strict bounds between consecutive integers or unit-enum variants finish empty without examining candidates, even when the listing uses another field for ordering.
- An explicit index prefix can supply the value for a field equality. On a `(lane, rank)` index restricted to lane 1, `rank = lane` seeks directly to `(1, 1)` and pages by ID in either direction. Adding an exact ID seeks the complete key.

### 🛠️ Tooling

- Both workspaces use Toko Tooling 0.46.2 for coordinated Cargo release metadata and publication evidence paths.

## [0.27.1] - 2026-11-22

### 🔍 Queries

- Use typed IDs directly in fluent predicates, for example `.eq(Route::USER_ID, user.id())` or `.filter_in(User::ID, &ids)`. They compare exactly like the underlying keys.
- An inclusive upper bound on the minimum or lower bound on the maximum fixes an integer or unit enum to one value. For example, `.lte(Item::LANE, 0_u8)` can page by rank on a `(lane, rank)` index. Enum extrema follow canonical variant names.
- A pair of integer bounds can also fix a compound-index prefix. For example, `.gt(Item::LANE, 0_u8).lt(Item::LANE, 2_u8)` pages by rank within lane 1.
- A membership filter and exclusions can fix the same prefix. With a `(lane, rank)` index, `.filter_in(Item::LANE, &[0_u8, 1, 2]).ne(Item::LANE, 0_u8).ne(Item::LANE, 2_u8)` can page by rank in lane 1.
- Two `filter_in` calls on the same field can also supply a compound-index prefix. Lists `[0, 1, 2]` and `[1, 3]` fix lane 1. Disjoint literal lists finish empty without examining rows.
- Range filters can narrow a membership list to an index prefix. With `.filter_in(Item::LANE, &[0_u8, 1, 2]).lt(Item::LANE, 1_u8)`, a `(lane, rank)` index can page by rank in lane 0.
- A listing restricted to lane 0 can use a present-only price index for `price is present OR lane = 2`. If the lane-2 alternative can match, the planner still requires a path that includes unpriced rows.
- Sparse-index admission also checks field-to-field alternatives. With lane fixed to 0 and rank to 2, `price is present OR lane > rank` can page through the present-only price index.
- Filtering an optional record as absent also fixes its direct scalar members to absence. With `price` absent, `prefix != price.currency` can use a present-only prefix index in either operand order.
- A present-only index can also serve an equality between two optional fields when the filters prove the other field is present. If two absent values can still match, automatic selection keeps those rows.
- With `rank = 1` and `lane = rank`, the planner can seek the `(1, 1)` prefix of a `(lane, rank)` index. Both field-comparison operand orders use the same proof.
- When a field equality fixes the primary key, row queries seek that ID directly and still check the other field on the stored row. A missing ID completes with zero candidates examined.
- Membership counts and existence checks skip IDs ruled out by a mandatory field equality before probing storage. A fixed category index can also seek the matching owner ID directly.

## [0.27.0] - 2026-11-22

### ⚠️ Breaking

- Use `db.batch()` or `Batch::new()` for managed writes. Each batch captures one timestamp. Remove explicit `batch_at(time)` and `Batch::at(time)` calls. Single-row session `replace` now returns the stored row, including its assigned update time.
- Delete succeeds when the primary key is absent. Surviving references still refuse the whole batch. Entity-typed `Id<E>` values work with direct lookups and deletes.
- Restart projected cursors and aggregate jobs whose query binding includes a field mask after upgrading.

### ✨ Schema

- Entities and records support 32 fields, including managed timestamps. A list of primary keys may declare `references(entity = "Target")`. The engine checks each distinct target and restricts its deletion.
- The encoded row limit defaults to 4 KiB. Set `max_row_bytes = 32768` on an entity to opt into a larger row. Changing this limit changes that store’s schema fingerprint.

### 🛠️ Tooling

- The private CLI fixture belongs to the qualification workspace at `0.0.0`. Both workspaces use Toko Tooling 0.46.1 so release verification can resolve the CLI’s dependency on that fixture. [IC DB #23](https://github.com/dragginzgame/ic-db/issues/23) · [Toko Tooling #403](https://github.com/dragginzgame/toko-tooling/issues/403)

## [0.26.12] - 2026-11-22

### ✨ Writes

- Typed inserts generate an omitted primitive ULID primary key synchronously.
  Supply the key explicitly to preserve an existing identity. The session returns
  the inserted row, including its key.

### 🔍 Queries

- Ordered queries can page through a unique index when its varying fields lead
  the requested order in one direction. For example, with a section fixed on
  a unique `(section, seat)` index, descending seat and ascending ID can use
  a reverse scan. Each matching seat already identifies one row.
- Equal inclusive bounds can fix a compound index component. For an index on
  `(lane, rank)`, `filter_between(lane, 0, 0)` can page by rank within lane
  zero. Separate mandatory `>= 0` and `<= 0` filters give the same proof.
- A listing with contradictory mandatory bounds on one field can finish empty
  without examining candidates, even when another index supplies its order.

### 🖥️ CLI

- The main workspace’s private `ic-db-cli` crate owns the `db` executable and its command tests.
  Install it with `make db` and run `db entities` to list the fixture schema.

### 🎯 Qualification

- On installed Wasm, a three-row page ordered by unique Decimal descending
  and ID ascending examined three rows, down from 28 on 0.26.11. The cursor
  resumed after upgrade and the full listing matched SQLite.
  [Instructions and Wasm size](qualification/README.md#unique-index-ordering-before-02612)
- With equal entry-ID bounds, the first 20 score-ordered documents took
  82.3% fewer instructions on the listing fixture. Requiring publication
  made the same page 94.8% more expensive. Both listings could resume in
  five-candidate steps. [Matched listing costs](qualification/README.md#equal-inclusive-bounds-on-ordered-listings)
- Adding a publication-aware index alongside the score index reduced the
  published listing from 80 owner reads to 20 and kept both listings pageable.
  A publication toggle cost 39.5% more on this fixture.
  [Index and write-cost comparison](qualification/README.md#index-choice-for-equal-bound-listings)

## [0.26.11] - 2026-11-21

### Queries

- On a non-unique index such as `(category, id)`, fixing the category lets
  counts and existence checks probe a list of IDs directly. Each distinct ID
  costs one probe, including a missing ID or one belonging to another category.
  On an explicitly selected index, equivalent Account IDs with an omitted or
  all-zero subaccount also fit a single-probe count.
- For a list of IDs, `exists()`, `count()` and `count_validated()` can now
  return an empty result without probing keys when other required filters
  contradict each other, such as a payload length both below and at least three.
- A zero-byte length filter on Text or Blob can seek the present empty value
  through an index. This covers length equal to zero, at most zero, or less
  than one. Automatic and explicitly selected indexes both restrict the range
  to the present empty value. In an OR filter such as byte length < 1 or text
  equal to `"A"`, the index scan ends at the largest alternative and checks
  intervening values. Absent values are excluded.

### CLI

- The local fixture accepts `db query Entry --select distinct-count:amount`
  and `db query Document --select distinct-count:note` in a single query call.
  An absent note contributes one distinct value. Use
  `db query Document --select count:note` to count rows with a present note.
- Entry category counts now use the same selector in single-call and resumable queries.
  Try `db aggregate Entry --select distinct-count:category --work 2` or
  `db aggregate Entry --select category,distinct-count:category --work 2 --page-size 1`.

### Qualification

- Compared installed Wasm against 0.26.10. With a fixed category and five
  candidate IDs, first-match existence used 17.1% more instructions for an
  early hit and 33.8% fewer for a late hit. Counting two empty indexed titles
  among eight used 68.8% fewer instructions. [Matched costs](qualification/README.md#filter-costs-before-02611)
- Verified live pagination of indexed empty text across a database reopen.
  A page ending on a filtered-out row can resume after that row leaves the index
  range. Rows entering ahead of the cursor remain visible to the resumed query.
- Verified that a retained DISTINCT count over 40 different amounts preserves
  its checkpoint when the 33rd value exceeds its 32-key bound. After cancellation,
  a new enumeration returns all 40 values and rejects the old cancellation handle.
- Checked a top-three grouped DISTINCT query against SQLite on 128 rows.
  A retained query examining seven rows per step returned the same ranked
  groups as the single-call query, including the tie at the cutoff.
- Verified on installed Wasm that a grouped distinct-count ranking resumed
  after a same-version canister upgrade and matched SQLite. [Measurements](qualification/README.md#sqlite-reference-checks)

## [0.26.10] - 2026-11-21

### Queries

- Boolean ranges that select one present value can use it as a fixed compound
  index field. For example, `published > false` can page distinct categories
  and amounts through an index on `(category, published, amount)`.
  On a required Boolean, `!= false` and `!= true` also fix the index field.
  On an optional Boolean, either inequality can also match an absent value.
- Counts for impossible Boolean ranges return zero without examining rows.
  On a required Boolean, `>= false` and `<= true` can use maintained cardinality.
  On an optional Boolean, these comparisons count only present values.
- A membership filter containing both Boolean values covers a required field's
  entire range. On required unit enums, the planner likewise recognizes the
  first and last variants in canonical name order as the scan boundaries. A narrower index
  can serve the aggregate while membership still filters out unlisted variants.
  On an optional field, membership from absence through the largest present
  value also spans its ordinary index. On a present-only index, the first and
  last present values span the index. Aggregate scans can still use that index
  to skip absent rows when there is no narrower range.
- An inclusive bound covering the entire primary-key domain, such as
  `id >= 0` on a `Nat64`, lets aggregate scans use a narrower secondary
  range. The same applies to membership lists spanning both key extremes when
  the operation scans rows. Counts that can probe individual IDs retain those
  probes. Exact IDs and restrictive primary-key ranges keep their existing seeks.

### Qualification

- Repeated the installed-Wasm query matrix against v0.26.9 and frozen IcyDB.
  The planner changes added 941 bytes to the query host. Across 2,340 IC DB
  executions, the largest measured instruction increase was 0.0412%.
  [Results](qualification/README.md#planner-checkpoint-after-0269)
- `make m2-ranked-listing` checks a filtered project listing against SQLite on
  installed Wasm. It covers automatic index selection, sparse continuation
  pages, timestamp ties and the work boundary for a globally ordered top two.
  It also measures how time, price and equality indexes affect first-page work
  with zero, sparse and dense price matches, including automatic selection when
  both price and time indexes are declared.

## [0.26.9] - 2026-11-21

### Breaking storage change

- Aggregate jobs accept only the `ICDBAJ02` job allocation. Opening an
  `ICDBAJ01` allocation returns `InvalidJob` and preserves its contents.
  A host that needs old results must retrieve them under its old executable.
  To start fresh, use a new job allocation and incarnation.

### 🔍 Queries

- For Text and Blob, an empty `StartsWith` or an inclusive lower bound of
  empty matches every present value. Aggregate planning can use a narrower
  index on another field. A strict upper bound of empty examines zero rows.
  An inclusive upper bound of empty fixes the field to the present empty value.
- When a membership list spans a field’s full encoded range, aggregate planning
  can choose a narrower index on another field. Membership still filters each
  candidate, including values between the listed endpoints. Row-scanning
  operations such as `count_field()` use this rule on unique indexes too.
- A membership filter containing only copies of one value can fix a compound
  index component. For example, `tenant IN [1, 1]` and `code = 20` can seek
  the unique booking directly.
- `filter_in(...).count()` can count from a unique compound index when the
  requested values cover a complete range on its leading fixed-width field.
  Each matching entry counts, including entries that share that leading value.
- Two required literal prefixes or two required literal suffixes on the same
  Text or Blob field can rule out a query before it reads any rows. For
  example, a currency cannot end in both `SD` and `UR`.

## [0.26.8] - 2026-11-21

### Schema inspection

- Enable the optional `wire` feature to derive endpoint manifests for DTOs
  containing IC DB scalar values, including the fixed 16-byte `Ulid`. Add
  `wire` to schema declarations selected for inspection. See the
  [type inventory](README.md#inspect-application-types-with-ic-wire) for the supported layouts.
- `ic_db::U256` now wraps ethnum’s integer and supports Wire inspection.
  Serde retains hexadecimal quantity text and storage retains 32 big-endian
  bytes. Code using the ethnum type directly can convert with `.into()`.

### 🔍 Queries

- Endpoint page arguments and replies can use `Option<QueryCursor>` with the
  existing length-prefixed byte encoding. Wire inspection exposes opaque bytes.
- `filter` and `filter_fields` accept `EndsWith` on direct Text and Blob record
  members. The suffix can be a literal or another field on the same row.
  Both use the existing bounded row comparison.
- Combined length filters use the field’s declared maximum to skip impossible
  scans. For a three-byte field, a length greater than two and unequal to
  three now finishes with zero candidates examined. A required literal prefix
  or suffix, or a required literal substring, also rules out lengths shorter
  than the pattern’s byte length. This also works when an equality or index
  prefix fixes the pattern field. List and set membership rules out an empty
  collection for both literal and field operands.
- A count using an indexed `filter_in` with repeated copies of one value can
  count every matching owner from its index entry, including on record-member indexes.
  Nonunique indexes can also answer directly when the list includes every
  fixed-width key between its smallest and largest members. The planner checks
  coverage directly against the supplied keys without allocating a scratch key.

## [0.26.7] - 2026-11-21

### 📦 Dependencies

- Update the qualification canisters, host driver and local CLI to IC Wire 0.28.0.

### 🔍 Queries

- `filter_fields` accepts direct scalar members of records on either side of
  equality and ordering comparisons. Text and Blob prefix comparisons can also
  take either operand from a record. List and set membership filters can take
  the value to find from a record on the same row. An absent record or member
  produces no membership match.
- Row filters compare decoded scalar values, Text and Blob bytes, and list or
  set elements directly. This avoids building an index key for every comparison.
- Counts use schema bounds and contradictory length predicates to skip scans.
  Self-comparisons and empty Text or Blob patterns can use stored cardinality
  when every row matches. When only present values match, counts can use a
  present-only index or seek past absent entries in an ordinary index.
- Equality between fields can use a present-only index when the other field
  must be present by declaration or filter. An index prefix can fix one side
  of the comparison, including inequality against an absent value.
- For bounded exact-key membership, `count()` probes each distinct candidate
  that survives the index prefix, range and filters. It skips keys exceeding
  the 512-byte encoded limit and fetches an owner row only when a remaining
  filter needs it. `exists()` stops at the first validated match.
  `count_validated()` checks the owner of every hit.

## [0.26.6] - 2026-11-21

### ✨ Writes

- Single-row session `insert_input` and `replace` automatically capture the time
  for database-managed timestamps. Replacing an unchanged row preserves its update time.

### 🔍 Queries

- `filter_length` accepts a direct Text or Blob member of a record. It counts
  UTF-8 or raw bytes and keeps absent values distinct from present empty values.
- `filter(path, Comparison::StartsWith, prefix)` now accepts direct Text and Blob
  members of a record. A matching member index can serve an ordered prefix scan
  or count matching entries without fetching their owner rows.
- `filter_present` and `filter_absent` on a top-level optional scalar use its
  validated presence directly, avoiding a comparison-key allocation for present values.
- Query scans decide once whether field offsets are needed and reuse the offset
  buffer across rows. Each row still passes full codec validation.
- Bounded sorts keep their best rows in order during the scan. Sorted DISTINCT
  uses the same binary search to recognize an already retained tuple. Once the
  window fills, a single comparison can discard a candidate at or below the cutoff.
  Scanned rows reuse the comparison buffer. Only retained keys get an owned copy.

### 🎯 Qualification

- Compare live nested-index pagination with SQLite after deleting the cursor row,
  moving two rows across its position and reopening the database.
- Verify that a descending nested-index query resumes through consecutive empty
  pages when its work budget allows one candidate per call.

## [0.26.5] - 2026-11-21

### ✨ Features

- Capture the current millisecond database timestamp with `Timestamp::now()`. [#20 — Add Timestamp::now() for application database writes](https://github.com/dragginzgame/ic-db/issues/20)

### 🔍 Queries

- Grouped summaries ordered by count compare the completed counts directly,
  saving one allocation per group. Tied counts still use ascending group-key order.
- Scalar aggregates that use decoded values can skip recording each field's
  byte offsets during a primary scan. The scanner still validates the full row.
- Synchronous grouped DISTINCT counts omit fields fixed by the query or group
  from each group's comparison keys. The full group key still appears in the result.
- Synchronous scalar `count_distinct` now uses the same fixed-field proof as
  projected DISTINCT. A fully fixed tuple counts once if any row matches.
- Bounded complete DISTINCT now compares only projected fields that can vary.
  Fields fixed by equality or an index prefix still appear in the output and
  count toward its byte limit.
- Bounded sorts omit order fields fixed by mandatory literal equality from
  each row’s sort key. A large fixed Text or Blob can precede the varying sort
  fields without consuming the 4 KiB comparison-key budget.
- A bounded DISTINCT sort can order by a nested scalar before the projected
  fields when a mandatory literal equality fixes that exact member.
- `filter_present` and `filter_absent` on a direct scalar member now skip
  construction of a comparison key after reading the validated member.
- Flat and nested enum index queries now validate the operand's variant list
  once and compare its full definition with the indexed field.
- A nonunique index can select one required scalar in a required record, including bounded Text, Blob,
  primitive-backed newtypes and unit enums,
  e.g. `index(name = "by_reading", fields = ["first.right"])`. Fluent equality can seek it automatically.
  Range comparisons on that path can bound an ordered scan or an index-only count. Changing either part of the path requires a schema
  transition even when the index keeps its name.
- Use the same nested scalar path with `order_asc`, `order_desc` or mixed-direction
  ordering. The bounded sorter returns a terminal result after scanning the complete input.
- Scalar literal filters can select a direct member of a named record.
  Use `.gt(ic_db::field_path!(Product::PRICE, Price::AMOUNT), amount)`.
  An absent parent or member makes the selected value absent. Record fields
  now generate the same name constants and collision diagnostics as entity fields.

### 🎯 Qualification

- Checked nested OR filters against SQLite when sibling fields match different
  rows or both match the same row. Exact counts enforce the candidate budget.
- `make m2-nested-path-costs` compares Wasm size at 1, 8 and 32 reachable
  record paths and measures flat and nested filtering and sorting in PocketIC.
  A matched scalar index control also reports lookup and batch write costs.

## [0.26.4] - 2026-11-21

### Queries

- A bounded sort can now use a filtering index even when the requested order
  uses a different field. It returns the top rows after exhausting the selected
  range, or `Limit` if the scan cannot finish.
- Retained row counts, present-field counts and distinct tuple counts now share
  the same start and resume path. Existing count checkpoints remain readable.
- Retained scalar min, max, sum and average now check the output-byte budget
  before decoding the completed result, using the same check as synchronous queries.
- Ordering groups by a completed aggregate with `.limit(n)` now buffers at
  most `n` qualifying groups. Ties go to the smallest group keys in either
  sort direction, including ties at the cutoff.
- Restoring a grouped minimum or maximum now validates each winner directly
  from the checkpoint and copies its record once into the reducer.
- Retained DISTINCT pages in ascending or descending key order now walk to the
  cursor once and collect results as they check the output-byte budget.
- Retained group and DISTINCT pages use the shared byte-budget check once per
  candidate. A partial page can retry its first unreturned row at the next cursor.
- In ascending and descending group-key order, `.limit(n)` now buffers at most
  `n` qualifying results during finalization. Every group still undergoes
  overflow and state-limit checks, including groups beyond the limit.
- Grouped summaries encode comparison keys for sum and average only when a
  group filter or sort uses that measure. Both results still undergo their
  normal range and rounding checks for every group.
- Scalar `sum` and `avg` skip comparison-key encoding entirely. Their grouped
  forms encode a value key when filtering or sorting by the aggregate result.

### Qualification

- Measured automatic exact listing totals with the original, additional and
  replacement compound indexes. At 530 documents the matching index counted
  440 published matches without fetching rows and stayed within one call.
- Checked a live listing and its exact filtered total under one session read
  guard against SQLite, including a write between pages that changes both
  the order and the total.
- Checked a filtered listing with 530 candidates against SQLite on PocketIC.
  The ordering index paged through all matches. The unindexed sort refused
  when it could not establish the complete order within one call.
- Checked ranked Float64 summaries in PocketIC across upgrades when rounded
  averages tie at both the page boundary and the query limit. Raising or lowering
  the query limit on resume refuses and preserves the original checkpoint and cursor.
  The same cursor supports smaller or larger pages across an upgrade.
- Updated the mixed-lifecycle qualification canister and its driver to Canic 0.246.0.
- Checked grouped summaries across a write that changes which groups rank in the top two. A running
  job invalidates, a completed job keeps its original pages, and a fresh query
  returns the updated counts and sums in SQLite order.

## [0.26.3] - 2026-11-20

### Queries

- `group_summary::<Key, Sum, Average>()` returns the present count, sum and
  average of one numeric field per group in a single synchronous scan.
  Use `start_group_summary` and `resume_group_summary` for bounded steps and
  pages over the completed groups. Use `order_groups(GroupOrder::Summary(measure,
  direction))` to rank by count, sum or rounded average. Ties use ascending
  group keys. Chain `filter_summary(measure, comparison, value)` to filter
  completed measures before ranking and limiting. Use `filter_summary_any`
  for alternatives, such as a minimum count or a minimum total.
- Grouped aggregates now discard each result that fails its group filter as soon
  as it is finalized. Every group still undergoes overflow and resource checks.
- Rows joining an existing group reuse the scan buffer for key lookup. Each
  new group retains exactly its encoded key bytes under the 128 KiB state limit.
- Scalar and grouped DISTINCT counts also reuse the scan buffer on duplicates.
  Each new tuple is charged its encoded length, including any escaped bytes.
- Complete DISTINCT projections now use the same bounded key representation as
  DISTINCT counts. Duplicate rows reuse the scan buffer, and results retain
  their first-occurrence order.
- Group keys, DISTINCT tuples and projected output now visit only the fields
  selected by the query after row validation.

The local console supports summaries for both `DecimalRow.value` and
`DecimalRow.maybe`. A group with no present values displays count zero and
a dash for each absent measure.

To compare grouped summaries in the local fixture:

```sh
db query Entry --select category,summary:amount --group-min count:3 --group-order avg:desc
db aggregate Entry --select category,summary:amount --group-max sum:120 --page-size 1
db query DecimalRow --select zone,summary:maybe --group-max count:0
```

### Qualification

- Updated the product and qualification workspaces and the shared tooling snapshot to Toko Tooling 0.46.0.
- Compared synchronous and retained Decimal summaries against SQLite for all
  six combinations of measure and sort direction.
- Verified that an interrupted Float64 grouped summary resumes after a canister
  upgrade and that a completed page cursor survives a second upgrade.

## [0.26.2] - 2026-11-20

### Logging

- The optional `logging` feature records bounded, payload-free failure diagnostics.
  Standalone callers can page through local entries. A Canic adapter can route
  them directly to its existing operator log.

### Metrics

- The optional `metrics` feature records fluent query work, result rows and
  bytes, commits, retained aggregate calls and returned failures. Standalone
  callers can read a heap snapshot, and Canic adapters can attach shared storage.

### Queries

- Completed groups release their encoded comparison keys during finalization.
  Ordering by aggregate value preserves ascending group-key order for ties,
  including across retained result pages.
- Scalar and grouped minima and maxima reuse comparison storage during each
  scan. Equal-sized winners reuse the retained key and value buffers, including
  during a retained job step.
- Synchronous scalar and grouped sums, averages, minima and maxima now seek a
  complete unique key supplied by equality filters. A hit needs one owner read,
  and a missing key needs none. Other filters still apply to the owner.
- Grouped counts can stay on the primary scan when a secondary-index bound
  spans the entire fixed-width field, such as `category >= 0` for a Nat64.
  A selective range on another index can still narrow the scan. Scalar counts
  retain their key-only path when index keys prove every filter.

## [0.26.1] - 2026-11-20

### Queries

- Float32 fields support scalar and grouped `sum` and `avg`, including retained
  jobs. Results round directly to Float32. Each exact total uses 40 bytes.
- Float64 fields support scalar and grouped `avg`, including retained jobs.
  The engine divides the exact total by the number of present values and
  rounds once to Float64, nearest with ties to even.

### Qualification

- Verified that committed writes invalidate unfinished Float32 scalar and grouped
  averages, that retries preserve this outcome, and that a completed result
  remains readable after later writes and a canister upgrade.

## [0.26.0] - 2026-11-20

### Schema

- Add `timestamps` inside `#[entity(...)]` for the standard managed `created_at`
  and `updated_at` fields. Switching from the equivalent explicit declarations
  preserves the schema fingerprint. Inspect managed roles in the local fixture:

```sh
db schema ManagedRow
```

### Queries

- Fluent scalar and grouped sums now accept Float64 fields in synchronous and
  retained queries. They round the exact total once, including across upgrades.
  Empty or all-absent input returns absence; exact cancellation returns positive
  zero. Finite-result overflow refuses at completion.

## [0.25.0] - 2026-11-20

### Queries

- Retained DISTINCT can page its completed keys in descending order with
  `order_groups(GroupOrder::KeyDescending)` or preserve first occurrence with
  `order_groups(GroupOrder::FirstOccurrence)`. Both use the existing 255-key
  and 16 KiB limits. Code exhaustively matching `GroupOrder` needs the new arm.
  In Rust, combine first occurrence with `order_by` and `descending` to visit
  rows in a supported index order. The planner selects the index automatically.
  Try either output order on the local fixture:

```sh
db aggregate Entry --select distinct:amount --group-order key-desc --work 2 --page-size 2
db aggregate Entry --select distinct:amount --group-order first --order-by category --desc --work 2 --page-size 2
```

- Resuming a grouped or DISTINCT job decodes each saved projected field directly
  into its comparison key. At 255 Nat64 keys the measured single-key page update
  fell from 10.43M to 10.26M cycles.
- Group accumulation now finds or inserts each key with one tree lookup. Scanning
  255 distinct Nat64 keys and returning the first result cost 13.92M cycles
  in the fixture, down from 15.07M.
- DISTINCT counts track the memory used by retained keys as each new value
  arrives. Across two groups with repeated amounts, both scan steps together
  fell from 22.95M to 21.31M cycles. Insertion uses one set lookup. At capacity,
  duplicates still count toward scan progress and a new key refuses the step.

### Qualification

- Confirmed on PocketIC that the 0.25.0 candidate resumes a running default-order
  DISTINCT job and a completed page cursor created by the released 0.24.4 engine.
- The SQLite comparison now checks that synchronous and resumable DISTINCT return
  the same first qualifying prices in both directions of a compound index.
- PocketIC now measures one completed DISTINCT page at 8, 32, 128 and 255
  retained keys. The fixture checks that paging leaves input progress unchanged.

## [0.24.4] - 2026-11-20

### Queries

- Indexed `distinct()` can page a projection such as `(category, amount)` in
  category order when `.eq(Entry::AMOUNT, 1_u64)` fixes the amount. Only the
  varying part of the tuple needs to identify a run of equal index entries.
  A compound index on `(category, published, amount)` can also page distinct
  `(category, amount)` pairs when the query fixes `published`.
- A primary-key scan can page a DISTINCT projection when literal equalities
  fix every projected field. It emits the tuple once if a matching row exists.
  On an ordered traversal, `distinct().try_one_as::<R>()` can return on the
  first validated match when equality fixes the projection.

### Qualification

- Both workspaces now use Toko Tooling 0.45.25. Its new PR baseline rule
  applies to canisters that ship a supported public Wire API. IC DB's
  Wire endpoints are private qualification fixtures.
- The listing benchmark has its own test target and still runs through
  `make m2-listing-costs`.
- `make m2-listing-costs` now compares the first 20 IDs from indexed listing
  and bounded sorting against SQLite, including refusal and cursor behavior
  with a one-candidate work budget. It also checks how many owner rows the
  index fetches as the ID cutoff changes. A separate compound-index experiment
  compares both equality prefixes on the same database and measures the extra
  index’s single-row write cost and stable-memory allocation. The replacement
  variant checks restricted deletion and score ordering across publication states.
  Native checks cover preserved row bytes. A PocketIC upgrade after the first
  copied document verifies incompatible-version refusal, resumption, cutover
  and source cursor refusal for the replacement index. Cancelling before cutover
  restores the original cursor after cleanup, including across upgrades.

## [0.24.3] - 2026-11-20

### Queries

- `start_distinct::<K>()` and `resume_distinct::<K>()` enumerate scattered
  projection values in canonical key order. One revision-bound job scans the
  input in chunks and pages the completed keys. It holds up to 255 tuples
  within the existing 16 KiB frame. Completed pages read directly from the
  restored keys in canonical order. Try the local fixture with:

```sh
db aggregate Entry --select distinct:amount --work 2 --page-size 2
```

- Retained-job retries and completed-page reads skip the checkpoint write when
  both the saved sequence and the host observation time stay the same.
- Indexed DISTINCT can now page through classes where only some rows match.
  A page can examine a nonmatch and resume inside the same class.

```sh
db query Entry --order-by category --min 40 --select category --distinct --work 1
```

- `first_as::<Summary>()` and `try_one_as::<Summary>()` return an optional
  projected record. The latter checks for a second match even after `limit(1)`.
  With DISTINCT in traversal order, these methods can finish as soon as the
  first value or a second distinct value establishes the answer.
- When an equality fixes a scalar value, the planner can resolve range
  comparisons on that value before reading rows. A contradictory range visits
  zero candidates. A covered count can read just the matching index entries.

### Schema lookup

- Named counts in the console and migration target lookups now use binary search
  over the store’s existing logical-name order.

### Qualification

- Native fixtures enumerate 64 unindexed DISTINCT values using sorted pages
  over 128 rows and two retained key ranges over 521 rows. They check the
  returned values and the cost in examined candidates.
- The mixed-lifecycle fixture now type-checks its Canic database endpoints on
  native builds and checks pre-decode inspection authorization on PocketIC.

## [0.24.2] - 2026-11-20

### Store routing

- Each store now admits up to 128 entity types. Migration checkpoints use the existing
  one-byte entity position throughout that range.
- `CanisterSession<Hub>` routes fluent queries, key lookups and single-row writes
  to the entity’s declared store. Hosts attach the canister’s compiled roster once.
  Rust rejects entities declared for a different canister. A generic helper can
  accept `&impl QuerySession<E>` to read through either kind of session.
- `CanisterSession::batch()` groups dependent writes in the store inferred from
  their entity types. Rust rejects a batch that mixes stores. `batch_at(time)`
  supplies one timestamp for managed fields.
- Store and canister sessions accept `db.insert_input(Entity::insert()...)` for
  a single row with schema defaults. The call returns the row after commit succeeds.
- Store and canister sessions now expose `get_many::<E>(&keys)` for up to
  32 primary keys. Each input position receives its row or `None`.
- Schema declaration errors now identify the referencing field and show which declared
  indexes can serve it, or which leading field a new index needs.

### Qualification

- Added `make m1-store-width` to measure creating and reopening a single game
  store, reading a late entity and enforcing its reference on PocketIC.
- Qualified restricted deletion with a full 32-mutation batch. One surviving child
  refuses the batch intact. After repointing that child, the same delete succeeds
  and the adjacent parent’s 257 children remain reachable after reopen.
  `make m3-references` also measures early and late refusal against an installed
  self-referencing schema, separates setup, open, batch construction and commit,
  and checks that both refusals preserve stable memory. The qualification matrix
  includes a one-off comparison with frozen IcyDB.
- Remote inspection moves to `canic db inspect`. The fixture-specific `db witness-list`
  command is retired. Inspect the local database with these commands.

  ```sh
  db entities
  db schema
  ```

- The remaining Wire 0.22 dependency belongs to the
  frozen IcyDB query comparison. Standalone legacy startup and write benchmarks
  are retired. Use `make m2-query-matrix` for query comparisons and
  `make m1-write-envelope` for IC DB write qualification.
- Schema qualification inherits the caller’s Cargo network mode. The public CI
  test resolves the schema fixtures independently of the private legacy benchmark.
- Verified the single-store shard reopens populated data through the same typed
  root API as the two-store hub. Qualified dependency requirements are inherited
  from their workspaces, including the consumer’s renamed schema dependency.

### 🧪 Testing

- CI prepares the workspace compiler, rustfmt and Clippy with Toko Tooling’s shared installer. [#14 — Use the shared Rust toolchain installer in CI](https://github.com/dragginzgame/ic-db/issues/14)

## [0.24.1] - 2026-11-19

- Fixed rust-analyzer marking active entity, store and canister declarations
  as inactive UEFI code with default Cargo settings.
  [#17 — Keep ctor diagnostics off authored schema declarations](https://github.com/dragginzgame/ic-db/issues/17)

## [0.24.0] - 2026-11-19

### Schema authoring

- Schema owners now call `ic_db::include_schema!();` at the crate root.
  This replaces the handwritten generated-file include. Use `build_schema!`
  in the owner’s build script in place of the retired `ic_db::build::generate`.
- Shared schemas now declare `#[canister]`, `#[store(canister = Hub, key = "...")]`
  and `#[entity(store = Accounts, ...)]`. Add an explicit store to every entity.
  Replace the shared crate's flat `compiled()` with `Accounts::compiled()` or
  iterate `Hub::STORES` to open all of its allocations. Keeping the same store
  key and entity contract preserves existing backing.
  [#15 — Typed canister and store ownership](https://github.com/dragginzgame/ic-db/issues/15) ·
  [#16 — Export each canister’s owned stores](https://github.com/dragginzgame/ic-db/issues/16)
- Schema declarations use `ic_db::design::prelude::*`. The schema build check
  enforces this convention across the product and qualification sources.

### 🎯 Qualification

- Qualification uses IC Wire 0.27.0 for DB fixtures and their clients. Canic
  inspection retains Wire 0.25.2 to match the pinned producer's protocol types.

## [0.23.22] - 2026-11-19

- Document schema fixture types and generated insert and unique-key types.
  Qualification endpoints now identify their frame guards and byte limits.
- Removed the public `ic_db::build::write_schema(source, target)` function.
  A shared schema crate can call `ic_db::build_schema!(model)` in its own
  package-root build script and export one `compiled()` for the whole subtree.
  The records qualification canister now imports that function. CI checks shared
  schema build ownership with `make schema-build-check`. Local
  `build_schema!()` is preserved. Scripts beside their declarations can use
  `build_schema!(self::schema)` for the previous colocated layout.
- Schema builds identify the referencing field and missing target, or show both
  fields and their types when a reference disagrees with the target primary key.
- All schema declaration macros now report the same 1–64 UTF-8 byte rule for
  an invalid logical name, identifying the declaration that failed.
- Resuming a retained aggregate now constructs its progress report directly from
  the saved checkpoint. Group page cursors reuse the job handle decoder for length validation.
- Adopt Toko Tooling 0.45.22 in the product and qualification workspaces.
- Centralize entity, field and named index lookup in the compiled schema. Stored index entries,
  pagination cursors and DISTINCT now share optional-component decoding.

## [0.23.21] - 2026-11-19

- After an interrupted migration, `SchemaCopy::resume(...)?` exposes
  `.status()`. It identifies the entity to copy, validation, readiness for
  explicit publication, or cancellation from the saved checkpoint.
- Migration validation checks a managed row’s saved timestamps with one peer-row
  lookup. For rows without managed timestamps it only checks peer-key existence.
- Counts, writes and migration validation now reject entity metadata that reports
  rows at revision zero. The first insertion always advances the revision.
- An explicit schema migration can add an empty entity. Include it in the
  compiled target schema and copy the existing entities as usual. The new
  entity becomes writable at cutover. Cancelling before cutover reopens the
  source schema with its existing rows and indexes. After publication, the new
  entity can reference migrated rows through ordinary checked batches.
- A DISTINCT projection containing a complete unique index key can page in
  primary-key order using ordinary row and work limits. Sparse unique keys
  qualify when all matching rows participate in the index. A required equality
  filter or selected index prefix can supply omitted key fields. For example,
  fixing the section lets callers project just the seat number when the
  schema declares the section/seat pair unique.
- When equality filters fix the primary key or every field of a unique index,
  a fresh DISTINCT row query can use the same single-owner lookup as an ordinary projection.
  A miss examines zero candidates and a hit still checks the remaining filters.
- Counts can resolve prefix, suffix and substring filters directly when the
  selected key fixes the Text or Blob value. A contradictory pattern finishes
  with zero candidates examined.

## [0.23.20] - 2026-11-19

- Update product dependencies to IC Platform Types 0.21.4, Runtime 0.23.22 and
  Storage 0.78.7. Qualification uses Host Artifacts 0.80.6, Host Text 0.77.4,
  Wire 0.26.1 and the Testkit 0.110.3 client. Canic inspection still uses its
  pinned producer’s Wire 0.25 contract and the external PocketIC launcher stays
  at 0.110.0. The release editor stays at IC Build 0.84.1 because 0.87.6 does
  not ship the required `ic-build-release-metadata` binary.
- A fixed Text or Blob value can satisfy or disprove a length filter before
  reading rows. A covered count uses its index entries directly. Conflicting
  byte lengths finish with zero candidate visits.
- `filter_fields(left, Eq/Ne, right)` can also count from a compound index
  when its prefix fixes both fields. A contradictory equality or inequality
  between two known values finishes before traversal.

## [0.23.19] - 2026-11-19

- Adopt Toko Tooling 0.45.21 in both workspaces and apply its shared Clippy policy.
- Schema scalars, including `Ulid` and `U256`, and macro-authored entities,
  records, enums, lists, sets, maps and newtypes support Serde by default.
  Canic endpoints can take a bare `Ulid`. Its Wire payload matches the
  existing fixed 16-byte array, and its JSON form is canonical ULID text.
- `filter_in` on the primary key followed by `.count()` can probe each
  distinct requested ID directly. Additional literal ID filters narrow the set
  before any probes, including intersecting multiple membership lists. A miss
  costs one probe. When the probes
  exceed the work budget, the existing bounded range scan still applies.
- Membership on the last component of a selected unique index can use the same
  probes. For a compound unique index on `(tenant, code)`, an equality on
  `tenant` and `filter_in` on `code` can select that path automatically.
  On a non-unique index, fixing all indexed fields also lets `.count()` probe
  requested owner IDs directly. IDs belonging to another prefix count as misses.
  On a sparse unique index, `filter_in` may include `None` alongside present
  values. If another filter requires presence, only the distinct present values
  cost probes.
- `.count()` can use a compound `present_only` index when its membership
  proves the presence filters, including on trailing fields. The usual scan
  budget applies, and `.count_validated()` still checks owner rows.
- A query that fixes an index component to absent and requires it to be present
  now finishes with zero candidate visits, including for length and field comparisons.

## [0.23.18] - 2026-11-19

- Start a fluent query with `session.query::<User>()`. It sees data at
  execution. Look up a declared unique index with a generated named key,
  for example `session.get_unique(&UserByIssuerAndSubject { issuer, subject })?`.
  Use `session.insert(&row)`, `session.replace(&row)` or
  `session.delete::<User>(id)` for one-operation batches.
- `filter_present` on the next indexed field can seek directly to present values,
  including empty text. A count covered by that range fetches zero owner rows.
- A fluent query with a fixed field value finishes with zero candidate visits
  when `filter_in` excludes that value or `filter_not_in` forbids it. This
  applies to equality filters and explicit index prefixes. When the selected
  prefix proves a membership filter true, `.count()` can count matching
  index entries without fetching their owner rows. A primary-key lookup also
  skips the row read when the ID proves every remaining filter.
- Adopt Toko Tooling 0.45.19 in both workspaces. The records qualification
  fixture now uses Platform Runtime’s trap directly on native and Wasm.

## [0.23.17] - 2026-11-19

- Hosts can hand nested helpers a clone of `DatabaseSession` over one attachment.
  Typed lookups return owned rows, fluent queries use a read guard, and batch commits
  report a borrow conflict before mutation if that guard is still live. [#10](https://github.com/dragginzgame/ic-db/issues/10)
- A query that requires two different values for the same field now finishes with
  zero candidate visits. This also applies when an equality filter contradicts an
  explicit index prefix. Invalid filters still return an admission error.

## [0.23.16] - 2026-11-19

- IC DB’s qualification fixtures and local CLI use IC Wire 0.26.0. Calls to the
  pinned Canic inspection and witness endpoints retain their 0.25 protocol dependency.
- Canister ULID generators use Platform Runtime’s bounded entropy request. IC DB
  requests entropy once per fresh generator with the existing ten-second wait.
  Native generators continue to use OS entropy.

## [0.23.15] - 2026-11-19

### 🆔 ULIDs

- Use `ids.next_at(timestamp)?` to issue a seeded ULID with the same captured
  time passed to `Batch::at(timestamp)`.
- Native applications and canisters can both call `Ulid::generate_prand()`,
  `Ulid::generator_prand()`, and `Ulid::generator().await` directly. Native
  issuance uses the system clock and OS entropy. Canister pseudorandom issuance
  continues to use the existing principal/version seed without an IC call.

### 🔎 Queries

- Filters, projections, sorting and aggregates now resolve field positions from
  the same compiled entity declaration used to inspect stored rows.
- Explicit and automatic index selection, counting and scan bounds now share
  the rule for a unique lookup. On a unique `(issuer, subject)` index, fixing
  both fields identifies at most one row. Fixing only `issuer` can match many.

### 🔄 Retained aggregates

- Job creation and resume now use the same checkpoint encoder. Existing saved
  jobs keep their encoding and operation identity.
- Scalar and grouped aggregates now share the final check before saving
  progress. If a completed result exceeds the output budget, the caller can
  retry from the previous checkpoint.

### ▶️ Try it

With the local fixture CLI installed, run this from the repository root to
exercise grouped counting in two-candidate steps and request one group per page.
Each of the two categories has three entries.

```sh
db aggregate Entry --select category,count --work 2 --page-size 1
```

### 🧪 Testing

- Installed qualification probes supply the IC instruction counter at dispatch.
  Native tests exercise the database operations directly. [#7](https://github.com/dragginzgame/ic-db/issues/7)
- Retained console aggregates use one IC timestamp per start or resume. A native
  test checks expiry at exactly 60,000 milliseconds. [#8](https://github.com/dragginzgame/ic-db/issues/8)
- A raw stable-memory check now verifies the initial checkpoint bytes for
  counts, sums, minima, DISTINCT counts and grouped counts. Query tests also
  cover the last allowed visit for complete and partial compound unique keys.
- The product and qualification workspaces now use Toko Tooling 0.45.18.
  In the mixed-lifecycle fixture, one Wasm guard covers the Canic endpoint mount
  and its caller check. Native builds also compile the database handlers.

## [0.23.14] - 2026-11-19

- Fix rust-analyzer marking active schema entities as inactive on non-UEFI
  hosts. Editor analysis skips the host linker constructor and resolves the
  generated fields and field constants.
- Text and Blob filters can borrow application fields directly, for example
  `.eq(Document::TITLE, &title)` for a `String` or
  `.eq(BinaryObject::PAYLOAD, b"abc")` for a byte literal.
- Optional filter operands accept Rust `Option` values. For example,
  `.filter_in(Document::PRIORITY, &[None, Some(3_u64)])` matches absent
  priorities and priority 3.
- Pass typed slices directly to `filter_in` and `filter_not_in`, for example
  `.filter_in(Entry::AMOUNT, &[60_u64, 80])`. Remove redundant element
  conversions from existing calls so Rust can infer the slice type.
- `filter_in(field, &values)` can seek between the smallest and largest members
  when the field is the next primary or secondary key component. Every owner
  tied at the upper end is included. Interior candidates still consume work.
  Automatic count and existence checks can select this indexed interval.

## [0.23.13] - 2026-11-19

### Schema and queries

- Use generated field constants such as `User::ID` and `User::NAME` in
  filters and ordering. Read entity metadata through `<User as Entity>::SCHEMA`
  and the Rust declaration path through `User::declaration_path()`.
- Use `eq`, `ne`, `lt`, `lte`, `gt` and `gte` for literal comparisons,
  for example `.gte(Entry::AMOUNT, lower).lt(Entry::AMOUNT, upper)`.
  Use `filter` when the comparison is selected at runtime.
- Chain sort fields in priority order with `.order_asc(Entry::CATEGORY).order_desc(Entry::ID)`.
  The equivalent `order_by_each` list shares the same cursor.
- Replace `Database::get_by_unique` with ordinary equality filters and
  `.try_one()?`. A second match returns `MultipleMatches`; use `.first()?`
  when the first match in order is sufficient. Keep schema uniqueness for
  values that must have a single owner.
- Load related rows with `db.get_many::<E>(&keys)?`. Up to 32 supplied
  positions each receive an optional row, preserving duplicates and misses.

### Query execution

- Default-order entity and projection pages can use a non-unique equality
  prefix in either direction. Existing primary cursors resume on primary storage.
  Once all indexed fields are fixed, an ID range can seek on the owner key.
- Counts and existence checks use these bounds with residual filters. Covered
  counts can use index keys alone; complete unique keys can finish within a
  one-candidate budget even when another index covers the same equality.
  A separate indexed equality can narrow the scan when an ID occurs inside an OR.
- A field required to be both present and absent yields an empty traversal.

## [0.23.12] - 2026-11-18

### 🔎 Queries

- Use `.eq(field, value)` for literal equality and `.first()?` for one optional
  entity. Fresh entity reads and field projections use a complete unique key when the
  filters bind all its fields, including compound keys in either filter order. An explicit result order
  also uses the unique lookup because at most one row can match.
  A partial scan that finds no match returns `Limit` from `first()`; use
  `fetch()` when the caller wants a continuation.

### Qualification

- The `test-support` feature exposes the same qualification backing types on
  native and Wasm. Native qualification now runs the records fixture’s
  initialization and indexed category query directly. IC instruction counts
  still require the installed canister.

## [0.23.11] - 2026-11-18

### 🔎 Queries

- Query output uses the engine’s 128 KiB limit automatically. Remove
  `.byte_limit(...)` from application queries and `--bytes` from local CLI
  commands. Row limits, work bounds and continuations retain their meanings.
- `Database::get_by_unique` returns one optional entity for a complete unique
  index key. A hit verifies the owner row. No page or caller-selected limits
  are needed.

### Schema authoring

- Declare the primary key with `pk(field = "id")`. Remove `source = "external"`
  from existing declarations. Callers supply the key on insert. This syntax
  change requires no stored-data migration.

## [0.23.10] - 2026-11-18

### 🗄️ Database handles

- Application helpers take `&Database` or `&mut Database`. Remove the memory
  parameter from database, query, aggregate-job and maintenance handles when
  updating. IC Storage selects stable memory on Wasm and vector memory on native.

### 🆔 ULIDs

- `Ulid::generate_prand()` issues synchronous IDs in canister updates. Its private
  stream is seeded once from the canister identity and version. Equal or
  backward times increment the previous suffix. Explicit generators own
  independent state. `Ulid::generator_prand()` creates an owned stream with
  synchronous `next_id()`. Both pseudo-random paths use local system reads.
  `Ulid::generator().await` requests fresh entropy once per owned stream.

### 🔎 Queries

- Ordered DISTINCT can select the best 1–32 projected values even when the input
  contains more than 32 distinct values. The complete sorter retains only the
  requested number and still requires enough work to finish the scan.
  The order can include tie-breakers after the projected fields. In the local
  fixture, the first two commands return the highest category. The third fixes
  category by equality, so ordering by amount also yields the single category 1.

```sh
db query Entry --order-by category:desc --select category --distinct --limit 1
db query Entry --order-by category:desc,amount:asc --select category --distinct --limit 1
db query Entry --category 1 --order-by amount:desc --select category --distinct
```

### 🎯 Qualification

- `make m4-lifecycle` now checks an indexed listing and a retained count across
  an item move and an executable upgrade in a skewed 128-item application.
  It compares total listing instructions and the most expensive page at
  work budgets of 1, 8 and 32 candidates. This application listing defaults to
  eight and can resume a cursor issued with a one-candidate budget.
  [Workload and measurements](qualification/README.md#skewed-application-workload-02310)
- Added a complete two-row listing comparison against frozen IcyDB. After upgrade
  and startup, IC DB used 14.57M handler instructions and IcyDB used 30.18M
  to return the same ten tuples across six calls. A second run changes category
  membership on both sides of the last returned ID and checks the remaining
  pages after upgrade against a fresh traversal. [Comparison](qualification/README.md#matched-complete-keyset-listing-02310)

## [0.23.9] - 2026-11-18

### 🔎 Queries

- Synchronous existence checks and scalar and grouped counts now seek mandatory
  indexed equalities and ranges. At equal prefix length, the planner prefers
  an index that can also bound the next field. A range on the first indexed
  field works without an equality or an explicit index hint.
- Ordinary `.count()` reads just the index keys when they enforce every filter.
  Other counts check candidate rows for remaining predicates, field presence
  or distinct values. `.count_validated()` checks each visited owner.

### 🎯 Qualification

In the local fixture, existence finds ID 4 on its second category-0 candidate.
The count commands examine three category-0 rows, of which two meet the minimum
amount. The document title-prefix count reads one index key and zero rows.

```sh
db query Entry --category 0 --min 40 --select exists --work 2
db query Entry --category 0 --min 40 --select count --work 4
db query Entry --category 0 --min 40 --select count:amount --work 4
db query Entry --category 0 --min 40 --select distinct-count:category --work 4
db query Entry --category 0 --min 40 --select category,count --work 4
db query Document --bool published false --text title starts-with First --select count
```

### 🧪 Testing

- Restored the isolated prefix-counter experiment against the current write path.
  Replacing a row subtracts a counter only when the old index entry is removed.

### 🔧 Tooling

- Updated both workspaces and their shared snapshot to Toko Tooling 0.45.8.

## [0.23.8] - 2026-11-18

- Enable `test-support` on a native `ic-db` dev-dependency to use
  `ic_db::testing::Fixture`. Create and reopen a database under application
  schemas on retained vector backing, or supply a named allocation factory.

## [0.23.7] - 2026-11-18

### Fluent queries

- Primary-key ranges finish on the page that processes their inclusive endpoint.
  For IDs 3 through 5 with a one-candidate budget, three calls suffice in either
  direction when all three IDs exist. Counts can also finish exactly at their
  work limit. For a single ID that fails another filter, `.exists()` returns
  false after checking that row.
- Ordered queries can now use another path when a valid field value would
  exceed the 512-byte index key limit. Filtering and page work limits still
  apply. An explicitly requested oversized index prefix returns `Limit`.
- An ordered entity or projection query with an exact primary-key filter can
  reach its row with a work budget of one, even with additional indexed filters.
  Other predicates still decide whether that row is returned. A live cursor
  issued for a previously selected secondary path requires a fresh query.
- `.exists()` automatically seeks a secondary index when its leading fields
  cover all equality filters. A hit reads and validates one owner row. A miss
  reads zero rows. Try the missing reference in the local fixture.

  ```sh
  db query Document --select exists --entry-id 999 --work 1
  ```

- `.filter_between_fields("price", "floor", "ceiling")` includes prices at
  either bound. `.filter_not_between_fields(...)` selects a price below the
  floor or above the ceiling. Both take their bounds from the current row.
- The local console accepts both filters for queries and retained aggregates.
  On the six-row fixture, the first command returns ID 1 and the second counts five rows.

  ```sh
  db query Entry --between-fields category id amount --select id
  db aggregate Entry --not-between-fields category id amount --select count --work 1
  ```

### Qualification

- Indexed existence used 37–60% fewer handler instructions than frozen IcyDB’s
  maintained-count lookup across 36 matched PocketIC cases.
  [Existence comparison](qualification/README.md#indexed-existence-0237)
- Updated Canic to 0.223.1 and removed the Wire 0.24 compatibility dependency.
  All maintained qualification callers use IC Wire 0.25.0.
- Measured an indexed page, an identical replacement and a retained-count
  advance across upgrade. The count advance used 14.6% more handler instructions
  and 1.6% more cycles at the same checkpoint. [Opening costs](qualification/README.md#opening-costs-through-the-canic-mount-0237)

## [0.23.6] - 2026-11-18

### Qualification

- Exercised a live listing across a write and upgrade through the pinned Canic
  application. Measured when an exact sum completes before the next source
  write at two candidate budgets. Reproduce with `make m4-inspection` and
  `make m2-contention`.
- The three-entity workload now runs its ordinary batches, snapshot queries, sparse listings and retained counts
  through Canic’s typed adapter against the current IC DB engine.
- Extended `make m4-lifecycle` to check a three-entity batch during copy,
  validation and retired-source cleanup, including an upgrade after the target
  accepts the batch. The listing also checks cursor lifetime across cutover:
  the target refuses a source cursor, and a fresh target cursor survives cleanup
  and upgrade.
- Qualified retained counts across cutover. The target refuses a source job,
  the caller releases it, and a fresh target count completes across cleanup and upgrade.
  The fixture retries the old cancellation and replays a lost advance reply on the new count.

### Tooling

- Updated both Cargo workspaces and the vendored snapshot to Toko Tooling 0.45.7.

## [0.23.5] - 2026-11-18

### 🔧 Changed

- Fluent `.count()` counts primary keys directly for an exact key or an interval
  defined by literal comparisons. The scan work limit still applies.
  A 511-row range used 47.1% fewer instructions than `.count_validated()`
  on the measured fixture, including opening. [Count comparison](qualification/README.md#primary-key-range-count-costs-0235)
- Qualified automatic and hinted ordered listings against SQLite and PocketIC,
  including tied keys, empty pages and retries at the output-byte limit.
  [Listing comparison](qualification/README.md#filtered-ordered-listing-with-automatic-index-selection-0235)
- Qualification fixtures and their CLI use IC Wire 0.25.0. The pinned Canic
  inspection client uses Wire 0.24.6 for Canic’s public protocol types.
  The native SQLite reference uses rusqlite 0.43.0. Both workspace locks select IC Storage 0.78.2.
- Updated both workspaces and the tooling snapshot to Toko Tooling 0.45.6.
  Release and benchmark commands honor the caller’s Cargo network settings.

## [0.23.4] - 2026-11-18

### ➕ Added

- `.filter_fields("allowed_roles", Comparison::Contains, "requested_role")`
  checks a row’s requested role against its own list or set. Duplicate members
  select the row once. An absent role or collection yields no match.
- In the local fixture, `--text FIELD OP VALUE` passes a text literal to
  fluent filtering. Both query pages and retained aggregates accept it.

  ```sh
  db query Document --text title eq 'First document' --select id
  db aggregate Document --text title ends-with document --select count --work 1
  ```

- `db query Document` accepts `--select title` and `--select note`. The
  output budget counts the encoded field. An absent note displays as `—`.

  ```sh
  db query Document --text title contains doc --select title --bytes 18
  db query Document --select note --distinct --bytes 1
  ```

- `--bool FIELD true|false` adds an exact Boolean filter to fixture queries
  and aggregates. For an ordered Document listing, specifying `published` lets
  the engine seek a title prefix in the existing compound index.

  ```sh
  db query Document --bool published false --text title starts-with First --order-by title --select title
  ```

### 🔧 Changed

- Fluent `.count()` automatically uses maintained entity cardinality, a fully
  covered equality index prefix, or row execution. Use `.count_validated()`
  when checking the integrity of visited rows and selected index entries.

  ```sh
  db query Entry --select count --work 1
  db query Entry --category 1 --select count
  ```

- Updated Toko Tooling to 0.45.5 in both workspaces and grouped the root
  dependency catalog by its production and tooling uses.

## [0.23.3] - 2026-11-18

### ➕ Added

- `filter_fields("title", Comparison::Contains, "note")` uses the note on each
  row as the search text. Text and Blob field pairs also support prefix and
  suffix matches. Try both a present and an absent needle in the local fixture.

  ```sh
  db query Document --compare title starts-with title --select count
  db query Document --compare title contains note --select count
  ```

- `.filter("labels", Comparison::Contains, "art")` matches a list or set
  that has an element equal to `"art"`. Repeated list elements return the row
  once. Text and Blob fields retain their substring behavior.
- Use `ic_db::build_schema!(src::schema);` in a standard package-root `build.rs`.
  An existing build script can generate a named helper and call it from its own
  `main`. [Canister setup](README.md#build-a-canister-local-schema)

## [0.23.2] - 2026-11-18

### ➕ Added

- A canister can put `ic_db::build_schema!(schema);` in `src/build.rs` to
  generate its local schema. The macro uses ordinary Rust module discovery and
  writes the validated assembly to Cargo’s output directory.
  [Canister setup](README.md#build-a-canister-local-schema)
- Filter by length in fluent Rust. `.filter_length("labels", Comparison::Gt, 0)`
  selects a present list with at least one item. Text counts UTF-8 bytes, Blob
  counts raw bytes, and lists, sets and maps count items. Use `QueryOperand::Length`
  in a mixed OR clause. The local fixture commands accept `--length FIELD OP N`.

  ```sh
  make db
  db query Document --length title eq 14 --select id
  db aggregate BinaryObject --length payload eq 3 --select count
  ```

- Fluent Rust queries can combine field-to-field and literal comparisons in one
  bounded OR clause using `filter_any_operands`. For example, this counts an
  event once if its observed value exceeds its target or it is marked urgent.

  ```rust
  use ic_db::{Comparison, QueryOperand::{Field, Literal}};
  let urgent_or_over_target = db.query::<Event>().filter_any_operands(&[
      ("observed", Comparison::Gt, Field("target")),
      ("urgent", Comparison::Eq, Literal(true.into())),
  ]).count()?;
  ```

- Compare two fields on each row with `.filter_fields("actual", Comparison::Gt, "expected")`.
  Equality and ordering use the declared scalar types, including optional values.
  A present-only index can serve the query when the comparison guarantees its
  optional fields are present, including when the indexed field is on the right.
  If automatic selection switches indexes, restart a cursor or retained job
  issued for the previous path.
  Both fixture commands accept `--compare LEFT OP RIGHT`. The Entry examples
  return ID 1 and a retained count of five. Every DecimalRow lacks `maybe`,
  so its present-only index returns an empty page with zero candidates examined.

  ```sh
  make db
  db query Entry --compare id eq category --select id
  db aggregate Entry --compare id gt category --select count --work 2
  db query DecimalRow --compare value gt maybe --order-by maybe --work 1 --select id
  ```

- Fluent queries accept `.filter_present("field")` and `.filter_absent("field")`
  for optional fields. Present empty strings, blobs and collections match
  `filter_present`. The fixture CLI exposes both filters on queries and aggregates.
  Its Document has a present priority of zero and an absent note.

  ```sh
  make db
  db query Document --present priority --absent note --select id
  db aggregate Document --present note --select count
  ```

- Use `Comparison::Contains` to find an exact byte sequence in a Blob field
  or a completed Blob group extremum. An empty sequence matches every present
  Blob, including an empty one. Try it on the local fixture’s `0x00ff80` payload:

  ```sh
  make db
  db query BinaryObject --payload-contains 0xff --select payload
  db aggregate BinaryObject --payload-contains 0xff --select count
  ```

- Fluent sums can return the input field’s numeric newtype. Averages can
  return the input’s Decimal newtype at its declared scale, including when
  the calculation spans multiple calls.

### 🔧 Changed

- Adopt Toko Tooling 0.45.3 in both workspaces. CI setup diagnostics now
  redact environment assignments and URL fragments. Release timing reports
  include more detail about repeated checks.

## [0.23.1] - 2026-11-18

### ➕ Added

- Entity fields can use a Timestamp `#[newtype]` for `managed = "created_at"`
  or `managed = "updated_at"`. The insert builder supplies the typed value
  from the batch clock.
- Fluent sums and averages accept integer- and Decimal-backed newtype inputs,
  including grouped and resumable queries. Result records declare the usual
  primitive width or Decimal scale.
- Use a primitive-backed `#[newtype]` as a `#[set]` element or `#[map]` key.
  The macro derives `Ord` and `PartialOrd` from the primitive. Remove any
  manual implementations or duplicate derives when upgrading.

### 🔧 Changed

- Release delivery now includes the `ic-db` and `ic-db-macros` packages on crates.io.
- Adopt Toko Tooling 0.45.1 in both workspaces. CI setup failures retain the
  original command status and up to 100 lines or 32 KiB per sanitized excerpt.

## [0.23.0] - 2026-11-18

### ➕ Added

- Import `ic_db::design::prelude::*` in schema modules to use all seven declaration
  attributes, their database scalar types, and `Entity`, `Value` and `Schema`.
- Add `#[list]` for reusable bounded lists and `#[newtype]` for distinct
  primitive-backed types. Scalar newtypes support fluent comparisons and
  secondary indexes. Wrapping an existing field changes its schema identity.
  Inspect the Sample and Samples definitions in the local fixture.

  ```sh
  make db
  db schema ListRow
  ```

## [0.22.3] - 2026-11-18

- Add the fluent Rust terminal `count_index_entries()` for a bounded, explicitly selected secondary index range. It counts physical entries, including dangling entries, and reports zero owner rows fetched.

## [0.22.2] - 2026-11-17

### 🔧 Changed

- Let `db count ENTITY` count every entity listed by `db entities`, including
  empty entities such as `SetRow`. Resolve exact names against the admitted
  schema and read maintained count metadata, with no row scan or extra storage.
  Unknown names still refuse. Filtered fluent query coverage is unchanged.

  ```sh
  db count SetRow
  db count IdentityRow
  ```
- Add `db entities` to list declared entity names alphabetically with column
  counts, including primary keys and optional fields. `db entities --json`
  emits objects with `name` and `columns`. The interactive console accepts
  the same command and retains the query-call statistics footer. Main help now
  alphabetizes commands, uses terminal colours and omits the long explanatory footer.

  ```sh
  db entities
  db entities --json
  db --help
  ```

- Keep the 512-byte key limit on writes and decoding. Reading malformed backing can incur supplier I/O before rejection.
- Adopt Storage 0.78.0 named stable allocations. Remove the OP heap preparation, scoped adapter and Keep-upgrade fixture. Storage owns its directory codec. Existing OP images and 0.77 backing require explicit migration or fresh backing.
- Review the release gate for [IC DB #5](https://github.com/dragginzgame/ic-db/issues/5):
  prepare dependencies/tools/server once per validation attempt, consolidate four
  M0 checks around one artifact/session, and exclude the frozen comparison packages
  from routine release lint. Retain all selected product checks and fresh delivery
  admission; keep setup/validation logs and timings on success or failure. Failed
  rechecks invalidate old whole-gate receipts. All 63 disposable wiring assertions
  pass, including the real runner's saved-phase resume and fresh admission. Five
  installed M0 tests also pass with the combined session against the retained
  pre-cutover artifact; this qualifies orchestration, not the Storage 0.76 port.
  Real releases still require fresh builds and the complete selected gate.
- Adopt Testkit 0.110.0 for the default-feature-free client and external launcher.
  Setup/run use explicit fresh evidence directories and the new prepared asset
  location. Commands, including the console, have a one-hour total limit; the
  former 24-hour console idle setting is no longer supported by this launcher.

### 🔧 Fixed

- Bound each remote query, including its signature-verification reads, with
  `--timeout-secs` (1–300, default 30). The explicit local root-key fetch gets
  the same separate deadline. A timed-out query still reports elapsed time and
  unavailable cycles. HTTP 429/503 responses now refuse immediately instead of
  taking Agent's implicit retries; signature verification remains enabled.
- Qualify both remote commands against stalled/busy root-key setup, redirects
  and HTTP replies exceeding 2 MiB. Failed setup dispatches no database query;
  real Canic replies require explicit local-root trust before they can render.
- Add a Wasm live-pagination witness combining ordering-key updates, predicate
  entry/exit, inserts and upgrade between pages. Both directions match an
  independent tuple oracle, including permitted repeats/omissions, empty-page
  progress, changed-query refusal and exact byte-limit retries. No engine or
  stored-format changes are needed; `make m2-queries` includes this witness.
- Extend live-page qualification to deleting the cursor row with its dependent,
  then reinserting the same key with a new payload across two upgrades. Both
  directions retain the exclusive position; a fresh query sees the replacement.

### ▶️ Try it

Run `make m0-setup` and `make db` to prepare the simulator and install the CLI.
Application inspection requires an explicit provider, canister and identity;
the local query uses a fresh temporary fixture.

```sh
db inspect --help
db inspect --provider "${IC_DB_PROVIDER:?}" --canister "${IC_DB_CANISTER:?}" --identity "${IC_DB_IDENTITY_PEM:?}" --timeout-secs 10 --json
db query Entry --order-by category --min 50 --limit 1 --work 1 --select id,amount
```

The fixture query demonstrates an empty page with continuation: one examined
candidate failed the filter, so enumeration is not finished.

## [0.22.1] - 2026-11-17

### 🔧 Changed

- Route qualification's original PocketIC client through Testkit 0.108.0 with
  default features disabled. Keep PocketIC 16.1.0, the external prepared launcher,
  explicit deletion and request deadlines. Remove the HTTP fixture's direct
  `ic-ed25519` dependency in favor of a literal public synthetic PEM. The pending
  CLI's direct Agent dependency remains an unresolved Platform migration;
  [Platform #4](https://github.com/dragginzgame/ic-platform/issues/4) records the
  missing secp256k1 signed-query capability. Product crates and durable bytes do
  not change.
- Release supplier review: current stable registry selections are Storage 0.74.1,
  Types 0.21.2, Host Artifacts 0.80.3, Host Text 0.77.2, Wire 0.24.3, Testkit
  0.108.0 and Toko Tooling 0.44.11. Defer optional Runtime 0.23.16 (selected
  0.23.13), Build 0.87.3 (release tool 0.84.1) and launcher 0.108.0 (prepared
  0.106.0) refreshes to the next dependency-maintenance pass. Canic remains pinned
  to the committed inspection producer; frozen IcyDB remains unchanged.

### ➕ Added

- `db inspect` connects to an explicit application provider/canister using Canic
  inspection v1 from committed Canic 0.219.0. Select `--identity PEM` or
  `--anonymous`; catalogue and selected-entity tables/JSON distinguish executable
  schema, stored admission and optional maintained counts. Verified query calls
  report elapsed time and unavailable remote cycles, with no fixture fallback.
  Local development root-key trust requires `--local-root-key` on a loopback URL.
  This implements the inspection slice of [IC DB #4](https://github.com/dragginzgame/ic-db/issues/4).
- `db witness-list` reads one page from Canic's qualification application's
  versioned Item listing: minimum score, ID order, 1–3 results and 1–4 examined
  candidates. Empty pages preserve the opaque cursor; `--after` forwards it
  unchanged. Authorization, filtering and continuation remain application/engine
  responsibilities. This is one explicit application client, not arbitrary remote
  database querying. Target/identity/verified HTTP and call reporting are shared
  with inspection; the engine, durable state and resolved package set are unchanged.
- `db query BinaryObject` and `db aggregate BinaryObject` accept
  `--payload-suffix HEX`, composed with existing bounds and prefix filters.
  Exact raw bytes use the engine's residual predicate and work limits; empty
  suffixes match present payloads. Retained jobs bind the suffix across resume.

### ▶️ Try it

```sh
make db
db query BinaryObject --payload-suffix 0xff80 --select payload
db aggregate BinaryObject --payload-prefix 0x00 --payload-suffix 0x80 --select count --work 1
db query BinaryObject --payload-suffix 0xff --select count
```

The temporary fixture returns `0x00ff80`, count one, and count zero respectively.
These commands do not access deployed data.

For an application exposing Canic inspection, select its actual target and identity:

```sh
db inspect --provider "${IC_DB_PROVIDER:?}" --canister "${IC_DB_CANISTER:?}" --identity "${IC_DB_IDENTITY_PEM:?}"
db inspect --provider "${IC_DB_PROVIDER:?}" --canister "${IC_DB_CANISTER:?}" --identity "${IC_DB_IDENTITY_PEM:?}" --entity Item --count --json
```

`Item` is the qualification application's entity; use the exposed catalogue for
your application's names. `make m4-inspection` runs the signed HTTP client against
the committed Canic witness, including denied access and closed/unsupported storage.

Against that specific Canic witness, read a bounded page and reuse its `next`
cursor with the same minimum score:

```sh
db witness-list --help
db witness-list --provider "${IC_DB_PROVIDER:?}" --canister "${IC_DB_CANISTER:?}" --identity "${IC_DB_IDENTITY_PEM:?}" --minimum 30 --limit 1 --work 1 --json
db witness-list --provider "${IC_DB_PROVIDER:?}" --canister "${IC_DB_CANISTER:?}" --identity "${IC_DB_IDENTITY_PEM:?}" --minimum 30 --limit 1 --work 1 --after "${IC_DB_CURSOR:?}" --json
```

`make m4-inspection` now also checks sparse listing across upgrade, altered-filter
and corrupt-cursor refusals, repeated caller authorization and unchanged backing.

## [0.22.0] - 2026-11-17

### ⚠️ Breaking

- `LiteralDefault::Blob` extends the public metadata enum; exhaustive consumers
  must handle raw byte defaults. Existing declared contracts retain their identity.

### ➕ Added

- Fluent `filter_between(field, lower, upper)` includes both endpoints using
  existing exact comparisons and index ranges. It consumes two filter terms;
  absent values do not match, and reversed bounds produce no matches.
- Fluent `filter_not_between(field, lower, upper)` matches present values strictly
  outside the inclusive interval. It reuses one bounded OR traversal; reversed
  bounds match every present value once. Both range helpers retain continuation
  identity with their explicit comparisons, including aggregate jobs after reopen.
- Blob insert defaults use Rust byte literals, such as `default = b""` or
  `default = b"\x00\xff"`. Omission applies the default; explicit absence/empty
  bytes and replacements remain unchanged. Defaults obey field bounds and normal
  batch constraints. Schema inspection displays them as hex in the Info column.
- Fluent Blob prefix filtering with `Comparison::StartsWith`, including indexed
  prefix seeks, completed group extrema and retained aggregates. Empty prefixes
  match present values; absence never matches. Use `--payload-prefix HEX` with `db query BinaryObject`
  or `db aggregate BinaryObject`; it combines with inclusive payload bounds.
- Fluent Blob suffix filtering with `Comparison::EndsWith`, also available to
  completed group extrema and retained aggregates. Matching respects raw byte
  boundaries, so a zero byte cannot falsely match suffix `0xff`. Empty suffixes
  match present values; absence never matches. Suffixes remain residual filters
  under the work budget. Blob `Contains` remains unsupported; no CLI suffix flag
  is added in this increment.
- Filter retained `BinaryObject` aggregates with `--payload-min` and
  `--payload-max`, using the same inclusive byte bounds as `db query`.
  Filters apply before accumulation; changing them during resume refuses
  without changing the saved job. Empty results still require a completed scan.
- Group optional Blob values and count their DISTINCT projection with both
  `db query` and `db aggregate`. Absence is one distinct value/group, separate
  from empty bytes. `optional,count:optional` keeps the absent group with count zero.

### ▶️ Try it

```sh
make db
db aggregate BinaryObject --select count --payload-min 0x00ff80 --payload-max 0x00ff80 --work 1
db aggregate BinaryObject --select max:payload --payload-max 0x00ff --work 1
db query BinaryObject --select distinct-count:optional
db aggregate BinaryObject --select optional,count:optional --work 1
db query BinaryObject --payload-prefix 0x00ff --select payload
db aggregate BinaryObject --payload-prefix 0xff --select count --work 1
db schema DefaultRow
```

The temporary fixture contains `0x00ff80`: the first command returns one;
the second completes with no maximum. Its absent optional value contributes one
DISTINCT value; the optional-field group command shows the absent group with count zero.
The prefix query returns `0x00ff80`; the prefix aggregate completes with count zero.
`DefaultRow` shows empty (`0x`) and binary (`0x00ff`) defaults.
These commands do not access deployed data.

### 🧪 Testing

- Release supplier review (2026-11-17): selected Platform Types 0.21.2,
  Runtime 0.23.16, Storage 0.74.1, Host Artifacts 0.80.3, Host Text 0.77.2,
  Wire 0.24.3 and Toko Tooling 0.44.11 match current stable, non-yanked Cargo
  index releases. Retain the qualified external Testkit 0.106.0 launcher and
  Build 0.84.1 release editor; defer their optional 0.108.0 / 0.87.3 refreshes
  to the next tooling-maintenance pass.
- Qualify Blob prefixes with DISTINCT projections and retained grouped min/max.
  Late winners can enter or leave the selected prefix; filtering, aggregate order
  and the result limit apply only after complete accumulation. Single-row work
  chunks and result pages survive repeated reopen on primary and index paths.
- Qualify retained Blob grouping and DISTINCT across sparse scans, indexed
  paths, repeated reopen, retries and completed-result pages. Independent byte
  keys establish exact results; group filters and output limits never truncate
  the input, and output refusal preserves progress. This is native engine
  qualification, complementing the existing Wasm upgrade checks.
- Check optional Blob groups across retained chunks and reopened stores on both
  scan and index paths. Absence contributes one DISTINCT key and a group with
  zero present values; empty bytes remain a separate, counted value.

## [0.21.0] - 2026-11-17

### ⚠️ Breaking

- `IndexValue::Blob(&[u8])` extends the public operand enum. Downstream exhaustive
  matches must handle it. Stored Blob bytes and existing schema identities do not
  change; adding an index still requires an explicit schema transition.

### ➕ Added

- Query bounded Blob fields with exact byte operands: comparisons, secondary
  indexes, ordered pages, DISTINCT, grouping and synchronous/retained min/max.
  Byte ordering is lexicographic, including empty values and embedded zeroes;
  text matching remains text-only. Existing row, key and job bounds still apply.
- Expose `BinaryObject` through local `db query` and `db count`. Payload bounds
  accept exact `0x`-prefixed byte pairs, including `0x` for empty bytes. Project,
  order, deduplicate, count/group and take min/max using the existing engine.
  Render bytes as lowercase hex and absence as `—`.
- Run `db aggregate BinaryObject` through the existing retained-job endpoints:
  counts, DISTINCT counts, min/max and payload groups. Show progress and per-call
  charges, then release the job. These commands aggregate all rows; payload range
  flags remain specific to `db query`.

### ▶️ Try it

From the repository root, install the updated local demo:

```sh
make db
```

Then inspect its single Blob row (`0x00ff80`, optional field absent):

```sh
db count BinaryObject
db query BinaryObject
db query BinaryObject --payload-min 0x00ff80 --payload-max 0x00ff80 --select payload
db query BinaryObject --select payload --distinct
db query BinaryObject --select max:payload
db query BinaryObject --select payload,count
db aggregate BinaryObject --select max:payload --work 1
db aggregate BinaryObject --select payload,count --work 1 --page-size 1
```

Each invocation uses a fresh temporary canister; it does not access deployed data.

### 🧹 Cleanup

- Use directory module roots consistently in the Wasm qualification canister,
  preserving module paths and behavior.

### 🩹 Fixed

- Bound CI jobs using recent Linux/macOS timings, with shorter work-step limits
  and reserved failure-evidence time. Retain step status even when interrupted
  setup cannot finish its sanitized excerpt; keep original failures visible.
- Retain bounded, sanitized compiler/tool/scanner setup diagnostics when CI fails
  before tests start. Preserve the original failure, upload only selected evidence
  for seven days, and report missing evidence or failed uploads explicitly.

### 🧪 Testing

- Release supplier review retains Host Artifacts 0.80.2, Build 0.84.1 and the
  Testkit 0.106.0 launcher. Optional updates to 0.80.3, 0.87.3 and 0.108.0,
  respectively, are deferred to the next dependency-maintenance batch.
- Check retained scalar/grouped Decimal averages through the existing console
  adapter across real upgrades: signed ties-to-even rounding, unequal chunks,
  absent versus zero values, widened totals and intact output-budget retries.
  No new arithmetic implementation or endpoint is added.
- Check Blob CLI parsing, displayed results and call accounting; direct console
  queries preserve empty/absent/non-UTF-8 values across upgrades and refuse
  oversized operands, wrong-entity filters and insufficient output intact.
- Check all retained Blob selectors across upgrades, refusal/retry, query binding
  and source invalidation. Thirty completed jobs reuse the 16 slots through
  explicit cancellation; incomplete calls expose no aggregate result.
- Extend retained-aggregation Wasm coverage to optional text, large key/winner
  pairs, upgrade recovery and completed-result pagination. Verify that output
  limits and checkpoint exhaustion preserve progress. Blob winners also survive
  upgrade and output-budget refusal/retry. See the
  [variable-sized value witness](qualification/README.md#retained-variable-sized-values-0210).

The existing local fixture demonstrates optional-text aggregation:

```sh
db aggregate Document --select entry_id,min:note --work 1
db aggregate DecimalRow --select avg:value --work 1
db aggregate DecimalRow --select zone,avg:maybe --work 1
```

The larger datasets are qualification-only; run `make m2-job-envelope` to exercise them.

## [0.20.0] - 2026-11-16

### ▶️ Try it

```sh
make m0-setup
make db
db count Document
db aggregate Entry --select count --work 2
db schema Document
db schema Document --json
db schema --json > schema.json
db
```

These inspect a fresh local fixture. JSON contains display metadata, not an
importable schema or a deployed-database protocol.
At `db>`, enter `count Document`, then press ↑ and Enter to repeat it. ↓ returns
toward newer commands or your draft; ←/→ edit the line. Use `quit` to leave.
Quoted bounds now work there too:

```text
query Document --select entry_id,min:note --group-min "a b"
query Document --select entry_id,min:note --group-min ''
```

### ➕ Added

- Split schema reference details into `References` (for example, `Entry.id`)
  and `Reference index` (for example, `(entry_id, score)`) columns.
- Accept quoted text, empty strings and backslash escapes in console commands.
  Parse argument words with Shlex, then use the existing Clap command grammar;
  no shell expansion or execution. Incomplete quotes/escapes refuse before a
  database call and leave the interactive session usable.
- Add interactive command history and cursor editing with Rustyline, the same
  library used by IcyDB's shell. Retain 100 session-local commands without a
  history file. Ctrl-C cancels input and Ctrl-D exits an empty prompt. The
  foreground editor waits until each command finishes before showing its prompt.
- Show query/update call kind, elapsed seconds and measured cycles charged for
  every user-requested database call, including aggregate/migration steps.
  Elapsed time excludes fixture startup and balance sampling; cycle charges are
  the observed canister balance decrease, with exact values beside rounded
  millions. Statistics follow normal results; JSON and failed-command statistics
  use stderr. A zero charge
  does not claim zero computation; balance increases are reported explicitly.
- Add `db schema [ENTITY] --json`. A single invocation emits one JSON document
  with the full schema fingerprint, selected entities, fields, indexes, references
  and reachable named types. Unknown entities fail with empty stdout. Existing
  tables remain the default; JSON encoding stays in the native CLI.

### 🔄 Changed

- Inspect retained-job frames through borrowed slices for cancellation/expiry,
  then use the supplier's discard deletion instead of retrieving removed values.
  Keep one framing validator shared with resume and preserve identity, malformed
  input and rollback checks. Full 16-job cleanup drops from 3.58M to 2.66M
  instructions and 10.60M to 9.68M charged cycles in the matched local fixture;
  this optimization does not change stored bytes or lifecycle semantics.
- Separate retained aggregation state from the row codec's size policy: jobs
  now admit 16 KiB including progress/metadata; rows remain at 4 KiB. Group,
  DISTINCT and per-message limits stay unchanged. The numeric 32-by-32 DISTINCT
  witness now completes instead of exhausting its checkpoint at 293 values.
- **Retained-job format v2:** existing v1 jobs remain readable and resume without
  a reset. The first successful insert/save atomically updates the store marker;
  older executables then refuse it. Keep a v2-capable executable afterward.
  Cancellation/expiry can clean v1 stores without promotion; row/schema formats
  are unchanged. Upgrade, rollback and older-executable refusal are qualified.

### 🔧 Tooling

- Add `make m2-job-envelope`: real Wasm checks for 32-group count/sum/average
  finalization, DISTINCT limits, filtered/ordered completed pages and retained
  checkpoint refusals across upgrades. Record per-call instructions and cycles;
  a wider two-field grouped DISTINCT fixture reaches its 16 KiB frame at 790
  tuples and safely refuses the next. Fill all 16 slots near that bound and
  qualify invalidation, cancellation, bounded/full expiry cleanup, rollback and
  stale-handle refusal after reuse, with measured instructions and cycles. See the
  [qualification receipt](qualification/README.md#retained-aggregate-envelope-020).
- Clarify the preferred next design slice: explicit-target application inspection
  through Canic, followed by a typed application listing. Specify bounded metadata,
  maintenance/refusal reporting, unavailable remote cycle charges and unknown
  update outcomes. This is a design contract, not delivered remote CLI support.
- Give local console sessions a 24-hour idle timeout instead of PocketIC's
  60-second default. `quit` and EOF still close the owned server immediately;
  ordinary test runs retain their existing timeout. Unexpected input-pipe closure
  now explains that the session ended and a new temporary fixture requires `db`.
- Fix the interactive console stopping on its first terminal read. The foreground
  CLI forwards input through a pipe to Testkit's separately owned child group;
  counts return without EOF, errors return to the prompt, and `quit` exits without
  another keystroke. A controlling-terminal regression covers this launch path.
- Build the prepared PocketIC launcher with the optimized release profile.
  Local asset validation fell from 5.69 s to 0.37 s with the same archive,
  executable and version checks; an installed `db count Document` took 4.09 s,
  down from the observed 9.52 s. Launcher admission remains separately measured
  overhead; no background server or validation bypass is added.
- Adopt Toko Tooling 0.44.11 through its published installer in both workspaces.
  The baseline adds repository-qualified issue references and bounded, sanitized
  CI setup-failure diagnostics. Executable tooling is unchanged.
- Adopt IC Host Text 0.77.2 in the native CLI. Use its `escape` and
  `bordered_table` APIs directly; remove redundant per-cell escaping and column
  conversions. Table layout and terminal-safe rendering remain covered.

## [0.19.0] - 2026-11-16

### ▶️ Try it

Rebuild the local fixture CLI, then compare maintained and filtered counts:

```sh
make db
db status
db count Entry
db count Document
db count DecimalRow
db query Entry --select count --category 1
```

Expect format `ICDBMF02`, whole-entity counts 6, 1 and 3, and filtered count 3.
Each command uses a fresh temporary fixture, not a deployed database.

### ➕ Added

- Add `Database::cardinality::<E>()` for exact whole-entity counts from maintained
  metadata. Update counts atomically with revisions, rows and indexes; validate
  rebuilt counts before schema-copy publication. Fluent `count()` still validates
  rows and supports filtering. No schema attribute changes or prefix counters.
- Add `db count ENTITY` for the maintained count of Entry, Document or DecimalRow
  in the local fixture. Filters and limits refuse; filtered counts remain
  `db query ENTITY --select count`. Normal output contains only the count table;
  `db count --help` explains the metadata/integrity distinction.

### ⚠️ Changed

- Hard-cut stored formats to `ICDBMF02`, `ICDBMT02` and `ICDBCP02`. Entity metadata
  now contains revision and count (16 bytes). Earlier live or maintenance formats
  refuse intact; this experimental cut supplies no converter or automatic reset.
  Schema fingerprints and row/index encodings are unchanged. `db status` shows
  the new live format.

### 🔧 Qualification

- Release supplier review (2026-11-16): direct selections of Platform, Storage,
  Wire, Host Artifacts and Toko Tooling match their latest stable registry releases.
  Defer optional Host Text 0.76.0 → 0.77.2 and release metadata tool IC Build
  0.84.1 → 0.87.3 to the next dependency-maintenance batch. The external Testkit
  bootstrap remains 0.106.0; review 0.107.2 at that same boundary. No Testkit
  library remains in the driver. Frozen IcyDB supplier selections stay unchanged.
- Remove the retired Testkit lifecycle library and its old native Platform graph.
  The harness uses PocketIC directly and explicitly releases instances; the
  prepared command launcher owns the server. `make m0-setup` prepares that external
  bootstrap, existing Make probes use it, and standalone `db` still launches
  normally. Missing endpoints never trigger driver-side binary discovery or
  downloads. [Ownership and checks](qualification/README.md#external-simulator-ownership-019).
  Lint native callers/schema builds on the host and all fixture libraries on
  Wasm, where Platform's IC system calls are available; retain feature-specific
  Wasm checks and the full release behavior gate.
- Refresh Platform Types to 0.21.2 and Runtime/macros/guard to 0.23.13 across
  product and qualification selections. Existing identity/endpoint APIs and
  durable codecs are unchanged; the new raw memory/cycle functions are not used
  by IC DB. Check identity keys and installed endpoint/callback rollback.
  Older supplier-owned selections remain explicitly documented in the
  [refresh receipt](qualification/README.md#platform-patch-refresh-019).
- Exercise all 16 retained aggregate slots together across four query shapes,
  two source entities and three upgrades. Check selective invalidation, stable
  completed results, capacity refusal, stale-handle protection after slot reuse
  and bounded expiry cleanup. Reuse existing host controls; no engine, scheduler
  or stored-format change. [Receipt](qualification/README.md#shared-retained-job-capacity-019).
- Adopt Toko Tooling 0.44.9 through its published installer in both workspace
  selections. Its executable payload is unchanged; the baseline clarifies named
  supplier adoption scope and frozen comparison exceptions.
- Check the count command through the installed CLI and direct Wasm calls,
  including empty entities, unsupported names, filter refusal and unchanged
  backing across upgrade. No new count implementation or stored metadata.
- Adopt Platform Types 0.21.1 and Runtime/macros/guard 0.23.12. Qualification
  hosts declare guards, codecs, bounds and trapping policy explicitly; upgrades
  stop the local canister before replacing its executable and restart it afterward.
  Cross-runtime identity preservation, endpoint refusal, commit rollback and
  maintenance probes pass. Schema syntax and durable encodings are unchanged.
  Storage and frozen IcyDB retain their older raw-import dependency; Canic's
  separate integration is not upgraded by this change.
  [Receipt](qualification/README.md#platform-adoption-019).
- Qualify and move the typed database adapter into Canic. Its focused `db-test`
  checks actual-caller authorization, bounded pages/cursors, atomic refusal,
  five upgrades, maintenance/cancellation and reply-encoding rollback. Retire
  IC DB's temporary adapter patch/runner; Canic owns the sole implementation.
  The IC DB engine and durable format are unchanged; remote CLI access remains
  pending. [Receipt](qualification/README.md#canic-typed-application-integration-019).

```sh
make -C ../canic db-test
```

- Add isolated `make m2-prefix-candidate`. Prototype first-component counters
  in a disposable source copy, retaining scanned counts and the main engine's
  format. At 512 rows the selected count falls from 7.80M to 0.67M instructions;
  inserts add 7.31%, category-changing replacement 12.06%, same-category replacement
  1.13%, and the host adds 7,955 raw Wasm bytes. Native invariants, upgrades and
  IC rollback probes qualify the narrow candidate. Opt-in schema declarations,
  broader type/fan-out coverage and bounded publication validation remain before
  product adoption. This candidate is now parked while application integration
  and engine maturity take priority; see the [receipt](qualification/README.md#maintained-prefix-count-candidate-019).

```sh
make m2-prefix-candidate
```

- Add `make m2-aggregate-paths` to compare primary scans and explicit-index
  grouping over the same balanced/skewed datasets. All 3,072 executions pass.
  Missing-category grouping becomes about 7× cheaper, but full grouping becomes
  4.1–4.2× more expensive through the index. Keep the planner default unchanged.

```sh
make m2-aggregate-paths
```

- Add `make m2-query-matrix`: 195 query cases across balanced/skewed populations
  of 32/128/511 rows, before and after upgrade. All 4,680 executions pass their
  result/refusal checks; retain 1,908 successful matched pairs and exclude 432
  legacy sort refusals. Record per-query CSV and family summaries. IC DB wins
  most selected cases, but populated prefix counts cost up to 14.4× and selective
  grouped queries up to 2.6× legacy instructions. No product optimization is made.

```sh
make m2-query-matrix
```

- Add `make m4-lifecycle`: an independent map model checks 153 attempted batches,
  612 complete read checks, exact-job invalidation, six upgrades and a bounded
  schema copy after accumulated writes. Cover unique swaps, reference chains,
  repointing, refused/no-op backing preservation and target cleanup. Record full
  read/modify/write instructions and observed cycle deltas; this is scoped
  qualification, not a matched IcyDB performance comparison.

```sh
make m4-lifecycle
```

- Qualify the adopted count format through native invariants, real IC write traps,
  growth refusal/retry, retained-job invalidation and copy/cutover/recovery. The
  final matched host adds 967 Wasm bytes, with about 0.11% extra instructions on
  the sampled read/modify/replace journey; whole-entity count remains 574,791
  instructions at 512 rows. See the [adoption receipt](qualification/README.md#maintained-count-adoption-019).
- Add isolated `make m2-count-profile` to separate opening, raw traversal,
  owner-row fetches and validated counting. Owner fetches dominate the sampled
  indexed-count increment; caching opening does not fix its scaling. Record the
  [decomposition](qualification/README.md#count-cost-decomposition-019),
  preserving the existing integrity contract and original comparison artifacts.
- Add `make m2-count-comparison` at 32/128/256/512 rows. Compare maintained
  whole-entity counts and scanned category counts with legacy's metadata path.
  Preserve the initial [scan tradeoff](qualification/README.md#exact-count-scaling-019)
  separately from the adopted metadata results. Check backing and exact results
  across upgrades. Correct the earlier grouped receipt's access-path
  description: IC DB used its unordered primary scan, not automatic index selection.
- Add `make m2-aggregate-comparison` using the existing 32-row scalar hosts.
  Compare public fluent category grouping with count, sum, minimum and maximum,
  including residual filtering, multiple groups, empty results and upgrade.
  Keep independent expected tuples and unchanged-backing checks. Exact legacy
  Decimal sum conversion belongs to the benchmark adapter; product code and
  frozen IcyDB remain unchanged. Four-group queries use 74.0–76.0% fewer
  instructions in this fixture; see the
  [comparison receipt](qualification/README.md#matched-grouped-aggregates-019).
- Correct stale private-release wording in `make help` and the design's 0.18
  release status.

## [0.18.0] - 2026-11-16

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

- Authorize public source releases to `dragginzgame/ic-db` and require public
  visibility in release preflight and push checks. All crates remain unpublished;
  source releases do not publish packages or deploy canisters.
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
