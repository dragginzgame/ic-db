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

## Standalone endpoint hooks

The first M2 fluent-read probe is selected separately:

```sh
make m2-queries
```

It exercises exact scalar filters, primary/index-prefix traversal, reverse order,
empty sparse pages, byte-limit retry, oversized-row refusal, cursor identity and
unchanged-schema upgrade. Native tests additionally cover duplicate secondary keys,
live key updates, deleted cursor rows and corruption hidden behind rejecting filters.
The Wasm probe advances virtual time before upgrade because its preceding query
calls do not drain the IC install-code rate limit. No production setting is changed.
Projection checks reuse named records, cover optional/nested output, exact
transitive type admission, unselected corruption and projected-byte retries. The
Wasm probe resumes a projected cursor after upgrade, rejects it for entity output,
and compares full decoding against ID-only projection over a 1024-byte blob row.
The local CLI adapter now uses these fluent operations for Entry and Document;
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

The blob witness returns identical IDs with 1,037 encoded bytes for full entities
versus 8 for projected records. Its measured execution after opening is 74,795
versus 74,464 instructions. This is selective materialization, not partial storage
I/O. Both paths validate all input bytes. The complete fixture grows by 9,871 total
and 9,406 code bytes, including two new endpoints and their output codecs; this
does not isolate executor or per-output-type growth. Projection-increment fixture SHA-256:
`fcd716b40d9a7858d75bad7898476517addf419554399dc8aa08f22719a29225`.

The scalar benchmark isolates shared-code overhead, with fluent execution
unreachable: 224,548 total/203,915 code bytes, SHA-256
`091b9f0f02c298036443c8b383b8e64fe83342de2bf719187296fb48506c4c54`.
Projection support adds 14 total/code bytes; hit/miss use 471,728/477,249 instructions
(-1 each), direct listing 771,661 (-1), and read/modify/replace 828,622 (+25,
about 0.003%) with 7,842,397 cycles (+25).
The shared borrowed reader fills its bounded field view in place; a temporary
copy found during measurement was removed. These costs remain below the matched
IcyDB ceilings. No direct-to-fluent performance parity is claimed.

All six canister hosts use Platform's generic `guard`, `decode_with` and
`encode_with` hooks with the [fixed fixture codecs](transport/src/lib.rs), shared
through one private qualification crate with ordinary Cargo dependencies. No Canic or
Wire Endpoint dependency is needed. The [standalone example](rollback-canister/src/entrypoints.rs)
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
This implements [IC DB issue #1](https://github.com/dragginzgame/ic-db/issues/1)
locally; the frozen IcyDB engine and Canic sources are unchanged.

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
Its [shared declarations](records-schema/src/lib.rs) are a private qualification
crate used by both the [host build script](records-canister/build.rs) and Wasm
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
below; signed primary keys remain unimplemented.
The separate WideNumbers fixture checks required/optional 128-bit integers,
including signed extrema, unsigned maximum and values just beyond 64 bits.
Independent 16-byte scalar vectors and schema hashes qualify encoding;
truncation, surplus bytes and incompatible width/signedness refuse. Native and Wasm
checks preserve all bits across writes, no-ops, traps and reopen. Wide secondary
indexes are qualified below; wide primary keys and arithmetic remain unimplemented.
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
The locked Testkit is 0.106.0 with PocketIC 16.1.0. Calls use raw client methods
and this workspace's explicit Wire codec/budgets; no `ic-testkit-wire` adapter is
required. Direct benchmark inspection uses `ic-host-artifacts` 0.79.0; the CLI
uses `ic-host-text` 0.76.0. Testkit's Artifacts/FS/Process dependencies also
resolve to 0.79.0, with one Artifacts version. Tests use fresh instances and do
not need the reusable pools retired in Testkit 0.105.0 or the diagnostic reports
retired in 0.106.0. The CLI uses Text's retained escaping/width/table APIs, not
its removed domain formatters. No host package enters
the database product or deployed Wasm.
The adoption was qualified with native/Wasm Clippy, record/default/generator and
endpoint-boundary Wasm tests, the schema fingerprint oracle and the matched scalar
comparison. Earlier probe runs retain their own dependency evidence; this refresh
does not relabel every historical M0 result as a new run.

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
`status`, `query [Entry|Document]`, `help` and `quit`. Entry is the default.
Queries use the fluent product engine. Entry supports `--min` and `--category`;
Document supports `--entry-id`. `--order-by FIELD[,FIELD]` selects a proven complete
order, `--desc` reverses it, and `--select id` or `--select id,amount` uses typed
projection (the latter is Entry-only). Full entities are the default result.
Entry also supports `--select category`; combine it with
`--order-by category --distinct` for the ordered-DISTINCT path. `--category` is
supported; `--min` varies within a category and refuses with this projection.
Full entities and ID projections are inherently unique.
`--select count` counts matching rows; Entry also supports `--select category,count`.
`--select count:FIELD` counts present field values (for example, Document's `note`).
Entry supports `--select distinct-count:category` and `--select category,count:FIELD`.
These are fixed typed projections and direct fluent terminals in the private
adapter; field presence uses the ordinary schema admission.
For aggregates, `--limit` selects completed groups and no continuation is returned.
They refuse `--after`, `--distinct`, explicit ordering and execution that reaches
`--work` before observing completion; an exact-boundary scan can also refuse.
These synchronous operations do not represent resumable aggregate support.
`--limit` bounds output rows, `--work` bounds candidates and `--bytes` bounds encoded
output. Their defaults are 3, 128 and 4096. An empty page may still continue.
The CLI prints a hexadecimal next cursor or `none`; copy it to `--after` with the
same entity, filters, order and projection. Numeric after-ID syntax is retired.
A full final page may need one last empty call. Both schema information
and records come from the running fixture's query controls. Schema responses carry
structured field/index descriptions; only the native CLI depends on `ic-host-text`
and renders bordered tables. Full storage identities live in `db status`.

Use `db query Entry --category 1 --order-by id --desc` for one command, or pipe
newline commands to `db`. `make m2-cli` checks piped/one-shot use, cursor reuse and
query-shape refusals, projected bytes, work-exhausted pages, both entities and help.
The old post-page-filter fixture endpoint is removed; mutation/growth probes now
read through the fluent witness. CLI qualification uses the concurrently selected
Wire 0.22.3 transport lock; product dependencies and persisted bytes are unchanged.
`db --help` needs no running server. The installed binary locates the fixture
relative to itself and works outside the repository directory. Testkit's prepared
server is still tied to the build checkout; run `make m0-setup` there before use.
Invalid commands/options fail scripts with a nonzero status;
interactive mistakes return to the prompt. The console never evaluates SQL or
shell commands. `quit` or EOF closes the Testkit-owned instance. Local fixture
endpoints are qualification controls, not a shipped Canic adapter or a new public
dynamic-query API; there is no remote connection or durable CLI session.
