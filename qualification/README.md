# Database qualification

This independent workspace contains disposable M0 mechanism probes and an M1
test host consuming the actual product library. The product crate has no dependency
on this workspace. Published suppliers and exact
resolved versions are recorded in [Cargo.lock](Cargo.lock). The comparison fixture
also consumes IcyDB at an immutable Git revision; it is not an IC DB dependency.

Package directories match their Cargo package names. Qualification callers use
Wire 0.24's root `encode`/`decode` APIs with explicit `Encoding::Frame` direction
and existing method IDs, or `Encoding::Payload` for isolated codec probes. The
frozen IcyDB comparison retains its separate transitive Wire 0.22.4 selection;
there is no cross-version Wire type exchange or product Wire dependency.

Probe schemas are synthetic examples of generic database invariants. Names such
as users and mappings in existing fixtures do not prescribe product entities or
establish application compatibility. The private Toko GitHub repository is a
separate tier-2 coverage target; the obsolete local checkout is not evidence.

The M1 `EnumRow` fixture now indexes the all-unit `Lifecycle` type through
`state,id` and sparse unique `previous`. Its focused Wasm witness checks reads,
replacement cleanup, no-ops, invalid ordinals, whole-message rollback and upgrade.
Native `enum_indexes` tests independently inspect literal keys, exercise swaps,
refuse wrong enum definitions and corrupt rows/indexes, and distinguish absence.
Payload-enum indexes remain unsupported, including optional fields selecting a
unit variant; a compile-fail witness covers the generated boundary.

`db schema ENTITY` selects one exact declared entity name. The fixture projects
its admitted fields/indexes and transitive named types; the CLI owns all tables.
References carry target names so selecting one entity does not require rendering
its targets. Recursive types appear once. Console checks cover scalar, enum,
record/list/map recursion, cross-entity references and unknown-name refusal.

`EnumCollectionsRow` adds bounded unit-enum sets and enum-keyed maps. Native
literal-byte tests check order, malformed ordinals, duplicates, reversed entries,
transitive key metadata and shared node exhaustion, including borrowed inspection
before deletion. The focused `enum_collections` Wasm test checks persistence,
no-op/refusal invariance, whole-message rollback, upgrade, replacement and deletion.
Inspect this fixture with `db schema EnumCollectionsRow`.

## Platform adoption (0.19)

On 2026-11-16 the product adopted Platform Types 0.21.1; all seven private Wasm
hosts adopted Runtime/macros/guard 0.23.12. Each export declares its guard, codecs,
request bound and non-returning error policy. The private transport retains its
existing Wire frames and installs an explicit IC-trapping panic hook. Product
schema declarations, durable codecs and stored formats do not change.

The local driver uses Testkit's stop → upgrade → start operations, including
restart after a refused upgrade so the preserved predecessor remains inspectable.
It adds no retry, reset or migration policy. Storage 0.74.1 and frozen IcyDB retain
Runtime 0.22.5 for raw IC imports; Testkit retains its independent older native
Platform facade/types. Only the new Runtime guard owns the Wasm executor. The
separate Canic checkout and its pinned integration cohort are not updated here.

Focused checks passed with Rust 1.100.0: 23 native Account/scalar-key tests,
all-feature Wasm checks/Clippy for the seven hosts and transport, native driver
test compilation and selected Clippy, installed endpoint admission/reply rollback,
Account/Principal primary keys, instruction exhaustion, all 137 backing-write
trap positions, growth refusal and identified maintenance/cancellation/reopen.
Safe failure and subsequent progress are checked separately. These are correctness
checks; the comparison measurements below retain their original supplier cohorts.
The parked prefix-count patch also uses the new hooks; applying it to a disposable
source copy and checking its count-comparison Wasm host passes. Its historical
performance measurements were not rerun for this adoption.

The cross-runtime `platform_upgrade` test installs the predecessor built with
Runtime 0.22.5 and Types 0.20.4, writes identity records, then upgrades to the new
cohort. It checks identical backing bytes and responses, canonical default-account
uniqueness, rollback of a later write and a successful new write. It requires both
artifacts explicitly; rebuilding both with the current lock is not this test.
The local receipt in `target/evidence/platform-upgrade/` has SHA-256 identities:

| Input | SHA-256 |
| --- | --- |
| `previous.wasm` | `ea7b7ca8bbd169e49a45451ad242348fd8ce7afc6de27a0af4a18cb48ef2cae1` |
| `current.wasm` | `d5a0807855ae98cf04b00de881f37d0e9de0ada263a18e226b791ee0f601cc45` |
| Product lock | `362ec29fb46bf3a3a1dd01946b7a53d1acde5256bd8fbcde7687b6073740ebf9` |
| Qualification lock | `5c7ccec75858e63a7f3e0acd51fe4e321291d33e968c9088404929f366f20048` |

With those retained local artifacts and the prepared PocketIC server:

```sh
IC_DB_PREVIOUS_PLATFORM_WASM="$PWD/qualification/target/evidence/platform-upgrade/previous.wasm" \
IC_DB_M1_RECORDS_WASM="$PWD/qualification/target/evidence/platform-upgrade/current.wasm" \
cargo test --offline --locked --manifest-path qualification/Cargo.toml \
  -p ic-db-m0-rollback-driver --test platform_upgrade
```

## Comparison with frozen IcyDB

Reviewed 2026-11-16. This is the summary matrix for IC DB versus frozen
IcyDB `6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac` (package 0.310.2).
Keep measured comparisons, historical reference points and unmeasured gaps
separate. Scalar and first-page fluent measurements cover 0.18 work; grouped
and count measurements cover the 0.19 qualification increment on 2026-11-16. The fan-out
measurements remain the earlier recorded run. Both
measured engines have SQL disabled, and neither host includes Canic.

### Recorded matched measurements

Scalar rows use the 0.18 refresh, not the much smaller initial M1 artifact.
Its IC DB source receipt is `59ed763` plus the captured working diff;
source/lock/artifact identities and observations are retained in
`target/evidence/records-comparison/`. The previous scalar run is preserved in
`target/evidence/records-comparison-before-018-refresh/`. Fan-out rows use the separately recorded
0.7 workload and `target/evidence/envelope-comparison/` receipt (`b3f8ed0` plus
captured diff). These are different reachable workloads, not additive savings.
The optional fluent run has its own `target/evidence/fluent-comparison/` receipt
at `59ed763` plus captured changes; both engines execute their typed fluent APIs.

| Measure | IC DB | Legacy IcyDB | Observed difference / qualification |
| --- | ---: | ---: | --- |
| Scalar host, uncompressed Wasm | 233,238 B | 3,087,364 B | 92.4% smaller; fluent execution is unreachable in the IC DB scalar host |
| Same host, code section | 211,963 B | 2,978,374 B | 92.9% smaller; not whole-product size |
| Primary-key hit, instructions | 472,287 | 1,098,766 | 57.0% fewer |
| Primary-key miss, instructions | 477,808 | 1,072,929 | 55.5% fewer |
| Eight-row indexed listing, instructions | 771,729 | 3,084,039 | 75.0% fewer; IC DB direct prefix versus IcyDB fluent planning |
| Complete read/modify/replace, instructions | 829,457 | 7,023,846 | 88.2% fewer, including IcyDB's settling work |
| Same complete journey, cycles | 7,843,732 | 21,197,965 | 63.0% fewer; immediate-call reduction alone is 21.8% |
| Fluent-enabled host, uncompressed Wasm | 277,595 B | 3,093,437 B | 91.0% smaller; selected first-page queries reachable, not full capability parity |
| Fluent indexed category listing, instructions | 843,180 | 3,208,262 | 73.7% fewer; seven rows after the fixture's index-changing replacement |
| Fluent category plus residual filter, instructions | 834,529 | 2,980,596 | 72.0% fewer; three rows |
| Fluent indexed scan with one residual match, instructions | 1,787,308 | 5,078,704 | 64.8% fewer; 32-row dataset |
| Whole-entity count at 256 rows, instructions | 2,690,976 | 1,295,035 | IC DB scans; legacy maintained metadata is cheaper. [Receipt](#exact-count-scaling-019) |
| Category count at 512 rows (129 matches), instructions | 7,795,044 | 1,503,444 | IC DB explicitly traverses index prefix; legacy reads metadata |
| Whole-entity synchronous count at 512 rows | Work-limit refusal | Exact result: 512 | Different completion bounds; refusal is not a speed result |
| Maintained whole-entity cardinality at 512 rows, instructions (0.19) | 574,791 | 1,413,832 | 59.3% fewer; metadata contract, not a row-integrity scan. [Adoption](#maintained-count-adoption-019) |
| Four-group count, instructions | 926,034 | 3,657,854 | 74.7% fewer; required Nat64 category key |
| Four-group sum, instructions | 939,805 | 3,914,882 | 76.0% fewer; small Nat64 inputs, exact legacy Decimal result conversion |
| 32-row, eight-index insert, instructions | 76,139,333 | 461,923,130 | 83.5% fewer including settling; 58.7% fewer before settling |
| Same batch replacement, instructions | 149,570,696 | 1,190,386,058 | 87.4% fewer including settling; 71.7% fewer before settling |

The [adopted maintained count](#maintained-count-adoption-019) completes the
512-row whole-entity count in 574,791 instructions versus legacy's 1,413,832,
with 967 added Wasm bytes and small measured write overhead. The authorized hard
cut requires no old-format converter; incompatible data remains intact.
It does not improve prefix counts. The scan-count rows above retain the original
comparison, before the metadata terminal was introduced.

The [scalar](#scalar-comparison-refresh-018),
[fluent](#matched-fluent-first-pages-018),
[grouped](#matched-grouped-aggregates-019),
[count](#exact-count-scaling-019) and [fan-out](#matched-high-fan-out-writes) receipts own the exact conditions and
historical increments. Scalar data is one three-Nat64 entity, one secondary
index and 32 rows; the count probe grows that fixture to 512 rows. Fan-out data
uses four 48-byte Text fields and eight indexes;
the two hosts have different surrounding schemas, so their Wasm sizes are not
compared. Settling follows the harness's explicit per-write policy, not a measured
production scheduler. Smaller mechanisms and narrower reachable obligations both
contribute; this is not evidence of blanket feature parity or SQL-removal savings.

### Current coverage and comparison gaps

| Area | IC DB evidence | Legacy comparison | What remains |
| --- | --- | --- | --- |
| Added reachable entity size | About 4,300 raw / 4,241 code bytes per entity in the 0.18 fluent fixture | Historical 4,451 raw bytes after shrinking, different build/workload | No matched win established; 2 KB objective and roughly 4 KB working ceiling remain unmet. [Receipt](#expanded-sorting-costs-and-direction-admission-018) |
| Broad query matrix | 195 cases over balanced/skewed 32/128/511-row populations, before/after upgrade; 4,680 checked executions | 1,908 successful matched pairs; IC DB cheaper in 1,796, legacy cheaper in 112 | Populated prefix counts up to 14.4× and selective grouped queries up to 2.6× legacy instructions. Case frequencies are not a production workload. [Receipt](#broad-query-matrix-019) |
| Selective grouping and prefix-count candidates | Explicit-index control plus isolated stable-prefix experiment | Index choice helps sparse cases but hurts dense ones; candidate metadata count beats scanned IC DB and legacy metadata in the selected fixture | No automatic planner change or prefix-count product API adopted; write/storage/qualification costs remain explicit. [Paths](#aggregate-access-path-control-019), [candidate](#maintained-prefix-count-candidate-019) |
| Fluent query shapes and application mapping | Six first-page cases now matched over 32 rows; separate controlled shape/map growth builds | 64.8–73.7% fewer instructions in the selected successful queries | Pagination contracts differ; broader shapes, maps and entity growth remain unmatched. [Receipt](#matched-fluent-first-pages-018) |
| Complete sorting at 511 rows | Mixed-order limit 32: 13.86M instructions; two-value DISTINCT: 5.77M | Legacy public typed sort refuses materialization in the 32-row comparison; no matched successful 511-row run | Refusal is not a speed comparison. Rich-key/payload costs and a suitable legacy execution lane remain open. [Receipt](#bounded-sorting-execution-envelope-018) |
| Exact entity/prefix counts | Maintained whole-entity cardinality plus bounded row-validating counts; independent results and upgrade checks | IC DB metadata count uses 59.3% fewer instructions at 512 rows; legacy remains cheaper for populated prefix counts | Prefix counters, broader write envelopes and retained-count costs remain open. [Adoption](#maintained-count-adoption-019), [scan profile](#count-cost-decomposition-019) |
| Grouping, sums/averages/extrema and retained aggregation | 32 count/sum/min/max cases over 32 rows, plus existing retained-job witnesses | Four-group queries use 74.0–76.0% fewer instructions | Rich types, averages, retained jobs, state ceilings and contention costs remain unmatched. [Comparison](#matched-grouped-aggregates-019), [jobs](#product-retained-aggregates) |
| Schema migration and recovery | Copy, validation, cancellation, publication and cleanup witnesses | No matched migration-cost comparison | Same entity set/keys only; broader envelopes and allocation lifetime remain open. [Receipt](#identified-maintenance-and-the-local-console) |
| Mixed write/query histories | Independent map oracle checks 153 attempted batches, jobs, six upgrades and maintenance; selected full read/write costs | Retains legacy count and final-batch uniqueness invariants; no matched mixed-history run | Small scalar/reference fixture, not production contention or all type combinations. [Receipt](#mixed-history-qualification-019) |
| Canic endpoints and full applications | Canic-owned typed adapter: caller/frame refusal, sparse pages, upgrades, maintenance and reply rollback | No matched integration benchmark | Adapter is in Canic's working checkout; delivery and application CLI remain M4 work. [Receipt](#canic-typed-application-integration-019) |

### Product structure and functionality

| Area | Frozen IcyDB | IC DB now / deliberate boundary |
| --- | --- | --- |
| Product crates | Six database crates: facade, core, diagnostic-code, model, model-macros, schema; separate CLI | Two: `ic-db` and `ic-db-macros`; `db` currently lives in the independent qualification workspace |
| Public query frontends | Typed/structural and dynamic APIs; optional SQL | Fluent Rust only; runtime inputs construct fluent queries. SQL and separate dynamic frontend excluded |
| Useful query capabilities | Filtering, ordering, projection, grouping, DISTINCT and aggregates | These families retained and implemented within explicit bounds; broader combinations remain open, not automatic parity |
| Read admission | Public typed lane requires an admitted indexed route and restricts materialized sorts; trusted lanes are separate | Explicit work/state/output bounds admit selected complete scans and sorting; no implicit trusted fallback |
| Schema authoring | Attributes, shared model, host-side schema/actor generation | Attributes and two-stage path resolution retained; no author-supplied numeric field IDs; generated host registration stays out of Wasm |
| Ordinary writes | Journaled writes with follow-up work in the measured host | Synchronous bounded final-state batches, preparation before mutation, IC message rollback; no ordinary-write WAL |
| Durable schema authority | Accepted schema snapshots and runtime binding/admission | Compiled contract admitted against a minimal manifest; explicit versioned maintenance, no runtime DDL or online activation |
| Endpoints | Generated actor wiring and facade surfaces | Canic owns product endpoints/guards; standalone library use remains possible. Current CLI targets disposable local fixtures |
| Codec boundary | Owned codec/runtime plus a product `ic-wire` dependency | Database-owned durable codec; no product Wire dependency. Qualification transport may use Wire |

Structural findings are from the frozen
[workspace manifest](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/Cargo.toml),
[facade](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/crates/icydb/src/lib.rs)
and [product overview](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/README.md),
checked against the read-only checkout. IC DB's
[delivery table](../docs/design/ic-db-system-design.md#9-delivery-and-deletion-gates)
owns implemented scope. Fewer crates alone do not prove lower complexity;
endpoints move to Canic, while SQL removal and maintenance-only evolution reduce
scope. Shared execution and removal of ordinary-write journaling simplify retained
behavior under the selected contracts. Full replacement readiness remains unproven.

## Standalone endpoint hooks

The first M2 fluent-read probe is selected separately:

```sh
make m2-queries
make m2-jobs
make m2-fluent-comparison
make m2-aggregate-comparison
make m2-count-comparison
make m2-count-profile
```

`make m2-sort-costs` selects the sorting oracle with a measurement-only update
wrapper; see the [bounded sorting receipt](#bounded-sorting-execution-envelope-018).

It exercises exact scalar filters, primary/index-prefix traversal, reverse order,
empty sparse pages, byte-limit retry, oversized-row refusal, cursor identity and
unchanged-schema upgrade. Native tests additionally cover duplicate secondary keys,
live key updates, deleted cursor rows and corruption hidden behind rejecting filters.
The Wasm probe advances virtual time before upgrade because its preceding query
calls do not drain the IC install-code rate limit. No production setting is changed.
Bounded `filter_any` clauses add OR alternatives within the same eight-comparison
budget, with AND between clauses. Native boolean oracles cover overlapping matches,
absence, all-operand admission, safe ordering/index choice, DISTINCT and aggregate
composition. Retained counts reject changed boolean structure without altering
progress and resume after reopen. The `fluent_any` Wasm witness covers sparse and
byte-limited pages in both directions, an explicit category prefix, query binding
and unchanged backing across upgrades. Set `IC_DB_QUERY_PREDECESSOR_WASM` to an
older compatible fixture to check conjunction cursors from that executable;
without it, the test checks same-code upgrades. The 0.14 probe used the pre-OR
console artifact SHA-256
`6fa69b9b9cec75a36b7825d54754539a8686fa5d96bbe1e5d7621c63b69c62b2`,
retained at `target/evidence/0.14/query-conjunction-predecessor.wasm` from the
repository root. No stored format or conjunction binding changed.
Membership qualification checks exact optional equality, duplicate members,
empty/oversized lists, hidden invalid operands and cross-method cursor/job identity.
The Wasm witness pages duplicate-containing membership lists in both directions,
applies an explicit index prefix, refuses changed members, and resumes unchanged
cursors after upgrade without changing stable bytes.
Exclusion-list native qualification checks `filter_not_in` against an independent
optional-value oracle, both paging directions, invalid/empty/oversized lists and
the shared term budget. Cursors and retained counts resume through equivalent
ordinary inequality chains. This builder-only addition introduces no new Wasm
execution branch or checkpoint format.
Text-prefix qualification compares `Comparison::StartsWith` against Rust's
`str::starts_with`, including empty/absent text, embedded NUL, case differences
and canonically different Unicode strings. Native cases cover ordered pages,
DISTINCT, OR composition, completed text groups and retained input/group bindings
across reopen. The Wasm witness checks sparse pages, byte refusal, operand bounds,
cursor identity and upgrade with unchanged stable bytes. It qualifies residual
matching, not prefix-based index seeks.
Projection checks reuse named records, cover optional/nested output, exact
transitive type admission, unselected corruption and projected-byte retries. The
Wasm probe resumes a projected cursor after upgrade, rejects it for entity output,
and compares full decoding against ID-only projection over a 1024-byte blob row.
The local CLI adapter now uses these fluent operations for Entry, Document and DecimalRow;
`make m2-cli` separately checks its installed-layout command surface.

Automatic ordering adds full-sort native oracles with duplicate values, fixed
index components, both directions, rejected incomplete paths and cursor binding.
The Wasm witness orders by category plus the implicit primary key, narrows a
category equality to its index prefix and resumes that selected path after upgrade.
No input-prefix sort or statistics-based planner is qualified.

Ordered DISTINCT adds an independent tuple-set oracle, duplicate-only pages,
optional values, fixed prefixes, reverse traversal, byte retries and deleted
cursor rows. Filters on projected/fixed-prefix fields cover all six comparisons,
absence, contradictory prefixes and empty pages. Non-leading projections and
filters varying within a duplicate class refuse. The Wasm probe resumes filtered
and unfiltered DISTINCT cursors after upgrade and scans duplicate pages to
completion without changing backing bytes. There is no durable seen-set.

Exact synchronous counts and grouped counts reuse this probe. Native cases cover
independent tuple-count oracles, interleaved groups, absence, empty input, output
limits after full counting, decimal equality across grouping/filter/index access,
corruption and separate work/cardinality/key/state/output refusals. The Wasm probe
checks counts and limited grouped output before and after upgrade, refuses
incomplete work and undersized output, and verifies unchanged backing bytes.
Present-field and distinct tuple counts share this coverage: absence versus
present empty blobs/lists, zero-count groups, independent output limits, unknown
fields on empty input and noncontiguous duplicate tuples. Wasm checks optional
note and grouped field counts, then distinct category counts across reopen.
This is not qualification of resumable jobs, arithmetic reducers or all type pairs.

Scalar and grouped distinct counts now share a bounded comparison-key set. Native
cases cover absent/multi-field tuples, projection overlap, Decimal equality, 32
distinct values per group, duplicate acceptance at capacity and shared byte/key
refusal before output selection. Scalar counting retains no projected records.
The Wasm probe repeats grouped distinct counts after upgrade and checks that an
absent note contributes one distinct tuple. The CLI selectors are
`category,distinct-count:amount`, `category,distinct-count:category` and
`entry_id,distinct-count:note`; all execute through the fluent engine.

Synchronous minima/maxima use the same query probe: exact single-field projection,
absence versus zero, empty input, decimal ordering, owned text winners and final
output admission after scanning. Work/key exhaustion, malformed source rows and
unsupported shapes refuse. Wasm checks Entry amounts and Document absence/zero,
then repeats maximum after upgrade and verifies unchanged backing bytes.
Grouped extrema add independent per-key minima/maxima oracles, absent keys,
all-absent groups, overlapping projections, and growth/release of winning text.
They share the count grouping budget: result limits cannot bypass group or state
caps. Wasm repeats grouped maxima after upgrade and distinguishes absent grouped
notes from zero priorities; the CLI displays both through ordinary result mapping.
Completed-group selection adds input/group-filter composition, exact operands,
aggregate ties, absent-value ordering and output limits after sorting. Refused
modifiers on scalar/row terminals and work/state caps remain covered. Wasm
repeats selected group output across upgrade with unchanged backing bytes.
The installed CLI exposes inclusive `--group-min VALUE`/`--group-max VALUE`
bounds and `--group-order key|key-desc|asc|desc`;
these construct fluent operations, without client-side filtering or sorting.

Scalar integer sums add every supported integer width, explicit same-family
widening, signed cancellation at the i128 boundaries, final result overflow and
U256 accumulator overflow. Empty/all-absent input differs from present zero;
unsupported result shapes refuse before scanning. The Wasm fixture returns 213
for Entry amounts, repeats after upgrade, then checks a widened total beyond
u64 after explicit fixture writes. Queries leave backing unchanged. The CLI adds
`sum:amount`, `sum:priority` and `sum:score`, with 128-bit result records.
Grouped sums add `category,sum:amount` and `entry_id,sum:priority|sum:score`
selectors. Native coverage checks exact result-typed group predicates, absent/zero,
stable ties, signed cancellation, excluded-group overflow and finalization-state
refusal before output selection. Wasm repeats group totals/filtering after upgrade
and checks widened per-group totals beyond u64. The CLI filters/orders through
fluent calls and only formats returned `GroupValue` records.

Decimal sums reuse the exact sum reducer at the source's declared scale. Native
coverage adds exact cents and 28-place precision, optional/zero groups, rescaling
refusal, signed cancellation and fixed-scale overflow in excluded groups. The
separate Wasm Decimal probe writes known values through `decimal_batch`, checks
scalar/grouped output across upgrade, then checks cancellation and overflow with
unchanged stable bytes. CLI setup seeds three local DecimalRow samples through
that same private endpoint; ordinary canister initialization remains unchanged.
`sum:value`, `sum:maybe` and their `zone,` grouped forms use fluent queries.
This does not qualify float arithmetic or resumable jobs.

Integer/Decimal averages use the declared result scale and round once with ties
to even. Native witnesses cover every integer width, repeating fractions through
28 places, signed ties, absent/zero groups, filtering after rounding and overflow
after division. A U256 input outside i128 still produces a fitting Decimal average
when divided by its present count. Wasm repeats averages after upgrade, checks
positive/negative ties and a valid average with an overflowing sum, and compares
stable bytes around queries. CLI demos use `avg:amount`/`category,avg:amount` for
Entry and `avg:value`/`avg:maybe` with optional `zone,` grouping for DecimalRow.

The blob witness returns identical IDs with 1,037 encoded bytes for full entities
versus 8 for projected records. Its measured execution after opening is 74,795
versus 74,464 instructions. This is selective materialization, not partial storage
I/O. Both paths validate all input bytes. The complete fixture grows by 9,871 total
and 9,406 code bytes, including two new endpoints and their output codecs; this
does not isolate executor or per-output-type growth. Projection-increment fixture SHA-256:
`fcd716b40d9a7858d75bad7898476517addf419554399dc8aa08f22719a29225`.

The earlier scalar benchmark isolates shared-code overhead, with fluent execution
unreachable: 224,548 total/203,915 code bytes, SHA-256
`091b9f0f02c298036443c8b383b8e64fe83342de2bf719187296fb48506c4c54`.
Projection support adds 14 total/code bytes; hit/miss use 471,728/477,249 instructions
(-1 each), direct listing 771,661 (-1), and read/modify/replace 828,622 (+25,
about 0.003%) with 7,842,397 cycles (+25).
The shared borrowed reader fills its bounded field view in place; a temporary
copy found during measurement was removed. These costs remain below the matched
IcyDB ceilings. The [0.18 refresh](#scalar-comparison-refresh-018) records the
current scalar measurements. No direct-to-fluent performance parity is claimed.

All six canister hosts use Platform's generic `guard`, `decode_with` and
`encode_with` hooks with the [fixed fixture codecs](ic-db-qualification-transport/src/lib.rs), shared
through one private qualification crate with ordinary Cargo dependencies. No Canic or
Wire Endpoint dependency is needed. The [standalone example](ic-db-m0-rollback-canister/src/entrypoints.rs)
is executable qualification, not a new product API or endpoint framework.
Platform owns execution/reply; Wire owns native frames. Existing method names,
IDs, payloads and directional limits remain unchanged: each direction allows
65,536 encoded bytes, 8 MiB allocation, 65,536 nodes and depth 32. Admission checks
size and the fixed header before Platform copies the complete request. Typed
decoding still validates the full payload, including framed unit requests.

```sh
make m0-endpoints
```

The installed probe checks guard refusal, malformed/truncated/trailing frames,
wrong identities/directions and oversized ingress before writes. A reply-encoding
trap reverts synchronous rows, indexes and allocator changes. After an outgoing
call, it reverts callback writes while preserving the first execution's writes.
Valid calls return one correctly framed result. These controls remain separate
from the two matched benchmark hosts. Both benchmark hosts use the same hooks;
transport-cost changes are harness changes, not database improvements.
The migration for [IC DB issue #1](https://github.com/dragginzgame/ic-db/issues/1)
was delivered in [d6991db](https://github.com/dragginzgame/ic-db/commit/d6991db7f2a72532acb2d6aa9a491b8871432051).
The 2026-11-16 acceptance review matched all 82 pre-migration endpoint names,
kinds and protocol IDs across the six hosts to their current Platform hooks.
No Endpoint dependency remains in the selected graph; both benchmark hosts
share the same transport owner. The focused endpoint probe passed both request
admission and synchronous/callback reply-rollback tests again. Existing 0.17
release evidence covers the installed rollback, aggregation, capacity, records
and maintenance paths; this review does not label those historical runs as new.
Later intentional fixture payload changes belong to their own feature increments,
not the Endpoint migration. The frozen IcyDB engine and Canic sources are unchanged.

## M1 persistent records

```sh
make m0-setup
make m1-records
make db
db schema
db query --limit 3 --min 40
db query Entry --min 40 --limit 3 --select id,amount
```

The explicit ULID issuance witness runs with `make m1-records-wasm`. It passes
operation time and deterministic test randomness to the real `UlidGenerator`,
inserts generated primary keys, and checks same/backward-time ordering, unchanged
invalid-time refusal, consumed IDs after normal application/database refusals,
complete rollback after a late trap, and explicit checkpoint restoration after
upgrade. The fixture's heap state intentionally disappears on upgrade; this does
not qualify automatic durable issuer recovery or cross-canister uniqueness.
No schema default or managed timestamp attribute is implemented by this witness.

The separate literal-default witness in `make m1-records-wasm` uses `DefaultRow`
and the real typed insert builder. It checks omission versus explicit zero/false/
`None`, unique default collisions, missing/oversized inputs, full backing rollback
after refusal or a late trap, replacement/no-op revisions and unchanged upgrade.
Native cases cover every integer width, canonical default identity and refusing a
changed policy without touching storage. The schema fixture has 30 entities and
eight named types; its expected hash includes independently framed default and
managed-policy bytes. `db schema` displays those policies in **Info**.

The managed-time witness uses `ManagedRow` and `Batch::at` with deterministic
host-supplied times. It checks one batch time across rows, preserved creation,
changed update indexes, byte-identical no-ops at earlier/equal/later times,
ownership/regression/uniqueness refusals, late-trap rollback and unchanged upgrade.
Native cases also cover independently selected roles, full-batch refusal,
corrupt stored timestamp order, exact policy admission and incompatible reopening.
No new clock, transport policy, persistent counter or historical reader is added.

The Principal-key witness uses `PrincipalKey`: management/short/29-byte identities,
length-first pages, exact lookup, cycles including `Some(empty Principal)`, unique
rank exchanges, restricted deletion, no-ops, late traps and upgrade preservation.
Native scalar-key tests add literal row/index frames, malformed lengths and wrong
owners; previously supported integer/ULID schemas retain their fingerprints.

The Account/Subaccount-key witness covers typed pages and direct lookup, mixed
reference cycles, canonical default-account aliases, duplicate targets, restricted
deletion, unique swaps, no-ops, rollback and upgrade preservation. Native scalar-key
tests independently frame owner/subaccount bytes and reject truncated, oversized,
trailing and wrong index owners. No transport bytes are used as durable keys.

The first scalar slice uses the real `ic-db` library and `#[entity]` generator.
Its [shared declarations](ic-db-m1-records-schema/src/lib.rs) are a private qualification
crate used by both the [host build script](ic-db-m1-records-canister/build.rs) and Wasm
target through ordinary Cargo dependencies. Host-only `ctor`
registration collects full Rust paths; the second stage validates and seals the
selected module's schema, then emits its typed assembly and expected fingerprint.
Document's imported Entry type and OptionalLink's Self target exercise compiler
resolution. Logical names now define stored identity: optional entity/index
`name` overrides replace numeric IDs, and references infer a sole reverse index
or select one by name. Contract grammar 2 refuses old numbered manifests intact.
Native tests include known physical-handle collisions and unchanged backing on
admission refusal; full names remain authoritative. `make m1-records` additionally
selects real constructor collection and deterministic/duplicate/late/missing-target
host checks. The existing golden fingerprint and Wasm checks qualify the resulting
runtime; ctor is absent from the Wasm dependency graph even with `ic-db/build` enabled.
Native checks cover canonical codecs/schema identity, cross-entity batches,
final-state uniqueness (including swaps), unchanged refusals, indexes and reopen.
Literal keys and payload bytes supply independent representation checks.
`SignedKey` and `WideKey` qualify Int128/U256 primary keys and mutual references
in Wasm: signed minima, the U256 high half, paging through MAX, index owners,
uniqueness exchanges, no-ops, rollback and unchanged upgrade. Native cases cover
every integer width with independent physical-key vectors and malformed owners;
reference type mismatches refuse during schema admission.
`KeyRow` and `Numbered` qualify supplied ULID/Nat64 primary keys, typed paging,
full-width index owners, final-state mixed-key reference cycles and restricted
deletion. Native corruption checks use literal physical row/index keys; Wasm
checks include no-ops, refusals, unique exchanges, rollback and unchanged upgrade.
`DecimalRow` qualifies declared scale 2, exact 17-byte rows, numeric key ordering,
equivalent-value uniqueness/no-ops, coefficient limits, malformed stored scales,
rollback and upgrade. Native checks additionally compare cross-scale ordering
against independent I256 arithmetic and exercise scalar collection corruption.
`TimeRow` qualifies signed Unix-millisecond timestamps and unsigned durations,
with optional unique ownership and ordered secondary indexes. Retained IcyDB
regressions cover pre-epoch flooring, RFC3339 offsets/fractions, text overflow
and arithmetic across the full unsigned duration domain. Independent native/Wasm
bytes cover minima/maxima, absence, collection keys, corrupt framing, no-ops,
unique exchanges, rollback and unchanged upgrades. Both time types share the
ordinary codec and commit path; no clock or managed timestamp is involved.
`DateRow` qualifies bounded Gregorian dates as signed epoch days, including
pre-epoch order, optional uniqueness exchanges and invalid-domain refusal.
An independent native month-by-month counter checks every day in years 0000–9999
against the adapted calendar algorithm. Native/Wasm witnesses also cover exact
bytes, malformed rows/indexes, collections, rollback and unchanged upgrade.
`UlidRow` qualifies exact 16-byte ULID values across the 80-bit suffix boundary,
the top bit and the full maximum. Native checks cover independent row/schema
bytes, canonical uppercase text, collection keys and the 4 KiB row limit. Wasm
checks cover parsing, byte order, optional uniqueness exchanges, no-ops/refusals,
late traps and unchanged upgrades. The built fixture imports only `ic0`.
The feature-disabled `ulid` dependency still declares `web-time` on Wasm, but its
browser code is not reachable here. No ULID generation or entropy path is enabled.
`AccountRow` qualifies Account/Subaccount storage and secondary indexes. Native
checks cover all owner lengths 0–29, independent bytes/order, default-subaccount
identity in sets/maps, duplicate stored keys and the row byte limit. Wasm checks
cover optional uniqueness, zero no-ops, final-state exchanges, malformed input,
late traps and unchanged upgrade, including complete backing comparisons.
`FloatRow` qualifies finite Float32/Float64 extremes, normals/subnormals, zero
normalization and optional unique exchanges. Native tests compare key order to
primitive numeric comparisons, including deterministic raw-bit samples, and
check collection-key equality and malformed unindexed data. Wasm tests check
exact bytes, refusal before mutation, no-op zero rewrites, late traps and upgrade.

The `BigNumbers` fixture adds full-domain U256 values, optional unique ownership
and compound index ordering. Its independent byte vectors cross the 128-bit
boundary and highest bit; native collection tests also exercise U256 keys and
row-byte limits. Wasm checks cover final-state swaps, refused/no-op writes, late
traps and unchanged upgrade, comparing complete backing as well as rows.
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
reopen checks preserve the exact values. Signed secondary indexes are qualified
below; the later primary-key witness qualifies signed primary keys.
The separate WideNumbers fixture checks required/optional 128-bit integers,
including signed extrema, unsigned maximum and values just beyond 64 bits.
Independent 16-byte scalar vectors and schema hashes qualify encoding;
truncation, surplus bytes and incompatible width/signedness refuse. Native and Wasm
checks preserve all bits across writes, no-ops, traps and reopen. Wide secondary
indexes and later primary keys are qualified below. This codec probe does not
qualify arithmetic; the later scalar-sum coverage above owns that evidence.
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
upgrades. Required Bool/Text indexes are also qualified: complete leading-field
matching, empty/NUL/Unicode order, tuple boundaries, uniqueness, byte bounds,
replacement and deletion. Native checks enforce the 512-byte complete-key cap,
including escaping and primary-key suffixes, while retaining 4 KiB row bounds.
Wasm checks Document's `(published, title)` index through
updates, bounds refusals, late traps and unchanged upgrades. Original admitted
key bytes remain unchanged; semantics version 2 changes fingerprints and refuses
prior wider-key contracts intact. A supplier-level read witness injects a 513-byte
raw key and proves refusal before its payload is read, without changing backing.

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
Required and optional Nat64 references use a declared ordinary index beginning with the
reference field. Native checks cover missing targets, final-state insertion,
repointing/deletion, cycles, self references and a 31-dependent removal batch.
Wasm checks verify the same core refusal/composition boundary and reopen. No
recursive cascade or nested reference support is implied. OptionalLink exercises
absence without a target, present zero/max targets, self-reference, final-state
unlinking/deletion, unchanged refused writes, no-op revisions, late-trap rollback
and unchanged upgrade. Native cases additionally cover independent references,
31-dependant unlinking, literal absent/present reverse keys, incompatible optionality
and malformed rows/index owners. The console includes its empty schema for inspection.
Named types add EnumRow and NestedRow fixtures. The latter combines bounded Text,
Nat16, optional records, unit/payload variants and boxed recursion. The host/target
schema pipeline discovers each reachable definition once. Independent byte/hash
oracles qualify canonical order and the named-type suffix. Native checks cover
transitive type changes, reordered fields, required cycles without a finite base,
corrupt unindexed payloads on delete/replace, exact depth and 256-node boundaries
(including sibling fields), and the complete 4 KiB row envelope. Wasm checks cover
malformed payloads, oversized values, depth/node refusals, no-op revisions, late
traps and upgrade/reopen. Earlier unit-only manifests refuse without mutation.
Inspection shows bounded named-definition tables without recursively expanding
links. ListRow adds optional text lists, ordered Nat16 lists and recursive
ListNode children. Independent count framing and schema hashes cover list bounds.
Native checks distinguish absence/empty, preserve order/duplicates, reject bad
UTF-8/counts/truncation, and enforce sibling item totals, empty-record node counts,
depth and the full 4 KiB row envelope. Wasm checks include malformed input,
preparation refusals, no-op writes, late traps and exact upgrade preservation.
Counts are admitted before allocation or traversal; all lists in a row share a
256-item budget alongside the 32-depth/256-structured-value bounds. SetRow adds a
named Int16 Scores set. Native cases exercise every admitted scalar family and
borrowed stored inspection, numeric order versus row-byte order, construction
order independence, absence/empty, mixed list/set totals, whole-row bytes,
duplicates/descending entries, incompatible reopen and unchanged failed writes.
The Wasm set case verifies literal bytes, malformed input, no-op revision/backing,
preparation refusal, a late trap and unchanged upgrade. Inspection names the set
and its element/count bounds. Collection indexes, nested references/indexes,
non-scalar set elements remain unimplemented.
MapRow adds an Int16-keyed Rates map with optional bounded text values. Native
checks distinguish missing/absent/empty values, prove canonical key order and
refuse duplicate keys with different payloads before overwrite. Recursive maps,
lists inside map values, zero counts and the 4 KiB row boundary exercise shared
budgets. Corrupt unindexed map contents refuse reads, replacement and deletion
without changing backing. Wasm checks exact bytes, malformed entries, no-op
revisions, preparation refusals, late traps and unchanged upgrade. MapTree's
independent bytes exercise the exact 32-depth/256-node limits in Wasm. Inspection
shows key/value roles, optionality and map bounds. Non-scalar keys, map member
indexes and nested references remain unimplemented.
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
The current 30-entity fixture with codec v3, retained host setup and Storage 0.74.1
covers 137 replacement writes and reaches growth with the insert batch starting
at ID 1700. That batch performs 1,326 backing writes, one growth request and
expands from 26 to 34 physical pages. These are
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
Remaining scope includes other identity/composite primary keys, nested references
and broad fluent queries. Supplied integer/ULID/Principal/Account/Subaccount primary keys and exact reference
types use the shared paths qualified above. Secondary-index capabilities follow
the current product type contract.
Ordinary indexes retain absent values; optional unique indexes require explicit
`present_only` membership for every optional component. Native checks cover
compound guards, reverse-reference admission, omitted-entry row validation and
corrupt presence; Wasm checks absent/present order, unique swaps, transfer,
late-message rollback and unchanged upgrade. `db schema` projects membership
alongside the compiled entity descriptions, including the
Document-to-Entry reference, its `(entry_id, score)` reverse index and IdentityRow's
unique owner and present-only optional identity indexes, plus Document's
`(published, title)` and optional priority indexes;
the current `query` command reads Entry only. The private Wasm controls test
Document updates and unchanged-upgrade persistence.

The current M1 selection uses Storage 0.74.1, Runtime/macros/guard 0.22.5 and
Types 0.20.4, Wire 0.22.2 and Testkit 0.106.0/PocketIC 16.1.0. Testkit's
transitive Platform resolves to 0.33.5.
Prior recorded M0/baseline results used the versions stated with that evidence;
the updated lockfile does not retroactively requalify those results.

## Maximum write envelope

```sh
make m1-write-envelope
```

`EnvelopeRow` exercises 32 rows of exactly 4096 encoded bytes, with eight
four-field indexes each. Its zero-heavy text components form complete 512-byte
keys; an unindexed 3821-byte Blob fills each row to the maximum. The 128 KiB row
batch creates 256 entries containing 128 KiB of index-key bytes. Replacement
changes the leading text value and produces 511-byte keys. A separate four-field
513-byte key case refuses before mutation even though its row fits easily.
The driver independently assembles all row/index bytes, then reads both complete
namespaces through bounded private audit calls. Two maximal rows fit each reply;
continuation uses the last returned key without duplicate metadata.

The Wasm journey covers oversized row/batch/key refusals, insertion, all eight
indexed reads, byte-identical no-ops, duplicate refusal, late-trap rollback,
replacement with changed keys, unchanged upgrade and complete deletion. It records
IC instructions over reopen/construction/preparation/commit and whole-update cycles.
This is a selected shape on a small database, not proof of every type/constraint
combination, tree size, allocation history or peak heap envelope. IC Storage owns
allocation behavior; repeat the evidence after its planned overhaul.

The earlier semantics-v2 key-cap run, before host reuse and operation-specific
revision admission, recorded:

| Operation | Instructions | Whole-update cycles |
| --- | ---: | ---: |
| Insert 32 maximum rows | 89,485,042 | 96,494,127 |
| Replace all 32 and their index keys | 175,000,090 | 182,009,175 |
| No-op replacement | 23,204,470 | 30,213,555 |
| Delete all 32 | 92,956,462 | 99,967,599 |
| Refuse a 33-row batch | 8,817,197 | 15,826,455 |
| Refuse a 4097-byte row | 1,862,320 | 8,871,463 |

Indexed reads use 19,553,560–19,769,806 instructions. Physical stable allocation
is 2,228,224 bytes after insertion and deletion; logical cleanup does not shrink it.
Refusals, no-ops, traps and upgrade compare complete backing, including supplier
metadata. Artifact SHA-256:
`fb57769bb7a0b813c432989a29cd16873a344850a7a8030ca08781f05544a9b8`.
Those logs and independent identity evidence remain under `target/evidence/key-bounds/`.

The earlier wider-key run is historical evidence under
`target/evidence/write-envelope/`; it used 8169-byte keys, approximately 950M
insert and 1.96B replacement instructions, and 3,801,088 physical bytes. The new
fixture moves most payload out of the indexes while retaining full-size rows.
Those costs are different workloads, not a like-for-like optimization claim.
The former large-key transport-failure regression was removed when that reply
shape ceased to be part of the supported key contract.

## Matched high-fan-out writes

The optional `make m1-envelope-comparison` selects the baseline's `envelope`
fixture feature. Ordinary scalar builds do not include that schema or its controls.
It runs 32-row inserts and replacements with the same eight four-field indexes
on both engines, using four 48-byte zero-filled Text values and empty unindexed
padding per row. Every row is checked against independent logical values after
each write; queries check
current and obsolete leading-field membership. IcyDB's explicit journal settling
is reported separately and included in total instructions. This is the same
logical workload, not identical codecs, storage layout or generated host schema:
IC DB uses its 30-entity qualification host with other fixture rows already present;
IcyDB uses Entry plus EnvelopeRow, initially empty.
It is not a Wasm-size or planner comparison.

The original 1018-byte values cannot be a matched successful-write benchmark:
frozen IcyDB caps the **whole index key at 512 bytes** (individual components at
487). The earlier IC DB stress fixture created 8169-byte keys. IC DB now adopts
the same complete-key cap. The comparison verifies IcyDB's refusal with unchanged
stable bytes before running the smaller
common workload. It does not relax either engine's limits. Reproduce with:

```sh
make m1-envelope-comparison
```

Source/lock hashes, compiler, features, Wasm artifacts and raw measurements are
recorded under `target/evidence/envelope-comparison/`. The initial oversized-key
attempt is retained there separately. The frozen IcyDB source is unchanged.

The earlier 0.7 comparison, before host reuse and operation-specific revision
admission, passed under the recorded locked suppliers and release profile.
Instructions, excluding transport framing, were:

| Operation | IC DB operation/total | IcyDB operation | IcyDB settling | IcyDB total | IC DB reduction vs total |
| --- | ---: | ---: | ---: | ---: | ---: |
| Insert all 32 | 76,139,333 | 184,260,093 | 277,663,037 | 461,923,130 | 83.5% |
| Replace all 32 and their index keys | 149,570,696 | 528,829,971 | 661,556,087 | 1,190,386,058 | 87.4% |

IC DB has no settling messages. Against just IcyDB's immediate operation, the
reductions are 58.7% and 71.7%. IcyDB needs one settling call after each measured
write under this harness policy; that policy is not a throughput benchmark or a
claim about Canic scheduling. Whole-update cycles are 83,167,906 / 156,599,238
for IC DB and 476,111,521 / 1,204,574,566 including settling for IcyDB.
Initial startup and the oversized-key refusal are reported separately, outside
the successful-write totals. Both engines' final row values and relevant index
membership checks pass. No SQL is enabled on either side. These are workload
observations, not an attribution of every saved instruction to the missing WAL.

Artifact SHA-256:

- IC DB: `fb57769bb7a0b813c432989a29cd16873a344850a7a8030ca08781f05544a9b8`.
- IcyDB: `8addf7680016ce09bb0fc3c0c2667010045a6071b4752aa1937e3f288b2de9c6`.

## Controlled entity growth

```sh
make m1-entity-growth
```

One private qualification host builds with 1, 8 or 32 distinct entities, each
with three Nat64 fields and the same `(category, id)` index. A single update
endpoint selects the entity and operation from runtime input. Every entity's
typed insert/replace/delete, lookup, primary page, index-prefix read and tuple
mapping stays reachable. The driver independently checks the same primary key
with different values in all entities, old-index removal, atomic revisions,
unchanged upgrade and complete deletion. Each shape has a fresh-host build and
a `retained-host` build. The latter retains the supplier runtime and immutable
Schema only; each call clones the schema and opens a new database/lease, checking
its current manifest. Explicit revision reads and batch preparation validate the
needed counters. The product exposes ordinary
Schema cloning, not a cache registry or a second admission path.
This isolates equivalent-entity growth;
it does not vary endpoint counts, field shapes or query syntax.

Host Artifacts inspects uncompressed Wasm/code-section bytes and verifies the
fixed export set. Four IC counter intervals record host bootstrap, acquiring the
allocation lease, schema construction/clone and database open. A fifth records
the selected operation plus its lookup/page/index/revision observations. All
five belong in the complete call; the fifth is not a standalone mutation cost. Update-cycle deltas include
transport and system charges. Read-only observations also use the one update
endpoint, so those cycle values are not query-cycle estimates. Every sample is
retained under `target/evidence/entity-growth/`, alongside artifacts, source/lock
hashes, feature trees and compiler/build settings. The release profile matches
the scalar comparison; there is no post-link optimization. Installation cycles
include initial setup. After unchanged upgrade, the first call rebuilds cold
state, commits a replacement and traps; complete stable bytes must roll back.
The next call must return the old row/index/revision, then deletion proves the
lease/runtime remain usable. Cold and warm instruction samples stay separate.

This follows IcyDB's
[controlled composition investigation](https://github.com/dragginzgame/icydb/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/docs/design/0.260-runtime-footprint/w1-reconciliation.md):
distinguish the first reachable engine path from the incremental cost of more
entities, and do not infer heterogeneous-application growth from matching simple
records. Its older toolchain/Canic workload is a methodological reference, not
a subtraction baseline. No IcyDB or Canic source is modified or copied.

Current 0.7 results (Rust 1.100.0, Storage 0.74.1, Runtime 0.22.5,
Testkit 0.106.0/PocketIC 16.1.0; native driver Platform 0.33.6):

| Host | Entities | Wasm bytes | Code-section bytes | Opening instructions | Complete measured instructions |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fresh | 1 | 220,953 | 200,243 | 433,718 | 516,225 |
| Retained, warm | 1 | 222,375 | 201,491 | 98,590 | 181,208 |
| Fresh | 8 | 261,066 | 239,900 | 555,709 | 671,197–687,321 |
| Retained, warm | 8 | 262,495 | 241,155 | 106,972 | 222,606–238,781 |
| Fresh | 32 | 398,506 | 376,151 | 959,989 | 1,090,306–1,134,854 |
| Retained, warm | 32 | 399,925 | 377,396 | 112,606–112,661 | 243,256–287,842 |

Instruction ranges are the per-entity read observations after every row has been
replaced. Each includes a lookup, primary page, index read, revision read and tuple
mapping. There is one row per entity; the total dataset grows with entity count.
At 32 entities, whole-update cycles fall from 8,104,083–8,148,631 to
7,252,307–7,296,893: about 10%, versus roughly 75–78% fewer measured instructions.
These are not standalone lookup or query costs. Query-message heap retention is
not qualified by this update-only fixture; do not apply its warm costs to
query-only traffic. The first successful call following the upgrade/trap sequence
costs 1,125,788 measured instructions in the retained 32-entity build;
later calls reuse the freshly prepared host state. Initialization is not free.
The preceding trapped execution is checked for rollback but its cost is not
included in that successful-call figure or the warm table.

From 1 to 32 fresh entities, growth is 177,553 total bytes and 175,908 code bytes:
approximately 5.7 KB per additional equivalent entity. This includes generated
codecs/metadata, typed generic paths, mapping closures and fixture dispatch.
It does not identify those contributions individually or prove all generic
execution is shared. Identical layouts also permit compiler merging; richer
schema/query/endpoint growth remains unmeasured.

The fresh 32-entity opening breakdown is:

| Stage | Instructions |
| --- | ---: |
| Storage runtime construction/bootstrap | 304,904 |
| Acquire allocation lease | 5,340 |
| Validate/hash executable schema | 543,034 |
| Load collection and validate manifest | 106,711 |

This carries forward IcyDB's
[separation of cached runtime facts and current durable authority](https://github.com/dragginzgame/icydb/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/crates/icydb-core/src/db/session/accepted_schema/mod.rs).
Its accepted-root publication/invalidation tests establish the stale-authority
failure to preserve; IC DB does not copy the registry cache, historical schema
reconciliation or runtime-DDL invalidation. Only immutable executable metadata
and supplier host custody survive calls here; no database handle, lease,
manifest, incarnation or revision is cached. The native `schema_reuse` test
checks independent allocations and changed manifest identity under a reused nested
schema. Missing/malformed revisions refuse explicit revision reads and affected
writes, including no-op replacement; exhausted valid counters permit no-ops but
refuse real changes. A mixed batch with a bad counter leaves every row/index and
revision untouched. Other entities' valid writes and ordinary row reads proceed.

The accepted contract now defers revision validation from opening to operations
using that counter. One bounded preparation map holds each touched entity's current
revision and real-change flag, replacing the previous changed-entity set; nothing
survives the batch. This deliberately changes when corrupt metadata is discovered,
not its encoding or repair policy. Row reads need no revision value. Compared with
the preceding all-revision opening run, warm 32-entity opening drops from about
755k to 113k instructions. Relative to the original fresh host's 1.61m, the two
changes remove about 93% of opening work. Residual variation includes storage-tree
size and allocation layout; no constant-cost claim or supplier tuning is made.

Generated schema assembly now reuses immutable metadata per thread, and the local
M1 host retains its supplier runtime. The normal record witness checks cold
queries after upgrade followed by an update and fresh query. The separate growth
fixture still measures update-call costs; query performance and Canic integration
remain unmeasured. The scalar comparison deliberately retains its fresh-host recipe
to isolate the product change: before unit-enum indexes it measured 222,187 total/201,524 code bytes,
with hit/miss/listing 471,698/477,219/771,669 instructions and read/modify/replace
828,387 instructions, 7,842,154 cycles. Its SHA-256 is
`3852bfcdda0d40dea989ce2caa003b35f8c55d5ef34a7727bf1ff09cc32c6186`.
The product change adds a net 2,578 total/2,553 code bytes to that scalar
artifact; every sampled query/write uses fewer instructions. Keeping host setup
adds roughly 1.4 KB at each entity count in the matched growth pair. No schema
attribute, durable representation or product dependency changes.

After unit-enum index support, the same scalar host measures 223,327 total/202,735
code bytes; hit/miss/listing cost 471,717/477,238/772,008 instructions and
read/modify/replace costs 828,471 instructions, 7,842,238 cycles. SHA-256:
`9462837ca78620396b7dffe6c6a34505d4b7e13599b50e98a35b1ee60e9c281b`.
This adds 1,140 total/1,211 code bytes and 19/19/339 query instructions, with 84
additional instructions on the complete replacement journey. The design records
the explicit tradeoff for enum capability. Frozen IcyDB is unchanged. The growth
results below precede enum indexes; they have not been remeasured for this addition.

Enum-keyed collections subsequently add 703 total/701 code bytes: the current
scalar host is 224,030 total/203,436 code bytes, SHA-256
`c8986fdd28897b4390885e596d3909f9efd1e802d87bc26e3bb62fa3d0886318`.
Every sampled scalar read/write instruction count and execution-cycle measurement
is unchanged; frozen IcyDB is also unchanged. This is shared-code overhead only,
not collection-heavy performance evidence.

Artifact SHA-256 values:

- Fresh 1: `7bf7e9e157227e651f0d71675adf5beae2ba90bdfd5afedd6056403ac0c67f57`
- Fresh 8: `1f417f6414152440c74b0adc71d2da147664855684701499ff4aaadcd16c8eb8`
- Fresh 32: `5cc9af5983a68521a6d93517490cc2a4c4ae0774e0a64f9c758a752ef88eafb8`
- Retained 1: `8356b4d6c7f4c5c2f61b1054f7c8124aad5011535982b47a2b56c33d56e588ab`
- Retained 8: `57ab693128bfc06c1070f2249ff497349afc9670f3205879424fbf2fce26c664`
- Retained 32: `2593a4cb9b841396579e15c505db576e2d0ca8c7a61185ca62126938385f70da`

The original 218,020/257,937/394,627-byte fresh-host artifacts and source evidence
remain in `target/evidence/entity-growth-before-opening-profile/`; the subsequent
four-stage breakdown is in `target/evidence/entity-growth-before-host-reuse/`.
The all-revision admission comparison is in
`target/evidence/entity-growth-before-demand-revisions/`, with the preceding
scalar comparison in `target/evidence/records-comparison-before-demand-revisions/`.
The added counter/reply fields and trap branch affect current fixture Wasm;
use the current matched fresh/retained pairs to attribute reuse cost.

## Controlled fluent composition

```sh
make m2-query-growth
```

The existing entity-growth canister and measurement runner build six subjects
with one update endpoint, the same tuple transport, retained host/immutable schema,
scalar codec, index and direct read/write operations. Only selected fluent call
sites, post-fetch application maps and entity declarations vary:

| Subject | Entities | Fluent shapes | Extra mapping branches |
| --- | ---: | ---: | ---: |
| `direct` | 1 | 0 | 0 |
| `direct-entities` | 8 | 0 | 0 |
| `one` | 1 | 1 | 0 |
| `shapes` | 1 | 8 | 0 |
| `maps` | 1 | 8 | 8 |
| `entities` | 8 | 8 per entity | 0 |

The eight distinct `fetch()` call sites exercise comparisons, OR, descending
primary traversal, complete amount sorting, an equality-prefix index, primary
ranges and exclusions. Ingress arguments keep their inputs runtime-selected.
Maps run over fetched entities and change the returned tuple, making their
reachability observable. The mapping delta includes application code and routing;
it is not an isolated measurement of closure monomorphisation.

The driver seeds eight rows per entity with duplicate amounts and independently
checks every selected query/map against ordinary Rust tuples. It verifies the
fixed export set, entity isolation, repeated warm calls, cold opening after a real
upgrade and empty matches. Each call reports separate bootstrap, lease, schema
and open instruction intervals, plus operation instructions. Operation cost
includes result mapping and the fixture's revision/schema observation; external
cycle deltas also include transport and execution overhead. No native or elapsed
time is used as performance evidence.

Artifacts, build feature trees, compiler/profile inputs, dirty-source diff and
input hashes, artifact hashes, raw/code-section byte counts and execution results
are retained under `qualification/target/evidence/query-growth/`. Builds use the
qualification release profile without post-link shrinking. This is a scalar
row-query composition witness, not a full application, aggregate/projection
growth study, Canic endpoint comparison or matched IcyDB benchmark. The frozen
IcyDB composition investigation supplies the method of separating first-use cost
from incremental reachability; no engine code is copied for this experiment.

Initial measurement, before the shared-boundary reduction below, on 2026-11-16
with Rust 1.100.0, `opt-level=z`, fat LTO, one codegen unit,
panic abort and stripped symbols, using the recorded qualification lock:

| Subject | Total Wasm bytes | Code-section bytes | Compared with | Total delta | Code delta |
| --- | ---: | ---: | --- | ---: | ---: |
| `direct` | 231,075 | 209,645 | — | — | — |
| `direct-entities` | 267,211 | 245,320 | `direct` | +36,136 | +35,675 |
| `one` | 268,683 | 245,111 | `direct` | +37,608 | +35,466 |
| `shapes` | 269,880 | 246,279 | `one` | +1,197 | +1,168 |
| `maps` | 271,673 | 248,072 | `shapes` | +1,793 | +1,793 |
| `entities` | 324,266 | 300,266 | `shapes` | +54,386 | +53,987 |

The same first query uses 124,071 operation instructions in `one`, 124,610 in
`shapes` and 124,620 in `maps`; corresponding complete-call cycle deltas are
7,233,088 / 7,233,627 / 7,233,637. Warm opening is 104,534 instructions in all
three. Cold post-upgrade opening is 441,181 instructions and must not be hidden
in the warm samples. The eight-entity subject has 64 total rows rather than
eight: its 156,623 first-query instructions include a larger physical collection
and metadata, so that increase is not attributed solely to generic code.

The small additional shape/map deltas support the current shared-execution
boundary for those inputs, but do not establish acceptable entity growth.
The same-build direct control adds 36,136 bytes across seven entities: about
5,162 bytes each before fluent queries. With the eight fluent shapes, growth is
54,386 bytes, about 7,769 each. The difference of slopes is 18,250 bytes, or about
2,607 additional bytes per entity associated with fluent reachability. These
totals include compiler layout, codecs, metadata, typed boundaries and fixture
dispatch; they are not isolated removable ownership totals.

Maintainer feedback retains roughly 2 KB as the per-entity objective and roughly
4 KB as the working comparison ceiling. IcyDB's frozen
[0.251 investigation](https://github.com/dragginzgame/icydb/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/docs/design/archive/0.251-runtime-and-entity-codec-wasm-reduction/0.251-status.md#codec-baseline)
records a 2 KiB objective and 4,451-byte reachable result after post-link shrinking;
its older compiler, different workload and separate 8 KiB gate do not redefine
IC DB's target or provide a matched subtraction baseline. The current reachable
result is an unresolved size gap, not an accepted budget.

A separate symbol-preserving diagnostic shows shared planning/execution alongside
eight typed `fetch` wrappers and a large dispatch body containing inlined typed
operations. This directs the next experiment toward typed wrappers and inlining,
not a new schema registry or removal of integrity checks. The diagnostic code
section did not match the measured stripped build byte-for-byte, so its function
sizes are not used to partition the release delta or promise savings. No product
optimization was introduced by that initial measurement-only increment.

### Shared fluent boundaries (0.18)

Move predicate, clause, index-prefix and order construction from `Query<E>` onto
its existing `Request`. Keep allocations and loops outside entity-specialized
inlining. `fetch` and `fetch_as` share one private boundary for schema admission,
planning, execution and request disposal; final result decoding remains typed.
This retains one request representation and one executor, without a public dynamic
frontend, callback table, prepared handle, cache or stored-format change. Limits,
clause order and cursor/job bindings are unchanged.

The same six-subject workload passes with these retained changes, before the
direct-traversal reduction below:

| Subject | Total Wasm | Code bytes | Total change | Code change |
| --- | ---: | ---: | ---: | ---: |
| `direct` | 231,075 | 209,645 | 0 | 0 |
| `direct-entities` | 267,211 | 245,320 | 0 | 0 |
| `one` | 268,973 | 245,400 | +290 | +289 |
| `shapes` | 270,389 | 246,796 | +509 | +517 |
| `maps` | 272,180 | 248,587 | +507 | +515 |
| `entities` | 320,305 | 296,318 | -3,961 | -3,948 |

The entity slope falls from 54,386 to 49,916 bytes across seven added entities:
7,769 to 7,131 bytes each, an 8.2% reduction. Direct typed paths still account for
5,162 bytes per entity; fluent-associated incremental growth falls from 2,607 to
1,969 bytes. The one-entity fixed cost increases slightly in exchange for sharing
across entities. This is progress, not satisfaction of the 2 KB objective / 4 KB
working comparison ceiling.

Every sampled warm fluent operation uses fewer instructions and cycles than the
initial baseline: 261–1,662 fewer operation instructions and 299–1,662 fewer cycles.
For the first query, `one` / `shapes` / `entities` measure 123,652 / 124,063 /
155,848 work instructions and 7,232,631 / 7,233,042 / 7,274,328 complete-call cycles.
The direct controls remain byte-identical. The six-subject result/upgrade checks,
79 native query tests, 68 retained-job tests, focused Wasm projection/cursor/
refusal and bounded-sort/upgrade regressions pass. These remain workload-specific
measurements, not a matched IcyDB performance comparison.

This adapts IcyDB `6d421e3`'s concrete prepared-read boundary lesson from its 0.250
exact-key reduction: share admission/execution and retain only final Rust conversion
at the typed edge. IC DB needs neither IcyDB's binding/catalog reconciliation nor
its prepared public facade; it already owns the required request and encoded result.
An additional concrete `get` lookup candidate was rejected: only about 42 bytes
less per added entity, with a one-entity read cost increase. Direct lookup remains
unchanged. No diagnostic-only candidate or alternate runtime route remains.

This stage is preserved under `target/evidence/0.18/query-growth-request-boundary/`.
The initial baseline, fetch-only intermediate and rejected lookup candidate are
also preserved under `target/evidence/0.18/`; their names identify the experiment.

### Shared direct traversal (0.18)

Primary pages and index-prefix reads now share their storage-advancement steps
across entities. Index-prefix admission also has one concrete boundary. Resolve
the admitted primary-key type once per operation; retain typed row decoding,
physical-key agreement and full index-entry validation at the caller. Each step
handles one row: no extra page buffer, callback executor, cache, supplier-specific
layout assumption, public API or durable representation is added. This continues
the IcyDB boundary lesson above rather than importing its prepared-query facade.

Against the preceding shared fluent boundaries, the same six-subject workload gives
the following intermediate result, before shared typed page conversion:

| Subject | Total Wasm | Code bytes | Total change | Code change |
| --- | ---: | ---: | ---: | ---: |
| `direct` | 231,828 | 210,395 | +753 | +750 |
| `direct-entities` | 262,110 | 240,219 | -5,101 | -5,101 |
| `one` | 269,708 | 246,132 | +735 | +732 |
| `shapes` | 271,130 | 247,534 | +741 | +738 |
| `maps` | 272,921 | 249,325 | +741 | +738 |
| `entities` | 315,316 | 291,326 | -4,989 | -4,992 |

The combined entity slope falls from 7,131 to 6,312 bytes each (11.5%); direct
paths fall from 5,162 to 4,326. Relative to the original 7,769-byte slope, the two
retained reductions remove 18.8%. The one-entity fixed-cost increase is explicit;
the combined result still exceeds the 4 KB working ceiling and 2 KB objective.

All six sampled warm direct calls use 356–465 fewer instructions and cycles.
Post-upgrade direct controls improve by 194/461 instructions and cycles for one/
eight entities. Fluent/mapping costs remain within two instructions and cycles of
the preceding measurement (one mapping case adds two); accept that tiny movement
with the measured size reduction, without claiming a fluent speedup from this
change. Opening-stage counts are unchanged. These are selected read costs, not
a new full-application write-performance comparison.

A buffered primary-page candidate was rejected for extra allocation and roughly
1,500 additional direct-read instructions. Per-row key-type lookup and consuming
supplier entries by value also measured worse than the retained combination.
No alternate product path from these trials remains. The 38 native record/key
tests and focused Wasm records/message-boundary regression pass, including
corruption, ownership, index consistency, reopening and rollback checks; the
six-subject composition workload also passes its result and upgrade assertions.

This stage is preserved under `target/evidence/0.18/query-growth-shared-traversal/`.
The preceding baseline and discarded experiments are retained under
`target/evidence/0.18/query-growth-*`; evidence is local and does not imply a
release or a matched IcyDB performance comparison.

### Shared typed page conversion (0.18)

Keep the existing typed `QueryPage::decode_entities` loop out of caller inlining.
This removes repeated result-conversion code while preserving typed decoding,
page metadata, decode errors and maintenance source-page behavior. It adds no
representation, callback, framework, schema attribute or durable change.
Macro-generated codecs and key extraction remain unchanged.

| Subject | Current total Wasm | Current code bytes | Total change | Code change |
| --- | ---: | ---: | ---: | ---: |
| `direct` | 231,828 | 210,395 | 0 | 0 |
| `direct-entities` | 262,110 | 240,219 | 0 | 0 |
| `one` | 269,820 | 246,243 | +112 | +111 |
| `shapes` | 271,140 | 247,543 | +10 | +9 |
| `maps` | 272,931 | 249,334 | +10 | +9 |
| `entities` | 311,674 | 287,683 | -3,642 | -3,643 |

The seven-added-entity delta falls from 44,186 to 40,534 raw bytes: 6,312 to
5,791 bytes per entity, another 8.3% reduction. Direct growth remains 4,326;
the fluent-associated difference of slopes is now 1,465 bytes. The result is
25.5% below the initial 7,769-byte slope but still exceeds the 4 KB ceiling.
These equivalent three-Nat64-field entities do not establish a heterogeneous
application's per-entity cost.

Explicit tradeoff: the shared conversion adds 18 instructions/cycles to each
sampled multi-shape fluent operation, and 201 in the single-shape build (about
0.16% of operation instructions). Retain that small cost for the measured size
reduction; this change is not a query speedup. Direct reads and opening stages
are unchanged. The driver now records first/eighth inserts and a read followed
by replacement after the existing query samples. Both baseline and candidate
give identical measured insert and read/replace work/cycles in all six variants.
The read/replace figure sums two IC calls, including their transport/open costs
in cycles; it is not one atomic transaction or a client-side timing measurement.

IcyDB's 0.251 codec investigation cautioned that apparent source duplication may
already be compiler-shared. Separate no-inline probes of generated row decoding
and key extraction produced byte-identical artifacts here; outlining batch append
also saved no bytes. Remove all three hints. Consuming raw page buffers while
decoding measured worse than the retained borrowed loop and was discarded too.
No new generic decoder or binding/ordinal machinery was imported from IcyDB.

Two further candidates were measured and discarded. Outlining request defaults
saved 553 raw bytes in the eight-entity build but added 21 in the one-entity
eight-shape build: only 82 bytes per entity, with 15–218 extra sampled fluent
instructions/cycles. Moving the existing managed-time check into shared batch
append saved 224 raw bytes in the eight-entity build but added 28 in the
one-entity eight-shape build: 36 bytes per entity. Sampled inserts added 72
instructions/cycles, while read/replace improved by 14. These small gains do not
justify retaining the hints or moving validation. Both six-subject Wasm runs
passed; the batch candidate also passed 53 native managed/record/copy tests.
That stage retained 5,791 bytes per entity, with no constructor or batch
admission change. Trial evidence is under `query-growth-request-default` and
`query-growth-rejected-batch-admission` in the local 0.18 evidence directory.

The retained boundary passes 79 native query, six maintenance and 13 schema-copy
tests, plus the six-subject Wasm result/upgrade/write comparison and focused Wasm
sparse-page, projection, refusal and upgrade regression. Product/driver Clippy
and formatting checks pass. No discarded hint or consuming-loop path remains.

Current artifacts, hashes, source inputs and results stay under
`qualification/target/evidence/query-growth/`. The baseline with extended write
measurements and discarded probes are retained under `target/evidence/0.18/`.

### Deeper specialization investigation (0.18)

The follow-up investigates whether the remaining growth is a private boundary
problem or requires changing schema authoring, codecs or durable contracts.
Diagnostic builds retain symbol names with `-Cstrip=none -Cdebuginfo=0`.
Their concatenated non-custom Wasm sections match the measured stripped builds
byte for byte, so symbol attribution applies to the actual baseline executable,
not a differently optimized debug build. The one/eight core hashes are
`a00c5cde04e2cf126bd6d4ed2c24da25ac54da8678315a49b0a7b419347bb540` and
`992712c3aea50c366ad9ea9d7d902339233b752cdef77c745c1baff72e2b8b56`.

| Baseline growth from one to eight entities | Bytes |
| --- | ---: |
| Application operation wrappers, including inlined database work, plus commit | 36,113 |
| Typed `Query::fetch` functions | 2,225 |
| Typed revision access | 854 |
| Other code, including shared execution/admission and layout changes | 948 |
| Non-code sections | 394 |
| Total | 40,534 |

Commit is inlined into the one-entity operation wrapper but outlined once in the
eight-entity artifact; those owners must be counted together. The 36,113 bytes
are neither pure application overhead nor automatically removable. The uniform
fixture has one named encoder, decoder and row-info body each, totaling 794
bytes in either build. Named-leaf sharing does not exclude inlined copies;
the earlier decoder/key-extraction experiments separately found no saving.
All eight 314-byte `fetch` bodies differ only in the embedded schema-address
operand. This is direct evidence of specialization at the typed boundary.

A diagnostic fixture splits write, exact-read, primary-page, index-page and
fluent wrappers into separate non-inlined functions. All six result/upgrade/write
subjects pass, but combined growth rises to 6,035 bytes per entity. Its largest
delta families are fluent call sites (10,626 bytes), writes plus shared commit
(8,052), indexed reads/conversion (7,411), primary pages/conversion (5,654),
host dispatch (3,015) and exact reads/conversion (3,010). These explain where to
investigate; they are not additive savings or a reason to weaken the fixture.

A matched mixed-width control changes only `amount` to two entities each of
Nat64, Nat128, Int64 and Int128, with correctly typed operands and the same
positive test values/result oracle. Its eight-entity artifact is 316,433 raw /
291,996 code bytes versus the matched uniform control's 312,167 / 288,176.
The raw slope rises from 5,851 to 6,460 bytes per entity, another 609 bytes.
This includes necessary layout/operand conversions and does not establish costs
for owned text, containers, aggregate outputs or Canic. The matched control
differs slightly from the original fixture because it constructs typed operands.

A disposable product prototype moves the existing streaming primary-page
conversion loop behind a private function boundary, retaining range admission,
key/row agreement checks, decoding and continuation. It adds no raw-page buffer,
allocation, callback, public API or durable representation. The original fixture's
slope falls from 5,791 to 5,503 bytes per entity; the mixed-width slope falls from
6,460 to 6,247. Both six-subject Wasm runs and 38 native record/key tests pass.
Sampled inserts add 66–72 instructions/cycles and direct page calls add 72; the
two-call read/replace journey adds 144. Inserts include page readback, so this
does not isolate commit cost. This prototype is not retained as a new baseline.

A second disposable prototype carries the existing static schema reference in
the private `Query` value and passes it to `fetch`/`fetch_as`, instead of embedding
`E::SCHEMA` at those terminals. Admission still occurs at execution; the reference
is not a new catalog, cache or public dynamic-query API. The eight named `fetch`
bodies merge into one 320-byte body. Its symbol-bearing executable sections again
match the stripped artifact exactly (SHA-256
`d363e01452675c4226462cdb0fc25fe01865bb4fd3cb5c6a1401e53ce2dab820`).
The original fixture's one-entity eight-shape build grows to 271,794 raw / 248,153
code bytes, while the eight-entity build falls to 309,554 / 285,570: a slope of
5,394 raw bytes per entity. Direct-only builds are unchanged. All six Wasm
subjects pass. Sampled eight-entity fluent calls add 6–10 instructions/cycles;
one-entity multi-shape calls add 222–554, and single-shape calls add 316.
The one-entity fluent builds also add 46–48 instructions to sampled inserts and
92–96 to the two-call read/replace journey through changed code layout, despite
unchanged write code. This is not a uniform performance improvement. The added
transient pointer and effects on other aggregate terminals require a cohesive
design and broader qualification before adoption. This prototype is removed too;
the two prototype savings must not be added together.

This follows frozen IcyDB `6d421e3`'s
[operation-wrapper investigation](https://github.com/dragginzgame/icydb/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/docs/design/archive/0.250-generated-entity-specialization-reduction/0.250-landing-slice-6-receipt.md):
generic operation wrappers can dominate generated codec leaves, and symbolic
size attribution is not an additive savings estimate. No IcyDB implementation,
descriptor framework or supplier-specific layout is imported. Local artifacts,
source controls, section checks and diagnostic scripts are under
`target/evidence/0.18/attribution/`; these are experiments, not new product tools.

### Admitted descriptors at typed read boundaries (0.18)

Retain the follow-up that returns the existing validated entity descriptor from
`Schema::admits` and passes it into the typed exact-read, primary-page and
index-prefix bodies. Admission still checks logical identity, fields, indexes
and the complete reachable type graph before any read. The index-key helper no
longer repeats admission or takes an unused database receiver. Public methods,
key/row agreement, index validation, decode errors and continuation are unchanged.
There is no cache, prepared public handle, extra page buffer, callback, new
representation or durable-state change.

The important difference is where metadata comes from. Merely moving static
`E::SCHEMA` arguments into private helpers let the compiler propagate their
addresses and specialize the bodies again. A borrowed two-pointer reader context
did not prevent that either. Execution-only query request objects shared code
but added request-movement/cleanup costs; borrowing reduced some of those costs
without improving the tradeoff sufficiently. Remove those prototypes. Passing
the descriptor returned by actual admission shares the direct read bodies
without retaining any of the experimental context types or query changes.

| Subject | Previous raw / code bytes | Retained raw / code bytes | Raw / code change |
| --- | ---: | ---: | ---: |
| `direct` | 231,828 / 210,395 | 232,564 / 211,125 | +736 / +730 |
| `direct-entities` | 262,110 / 240,219 | 252,087 / 230,193 | -10,023 / -10,026 |
| `one` | 269,820 / 246,243 | 270,311 / 246,732 | +491 / +489 |
| `shapes` | 271,140 / 247,543 | 271,631 / 248,032 | +491 / +489 |
| `maps` | 272,931 / 249,334 | 273,422 / 249,823 | +491 / +489 |
| `entities` | 311,674 / 287,683 | 301,690 / 277,695 | -9,984 / -9,988 |

Combined raw growth falls from 5,791 to 4,294 bytes per added entity (25.8%);
direct growth falls from 4,326 to 2,789. The combined ceiling remains unmet.
Symbol-bearing executable sections match the retained eight-entity artifact
exactly, SHA-256
`6cf0cae148f5a35bbc07b973f62b607573bc02efae5238cf6f30fa34e40284c5`.
There is one named typed exact-read body (336 bytes), primary-page body (796)
and index-prefix body (866) for all eight uniform entities. This demonstrates
sharing for these codecs/layouts, not identical costs for arbitrary entity types.

The final mixed-width control also passes. Its one-entity eight-shape build is
271,704 raw / 248,105 code bytes and its eight-entity build is 307,927 / 283,483,
down from 271,213 / 247,616 and 316,433 / 291,996 respectively. Mixed raw growth
falls from 6,460 to 5,175 bytes per entity (19.9%). This remains a controlled
integer-width comparison, not an owned-container or aggregate-output workload.

Matching one/eight named builds attribute the remaining 29,663 code-byte delta
to operation wrappers plus shared-commit inlining (25,670), typed fluent fetch
(2,225), revision access (931) and other code/layout effects (837). As before,
the combined wrapper/commit owner includes inlined database work; it is not
25,670 bytes of removable application overhead. Direct-read bodies now share,
so further work should investigate the remaining terminal/conversion boundaries.

Explicit retained tradeoff: sampled first inserts add 44–98 work instructions;
eighth inserts save 62–116. These operations include readback. Direct reads save
67–120 work instructions, and fluent calls save 82–896 instructions/cycles.
The complete two-call read/replace journey ranges from 70 fewer to 44 more work
instructions. Total cycles mostly follow work, but the standalone one-entity
direct build increases by 138 cycles per sampled read and 456 per read/replace
pair; its opening costs also change. Keep those increases explicit rather than
calling every operation faster. The measured size reduction justifies this small
cost; it does not establish general write-performance or heterogeneous parity.

The native regression extends compact-handle collision refusal to primary pages
and index reads, and confirms that an equivalent renamed Rust type still reads
through the admitted descriptor. This applies IcyDB's concrete prepared-read
boundary lesson using IC DB's existing compiled schema; no historical binding
catalog or runtime registry is copied. Diagnostic alternatives and retained
artifacts stay under `target/evidence/0.18/terminal-boundaries/`.

Qualification passes 136 focused native record/key/query/maintenance/copy tests,
both six-subject Wasm composition controls, 11 Wasm query and 24 Wasm record
regressions, and the installed-console regression. The console initially retained
an obsolete expectation that unindexed `order_by amount` must fail; replace it
with the supported sorted result and an insufficient-work refusal. Its focused
rerun passes. Product/driver Clippy, formatting and local-link checks pass.
The original fixture and fluent query implementation are restored; only the
admitted-descriptor read change and applicable regression corrections remain.

### Projection decoding and admitted revision reads (0.18)

Retain a result-type-only boundary for the existing projection decoding loop.
`fetch_as<E, R>` still performs the same admission/execution, then delegates to
`QueryPage::decode_projection<R>`. This avoids carrying the source entity type
through output decoding. Byte validation, decoder completion, error propagation
and page metadata are unchanged. No result abstraction, callback or alternate
codec is introduced. This applies the same measured conversion-boundary lesson
as entity pages, rather than importing IcyDB's codec descriptor machinery.

A disposable matched fixture replaces entity fetches with full-field projections
into one shared named record; source entities, predicates, ordering, application
maps, writes, independent tuple expectations and upgrade checks stay the same.
This isolates repeated use of one output shape across entities. It does not
measure different output-record counts, rich projected types or aggregate outputs.
Both controls include the revision-read cleanup described below.

| Projected subject | Before raw / code bytes | After raw / code bytes | Raw / code change |
| --- | ---: | ---: | ---: |
| One entity, one shape | 271,228 / 247,648 | 271,342 / 247,761 | +114 / +113 |
| One entity, eight shapes | 272,640 / 249,040 | 272,662 / 249,061 | +22 / +21 |
| One entity, mapping closures | 274,449 / 250,849 | 274,471 / 250,870 | +22 / +21 |
| Eight entities, eight shapes | 308,620 / 284,621 | 304,932 / 280,935 | -3,688 / -3,686 |

Projected raw growth falls from 5,140 to 4,610 bytes per entity (10.3%). In the
eight-entity build, sampled projected queries save 37–96 instructions/cycles.
Retain the opposite small fixed-cost tradeoff in the one-entity builds: +20 per
multi-shape/mapping query and +203 for the single-shape query. Direct reads,
inserts and complete read/replace costs are unchanged by projection outlining.
The named artifact's executable sections match the stripped eight-entity build
exactly (SHA-256
`10ad4a720e0c25742b7e4be9f73c072d4f6c6f094f7f11cba3c2616f97e169a5`),
with one shared projection-decoding body and its iterator helper.

Also retain the simpler public `revision` path: use the entity ID returned by
schema admission and call the existing revision reader, avoiding a second entity
membership lookup. ID-only internal callers retain their membership checks.
This saves 27 sampled read instructions/cycles, 30 on inserts and 57 on the
two-call read/replace journey. It adds 6–8 raw/code bytes to one-entity builds
and 35 to eight-entity builds. This is a small runtime/simplicity tradeoff, not a
Wasm saving. The ordinary entity-query slope is consequently about 4,298 bytes,
slightly above the preceding 4,294-byte result; the 4 KB ceiling remains unmet.

A separate entity-fetch prototype moved admission into typed terminals and
passed its descriptor to a shared typed helper. It reached 4,057 raw bytes per
entity but added 25–1,117 sampled fluent instructions/cycles. Remove it: the
additional request/error handling is a poor tradeoff for this size reduction.
No changed builder layout or prepared query object remains. Local controls,
source variants, artifacts and costs are under `target/evidence/0.18/fluent-terminals/`.

The final ordinary entity-page artifacts are 271,637 raw / 248,038 code bytes for
one entity with eight shapes and 301,725 / 277,730 for eight entities. The final
mixed-width control is 271,710 / 248,111 and 307,962 / 283,518 respectively,
about 5,179 raw bytes per added entity. Those controls use ordinary entity pages;
the separate projection fixture establishes only the shared uniform output case.

Qualification passes both matched six-subject projection runs, the final uniform
and mixed-width entity-page runs, 134 focused native record/query/maintenance/copy
and revision-invalidation tests, and 11 Wasm query regressions including projection
and upgrade behavior. Product Clippy, formatting and local-link checks pass.
The original fixture is restored; the only retained product changes in this
increment are the result-type decoding boundary and duplicate revision-lookup
removal. No experimental entity-fetch helper or request context remains.

### Expanded sorting costs and direction admission (0.18)

Re-run the unchanged six-subject `make m2-query-growth` fixture after per-field
directions, bounded winner selection and sorted DISTINCT. Compiler, profile,
feature trees and fixture sources match the earlier ordinary entity-page run.
The tooling/driver lock updates do not alter the measured Wasm dependency graph.
This measures their combined reachable cost, not each feature independently.

| Subject | Earlier raw / code bytes | Final raw / code bytes | Change raw / code |
| --- | ---: | ---: | ---: |
| Direct, one entity | 232,572 / 211,133 | 232,575 / 211,136 | +3 / +3 |
| Direct, eight entities | 252,122 / 230,228 | 252,125 / 230,231 | +3 / +3 |
| One fluent shape | 270,317 / 246,738 | 272,543 / 248,766 | +2,226 / +2,028 |
| Eight fluent shapes | 271,637 / 248,038 | 273,862 / 250,065 | +2,225 / +2,027 |
| Mapping closures | 273,428 / 249,829 | 275,653 / 251,856 | +2,225 / +2,027 |
| Eight entities and shapes | 301,725 / 277,730 | 303,960 / 279,751 | +2,235 / +2,021 |

The increment is predominantly shared. Raw growth per added entity moves from
4,298.3 to 4,299.7 bytes; code growth moves from 4,241.7 to 4,240.9. The roughly
4 KB working ceiling remains unmet. Total/code bytes use the existing validated
Wasm artifact reader; no post-link optimizer or changed build recipe is involved.

The first run exposed 1,074–1,227 extra instructions on common ordered reads.
Avoid proving fixed fields when directions already agree: skip the uniform
ascending case, and inspect equality only for terms disagreeing with the primary
direction. Also skip primary-field name lookup when no direction bit is set.
These checks use the existing request bits; no cached proof or planner object is
added. The refinement costs 21 raw/code bytes per fluent artifact and saves
913–1,073 instructions on the affected ordered shapes relative to that first run.

| Warm workload | Earlier work instructions / call cycles | Final work instructions / call cycles | Change |
| --- | ---: | ---: | ---: |
| One entity, primary order | 123,497 / 7,232,476 | 123,658 / 7,232,637 | +161 / +161 |
| One entity, complete sort | 159,589 / 7,268,513 | 157,771 / 7,266,695 | -1,818 / -1,818 |
| Eight entities, primary order | 155,622 / 7,274,102 | 155,776 / 7,274,256 | +154 / +154 |
| Eight entities, complete sort | 192,253 / 7,310,733 | 190,476 / 7,308,956 | -1,777 / -1,777 |

Retain the measured tradeoff for the broader query contract: sampled non-sort
queries still add 125–161 instructions/cycles against the earlier baseline.
Direct reads, inserts and complete read/replace journeys are unchanged in these
samples. The sort selects two results from eight scalar rows; it does not measure
511-row execution costs, rich-key worst cases, DISTINCT execution or Canic.
Frozen IcyDB's uniform-order contracts and bounded-selection regressions inform
the proof obligations; no normalized order object or heap-index machinery is copied.

Raw before/expanded/intermediate/final artifacts, source receipts and logs are
preserved under `target/evidence/0.18/sorter-growth/`, including `comparison.json`.
The final six-subject run passes entity isolation, all reachable query/map oracles,
warm calls and cold upgrade checks. The existing native query and Wasm sorting
checks cover mixed directions, fixed fields, live cursors and bounded DISTINCT.

### Bounded sorting execution envelope (0.18)

`make m2-sort-costs` builds the records host with `query-measurements` and runs
the existing independent sorting oracle through `measured_query`. The wrapper
calls the exact console query function; no second executor or input fixture is
introduced. Its instruction interval includes open, admission, scanning,
selection, final sorting and display conversion, but excludes transport decoding
and encoding. Cycle deltas cover the complete update. This measures reads inside
an update, not replicated-query billing or an isolated comparator. Every measured
call, including refusals, preserves all stable-memory bytes.

Rust 1.100.0, the qualification release profile and current locked dependencies
produce 984,844 raw / 934,955 code-section bytes for this broad qualification host
(SHA-256 `72d22ea4dde18b9964ca91a12f16e9135af83814d552eae77710d87a911536d3`).
That is a measurement artifact identity, not the library's Wasm footprint or a
new comparison against IcyDB. The wrapper is absent from the normal CLI artifact;
the same regression checks the export boundary and ordinary query results there.

All rows have Nat64 ID, category and amount. Amounts increase with ID; category
alternates. Work 512 admits 511 candidates plus exhaustion. Selected pre-upgrade
warm observations are:

| Operation over 511 rows | Instructions | Complete-call cycles |
| --- | ---: | ---: |
| Mixed category ascending / amount descending, retain 1 | 6,581,565 | 13,712,066 |
| Same order, retain 32 | 13,859,773 | 21,113,724 |
| Sorted DISTINCT, two repeated categories | 5,774,489 | 12,879,692 |
| One qualifying final row | 5,106,561 | 12,204,290 |
| No qualifying rows | 5,098,469 | 12,192,280 |
| Refuse work 511 without observing exhaustion | 5,537,510 | 12,622,423 |
| Refuse 511 output bytes for 32 16-byte results | 9,973,873 | 17,060,809 |

After upgrade, the first two-value DISTINCT call includes reopening and uses
7,130,830 instructions / 14,237,848 cycles. Subsequent mixed-order 32-row selection
uses 13,861,051 / 21,114,979. A later dataset retaining exactly 32 distinct categories
uses 8,397,483 / 15,578,287; adding a 33rd distinct value refuses even for limit 1.
Results, live cursor refusals, sparse/empty scans and unchanged backing all pass
before and after upgrade. Build inputs and full observations are retained in
`qualification/target/evidence/m2/sort-costs-build.jsonl`, `sort-costs.wasm` and
`sort-costs.log`.

This carries the cost-envelope lesson from frozen IcyDB `6d421e3`'s
`bounded_large_window_stays_within_sort_comparison_budget` regression into IC
execution measurements. It does not copy IcyDB's heap index or resource tracker:
the current bound remains 32 retained rows, and no product change is justified
by this sample. Long escaped keys, large projected payloads, maximum retained
bytes and larger populations still need separate cost evidence. Native witnesses
cover selected rich-key correctness/refusals; they are not IC performance proof.

## Frozen IcyDB baseline

```sh
cargo fetch --manifest-path qualification/Cargo.toml --locked
make m0-setup
make m0-icydb-baseline
```

Fetching requires read access to the private `dragginzgame/icydb-legacy` repository.
The repository rename preserves the frozen source revision. The benchmark runs
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
fixture entities. No engine code is copied into the host.

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

### Scalar comparison refresh (0.18)

On 2026-11-16, rerun `make m1-records-comparison` against current IC DB and frozen
IcyDB `6d421e3`, preserving the previous complete evidence directory as
`target/evidence/records-comparison-before-018-refresh/`. The shared workload,
schemas, host sources and independent tuple oracle are unchanged: 32 inserts,
duplicate refusal, primary hit/miss, an eight-row index listing, index-changing
read/modify/replace and all row/index results after upgrade. Both engines pass.
Every IC DB insert has fewer measured instructions and complete-call cycles
than its IcyDB counterpart, even before journal settling.

The [comparison matrix](#recorded-matched-measurements) carries the refreshed
headline values. Compared with the preceding scalar run:

| Observation | IC DB change | Frozen IcyDB host change |
| --- | ---: | ---: |
| Total / code bytes | +8,690 / +8,048 | +4,310 / +3,516 |
| Hit / miss instructions | +559 / +559 | -240 / -240 |
| Listing instructions | +68 | +291 |
| Immediate read/modify/replace instructions | +835 | +1,330 |
| Complete replacement cycles, including settling | +1,335 | +392 |

IC DB's lookup increase is about 0.12%, replacement about 0.10%, and listing
under 0.01%. This refresh includes accumulated product and dependency changes;
it does not isolate which change owns each byte or instruction. Retain the
current shared implementation with these measured costs; no new optimization,
cache, separate scalar path or relaxed bound was introduced for this run.
Frozen IcyDB source is unchanged; its different artifact reflects the refreshed
dependency/transport graph, not a legacy engine modification.

IC DB individual inserts span 612,576–750,489 instructions and
7,601,377–7,741,335 cycles. IcyDB spans 1,909,389–2,339,699 instructions and
8,978,031–9,410,504 cycles before settling. For replacement, IcyDB's immediate
call is 2,933,651 instructions / 10,027,899 cycles, followed by
4,090,195 / 11,170,066 settling. IC DB uses 829,457 / 7,843,732 with no settling.
Reopen uses one explicit advance for IC DB (453,236 instructions), versus two
for IcyDB (5,842,738). These are the harness's startup advances, not the entire
upgrade/install cost or a production scheduler. Stable allocation remains
1,703,936 versus 20,578,304 bytes, including supplier reservations.

Both artifacts use Rust 1.100.0/LLVM 23.1.5, the same qualification release
profile, Storage 0.74.1, Runtime 0.22.5, Types 0.20.4 and Wire 0.24.1 host
transport. The driver uses Testkit 0.106.0 with PocketIC 16.1.0. IcyDB separately
retains its product Wire 0.22.4 dependency; IC DB has none. SQL remains disabled.
The scalar IC DB host reaches direct reads/writes, not its fluent, aggregate or
maintenance engines. No current whole-product Wasm ratio follows from this run.

Evidence is under `target/evidence/records-comparison/`, including raw samples,
features, compiler, dirty diff and source hashes (`59ed763` plus captured changes).
Qualification lock SHA-256:
`a9bafdab1aeab887808d7d5f4e6e71a4bfd51540fbc3b556525981cc46dea6be`.
Artifact SHA-256:

- IC DB: `14a2dd78638a1ab3bcdf41d402e399dd6daf28dc361fac799fc086d4789b3b27`.
- IcyDB: `3e835bcad39666ead432abd9fbdfbf2c16286a9104df1277d285ee5e388683cc`.

### Matched fluent first pages (0.18)

`make m2-fluent-comparison` enables `fluent-comparison` only in the two existing
scalar hosts. They expose the same additional `fluent_listing` query endpoint,
construct typed fluent requests and return complete entity tuples through the
same transport. The original scalar controls and independent tuple oracle remain
in use. No product implementation or frozen IcyDB source changes are involved.
The ordinary scalar build keeps its five query/update exports; its regression
also passes after this shared-driver extension.

The dataset is the existing 32 three-Nat64 records with `(category, id)` index,
after replacing ID 7's category/amount with `(0, 707)`. This leaves seven rows
in category 3 and nine in category 0. Cases run on identical data before and
after an unchanged executable upgrade, after each engine's explicit startup
advances. Every returned tuple and order agrees with an independent Rust oracle;
every successful or refused query leaves stable memory unchanged.

Measured on 2026-11-16 under the scalar refresh's compiler, locked suppliers,
release profile and PocketIC setup. These are query instruction intervals,
including the existing host's open/session/admission, execution and typed result
mapping; transport encoding is outside the interval. Query cycles are not inferred.

| Fluent request over 32 rows | Rows | IC DB instructions | IcyDB instructions | Reduction |
| --- | ---: | ---: | ---: | ---: |
| Category 3, amount >= 0, ID ascending, limit 8 | 7 | 843,180 | 3,208,262 | 73.7% |
| Category 3, amount >= 200, ID ascending, limit 8 | 3 | 834,529 | 2,980,596 | 72.0% |
| Missing category 99, ID ascending, limit 8 | 0 | 558,551 | 2,052,511 | 72.8% |
| Category 3, amount >= 200, ID descending, limit 8 | 3 | 826,958 | 2,990,207 | 72.3% |
| Category >= 0, amount >= 700, category/ID ascending, limit 8 | 1 | 1,787,308 | 5,078,704 | 64.8% |
| Same indexed range, amount >= 999 | 0 | 1,786,237 | 6,287,630 | 71.6% |

IC DB's post-upgrade instruction counts are identical. IcyDB's corresponding
counts are 3,205,359 / 2,981,667 / 2,050,826 / 2,990,708 / 5,065,122 / 6,267,659.
These are settled-after-upgrade reads, not measurements of cold startup itself.
Both complete hosts include the original direct read/write controls. IC DB is
277,595 raw / 253,945 code bytes; IcyDB is 3,093,437 / 2,983,940. The increment
over their scalar-only artifacts is 44,357 / 41,982 and 6,073 / 5,566 bytes
respectively. IcyDB already reached its fluent planner in the scalar listing;
these deltas are not comparable first-use ownership costs or general entity growth.

Two contract differences are part of the result:

- IcyDB's typed `limit` caps the traversal across resumed pages; IC DB's row limit
  caps each page. This test compares only the first requested rows on fixed input,
  without claiming continuation or mutation parity.
- Legacy `PublicRead` refuses an unbounded residual scan (code 173), and the
  primary ID-range candidate also refused its index requirement (172). The final
  matched scan uses `category >= 0` with category/ID order on both sides, following
  the legacy [complete indexed-range contract](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/docs/contracts/READ_ADMISSION.md).
  Sorting category 0 by amount descending / ID ascending is a separate capability
  probe: IC DB returns the correct top 8, top 1 and all 9 rows at limits 8/1/32;
  IcyDB consistently returns `QUERY_READ_SORT_REQUIRES_MATERIALIZATION` (174).
  The driver asserts this typed refusal before and after upgrade. It never swaps
  to trusted execution, and no speedup is computed against refused work. This
  does not mean IcyDB cannot sort through other supported lanes.

Initial refusal evidence is preserved under `fluent-comparison-unbounded-refusal/`
and `fluent-comparison-primary-range-refusal/`. The successful complete run is
under `target/evidence/fluent-comparison/`, with source diff/hashes, feature graphs,
artifacts and every observation. No SQL is enabled. Artifact SHA-256:

- IC DB: `e6e7b6944f785ddcab98494b332ddc6649a4e7488271db220c252214d01e9a3c`.
- IcyDB: `8059181d7a6da78c2f1b3c42adabf9eea44ce0bae9077431fa091cc9aea922dd`.

This closes the small first-page fluent comparison gap. Large populations,
projected results, grouping/aggregation, matched successful sorting, cursor
semantics, resource-refusal envelopes and Canic composition remain separate work.

### Matched grouped aggregates (0.19)

`make m2-aggregate-comparison` enables only `aggregate-comparison` in the existing
scalar hosts. One `grouped_aggregate` query endpoint per host executes count,
sum, minimum or maximum grouped by the required Nat64 category. Both use public
fluent APIs and the same 32-row dataset, including ID 7's replacement `(0, 707)`.
IcyDB selects the existing `(category, id)` index for category equality or
`category >= 0`. IC DB retains the default unordered primary scan because this
probe does not select an index explicitly. Both apply `amount >= minimum` as
a filter. These are equivalent results, not matched physical access paths.
No SQL, trusted fallback or client reducer is used.

Eight selections cover one category, sparse matches, a missing category, all four
groups and empty results. All 32 cases agree with an independent tuple/arithmetic
oracle before and after upgrade, and every query preserves the raw stable backing.
There are no null operands in this fixture. Empty input yields no groups. The
normal scalar host's export set stays unchanged; the shared driver's scalar and
fluent regressions also pass using preserved artifacts.

Measured 2026-11-16. Query instruction intervals include open/session creation,
admission, execution and output mapping, excluding transport encode/decode.
Query cycles are not inferred. Four-group output is count `(9, 8, 8, 7)`, sum
`(2147, 1200, 1280, 1290)`, min `(40, 10, 20, 30)` and max `(707, 290, 300, 310)`
in category order 0–3.

| Selection / terminal | IC DB instructions | IcyDB instructions | Reduction |
| --- | ---: | ---: | ---: |
| All rows, grouped count | 926,034 | 3,657,854 | 74.7% |
| All rows, grouped sum | 939,805 | 3,914,882 | 76.0% |
| All rows, grouped minimum | 967,024 | 3,784,791 | 74.4% |
| All rows, grouped maximum | 984,100 | 3,789,956 | 74.0% |
| Category 3, amount >= 200, count | 830,800 | 2,491,948 | 66.7% |
| Category 3, amount >= 200, sum | 834,059 | 2,546,204 | 67.2% |
| All categories, amount >= 700, sum | 851,682 | 3,565,598 | 76.1% |
| All categories, amount >= 999, sum (empty) | 843,328 | 3,540,281 | 76.2% |

Across all 32 settled cases the instruction reduction is 55.7–76.2%. IC DB's
post-upgrade counts are identical; legacy four-group counts are 3,657,651 /
3,912,529 / 3,782,438 / 3,787,603. These are settled reads after startup, not cold
startup measurements. Aggregate-enabled hosts are 296,900 raw / 272,519 code
bytes versus 3,262,572 / 3,151,697: 90.9% smaller raw Wasm for this reachable
workload, not a whole-product comparison. Added bytes relative to the recorded
scalar artifacts are 63,662 / 60,556 versus 175,208 / 173,323; shared reachability
and different result representations prevent interpreting these as per-query costs.

IcyDB returns Decimal for integer sums. Its host converts only exact, in-range
integers to the common tuple; IC DB uses a declared Nat64 result record. Small
fixture totals fit both contracts. This does not compare overflow, rounding,
optional inputs, rich keys, averages, DISTINCT, retained jobs or contention.
Legacy requests allow four groups / 64 KiB state, within its public cap; IC DB
retains its standard 32-group / 128 KiB bounds. Actual output has at most four
small groups; this is not a matched maximum-state or memory comparison.

The first candidate requested 128 KiB from legacy and correctly refused with
code 176. After correcting that harness limit, plain zero-key field aggregates
refused with code 3. Frozen
[zero-key regressions](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/crates/icydb-core/src/db/session/tests/cardinality_tiebreak/zero_key_aggregates.rs)
document that route's special DISTINCT/expression policy. Both final hosts use
explicit category groups instead. Neither refusal is counted as a speed result;
this is not a claim that legacy lacks scalar aggregates through other lanes.
Its metadata-only exact count is also outside this workload. No frozen code was
copied or changed; the retained lesson is to establish admitted shape, complete
input and empty-result semantics before comparing reducer costs.

Evidence: `target/evidence/aggregate-comparison/`, source `88dca75` plus captured
working diff, Rust 1.100.0 / LLVM 23.1.5, locked release-profile Wasm builds and
PocketIC 16.1.0. SQL is disabled in both feature graphs. Qualification lock SHA-256:
`74eb27791daf83bf5ee173d770290c323ed961bc4d809565f27df7417ddfa16d`.
Artifact SHA-256:

- IC DB: `481d59fed658cd5fd9413b77e39ec6fd15605af0efd8f031e00326bdf63d8e69`.
- IcyDB: `8afaafec05a234b8d63292515df04088526bf5484ae775a233889a5478858c57`.

Earlier attempts remain in `aggregate-comparison-initial/` and
`aggregate-comparison-zero-key-refusal/`. No product code, durable state, new
workspace package or dependency is added. Broader grouped and retained-job
comparisons remain open.

### Exact count scaling (0.19)

The original `make m2-count-comparison` selected a separate `count-comparison` feature in the
same scalar hosts. It compares equivalent whole-entity and category-equality
counts using IC DB `query.work_limit(512).count()` and frozen IcyDB
`execute_exact_count()`. IC DB explicitly selects `category,id` for category
counts; the legacy terminal reads maintained cardinality metadata and refuses
unavailable metadata instead of falling back to a scan. No grouped aggregate,
SQL or client reduction substitutes for either terminal.

The existing 32-row workload first rejects a duplicate insert and moves ID 7
from category 3 to 0. The driver then grows the population to 128, 256 and 512
using individual inserts, explicitly settling each legacy write. Counts cover
the whole entity, categories 0/3 and missing category 99; an independent source
vector supplies expected cardinalities. Upgrade checks repeat at 32 and 512
rows. All 24 requests per engine return the expected count or explicit work
refusal and preserve backing. Sampled rows survive the final upgrade. This
qualifies settled counts, not reads during legacy recovery or metadata rebuild.

Measured 2026-11-16; intervals include host opening/session construction,
admission, execution and result conversion, excluding transport. These are
instructions, not inferred query cycles or isolated metadata-lookup costs.

| Population / count request | IC DB instructions | IcyDB instructions | Result |
| --- | ---: | ---: | --- |
| 32, whole entity | 771,727 | 1,193,000 | Both 32 |
| 32, category 0 | 886,654 | 1,281,586 | Both 9 |
| 128, whole entity | 1,564,302 | 1,265,471 | Both 128 |
| 128, category 0 | 2,176,558 | 1,355,299 | Both 33 |
| 256, whole entity | 2,690,976 | 1,295,035 | Both 256 |
| 256, category 0 | 4,014,959 | 1,387,102 | Both 65 |
| 512, whole entity | 4,856,372 (refused) | 1,413,832 | IC DB `Limit`; legacy 512 |
| 512, category 0 | 7,795,044 | 1,503,444 | Both 129 |
| 512, category 3 | 7,328,021 | 1,496,075 | Both 127 |
| 512, missing category | 746,763 | 1,511,632 | Both zero |

IC DB's populated counts grow with scanned rows; legacy's complete-call cost
still grows somewhat despite avoiding row enumeration. This fixture's crossover
is between 32 and 128 rows for populated selections, not a universal threshold.
An empty explicit prefix remains cheaper in IC DB. At 512 whole-entity rows,
IC DB cannot observe exhaustion inside its 512-candidate budget and refuses;
subsequent prefix counts still succeed. It never returns a prefix total.
Post-upgrade IC DB counts are identical; legacy's 512-row whole/category-0/category-3/
missing counts use 1,415,167 / 1,503,188 / 1,495,819 / 1,511,376 instructions.
IC DB retained count is available but its cost is not measured here.

The initial candidate omitted `using_index`; IC DB's default unordered primary
scan also exhausted work for category counts at 512 rows. Evidence remains in
`count-comparison-primary-scan/`. The successful probe explicitly selects the
index for those queries. The preceding grouped receipt is corrected to record
its actual primary-scan/index distinction without changing historical numbers.

This demonstrates useful legacy functionality, not a mandate to port its
cardinality subsystem. Frozen
[exact-count admission](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/crates/icydb-core/src/db/session/query/exact_count.rs)
and [prefix-cardinality regressions](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/crates/icydb-core/src/db/index/cardinality/tests.rs)
require complete, synchronized metadata and refuse partial projections after
capacity loss. Historical cardinality-build progress also had to survive journal
folds. IC DB had no such counts in this run; introducing them adds write and durable
identity obligations. No product/frozen code, durable metadata or
ordinary-write responsibility changes in this probe. Counter maintenance overhead
and larger/other index distributions remain unmeasured.

Evidence: `target/evidence/count-comparison-before-maintained-count/`, source `88dca75` plus captured
working diff, Rust 1.100.0 / LLVM 23.1.5, locked release Wasm and PocketIC 16.1.0;
SQL disabled. Qualification lock SHA-256:
`74eb27791daf83bf5ee173d770290c323ed961bc4d809565f27df7417ddfa16d`.
IC DB is 278,637 raw / 254,734 code bytes; legacy 3,111,171 / 3,002,089.
These are the scalar controls plus one count endpoint, not whole-product sizes.
Artifact SHA-256:

- IC DB: `2e69499ddec2203f5e45ab28ed1e4ec24996b9677f99b17b5478990ac0ec1ddf`.
- IcyDB: `8ba4b59598ec565f9ad2395ef2771050d270f9b28b2b63a01ecdae881b022a35`.

### Count cost decomposition (0.19)

`make m2-count-profile` builds only the IC DB host with `count-profile`, which
includes the original count endpoint and one read-only diagnostic endpoint.
The ordinary comparison and CLI do not expose it. The same driver grows the
fixture through 32/128/256/512 rows and checks counts, refusal, unchanged backing
and upgrades. The original matched-count artifacts remain separate and unchanged.

Two modes execute real product counts (primary scan or explicit category index)
with instruction timestamps around database opening and query execution. Three
controls read the supplier map directly: primary rows; index entries and owner
bytes; and index entries plus owner-row fetches. Raw controls use fixed physical
prefixes for this Nat64 fixture, omit integrity/admission/reducer work, and are
not supported database queries. They read actual payloads, validate expected
counts independently and cap traversal at the 512-row fixture envelope. Passing
them does not authorize skipping product integrity checks.

Measured 2026-11-16, after settled writes:

| Population / category-0 matches | Product opening | Raw index traversal | Raw index plus row fetches | Validated indexed count work |
| --- | ---: | ---: | ---: | ---: |
| 256 / 65 | 458,557 | 582,060 | 2,655,531 | 3,577,438 |
| 512 / 129 | 461,341 | 1,083,725 | 5,580,445 | 7,377,587 |

Work columns exclude opening; raw-map opening is 394,324 instructions. At 512
rows the incremental owner-fetch control accounts for about 4.50M instructions,
while the difference between that control and the full engine is about 1.80M.
These are controlled comparisons, not exact additive profiling samples: raw keys
use `Vec<u8>` rather than the product's bounded key wrapper, and the controls
omit admission and validation. Nevertheless, fetching scattered owner rows is
the largest observed indexed-count increment; opening is only about 6% of the
full indexed call. Caching opening cannot remove its population-dependent work.

At 256 rows the validated primary scan with category filtering costs 2,490,126
work instructions versus the indexed count's 3,577,438, despite examining all
256 rather than 65 candidates. Whole-entity validated scanning costs 2,236,393
versus 1,533,773 for raw primary traversal. At 512 rows the primary count refuses
at its work boundary after 4,404,913 work instructions; raw traversal completes
at 3,021,394. Raising the bound only moves that boundary. These supplier-version
observations do not justify changing storage structure around its current tree
implementation or adding a statistical planner.

The query path loads the owner row, validates its complete encoding and primary
key, reconstructs row/index consistency, evaluates predicates and then counts.
That invariant also detects missing owners and stale/wrong entries. Index-only
counting would reduce work but would change that guarantee. Durable cardinality
would avoid enumeration, with different obligations: atomic counter deltas,
missing/corrupt metadata refusal, rollback, initialization and supported format
migration. The current same-schema transition refusal means an internal-format
upgrade must be admitted deliberately, not smuggled in as ordinary reopen.

The resulting experiment is one maintained whole-entity count per entity, using
the existing bounded preparation/commit boundary, and an explicit metadata-count
contract distinct from row-validating `count`; its findings follow below.
The subsequent hard-cut decision waives an old-format converter. Indexed-prefix counts are a separate
extension because their number and write fan-out depend on distinct prefixes.
Do not copy legacy's online cardinality build, journal folding, broad statistics
or fallback framework. Retain its lesson that unavailable/partial metadata must
never masquerade as an exact count. No product counter or integrity behavior is
changed by this investigation; retained counts remain available for larger scans.

Evidence: `target/evidence/count-profile/`, source `88dca75` plus captured diff,
Rust 1.100.0 / LLVM 23.1.5, PocketIC 16.1.0, the same locked supplier graph as the
matched count run. The instrumented host is 283,837 raw / 259,418 code bytes,
SHA-256 `a6afbab9fafd0001024f664f4acde551e58ef85315c5d67be39a5705cb355483`.
Its extra reachable measurement code shifts instruction costs slightly, so its
numbers are not replacements for the original matched-count receipt. Every
profile case passes before/after upgrade with stable backing unchanged.

### Maintained entity-count candidate (0.19)

Investigated 2026-11-16 in a disposable source copy, with no product source or
shipped format change. The candidate extends the existing per-entity revision
record from 8 to 16 bytes: revision plus exact row count. Preparation accumulates
the final insert/delete delta per entity, checks arithmetic, and publishes count,
revision, rows and indexes in the existing synchronous rollback boundary. Replaces
leave the count unchanged; no-op writes leave the entire backing unchanged.
This adds eight logical payload bytes per entity, no additional metadata key or
per-batch metadata lookup/write, and no journal or cardinality-build lifecycle.

The experimental `cardinality::<E>()` reads admitted metadata without inspecting
rows. It is not a transparent replacement for the row-validating fluent `count`.
Missing or malformed metadata refuses; arbitrary well-formed count corruption
cannot be detected by a metadata-only read. The benchmark substitutes this call
only for whole-entity counts; category counts retain ordinary indexed execution.

| Whole-entity population | Current IC DB scan, instructions | Candidate metadata, instructions | Frozen IcyDB metadata, instructions |
| --- | ---: | ---: | ---: |
| 32 | 771,727 | 475,919 | 1,193,000 |
| 128 | 1,564,302 | 483,637 | 1,265,471 |
| 256 | 2,690,976 | 489,214 | 1,295,035 |
| 512 | Refuses after 4,856,372 | 574,791 | 1,413,832 |

The candidate completes all expected counts and preserves them across executable
upgrade. It uses 59.3% fewer instructions than legacy at 512 rows, including
database opening and the public call's work. One metadata lookup still has tree
and opening costs; this is not a promise of constant instructions at every size.
The scan refusal is not a successful performance comparator. Prefix counting is
not solved: category 0 at 512 rows costs 7,824,202 instructions, versus the
unchanged engine's 7,795,044 and legacy's 1,503,444.

Against the original IC DB count artifact, the 32 sampled inserts each add 1,146
instructions/cycles; the complete read/modify/replace adds 1,638 (830,081 to
831,719 instructions, about 0.20%; cycles 7,844,600 to 7,846,238). Rejected insert
adds 172 instructions/cycles; a primary hit adds 44 instructions. These small
regressions belong in the tradeoff, not a claim of free counts. The host grows
from 278,637 to 279,796 uncompressed Wasm bytes (+1,159), and from 254,734 to
255,796 code bytes (+1,062). Both end at 1,703,936 allocated stable-memory bytes;
allocation granularity hides the extra metadata payload. High fan-out and
multi-entity Wasm write costs remain unmeasured for this candidate.

Four native tests cover empty/multiple entities, final net-zero deltas, replacement,
no-op preservation, late batch refusal, missing/malformed metadata, checked
overflow/underflow, reopen, interrupted same-format copy, and refusal to publish
when source/target counts differ. Target counts are rebuilt through private copy
inserts. This uses the existing key-preserving, whole-database copy contract; it
does not support transformations that add or remove records.

A separate fault-enabled Wasm artifact proves successful insert/delete/replace
and net-zero batches, no-op and late-refusal backing preservation, and full
message rollback after a successful database commit. It also proves same-format
upgrade and bidirectional old/new format refusal with backing unchanged and
service restored by reinstalling the matching executable. The measurement host
has no fault endpoint. These checks do not replace supplier-write-boundary trap
coverage or qualify the entire maintenance lifecycle for the candidate.

**Historical candidate limitation:** this prototype used a distinct manifest and
canonical contract version. It safely refused old data but could not convert it.
The maintainer subsequently authorized a hard cut without a converter. The
adopted form below versions all physical manifests, retaining schema fingerprints.
Ordinary open must not synthesize missing metadata.
Prefix counts remain a separate bounded fan-out/storage proposal. Legacy's
fail-closed exact-cardinality behavior is the lesson retained from
`executor/aggregate/exact_terminal` at `6d421e3`; its generation, online build,
journal folding and publication framework are not copied. The prototype is
independently implemented within IC DB's existing batch/copy boundaries.

Receipt: `target/evidence/entity-count-candidate/`, based on `88dca75` plus
captured pending qualification changes and `candidate.patch`. The final patch
SHA-256 is `e9d594ca658687f12647fff7d906c2bfae8b208e294e00dc05ecc893bee136ae`.
Compiler, locks and PocketIC match the count comparison. Measurement artifact:
`802dfcadd39f1e394e5e5d1c531205a73c83cd58c7057d70349a891b23488526`;
separate proof artifact:
`12843ca741f5e05fcd80f621ecaba791fea0fc5053536e1fd0fa061365f3e541`.
Native invariants, PocketIC count workload and rollback/format proof pass;
focused Clippy covers the candidate library/test, Wasm host and proof driver.
The durable outcome of this disposable experiment is this contract/cost finding,
not a second maintained engine or a new product API.

### Maintained-count adoption (0.19)

The maintainer authorized a hard cut after the experiment: no old-format
conversion or backfill is required. `Database::cardinality::<E>()` now reads the
16-byte revision/count record under live format `ICDBMF02`. Frozen maintenance
and copy/retired records also version to `ICDBMT02`/`ICDBCP02`, preventing old
in-progress work from resuming under the new layout. Old records refuse intact;
there is no reset path. Schema fingerprints and row/index encoding are unchanged.
This replaces the prototype's schema-grammar bump with the existing physical
format admission boundary.

One named entity-state record owns encoding, count and revision. Writes compute
final net deltas during preparation and publish both values in the same existing
metadata write. Missing/malformed metadata, count overflow and underflow refuse
before mutation. Metadata-only counts do not inspect row/index integrity; fluent
`count()` retains its existing semantics. No prefix counters, journals, background
builders, new package or dependency are introduced.

`make m2-count-comparison` now uses `cardinality` for whole-entity requests and
the ordinary explicit index for category counts. `make m2-count-profile` still
profiles real scanning modes independently. The preserved scan receipt above
must not be relabelled as a metadata result.

Native `cardinality`, `schema_reuse`, `maintenance`, `schema_copy` and `records`
tests cover deltas, refusal, corruption-contract separation, physical format
rejection, copy/cancellation/publication and retained metadata admission. The
focused retained-job invalidation test keeps revision behavior explicit. The
existing Wasm commit-failure host exposes count observations only under its fault
feature; its driver checks counts alongside revision and backing restoration at
every injected write trap, real instruction exhaustion, growth refusal/retry and
upgrade. Ordinary hosts do not acquire these fault controls.

Final adopted measurement, 2026-11-16: the whole-entity count costs 574,791
instructions at 512 rows, unchanged from the prototype. Category-0 counting still
costs 7,795,079 versus legacy's 1,503,444. The final host is 279,604 raw / 255,700
code bytes: +967 / +966 over the original scanned-count host. Each of the 32
sampled inserts adds 649 instructions/cycles; read/modify/replace adds 940
(831,021 instructions, 7,845,540 cycles), about 0.11% instructions. Duplicate
refusal adds 30 instructions/cycles and primary-hit instructions are unchanged.
Allocated stable memory remains 1,703,936 bytes. These measurements include the
same reachable scalar/query controls; they do not establish general write costs.

The matched count workload passes for both engines. Supplier fault qualification
passes all 137 replacement backing-write traps, actual instruction exhaustion,
growth refusal at the batch beginning at ID 1700, successful retry, and unchanged
reopen. Six Wasm copy tests pass cancellation, publication, managed values,
successive migrations, intervening writes and retired cleanup under the new
formats. Native revision-invalidation behavior remains unchanged.

Evidence: `target/evidence/count-comparison/` for final performance and captured
source/locks; `target/evidence/entity-count-adoption/` for focused qualification.
IC DB artifact SHA-256:
`9f91fab79dbf0d2ff590dcf1dc6ffe4cefcfdb2b826ae167aee86bb574bcb475`.
The earlier scan and candidate artifacts retain their separate identities above.
No frozen IcyDB code, metadata or release was changed.

### Canic typed application integration (0.19)

Canic now owns the optional adapter and `qualification/canic-db-witness`.
Its focused `make db-test` qualifies a real application in local PocketIC,
building normal and fault-enabled Wasm through the current Canic workspace.
The witness consumes IC DB Git revision
`a8b2b8406e8388619e0836ad9785939209cd13e1`; it does not use the prefix-count
candidate. The adapter was first qualified against isolated Canic revision
`e84702b3bdcf4eb209476aadd053df21f18dd8eb`, then moved into Canic and requalified
with its existing Wire 0.24.2 selection. Unrelated pending Auth work is preserved.
Canic's adapter changes are local and uncommitted; no release or push is claimed.

```sh
make -C ../canic db-test
```

Prepare Canic's simulator with its `make qualification-setup`, or explicitly
select an already prepared directory through `IC_TESTKIT_SERVER_DIRECTORY`.
The adoption run used IC DB's prepared `.tools/m0-pocketic` directory. No
deployment, initialization of a live database or simulator download is implicit.
The temporary IC DB adapter patch and runner are removed; do not maintain a second
adapter or fixture protocol here.

The candidate mounts `db_inspect`, `db_query` and `db_write`. Their passive types
and Wire identities belong to the application. Canic captures the actual caller,
invokes synchronous application authorization before payload copying/decoding,
then delegates framed transport to Wire and execution to Platform. The application
explicitly initializes its owner allocation and IC DB allocation, declares `Item`
with named `id`/`score` fields and a `score,id` index, and constructs existing fluent
operations. No engine dependency, endpoint registry, schema compiler, authorization
store, migration controller or dynamic query executor is added to the adapter.

The installed test passes these externally observable checks:

| Boundary | Observed result |
| --- | --- |
| Inspection | Actual declared entity, field types and index names; current admitted row count |
| Authorization and framing | Wrong caller denied before malformed payload decoding; wrong method/direction and oversized frames refuse; rejected writes leave the database empty |
| Batch interactions | A conflict with an existing key and duplicate targets refuse the complete batch |
| Sparse listing | Two nonmatches produce an empty page with progress; continuation returns the expected later rows; completion is explicit |
| Continuation admission | Changed predicate and malformed cursor refuse; work outside the application's bound refuses |
| Upgrade | Five upgrades preserve stored owner/data; unchanged query continuation works after the first upgrade |
| Reply failure | An oversized application reply traps after successful mutation; row count remains unchanged and the same inserts succeed on retry |
| Maintenance | Reads/writes refuse while frozen, including after reinstalling the ordinary source executable; explicit supported cancellation restores access |
| Fault isolation | Ordinary Wasm has no private fault/maintenance endpoint |

Requests permit 1–3 results and 1–4 examined candidates; writes contain at most
four rows. Engine output is bounded to 1 KiB and transport to 64 KiB. These are
witness envelopes, not new product limits. Authorization is deliberately one
installation-owned principal; tenant/row policy, multi-application composition,
schema graph growth, complete migration workflows and a deployed application CLI
are not established by this test. The normal `db` CLI still uses its local fixture.
No performance comparison with legacy Canic/IcyDB is claimed.

Canic's current `target/db/` contains the source archive, lockfile/compiler/features, Cargo
artifact messages, normal/fault Wasm and passing installed run log. Historical
isolated evidence remains under IC DB's
`qualification/target/evidence/canic-db-integration/`, including the original
patch and source snapshot. Historical artifact SHA-256 (not the adoption build):

- Normal: `5083e0f1c5018ffa6cb78e9e228de3c2ace1978563390f1e96d705ae1375f525`.
- Fault: `dd39dc5e850085286f8ef811a58c3107e66a7c5e6dcd82f6c23d4c9bcc138924`.

Ownership review: IC DB retains its existing executor, codecs, schemas and durable
format. Canic's established Blob mounting pattern supplies the adapter mechanics;
IcyDB's frozen `guards/mod.rs` supplies the retained authorization-before-request
lesson, not copied SQL or session machinery. The witness's owner record is
application state, not a new database representation. Canic is now the sole
source/test owner. The application-specific request types do not establish a
generic remote CLI contract; common inspection and explicit application-query
selection must be settled before connecting `db` to deployed databases.

### Maintained prefix-count candidate (0.19)

Parked by maintainer sequencing: application integration and engine maturity take
priority over further optimization. Retain this evidence without adopting the
candidate format or its additional durable counter obligations.

`make m2-prefix-candidate` copies the current source to an ignored evidence
directory and applies the reviewed [candidate patch](experiments/prefix-counts.patch)
only there. It builds a current control, the candidate and frozen IcyDB, then runs
native invariants, matched count/write journeys and real IC rollback checks.
The existing engine under `crates/`, its format and public API remain unchanged.
Source copies, exact patch, archives, locks, feature graphs and artifacts are
retained in `target/evidence/prefix-count-candidate/`; this is a disposable
experiment, not an alternate supported engine or an automatically applied patch.

The candidate maintains one exact first-component count for compound indexes
whose first field is required Nat64 and is not the primary key. These narrow
eligibility rules select the experiment, not the eventual product type surface.
Each nonzero prefix has one record in the existing map: tag/entity/index/value
key (17 bytes) and a little-endian u64 count (8 bytes). Zero removes the record;
an absent record means zero only under the admitted, completely maintained
candidate format. Old `ICDBMF02` data refuses intact; isolated live/maintenance/
copy markers are `ICDBPFX1`, `ICDBPM01` and `ICDBPC01`, with no converter/reset.

Preparation folds old/new index memberships into signed net deltas, checks
underflow/overflow and publishes counts with the existing atomic row/index
commit. Same-prefix replacements have zero net count change. Private migration
inserts rebuild counts rather than copying source counters. The explicit
experimental `prefix_cardinality` lookup does not inspect row integrity; normal
fluent `count()` remains reachable and keeps its row-validating behavior. Both
paths are measured, preventing executor deletion from masquerading as savings.

IcyDB research at the frozen revision:
[`index/cardinality`](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/crates/icydb-core/src/db/index/cardinality/mod.rs)
and its [regressions](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/crates/icydb-core/src/db/index/cardinality/tests.rs)
require synchronized ownership, checked signed changes, nonzero retained
multiplicity and invalidation of incomplete evidence. Its optional heap projection
is bounded at 4,096 retained prefixes and can become unavailable rather than
rejecting writes. The candidate independently applies the signed-delta algorithm
to durable counters in one live state; it does not copy the heap cache, overlay,
watermark or generation machinery. It also does not inherit the optional-cache
space guarantee: durable space grows with distinct prefixes and there is no
eviction. Adding eviction would require an explicit availability contract.

Measured 2026-11-16, matched three-Nat64-field / one-index fixture, with the same
locked supplier graph and complete opening/admission/result-conversion interval:

| Operation | Current IC DB instructions | Candidate instructions | Change |
| --- | ---: | ---: | ---: |
| 32 individual inserts, total | 22,727,917 | 24,390,060 | +7.31% |
| Read/modify/replace, category changes | 831,021 | 931,209 | +12.06% |
| Read/modify/replace, same category | 827,013 | 836,347 | +1.13% |
| Read/modify/replace, identical value | 537,252 | 541,081 | +0.71% |
| Primary hit | 472,769 | 480,520 | +1.64% |
| First indexed listing | 773,046 | 785,459 | +1.61% |
| 512 rows, category-0 count (129 matches) | 7,795,079 scanned | 669,448 metadata | 11.64× fewer |

Legacy's metadata count for the same prefix costs 1,503,444 instructions: the
candidate uses 55.5% fewer in this fixture. Candidate metadata lookup grows from
482,928 at 32 rows to 669,448 at 512, so this is not a constant complete-call cost.
Scanned counts are still exercised separately against independent expected rows.
Insert overhead ranges from 3.07% to 13.82% across the 32 samples. Cycle debits
are in the logs; for the category-changing replacement they are 7,845,517 versus
7,945,705, including platform/transport costs. Query cycles are not inferred.

The candidate host is 287,548 raw / 262,459 code bytes versus control 279,593 /
255,692: **+7,955 raw / +6,767 code bytes**, including the extra metadata-query
endpoint and retaining scanned counts. Per-entity growth is not measured here.
At 512 rows both allocate 1,703,936 stable bytes; fitting four 25-byte counters
into existing allocation does not make them free. A separate native witness
stores 512 distinct counters, checks 12,800 logical key/value bytes and removes
them on last-row deletion. Physical reclamation or shrink is not inferred.

Four native tests pass final-state exchanges, same-prefix and changing-prefix
writes, last-row deletion, no-op/refusal backing preservation, exact counter bytes,
malformed/zero/missing-underflow/overflow cases, incompatible-format refusal,
reopen, high prefix cardinality and a chunked migration that changes categories
and row encoding. The target's independently expected counters match after
publication. Both control and candidate Wasm count journeys pass before/after
upgrades. The candidate also passes full-backing rollback at all 218 writes of
the selected replacement, real instruction exhaustion, growth refusal at ID 1700
and successful retry; the retry grows from 26 to 34 pages. No ordinary-write WAL
or second mutation executor is introduced.

**This is not ready for automatic adoption.** Product work still needs an explicit
counted-prefix declaration and schema identity, supported key/equality semantics,
and broader fan-out/growth envelopes. The candidate's maintenance pass validates
counter record shape, while the probe independently checks rebuilt multiplicities;
it does not yet supply a bounded, independent publication proof for every prefix
counter. Missing or well-formed but wrong metadata is not an integrity scan result.
Resolve that admission/validation contract before shipping the format. The
evidence supports investigating opt-in counts, not maintaining every possible
prefix or silently replacing fluent `count()`.

Final artifact SHA-256:

- Control: `586867f82339aef0a94d098048db728eb39c809cac22d5ddb9661b4257180759`.
- Candidate: `14c3960ea23fc41dbfecb860b7489d9ab6ec731721eb41ca051e5482c1429c12`.
- Legacy: `1ed815dfb58ef7e50747c163fbec2c1c7ed3c159bb8d2c6c3b7e926f77814b89`.

### Aggregate access-path control (0.19)

`make m2-aggregate-paths` retains the query matrix's populations, distributions,
thresholds, result oracle and upgrade checks. It compares the default primary
scan with explicit `using_index("category,id", &[])` for each IC DB aggregate.
Legacy executes its same public, automatically indexed query in both arms; the
private operation selector does not substitute another legacy execution lane.
All **3,072 executions / 1,536 matched pairs** pass. Both IC DB paths return
identical complete groups, and the backing remains unchanged.

Selected grouped-count instructions at 511 rows, amount threshold zero:

| Distribution / selection | Primary scan | Explicit index | Index / scan |
| --- | ---: | ---: | ---: |
| Balanced, all categories | 6,965,800 | 28,891,096 | 4.15× |
| Balanced, category 0 (127 rows) | 5,742,943 | 8,038,537 | 1.40× |
| Balanced, missing category | 5,404,490 | 758,725 | 0.14× |
| Skewed, all categories | 6,814,298 | 28,755,858 | 4.22× |
| Skewed, category 0 (415 rows) | 6,498,990 | 23,607,628 | 3.63× |
| Skewed, category 3 (32 rows) | 5,492,566 | 2,605,029 | 0.47× |
| Skewed, missing category | 5,403,706 | 753,820 | 0.14× |

This rules out an unconditional switch to the index, including for every category
equality predicate. The non-covering index fetches and validates owner rows;
the primary scan reads rows directly. The measured crossover depends on input
selectivity, layout and supplier, so it is not a constant to hard-code into the
planner. Keep explicit access selection and the current default. Any later
automatic choice needs a bounded information source and evidence across both
sparse and dense cases, while preserving admitted semantics and continuation.

Measured 2026-11-16 with the same compiler/locked graph as the query matrix.
The original matrix artifacts remain intact. New evidence lives in
`target/evidence/aggregate-paths/`, including `queries.csv`, `summary.tsv`, raw
results and source/build receipts. Hosts are 321,866 raw / 296,887 code bytes and
3,293,226 / 3,181,702 respectively. SHA-256:

- IC DB: `854dc15dfa3103429449034818d5c912bddc387ec53e27b122c8321b8a10981d`.
- IcyDB: `5402e8ab13bc2b714a160fd7ae9afaf09ea8fba1c5540c86d29180ca6f8fd295`.

### Broad query matrix (0.19)

`make m2-query-matrix` explicitly builds both existing scalar hosts with fluent,
grouped and count endpoints together. It uses frozen IcyDB
`6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac`, with SQL disabled, and the current IC DB
working tree. No product implementation, frozen source or supplier is modified.
The new host feature explicitly gives IC DB queries a 512-candidate work budget;
its normal 128-candidate default and older comparison builds are unchanged.

Measured 2026-11-16: **4,680 executions** cover 195 cases at each combination of
population (32/128/511), distribution (balanced/skewed), engine and phase (settled
before/after upgrade). Every result agrees with independent source tuples, sorting
and arithmetic; each complete read matrix preserves stable backing. Of 2,340
paired cases, 432 have the expected IcyDB public-lane sort refusal (code 174).
Those are excluded from speed comparisons. Of the remaining **1,908 successful
pairs**, IC DB uses fewer instructions in 1,796 and legacy in 112. These frequencies
reflect the selected matrix, not application query frequencies or independent
random trials.

The schema has three Nat64 fields (`id`, `category`, `amount`) and one
`(category,id)` index. IDs are sequential. Amount is `(id * 37 % 101) * 10`, giving
non-monotonic values and repeated ties. Balanced categories use `id % 4`; skewed
categories use zero when `id % 16 < 13`, otherwise `id % 4`. Thus category zero
contains 415 of the largest skewed fixture's 511 rows. Amount thresholds
0/500/950/1001 exercise dense, selective, sparse and empty results.

The 195 cases comprise four primary hit/miss requests, three first-ten index
listings, four exact counts, 120 fluent listing/sort requests and 64 grouped
count/sum/min/max requests. Fluent limits are 1/8/32; category selections include
0/3/missing 99 and all categories. Ordering is ID ascending/descending, compound
category/ID, or amount descending with ID tie-breaking. This compares first
results, not cursor compatibility. The 511-row ceiling leaves room to establish
scan exhaustion within IC DB's 512-work bound; it does not qualify large retained
jobs, arbitrary datasets, rich types, DISTINCT, averages or projection costs.

The table gives **IC DB / IcyDB instruction ratios**: below 1 is cheaper for IC DB.
Each population's p50 is the upper median of paired ratios across both
distributions, all selected parameters and both phases. The range preserves
the fastest and slowest relative cases instead of hiding them in one average.

| Query family | 32-row p50 | 128-row p50 | 511-row p50 | 511-row range |
| --- | ---: | ---: | ---: | ---: |
| Primary hit | 0.434 | 0.435 | 0.455 | 0.391–0.497 |
| Primary miss | 0.446 | 0.445 | 0.507 | 0.467–0.507 |
| First ten index matches | 0.250 | 0.234 | 0.235 | 0.218–0.264 |
| Empty index listing | 0.260 | 0.258 | 0.314 | 0.308–0.314 |
| Filtered ID ascending | 0.270 | 0.272 | 0.326 | 0.083–0.600 |
| Filtered ID descending | 0.271 | 0.272 | 0.327 | 0.076–0.587 |
| Compound ordered listing | 0.290 | 0.262 | 0.258 | 0.012–0.384 |
| Whole-entity maintained count | 0.394 | 0.390 | 0.392 | 0.348–0.393 |
| Populated prefix count | 0.679 | 1.553 | 5.115 | 1.605–14.441 |
| Empty prefix count | 0.427 | 0.424 | 0.497 | 0.480–0.498 |
| Grouped count | 0.335 | 0.455 | 0.543 | 0.150–2.608 |
| Grouped sum | 0.327 | 0.431 | 0.503 | 0.150–2.571 |
| Grouped minimum | 0.331 | 0.452 | 0.542 | 0.150–2.571 |
| Grouped maximum | 0.331 | 0.451 | 0.541 | 0.150–2.571 |

Two losses deserve separate interpretation:

- Skewed category-zero count at 511 rows costs **22,425,798** IC DB instructions
  versus **1,553,142** legacy instructions before upgrade. IC DB performs an
  indexed, row-validating count; legacy reads synchronized prefix metadata.
  Their exact logical answers agree, but their integrity work and write-side
  obligations differ. The earlier [count profile](#count-cost-decomposition-019)
  identifies owner-row fetch cost; this is not primarily opening overhead.
- Missing-category grouped count at 511 balanced rows costs **5,404,276** versus
  **2,072,550**. IC DB's unordered default uses the primary scan; legacy chooses
  the category index. By contrast, grouping all 511 balanced rows costs 6.97M
  versus 40.61M for count and 7.07M versus 44.30M for sum. These results expose
  access-path/selectivity tradeoffs, not a universal reducer advantage. Explicit
  index selection exists in IC DB but is not substituted for its default in this
  matrix. No planner or counter is added merely to improve these numbers.

Instruction intervals include opening/session construction, admission, query
execution and result conversion, excluding Wire decode/encode. Direct listing
uses IC DB's index-prefix API and legacy's fluent API. Grouped integer sums in
legacy return Decimal and undergo exact in-range conversion in its host. The
group-state ceilings remain different (legacy four groups / 64 KiB, IC DB 32 /
128 KiB); both produce at most four small groups here. Legacy startup and journal
settling happen explicitly after each seed write. Upgrade readiness is measured
separately before either engine's post-upgrade queries; this is not a cold versus
heap-cache benchmark. Query cycles are not inferred from instructions or a
delayed query charge. Startup/reopen call counts, instructions and observed cycle
debits remain in the raw log.

Combined reachable hosts: **321,798 raw / 296,819 code bytes** for IC DB versus
**3,293,215 / 3,181,691** for legacy: 90.2% less raw Wasm in this fixture. This
does not establish complete product parity, entity growth savings or SQL-removal
savings; SQL is already excluded from both builds. Rust 1.100.0 / LLVM 23.1.5,
locked release builds and PocketIC 16.1.0 are used. Artifact SHA-256:

- IC DB: `1ad869f84cd36506fbf1115fc1641c10fb747b3e74c1f4510bd926879d93bbed`.
- IcyDB: `3056bfc82d31d63572328480e1071c95e4eb7c30742a74bd49387b006bc8eb43`.

Evidence is in `target/evidence/query-matrix/`: `queries.csv` contains every
measurement/refusal; `summary.tsv` contains family p50/ranges; `results.log`
contains readiness costs and exact-result checks. Source HEAD/diff/hashes, the
driver, lock, feature graphs, compiler/build receipts and both artifacts identify
the run. The qualified lock SHA-256 is
`0344fcaf2a4482df6935a9e70a146622095086f329ca594f6bb070ffcd078f1a`.

### Mixed-history qualification (0.19)

Run `make m4-lifecycle` after the existing `make m0-setup`. This selects only the
`mixed-lifecycle` host feature and one driver test; it does not run the full
qualification suite or expose fault controls in ordinary builds. Product crates,
formats and public APIs are unchanged. The model uses ordinary `BTreeMap`s and
full-state uniqueness/reference checks, without an IC DB dependency, codecs,
schema metadata or product query helpers.

The three synthetic entities are parents with unique codes, items with indexed
scores and references to parents, and links referencing items. Twelve fixed
rounds change keys, values and batch order. Initial population is 28 records,
peak population 31; the largest batch has 24 commands. Queries return complete
results within 32 rows, retained jobs advance two candidates per call, and copy
uses two rows / 4,096 encoded bytes per chunk. These names are fixture data, not
a proposed application schema or public request protocol.

The 2026-11-16 run passes:

- 153 attempted batches, including three-record chain insertion/deletion,
  read/modify/replace, reference repointing and unique-value swaps. Every attempt
  is followed by four complete observations compared with independently expected
  rows, counts, revisions, parent-prefix filtering, score-index order and grouping.
- 73 refused batches and 13 no-ops preserve the complete stable backing. Missing
  targets, duplicate commands, unique collisions and surviving references refuse
  without applying an earlier valid command in the same batch.
- An incomplete exact count survives unrelated writes, no-ops, a refused write
  and upgrade, then explicitly invalidates after its source changes. A completed
  count retains its original answer after another row is inserted. Jobs are
  explicitly cancelled afterward, without adding a scheduler.
- Six upgrades preserve stable backing, including interruptions during copy and
  validation. Copy transforms the accumulated 28 records, widening indexed score
  values from Nat64 to Nat128 and adding 1,000. Source and target ordinary access
  remain closed before publication. Rebuilt counts, both index orders and groups
  match the transformed model; target writes and reads remain valid through
  bounded retired-source cleanup and another upgrade.

The fixture found no product defect in this envelope. It supplements the separate
backing-write trap, instruction-exhaustion, growth-refusal and migration-cancellation
probes; it does not replace them. It does not establish random-history coverage,
continuous-write aggregate completion, rich-type parity or large-dataset bounds.

Selected complete update measurements (opening, preparation, commit and the
caller-side read for read/modify included; Wire decoding/encoding excluded from
the instruction counter):

| Journey | Samples | Instructions | Observed cycle-balance decrease |
| --- | ---: | ---: | ---: |
| Insert three-record reference chain | 12 | 905,594–1,386,827 | 7,941,761–8,424,930 |
| Delete entire chain in one batch | 12 | 1,049,291–1,119,600 | 8,083,387–8,153,826 |
| Read item, change score, replace | 13 | 659,373–774,069 | 7,668,499–7,783,195 |
| Repoint an item to another parent | 12 | 683,288–736,553 | 7,692,414–7,745,680 |
| Exchange two unique codes | 12 | 726,491–767,428 | 7,751,221–7,792,288 |
| Identical replacement | 13 | 238,672–253,553 | 7,247,801–7,262,679 |

The 612 complete read checks cost 598,714–2,041,805 instructions each. One check
includes all three entity scans, metadata, two item-index queries and grouping;
it is not the cost of a single lookup. These samples follow update calls with an
initialized host runtime. Post-upgrade reopening is checked separately, without
claiming a cold/warm performance comparison. Cycle-balance deltas also include
transport/platform costs and potentially periodic query charging; they are not
isolated commit-compute costs. For example, one refused-delete sample has a larger
cycle debit despite comparable execution instructions. No matched legacy run or
general speedup is claimed.

The feature-enabled host is 1,069,948 raw / 1,017,816 code bytes, including existing
unrelated qualification endpoints, both schemas, jobs and maintenance. This is
not comparable with the small scalar/count host. SHA-256:
`2b9a39df550b30f4a73ba2d5ccb41be89e24030840c676991967f36fcaa3a4e8`.
`target/evidence/mixed-lifecycle/` retains the host, compiler/build receipt, selected
lock, source HEAD/diff, explicit fixture sources and test log. The frozen IcyDB
count regressions and final-batch uniqueness rule inform these checks; no legacy
executor or journal code is copied, and neither IcyDB nor Canic is modified.

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

The earlier M0 schema witness serializes one bounded canonical contract with field names,
explicit entity/index IDs (superseded by M1 logical names), named enum variants and versions for row/key codecs, comparison,
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

`nested_row_codec` adds one handwritten recursive row projection, originally
qualified with Wire 0.22.1 and now using Wire 0.24.0's payload API. It retains
test-only Serde 1.0.229 (already transitive).
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
canonical field-name order. Wire 0.24.0 owns this fixture's payload serialization; a small
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
The locked Testkit is 0.106.0 with PocketIC 16.1.0. Calls use raw client methods
and this workspace's explicit Wire codec/budgets; no `ic-testkit-wire` adapter is
required. Direct benchmark reads use `ic-host-artifacts` 0.80.1 for bounded
complete bytes and SHA-256 identity; the CLI uses `ic-host-text` 0.76.0.
Testkit's Artifacts/FS/Process dependencies still resolve to 0.79.0, so two
Artifacts versions are intentionally present until that separate API migration.
Tests use fresh instances and do
not need the reusable pools retired in Testkit 0.105.0 or the diagnostic reports
retired in 0.106.0. The CLI uses Text's retained escaping/width/table APIs, not
its removed domain formatters. No host package enters
the database product or deployed Wasm.
The earlier Testkit/Text adoption was qualified with native/Wasm Clippy, record/default/generator and
endpoint-boundary Wasm tests, the schema fingerprint oracle and the matched scalar
comparison. Earlier probe runs retain their own dependency evidence; this refresh
does not relabel every historical M0 result as a new run.

Artifacts 0.80 removed the Wasm inspector. The benchmark-only `tests/artifact`
helper now derives export names and code-section payload bytes with `wasmparser`
0.263.0 and validates the complete module before returning facts. It retains
the 32 MiB input, 128-section/export and 32-custom-section limits; byte admission
now happens while reading rather than after an unbounded file read. No supplier
inspection framework, mutable file API or product dependency was copied into IC DB.
Qualification covers independent literal Wasm measurements, malformed/truncated
and duplicate-export refusals, byte/custom-section limits, affected benchmark
callers and the six preserved query-growth artifacts. Their raw/code byte counts
and SHA-256 identities must match the prior receipt; runtime samples do not rewrite
historical measurement evidence. Run the focused native boundary check with:

```sh
cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test artifact_admission
```

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

## Product retained aggregates

`make m2-jobs` selects the native job tests and a feature-selected records-canister
build using the actual product `AggregateJobs`, query scanner, exact reducers and
IC Storage allocation. The CLI's optional `retained-console` build shares the job
allocation without rollback fault controls. Testkit owns the PocketIC instance. Private endpoints
are test controls; production authorization and endpoint generation remain Canic's.

Native tests qualify counts beyond the synchronous 512-candidate budget,
sparse indexed filters, independent 64-KiB source-byte steps, exact signed/Decimal
cancellation across checkpoints, U256 preservation, all-absent results, query/
result/database/store binding, sequence refusals, same-code reopen, backwards
time, expiry/capacity/cancellation and safe slot reuse. Output/arithmetic refusal
preserves prior progress. Malformed cleanup input and unsupported store headers
refuse with backing bytes intact. Synchronous query tests separately qualify the
shared scanner and reducers.
Average cases additionally cover unequal step sizes and present counts, positive/
negative even ties, absence versus zero, 1025-row cancellation at scale 28,
reopened denominators, operation/result-scale mismatches, U256 conversion after
division, and finalization refusal without a scale fallback or lost checkpoint.
Grouped-count cases use an independent complete-input count/sort oracle, including
521 rows, multi-field/absent keys, sparse indexed input, empty input, selection
bindings and output refusal. Separate probes exceed group cardinality and the
4-KiB whole-job bound even when selection hides groups, preserving backing bytes.
Reopening reconstructs comparison keys; malformed group counts refuse intact.
Grouped numeric cases add 522-row complete totals, all-absent groups, signed
rounding ties and selection on rounded values, reopened cancelling totals and
denominators, Decimal/U256 preservation, operation/key/scale mismatches, excluded
overflow and output/frame refusals. A malformed nonzero total with zero present
count refuses intact and remains explicitly cancellable.
Field-count cases distinguish 520 rows from 260 present operands, count empty
blobs/text/collections, reopen zero-count groups and bind required as well as
optional fields. Unknown/mismatched fields and output refusals preserve progress.
Retained DISTINCT cases use an independent tuple-set oracle over 521 rows and
reopen across varied work bounds. They cover empty/all-absent input, zero values,
UTF-8/embedded-NUL keys, duplicate suppression across steps, cardinality/frame
refusals, changed projection/terminal, output refusal, revision invalidation and
malformed/duplicate saved keys. Corrupt payloads refuse intact and remain cancellable.
Grouped DISTINCT cases compare 521 rows with an independent map of sets, including
overlapping projections, absent tuples and completed selection. Reopen/binding/output
checks preserve exact sets; separate probes exercise group cardinality, per-group
cardinality and the combined frame bound. Two independent full 32-key sets succeed.
Sparse/empty input and malformed inner keys have explicit results/refusals.
Completed-page cases cover aggregate ordering/ties and total limits, cursor retry,
reopen, later source writes, expiry, foreign/pending/reused jobs, malformed and
out-of-range positions, byte-budget retries and oversized first groups. Count,
field/DISTINCT count, sum and average pages share selection and conversion. Paging
cannot hide an excluded overflow; non-resume terminals refuse the modifier.
Scalar extrema additionally cover cross-chunk winners, retries/reopen, historical
completed results, empty/absent/zero/extreme integers, UTF-8 and embedded-NUL text,
operation/field/query binding, invalidation and exact output/frame refusals.
Malformed presence markers/optional tags refuse intact and remain cancellable.
Provisional winner size is independent of completed output allowance.
Grouped extrema compare independently calculated winners across 521 rows, reopen
and completed pages. Cases cover empty/all-absent and overlapping projections,
selection bindings, ordering ties, byte retries, historical results, UTF-8/NUL
winner ordering, group/frame caps despite excluded output and malformed winners.

The Wasm witness injects traps during creation and each measured checkpoint
backing-write position, then after a successful advance returns to application
code. It compares the complete stable-memory image, including allocator metadata.
A trapped source write preserves the revision; no-op and unrelated-entity writes
allow completion. Same-code upgrade preserves unfinished scalar and grouped state;
retries do not double-count. Real source changes invalidate running jobs; completed totals retain
their captured boundary until expiry. Explicit bounded cleanup frees slots and
stale handles cannot cancel later occupants. Average checkpoint and later
application traps also restore complete backing bytes, as do grouped checkpoint
and later application traps. Grouped sums/averages receive both trap probes and
same-code upgrade/retry/completion checks. This qualifies scalar/grouped
count/sum/average and scalar/grouped DISTINCT count. A separate scalar min/max
witness checks checkpoint/application traps, upgrade, retry, invalidation and
completed historical winners. Grouped min/max additionally qualify checkpoint and
application traps, unfinished upgrade recovery, completed paging across a second
upgrade/source write, retry, invalidation and expiry. It does not establish
physical-memory shrink or completion
under sustained writes. Initial quotas are design bounds, not peak-memory or
performance claims. The earlier fixed M0 grouped probe remains separate evidence.

The driver can also install a recorded scalar predecessor before upgrading to
the current artifact, selected explicitly by `IC_DB_M2_SCALAR_JOBS_WASM`. The
grouping run used the previously verified scalar artifact with SHA-256
`3602176165900fc7ad6475ad273b7e28f0d29cbcc0888c5a02da7e9ba6b1785c`, retained at
`qualification/target/evidence/m2/scalar-jobs-before-grouping.wasm`. Its unfinished
count/sum/average jobs resume under the grouped implementation without migration.
Without that optional artifact, `make m2-jobs` verifies same-code upgrades. This
specific compatibility witness does not qualify arbitrary changed-code recovery.
The numeric grouping run also selected `IC_DB_M2_GROUP_COUNT_JOBS_WASM` for the
intermediate grouped-count predecessor, SHA-256
`70998b9a5bcce9abbc60a019ed9853d257e237cb4ef6549f453b2e49f472ebad`, retained at
`qualification/target/evidence/m2/count-groups-before-numeric.wasm`. Its unfinished
grouped counts resume under the numeric grouping implementation without migration.
Omitting either predecessor uses the current executable for that upgrade step.

The 0.11 field-count run selected the pushed 0.10.0 checkpoint artifact for both
predecessor steps, SHA-256
`fcc68105c9b20de36584ad8a7a53e8886a2424ac949bbeffc4947dd2d7640ea4`, retained at
`qualification/target/evidence/m2/v0.10.0-jobs.wasm`. A second Wasm test saves
scalar/grouped counts of an absent Document note, verifies backing-write and
later-application rollback, then upgrades and completes with zero counts.
The DISTINCT run additionally supplied `IC_DB_M2_FIELD_COUNT_JOBS_WASM` to this
second test, using the prior verified field-count artifact with SHA-256
`988ede03d5722dc39dbf064f0b1e393c88ffc1c1823ebd56ca242b4cccf01275`, retained at
`qualification/target/evidence/m2/field-count-before-distinct.wasm`. The first
test also checks DISTINCT backing-write/later-application traps, upgrades an
unfinished set, retries and completes with two categories across six rows.
Grouped DISTINCT adds both trap probes and same-code upgrade/retry, completing
two groups with one distinct category each. The grouped run supplies
`IC_DB_M2_DISTINCT_JOBS_WASM` for the intermediate scalar-DISTINCT predecessor,
SHA-256 `664aaa344ecb77b2454b72365d23ab82e6c2ec4b70935e671333a2ba9ad58080`, retained at
`qualification/target/evidence/m2/scalar-distinct-before-grouped.wasm`. Its saved
scalar DISTINCT set resumes under the grouped implementation without migration.

A third Wasm test qualifies completed-group pages, final-progress/first-page and
later-application rollback, and continuation across source changes and upgrade.
It also pages a job completed by the prior executable, selected with
`IC_DB_M2_GROUP_PAGES_PREDECESSOR_WASM`: SHA-256
`3fe0851839f25583ff1ac2277123fd0770f36c53c49f637e4478ff87e8604e6a`, retained at
`qualification/target/evidence/m2/group-distinct-before-pages.wasm`. Omitting this
artifact checks same-code upgrades. The API now returns GroupPage rows/continuation;
job bytes remain unchanged. The CLI keeps its synchronous `query` terminals and
adds `aggregate Entry`, `aggregate Document` and `aggregate DecimalRow` to demonstrate retained progress
and completed pages. `make m2-cli` selects the console feature and tests fourteen
Entry selectors against
independent fixture results, a twenty-nine-job session that proves slot cleanup,
bounded partial progress, one-group pages, invalid commands and byte refusal.
Twelve query-control cases distinguish input filters from completed-group
threshold/order/total limits, cover Decimal/widened-integer operands, ordering
ties, sparse/empty input, empty selected output and scalar totals under `--limit 1`.
Twenty-six Document cases cover twenty scalar/grouped selectors plus input and
group controls: absent notes, present zero priority, signed score sums, empty
input and empty selected output. Invalid entity/filter/selector combinations
refuse. Fourteen DecimalRow cases cover ten count/sum/average selectors and
completed-group controls over signed fractional inputs and all-absent optional
inputs. Every supported entity reuses synchronous query input-filter validation.
Paired synchronous/retained CLI cases cover text, signed, fractional and
full-width integer group thresholds, inclusive bounds, absent results, inverted
ranges and filter/order/limit/page composition, plus required/optional Decimal
extrema and their exact fractional bounds, plus field/DISTINCT counts that
distinguish absent values and zero counts. Count thresholds stay Nat64 even for
Decimal fields. Direct query cases check that equivalent Decimal spellings and
signed zero deduplicate before and after upgrade, without changing stored bytes.
Incomplete retained steps emit no
result tables. Direct Wasm query cases check invalid
syntax, integer fractions, numeric overflow, excess Decimal precision and the
64-byte text bound, including Decimal admission after upgrade. Retained CLI
refusals cover integer fractions, signed overflow and excess Decimal precision.
Upper-bound refusals include invalid operands even when the lower bound would
exclude every group, and group modifiers on scalar aggregates.
The focused `console_text` Wasm test changes stored notes to distinguish absent
from empty, lexical from numeric order, and literal `None` from absence. It checks
64-byte UTF-8 bounds, invalid upper bounds even after an excluding lower bound,
unchanged query bytes, and retained resumption across upgrade. Changed text bounds
refuse without changing the saved checkpoint. Both min and max use the existing
product comparator; no adapter-side text normalization is introduced.
Empty or whitespace-containing CLI bounds use one-shot shell arguments; the
interactive console's whitespace splitting does not interpret quotes.
It also preserves the existing installed-layout query/schema and product-record
checks. `make db` installs this console fixture without starting the program.

### Controlled product aggregate contention (0.18)

Run only the product sum/write-schedule witness:

```sh
make m2-contention
```

It shares `m2-jobs-artifact` with `make m2-jobs`, which also selects this test.
The existing private `write`, `job_start`, `job_advance` and `job_control`
endpoints exercise the actual engine and supplier; no new endpoint, product state
or host scheduler is introduced. Testkit owns the local PocketIC instance.

The dataset has 96 entries with independently calculated amounts
`id * 10 + id % 2`. Each advance permits 12 candidates. The successful runs
need nine advances: eight process 96 rows and the ninth observes exhaustion.

| Intervening messages | Observed result |
| --- | --- |
| None | Complete, sum 46,608 |
| Unrelated entity write after each running step | Complete, same revision and sum |
| Same-value source replacement after each running step | Complete, same revision and sum |
| Source write followed by a trap after each running step | Complete, same revision and sum; full backing restored by each trap |
| Refused duplicate insertion after each running step | Complete, same revision and sum; full backing unchanged by each refusal |
| Source change after the first step, three explicit attempts | All invalidated after 12 examined rows, without a result or hidden restart |
| Fresh attempt after those writes stop | Complete, sum 46,610 for the newly captured revision |

One contended attempt restores the old row value and upgrades before resuming;
it still invalidates. Retrying an invalidated handle preserves its outcome, and
explicit cancellation makes that handle unusable. Completed output keeps its
captured revision and value after a subsequent source write. Job-checkpoint
writes never invalidate their source.

These are deterministic execution schedules, not wall-clock load or performance
measurements. They demonstrate the accepted possibility of starvation and
recovery after a quiet interval; they do not establish its frequency for a real
application, high-cardinality group finalization costs or completion guarantees
under continuous source changes. The historical M0 aggregation probe above
remains candidate evidence, separate from this product-level check.

## Identified maintenance and the local console

The product gate is now separately qualified with `make m3-maintenance`.
It uses the actual runtime and record fixture, with executable variants that
support transformation versions one and two for the same source/target schemas.
Native probes compare complete row/index/revision entries, exact manifest bytes,
identity bounds, malformed records and source-compatible cancellation/reopen.
Wasm probes test message rollback of freeze/cancellation, immutable identity
admission across upgrades, read refusal while frozen and restored writes after
cancellation. The copy witnesses exercise the actual
product `SchemaCopy` against fresh source/target allocations. It checks chunk
checkpointing, key preservation, rebuilt indexes, unique collisions across chunks,
wrong allocation pairs, bounded validation, publication and cancellation. Native
corruption probes remove rows/indexes/revisions and introduce stale indexes,
wrong owners, extra rows and malformed payloads. None may reach publication.
Whole-database cases additionally copy unchanged, changed and empty entities,
resume at entity boundaries, reject wrong entity order/pairing, validate cyclic
references and detect missing empty-entity revisions. Wasm probes copy references
before their targets, compare backing after chunk/validation/publication traps,
reject changed code and a recorded 0.16 executable, then publish and enforce
ordinary restricted deletion. A second journey constructs a dangling target
reference, refuses publication and resumes bounded cancellation to the original
source. Entity additions/removals and key remapping remain unsupported.
The managed-time witness renames timestamp fields and rebuilds their indexes,
preserving distinct creation/update times. Native checks reject role changes,
changed timestamps in a prepared chunk and validly encoded timestamp corruption
before publication. Wasm checks compare whole backing after refusal/traps, resume
across upgrades, reject a changed transformation version and query the rebuilt
timestamp index before/after an ordinary target update.
The maintenance-only source-page endpoint also checks byte-limit retry, oversized
rows, exact typed contents and continuation across upgrades. Native checks cover
source schema admission, corrupt rows/revisions, cursor separation from ordinary
queries and cancellation/edit/refreeze invalidation (including edits restored to
the old value). Raw backing comparisons prove reads/refusals do not repair data.

Set `IC_DB_M3_PREDECESSOR_WASM` to a recorded 0.15 record-fixture artifact to also
prove old-executable read/write refusal while frozen and reopening after cancel:

```sh
IC_DB_M3_PREDECESSOR_WASM=/absolute/path/to/0.15-records.wasm make m3-maintenance
```

The first run used the artifact built from release-qualified source `057fa51`
(the implementation released as `v0.15.0`). Without this optional artifact, the
probe still tests current ordinary admission and the two distinct executable
transition versions; it does not claim older-executable coverage. Production
endpoints and authorization remain Canic-owned. The controls in this probe are
private fixture endpoints, not additions to `db` or a public dynamic API.
The copy probe traps after target initialization, row/index copying, validation,
publication and cleanup,
compares complete stable memory, then resumes across upgrades. Changed executable
transformation identities refuse both copying and cancellation. Raw supplier
inspection verifies transformed row bytes; ordinary target admission stays closed
until validation and publication, and remains closed after cancellation. The
publication journey writes/reopens target after upgrades and verifies that source
stays retired and cannot be cancelled back into service. The predecessor artifact check
applies to the original freeze gate, not the new copy fixture allocations.

The publication journey also runs bounded retired-source cleanup: traps restore
the entire chunk, mismatched transformation versions refuse, interrupted deletion
resumes, and repeated completion leaves backing unchanged. The retired manifest
survives and target entries remain identical. Set
`IC_DB_M3_RETIRED_PREDECESSOR_WASM` to a recorded 0.16 maintenance-feature artifact
to additionally check that older executable with partially cleaned source and
the live target. The first run used release-qualified source `92f3228` (released
as `v0.16.0`); without this optional artifact, only current executable versions
are covered. Neither probe claims physical shrink or allocation reuse.

The successive whole-database journey runs A → B, replaces/inserts records and
references in B, then runs B → C into a third fresh allocation. C adds a note field
and index; expected results are checked through that new index after upgrade.
Second-migration cancellation restores the exact B data, including intervening
writes. Stale A → B commands, changed transformation versions and cleanup of an
active source refuse without changing backing. A cleanup runs safely while B is
frozen; B cleanup after C publication preserves target queries and reference checks.
Initialization/copy/publication/cleanup traps restore complete backing. Bounded
loops require progress; repeated cleanup is a no-op. The driver reports physical
backing bytes after each publication and cleanup. These are fresh-allocation
measurements, not evidence of reclamation/reuse or a production migration envelope.

The 0.17 run with Storage 0.74.1 recorded the following whole-canister backing
(including fixture data and supplier overhead, not just migrated rows):

| Checkpoint | Bytes | 64 KiB pages |
| --- | ---: | ---: |
| A → B published | 2,752,512 | 42 |
| C initialized/partly copied; A cleaned | 3,276,800 | 50 |
| B → C published | 3,276,800 | 50 |
| B cleaned | 3,276,800 | 50 |

The third fresh allocation grew backing by 524,288 bytes. Logical source cleanup
did not shrink that allocation. These tiny datasets establish lifecycle behavior;
the selected larger envelope and failed-growth probe follows below. Repeated
allocation reuse needs separate evidence. Do not optimize product structure
around this supplier layout.

`make m3-maintenance-wasm` also builds `maintenance-faults` and runs
`product_capacity`. This variant reuses the ordinary commit-failure backing
wrapper. It copies 128 rows of 4096 bytes into eight indexes with 512-byte physical
keys, in at most 32-row/128 KiB chunks. It checks normal structural refusals and a
trap after the first backing write, then denies real physical growth. Failure must
leave complete backing unchanged. Retrying the same chunk after upgrade with growth
allowed must both grow and make progress. The independent envelope oracle (shared
with `write_envelope`) checks all 128 rows and 1024 index entries through bounded
raw pages. Validation and retired-source cleanup must finish within explicit call
bounds; maximum observed IC instructions are reported per phase.

The fault flag is heap-local, set within the tested message and cleared on a
normal return; traps roll it back with that message. The probe does not turn a
recoverable database/runtime error into a trap. It adds no product testing hook,
allocator or execution-budget machinery. Claims apply to this selected shape and
supplier version, not all combinations of references, types, heap sizes and tree
histories. Normal measurement/console builds do not enable this feature.

With Storage 0.74.1, the 0.17 probe denied one initialization growth request and
one growth request in each of four 32-row copy chunks. Each failure preserved
backing; the same operation then succeeded with growth enabled. Copy retries
performed 4,671–5,969 backing writes per chunk. After publication and source
cleanup, all 128 rows and 1,024 index entries matched the independent oracle.

| Phase | Per-call bound | Maximum observed IC instructions |
| --- | --- | ---: |
| Target initialization | One fresh target | 1,497,721 |
| Copy | 32 rows / 128 KiB source bytes | 118,030,506 |
| Validation | 32 physical entries, plus bounded admission | 61,016,055 |
| Retired-source cleanup | 32 entries | 3,366,626 |

The measured interval includes host reopen/allocation admission, product work and
supplier calls, excluding transport decoding/encoding. These fault-enabled fixture
measurements establish completion for this workload; they are not an IcyDB
comparison, universal maximum or production heap/capacity proof. The selected
tests also reject a 33-row request and a 4,095-byte source-page budget without any
backing write or growth. The original ordinary-write and write-envelope probes
exercise the shared fault wrapper and byte oracle independently.

### Cross-chunk reference migration (0.18)

```sh
make m3-references
```

This focused Wasm probe migrates 256 rows forming 128 two-way reference pairs.
Each partner is 128 primary keys away, so the first 32-row copy chunk references
targets that have not been copied yet. The transformation adds a unique rank
index while preserving entity names and primary keys. Independent literal-byte
oracles check source rows/indexes and all 256 target rows plus 512 index entries.

The valid case resumes across upgrades during copying and validation, then
publishes and cleans the retired source. Deleting one referenced target refuses;
deleting its pair in the same batch succeeds. The invalid case introduces a
missing target in the last row: validation refuses after earlier checkpoints,
publication stays closed, and bounded cancellation restores source service.
Backing comparisons cover a trap on the first copy write, a later application
trap after a validation checkpoint, and normal refusals. Cleanup retains only the
recorded manifest in the discarded allocation.

After successful publication and pair deletion, the same probe repoints the
remaining 254 rows to one target (253 dependants and its self-reference). A
32-command batch deleting that target and the first 31 dependants must refuse
without a backing write: a later surviving reference still blocks deletion.
Eight explicit batches then remove the dependants, with the target included only
in the final batch. Literal row/index oracles check every intermediate result;
a late application trap restores a deletion batch, and deletion continues after
an upgrade. These are application-issued bounded batches, not automatic cascades
or a transaction spanning messages.

With Storage 0.74.1, the fault-enabled fixture recorded:

| Phase | Per-call bound | Maximum observed IC instructions |
| --- | --- | ---: |
| Copy | 32 rows / 4096 source bytes | 17,053,201 |
| Validation | 32 physical entries, plus bounded admission | 7,615,070 |
| Retired-source cleanup | 32 entries | 2,754,782 |
| Cancellation cleanup | 32 entries | 3,473,406 |
| Post-publication restricted deletion refusal | Target plus first 31 dependants | 6,849,884 |
| Post-publication accepted deletion | At most 32 rows per batch | 13,699,351 |

Counters include host reopen, schema construction, product and supplier work,
excluding transport decoding/encoding. The measured artifact SHA-256 is
`9c1064deeee28fda77cb7d680112d52264951956d3c2f29c5cf6df859f6e8a65`.
Both fixture endpoints are gated by `maintenance-faults`; inspection of the normal
record artifact confirms neither export is present. `make m3-maintenance-wasm`
also selects this test.

This carries forward IcyDB's target-existence and final-batch deletion invariants
using the existing IC DB maintenance mechanism, without its general relationship
framework. Migration qualifies one entity with small Nat64 rows and one incoming
reference per row; post-publication deletion additionally checks 254 incoming
references on one target. Nested references, larger populations, arbitrary payload
sizes and allocation reuse remain outside this result. It introduces no product
API or durable state.

### Retained jobs during maintenance (0.18)

```sh
make m3-jobs
```

These focused Wasm tests combine the existing `aggregate-jobs` and maintenance
fixture features. The cancellation case starts running,
completed, cancelled and expiring sums over the six-row source, then freezes the
database. Both starting a job and accessing running/completed jobs must remain
closed across upgrades. These fixture adapters unwrap ordinary database admission,
so the `Maintenance` refusal appears as a message trap; complete backing must stay
unchanged.

An executable supporting only transition version two cannot cancel the active
version-one maintenance. It can still cancel a known-format job and remove expired
jobs one at a time through the separately owned job store. A cleanup no-op and
stale cancellation leave backing unchanged. Reinstalling the supported executable
permits maintenance cancellation; a trap after cancellation restores the gate.
Successful cancellation restores the unchanged source, allowing the original
running sum to continue from its checkpoint without double-counting. The completed
sum retains its exact result, and removed handles cannot affect a newly reused slot.

The test uses explicit synthetic host times for job expiry; PocketIC clock advances
only drive upgrades. It does not claim survival past expiry, access through a
closed source, compatibility with changed job formats, or job transfer at cutover.
`make m3-maintenance-wasm` includes this combination. No new durable state,
automatic cleanup, retry policy or migration/job coordinator is introduced.

A separate native cutover regression migrates one entity while copying another
unchanged. The unchanged entity has the same rows and numeric revision in the
published target. Its source cursor and both running/completed source jobs still
refuse with `InvalidCursor`/`InvalidJob`, without changing backing. Retired source
admission remains closed, while independent job cancellation/expiry cleanup and
a fresh target job succeed after reopening. This tests the existing whole-database
schema/incarnation binding; it does not provide job transfer. Run just this
regression with:

```sh
cargo test --locked -p ic-db --test schema_copy publication_refuses_source_cursors_and_jobs_for_an_unchanged_entity -- --exact
```

The second `make m3-jobs` Wasm case carries that boundary through real upgrades.
It reuses the whole-database fixture: two unchanged mapping rows reference two
entries whose amount type widens. A private feature-gated endpoint starts/resumes
mapping counts and reads cursors against explicitly selected source/target memory.
Copying still requires a separate exhaustion call after each full page. Publication
is first trapped and checked against complete stable backing, then committed.
Upgrades during copying, after publication and after cleanup retain old-position
refusal, source closure and independent cleanup. A fresh target count completes
with two rows at the same numeric revision; removed handles cannot cancel it.
Both Wasm cases pass. No product code, durable representation, automatic routing
or new allocation is added; this small fixture is not a capacity measurement.

### Historical M0 maintenance probe

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
console. `DB_INSTALL_PREFIX` overrides `~/.local`. Fault-injection maintenance
variants are not built for the console. Cargo artifact records select both
build outputs. Keep the installation's `bin` directory on `PATH`.

Run `db` to open a fresh temporary local record database. Commands are `schema`,
`status`, `query [Entry|Document|DecimalRow]`, `aggregate ENTITY`, `migration-demo`,
`help` and `quit`.
`migration-demo` creates separate source/target allocations for a six-row `Item`
transformation. The console feature carries its source and target declarations;
product manifests own all progress. After copying two rows, the CLI upgrades the
local canister with the same executable and resumes. It finishes bounded copying,
validation, publication and retired cleanup, then reads the new label index.
The command runs once per console session; it leaves the ordinary query fixture
unchanged. It has no fault controls or deployed-canister access.
`make m3-cli` selects the installed-command check and a Wasm test of wrong-phase
refusals, exact copy progress across upgrade, validation before publication,
source closure, repeated cleanup and target reopen. Refusals compare complete
backing, including allocator metadata. This demonstrates the existing contract;
it does not add migration capabilities or replace the larger M3 probes.
Entry is the default for `query`; `aggregate` requires an explicit entity.
Queries use the fluent product engine. Entry supports `--min` and `--category`;
Document supports `--entry-id`. `--order-by FIELD[,FIELD]` selects a proven complete
order or a bounded complete sort, `--desc` reverses it, and `--select id` or `--select id,amount` uses typed
projection (the latter is Entry-only). Full entities are the default result.
For example, `db query Entry --order-by amount --desc --limit 2 --select id,amount`
scans all six entries and returns IDs 6 and 5 with no continuation. Complete sorting
retains the best requested 1–32 matches and refuses work/state/output exhaustion
and `--after`. The Wasm sorting witness scans 511 candidates, selects the best
one or 32 with mixed directions, and repeats across upgrade. Native full-sort
oracles also exercise primary ties, identity DISTINCT and capacity recovery as
losing rows are replaced. An exact 511-candidate work budget refuses; 512 permits
the exhaustion check. Non-identity sorted DISTINCT retains at most 32 distinct tuples.
`make m2-queries` includes its Wasm console-boundary check for
global selection, limits, unchanged backing and indexed cursors across upgrade;
no fixture endpoint or client sorting engine is added.
Per-field directions use `--order-by category:asc,amount:desc`; the six-row fixture
returns IDs 6, 4 and 2 with `--limit 3 --select id,amount`. `--desc` reverses this
entire order. The same Wasm check covers mixed sorting and explicit descending
primary cursors across upgrade, and the installed-console test covers the syntax.
Native query tests compare optional text (including NUL and equal labels) against
independent Rust ordering, exercise equality-fixed index proofs and byte retries,
and retain all work/row/state/output limits. These checks establish correctness,
not performance parity for the added direction capability.
Sorted DISTINCT can use `db query Entry --order-by category:desc --select category --distinct --limit 1`:
the six matching rows become two sorted categories before selecting category 1.
Non-identity projections require exactly their projected fields in the explicit
order. Native tuple/text oracles and Wasm upgrade checks cover duplicate removal
before limits, hidden-order refusal and complete-input work bounds. The Wasm
witness reduces 511 rows to two categories across upgrade, accepts exactly
32 distinct categories and refuses a 33rd even with a one-row output limit.
Native mixed-direction tuple oracles fill all 32 retained slots and then revisit
them with duplicates. This reuses the sorter, not the
unordered DISTINCT set or a new replay mechanism.
DecimalRow also supports `--select value` and `--select maybe`, including sorted
DISTINCT; its default console population is empty. The sorting Wasm test seeds
eight rows with scale aliases, zero, signed fractions and both fixed-scale i128
extremes. Both directions retain exact values across upgrade: six distinct
required decimals fit exactly 102 output bytes, while 101 bytes refuses; eight
candidates require a work allowance of nine to observe exhaustion. Eight absent
optional values collapse to one byte without using the sparse index. Native
integer-cent oracles also check optional mixed-direction ordering, primary ties,
grouping agreement and equality filters. Refusals and successful reads preserve
complete backing. These are correctness bounds, not a new performance claim.
Entry also supports `--select category`; combine it with
`--order-by category --distinct` for the ordered-DISTINCT path. `--category` is
supported; `--min` varies within a category and refuses with this projection.
Full entities and ID projections are inherently unique.
`--select count` counts matching rows; Entry also supports `--select category,count`.
`--select count:FIELD` counts present field values (for example, Document's `note`).
Entry supports `--select distinct-count:category` and `--select category,count:FIELD`.
Entry also supports `--select min:amount` / `max:amount`; Document supports
`min:note` / `max:note` and `min:priority` / `max:priority`. No present operand
produces zero output rows; present zero produces one result row.
Prefix those extrema with `category,` for Entry or `entry_id,` for Document to
group the results. An all-absent group stays visible with `—` as its value.
DecimalRow has three CLI-seeded samples (1.25, 2.50, -0.75), all in zone `true`.
`count:value` and `distinct-count:value` both return 3. `count:maybe` returns
0 while `distinct-count:maybe` returns 1; prefix with `zone,` for grouped counts.
Its `min:value` is -0.75 and `max:value` is 2.5; prefix either with `zone,`
for grouped results. Optional `min:maybe`/`max:maybe` return no scalar row,
while grouped extrema retain the all-absent group. Decimal extrema use exact
scale-2 operands for group bounds in both synchronous and retained CLI modes.
`sum:value` totals 3; `zone,sum:value` groups that total. `sum:maybe` is absent,
while `zone,sum:maybe` retains the all-absent group. Decimal sums preserve scale 2.
These are fixed typed projections and direct fluent terminals in the private
adapter; field presence uses the ordinary schema admission.
For synchronous `query` aggregates, `--limit` selects completed groups and no
continuation is returned.
They refuse `--after`, `--distinct`, explicit ordering and execution that reaches
`--work` before observing completion; an exact-boundary scan can also refuse.
The separate `aggregate` command follows retained progress and completed-page
continuations as described in the retained CLI qualification above.
For `query`, `--limit` bounds output rows, `--work` bounds candidates and `--bytes` bounds encoded
output. Their defaults are 3, 128 and 4096. An empty page may still continue.
The CLI prints a hexadecimal next cursor or `none`; copy it to `--after` with the
same entity, filters, order and projection. Numeric after-ID syntax is retired.
A full final page may need one last empty call. Both schema information
and records come from the running fixture's query controls. Schema responses carry
structured field/index descriptions; only the native CLI depends on `ic-host-text`
and renders bordered tables. Full storage identities live in `db status`.

Use `db query Entry --category 1 --order-by id --desc` for one command, or pipe
newline commands to `db`. `make m2-cli` checks piped/one-shot use, cursor reuse and
query-shape refusals, projected bytes, work-exhausted pages, all three entities and help.
The old post-page-filter fixture endpoint is removed; mutation/growth probes now
read through the fluent witness. CLI qualification uses the transport version
selected in `qualification/Cargo.lock`; product dependencies and persisted bytes
remain independent of Wire.
`db --help` needs no running server. The installed binary locates the fixture
relative to itself and works outside the repository directory. Testkit's prepared
server is still tied to the build checkout; run `make m0-setup` there before use.
Invalid commands/options fail scripts with a nonzero status;
interactive mistakes return to the prompt. The console never evaluates SQL or
shell commands. `quit` or EOF closes the Testkit-owned instance. Local fixture
endpoints are qualification controls, not a shipped Canic adapter or a new public
dynamic-query API; there is no remote connection or durable CLI session.
