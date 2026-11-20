# Producer-owned schema tree

The [producer build script](producer/build.rs) calls `ic_db::build_schema!(model)`.
Rust compiles [the declarations](producer/src/model/mod.rs) for host collection.
The generator validates each store and writes a roster per declared canister. Only
the [producer library](producer/src/lib.rs) includes that output with `ic_db::include_schema!()`. The
[canister](consumer/src/lib.rs) selects the hub roster through a renamed Cargo dependency. The hub has two
stores and the shard has one. The native host test installs and reopens both hub
stores on a shared stable-memory backing, and checks single-store shard reopen through the same session API. It has no schema build step.

The mutation probe verifies that entities in different modules can belong to the same declared store, that a
reference between children is valid, and that an incompatible reference key or
duplicate logical identity is refused. Adding an entity changes the fingerprint;
moving it inside the tree or re-exporting a type preserves the contract. Missing
target types and target-only additions in the tree fail compilation. Changing
the target’s key type traps on the fingerprint check in PocketIC. Native admission
also checks reopening a stored row and refuses a schema that omits one entity.

A cross-version check wrote user 73 with the tagged v0.23.22 producer and
reopened its complete IC Storage backing with the typed hub roster. Both used
`probe.account.v1` and fingerprint
`bb5e1da6add2f0d06c497080206c449ac2f2ca45395bbddf319dad6c3be8277d`.
Splitting a populated store into several allocations requires an explicit data
transition.

Cargo’s build-script environment applies across the producer package. Auxiliary
targets declaring entities under the same top-level module name are subject to
the same membership check.

```sh
make schema-build-check
python3 qualification/experiments/schema-owner/probe.py
IC_DB_SCHEMA_OWNER_ARTIFACTS="$PWD/qualification/target/schema-owner" bash scripts/with-pocketic.sh cargo test --locked --manifest-path qualification/Cargo.toml -p ic-db-m0-rollback-driver --test schema_owner
```

Run `python3 qualification/experiments/schema-owner/editor.py /path/to/rust-analyzer`
with the VS Code extension’s server to check editor diagnostics. The isolated
consumer uses default Cargo cfgs. The probe checks generated newtype hover,
active entity/store/canister declarations, and a genuinely inactive control.
