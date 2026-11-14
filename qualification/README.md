# Database qualification

This independent workspace contains disposable M0 mechanism probes and an M1
test host consuming the actual product library. The product crate has no dependency
on this workspace. Published suppliers and exact
resolved versions are recorded in [Cargo.lock](Cargo.lock). The comparison fixture
also consumes IcyDB at an immutable Git revision; it is not an IC DB dependency.

Probe schemas are synthetic examples of generic database invariants. Names such
as users and mappings in existing fixtures do not prescribe product entities or
establish application compatibility. The private Toko GitHub repository is a
separate tier-2 coverage target; the obsolete local checkout is not evidence.

## M1 persistent records

```sh
make m0-setup
make m1-records
make db
db schema
db query --limit 3 --min 40
db query --after 3 --limit 3 --min 40
```

The first scalar slice uses the real `ic-db` library and `#[entity]` generator.
Native checks cover canonical codecs/schema identity, cross-entity batches,
final-state uniqueness (including swaps), unchanged refusals, indexes and reopen.
Literal keys and payload bytes supply independent representation checks.
Typed reads decode once; generated key extraction reuses the decoded value.
Batch construction extracts keys from the value it has just validated/encoded.
Stored-key inspection in mutation and reference preparation now uses one shared
schema-driven reader. It borrows text/blob payloads and retains only bounded key
scalars, replacing generated whole-entity decoder callbacks. A native corruption
witness puts a 1,024-byte blob before keys and text after them: bad lengths,
presence/boolean tags, UTF-8, trailing bytes and physical-key disagreement refuse
replacement, deletion and reference insertion without mutation. A malformed
non-key field on a surviving dependent also refuses reverse-reference inspection.
This does not qualify selective fluent query execution or measure cycle savings;
the supplier still materializes the row buffer and all fields remain validated.
Corruption controls inject malformed rows, mismatched physical primary keys and
stale index owners, ensuring this reduction preserves the read trust boundary.
Stored fields now include signed/unsigned 8/16/32/64/128-bit integers, Bool, Principal, bounded UTF-8 Text, Blob and
their optional forms.
Signed tests cover the complete i64 endpoints, negative/zero values, absence,
literal fixed-width little-endian vectors, truncated/extra payloads, no-op revision
preservation and incompatible signed/unsigned schema refusal. Native and Wasm
reopen checks preserve the exact values. Signed keys/indexes remain unimplemented.
The separate WideNumbers fixture checks required/optional 128-bit integers,
including signed extrema, unsigned maximum and values just beyond 64 bits.
Independent 16-byte scalar vectors and schema hashes qualify encoding;
truncation, surplus bytes and incompatible width/signedness refuse. Native and Wasm
checks preserve all bits across writes, no-ops, traps and reopen. Wide keys/indexes
and arithmetic are not enabled by storage support.
NarrowNumbers exercises required and optional 8/16/32-bit integers using IcyDB's
exact-width/extrema regression cases. Independent literal payloads and a schema
fingerprint cover 1/2/4-byte encoding, signedness, all truncated prefixes, surplus
bytes and invalid presence tags. Native reopen rejects changed widths/signedness
without mutation; Wasm checks extrema, absence/zero, no-op revisions and late-trap
rollback. Native field types enforce representability before database entry;
there is no cast from a generic widened value that could silently truncate.
IdentityRow carries over IcyDB's principal identity cases: management (zero bytes),
anonymous, short opaque identities and the 29-byte maximum. Native tests use an
independent contract fingerprint and literal framed bytes. Absent and present-empty
optional principals differ; 30-byte values, excessive lengths, invalid presence,
truncation and trailing bytes refuse. Corrupt accessed principal fields also refuse
replacement/deletion during shared key inspection without changing backing.
Wasm checks required/optional identity bytes, raw decoder refusals, no-op revisions,
late-message rollback and unchanged upgrade. Wire transports opaque test bytes;
the product uses Platform Types for identity and its own codec for persistence.
Principal secondary indexes are now qualified alongside signed/unsigned integer
indexes. Native checks cover all widths, exact operand types, Principal ordering
at every length, compound boundaries, unique exchanges, corrupt entries and
restricted deletion through mixed-type reverse indexes. Wasm checks exercise
Principal uniqueness and signed score ordering, including duplicate secondary
values, final-state swaps, refusals without writes, late traps and unchanged
upgrades. Original Nat64 key bytes/fingerprints remain unchanged.

Blob qualification carries over IcyDB's empty, zero-bound and over-limit cases.
Literal length-framed payloads contain non-UTF-8 bytes; absence differs from an
empty blob. A 1,024-byte bulk value is encoded directly within field/row bounds.
The private raw-decoder query checks literal payloads, invalid presence tags,
truncation, trailing bytes, u32::MAX lengths and oversized rows on 32-bit Wasm;
Wire carries these bytes opaquely and does not encode/decode the stored record.
Malformed, oversized and incompatible stored values refuse. The BinaryObject
Wasm fixture verifies limit refusals before mutation, no-op revisions, message
rollback and unchanged upgrade; whole-row limits remain independent of fields.
Checks distinguish absence from empty/zero/false, count text bytes rather than
characters, reject oversized writes and stored payloads, retain the whole-row byte
envelope, and preserve values and per-entity revisions across reopen. Independent
SHA-256 vectors cover the original Nat64 contract, mixed types and references.
Required Nat64 references use a declared ordinary index beginning with the
reference field. Native checks cover missing targets, final-state insertion,
repointing/deletion, cycles, self references and a 31-dependent removal batch.
Wasm checks verify the same core refusal/composition boundary and reopen. No
recursive cascade or optional/nested reference support is implied.
The Wasm host checks generated inspection, sparse page progress, indexes,
normal errors versus late application traps, atomic revisions and unchanged
upgrade/reopen. Full raw stable-memory comparisons include supplier metadata.
`make m1-commit-failures` separately builds the host's `fault-injection` feature.
It delegates backing identity and IO to the real supplier, traps after each
observed backing write in a three-row replacement including a dependent repoint,
and rejects a real `grow()`
request with `-1` during a maximum-cardinality insert batch. Raw backing/extent,
indexes and revisions must revert; the identical operation must succeed on retry.
It also exhausts the actual IC instruction meter after the first backing write
inside that replacement. A fixed 4 KiB scratch buffer is repeatedly filled until
PocketIC reports `CanisterInstructionLimitExceeded`; no synthetic budget counter
or explicit application trap substitutes for that failure. Full raw backing,
revision, reference and index checks remain unchanged after rejection. The later
successful replacement proves the instruction fault does not leave the host armed
or poison retry. This carries over IcyDB's instruction-refusal preservation
invariant using IC DB's message rollback boundary, without its budget machinery.
The current six-entity fixture with typed indexes, codec v3 and Storage 0.74.1 covers 73 replacement writes and reaches growth
with the insert batch starting at ID 1700: successful retry performs 1,362 backing
writes, one growth request and expands from 26 to 34 physical pages. These are
version-specific observations, not layout assertions or performance measurements.
The test discovers boundaries instead of depending on those counts. Reopen after
unchanged upgrade also passes. Instruction exhaustion is qualified at this selected
post-write boundary; completion within the instruction envelope for all accepted
shapes, additional corruption cases and other workloads remain M1 work.

Private Wire exports are test controls, not a production adapter. `make db`
installs six Entry rows and one record each for Document, WideNumbers, BinaryObject, NarrowNumbers and IdentityRow;
every CLI session is fresh. Clap
owns argv and console command admission; empty console lines remain silent.
Fault controls are compiled only for the dedicated probe and excluded from the
ordinary CLI host. The product library has no injection feature or durable fault
state; the instrumented host must not be used for comparative performance claims.
The M0 maintenance fixture remains available only through its dedicated probe.
Product scope still includes richer types, broader reference constraints and broad fluent
queries. Primary keys/references still require Nat64; secondary indexes admit
required integers and Principal. Text and optional indexing remain required
follow-on work. `db schema` inspects all six entity descriptions, including the
Document-to-Entry reference, its `(entry_id, score)` reverse index and IdentityRow's
unique owner index;
the current `query` command reads Entry only. The private Wasm controls test
Document updates and unchanged-upgrade persistence.

The M1 selection updates Storage to 0.74.1, Runtime to 0.22.3 and Types to 0.20.3.
Prior recorded M0/baseline results used the versions stated with that evidence;
the updated lockfile does not retroactively requalify those results.

## Frozen IcyDB baseline

```sh
cargo fetch --manifest-path qualification/Cargo.toml --locked
make m0-setup
make m0-icydb-baseline
```

Fetching requires read access to `dragginzgame/icydb`. The benchmark itself runs
offline with the prepared PocketIC server. It uses Git revision
`6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac`, never a moving sibling checkout.
The two small comparison packages exist because IcyDB's actor generator consumes
schema registration in a native build script and the same authored types in Wasm.
They are qualification infrastructure, not a proposed IC DB crate split.

The source is frozen; suppliers resolve through **this workspace's lockfile**,
not IcyDB's original lockfile. The selected Wasm facade has no features, its core
has `default`, and the native generator adds `build`; SQL is absent. The feature
tree and compiler/build records are saved under `target/evidence/icydb-baseline/`.
Compare against this complete selection, not historical audit numbers. The
numbers currently recorded in the design were measured with Storage 0.74.0,
Runtime 0.22.2 and Types 0.20.2. Rerun with the current selection before a matched
product comparison; the older artifact and measurements remain historical evidence.

The fixture uses one journaled entity with `id`, `category` and `amount` Nat64
fields, external IDs, and one `(category, id)` secondary index. It inserts 32
rows individually, checks present/missing primary-key reads, refuses a duplicate,
reads a category in ID order, changes an indexed category through a complete
read/modify/replace journey, and verifies all rows and category results after an
unchanged executable upgrade. Independent tuples supply the expected results.

Instruction counter 0 surrounds request accounting, session acquisition, database
work and result mapping; Wire request/reply encoding is outside that interval.
Update cycle deltas include the complete host call and system charges; they are
not pure database instructions. Query cycles are not inferred from those deltas.
The driver also advances startup/journal work after every write, including a
refusal, and reports those steps/instructions/cycles separately. This deliberately
measures a settled database between requests; it is not a throughput workload or
the cost of Canic's production scheduling policy. Count that work when comparing
the complete journey rather than treating a successful write return as all work.
The driver reports complete uncompressed Wasm and code-section payload bytes
through Host Artifacts' bounded Wasm inspection, plus the artifact SHA-256.
The recipe is this workspace's `release` profile (`z`, fat LTO, one codegen unit,
abort, stripped symbols), without a post-link optimizer. It is an M0 comparison
recipe, not a selected production deployment optimum.

Startup is explicitly advanced by the driver, with fixed test-only entropy and a
recorded recovery wakeup. No Canic endpoints, watchdog scheduling, production
authorization or general failure recovery are qualified by this host. IcyDB's
native schema macros require generated constructor registration; the comparison
schema permits those macro-owned internals under `deny(unsafe_code)`, while the
ordinary probe libraries and console retain `forbid(unsafe_code)`.

This standalone target measures one engine. The matched scalar comparison below
now covers IC DB's delivered record operations. High fan-out, rich records,
multi-entity growth, aggregate costs, maintenance and Canic composition remain
separate measurements.
This focused target is not implicitly run by scaffold CI or release qualification.

## Matched scalar records

With the server already prepared, run:

```sh
make m1-records-comparison
```

This adds one private qualification host, not a product crate. Both hosts expose
the same five query/update controls and use one three-Nat64 entity with a
`(category, id)` index. A single shared driver runs the baseline's independent
tuple oracle: 32 inserts, duplicate refusal, hit/miss reads, ordered category
listing, index-changing read/modify/replace and all-row/index checks after upgrade.
The IC DB host calls the actual product, without fault controls or the CLI's other
four entities. No engine code is copied into the host.

The IC DB interval includes construction of its storage runtime, compiled schema,
manifest admission, database operation and result mapping on every request.
There is no hoisted setup or benchmark-only cache. Initial database creation
runs in its init hook; installation cycles are reported separately, so its first
open call must not be presented as its entire setup cost. Subsequent writes have
no settling message. IcyDB keeps the baseline's explicit settling policy, reported
separately and included in complete write totals. Neither side uses Canic.

Both sides use the same locked Storage/Runtime/transport, Wasm release profile
and PocketIC application-subnet setup. The script records source commit, dirty
diff/status, tracked and untracked source hashes, compiler, flags, feature graphs,
build artifacts and raw measurements under `target/evidence/records-comparison/`.
Artifact inspection checks reachable query/update endpoints and records all
exports. Re-running replaces that local evidence; published numerical observations
and artifact identities remain in the [system design](../docs/design/ic-db-system-design.md#first-matched-scalar-comparison).

The first comparison passes the initial scalar ceilings, with all 32 IC DB insert
positions below their IcyDB counterparts for measured instructions and update
cycles, even before settling is included. This is an observation, not a general
speed or size guarantee. IC DB's listing uses a direct index prefix; IcyDB admits
the equivalent filter/order/limit through its fluent planner. Their richer
contracts, durable machinery and reachable internal code differ. Do not attribute
the full Wasm difference to removing SQL: SQL is disabled in the IcyDB build.
The measurement adds no product state, codec, allocator or dependency change.
The later Principal increment is measured separately in the design: it adds a
canonical identity dependency and codec branch. The scalar host grows by 410
Wasm bytes; measured operations rise by 6–10 instructions, staying below the
IcyDB comparison ceilings. This does not measure principal-heavy query workloads.

## Numeric and schema contracts

Run `make m0-contracts` without PocketIC setup. These native witnesses use the
existing driver package, with no new package, durable state or product API.
Schema fingerprint qualification uses the approved SHA-256 implementation through
the dev-only `sha2` dependency (registry-verified 0.11.1, already resolved transitively).

The decimal candidate takes an i128 coefficient and scale 0–38. Its canonical
ordered key makes `1.0` and `1.00` equal. An independent fixed-precision decimal
oracle checks 91 boundary/scale values in 8,281 pair comparisons, then verifies
uniqueness acceptance and grouped counts. Negative extremes, zero, equivalent
scales and large adjacent integers that collapse through f64 are included.
The 41-byte key is a deliberately simple feasibility construction, not a selected
storage format: its size and conversion costs need Wasm and index measurements.

Separate witnesses cover finite f64 ordering with equal signed zeros and refusal
of non-finite values, all four limbs of a U256 stand-in, and optional-decimal/ID
composite order against component comparison. They do not qualify numeric
arithmetic, aggregate precision/overflow, mixed-type promotion, Float32, production
codecs, supplier integration or the complete retained type surface.

The schema witness serializes one bounded canonical contract with field names,
explicit entity/index IDs, named enum variants and versions for row/key codecs, comparison,
normalization, absence ordering, constraints, defaults/timestamps and supplier
format. A literal byte oracle fixes the example; 19 changes to stored behavior/layout
change its identity, including field renames. Fields are ordered by their exact
names; declarations can reorder, but field names are not display-only labels.
Entity/index source labels with explicit IDs remain outside identity. Variant
renames change identity; reordering variant declarations does not. Index column order
remains semantic. Duplicate/empty/oversized field names, duplicate IDs, dangling
index fields and selected invalid bounds/versions refuse. Renaming a field without
updating its referencing index refuses; updating both still changes the contract.
Primary-key membership and component order are included, with empty, duplicate
and dangling key components rejected before fingerprinting. SHA-256 covers the
domain `ic-db:schema:v1\0` followed by the canonical bytes. Its golden vector was
independently computed with OpenSSL over literal bytes; declaration-order/display
changes preserve it and semantic changes do not.

This tests candidate identity inputs and the selected hash, not a durable manifest format,
runtime admission implementation or automatic detection of changed code. A codec
or comparator change still requires its author to change the declared version.
Positional codecs cannot assume declaration reordering is compatible. Existing
Wasm maintenance probes separately exercise admission and transformation recovery.

## Historical M0 row codec candidate

The Wire candidate below is retained as historical experimental evidence. The M1
product now owns scalar row codec version 3 and has no IC Wire dependency.
Generated-record checks cover fixed-width decoding, truncation, trailing bytes,
malformed lengths and unchanged refusal of version 1 and 2 manifests. Qualification endpoints
still use Wire for messages, converting binary values at that transport boundary.

The design now specifies a finite compiled type graph, retaining recursive
values under separate value-depth/node/count/byte bounds. Fields, named types
and enum variants use schema names, not author-supplied numeric IDs. Stored enum
ordinals derive from canonical variant-name order within the admitted contract.
The existing identity witness now uses named variants (`ICDBM0C4`); its independent
OpenSSL vector changed deliberately. No product format or retained data exists
under either fixture encoding.

`nested_row_codec` adds one handwritten recursive row projection using published
Wire 0.22.1 and test-only Serde 1.0.229 (registry-verified, already transitive).
The application struct and enum declaration orders deliberately differ from their
storage traversal. A literal payload oracle checks canonical order; map/set
construction order does not affect bytes, while list order and absent/empty
values remain distinct. Decode admits entry sequences before constructing maps
or sets, rejecting duplicates and noncanonical entry order rather than silently
discarding entries. Unknown enum ordinals and payload mismatches also refuse.

The sample permits eight recursive nodes, three levels, three entries per
collection and eight UTF-8 bytes per string. An all-maximal sample with u64 maxima
must round-trip under 512 payload bytes, 512 Wire visits, depth 32 and a 64-KiB
allocation allowance. Wire's allowance conservatively charges traversal and is
not actual heap allocation; the initial 8-KiB allowance refused otherwise valid
sample values. Database limits remain separately enforced in both directions,
and a low value of each Wire allowance refuses independently. These numbers are
fixture limits, not proposed product budgets or IC instruction measurements.
The fixed enum payload uses a tuple stand-in; it does not fix the product's
variant framing or qualify a macro generator. Generated traversal, every retained
type combination and reopen integration remain M1 obligations.

For only these new/changed contracts:

```sh
cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test schema_identity --test nested_row_codec
```

`make m0-contracts` also runs `row_codec`, a fixed `(amount, id, note)` tuple in
canonical field-name order. Wire 0.22.1 owns payload serialization; a small
database-owned frame and externally admitted schema identity precede decoding.
The literal example occupies 19 bytes: 12 fixture framing bytes and 7 payload
bytes, with no repeated field names. This is not a production overhead budget.

Checks cover absent versus empty notes, u64 extremes, UTF-8 byte lengths,
truncation, unsupported framing, length mismatch, extra payload bytes, invalid
optional tags and refusal under each byte/allocation/node/depth limit. Database
field limits also refuse a valid Wire string that exceeds its declared eight-byte
bound. Fixture limits are 64 payload bytes, 2,048 traversal-allocation allowance,
32 nodes and depth 8; the allowance is not an observed heap maximum.

This does not qualify generated codecs, arbitrary Serde implementations, nested
types, index ordering, checksums, persisted supplier integration or Wasm execution.
Wire primitive decoding can accept noncanonical bytes; original-byte identity
must not be reconstructed by decoding and re-encoding. The product must bind
supported normalization and stored traversal to its versioned contract.

## Query composition contracts

`make m0-contracts` also runs the four-row examples from design section 5.
These use two small native accumulators, not a builder, planner or general query
representation. Exact expected outputs are literal, independently worked answers.
Work budgets 1–4 produce the same completed result and expose no prefix result.

Projection precedes DISTINCT: category has two distinct values, while the
category/value tuple has three. Counting these gives two and three respectively,
independently of output page size. Summing page-local distinct counts would give
the wrong answer. Grouping completes before the group filter, aggregate ordering
and output limit. Row filtering instead changes the input to the sums. Empty
completed output is distinguished from incomplete work.

Retained distinct keys and groups have independent cardinality bounds of at most
four. Exceeding a bound refuses without returning a truncated answer or changing
the native accumulator. Finalization and output collection visit at most four
entries; this is a structural toy bound, not an IC instruction/byte qualification.
Output offsets apply only to the fixed completed vector and are not live-page
cursors. Projection, filters and capacities remain fixed by the test caller;
there is no public resumable handle accepting changed query parameters.

Three additional optional-aggregate examples check row versus present-field
counts, distinct present-field count versus DISTINCT projection row count, empty
versus all-absent versus present-zero inputs, and average accumulation across
unequal chunks. Six inputs `[10, None, 30, 10, None, None]` give counts `6/3/2`,
sum `50`, extrema `10/30` and mathematical mean `50/3`, regardless of work budget
1–6. A six-row input cap makes a `u128` sum of `u64` operands exact; a literal
above-`u64` total checks that no float or narrow intermediate is used. The exact
average pair is a witness, not the public result representation. No supplier
allocation behavior participates in these semantics.

A grouped optional-input witness keeps one absent-key group, an all-absent
aggregate group and a present-zero group distinct. Output pagination includes
the all-absent group; only a subsequent explicit group predicate removes it.

The examples do not qualify builder method-order admission, persistent DISTINCT
state, authenticated result cursors, concurrent changes, general expressions,
general aggregate arithmetic/rounding or production resource limits. Existing Wasm probes own
durable job progress/retry and live keyset behavior. The retained product scope
is unchanged, and no package, dependency or durable representation was added.

## Write rollback

Prepare the installed Rust 1.100.0 Wasm target and normal repository tools, then:

```sh
make m0-setup
make m0-rollback
```

Setup explicitly provisions PocketIC through Testkit's authenticated installer
under `.tools/m0-pocketic`; the probe only checks that prepared selection. Testkit
owns server and instance lifecycle. Cargo's artifact messages select the compiled
Wasm; the probe does not infer an internal target-directory filename.
The selected Testkit is 0.104.0 with PocketIC 16.1.0. Calls use raw client methods
and this workspace's explicit Wire codec/budgets; no `ic-testkit-wire` adapter is
required. Testkit's native host helpers resolve to 0.77.2.

The fixture uses published IC Storage collections with one row map and two index
maps. Wire/Platform exports are private experiment controls, not product endpoints;
the product's Canic adapter remains separate. Every operation cold-opens storage.

The probe forces allocation growth and injects traps after reservation, row write,
each index write, and after the complete batch returns to surrounding application
code. It compares the entire stable-memory image before/after failure through
PocketIC, independently of database lookup, and enumerates each map separately.
It also checks pre-write refusal, uniqueness refusal, normal success, same-code
upgrade, and two negative controls: normal error after a partial row write retains
the partial state; normal error after a complete batch retains the complete batch.

This does not qualify internal supplier failure windows, instruction exhaustion,
cross-message callbacks, intra-batch constraint semantics, migration or performance.
It is evidence for the synchronous commit candidate only. A native panic is not
an IC rollback substitute. No live network canister is deployed.

See the [system design](../docs/design/ic-db-system-design.md) for current findings
and remaining M0 decisions. Aggregate scaffold CI does not run this probe.

## Final-state batches

`make m0-batch` uses additional controls in the rollback canister on a separate
fresh installation. The single-row and batch controls use different toy layouts;
do not mix them in one instance. No package or durable staging/journal is added.
This model uses numeric stand-ins for users, mappings and principals: IDs 1–16,
present principal values 1–100 (zero stands for absence) and at most eight commands
per batch. It is not a rich
codec, generic constraint engine or public database API.

Preparation builds only the bounded intended row changes in memory. It refuses
duplicate targets, conflicting principal ownership and invalid final references
before persistence. It accepts a mapping created before its user in request order,
unique-value swaps, and releasing/acquiring a principal across different mappings.
Commit removes touched old index entries, applies rows, then adds new index entries.
Five trap boundaries prove that partial removal/replacement restores the entire
stable-memory image; expected refusals also leave it unchanged.

The unique index has explicit present-only membership. Several mappings may have
an absent principal; no zero-key unique entry is stored. Absence-to-presence
conflicts refuse, while one batch may release a present value by setting absence
and assign it to another mapping. Returning every principal to absence empties
the unique index without removing rows or reverse references. Five trap boundaries
also cover that transfer; complete row/index expectations and same-code reopen
include absent values. This numeric sentinel is fixture encoding, not a proposed
optional-value codec or a proof for composite/nested partial indexes.

Restricted deletion uses a reverse `(user, mapping)` index and consults final batch
values. Deleted/repointed references do not block the user deletion, while an
untouched reference after the changed prefix does. Every skipped reverse entry
requires a distinct batch command, bounding examination by batch size plus one.
Deleting an unrelated user examines zero candidates despite other users' mappings.
An independent complete row/index oracle verifies accepted outcomes and reopen.
Every mapping must have a user; a user need not have a mapping.

Production byte/instruction limits, rich/nested references, malformed index
rejection, full constraint combinations and crash windows inside supplier calls
remain outside this small witness.

## Bounded reads and ordered DISTINCT

`make m0-reads` uses separate read controls in the existing rollback canister on
a fresh installation. Do not mix the single-row, batch and read fixture layouts.
Eight fixed rows have an ID, optional score and price. The row map supports ID
order; a `(score, id)` index supports score order with absence first. Requests
whose chosen path does not provide their requested order refuse. A complete-sort
oracle demonstrates why sorting a limited primary-key prefix would be incorrect.

Work/output row limits are independently bounded to 1–8. Sparse residual filtering
can return an empty page with a continuation after the last processed candidate.
Candidate and row-fetch counts are reported separately. The success-byte budget
covers the actual Wire reply, including its continuation and counters. A row that
fits a new reply but not the current reply is retried; one that cannot fit alone
refuses. Small bounded error replies are outside that requested success budget.
The fixture re-encodes tentative pages to check sizes; this deliberately bounded
technique is not a selected product executor or performance optimization.

The DISTINCT witness projects only the score from its ordered index, returns
`Option<u64>` results, and suppresses adjacent equal keys across work/page boundaries.
It returns five distinct values from eight rows, including one absent value, with
zero row fetches and no retained seen-set or durable query state. It qualifies only
this ordered, unfiltered projection; arbitrary DISTINCT still needs its own bounded
state/access-path contract. The accepted product scope is unchanged.

Continuations bind canister, fixture incarnation, contract marker, operation,
predicate and logical position. Invalid bindings/positions refuse; same-code
upgrade preserves valid cursors, and explicit fixture reset invalidates them.
These unsigned cursors are validated seek positions, not trusted evidence that
the caller visited a prefix. They contain no authorization or resource entitlements.
General schema identity, authenticated cursor claims and real host policy remain
outside the probe.

Tests independently check complete output, actual encoded byte lengths, unchanged
backing after queries and explicit live behavior: inserts/deletes are visible;
ordering updates can repeat a returned identity or omit an unseen one; filter
membership changes can enter/leave the remaining enumeration. This does not
provide exact snapshot enumeration or justify accumulating exact aggregates over
live pages. Production instruction envelopes, full bounded sorting, arbitrary
predicates/types and filtered/unordered DISTINCT remain unqualified.

## Resumable aggregation

After the same explicit `make m0-setup`, run `make m0-aggregation`. The existing
native driver package also owns this focused test; a separate Wasm fixture keeps
the original rollback experiment's allocation and failure cases unchanged.

The fixed operation groups rows by `id % 3`, sums their values, retains completed
groups with sums greater than 150, and orders by sum descending then group ascending.
It admits IDs 1–12 and values 0–1000. Each advance processes at most three rows
and examines one extra row for exhaustion; finalization visits at most three groups.
There is one server-held job slot, a monotonic source revision and explicit job/
sequence tokens. Partial results never appear as output. Completed output retains
its captured revision after later writes. Cancellation clears fixed-size state;
the next job receives a new identity, so old requests cannot advance it.

The Wasm test checks exact results against a full-input oracle, replay without
double-counting, raw backing rollback between accumulator/cursor/sequence writes,
row/revision rollback, pre-write refusal, restored values, deletion, same-code
upgrade, self-invalidation avoidance, cancellation, sparse input and empty input.
Twelve rows finish in four quiet advances. In a synthetic schedule with a source
write between advances, five out of five attempts explicitly invalidate; none
silently restart. This is a correctness/feasibility result, not a representative
representative write-rate measurement or a liveness guarantee.

The source and fixed state use two Storage allocations and no write journal or
historical rows. The fixture's scalar contract ID represents only this fixed
schema/query/algorithm; it does not implement general identity admission. Caller
authorization, expiry, changed-code recovery, multi-entity inputs, arbitrary group
cardinality/types and production instruction/byte budgets remain unqualified.
These limits do not reduce the accepted product capabilities.

## Identified maintenance and the local console

Run `make m0-maintenance` after setup. Cargo builds four executable variants from
one small fixture: source A, target B, B-prime with a different transformation
identity, and a target whose upgrade hook deliberately traps. Artifact messages
select each Wasm before it is copied into ignored build evidence; no internal
Cargo artifact path is assumed. The native driver tests these and the CLI.

Six source rows use scalar values. B writes a 16-byte row containing a normalized
value and its original value, and builds a unique normalized-value index. Copy
and cleanup process at most two rows per message; complete validation is bounded
by this six-row fixture. Durable state contains format, phase, transformation,
target format, copy position and the retired allocation slot. Tests establish:

- Traps restore chunk rows/index/progress, cutover and cleanup byte-for-byte.
- Source A refuses ordinary access while frozen; B-prime cannot resume B's work.
- Old values 11 and 12 collide in different chunks under the new unique rule;
  publication refuses and cancellation preserves the source.
- Cancellation cleans target rows/indexes, but source service requires A again.
- Target B reopens after cutover; A refuses even while source allocations remain.
- Missing/unsupported manifest identity is refused intact; a trapped upgrade
  preserves both the previous executable and stable memory.

Logical source cleanup leaves physical allocation at 50 pages. All four allocations
are predeclared for this fixed experiment. A subsequent capacity-only control
resets the fully cleaned source slot using IC Storage's `StableBTreeMap::reset`,
then copies the live target into six candidate rows, two per call. It tries row
widths 8 KiB, 96 KiB, 96 KiB and 8 KiB: larger rows must force physical growth;
equal/smaller repeats must reuse the same capacity. Observed physical pages are
50/58/58/58; slot logical pages are 2/13/13/13, with 64 KiB per page. Physical
allocation does not shrink when the candidate becomes smaller. Each reset and copy chunk is
trapped before retry, with byte-for-byte backing comparison. Reset before source
retirement or during copying refuses; same-code upgrade resumes partial copying.
The driver checks all candidate bytes in bounded pieces and the complete live
index, while original target queries remain intact.

This adds three probe control entries (state, position, width) in the existing
control allocation, no new allocation or dependency. It changes the retired
slot's codec from scalar to byte-vector records, not the published schema. It
does not publish any candidate, alternate live roles, prove transfer to a fresh
allocation, or qualify arbitrary collection types, exhaustion or production
validation/cleanup bounds. Full repeated-migration lifecycle qualification remains
open. The transformation is synthetic; full coverage belongs to the delivered engine.

## Installed console

`make db` builds and installs the release binary at `~/.local/bin/db` and the M1
host at `~/.local/share/ic-db/m1/records.wasm`; it does not start PocketIC or the
console. `DB_INSTALL_PREFIX` overrides `~/.local`. Maintenance variants are not
built for the console. Cargo artifact records select both
build outputs. Keep the installation's `bin` directory on `PATH`.

Run `db` to open a fresh temporary local record database. Commands are `schema`,
`status`, `query [--after ID] [--limit 1..3] [--min VALUE]`, `help` and `quit`.
Queries are read-only, order by ID ascending and inspect at most `limit` candidates.
The minimum-value filter can return an empty page with a continuation. The CLI
prints a next-page command preserving the filter/limit, or `Complete: no more
candidates.` A full final page may need one last empty call. Both schema information
and records come from the running fixture's query controls. Schema responses carry
structured field/index descriptions; only the native CLI depends on `ic-host-text`
and renders bordered tables. Full storage identities live in `db status`.

Use `db query --after 3 --limit 2` for one command, or pipe newline commands to `db`.
`db --help` needs no running server. The installed binary locates the fixture
relative to itself and works outside the repository directory. Testkit's prepared
server is still tied to the build checkout; run `make m0-setup` there before use.
Invalid commands/options fail scripts with a nonzero status;
interactive mistakes return to the prompt. The console never evaluates SQL or
shell commands. `quit` or EOF closes the Testkit-owned instance. Local fixture
endpoints are qualification controls, not a shipped Canic adapter or a new public
dynamic-query API; there is no remote connection or durable CLI session.
