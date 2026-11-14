# M0 qualification

This independent workspace contains disposable mechanism probes, not a database
engine. The product crate has no dependency on it. Published suppliers and exact
resolved versions are recorded in [Cargo.lock](Cargo.lock).

Probe schemas are synthetic examples of generic database invariants. Names such
as users and mappings in existing fixtures do not prescribe product entities or
establish application compatibility. The private Toko GitHub repository is a
separate tier-2 coverage target; the obsolete local checkout is not evidence.

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
explicit entity/index/variant IDs and versions for row/key codecs, comparison,
normalization, absence ordering, constraints, defaults/timestamps and supplier
format. A literal byte oracle fixes the example; 19 changes to stored behavior/layout
change its identity, including field renames. Fields are ordered by their exact
names; declarations can reorder, but field names are not display-only labels.
Other source labels with explicit IDs remain outside identity. Index column order
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

## Row codec candidate

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

`make db` builds and installs the release binary at `~/.local/bin/db` and source A
at `~/.local/share/ic-db/m0/maintenance-a.wasm`; it does not start PocketIC or the
console. `DB_INSTALL_PREFIX` overrides `~/.local`. Only source A is built for the
console, not the other maintenance variants. Cargo artifact records select both
build outputs. Keep the installation's `bin` directory on `PATH`.

Run `db` to open source A as a fresh temporary local database. Commands are `schema`,
`status`, `query [--after ID] [--limit 1..3] [--min VALUE]`, `help` and `quit`.
Queries are read-only, order by ID ascending and inspect at most `limit` candidates.
The minimum-value filter can return an empty page with `next_after`; only
`complete=true` ends enumeration. A full final page may need one last empty call.
Both schema information and records come from the running fixture's query controls.

Use `db query --after 3 --limit 2` for one command, or pipe newline commands to `db`.
`db --help` needs no running server. The installed binary locates the fixture
relative to itself and works outside the repository directory. Testkit's prepared
server is still tied to the build checkout; run `make m0-setup` there before use.
Invalid commands/options fail scripts with a nonzero status;
interactive mistakes return to the prompt. The console never evaluates SQL or
shell commands. `quit` or EOF closes the Testkit-owned instance. Local fixture
endpoints are qualification controls, not a shipped Canic adapter or a new public
dynamic-query API; there is no remote connection or durable session in M0.
