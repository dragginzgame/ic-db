# Database qualification

This independent workspace contains disposable M0 mechanism probes and an M1
test host consuming the actual product library. The product crate has no dependency
on this workspace. Published suppliers and exact
resolved versions are recorded in [Cargo.lock](Cargo.lock). The comparison fixture
also consumes IcyDB at an immutable Git revision; it is not an IC DB dependency.

IC DB uses Storage 0.78.2 named stable-memory allocations and upstream stable collections. Storage owns its allocation directory codec. Version 0.78 rejects 0.77 backing. Existing data requires an export with the previous owner. IC DB owns its row, index and job codecs.

Storage 0.78 may read malformed keys before invoking the DB codec. Writes reject keys over 512 bytes. Decoding an oversized stored key traps and preserves backing. The installed count witness preserved stable backing across an ordinary upgrade.

Package directories match their Cargo package names. Qualification callers use
Wire 0.27.0's root `encode`/`decode` APIs with explicit `Encoding::Frame` direction
and existing method IDs, or `Encoding::Payload` for isolated codec probes. The
frozen IcyDB comparison retains its separate transitive Wire 0.22.4 selection;
there is no cross-version Wire type exchange or product Wire dependency.

Probe schemas are synthetic examples of generic database invariants. Names such
as users and mappings in existing fixtures do not prescribe product entities or
establish application compatibility. The private Toko GitHub repository is a
separate tier-2 coverage target; the obsolete local checkout is not evidence.

## Native and Wasm boundaries

| Boundary | Why the target matters |
| --- | --- |
| Schema build and registration | The host build collects declarations through `ctor`. Generated entities use an empty registration macro in Wasm so target code needs neither the linker constructor nor the host registry. |
| Native database fixture | `testing::Fixture` supplies vector-backed memory to the product's native `Database`. On Wasm that handle requires IC stable memory. |
| ULID issuance | IC time, canister identity/version and the management entropy call come from Platform on Wasm. Native issuance needs the system clock and OS entropy. Both call the same generator. |
| Fixture ingress, traps, time and measurements | Platform type-checks fixture dispatch on native and Wasm. A measured request requires the installed IC instruction meter; the native decoder reports that the meter is unavailable. Installed dispatch supplies the instruction meter to measured handlers and captures message time for retained console jobs. Native tests call database operations directly, supply fixed job timestamps and panic on traps. Its private handlers have no generated IC entrypoints on native, which accounts for the target-specific dead-code lint expectation. |
| Mixed-lifecycle endpoints | Canic 0.242.0 type-checks the mounted inspection, query and write handlers and their caller guard on native builds. PocketIC supplies the caller and instruction meter during installed execution. |
| Frozen comparison fixture | The legacy actor macro supplies its own native test watchdog stub. The qualification shim supplies the watchdog in the other configurations. |

## External simulator ownership (0.19)

The native driver uses PocketIC 16.1.0 through Testkit 0.110.3 with default
features disabled. It requires
`IC_TESTKIT_POCKET_IC_URL`, borrows the upstream async client on the caller's Tokio
runtime and explicitly attempts instance deletion. It has no binary discovery,
download, server startup or private session wrapper. Creation/request waits are
bounded to 30 seconds; timeout/cancellation can leave remote effects. Upstream
async `drop` is best-effort, not proof of successful deletion.

`make m0-setup` explicitly prepares an optimized release-profile build of the
released `ic-testkit-server` 0.110.0 CLI
and authenticates the server assets. This retained executable is a development
bootstrap outside the Cargo driver graph; the lightweight library selects only
the upstream client, not Testkit's optional server manager.
Assets now live in `.tools/testkit-server`. Setup and every owned command get a
fresh directory under `target/evidence/testkit` for bounded process diagnostics;
the runner uses the launcher's maximum one-hour total command budget. Preparation
is still explicit; inherited server endpoints remain externally owned.

The 0.109.0 adoption passed focused Clippy, endpoint refusal/instance deletion,
both live-page Wasm witnesses, terminal interaction and the 90-second idle test.
Those six tests used Storage 0.74.1 artifacts. Their logs are in `target/evidence/testkit/adoption-109-{tests,console}.log`.

The 0.110.0 adoption (2026-11-17) keeps PocketIC 16.1.0 and the default-feature-free
client; its external launcher now selects Host Process 0.81.0. Both locked Cargo
graphs, snapshot admission and focused driver Clippy pass. The new launcher
passes endpoint refusal, instance deletion, the retained rollback/Keep-upgrade
witness and both terminal tests, including 90 seconds idle without keepalive.
`make db` rebuilt and installed the client; `db count Document` returned one row
and its query-call footer. Launcher receipts report no abandoned instances or
cleanup errors. Logs are `target/evidence/testkit/adoption-110-{tests,console,installed-cli}.log`;
setup, installation and the 63 passing release-adapter assertions use the same
prefix. Those launcher checks used the historical OP artifacts.

[The command runner](../scripts/with-pocketic.sh) checks that
prepared selection, then delegates process ownership to it. Existing Make probes
use this runner. Standalone `db` re-enters itself through the same runner before
creating its Tokio runtime; `--help` and argument refusal need no server. A supplied
endpoint is borrowed without starting or stopping its server. No path downloads
or provisions missing tools implicitly.

This adapts IC Tooling's `qualification/db/external-session.patch`, inspected at
local HEAD `89d94189e2efb0ea8e8a1eb39f56a816a68339fd` (patch SHA-256
`435e460d1d1bf3a3def18e8d394456d6abd3c3a7363205a3bff9cc968cb340c5`).
IC DB keeps its current callers, stop/upgrade/start policy and independent
rollback assertions. No IcyDB source or product storage representation changes.

Focused checks cover missing/malformed endpoint refusal with an unusable binary
selected, the nine rollback modes, endpoint/callback rollback, count reads across
upgrade and standalone CLI startup. All driver targets compile and pass Clippy.
Independent `/instances` observations show one instance marked `Deleted` while
the external server remains reachable, including after CLI setup refuses an
incompatible fixture (missing `decimal_batch`). Receipts are retained locally under
`target/evidence/external-server/`. This proves that run's deletion, not cleanup
after every panic/cancellation. The command owner shuts down its server on exit.

The 0.19 migration removed Testkit 0.106.0, Platform facade 0.33.7 and legacy Host
FS/Process dependencies. Old Types 0.20.4 remains only under frozen IcyDB; Runtime
0.22.5 remains under Storage/frozen IcyDB for raw IC imports. Historical performance
receipts keep their original graphs and are not claimed as remeasured here.

The pending 0.22.1 boundary correction selects Testkit 0.108.0's original-client
export. Its only selected dependency is PocketIC 16.1.0; managed-server, CLI and
held-reply features remain disabled. Existing endpoint admission, deadlines,
caller-owned runtime and explicit deletion remain unchanged. This changes the
dependency owner, not the simulator implementation or its transitive dependencies.
The historical 0.22.1 client used Agent 0.49.3 for signed inspection. Canic now owns the client and uses Platform for signed queries.
Focused requalification passes all 11 native CLI tests, missing/malformed endpoint
refusal, explicit instance release, the nine Wasm rollback modes and the real
Canic HTTP inspection/listing journey across upgrade and closed admission. All
driver targets pass strict Clippy; formatting and local documentation links pass.
The retained receipt is `target/evidence/external-server/testkit-0.108-boundary.log`.
These checks use the existing prepared simulator and producer Wasm artifacts;
they do not rebuild or requalify the complete product or establish Agent removal.

```sh
make m0-setup
make m0-rollback
make db
db count Entry
```

For direct Wasm-test invocations, prefix Cargo with
`bash scripts/with-pocketic.sh`; pure native contract tests need no simulator.

The 0.20 startup investigation timed the installed CLI separately from database
execution: 9.51 s to the session-ready banner versus 0.90–2.01 ms for three
subsequent counts in one session. The debug launcher spent 5.69 s authenticating
and decompressing the PocketIC bundle. The same 0.106.0 source built with the
release profile spent 0.37 s on that check and 0.75 s on check/start/stop with an
empty child command. Toko Tooling's release-launcher admission took another
2.84 s. Installed `db count Document` then took 4.09 s end to end versus the
earlier 9.52 s standalone invocation. These are local wall-clock startup observations, not IC performance
results. Both setup and command admission now select the release profile;
archive/executable/version validation and command-scoped ownership remain intact.

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

`db count Entry`, `db count Document` and `db count DecimalRow` read the existing
typed maintained counts (6, 1 and 3 after CLI seeding). Filtering remains
`db query Entry --select count --category 1` (3). The count command refuses
filters/limits and missing or unsupported entity names. The focused `records`
tests selected by `console_counts_` check CLI rendering/admission and direct
endpoint calls before/after upgrade, including the unseeded empty DecimalRow
entity and byte-for-byte unchanged backing. No product count implementation or
durable representation is added. After building the fixture:

```sh
make m2-cli-artifact
IC_DB_M1_RECORDS_WASM="$PWD/qualification/target/evidence/m2/console.wasm" \
bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml \
  -p ic-db-m0-rollback-driver --test records console_counts_
```

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
bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml \
  -p ic-db-m0-rollback-driver --test platform_upgrade
```

### Platform patch refresh (0.19)

The next published patch selection is Types 0.21.2 and Runtime/macros/guard
0.23.13, applied to both maintained workspaces. The source comparison against
0.21.1/0.23.12 finds unchanged Types, macros and guard source. Runtime adds raw
stable-memory/cycle operations; existing executor/endpoint code is unchanged.
IC DB does not adopt those raw operations: Storage still owns backing access.
The product lock now resolves `thiserror` 2.0.21 after Types relaxes its exact pin;
the independent qualification graph retains its supplier-constrained 2.0.18.

Focused checks cover the 23 Account/scalar-key tests and the two installed
endpoint admission/reply/callback rollback tests. Earlier cross-runtime artifact
hashes and maintenance receipts above remain historical evidence, not artifacts
rebuilt for this patch. No schema, durable-codec or stored-format change.

At this patch's initial qualification, older transitive selections included:

- Latest published Storage 0.74.1 requires Runtime `^0.22.0` with `ic0`; the
  frozen IcyDB graph also uses this raw-import generation. Revisit when Storage
  publishes support for the new raw API; do not replace its allocator here.
- Testkit 0.106.0 requires native Platform `^0.33.3` and Types `^0.20.4`, resolving
  to 0.33.7/0.20.4. New Testkit 0.107.1 removes `pic::PocketIcStartupConfig`,
  `PocketIcInstance` and `server_assets` used by our local driver/setup commands.
  It was not a drop-in update. The subsequent [external-session migration](#external-simulator-ownership-019)
  removes this Testkit/facade graph; its historical types did not cross into
  product codecs or the Wasm executor.

All direct Platform uses select the current published versions. The new facade
0.34.6 is not a direct dependency and is not added merely to change the lock graph.
The frozen IcyDB source/revision and sibling Canic integration are unchanged.

## Comparison with frozen IcyDB

The [November 18 three-engine performance matrix](#three-engine-performance-matrix-0234)
adds a current PocketIC rerun and matched native timings for SQLite.
The [indexed existence comparison](#indexed-existence-0237) covers the subsequent
fluent equality-prefix change.

The [installed restricted-delete comparison](#restricted-delete-phases-against-frozen-icydb) measures
open/session access, batch construction and commit on both engines.

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
| Stable Storage adoption | Storage 0.78.0 passes the installed count upgrade witness. | At 512 rows, IC DB uses fewer instructions for whole-entity and missing-category counts. IcyDB uses fewer for populated categories. [Measurements](#storage-078-count-comparison) | Other query shapes and larger populations remain open. |
| Blob queries (0.21) | Native exact-byte comparisons, indexed pages, DISTINCT, groups/extrema, unique changes and corrupt-key refusal; retained winner upgrade probe; synchronous/retained hex CLI | Frozen `6d421e3` rejects Blob index keys | New capability, no matched performance result. 0.22 adds retained CLI payload bounds. [Scope](#blob-query-contract-0210) |
| Added reachable entity size | 4,015 raw / 3,955 code bytes per additional entity in the 0.22.3 eight-shape fixture | Historical 4,451 raw bytes after shrinking, different build/workload | Near the roughly 4 KB ceiling. The 2 KB objective and a matched legacy growth result remain open. [Current receipt](#application-workload-and-wasm-growth-on-storage-078) |
| Broad query matrix | 195 cases over balanced/skewed 32/128/511-row populations, before/after upgrade; 4,680 checked executions | 1,908 successful matched pairs; IC DB cheaper in 1,796, legacy cheaper in 112 | Populated prefix counts up to 14.4× and selective grouped queries up to 2.6× legacy instructions. Case frequencies are not a production workload. [Receipt](#broad-query-matrix-019) |
| Selective grouping and prefix-count candidates | Explicit-index control plus isolated stable-prefix experiment | Index choice helps sparse cases but hurts dense ones; candidate metadata count beats scanned IC DB and legacy metadata in the selected fixture | No automatic planner change or prefix-count product API adopted; write/storage/qualification costs remain explicit. [Paths](#aggregate-access-path-control-019), [candidate](#maintained-prefix-count-candidate-019) |
| Fluent query shapes and application mapping | Six first-page cases now matched over 32 rows; separate controlled shape/map growth builds | 64.8–73.7% fewer instructions in the selected successful queries | Pagination contracts differ; broader shapes, maps and entity growth remain unmatched. [Receipt](#matched-fluent-first-pages-018) |
| Complete sparse ID listing | 128 rows, two categories, default-order IC DB and explicit-index control | Automatic IC DB used 51.7–55.1% fewer handler instructions than legacy after upgrade and readiness | Two-row pages on fixed input with last-ID boundaries. [0.23.13 comparison](#automatic-equality-listings-02313) |
| DISTINCT with a fixed compound unique-key component | IC DB pages projected subjects in subject or ID order with one candidate per call | Legacy typed route reads agree in subject order and refuse ID order on this fixture | Result and admission comparison. [Route comparison](#compound-equality-lookup-02312) |
| Complete sorting at 511 rows | Mixed-order limit 32: 13.86M instructions; two-value DISTINCT: 5.77M | Legacy public typed sort refuses materialization in the 32-row comparison; no matched successful 511-row run | Refusal is not a speed comparison. Rich-key/payload costs and a suitable legacy execution lane remain open. [Receipt](#bounded-sorting-execution-envelope-018) |
| Exact entity/prefix counts | Automatic whole-entity metadata, covered equality-prefix key scans, or row execution. Explicit owner validation remains available. | IC DB metadata count uses 59.3% fewer instructions at 512 rows; legacy remains cheaper for populated prefix counts | [Automatic count measurements](#automatic-prefix-counts-0235) cover the default fluent call. [Prefix comparison](#prefix-count-paths-0234), [adoption](#maintained-count-adoption-019), [scan profile](#count-cost-decomposition-019) |
| Grouping, sums/averages/extrema and retained aggregation | 32 count/sum/min/max cases over 32 rows; IC DB-only 32-group finalization, text winners, retained Decimal rounding, checkpoint-byte probes and cleanup of 16 near-limit jobs | Four-group queries use 74.0–76.0% fewer instructions | Rich types, averages, retained jobs, state ceilings and contention costs remain unmatched. [Comparison](#matched-grouped-aggregates-019), [jobs](#product-retained-aggregates), [shared capacity](#shared-retained-job-capacity-019), [envelope](#retained-aggregate-envelope-020), [text](#retained-variable-sized-values-0210), [Decimal](#retained-decimal-averages-0210) |
| Schema migration and recovery | Copy, validation, cancellation, publication and cleanup witnesses | No matched migration-cost comparison | Existing entities and keys must survive; added entities start empty. Broader envelopes and allocation lifetime remain open. [Receipt](#identified-maintenance-and-the-local-console) |
| Mixed write/query histories | Storage 0.78.0 rerun checks 153 attempted batches, sparse listing, jobs, six upgrades and interrupted maintenance on the same records. [Receipt](#application-workload-and-wasm-growth-on-storage-078) | Retains legacy count and final-batch uniqueness invariants; no matched mixed-history run | Small scalar/reference fixture, not production contention or all type combinations. [Receipt](#mixed-history-qualification-019) |
| Canic endpoints and full applications | Canic-owned typed adapter: caller/frame refusal, sparse pages, upgrades, maintenance and reply rollback | No matched integration benchmark | Canic 0.220.0 witness has a separate [endpoint size comparison](#application-workload-and-wasm-growth-on-storage-078). Native inspection checked through HTTP on the committed inspection contract. The fixed application listing also preserves sparse pages and cursor refusal after upgrade; broader application support is not claimed. [Receipt](#canic-typed-application-integration-019) |

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
| Endpoints | Generated actor wiring and facade surfaces | Canic owns product endpoints/guards; standalone library use remains possible. Canic owns remote inspection and application listing endpoints; IC DB CLI query demos run locally |
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

#### Stored type authoring audit (0.22.3)

After the tagged audit, IC DB added `#[list]` and primitive-backed `#[newtype]`.
The remaining declaration gaps are `#[tuple]` and standalone `Unit`. The local
`db schema ListRow` fixture shows Samples (a bounded list) and Sample (a Nat16
newtype). The table below describes the original tag.

Of the 25 primitive spellings in frozen IcyDB, IC DB accepts 24. The difference
is `prim = "Unit"`. Payload-free enum variants are supported. Both engines
have Timestamp, Date, Duration, Decimal, Float32/64, Int and Nat at widths
8/16/32/64/128, U256, Ulid, Principal, Subaccount, Account, Bool, Text and
Blob. This compares stored declarations only. Operation support, bounds and
stored bytes have separate contracts. [Legacy primitive registry](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/crates/icydb-schema/src/scalar_macros.rs)
and [IC DB field parser](../crates/ic-db-macros/src/field.rs).

| Missing legacy declaration | Current capability and consequence |
| --- | --- |
| `#[list]` | A field can use bounded `many` with scalar or named items. The bound and item type belong to that use site. There is no reusable named list definition for `item(is = "...")`. |
| `#[newtype]` | The underlying value can be stored. A Rust type alias provides another spelling and a one-field record creates a record contract. Neither supplies a distinct schema type that forwards its inner scalar's operations. |
| `#[tuple]` | Named records use canonical field-name order. The design retains positional tuples as later work. A multi-field projection or compound index does not declare a reusable positional stored type. |
| `prim = "Unit"` | No standalone unit field in the parser or stored field descriptor. No permanent exclusion or specific rejection rationale was found. |

The five delivered declaration attributes are entity, enum_, map, record and
set. The implemented named-value graph has record, enum, map and set bodies.
The design documents the initial field-list choice and tuple deferral. It gives
no permanent exclusion for named lists or newtypes. Legacy canister/store and
normalizer/validator declarations concern host integration or policy. The
legacy base domain library also bundled reusable definitions such as Seconds,
Percent, Sha256 and Url. These are a separate convenience-library surface.
[Legacy model exports](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/crates/icydb-model/src/lib.rs)
· [Legacy domain library](https://github.com/dragginzgame/icydb-legacy/tree/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/crates/icydb-model/src/base/types)
· [Current named types](../crates/ic-db/src/types.rs).

## Standalone endpoint hooks

The first M2 fluent-read probe is selected separately:

```sh
make m2-queries
make m2-jobs
make m2-fluent-comparison
make m2-keyset-comparison
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
absence, contradictory prefixes and empty pages. The native residual-filter witness visits 40 indexed classes in both directions
with one candidate per page and transports each cursor between calls. The Wasm probe resumes filtered
and unfiltered DISTINCT cursors after upgrade and scans duplicate pages to
completion without changing backing bytes. There is no durable seen-set.

Retained DISTINCT enumeration uses the shared job checkpoint and completed-page
machinery. The native fixture scans 521 rows once, reopens during accumulation,
then returns 255 scattered values, including absence, in canonical key order.
Separate witnesses exhaust the one-byte key count and the 16 KiB frame, check
backing preservation, and exercise invalidation, cancellation and a smaller retry.
The installed console fixture enumerates 40 values from 80 rows and upgrades
after returning the first 33. Its next cursor resumes the same completed job.
Run `make m2-cli` for the installed path or try
`db aggregate Entry --select distinct:amount --work 2 --page-size 2` locally.

#### Ordered DISTINCT release compatibility

The release-boundary probe installs a retained-console Wasm built from tag
`v0.24.4`, starts two default-order DISTINCT jobs, and upgrades the same
canister to the 0.25.0 candidate. One job has scanned two of six rows. The
other has returned two keys and issued a page cursor. After upgrade the first
returns all six canonical keys and the saved cursor returns the final four.
Each call uses its fixture version's Wire request shape.

To repeat the probe, set `IC_DB_V0244_CONSOLE_WASM` to the retained-console Wasm
built from that tag and run:

```sh
make m2-cli-artifact
IC_DB_M1_RECORDS_WASM="$PWD/qualification/target/evidence/m2/console.wasm" \
  bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml \
  -p ic-db-m0-rollback-driver --test console_aggregates \
  canonical_distinct_resumes_running_and_completed_v0244_jobs -- --ignored --exact
```

#### Retained DISTINCT cycle measurements

The 0.25.0 fixture measures the second single-key page of a completed
first-occurrence job. Each input row has a distinct Nat64 amount equal to its
ID. Every measured update resumes at offset one, returns amount 2 and leaves
the job sequence and examined-row count unchanged. Two fresh PocketIC runs
of `make m2-cli`'s `completed_distinct_single_key_page_cost_by_cardinality`
case gave the same baseline balance deltas. The second column of measurements
uses single-pass comparison-key decoding during checkpoint restore.

| Saved keys | Baseline update cycles | Single-pass update cycles |
| ---: | ---: | ---: |
| 8 | 7,915,497 | 7,910,381 |
| 32 | 8,090,254 | 8,069,754 |
| 128 | 8,952,064 | 8,870,028 |
| 255 | 10,427,427 | 10,263,984 |

These totals include endpoint and storage work. Each call validates the whole
saved checkpoint and reconstructs encounter order; the balance delta does
not isolate the cost of either operation. Wider projections can reach the
16 KiB frame bound sooner.

The same fixture also measures the preceding update, which scans all rows,
saves the completed checkpoint and returns the first key. Each row introduces
a new group. With a single tree lookup for group insertion, the matched
PocketIC updates cost:

| New keys | Before (cycles) | Single lookup (cycles) |
| ---: | ---: | ---: |
| 8 | 8,218,258 | 8,210,097 |
| 32 | 8,778,108 | 8,704,299 |
| 128 | 11,278,371 | 10,785,732 |
| 255 | 15,066,771 | 13,923,490 |

Job creation and fixture population occur before both measurements.
The repeated-key fixture alternates two categories across 256 rows. Each category
contains 32 distinct Nat64 amounts repeated four times. Both queries examine 128
rows per step. The second step restores the checkpoint, sees only previously
seen amounts and returns both groups. Each group has 128 rows and 32 distinct
amounts. The measured updates include transport and checkpoint work.

| Reducer | Step | Recount key bytes (cycles) | Track key bytes on insertion (cycles) | Single set lookup (cycles) |
| --- | --- | ---: | ---: | ---: |
| Group count (control) | First | 10,023,258 | 10,019,418 | 10,019,418 |
| Group count (control) | Second | 10,025,218 | 10,021,318 | 10,021,318 |
| Group distinct count | First | 11,397,453 | 10,693,989 | 10,550,243 |
| Group distinct count | Second | 11,553,263 | 10,758,751 | 10,759,775 |

Run `make m2-cli` to reproduce the installed fixture. The byte total uses the
allocated capacities of the live keys and is recomputed when restoring them.

The local 0.24.3 retained-console builds before and after direct DISTINCT
page selection used the same 80-row fixture, 17-candidate steps and 11-row
pages. PocketIC balance deltas measure the complete update, including transport
and checkpoint handling. These are individual matched observations.

| Completed output | Prior cycles | Direct selection cycles |
| --- | ---: | ---: |
| First 11 keys, including the last input step | 8,653,632 | 8,614,399 |
| Next 11 keys, warm | 8,345,337 | 8,317,566 |
| Following 11 keys, warm | 8,343,133 | 8,332,230 |
| Final 7 keys after upgrade | 9,565,694 | 9,561,662 |

Total uncompressed fixture Wasm went from 1,390,830 to 1,387,427 bytes;
its code section went from 1,330,462 to 1,327,061 bytes. The page reader still
restores and validates the full bounded checkpoint on every call. It takes
the selected keys directly from their canonical order and shares the selection
code across typed outputs. Instruction counts were not measured here.

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

All seven canister hosts use Platform's explicit `guard`, `decode`, `encode`,
`max_bytes` and `on_error` hooks with the [fixed fixture codecs](ic-db-qualification-transport/src/lib.rs), shared
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
Its [shared declarations](ic-db-m1-records-schema/src/model.rs) are a private qualification
crate whose [own build script](ic-db-m1-records-schema/build.rs) generates the compiled
schema imported by the Wasm canister. Host-only `ctor`
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

The retired `make m1-envelope-comparison` selected the baseline's `envelope`
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
common workload. The comparison remains historical evidence. Use `make m1-write-envelope` for current IC DB write qualification.

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

## Single-store width on PocketIC

`make m1-store-width` builds the temporary game schema with the schema owner's normal
build script, installs its consumer Wasm, and runs two updates separated by an upgrade
to the same executable. The first inserts a spell and its referenced ability in one
batch, dependent first. The second reopens the same allocation, reads the last
entity, verifies that deleting its referenced ability refuses intact, then deletes
both together. All intervening entities have just a primary key and no rows.

Measured 2026-11-20 with PocketIC 16.1.0. The 32-entity case used the original
32-entity admission limit. The same build refused 33 during schema generation with `InvalidSchema`.
For the other three measurements, a disposable source copy changed only the
schema admission comparison from `entities.len() > 32` to `entities.len() > 500`.
The supported cap is now 128. The 500-entity column records the earlier experiment. Exercise a selected width with
`make m1-store-width STORE_WIDTH=72` (also accepts 32, 33 and 128). This probe is
independent of the historical entity-growth benchmark.

| IC instructions per operation | 32 | 33 | 72 | 500 |
|---|---:|---:|---:|---:|
| Cold compiled schema admission | 254,023 | 264,846 | 539,104 | 3,688,806 |
| Database creation | 4,854,927 | 4,967,513 | 11,705,426 | 109,347,032 |
| Two-row insert commit | 1,322,112 | 1,321,215 | 1,476,681 | 1,866,360 |
| Post-upgrade database open | 158,540 | 158,540 | 178,150 | 195,150 |
| Late-entity primary lookup | 71,805 | 71,827 | 87,463 | 218,154 |
| Restricted delete refusal | 306,590 | 306,635 | 368,321 | 551,766 |
| Two-row delete commit | 1,386,767 | 1,386,879 | 1,588,653 | 2,170,807 |

Whole-update cycles (create and insert / reopen and delete) were
14,048,058 / 9,676,069 at 32, 14,171,274 / 9,687,093 at 33,
21,339,080 / 10,299,478 at 72 and 122,520,346 / 14,494,760 at 500.
Admission was cold on both calls. Creation and open exclude schema admission,
host bootstrap and lease acquisition. Commit timings exclude caller-side batch
construction. Each cell is a single measured execution on a sparsely populated
schema. The lookup includes typed admission and storage access. An unchanged-code
upgrade exercises reopen. The experimental 500-entity build reproduced the recorded
instruction and cycle counts. A separate native copy migrated 499 entities to
a 500-entity target, reopening persisted progress at positions 31, 32, 255,
256 and 498 before validating and publishing. A producer declaring 501 distinct
entities refused during schema generation in that experiment.

At the supported 128-entity cap, `make m1-store-width STORE_WIDTH=128` passed
on PocketIC. The create-and-insert update used 33,894,476 cycles and the
post-upgrade update used 11,165,683 cycles. A separate native migration with
128 source and target entities resumed at positions 31, 32, 126 and 127, then
validated and published. A producer with 129 distinct entities failed schema admission.

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
uses 8,397,483 / 15,578,287. These measurements precede bounded top-k retention for sorted DISTINCT.
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
make m2-query-matrix
```

Fetching requires read access to the private `dragginzgame/icydb-legacy` repository.
The repository rename preserves the frozen source revision. The benchmark runs
with the prepared PocketIC server and locked dependencies. It uses Git revision
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

The scalar write comparison was retired. The measurements below describe the historical workload.

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

On 2026-11-16, the then-current `make m1-records-comparison` was rerun against current IC DB and frozen
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

### Application workload and Wasm growth on Storage 0.78

The application run uses the 0.22.3 product (`4dccadc`) and its locked Storage
0.78.0 on PocketIC. One database contains parents with unique codes, items
indexed by parent and score, and links to items. An independent map model checks
153 attempted batches and 612 snapshots across six upgrades. The sparse
listing runs while an exact aggregate is in progress. With a one-candidate
budget, its four calls return zero, zero, one and zero rows. The first two
empty pages carry continuations. The fourth establishes exhaustion. The calls
take 468,992, 452,268, 480,160 and 381,621 instructions including opening.

An unrelated write, a refused write, a no-op and an upgrade preserve aggregate
progress. A committed input change invalidates it. A fresh run completes with
12 and retains that answer across another insert. Later upgrades interrupt copy
and validation of these same records. Reads through the published target agree
with the transformed model, including during retired-source cleanup.
Three-entity inserts cost 2.01–2.30M instructions and 9.05–9.34M cycles.
The 13 source read-modify-write samples cost 1.49–1.97M instructions and
8.51–15.38M cycles. This is a small deterministic application history.
A matched legacy lifecycle run and production contention remain open.

**Controlled growth.** Each installed variant checks its reachable operations
against independent expected rows, including after upgrade. The direct builds
use point, primary page and secondary prefix operations. The fluent builds
execute eight query shapes per entity through the same endpoint. Numbers below
are changes in uncompressed bytes at the release build profile.

| Single change | Added Wasm bytes | Added code bytes |
| --- | ---: | ---: |
| Direct entities, 1 to 8, retained host | 17,696 | 17,353 |
| Direct entities, 8 to 32, retained host | 64,162 | 62,999 |
| Fluent entities, 1 to 8, eight shapes each | 28,103 | 27,688 |
| One entity, add a second two-field index | 157 | 109 |
| One entity, make the first fluent query reachable | 40,133 | 38,047 |
| One entity, expand from one to eight fluent shapes | 1,309 | 1,281 |
| Add eight application mapping call sites | 1,791 | 1,791 |

The fluent slope is 4,015 raw bytes per added entity. Direct operations alone
add 2,528 per entity from one to eight and 2,673 from eight to 32. The 2 KB
objective remains open. Most of the first fluent query's cost is shared across
subsequent shapes. Multiplying typed entities still matters even at fixed shape
count. The seven-entity differential includes 10,407 extra bytes when fluent
operations are reachable. These differences measure reachability and do not
identify individual functions.

**Where the repeated code lives.** Symbol-bearing builds of all four
direct/fluent, one/eight-entity variants match their respective measured Wasm
byte for byte across all non-custom sections. In the eight-entity fluent build,
there is one 20,008-byte `fetch_rows` body and one named entity-page decoder.
There are eight 202-byte typed `fetch` wrappers and eight 128-byte typed
`revision` wrappers. Within each family only two bytes of the embedded schema
address differ. Seven additional copies of both families occupy 2,310 function-body
bytes in that artifact. This measures repetition within the eight-entity build.
Inlining and layout change between builds, so it is not a removable-byte forecast.

At fixed eight entities, enabling the eight fluent shapes grows the fixture's
dispatch body from 19,491 to 28,022 bytes. Dispatch includes request construction,
typed calls and inlined library work. The earlier [schema-pointer experiment](#deeper-specialization-investigation-018)
merged the fetch wrappers at a measured execution cost. Keep the typed boundary
and shared executor. The current audit changed no engine code. Names, per-function
sizes and the section comparison are in the application receipt's `attribution/` directory.

The extra index adds little Wasm but increases read-plus-replace work from
1,210,588 to 1,719,521 instructions excluding opening. Both index values change.
The full two-call balance cost increases from 15,533,354 to 16,080,971 cycles.
For the first fluent shape at eight entities, the four opening stages sum to
173,740 instructions warm and 535,288 after upgrade. Execution itself uses
230,727 and 232,015 respectively. Cold schema construction and ordinary
execution are separate costs in the retained-host fixture.

**Canic endpoint composition.** Four release Wasm builds use the committed Canic
0.220.0 database witness at `d652c55`, pointing its DB and Storage dependencies
at IC DB 0.22.3 and Storage 0.78.0 in an isolated source copy. Schema, protocol
types, owner guard and build profile are fixed. The table counts application
endpoints in addition to the common init export.

| Reachable application endpoints | Wasm bytes | Code-section bytes |
| --- | ---: | ---: |
| One listing | 481,002 | 369,977 |
| Eight listings sharing the handler, each with its own frame ID | 492,544 | 375,063 |
| Listing and write | 591,700 | 454,770 |
| Listing, write and inspection | 715,588 | 539,112 |

The seven additional listing exports added 11,542 bytes total, including 5,086
code bytes. The write handler added 110,698 bytes over listing alone. Inspection
added another 123,888 bytes. Those last two increments include the newly reachable
operations and their transport types. The eight-listing build is an experimental
channel-count control made by repeating Canic's query mount with distinct export
names and method IDs 9001–9007. The released adapter supplies the three standard
endpoints. Export names and Wasm validity were checked for each build. Endpoint
latency, cycles and a matched legacy host remain unmeasured in this experiment.

To repeat the application, entity and query checks with their recorded lockfile,
use the qualification changes against `v0.22.3` and a prepared PocketIC server.

```sh
make m4-lifecycle
make m1-entity-growth
make m2-query-growth
```

Local receipts are in `target/evidence/application-growth-0223/` and
`target/evidence/canic-growth-0223/`. They retain the build inputs, Wasm and
logs. The Canic receipt includes its pinned source copy and variant build script.
The earlier growth and mixed-history receipts are preserved under their
`-before-0223` names.

### Storage 0.78 count comparison

The count workload ran on IC DB 0.22.2 (`97e49f2`) with Storage 0.78.0 and frozen IcyDB `6d421e3` on PocketIC. Both returned the expected counts at 32, 128, 256 and 512 rows and across upgrades. The table shows query instructions after writes settled at 512 rows.

| Count | Result | IC DB | Frozen IcyDB |
| --- | ---: | ---: | ---: |
| Whole entity | 512 | 625,558 | 1,417,729 |
| Category 0 | 129 | 15,930,527 | 1,507,177 |
| Category 3 | 127 | 16,639,262 | 1,499,808 |
| Missing category | 0 | 826,300 | 1,515,365 |

IC DB reads maintained metadata for the whole-entity count. For a category it traverses an index and checks the matching rows. IcyDB maintains category counts during writes. A separate IC DB profile on category 0 measured 531,547 instructions to open the database and 15,466,901 to execute the count. Traversing the raw index cost 1,546,322 instructions. Adding owner-row fetches to that control raised it to 13,356,926. Fetching matching rows accounts for the largest measured increment.

The explicit fluent `count_index_entries()` terminal subsequently ran in the same count-profile canister as the validated count. With 129 matches at 512 rows, execution took 1,226,592 instructions versus 15,467,227. Opening added 531,555 to each, giving 1,758,147 versus 15,998,782 overall. Both returned 129 before and after upgrade. The native corruption witness counts a deliberately dangling entry and verifies `count_validated()` refuses it. Reproduce using `IC_DB_COUNT_EVIDENCE_DIR="$PWD/qualification/target/evidence/index-entry-count-078" make m2-count-profile` (modes 5 and 1).

The matched Wasm artifacts were 310,716 bytes (285,487 code) for IC DB and 3,115,697 bytes (3,006,452 code) for IcyDB. The two evidence directories contain the source revision and diff, lockfile, Wasm, hashes and `results.log`. To reproduce from the repository root:

```sh
IC_DB_COUNT_EVIDENCE_DIR="$PWD/qualification/target/evidence/count-storage-078" make m2-count-comparison
IC_DB_COUNT_EVIDENCE_DIR="$PWD/qualification/target/evidence/count-profile-storage-078" make m2-count-profile
```

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
generic remote CLI contract. Canic handles remote inspection.

### Maintained prefix-count candidate (0.19)

Rerun on Storage 0.78.0 with the 0.22.2 engine: at 512 rows the control's validated count of category 0 (129 matches) took 15,930,527 instructions. The candidate's explicit metadata lookup took 803,157. Mean insert cost across the first 32 inserts rose from 934,343 to 1,118,875 instructions. The read-modify-write that moved one row between categories rose from 1,219,344 to 1,695,795. Replacing a row within its category took 1,214,436 and 1,216,687 respectively. Reachable Wasm grew by 6,928 bytes, including 5,777 code bytes. The control and candidate use separate Wasm builds. Ordinary fluent counts in both builds still check each owner's row.

Reproduce with `IC_DB_PREFIX_EVIDENCE_DIR="$PWD/qualification/target/evidence/prefix-count-078" make m2-prefix-candidate`. The candidate passed its native and count journeys. Its fault sweep hit the probe’s maximum write count before one replacement completed, so this run does not qualify candidate rollback at every write. Product index-entry counting uses no maintained prefix records. The receipt contains `native.log`, `control.log`, `candidate.log`, the exact applied patch, and source and artifact identities.

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

### Skewed application workload (0.23.10)

`make m4-lifecycle` runs the existing Canic-mounted application on two parents,
128 items and 16 links. Initially 112 items share one parent. The listing scans
that parent's index in ID order with a residual minimum score of 10 and a
two-row output limit. In each phase, three independent listings start at the
beginning with work budgets of 1, 8 and 32 candidates. The application defaults
to eight when the caller omits the budget. The qualification checks that a
sparse page actually examines eight candidates and compares every returned ID
with the independent map. The table records the original explicit-budget run.
Totals include the final page.

| Listing phase | Work per call | Calls | Candidates | Matches | Empty continued pages | Total handler instructions | Peak per call |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Seeded | 1 | 113 | 112 | 9 | 103 | 62,001,039 | 637,262 |
| Seeded | 8 | 15 | 112 | 9 | 5 | 18,423,660 | 1,380,052 |
| Seeded | 32 | 5 | 112 | 9 | 0 | 13,989,303 | 3,482,578 |
| After write | 1 | 114 | 113 | 10 | 103 | 62,679,266 | 644,716 |
| After write | 8 | 15 | 113 | 10 | 5 | 18,550,901 | 1,380,052 |
| After write | 32 | 6 | 113 | 10 | 0 | 14,632,942 | 3,482,578 |
| After upgrade, before an update | 1 | 114 | 113 | 10 | 103 | 96,276,771 | 940,525 |
| After upgrade, before an update | 8 | 15 | 113 | 10 | 5 | 22,983,435 | 1,674,329 |
| After upgrade, before an update | 32 | 6 | 113 | 10 | 0 | 16,405,118 | 3,781,481 |
| After first post-upgrade update | 1 | 114 | 113 | 10 | 103 | 62,322,808 | 643,285 |
| After first post-upgrade update | 8 | 15 | 113 | 10 | 5 | 18,510,147 | 1,377,489 |
| After first post-upgrade update | 32 | 6 | 113 | 10 | 0 | 14,607,176 | 3,483,066 |

The move used 2,097,792 handler instructions and reduced the canister balance
by 9,113,932 cycles. A count already in progress reported invalidation on its
next advance (837,301 instructions). Its replacement processed two candidates
per advance and survived the upgrade. It returned exactly 128 after 65 advances
and 62,630,893 handler instructions summed across those advances, excluding
start and cancellation. No further item edits occurred during the replacement.

At budget 32, the post-write listing took six calls and each examined candidate
was counted once across the listing. Before the first post-upgrade update,
the six calls cost 16.41M instructions in total, versus 96.28M at budget 1.
The peak per call rose from 0.94M to 3.78M instructions. Budget 8 reached
22.98M total with a 1.67M peak. The absolute post-upgrade premium over the
post-write listing fell from 33.60M instructions at budget 1 to 1.77M at
budget 32, about 0.295M extra per query at either budget. After a successful
count advance, listings returned to their earlier cost range. This PocketIC sample uses the qualification
mount and measures instructions inside handlers. The retained run log is
`qualification/target/evidence/mixed-lifecycle/run.log`.

### Mixed workload rerun (0.23.21 development)

On November 19, `make m4-lifecycle` passed against `c5a8fc3` plus the
unreleased DISTINCT changes with Storage 0.78.7. The mixed host Wasm SHA-256
was `d3f3e8273c118c7c0087a021e9cbd1ec8dcca091467c044592bed2c30390a5b6`.
The independent model checked 616 complete read observations, including
grouped counts, over the write, upgrade and migration history.

| Operation | Observed result | Handler instructions |
| --- | --- | ---: |
| Seeded indexed listing, eight candidates per call | 112 examined, nine matches, 15 calls, five empty pages with continuation | 18,489,254 total |
| Source read-modify-replace, 13 samples | 8,206,365–8,562,925 cycles debited per update | 1,192,289–1,548,849 per update |
| Move item during an unfinished count | Next advance reported invalidation | 2,101,295 for the move, 838,918 for the advance |
| Fresh count across upgrade | 128 items in 65 advances | 62,718,929 across advances |

The separate `make m2-contention` run covered grouped jobs. An input write
invalidated unfinished groups for that entity. A completed grouped sum retained
its answer through the write and upgrade. In the 96-row scalar sum fixture,
the four fixed write schedules allowed zero completions at 12 candidates per
advance and two at 48. These schedules exercise the tradeoff between work per
message and opportunities for invalidation. They do not estimate a production
completion rate. Neither workload measures a DISTINCT speedup.

### Compound equality lookup (0.23.12)

The opt-in native test compares IC DB with frozen IcyDB at `6d421e3`.
A route has a unique key on two principals. Each principal also occurs in
another route. Both engines receive equality filters in both orders and must
return the expected ID or a miss under default and descending ID order,
including when a further ID filter accepts
or rejects the owner. IcyDB uses its typed query builder in
`crates/icydb/src/db/query/typed.rs` with `.limit(1).execute_live_page(None)`.
IC DB checks both `.first()` and an ID-only projection with a one-candidate
work limit. This measures result agreement. IC instruction costs remain unmeasured.

The 0.23.21 development run also fixes only the issuer and projects DISTINCT
subjects in subject order. The two-match, one-match and missing issuers exercise
continuation and exhaustion on the same records. Legacy's public typed page
returns whole routes. The comparison reads their subjects directly because
the declared unique pair already guarantees one occurrence per issuer.
For issuer 1 the two orders deliberately disagree. Subject order returns
subjects 2 then 3 on both engines. In ascending ID order IC DB returns subject
3 from ID 3, advances past another issuer at ID 6 with an empty continued page,
then returns subject 2 from ID 9. The same test checks that legacy refuses
that order with `QUERY_READ_SORT_REQUIRES_MATERIALIZATION` (174).
Frozen IcyDB tests adjacent and replay-based projected DISTINCT internally in
`crates/icydb-core/src/db/session/write/mixed_relation_batch_tests/mod.rs`
using `distinct_for_internal_execution()`. That internal entry point is outside
the SQL-disabled public typed benchmark.

```sh
cargo test --locked --manifest-path qualification/Cargo.toml -p ic-db-icydb-baseline-canister --features unique-lookup --test native_matrix compound_equality_lookup -- --ignored
```

### Automatic equality listings (0.23.13)

On the 128-row fixture below, the automatic IC DB query declares category equality
and the amount threshold. Its default order is ascending ID. The control on the
same Wasm supplies the category through `using_index("category,id", ...)`.
Frozen IcyDB `6d421e3` uses fluent filters and requests ascending ID explicitly.
The two IC DB routes include their respective predicate and validation costs.

| Category | Qualifying / category rows | Calls through exhaustion | IC DB automatic | IC DB explicit | IcyDB |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 10 / 113 | 6 | 14,577,642 | 14,407,193 | 30,175,770 |
| 2 | 2 / 15 | 2 | 2,557,372 | 2,521,915 | 5,693,865 |

Values are total handler instructions after upgrade and readiness. Each call
opens the database and returns up to two full rows. Both traversals include
the final empty call. The driver checks each tuple against its independent
oracle and verifies stable memory before and after each traversal. These
measurements use caller-supplied last-ID boundaries. Engine cursor resumption
is covered separately by the query suite. The automatic IC DB totals were
1.18% and 1.41% above the corresponding explicit totals.
[Seeded and post-upgrade page costs](measurements/0.23.13-keyset-listing.csv)

Reproduce with `make m2-keyset-comparison`. The runner retains artifacts and
source inputs in `qualification/target/evidence/keyset-comparison-02313/`.
Wasm SHA-256 was `a49b7b563df61d5950132d409b19648166d0140795cb15b6ca9b28a94748890e`
for IC DB and `08ce7449d2a9110b0a087afb328e68d94de7da3429856427e092e48bf6541dfa`
for frozen IcyDB's comparison host.

### Matched complete keyset listing (0.23.10)

`make m2-keyset-comparison` seeds both scalar Wasm hosts with the 128 item
tuples from the [skewed application](#skewed-application-workload-02310)
after item 128 moved. Both use the public fluent interface to select category
1 and amount at least 10 in ascending ID order. Each call asks for two full
rows. Following a full page, the caller adds `id > last_id`. Five pages each
return two tuples and a sixth returns empty. Every tuple and its position
agrees with an independent oracle, and the driver checks stable memory before
and after both traversals.

| PocketIC handler instructions | IC DB | Frozen IcyDB `6d421e3` |
| --- | ---: | ---: |
| Complete listing after seeding | 14,574,185 | 30,355,082 |
| Explicit readiness after upgrade | 438,073 (1 update) | 5,844,846 (2 updates) |
| Complete listing after upgrade and readiness | 14,574,182 | 30,175,705 |
| Most expensive page after readiness | 3,537,019 | 6,710,107 |

Frozen IcyDB's typed scalar limit applies across a cursor traversal. Starting
each request at the next ID gives both hosts the same output contract on fixed
input. IC DB allows up to 512 candidates in each request. Legacy uses public
read admission. The original Canic listing permits eight candidates, can return
empty continued pages, and measures the full application handler. This scalar
comparison measures query construction, execution and tuple conversion inside
each host, excluding transport. Each page uses the last emitted ID as its
boundary. Native cursor formats are outside this comparison. The isolated runner retains the feature graphs, Wasm
artifacts, source digests and individual page costs in
`qualification/target/evidence/keyset-comparison/`.

#### Membership changes between pages

The same runner takes a first page ending at ID 21, then moves IDs 8 and 32
into the matching category and ID 43 out of it using three read-modify-write
calls. After an unchanged executable upgrade and readiness, the caller
continues from ID 21. Both engines return the new match at 32 and omit 43.
ID 8 appears only when the caller starts again. The original traversal yields
ten rows and the fresh traversal yields eleven. Both sequences are checked
against independently maintained tuples on immutable primary-key ordering.

| Stage, handler instructions | IC DB | Frozen IcyDB |
| --- | ---: | ---: |
| First page before edits | 2,366,289 | 6,710,107 |
| Three read-modify-write calls | 4,511,383 | 8,955,810 |
| Explicit post-write settling in this harness | 0 | 12,794,476 |
| Readiness after upgrade | 435,851 | 5,844,846 |
| Remaining pages through exhaustion | 11,972,507 | 23,446,337 |
| Sum through exhaustion | 19,286,030 | 57,751,576 |
| Separate fresh traversal after edits | 14,415,124 | 32,477,547 |

The totals sum instrumented handler work. They exclude executable installation,
upgrade execution, transport and the independent fresh read. The legacy harness
drives startup after each write. The settling figure is the cost of that
schedule, and does not establish the minimum cost for a different schedule.

### Opening costs through the Canic mount (0.23.7)

The historical run used
Canic 0.223.1 at `bbc13c36afc88f8dcd09072c1cbfd4478943fffd` and workspace
IC DB. The signed-HTTP producer used its own locked dependencies. Both paths then used Wire 0.25.0. Current runs select the released Canic dependency in the qualification manifest.

The database contains four parents, twelve items and twelve links. The indexed
query returns the same item and cursor after examining one candidate. The batch
replaces an item with its existing values. All calls together preserve the
complete stable backing. Each row is one PocketIC observation.

| Application call | After seeding | First call of its kind after upgrade | After the first update |
| --- | ---: | ---: | ---: |
| Indexed page, handler instructions | 469,812 | 759,195 | 465,649 |
| Identical replacement, handler instructions | 425,347 | 717,369 | 423,709 |
| Identical replacement, canister balance decrease in cycles | 7,439,318 | 7,731,959 | 7,437,715 |

A second query before any update also used 759,195 instructions. Both queries
initialized the heap for their own execution. The first successful update
retained the initialized host for later calls. Handler instructions include
opening and database work. They exclude Canic guard and frame processing.
The balance deltas measure complete update calls and can include periodic charges.
These results cover reopening a populated database after upgrade.

The retained-count comparison uses two installations seeded by the same batches.
Both start a count and process two items. One installation then upgrades with
the same Wasm. At the same simulated time, both receive the same handle and
sequence 1 with a two-candidate budget. The driver checks byte-identical stable
memory in the two installations both before and after this measured advance.
Both return sequence 2, still running, and complete with 12 items at sequence 7.

| Retained-count advance | Warm installation | First update after upgrade | Increase |
| --- | ---: | ---: | ---: |
| Handler instructions | 890,112 | 1,020,156 | 130,044 (14.6%) |
| Canister balance decrease in cycles | 7,962,317 | 8,092,571 | 130,254 (1.6%) |

The cycle sample starts after the upgrade and a simulator tick. It covers the
complete update, including transport. The instruction counter covers the
handler, including opening the database and job store and saving progress.
These totals establish the first-advance premium for this fixture. They do
not isolate the cost of either store.

### Mixed-history qualification (0.19)

Run `make m4-lifecycle` after the existing `make m0-setup`. This selects only the
`mixed-lifecycle` host feature and the mixed-history and paired-count driver tests; it does not run the full
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

In 0.23.6 the three-entity fixture mounts Canic 0.219.0’s typed endpoints over
the workspace IC DB engine. The independent driver sends all ordinary batches
and full snapshot queries through the mount. The synthetic application admits
its own canister principal, rejects an anonymous write, and exposes the parent
count through its typed inspection method. The same authorized update mount
starts, resumes and cancels retained item counts. An unrelated write and an
upgrade preserve progress. A source write invalidates a running count. A count
completed at 12 still returns 12 after another insert. The application uses
Canic’s message time for its one-hour deadline. After expiry, cancellation
releases the handle and a stale resume preserves the backing. An anonymous
caller holding a valid handle cannot cancel it. The sparse listing uses the same query mount as the snapshot.
One candidate per call yields two empty continued pages before the matching
item. After the first page, the driver upgrades and retries with a different
score threshold, malformed cursor bytes and an anonymous caller. Each refuses,
and the original cursor still completes in index order.

The migration driver keeps an empty source page’s continuation for the same
parent and minimum score across copy and validation. After upgrades in each
phase, both schemas refuse the listing with the maintenance error. Publication
opens the target, where presenting the source cursor returns a cursor error.
A fresh target listing uses the widened score type and returns its first page
before retired-source cleanup with a one-candidate budget. After cleanup and
upgrade, the caller resumes the same cursor with the default eight-candidate
budget. The complete sequence of IDs must match the independent model.

The same run pauses an item count before maintenance. During copy and
validation, resumes against either schema receive `Maintenance` and preserve
stable backing. After publication, the target refuses the source handle with
`InvalidJob`. The caller cancels it during retired-source cleanup and starts a
target count after the final item write. The new count checkpoints before an
upgrade. After cleanup and another upgrade, retrying cancellation of the old
handle preserves every stable byte. Resending the new count’s first advance
returns its existing checkpoint. The count then completes with the 13 items
in the independent model.

Migration uses a private fixture control. The signed HTTP CLI test uses the separate
pinned Canic application and its inspection protocol.

The 0.23.6 driver also submits the same three-record insert during interrupted
copy, validation and retired-source cleanup. It creates a link, its item and the
item’s parent in dependent-first order. After an upgrade in each pre-publication
phase, both source and target refuse the batch with the maintenance error and
preserve the full stable backing. After the first cleanup chunk the source still
refuses it, while the target accepts it. The driver upgrades again and checks
the target against the independent model after every remaining cleanup chunk.

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

## Live pages across writes and upgrades (0.22.2)

`tests/live_pages.rs` uses the existing `write` and `fluent_ordered` Wasm endpoints.
For each direction, it captures a cursor, moves the returned row after that
position and an unseen row before it, changes filter membership, inserts on both
sides, then upgrades before resuming. The removed old index entry is not needed
to recover the logical position. Ordinary tuple sorting/comparison supplies the
oracle; expected resumed IDs are `[4, 6, 7, 1]` ascending and `[3, 1, 7, 6]`
descending. The first returned row deliberately repeats. This proves the accepted
live browsing contract, not snapshot or at-most-once enumeration.

Each page examines at most one candidate and returns at most one 16-byte
projection. Nonmatches yield empty pages with forward progress. A 15-byte result
allowance refuses without skipping the candidate; the same cursor succeeds with
16 bytes. Changed-filter cursors refuse even when the new predicate would select
the same current rows. Raw backing remains unchanged by the upgrade/read/refusal
sequence. The focused Wasm run and strict Clippy pass; the run log is retained at
`target/evidence/m2/live-pages.log`. The artifact was rebuilt with the current
qualification lock (Runtime/macros 0.23.17); this is scoped behavior evidence,
not blanket qualification of the concurrent dependency refresh.

Frozen IcyDB `6d421e3`, `db/cursor/boundary.rs`, supplies the ordered-boundary and
direction principle. IC DB keeps its existing compact logical position; no legacy
`Value` slots, cursor hierarchy or new runtime mechanism are copied.

A second installed case uses the existing `reference_batch` endpoint to delete
the cursor row and its dependent in one batch. After upgrade the old cursor still
seeks to the next key; after reinsertion at the identical primary/index key and a
second upgrade it returns the same next page. A fresh enumeration sees the new
payload. Literal remaining IDs `[2, 3, 4, 5, 6]` and `[6, 5, 4, 3, 2]` check both
directions independently, with one-candidate pages and unchanged backing during
upgrade/reads. This retains the visibility obligation from frozen IcyDB's
`positioned_data_overlay_retires_exactly_across_delete_and_reinsert`, without
copying its journal retirement mechanism. Both live-page tests and focused strict
Clippy pass; `target/evidence/m2/live-pages-delete.log` records the run against
Wasm SHA-256 `21f3571ab7318c2b0e7aec31366de07e0a51d9f33c01c85568dc602f7d72b658`.
These are current-supplier regressions to rerun at cutover, not evidence for the
rewritten Storage.

```sh
make m1-records-artifact
IC_DB_M1_RECORDS_WASM="$PWD/qualification/target/evidence/m1/records.wasm" bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml --locked -p ic-db-m0-rollback-driver --test live_pages
```

`make m2-queries` includes this witness alongside existing query/sorting checks.

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
16-KiB whole-job bound even when selection hides groups, preserving backing bytes.
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
As of 0.20, empty or whitespace-containing CLI bounds also work in the console
using single/double quotes or backslash escapes. Native parser checks compare
exact fluent operands, including Unicode and literal shell metacharacters;
the terminal regression checks quoted queries and recovery after an incomplete
quote without executing a database call. Each physical command line remains
bounded; a trailing backslash does not continue onto another line.
It also preserves the existing installed-layout query/schema and product-record
checks. `make db` installs this console fixture without starting the program.

### Controlled product aggregate contention (0.18)

Run the focused product write-schedule and shared-capacity witnesses:

```sh
make m2-contention
```

It shares `m2-jobs-artifact` with `make m2-jobs`, which also selects this test.
The existing private `write`, `job_start`, `job_advance` and `job_control`
endpoints exercise the actual engine and supplier; no new endpoint, product state
or host scheduler is introduced. Testkit owns the local PocketIC instance.

The dataset has 96 entries with independently calculated amounts
`id * 10 + id % 2`. In the schedules below, each advance permits 12 candidates. The successful runs
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

The 0.23.6 probe starts the same sum one, two, three or four advances before a
committed source write. At 12 candidates per advance, all four jobs invalidate.
At 48, the jobs with three or four advances available complete with 46,608.
Two advances process all 96 candidates but leave exhaustion unconfirmed, so a
write at that point still invalidates the job. Each attempt restores the initial
row values. These four deterministic offsets measure the completion boundary
for this fixture. Production completion rates depend on application traffic.

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

### Shared retained-job capacity (0.19)

The `shared_jobs_preserve_isolation_capacity_and_stale_handle_refusal` witness
uses all 16 existing aggregate slots: grouped sums, grouped counts and grouped
DISTINCT counts over six Entry rows, plus optional-note counts over one Document row.
It reuses the same four private controls and adds no product dependency to the
native oracle. Expected grouped totals come from literal fixture tuples.

The installed Wasm checks:

- Full capacity refuses without modifying backing, both before and after upgrade.
- A query-mismatched handle refuses intact. Cancel/refill changes handle identity;
  stale resume/cancel cannot affect the replacement job.
- A committed Entry write invalidates unfinished Entry jobs without exposing
  partial results. Document jobs complete unaffected; an already completed grouped
  sum retains its old revision and exact result.
- Completed and invalidated jobs still occupy capacity until explicit cleanup.
  Cancelling all jobs permits 16 replacements, which complete with the new sum.
- Across three upgrades, progress and bytes persist. Expiry cleanup removes
  3/3/3/3/3/1 jobs with limit 3, then zero; a fresh job can start afterward.

This proves scoped logical capacity/isolation and recovery behavior. It is not a
performance comparison, physical allocation-reuse claim or large-dataset bound.
Run just this witness with the prepared PocketIC server:

```sh
make m2-jobs-artifact
IC_DB_M2_JOBS_WASM="$PWD/qualification/target/evidence/m2/jobs.wasm" \
bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml \
  -p ic-db-m0-rollback-driver --test jobs_contention shared_jobs_ -- --nocapture
```

### Retained variable-sized values (0.21.0)

Run only the variable-sized value witness after preparing PocketIC:

```sh
make m2-jobs-artifact
IC_DB_M2_JOBS_WASM="$PWD/qualification/target/evidence/m2/jobs.wasm" \
bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml \
  -p ic-db-m0-rollback-driver --test jobs_envelope retained_text_winners -- --nocapture
```

`make m2-job-envelope` and `make m2-jobs` also select it. Three private,
`aggregate-jobs`-only controls write existing fixture entities and construct
typed product operations. They do not change the ordinary console, authored
fixture schema or stored format. Native literal tuples and standard string
comparison provide the independent result oracle; the driver imports no product
codec or reducer. Whole stable-memory snapshots check refusal/upgrade preservation.

Checked on 2026-11-17:

- Twelve Document rows form six groups containing absent/empty text, an embedded
  NUL prefix pair, Unicode, equal winners and a 64-byte UTF-8 value. Both grouped
  minimum and maximum advance two candidates at a time, survive heap-losing
  upgrades and return three exact pages in descending aggregate/key-tie order.
  All-absent groups survive with no winner. An output budget of one byte permits
  accumulation but refuses completion intact; retrying with sufficient output
  space succeeds from the same saved sequence.
- Thirty-two EnvelopeRow records use 480-byte text values. The group key includes
  `id` and that text; the winner is the text itself. Sixteen key/winner pairs fit
  the 16 KiB checkpoint, while the seventeenth refuses before the 32-group ceiling.
  A result limit of one does not hide that state exhaustion. Retrying after an
  upgrade refuses with exactly the same backing and checkpoint.
- Explicit replacement with 128-byte values invalidates the old run. A new run
  completes all 32 groups and yields eight four-group pages across another
  upgrade. An insufficient final-output allowance preserves its initial checkpoint.
- A Blob maximum containing non-UTF-8 bytes survives a checkpoint and upgrade.
  Insufficient output space refuses with unchanged backing; retry returns the
  literal byte vector intact through the existing typed result mapping.

The local receipt is `target/evidence/m2/jobs-values.log`; it records instructions
and observed whole-call cycle charges for advances and pages. The qualified Wasm
SHA-256 is `2d6a7786c51a6e0389c2fa630e72f88581d670672caf0e53c79d98799e7e3cfc`.
Cold accesses follow real upgrades and remain distinguishable in the call sequence.
These are scoped correctness/resource checks, not a matched IcyDB benchmark or a
claim about all variable-sized types. Frozen IcyDB `6d421e3`'s text ownership/null
regressions in `aggregate/value_reducer/tests.rs` and
`aggregate/contracts/state/grouped/tests.rs` inform the preserved behavior;
their in-memory reducer implementation is not copied.

### Retained Decimal averages (0.21.0)

```sh
make m2-cli-artifact
IC_DB_M1_RECORDS_WASM="$PWD/qualification/target/evidence/m2/console.wasm" \
bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml \
  -p ic-db-m0-rollback-driver --test console_aggregates -- --nocapture
```

`make m2-cli` and `make m3-cli` select this witness in their existing console
rosters. It uses the unmodified `decimal_batch` and console start/resume/cancel
handlers, the ordinary DecimalRow schema and real product jobs. No endpoint,
reducer, stored representation or new supplier requirement is introduced.

Checked on 2026-11-17: thirteen four-row datasets, each with scalar and grouped
averages at result scale 2, survive thirteen real upgrades. Literal input/result
strings form the independent oracle; no product arithmetic or codecs enter the
driver. Input advances in unequal 1/2/1-candidate chunks, followed by explicit
exhaustion. Replaying a pre-upgrade sequence neither rescans nor returns a prefix
average. Coverage includes:

- Positive and negative ties rounding toward the even final coefficient:
  1.245 → 1.24, 1.255 → 1.26, -1.245 → -1.24 and -1.255 → -1.26.
- Half-cent results round to present zero; absent inputs do not enter the
  denominator. All-absent input yields no scalar result and an absent grouped
  value, distinct from a present zero.
- Unequal chunks retain the complete sum/count, rather than averaging rounded
  chunk averages. Repeated maximum/minimum scale-2 coefficients finish with a
  fitting average even though the intermediate sum exceeds i128. Opposite large
  totals cancel across the upgrade and subsequent chunks.
- A final output refusal preserves all backing bytes; retrying the same sequence
  with adequate output space completes correctly. Ordinary upgrades preserve
  backing exactly, and completed jobs are explicitly cancelled.

The local receipt is `target/evidence/m2/console-values.log`. The console
Wasm SHA-256 is `acb67e37c4e3f03eced5bed095221efd30e1e3d49df2936c501f25c5063ad8b4`.
This extends retained-operation evidence; earlier synchronous Decimal checks
remain separate. It is not a performance or full IcyDB-parity comparison.
Frozen IcyDB `6d421e3`, `db/numeric::average_decimal_terms_checked` and
`aggregate/value_reducer/tests.rs`, supplies the sum/count, absence and typed
overflow lessons. IC DB deliberately retains widened totals and the agreed
declared-scale/ties-to-even result contract rather than copying IcyDB's adaptive
division precision or bounded Decimal running sum.

### Retained aggregate envelope (0.20)

```sh
make m2-job-envelope
```

`make m2-jobs` also selects `tests/jobs_envelope.rs`. The existing job fixture
adds grouped DISTINCT selections (`category` key, distinct `amount` or `(id, amount)`) and
private measurement wrappers around its existing advance/page/control handlers. They are
selected only by `aggregate-jobs`, not the normal console or product. The product
now owns a separate 16 KiB whole-job bound; rows remain at 4 KiB. Encoder/decoder
primitives are shared, without public budget setters or a second codec.

The first dataset has 93 rows in 32 groups, with `1 + category % 5` rows per
group and `amount = id * 10`. Independent native tuples establish counts, sums,
integral averages, count filtering (`>= 2`), descending aggregate order and
ascending key tie-breaking. All input is consumed before a separate exhaustion
call finalizes the result. The test proves:

- All 32 groups fit for these count/sum/average shapes; 32 distinct categories
  accept subsequent duplicates. The 33rd group/distinct category refuses.
- A one-byte output allowance refuses finalization without modifying stable
  memory. Retrying with 128 bytes yields nine pages containing the exact 25
  selected groups. Replays and upgrades preserve both progress and output.
- Cardinality refusals preserve the entire stable-memory backing before and
  after upgrade. Old-sequence retries do not accumulate twice. Cancellation
  releases the job for a new, successful count.

A separate 1024-row dataset uses 32 groups and 32 distinct amounts per group.
Numeric grouped DISTINCT completes, including an upgrade after 512 candidates
with a checkpoint larger than 4 KiB. On the same input, distinct `(id, amount)`
tuples reach the **16 KiB whole-job frame** before either cardinality limit:
steps of 16 save 784 candidates; a refused step preserves that checkpoint,
including after upgrade. Steps of one then reach 790; candidate 791 refuses
without a write. Cancellation permits a fresh count of all 1024 rows. This is
an encoding-dependent boundary, not a 790-value product limit. Smaller chunks
cannot make a larger retained state fit; the 128 KiB transient bound does not
override the persisted frame bound. Larger values can exhaust the frame earlier.

The earlier 4 KiB numeric probe stopped at 293 values. Increasing the independent
job budget admits the complete 32-by-32 numeric case without raising group,
per-group DISTINCT, row, output or per-message work limits. Sixteen full frames
contain at most 256 KiB of encoded job records, excluding supplier/allocation
overhead. This is a starting budget, not a claim about arbitrary query completion.

The lifecycle extension fills all 16 slots with the 790-tuple checkpoints and
proves that another start refuses without changing storage. After an upgrade,
a source mutation invalidates one job without silently freeing its slot.
Cancellation and multi-job cleanup traps restore the complete stable-memory
backing, including allocator metadata. A successful cancellation permits a
replacement with a new handle; the old handle cannot cancel it.
No-expiry cleanup and an invalid removal limit perform no writes. Expiry cleanup
then removes 1/3/12/0 jobs with limits 1/3/16/16 across the same store. All 16 slots
are filled with near-limit replacements, old handles still refuse, and one
maximum-size expiry cleanup removes all 16. Logical reuse is established;
physical allocation shrinking or long-term supplier reuse is not inferred.
Cleanup examines at most 16 frames before deletion; its limit bounds removals,
not examined slots. The optimized lifecycle path borrows cursor/state slices
while validating the complete outer frame, then uses IC Storage's existing
`remove_discard` rather than requesting the removed value. Resume shares this
decoder and copies bytes only when it needs owned mutable state. One bounded
supplier read per inspected frame remains; this is not metadata-only storage I/O.
Stored bytes, lifecycle policy and corruption admission are unchanged.
Native negative cases keep valid identity/expiry metadata but truncate the tail,
append trailing bytes or corrupt its length: both cancellation and cleanup refuse
without deleting the earlier valid slot. Correctly framed opaque accumulator
bytes remain cancellable even when the reducer rejects their contents.

Job store v2 (`ICDBAJ02`) preserves the record encoding and admits the wider
envelope. Native cases construct the v1 header independently and prove read-only
reopening, refusal preservation, insert/save promotion and cancellation/expiry
without promotion. Existing Wasm predecessor hooks also ran against the recorded
4 KiB artifact (SHA-256
`ee01c24a829691f6fff20e4088613712f701c69cbf473a4743ede6c690d439e8`):
old scalar/grouped/DISTINCT progress resumes, traps restore the header and job,
and the old executable refuses a promoted store intact. Reinstalling the new
executable restores access. No data reset or application schema migration is
required; after the first successful insert/save, keep a v2-capable executable.
Without predecessor paths, the normal job test qualifies same-code upgrades.
`IC_DB_M2_SCALAR_JOBS_WASM`, `IC_DB_M2_GROUP_COUNT_JOBS_WASM` and
`IC_DB_M2_DISTINCT_JOBS_WASM` select predecessors for those checkpoints;
`IC_DB_M2_V1_JOBS_WASM` explicitly selects a v1 executable for downgrade refusal.
All four pointed to the recorded 4 KiB artifact for this run.

Measured on 2026-11-16 with the pending 0.20 engine and qualification host,
Rust 1.100.0, IC Storage 0.74.1, Platform Runtime 0.23.14, qualification
Wire 0.24.3 and PocketIC 16.1.0 on its application subnet. The release-profile
fixture is 1,106,347 uncompressed Wasm bytes, 1,052,072 code-section bytes, SHA-256
`9fabafdb93a742606625f50bea3bc0dadfd9db5df32a402ebeef5ca0f9a05d39`.
It includes qualification controls; its size is not a production footprint.

| Selected operation | Instructions | Charged cycles |
| --- | ---: | ---: |
| Warm 32-group sum finalization | 982,321 | 8,082,194 |
| Warm 32-group average finalization | 1,386,264 | 8,485,073 |
| Warm count finalization, filter/order and first three output rows | 882,528 | 7,977,207 |
| First completed-page replay after upgrade | 2,138,861 | 9,233,817 |
| Subsequent three-row pages, maximum observed | 764,243 | 7,925,324 |
| Numeric grouped DISTINCT: 32 candidates taking retained state to 1024 | 6,986,953 | 14,075,043 |
| Numeric grouped DISTINCT: finalization at 1024 values | 6,455,613 | 13,554,268 |
| Tuple grouped DISTINCT: refused 16-candidate step just after upgrade | 6,772,683 | 13,858,461 |
| Tuple grouped DISTINCT: refused candidate 791, warm | 5,013,308 | 12,098,886 |
| Invalidate one 790-tuple job after a source change, warm | 4,461,004 | 11,549,071 |
| Cancel one near-limit job, warm | 713,803 | 7,796,049 |
| Check 16 near-limit jobs before expiry, warm (replacement set) | 1,598,695 | 8,621,443 |
| Cleanup first expired job from 16 near-limit frames, after upgrade | 2,202,431 | 9,225,894 |
| Cleanup all 16 near-limit replacements in one call, warm | 2,660,405 | 9,683,270 |

The matched pre-change fixture was 1,106,615 / 1,052,356 total/code bytes,
SHA-256 `b1e513bc311a6d81296e29375661b04f8445aa9968e04b22a5edc3546f5dbe82`.
With the same input, limits, sequence and supplier, full cleanup falls from
3,579,543 to 2,660,405 instructions (25.7%) and 10,602,408 to 9,683,270 charged
cycles (8.7%). Both perform 112 backing writes; ownership/return-value copies
changed, not deletion semantics. Empty cleanup costs 137,460 instructions and
7,160,134 charged cycles in the current fixture. Thus full cleanup adds 2,523,136
cycles above that baseline; the whole update charge is not database execution
alone. This is an IC DB before/after comparison, not new IcyDB savings evidence.

Instruction intervals include reopening, query admission, checkpoint work and
result conversion, excluding transport decode/encode. Finalization rows include
the exhaustion check and save; they are not isolated reducer microbenchmarks.
Cycles are the observed balance decrease around the entire isolated update,
including platform/transport charges. The runner emits each sample and artifact
identity; these are observations, not universal ceilings or a matched IcyDB run.
Rich keys, other arithmetic states, maximum transient memory and production
write rates need separate evidence. The lifecycle measurements cover the selected
near-limit numeric tuple frames, not every supported accumulator representation.

Frozen IcyDB `6d421e3`,
`crates/icydb-core/src/db/executor/aggregate/contracts/grouped/context/tests.rs`,
separately checks exact DISTINCT cardinality, duplicates at the limit and byte
ceilings. This witness carries those invariants to IC DB's retained-job boundary;
it copies no executor code or adjustable-budget framework. Existing shared-slot
and rollback probes remain responsible for their separate failure boundaries.
Borrowed inspection follows the existing owned codec's scalar-view approach,
also present in frozen IcyDB's `db/data/persisted_row/codec/scalar/mod.rs`:
validate framing once and materialize ownership only for the caller that needs
it. No additional legacy code, decoder or persisted metadata was introduced.

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
source. The successive migration adds the empty ManagedRow and Document entities
to C. Writes stay closed while copying, and cancellation restores B. After
publication and upgrade, a batch transfers a copied Entry's sole incoming reference
from its old mapping to the first Document. An attempted transfer to a missing
Entry leaves the complete backing intact. The valid transfer is visible through
the Document index, and the Entry remains protected from deletion across another
upgrade and retired-source cleanup. The fixture also checks the initial ManagedRow
count and reads its first insert. Native corruption checks reject a nonzero starting revision or
count. A native schema-copy test checks the opposite direction: a copied row
gains an optional reference to a newly declared entity. Copying `Some(0)`
fails target validation and permits cancellation. Copying `None` permits
publication, after which one batch inserts key zero and links the migrated row.
The reverse index finds the row and restricts deletion of its target.
Entity removal and key remapping remain unsupported.
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
For B → C, the host selects its ordinary read allocation from a stable cell.
The driver traps after updating that cell and both manifests, compares the full
backing, then upgrades and checks that a routed read still refuses during
maintenance. A successful retry routes reads to C after upgrade and after B
cleanup. Cancellation instead restores the exact B data, including intervening
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
32-command batch deleting that target and 31 dependants must refuse without a
backing write. Two calls start from identical backing. One leaves the first
reverse-index entry live, and the other stages removal of the first 31 entries.
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

The 2026-11-20 rerun of `make m3-references` used Storage 0.78.7 and
fault-enabled Wasm SHA-256
`529984b6fd593925b58cfffed613be6e2b653a924543b32a77d8ac826f5a917a`.
The two refused batches issued zero stable writes and preserved every backing byte.

| Ordinary delete batch after publication | IC instructions |
| --- | ---: |
| Refused with the first reverse-index entry still live | 13,052,773 |
| Refused after skipping 31 staged removals | 13,298,805 |
| Accepted final 32-row batch, including the target | 28,046,233 |
| Maximum across all eight accepted batches | 41,471,561 |

These counters cover the complete fixture operation including reopen and batch
preparation. The refused batches address different row keys, so their difference
does not isolate index traversal cost.

This carries forward IcyDB's target-existence and final-batch deletion invariants
using the existing IC DB maintenance mechanism, without its general relationship
framework. Migration qualifies one entity with small Nat64 rows and one incoming
reference per row; post-publication deletion additionally checks 254 incoming
references on one target. Nested references, larger populations, arbitrary payload
sizes and allocation reuse remain outside this result. It introduces no product
API or durable state.

### Restricted delete phases against frozen IcyDB

On 2026-11-20 the installed IC DB fixture and frozen SQL-disabled IcyDB
`6d421e3` performed the same ten delete requests. Each started the deletion
sequence with 254 Nat64 rows (IDs 0 and 128 absent), an optional self-reference
to 255, a compound reference/ID index and a unique rank. Both refused the two
target deletions, then removed all rows in eight batches, upgrading after the
first accepted batch. The legacy harness checked the expected presence,
reference and rank for every ID after each call and verified the relation
constraint diagnostic. IC DB checked raw row/index entries, backing
preservation and zero supplier writes on refusal.

| Workload | Engine | Fixture setup | Open/session | Build | Commit | Whole call |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| First survivor live | IC DB | 0.36 | 0.30 | 0.02 | 12.37 | 13.05 |
| First survivor live | IcyDB | — | 0.53 | 0.29 | 13.80 | 14.65 |
| First 31 removed | IC DB | 0.36 | 0.30 | 0.02 | 12.64 | 13.32 |
| First 31 removed | IcyDB | — | 0.53 | 0.29 | 14.57 | 15.57 |
| Final 32 rows | IC DB | 0.36 | 0.33 | 0.02 | 27.33 | 28.05 |
| Final 32 rows | IcyDB | — | 0.68 | 0.28 | 39.35 | 40.40 |
| Eight accepted batches combined | IC DB | 3.00 | 2.22 | 0.19 | 291.64 | 297.07 |
| Eight accepted batches combined | IcyDB | — | 4.99 | 2.29 | 338.59 | 346.48 |

Units are millions of IC instructions inside the handler. Totals include
unattributed wrapper work and exclude transport encoding/decoding. IC DB setup
includes constructing both schemas and the transition, acquiring allocations
and arming fault instrumentation. Open measures target admission alone. IcyDB
uses its post-startup session accessor and binds the entity while constructing
typed delete intents. Its commit executes the trusted typed batch. The legacy
harness separately drove startup once after each accepted write, costing another
458.80M instructions combined. That includes readiness checks and journal work.
It is excluded from the timing table for both the individual calls and their sum.

The legacy host seeded the matching logical rows directly. IC DB reached them
through migration and repointing. Their B-tree histories, host schemas,
instrumentation and reopen paths differ. Neither the historical cost increase
nor its cause can be inferred from these two binaries. The [per-call counters](measurements/0.24.2-restricted-delete.csv)
record all ten requests. IC DB Wasm SHA-256 was
`3bab1a8746617a48d90b5ef194245fbf0e9dd2e88aed8fe1c55c6919001b16b5`;
legacy was `d9dd9d116236e33cd6e309d6944cf826b00264825c957df9f16e723190c797a1`.
The one-off compatibility harness, binaries and output were retained locally in
`qualification/target/evidence/restricted-delete-comparison/`. Routine legacy
qualification continues to select query comparisons. Run `make m3-references`
for the instrumented IC DB workload.

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

The 0.20 schema JSON check runs the installed-layout CLI against the existing
records Wasm. It parses complete stdout, compares selected/full schema facts,
checks optional text bounds and reverse-reference indexes, includes a recursive
named type once, excludes unrelated types, and checks empty stdout on unknown
entity refusal. The default table command remains covered. JSON is native CLI
presentation only; schema authority, tuple transport and product codecs are
unchanged.

```sh
make m1-records-artifact
IC_DB_M1_RECORDS_WASM="$PWD/qualification/target/evidence/m1/records.wasm" \
  bash scripts/with-pocketic.sh cargo test \
    --manifest-path qualification/Cargo.toml --locked \
    -p ic-db-m0-rollback-driver --test records \
    console_schema_json_is_complete_selected_and_pipeable -- --exact
```

Per-call CLI statistics use PocketIC's canister balance before/after each query
or update and a monotonic host timer around that call only. No extra update or
tick is executed to manufacture a query cost. These are local call latency and
observed charges, not production latency or IC instruction measurements. JSON
tests require statistics on stderr; normal commands place them after results.
The installed migration demo checks this ordering and
schema/status/count/query labels, per-resume aggregate reporting, cleanup and
migration updates, two decimal places for time and positive measured update
charges. Unit checks cover exact large-integer deltas, rounding and explicit
balance-increase reporting instead of saturating to zero.

The terminal regression in `make m3-cli` runs the installed-layout binary through
the host's `script` utility and the real prepared launcher. It waits for each
prompt before sending another command, and requires counts plus their statistics
without closing stdin. Blank lines, parse-error recovery and prompt-free `quit`
are covered. This caught the separate launcher child group being suspended by
SIGTTIN when it inherited the foreground terminal. The foreground CLI now forwards
terminal input through a pipe and retains interactive prompting in the child;
Testkit still owns server and child cleanup. Linux execution passes; the macOS
`script` invocation is supplied but awaits native qualification.
The earlier launcher used `--idle-ttl 86400` for the console. Since Testkit 0.109.0,
the selected launcher instead imposes a one-hour total command budget for console and test launches.
The explicitly selected idle regression
waits 90 seconds without server requests, then exercises queries and normal quit:

```sh
IC_DB_M1_RECORDS_WASM="$PWD/qualification/target/evidence/m2/console.wasm" \
  cargo test --locked --manifest-path qualification/Cargo.toml \
    -p ic-db-m0-rollback-driver --test console_terminal \
    terminal_survives_pocketic_default_idle_timeout -- --ignored --exact
```

This long check is excluded from routine CLI tests. A fresh console must not
inherit the server's 60-second test-oriented idle shutdown; no heartbeat changes
call counts, canister state or cycle statistics.
Linux execution passed on 2026-11-16: 90 seconds with no requests, followed by
the terminal command/history checks and successful quit (94.21 seconds total).
The same terminal regression now sends actual ↑/↓, cursor/delete and Ctrl-C
sequences. It checks recalled results, draft restoration, editing, cancellation
and refusal of multiline bracketed paste without executing a prefix. Rustyline
owns line editing and session history; the foreground relay renders each result
before requesting another line. Piped and single-command JSON modes bypass it.

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
the exhaustion check. Non-identity sorted DISTINCT retains the best requested distinct tuples.
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
Non-identity projections require all varying projected fields to lead the explicit order. Fields fixed by a mandatory literal equality can be omitted or interleaved. Native tuple/text oracles and Wasm upgrade checks cover duplicate removal
before limits, hidden-order refusal and complete-input work bounds. The Wasm
witness reduces 511 rows to two categories across upgrade, accepts exactly
32 distinct categories and selects category 32 once a 33rd value is present.
Native mixed-direction tuple oracles exercise 64 distinct values twice in both directions. This reuses the sorter, not the
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
For `query`, `--limit` defaults to 3 output rows and `--work` to 128 candidates.
The local fixture supplies its 4096-byte output allowance automatically. An empty page may still continue.
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

### Blob query contract (0.21.0)

Focused native checks:

```sh
cargo test --locked -p ic-db --test queries --test key_bounds
cargo test --locked -p ic-db-macros --lib
```

The Blob cases in `crates/ic-db/tests/queries/blobs.rs` compare scans, indexed
ranges/pages, projected DISTINCT, groups and extrema with ordinary Rust byte
vectors and ordered collections. Values cover duplicate/empty payloads, zero
escaping, proper prefixes and bytes that are not UTF-8. Optional Blob indexes
preserve absence separately from empty values; unique replacement/deletion
removes old entries. Overlong encoded keys and duplicate unique values refuse
without changing backing. Literal malformed keys exercise the integrity boundary.
Existing Text key-bound tests protect the shared encoder's old behavior.

The [retained-value witness](#retained-variable-sized-values-0210) checks a Blob
winner across a real Wasm upgrade and an output refusal/retry. This small probe
establishes checkpoint ownership, not exhaustive Blob workload qualification.
The local CLI accepts `0x`-prefixed payload bounds and renders Blob values as hex.
Retained Blob commands use the same job endpoints as other fixture entities;
0.22 adds inclusive payload bounds through the same fluent predicates as
synchronous queries. There is no matched legacy benchmark.
The frozen IcyDB ordered-key implementation rejects Blob keys; this increment
extends IC DB's existing terminated-byte encoder and shared execution paths.

`make m2-cli` selects parser/display qualification and `console_text`'s Blob
boundary case. The latter replaces the existing BinaryObject fixture with empty,
zero-byte and non-UTF-8 values, reopens through real upgrades, and checks literal
hex results for projections/extrema/groups. Absent optional bytes differ from
present empty bytes. Whole backing snapshots verify read/refusal preservation;
an oversized direct operand exercises server admission independently of Clap.
The installed-console witness also checks payload bounds, DISTINCT, ordering,
counts and one call-accounting footer per command. These are fixture checks,
not production endpoint qualification.

`console_aggregates` also checks all ten retained Blob selectors with absent,
empty and non-UTF-8 inputs. Three upgrades preserve ten simultaneous jobs each;
thirty completions/cancellations reuse the 16-slot store. A changed selector and
an insufficient output budget refuse intact; retry completes. A source write
invalidates an unfinished result. The console witness checks that accumulation
shows no result table, then verifies start/resume/cancel accounting and cleanup.
The independent oracles are literal byte vectors and display strings. This
preserves IcyDB's selected-value ownership/null lessons through durable jobs;
no legacy reducer or dynamic value hierarchy is copied.

Checked 2026-11-17 with console Wasm SHA-256
`acb67e37c4e3f03eced5bed095221efd30e1e3d49df2936c501f25c5063ad8b4`.
Local receipts are `target/evidence/m2/console-values.log` and
`target/evidence/m2/console-blob-cli.log`.
The earlier `target/evidence/m2/console-callers.log` requalified five existing
query/sorting callers after the private request change, with console SHA-256
`bee63c173b5b7f7c1362502ddacc12e28291d87e83f9c779d508b5fff58cfdd1`.
Retained Blob commands do not change that request. Parser unit tests,
driver/canister Clippy, formatting and local documentation links also pass.

The 0.22 increment extends that witness with seven inclusive payload-range cases:
empty bytes, proper prefixes, non-UTF-8 bytes and contradictory bounds.
Each consumes one candidate before an upgrade, refuses changed or oversized
resume operands without altering backing, then completes with the exact count.
Wrong-entity bounds refuse before creating a job. The CLI witness covers filtered
counts, extrema and groups, including empty results and start/resume/cancel
accounting. Optional Blob grouping and projected DISTINCT counts are also exposed
through synchronous and retained selectors. Thirteen simultaneous jobs per
dataset retain absence, empty bytes and non-UTF-8 values across upgrades; 39
completions/cancellations reuse the existing 16 slots. Native multi-row checks use
an independent `BTreeMap<Option<Vec<u8>>, u64>` oracle, including duplicate values
and a zero present-field count for the absent group.
That optional/range increment changes the private retained request with its
callers; the later prefix increment below extends both console requests.
The durable job format remains unchanged.

Checked 2026-11-17 with console Wasm SHA-256
`aef602cf1b9d6636059e9da3755a2093820509eb0b69978c2ed604e126841748`:
`target/evidence/m2/console-optional-blobs.log` (two retained aggregate tests and
three text/Blob query tests) and
`target/evidence/m2/console-optional-cli-retry.log` (CLI integration). The CLI
test's table reader now recognizes an absent first-column value instead of
discarding that row; the original failure log is retained alongside the retry.
The focused native Blob oracle, driver/canister Clippy and formatting also pass.
These use the
pre-existing qualification lock selection of Platform Runtime 0.23.16 and
Host Artifacts 0.80.3; this increment did not resolve or update dependencies.

The focused native retained-Blob check uses 64 rows: 24 initial nonmatches,
followed by 40 rows with six duplicate byte keys (empty, zeroes, proper prefixes
and non-UTF-8). Work budgets 1 and 7 produce the same exact DISTINCT count and
group selection after reopening both stores on every advance/page. Primary
scanning examines 64 candidates; explicit indexed filtering examines 40.
An independent Rust map supplies counts, aggregate-order ties and the selected
top three groups. Replay leaves backing unchanged; a too-small completed-page
allowance refuses intact and the same continuation then succeeds. This is
native persistence evidence, not a new IC instruction or Wasm-upgrade claim.

```sh
cargo test --locked -p ic-db --test jobs retained_blob_groups_and_distinct_merge_across_sparse_reopens_and_pages
```

A second native retained check mixes 24 optional Blob rows, including repeated
absent, empty, zero-byte and non-UTF-8 values. Two-candidate chunks reopen both
stores before each advance, on primary and optional-index paths. An independent
map establishes four DISTINCT keys and present-field counts: absent zero, empty
eight, and four each for zero-byte and non-UTF-8 keys. No aggregate result is
returned until completion. This adds no engine or durable-format change.

```sh
cargo test --locked -p ic-db --test jobs retained_optional_blob_groups_preserve_absence_across_chunks
```

Pending fluent Blob prefix qualification compares `StartsWith` with Rust slice
prefix matching for every one-byte prefix, empty and multi-byte prefixes, and an
oversized index-seek operand that must remain a residual filter. Required and
optional fields, primary/index paths, both directions and small pages preserve
exact rows; eligible index seeks examine only matches. Completed group extrema
share the same predicate. A retained count reopens after an incomplete step,
refuses a changed prefix without changing backing, then completes with the
original input. These native checks establish engine behavior independently of
the console transport and do not establish Wasm performance.

```sh
cargo test --locked -p ic-db --test queries blob_prefixes_match_raw_bytes_on_scans_indexes_and_sparse_pages
cargo test --locked -p ic-db --test jobs retained_blob_prefix_binding_survives_reopen
```

The console exposes `--payload-prefix HEX` for `query BinaryObject` and
`aggregate BinaryObject`, using the existing bounded hex parser and one shared
predicate builder with the inclusive range flags. The private query/retained
tuples now carry the optional prefix; all callers change together. No application
protocol, durable representation or new executor is introduced.

Checked 2026-11-17 with console Wasm SHA-256
`3703ff816393985e49faa48b2423c6ef29b563e070eb138211b76c8ef012d32f`:
`target/evidence/m2/console-blob-prefix.log` covers parser checks, three console
query tests and two retained aggregate tests. Prefixes exercise empty/zero/non-UTF-8
bytes and combined bounds. Retained calls upgrade after an incomplete step,
refuse changed/oversized prefixes with unchanged backing, then complete and cancel.
`target/evidence/m2/console-blob-prefix-cli.log` checks actual CLI commands,
including 25 Blob aggregate cases and per-call reporting.
`target/evidence/m2/console-blob-prefix-callers.log` requalifies both sorting
callers, existence, sparse pages, Decimal aggregates and indexed Text prefixes.
Native/Wasm Clippy and formatting pass. These establish behavior in the selected
fixture; they are not a matched performance comparison or deployed application proof.

### Blob prefix composition (0.22)

Native checks extend prefix coverage to optional Blob DISTINCT projections and
distinct counts on primary and index paths, with independent raw-byte set results.
Retained grouped min/max use nine rows: the input range excludes one sentinel,
late winners enter/leave a zero-byte prefix, completed winners sort descending,
and a two-group limit spans two one-group output pages. Both stores reopen after
each one-candidate advance and each output page. Changing the prefix refuses
without changing backing. Primary reads examine eight candidates; the secondary
path examines nine, including the residual nonmatch. Empty values, non-UTF-8 bytes
and equal-winner key ordering are included.

```sh
cargo test --locked -p ic-db --test queries blob_prefixes_match_raw_bytes
cargo test --locked -p ic-db --test jobs retained_blob_group_prefix_selection
```

This checks the existing shared executor and finalization contract; no product
code, durable state or format changes are needed. It adds native persistence
evidence, not Wasm-upgrade or performance claims.

### Blob suffix matching (0.22)

`Comparison::EndsWith` uses the shared comparator for Text and Blob. Native
raw-slice oracles check every one-byte suffix, empty/multibyte suffixes, embedded
zeroes, non-UTF-8 bytes and a 600-byte nonmatch. Required/optional fields, primary
and secondary traversals, both directions and sparse pages agree with Rust
`ends_with`. All input candidates remain charged: suffixes do not seek an index
range. A suffix beginning with `FF` cannot start inside the escape for zero.
Completed group extrema use the same boundary rule. Retained counts reopen after
an incomplete chunk, refuse a changed suffix without changing backing, then
finish with the exact count and cancel normally.

```sh
cargo test --locked -p ic-db --test queries blob_suffixes
cargo test --locked -p ic-db --test jobs retained_blob_suffix
```

No durable/schema change or additional execution path is introduced. These are
native checks for the 0.22 release; the subsequent CLI witness below adds Wasm
qualification. No performance comparison is claimed. The later Blob substring checks use
the same sparse-page oracle with raw slice windows and cover overlapping
escape-boundary matches, group extrema and retained count after reopen.

### Blob suffix console (pending)

`--payload-suffix HEX` selects the same residual predicate in synchronous and
retained fixture queries. Raw-slice oracles exercise empty, zero and non-UTF-8
payloads after upgrade, including the escaped-zero/`FF` false-match boundary.
Retained counts combine suffix, prefix and range filters; changing or oversizing
the suffix during resume refuses without changing backing. Input work still
requires completion before a count is returned, including count zero.

The focused Blob parser/console/retained checks and actual interactive CLI check
pass with console Wasm SHA-256
`4d75dbd41fb8edc70a6457d4e748c4d83f6955d646f00cfc661564935969b097`.
This changes the paired private fixture request shape, not the product engine,
schema, stored jobs or Canic's production inspection contract.

### Literal Blob defaults (0.22)

Rust byte literals extend the existing omission-aware insert path. Native checks
cover exact encoded bytes, wrong-type/oversized metadata, compile-time literal
refusal, explicit absence versus empty bytes, default uniqueness conflicts,
replacement/reopen preservation and changed-default schema refusal. Existing
integer/Text default checks continue to pass. The host's existing hex formatter
is shared by query results and schema default descriptions.

The installed `DefaultRow` witness adds empty and non-UTF-8 Blob defaults. It
checks omitted/explicit values, oversized refusal, whole-message rollback,
replacement without default reapplication and unchanged-schema upgrade.
Checked 2026-11-17 using console Wasm SHA-256
`77dbd7351e4742c4a0dcff287fceca353c21a4e721e446d36204f71dc0646093`;
receipt: `target/evidence/m2/blob-defaults.log`. This changes the private fixture
schema and default endpoint reply together, not existing application contracts
or the database codec. Adding/changing an authored default still changes its
schema identity. Macro/library and affected native/Wasm caller Clippy pass.

```sh
cargo test --locked -p ic-db --test defaults
make db
db schema DefaultRow
```

### Remote inspection ownership

Canic owns the `canic db inspect` command and its signed HTTP and installed-canister tests. The application-specific `db witness-list` client was retired. The local console and the mixed lifecycle workload continue to exercise IC DB query semantics.

### Ordered Document prefix listing (0.23.4 worktree)

The PocketIC probe lists published documents whose titles start with
`special-`, projecting IDs two at a time. Among 260 documents, three match.
Two matching titles tie across the page boundary. The data also includes an
unpublished prefix match and published titles on both sides of the range.

| Traversal | Page 1 fetched / instructions | Page 2 fetched / instructions | Total instructions |
| --- | ---: | ---: | ---: |
| Default primary order | 258 / 4,264,296 | 2 / 582,045 | 4,846,341 |
| Title order via `(published, title)` | 2 / 999,428 | 1 / 833,248 | 1,832,676 |

Both requests used the same filters, 512-candidate budget and output shape.
Primary order coincides with title/ID order for these three matches. Both
returned IDs 257 and 258, then 259. The second indexed page cost more than the
primary continuation on this fixture. The overall saving came from avoiding
the long first scan. Instructions cover open, admission, traversal and result
formatting inside the measurement endpoint. Wasm SHA-256
`9500efde1f2f027b144ff282be2d0fbdae001e8e05c7e771c3339baaa0db536e`.

Reusing the primary-key position resolved during admission and resolving each
index component field once during row validation gave the following matched rerun.
The same page assertions passed, including both owner fetches on the first
indexed page and one on the continuation.

| Traversal | Page 1 instructions | Page 2 instructions | Two-page change |
| --- | ---: | ---: | ---: |
| Default primary order | 4,166,815 | 581,340 | −2.03% |
| Title order via `(published, title)` | 989,279 | 828,203 | −0.83% |

Rerun Wasm SHA-256
`95f441339f33db636a7ba66df4c42ade54a450991764194414ef0b548b45d963`.

A second fixture build added five single-field, non-unique Document indexes
(`score`, `note`, `previous_score`, `priority`, `entry_id`) after the three
existing declarations. The same probe against fresh backing returned identical
IDs, cursor boundaries and candidate/fetch counts.

| Traversal, both pages | Three indexes | Eight indexes |
| --- | ---: | ---: |
| Default primary order | 4,748,155 | 5,186,135 |
| Title order via `(published, title)` | 1,817,482 | 2,113,443 |

These are whole-call instruction counts. Additional declarations and stored
entries also affect open/admission and physical map layout. The difference
does not isolate key reconstruction. To repeat, add the five declarations to
the qualification schema in a disposable checkout, then run the same commands
below. Eight-index Wasm SHA-256
`326417746c59d9a0f3007806d855f037ed01d6d2f795b377f7b36054e80c2584`.

Comparing the visited key in place retained every page assertion. Both
fixtures used the same component framing checks and 512-byte key bound.

| Two-page traversal | Three indexes, before → after | Eight indexes, before → after |
| --- | ---: | ---: |
| Default primary order | 4,748,155 → 4,751,337 | 5,186,135 → 5,189,317 |
| Title order via `(published, title)` | 1,817,482 → 1,797,750 | 2,113,443 → 2,053,352 |

The indexed reads saved 1.09% and 2.84% respectively. The primary control
increased by 3,182 instructions in each build (under 0.07%). These totals
cover two query calls; write costs were not measured in this comparison.
Wasm SHA-256 was `4c02ba76ef91fd6cfe13651653d449f5a6970640636bf44d8672916af476c234`
for three indexes and `efd4826849f3d448788c0c5f6a2fe13dd4b1a92e96a87af4d0897d8fbed9f365`
for eight.

```sh
make m2-sort-costs-artifact
IC_DB_M1_RECORDS_WASM="$PWD/qualification/target/evidence/m2/sort-costs.wasm" \
  bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml \
  --locked -p ic-db-m0-rollback-driver --test console_aggregates \
  literal_text_scan_cost -- --ignored --nocapture
```

#### Write cost of visited-index validation

The existing `write_envelope` probe ran on fresh PocketIC instances against the
same pre-change and post-change eight-index fixture binaries identified above
(SHA-256 prefixes `32641774` and `efd48268`). This workload uses `EnvelopeRow`,
which has eight indexes in both binaries. Each successful call operates on 32
rows. Instructions include database open, batch construction and commit.
Cycles are the canister balance change across the whole update call.

| Operation | Instructions before | Instructions after | Cycles before | Cycles after |
| --- | ---: | ---: | ---: | ---: |
| Insert | 144,470,624 | 144,470,031 | 151,483,083 | 151,482,490 |
| Identical replacement | 25,150,445 | 25,149,712 | 32,162,762 | 32,162,029 |
| Changed replacement | 329,113,557 | 329,112,056 | 336,125,991 | 336,124,490 |
| Delete after upgrade | 190,807,052 | 190,806,389 | 197,820,137 | 197,819,474 |

Successful write instruction costs differed by less than 0.003%. The 33-row
and oversized-row refusals each used 35 additional instructions and 35 additional
cycles. Each of the eight typed index-prefix reads returned the same 32 IDs and
revision, with instruction reductions ranging from 4.26% to 4.87%. Both runs
passed the independent raw row/index checks and verified byte-for-byte backing
preservation after refusal, identical replacement, a post-commit trap and upgrade.

Run the following against each retained Wasm binary with
`IC_DB_M1_RECORDS_WASM` set to its path:

```sh
bash scripts/with-pocketic.sh cargo test --manifest-path qualification/Cargo.toml \
  --locked -p ic-db-m0-rollback-driver --test write_envelope -- --nocapture
```

### SQLite reference checks

Run `make m2-sqlite-reference` for native differential checks. The same suite
runs through `make m2-queries`. The qualification-only crate seeds an in-memory
SQLite database and IC DB with the same rows. SQLite is bundled with the test
crate and introduces no product or CLI dependency.

- Compare automatic whole-entity, category and sparse-index counts before and
  after a mixed write batch, a refused batch and reopen. Check individual keys
  when one row releases a prefix and another enters it at the same price.
- Compare both directions of a compound-index listing against SQLite's ordered
  IDs. Replay every observed cursor against the expected suffix, including ties,
  sparse empty pages and a matching row deferred by the output-byte limit.
- Compare synchronous and retained first-occurrence DISTINCT with the first qualifying
  position for each price in SQLite. Exercise forward and reverse compound-index
  traversal, tied ranks, absent prices and one-candidate steps with single-key pages.
- Compare grouped sums and non-null counts with SQLite using exact integer
  cents. Hand-calculated IC DB averages at scale two are 1.24, 1.26 and absent.
  Check synchronous results and reopen both stores between retained steps with
  work limits of one, three and five candidates.

The frozen predecessor also checked cursor suffixes and used a native SQLite
reference for shared semantics. [IcyDB continuation regression](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3/crates/icydb-core/src/db/session/tests/cardinality_tiebreak/seek_intersection.rs#L195) ·
[IcyDB reference tests](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3/crates/icydb-core/src/db/session/tests/tier_c_reference.rs#L257).

### Document replacement costs

Matched PocketIC runs edited the seeded Document through the ordinary
`replace_document` update on the three-index fixture. Each run checked the
stored fields and one revision advance after every edit. The balance delta
covers the update call, with verification reads outside the interval.

| Changed fields | Index entries changed | Cycles before | Cycles after |
| --- | ---: | ---: | ---: |
| Note | 0 | 8,648,870 | 7,894,293 |
| Title | 1 | 8,656,281 | 8,174,417 |
| Title, score and priority | 3 | 8,651,964 | 8,649,828 |

Before Wasm SHA-256 `4c02ba76ef91fd6cfe13651653d449f5a6970640636bf44d8672916af476c234`;
after `72079ba445366c2e698d328001b4a8a72389385ce83eb32c8711e4554cf3f7c4`.
The 32-row write-envelope probe on both artifacts verified raw index and row
bytes, refusal, trap rollback, upgrade and deletion. Inserts and identical
replacements used the same instructions in both runs.

```sh
make m2-sort-costs-artifact
IC_DB_M1_RECORDS_WASM="$PWD/qualification/target/evidence/m2/sort-costs.wasm" \
  bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml \
  -p ic-db-m0-rollback-driver --test console_aggregates document_replacement_costs -- --ignored --nocapture
```

### Three-engine performance matrix (0.23.4)

[Full 1,188-row CSV](measurements/0.23.4-three-engine.csv), measured November 18, 2026.
The benchmark compares the IC DB worktree, frozen IcyDB 0.310.2 at `6d421e3`
and bundled SQLite 3.54.1. All use the same three integer fields, ID primary key
and secondary index on (category, ID). Balanced and skewed data each run at 32,
128 and 511 rows, with the distribution and filter parameters specified in the
[broad query matrix](#broad-query-matrix-019).

**Balanced 511-row fixture.** Native columns are median microseconds. IC columns are settled instructions in millions. Inserts were sampled while growing from 128 to 511 rows.

| Operation | IC DB µs | IcyDB µs | SQLite µs | IC DB M instr. | IcyDB M instr. |
| --- | ---: | ---: | ---: | ---: | ---: |
| Insert one row | 22.52 | 170.25 | 10.12 | — | — |
| Replace amount only | 22.05 | 154.04 | 13.65 | — | — |
| Replace category and amount | 30.00 | 208.63 | 13.50 | — | — |
| Primary hit (ID 255) | 3.19 | 23.23 | 2.25 | 0.64 | 1.28 |
| Primary miss | 3.67 | 22.04 | 1.98 | 0.74 | 1.25 |
| First ten in category 3 | 31.46 | 121.23 | 10.61 | 1.89 | 5.00 |
| Missing category listing | 3.06 | 46.69 | 5.65 | 0.80 | 2.16 |
| Whole-entity count | 2.66 | 28.31 | 1.41 | 0.63 | 1.42 |
| Category 0 count (IC DB validates owners) | 369.62 | 32.88 | 5.64 | 15.29 | 1.50 |
| Missing category count | 3.65 | 32.19 | 2.88 | 0.83 | 1.51 |
| Ascending category 3, minimum 950, first eight | 376.06 | 402.62 | 28.39 | 15.43 | 15.11 |
| Descending category 3, same filter | 389.79 | 404.26 | 29.21 | 16.00 | 15.35 |
| Compound order, minimum 500, first eight | 50.81 | 168.33 | 17.76 | 2.38 | 5.85 |
| Sort category 0 by amount, minimum 500, first eight | 162.53 | Refused | 32.12 | 7.67 | Refused |
| Group count | 191.52 | 1,104.04 | 81.69 | 8.72 | 40.68 |
| Group sum | 195.75 | 1,252.09 | 88.51 | 8.83 | 44.30 |
| Group minimum | 208.94 | 1,190.20 | 93.47 | 9.34 | 41.94 |
| Group maximum | 206.34 | 1,189.85 | 90.94 | 9.33 | 41.94 |

The balanced category-3 listing above has 128 candidates in its category/ID
index range. The fixture formula puts the eighth qualifying row at candidate
121 ascending (ID 483) and 124 descending (ID 19). These positions are derived
from the source tuples. The selected index supplies the requested order, while
the amount threshold still requires checking candidate rows. The 0.23.7
primary-equality change applies to requests specifying one exact ID. Neither
listing specifies an ID.

For prefix cardinality with no row predicate, the [matched count paths](#prefix-count-paths-0234)
also measure IC DB’s `count_index_entries()` on these populations.

**Reading the CSV.** Each population/distribution has 195 read cases and three
writes. Native columns are median nanoseconds per call. The last four columns
are PocketIC instructions before and after upgrade. Blank cells have no sample.
Legacy refusal 174 means its fluent page API refused a sort requiring
materialization. The result from each successful native query was compared with
SQLite; the independent PocketIC suite checked results against source tuples.

- `lookup-ID`, `prefix-C` and `count-C` are primary lookup, first ten indexed
  rows and exact count respectively. `all` counts the full entity. The IC DB
  populated-category count verifies owner rows; legacy maintains prefix counts.
- `listing-M-cC-minV-limitN` filters by minimum amount. Modes 0 and 1 list the
  category by ID ascending and descending. Mode 2 lists all categories in
  category/ID order. Mode 3 sorts the category by descending amount, then ID.
- `group-M-cC-minV` groups by category after filtering. Modes 0 through 3 are
  count, sum, minimum and maximum. The `all` category includes every row.
- Writes measure single-row insert, amount-only replacement and replacement of
  both category and amount. Each replacement was read back before restoring
  the original row outside the timing interval.

**Native method.** Release profile, Rust 1.100.0, Linux x86-64, AMD Threadripper
7970X. The engines used in-process memory backing and an existing database or
connection. Legacy entered its request/session boundary on each call and
explicitly settled startup/recovery after writes. SQLite executed an autocommit
write or prepared, ran and collected a query per call. SQL text construction
was outside the interval. IC DB and legacy used their typed APIs. Reads had one
correctness warm-up and nine timed samples per case, rotating engine order.
Replacements used nine timed samples. Insert medians cover the rows added
between successive population sizes. Native timings include result collection
and exclude transport, result comparison and simulated IC execution. The write
times cover memory-backed operations; disk sync and power-loss durability were
outside the workload.

**PocketIC method.** Both IC engines ran 4,680 reads in total. Among 1,908
successful pairs, IC DB used fewer instructions in 1,668 and legacy in 240.
There were 432 legacy sort refusals. At 511 skewed rows, counting category
zero (415 matching owners) took 48,723,250 IC DB instructions and 1,555,756
legacy instructions before upgrade. SQLite ran natively only. The IC DB query
host measured 357,338 uncompressed Wasm bytes versus 3,297,394 for legacy.
Initial startup took one call and 7,382,507 cycles for IC DB versus three
calls and 32,850,054 cycles for legacy on this host. These figures cover the
selected query endpoints and startup policy. Retained aggregates, migration,
filesystem-backed SQLite and canister-scale concurrency were outside this run.
The [matched write measurements](#recorded-matched-measurements) retain their
own workload and revision.

Reproduce from the repository root:

```sh
export IC_DB_QUERY_MATRIX_DIR="$PWD/qualification/target/evidence/query-matrix-0234"
make m2-query-matrix
IC_DB_NATIVE_MATRIX_CSV="$IC_DB_QUERY_MATRIX_DIR/native.csv" \
  cargo test --release --locked --manifest-path qualification/Cargo.toml \
  -p ic-db-icydb-baseline-canister --test native_matrix -- --ignored --nocapture
```

The native CSV and the PocketIC `queries.csv` share the profile, population
and case keys. The Wasm SHA-256 values for this run are
`834ab2926dfb5172239b6c5b45a274f3f2fdb2329bc9858f8ba611718019a5fc` (IC DB)
and `72e52c1c4834e8e522168f00b047965150ade37cd2970aae3862f225739d4fac` (legacy).

### Prefix count paths (0.23.4)

`make m2-count-paths` runs the settled and post-upgrade count probes against
the same 32-, 128- and 511-row distributions as the three-engine matrix.
The original IC DB profile measured both terminals in one Wasm, including database open.
Legacy uses `execute_exact_count()` on its maintained prefix metadata. The
driver checks exact answers against the fixture tuples and stable memory
before and after each set of queries. [All 36 comparisons](measurements/0.23.4-prefix-counts.csv)

| 511-row fixture | Matches | IC DB validated count, instructions | IC DB index-entry count, instructions | Legacy maintained count, instructions |
| --- | ---: | ---: | ---: | ---: |
| Balanced, category 0 | 127 | 15,313,662 | 1,742,488 | 1,501,147 |
| Skewed, category 0 | 415 | 48,821,714 | 3,801,711 | 1,555,246 |
| Skewed, category 3 | 32 | 4,677,749 | 1,032,490 | 1,562,617 |
| Skewed, missing category | 0 | 829,540 | 823,634 | 1,560,949 |

For 415 matches the index-entry path saves 92.2% of the validated count's
instructions, with 2.44 times the instructions of legacy’s maintained count.
The former visits one index entry per match; the latter pays counter maintenance
on writes. Both IC DB paths use a 512-candidate cap here. The native
`index_entry_counts_are_bounded_and_independent_of_owner_rows` regression also
checks exact-boundary refusal and a dangling entry. The different benchmark
Wasm and timing boundary account for slightly different absolute readings
from the broad matrix. IC DB Wasm SHA-256
`7a2e25d1f090aa59b8e5261c0b33d4b13c0fa86a79cbb2e27985d68e1de6dadc`;
legacy `88fc1aa3a228ecde1458842a461c6a19612c5b9cf630e7899b8debfe599c491c`.


### Indexed existence (0.23.7)

Run `bash scripts/measure-records.sh exists` to compare a category equality
lookup against frozen IcyDB `6d421e3`. IC DB calls fluent `.exists()` with a
one-candidate work limit and validates the owner on a hit. The legacy typed
query uses `.execute_exact_count() > 0`, which reads maintained prefix
metadata. Both handlers include database opening and return a boolean.
The instruction counter excludes transport framing and fixture updates.

The matrix checks categories 0, 3 and 99 against the original input tuples at
32, 128 and 511 rows on balanced and skewed data, before and after upgrade
settling. All 36 pairs agreed and stable backing was identical across each
group of reads. IC DB used 37–60% fewer handler instructions across these
pairs. The settled 511-row skewed fixture measured:

| Category | Matching rows | IC DB instructions | Frozen IcyDB instructions |
| --- | ---: | ---: | ---: |
| 0 | 415 | 966,946 | 1,554,918 |
| 3 | 32 | 945,783 | 1,562,289 |
| 99 | 0 | 831,164 | 1,560,621 |

[All 72 observations](measurements/0.23.7-indexed-existence.csv). Wasm SHA-256
was `2972e6b4b878d82de272da56a94314f624d2550d16789421333ac8ff02954e02` for
IC DB and `18f65881f60bb0d77642bdf656f4112a233e026c8376aa05af72b5c264d0d528`
for IcyDB. Exact counting still has different scaling because it must establish
the total number of matches.

### Automatic prefix counts (0.23.5)

The count-path workload also measures `.filter("category", Eq, category).count()`
without an index hint. All 36 combinations of population, distribution, category
and upgrade phase returned the tuple-oracle answer and preserved stable memory.
Each IC DB measurement includes opening the database. The automatic path used
4,242–4,302 more instructions than explicitly counting index entries on the
same Wasm. [Matched results](measurements/0.23.5-automatic-counts.csv)

On the settled 511-row skewed fixture, counting 415 matches used 3,805,989
instructions automatically, 3,801,747 with the explicit index prefix, 48,841,362
with owner validation and 1,555,246 with legacy maintained counts. With 32
matches the automatic path used 1,036,816 and legacy used 1,562,617.
The frozen engine revision remains the one recorded above. This run used
Storage 0.78.2 and qualification transport Wire 0.25.0.

Reproduce with `make m2-count-paths`. IC DB Wasm SHA-256
`b917785f39292a806bf28d34e75a0ee5f871f2bd0be38d534273ce33f3b5a984`,
legacy `d50e4e97e0676f81e1892f64681cdd34e81761f1f9d0d9b1e531ac52ede6904b`.

### Primary-key range count costs (0.23.5)

`make m2-count-profile` measures `1 <= id < upper` on the same installed IC DB
Wasm using `.count()` and `.count_validated()`. The driver checks both results
against the inserted tuples and checks stable memory after every call. The 36
matched observations include 32, 128, 256 and 512 stored rows, with before/after
upgrade pairs at 32 and 512 rows. [Matched observations](measurements/0.23.5-primary-range-counts.csv)

| Matching rows after upgrade (512 stored) | Automatic count, instructions including open | Validated count, instructions including open |
| ---: | ---: | ---: |
| 1 | 662,235 | 670,574 |
| 32 | 823,831 | 1,045,533 |
| 256 | 2,237,370 | 3,977,700 |
| 511 | 3,886,755 | 7,352,708 |

Opening the database cost 531,712 instructions for both paths in these calls.
At 511 matches, skipping row validation saved 47.1% including opening. At
512 matches both paths returned `Limit` at the 512-candidate work bound.
The measurements cover Nat64 primary keys and scalar rows on Storage 0.78.2.
The counters cover database opening and query execution inside the endpoint.
Wasm SHA-256 `b67f0d8d87ac327cdd1dd849ff63ca84f02690f6796e5faa45af650d130e62af`.

### Filtered ordered listing with automatic index selection (0.23.5)

Run `make m2-listing-costs`. The native SQLite fixture replays every observed
cursor in both directions with and without an index hint. The installed PocketIC
fixture selects published documents for entry 1, ordered by score descending
and then ID descending, with a 20-ID page limit. Scores repeat. The fixture
includes a published, higher-scoring document belonging to entry 2.

Both fluent calls specify the same filters and order. One additionally selects
`using_index("entry_id,score", &[1_u64.into()])`. Each consumes its own cursors.
SQLite independently supplies the complete ordered ID sequence. Every page
matched its corresponding slice, and both calls reported identical candidate
and fetch counts on all 49 page pairs. All three traversals returned 40 IDs.

| Work / byte budget per page | Pages | Empty pages | Candidates examined / rows fetched | Automatic instructions, total | Hinted instructions, total |
| --- | ---: | ---: | ---: | ---: | ---: |
| 128 / 160 | 3 | 1 | 129 / 129 | 17,860,706 | 17,823,498 |
| 5 / 160 | 26 | 10 | 129 / 129 | 28,956,460 | 28,695,886 |
| 32 / 16 | 20 | 0 | 147 / 147 | 27,683,890 | 27,537,576 |

The five-candidate cap produced empty pages with advancing cursors. At 16 bytes
only two IDs fit. Replaying the same cursor with more space exposed the next
matching ID, and the narrow traversal subsequently returned it. Repeated visits
are included in the totals. The extra wide-page checks are outside the totals.

Instructions include opening the database, constructing and executing the query,
and collecting IDs and the continuation inside the measurement endpoint. The
counters exclude transport framing. The canister's stable bytes matched before
and after the workload. The automatic path used 0.21–0.91% more instructions
over a full traversal on this fixture. [Per-page measurements](measurements/0.23.5-ordered-listing.csv)

The cursor-suffix check follows frozen IcyDB's ordered-seek regression at
`6d421e3`. IC DB uses its existing compound-index traversal and a residual
visibility filter. The measurement Wasm SHA-256 was
`d12ea9a673031c6e7c4caf62fecc42316a097765479f1dd8e0818bd03ca8ea57`.
[IcyDB cursor regression](https://github.com/dragginzgame/icydb-legacy/blob/6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac/crates/icydb-core/src/db/session/tests/cardinality_tiebreak/seek_intersection.rs#L195)

### Filtered top 20 with and without an ordering index

`make m2-listing-costs` also selects published documents for entry 1 with
`id <= 100`, ordered by score and ID, both descending. Every eligible document
carries the same score in the indexed `score` and unindexed `previous_score`
fields. Using two fields keeps the population, executable and projection fixed.
The second field is optional and present on every eligible row. SQLite supplies
the expected top 20 independently. The indexed page has a live continuation.
Complete sorting returns a terminal top 20 after examining the primary range.

| First 20 IDs, work 256, output 160 bytes | Candidates | Owner rows fetched | IC instructions | Repeat in reverse call order |
| --- | ---: | ---: | ---: | ---: |
| Compound index | 87 | 87 | 12,320,176 | 12,320,136 |
| Complete sort | 100 | 100 | 2,197,651 | 2,197,611 |

With work 1, the index returned an empty page with a continuation after its first
nonmatch. Complete sorting returned `Limit`. Both successful paths matched
SQLite and all reads preserved stable memory. Counters cover opening the database
through result construction on a warm PocketIC canister. These measurements
cover the first 20 IDs only. They do not isolate the cost of owner lookup or
establish a general planner threshold. Measurement Wasm SHA-256:
`1ca1c863788a5d5d647056770dd116a2cb54b47fe9bf481ff0eae15473cda882`.

### Owner lookup cost as listing selectivity changes

The same target varies the inclusive ID cutoff while keeping the dataset,
compound index, order, projection and page budgets fixed. SQLite checks the IDs
and tie order. “Eligible” counts all matching rows; the first page returns at
most 20. Each examined index entry fetched one owner row.

| ID cutoff | Eligible | Returned | Candidates / owner fetches | Total IC instructions (millions) | Owner lookup alone (millions, separate probe) |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 20 | 6 | 6 | 129 | 17.57 | 12.80 |
| 60 | 18 | 18 | 129 | 17.60 | 12.80 |
| 100 | 31 | 20 | 87 | 12.31 | 8.89 |
| 130 | 40 | 20 | 80 | 11.35 | 8.20 |

A temporary instrumented Wasm timed four regions inside the shared scanner.
At cutoff 100, traversing the index and reading its owner key took 0.90M
instructions. Building the row key and fetching its bytes from the stable map
took 8.89M. Inspecting row fields and checking the primary identity took 0.47M.
Reconstructing and validating the index entry took 1.46M. The full instrumented
call cost 12.48M, including all four regions, the counters and the rest of the
query. The ordinary call cost 12.31M. Across the four instrumented calls,
owner lookup accounted for 71–72% of measured call instructions.

These are warm reads on this fixture. The sweep changes both matching density
and where matches occur in traversal order. The two cutoffs that exhaust the
range isolate the effect of returning different numbers of matches at the
same fetch count. The instrumented Wasm SHA-256 was
`0e10391822a1d6b93469efc50dbea8bff97c056303346ec93a41d9cd5a80812c`.

### Matching both equality filters in the listing index

The [compound-index experiment](experiments/listing-compound-index.patch) adds
`(entry_id, published, score)` to the qualification schema alongside the existing
`(entry_id, score)` index. Both indexes live in the same PocketIC canister and
cover the same 130 documents. Fixing entry and publication state leaves score
and the primary-key suffix in the requested descending order. The ID cutoff
still applies during traversal.

| ID cutoff | Returned | Owner fetches, old → automatic | Old index, M instructions | Automatic, M instructions |
| ---: | ---: | ---: | ---: | ---: |
| 20 | 6 | 129 → 40 | 19.28 | 6.15 |
| 60 | 18 | 129 → 40 | 19.42 | 6.18 |
| 100 | 20 | 87 → 27 | 13.34 | 4.32 |
| 130 | 20 | 80 → 20 | 12.36 | 3.28 |

Automatic selection matched the explicit three-field hint in examined and
fetched counts for every cutoff. At cutoff 100, all 59 page pairs across three
work/output budgets matched SQLite, including ties, empty pages with progress
and retry at a byte boundary. Reads preserved stable memory. Ordinary owner
validation ran on each visited row. Bounded sorting cost 2.21M instructions
for the first 20 at that cutoff and produced a terminal result.

To repeat in a disposable checkout containing this experiment:

```sh
git apply qualification/experiments/listing-compound-index.patch
make m2-listing-costs
git apply -R qualification/experiments/listing-compound-index.patch
```

### Write and storage cost of the additional listing index

Run `make m2-listing-costs` once with the ordinary schema and once with the
experiment above. After the listing checks on 130 documents, each run inserts
document 131, changes its note, score and publication flag in separate calls,
then deletes it. Each timing starts before database open and ends after commit.
Replacements include the application read. A subsequent lookup checks the
stored values or absence outside the timed interval. The driver repeats the
lifecycle three times, reusing the same key. [Individual instruction counts](measurements/0.24.4-listing-index-writes.csv)

| Single-row operation | Ordinary schema, median M instructions | With additional index, median M instructions | Increase |
| --- | ---: | ---: | ---: |
| Insert | 1.404 | 1.991 | 42% |
| Replace note | 1.400 | 1.679 | 20% |
| Replace score | 1.680 | 2.375 | 41% |
| Toggle publication | 1.701 | 2.376 | 40% |
| Delete | 1.874 | 2.113 | 13% |

The note belongs to no index. Replacement still verifies the old index entries
during preparation and leaves each unchanged entry in place. The timings include
all work of the write and can vary with the map layout across rounds.

Both fixtures occupied four 64 KiB stable-memory pages immediately after install.
The ordinary schema occupied five pages after seeding, and the additional index
brought the total to six. Each retained its seeded page count after the final
delete. The added index has 130 non-unique entries. Its encoded key/value payload
is 42 bytes per document (9 header, 17 field components, 8 owner suffix and
8 owner value), or 5,460 logical bytes before storage overhead. Physical growth
reflects page boundaries and the layout of the whole fixture.

Across the two schemas the automatic first page at cutoff 100 saved 7.99M
instructions (12.31M to 4.32M). The corresponding median score replacement
cost an additional 0.69M. These are measurements at this population and filter
distribution. Larger datasets, different write mixes and update batches require
their own comparison.

### Replacing the listing index

The [replacement experiment](experiments/listing-replace-index.patch) removes
`(entry_id, score)` and assigns the document reference to
`(entry_id, published, score)`. Apply it on top of the compound-index patch
in a disposable checkout and run `make m2-listing-costs`. Other fixture indexes
stay in place. All three schemas used the same 130 documents and the same
automatic queries. At ID cutoff 100, both listings request the top 20 by
descending score and descending ID.

| Listing indexes | Published only, fetches / M instructions | Both states, fetches / M instructions | Both states, next page | Seeded stable pages |
| --- | ---: | ---: | --- | ---: |
| Original | 87 / 12.31 | 28 / 4.30 | Live cursor | 5 |
| Original and wider | 27 / 4.32 | 28 / 4.79 | Live cursor | 6 |
| Wider only | 27 / 4.26 | 100 / 2.71 | Terminal top 20 | 5 |

With publication fixed, the wider index puts all matching scores in order.
Across publication states it orders documents by publication first. The
planner used the bounded sort to produce SQLite's global top 20. With a
one-candidate work limit this returned `Limit`. With the original index present,
the next cursor page also matched SQLite. The explicit hint naming the removed
index returned `InvalidQuery`.

The wider index retains the leading entry ID needed for restricted deletion.
The replacement schema accepted the setup batch that repointed the last
dependent and deleted its former parent. Deletion of a parent with surviving
documents then returned `ReferenceViolation` and preserved stable bytes.
These checks install fresh schemas; they do not qualify migration of existing
data or named-index callers outside this workload.

The wider-only schema used the same number of physical pages as the original
on this dataset. Its median single-row score replacement cost 1.73M instructions
(original 1.68M, both 2.38M). Toggling publication cost 2.23M (original 1.70M,
both 2.37M). The [write samples](measurements/0.24.4-listing-index-writes.csv)
include all three replacement runs.

To reproduce the wider-only case, apply the compound-index patch followed by
the replacement patch in a disposable checkout, run `make m2-listing-costs`,
then reverse the patches in the opposite order.

The native schema-copy suite also exercises the same index replacement on a
populated two-entity fixture. An explicit transition copies the first document,
reopens the checkpoint, copies the remaining three and then copies their parent.
Both allocations refuse ordinary access during the interruption. After bounded
validation and publication, the reopened target contains the same five raw row
keys and values. A fresh cursor pages the published documents in descending
score/ID order, including a score tie; the saved source cursor is refused.
Restricted deletion succeeds only after the batch removes the four dependents.

Run `cargo test --locked -p ic-db --test schema_copy populated_listing_index_replacement`.
The three-schema cost comparisons above each start from a fresh installation.

The installed maintenance suite also exercises this replacement using the
eight-field qualification `Document` and its `Entry` parent. After the first
document is copied, PocketIC installs a Wasm that declares a different
transition version for the same source and target schemas. Resume refuses
with `InvalidTransition`, the listing returns `Maintenance`, and the refused
calls preserve stable memory. Reinstalling the supported Wasm copies exactly
the three remaining documents and the parent. The client attempts a listing
after each bounded validation call until publication admits reads.

A further upgrade reopens the published target. Its listing refuses the saved
source cursor, returns IDs 3 and 1 (tied at score 7), then ID 4. The unpublished
document at score 99 is excluded. Deleting the referenced parent refuses and
the reads and refused delete leave stable memory intact. This probe runs in
`make m3-maintenance` as `listing_index_replacement_crosses_installed_upgrades`.

The companion PocketIC case, `cancelled_listing_index_restores_the_source_cursor_after_upgrade`,
starts cancellation with one target entry removed per message. At that point the
source cursor gets `Maintenance` and attempts to resume copying get
`InvalidTransition`. An executable declaring a different transition version
also refuses to advance cleanup. After restoring the supported executable, the
driver attempts the original cursor after each cleanup chunk. Its first successful
read returns the remaining document, ID 4, and finishes enumeration. A further
upgrade preserves that result and the discarded target still refuses ordinary
access. This covers cancellation before publication with the source frozen
throughout copying and cleanup.
