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
batches with required/optional integer or Ulid references and indexed restricted deletion. This is
experimental: there is no production compatibility promise or deployment.
The [first matched scalar comparison](docs/design/ic-db-system-design.md#first-matched-scalar-comparison)
measures a smaller host and lower operation costs than frozen SQL-disabled IcyDB
for one 32-row workload; it does not establish full-product parity or savings.
Rich types, broader references and the broad fluent builder remain required
implementation work; this increment does not complete M1. Secondary indexes now
support Nat/Int8/16/32/64/128, U256, Ulid, Date, Timestamp, Duration, Decimal,
finite Float32/Float64, Account/Subaccount,
Principal, Bool and bounded Text fields, including
optional values, compound keys and uniqueness. Optional unique indexes explicitly
require `present_only`; ordinary indexes retain absent values. Primary keys support supplied signed/unsigned 8–128-bit integers, U256 and Ulid;
references must match the target’s exact key type.
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
[shared schema](qualification/records-canister/src/schema.rs) and its
[build script](qualification/records-canister/build.rs).
Records use `#[record(fields(...))]` and enums use
`#[enum_(variant(name = "Ready"), variant(name = "Data", value(item(is = "Details"))))]`.
Both retain the empty-struct authoring style. Fields use `value(item(is = "Type"))`;
optional values add `opt`. Recursive values can use `item(indirect, is = "Self")`
and need a finite base case. Encoding, decoding and stored-row inspection share
32-level, 256-structured-value and 4 KiB row bounds. Lists use
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
Keys are ordered scalars; values may also be lists or named types. Stored duplicate
or descending keys refuse without overwriting values. Map entries share the row's
collection budget, and their nested contents consume the same budget. Inspect
`MapRow`/`Rates` and recursive `MapTree` with `db schema`.
`value(item(prim = "U256"))` uses `ic_db::U256` (ethnum), with exact 32-byte
big-endian storage and secondary-index keys. Optional values and collection
elements/keys retain all 256 bits. Inspect `BigNumbers` with `db schema`;
aggregate arithmetic remains separate work.
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
policies and aggregate arithmetic remain separate work.
`value(item(prim = "Decimal", scale = 2))` stores exact decimal values at the
declared scale (0–28). Parse `"1.00".parse::<ic_db::Decimal>()` or use
`Decimal::try_new(mantissa, scale)`. Equivalent values such as 1.0 and 1.00 have
the same equality, hash and index key. Writes refuse excess precision or
coefficient overflow; they never round. Inspect `DecimalRow` with `db schema`.
Decimal aggregate arithmetic remains separate work.
Enum/nested/collection indexes, nested references and non-scalar keys/set elements remain work.
IC DB owns its durable row codec and has no IC Wire dependency. Transport format
changes cannot change persisted decoding. The current experimental codec identity
is version 3, adapting IcyDB's owned fixed-width scalar approach; opening a
version 1 or 2 codec contract refuses without changing its data. The named schema
grammar is version 2; earlier numbered contracts also refuse intact.
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

At the `db>` prompt, use `schema`, `status`, `query --limit 3`,
`query --after 3 --limit 2 --min 40`, `help` or `quit`.
For one command: `db schema` or `db query --limit 2`. Use `db --help` for help.
Installation defaults to `~/.local/bin/db`; keep `~/.local/bin` on `PATH`.
Override the prefix with `make db DB_INSTALL_PREFIX=/your/prefix`.

The current console reads an explicitly labelled, temporary M1 database with six
`Entry` rows, one record each for `Document`, `WideNumbers`, `BinaryObject`, `NarrowNumbers` and `IdentityRow`,
and an empty `OptionalLink` entity showing an optional self-reference,
running in local PocketIC. Each session starts fresh and is discarded on exit.
`db schema` presents generated fields and indexes in bordered tables, with the
primary key first and separate Type, Optional and Info columns. Info contains
byte/item/entry limits, including list-element bounds; type names stay uncluttered. The CLI
uses `ic-host-text`, as IcyDB's describe renderer did. `db status` contains the
full manifest fingerprint, incarnation, Entry revision and physical pages.
`query` currently reads Entry and shows `id` and `amount`; its `--min` filter runs over a
bounded primary-key page, so an empty page can still have a continuation.
The output supplies a next-page command, retaining the limit and filter, or an
explicit completion message. Terminal layout belongs to the CLI, not the runtime.
It has no SQL, remote-canister connection or production credentials. Broader
query support remains required in the eventual product CLI, with explicit entity
selection once more than Entry can be queried;
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
