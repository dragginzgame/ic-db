# Producer-owned schema tree

The [producer build script](producer/build.rs) calls `ic_db::build_schema!(model)`.
Rust compiles [the declarations](producer/src/model/mod.rs) for host collection.
The generator validates the whole tree and writes the target assembly. Only
the [producer library](producer/src/lib.rs) includes that output. The
[canister](consumer/src/lib.rs) calls `accounts::compiled()` through a renamed
Cargo dependency. It has no schema build step.

The mutation probe verifies that both children belong to one schema, that a
reference between children is valid, and that an incompatible reference key or
duplicate logical identity is refused. Adding an entity changes the fingerprint;
moving it inside the tree or re-exporting a type preserves the contract. Missing
target types and target-only additions in the tree fail compilation. Changing
the target’s key type traps on the fingerprint check in PocketIC. Native admission
also checks reopening a stored row and refuses a schema that omits one entity.

Cargo’s build-script environment applies across the producer package. Auxiliary
targets declaring entities under the same top-level module name are subject to
the same membership check.

```sh
cargo test --offline --locked --manifest-path qualification/Cargo.toml -p ic-db-schema-owner-producer --test admission
python3 qualification/experiments/schema-owner/probe.py
IC_DB_SCHEMA_OWNER_ARTIFACTS="$PWD/qualification/target/schema-owner" bash scripts/with-pocketic.sh cargo test --offline --locked --manifest-path qualification/Cargo.toml -p ic-db-m0-rollback-driver --test schema_owner
```
