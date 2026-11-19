"""Mutate only a temporary producer, building the same consumer throughout."""
from pathlib import Path
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
QUAL = HERE.parents[1]
PACKAGE = "ic-db-schema-owner-consumer"
ARTIFACTS = QUAL / "target/schema-owner"
ARTIFACTS.mkdir(exist_ok=True, parents=True)


def check_variant(root, args, refusal=None):
    env = dict(os.environ, CARGO_TARGET_DIR=str(QUAL / "target"), CARGO_PROFILE_RELEASE_STRIP="none")
    run = subprocess.run(["cargo", *args, "--offline", "-p", PACKAGE], cwd=root,
                         env=env, capture_output=True, text=True)
    if refusal is None and run.returncode == 0:
        return run.stdout
    if refusal and run.returncode and refusal in run.stderr:
        return
    sys.exit(run.stderr or "expected refusal: " + str(refusal))


def probe(root):
    shutil.copy(QUAL.parent / "LICENSE.md", root / "LICENSE.md")
    shutil.copytree(HERE / "producer", root / "producer")
    shutil.copytree(HERE / "consumer", root / "consumer")
    manifest = (QUAL / "Cargo.toml").read_text()
    manifest = re.sub(r"members = \[.*?\]", 'members = ["producer", "consumer"]',
                      manifest, count=1, flags=re.S)
    manifest = re.sub(r'path = "([^"]+)"', lambda m: f'path = "{(QUAL / m[1]).resolve()}"',
                      manifest)
    manifest = manifest.replace('license-file = "../LICENSE.md"', 'license-file = "LICENSE.md"')
    deps, rest = manifest.split("[workspace.dependencies]\n")
    entries, lints = rest.split("[workspace.lints.cargo]")
    selected = [line for line in entries.splitlines() if line.split(" = ")[0] in {
        "ic-db", "ic-storage", "ic-platform-runtime", "ic-db-qualification-transport", "toko-tooling"
    }]
    manifest = deps + "[workspace.dependencies]\n" + "\n".join(selected) + "\n[workspace.lints.cargo]" + lints
    (root / "Cargo.toml").write_text(manifest)
    shutil.copy(QUAL / "Cargo.lock", root / "Cargo.lock")
    model_dir = root / "producer/src/model"
    hub = model_dir / "hub.rs"
    original = hub.read_text()
    native = ["run", "--example", "membership"]
    wasm = ["build", "--release", "--lib", "--target", "wasm32-unknown-unknown"]

    def artifact(name):
        check_variant(root, wasm)
        binary = QUAL / "target/wasm32-unknown-unknown/release/ic_db_schema_owner_consumer.wasm"
        image = binary.read_bytes()
        assert b"schema_size" in image and b"ic_db_schema_owner_consumer" in image
        for symbol in (b"__register_schema", b"build8__entity", b"build7__store",
                       b"build10__canister", b".init_array"):
            assert symbol not in image, symbol
        shutil.copy(binary, ARTIFACTS / f"{name}.wasm")

    baseline = check_variant(root, native)
    assert baseline.startswith('["Entry", "User"]')
    artifact("baseline")

    # The generated registration binding shares a spelling with this authored type.
    hub.write_text(original.replace("struct User", "struct REGISTER")
                   .replace("#[entity(", '#[entity(name = "User",')
                   + "\npub use REGISTER as User;\n")
    assert check_variant(root, native) == baseline
    hub.write_text(original)

    # A generic host operation can constrain an entity to its own canister.
    example = root / "consumer/examples/membership.rs"
    consumer = example.read_text()
    owner_check = """
fn accepts<C: ic_db::Canister, E: ic_db::Entity<Store: ic_db::Store<Canister = C>>>() {}
const _: fn() = accepts::<directory::model::Hub, directory::model::hub::User>;
"""
    example.write_text(consumer + owner_check)
    assert check_variant(root, native) == baseline
    example.write_text(consumer + owner_check.replace("model::Hub,", "model::Shard,"))
    check_variant(root, native, "type mismatch")
    example.write_text(consumer)

    moved = model_dir / "hub"
    moved.mkdir()
    hub = hub.rename(moved / "mod.rs")
    (moved / "accounts.rs").write_text(original)
    hub.write_text("pub mod accounts;\npub use accounts::User;\npub use super::shard::Entry;\n")
    assert check_variant(root, native) == baseline
    (moved / "audit.rs").write_text(original.replace("struct User", "struct Audit"))
    with hub.open("a") as out:
        out.write("pub mod audit;\n")
    added = check_variant(root, native)
    assert added.startswith('["Audit", "Entry", "User"]')
    assert added.split("] ")[1] != baseline.split("] ")[1]

    # Both child modules can use the same Rust identifier with independent logical names.
    shard = model_dir / "shard.rs"
    shard_source = shard.read_text()
    shard.write_text(shard_source.replace("struct Entry", "struct User").replace(
        "#[entity(", '#[entity(name = "Entry",'
    ) + "\npub use User as Entry;\n")
    assert check_variant(root, native) == added
    shard.write_text(shard_source)

    (moved / "audit.rs").write_text(original)
    check_variant(root, native, "both declare entity User")
    hub.write_text(original + '''
#[entity(store = crate::model::Accounts, pk(field = "id"), index(field = "entry"), fields(
    field(name = "id", value(item(prim = "Nat64"))),
    field(name = "entry", value(item(prim = "Nat64")), references(entity = "super::shard::Entry"))
))]
pub struct Link {}
''')
    assert check_variant(root, native).startswith('["Entry", "Link", "User"]')
    hub.write_text(hub.read_text().replace('name = "entry", value(item(prim = "Nat64"))',
                                          'name = "entry", value(item(prim = "Nat32"))'))
    check_variant(root, native, "references Entry.id")

    manifest = model_dir / "mod.rs"
    owners = manifest.read_text()
    manifest.write_text(owners.replace("probe.audit.v1", "probe.account.v1"))
    check_variant(root, native, "both declare probe.account.v1")
    manifest.write_text(owners.replace('key = "probe.audit.v1"', 'key = "probe.audit.v1", key = "probe.other.v1"'))
    check_variant(root, native, "duplicate attribute")
    manifest.write_text(owners.replace("canister = Hub,", "canister = MissingCanister,", 1))
    check_variant(root, native, "cannot find type")
    manifest.write_text(owners)
    hub.write_text(original.replace("store = crate::model::Accounts,", ""))
    check_variant(root, native, "entity requires store")
    hub.write_text(original.replace("crate::model::Accounts", "crate::model::Unknown"))
    check_variant(root, native, "cannot find type")
    hub.write_text(original + """
#[entity(store = crate::model::ShardStore, pk(field = "id"), index(field = "user"), fields(
    field(name = "id", value(item(prim = "Nat64"))),
    field(name = "user", value(item(prim = "Nat64")), references(entity = "User"))
))]
pub struct Link {}
#[entity(store = crate::model::ShardStore, name = "User", pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64")))))]
pub struct OtherUser {}
""")
    check_variant(root, native, "type mismatch")
    hub.write_text(original)
    imports, declaration = original.split("\n\n", 1)
    host_only = '#[cfg(not(target_family = "wasm"))]\n' + declaration
    hub.write_text(imports + "\n\n" + host_only)
    check_variant(root, wasm, "cannot find type `User`")
    hub.write_text(imports + "\n\n" + host_only + '#[cfg(target_family = "wasm")]\n' + declaration.replace("Nat64", "Nat32"))
    artifact("drift")
    hub.write_text(original + '#[cfg(target_family = "wasm")]\n' + declaration.replace("struct User", "struct Ghost"))
    check_variant(root, wasm, "declaration missing from host schema:")
    (moved / "only.rs").write_text(original)
    hub.write_text(original + '#[cfg(target_family = "wasm")]\npub mod only;\n')
    check_variant(root, wasm, "declaration missing from host schema: ic_db_schema_owner_producer::model::hub::only::User")
    hub.write_text(original)
    # A similarly spelled name outside the selected module cannot join its tree.
    target = root / "producer/src/lib.rs"
    with target.open("a") as out:
        out.write("pub mod model_extra;\n")
    (target.parent / "model_extra.rs").write_text(original)
    assert check_variant(root, native) == baseline
    check_variant(root, wasm)
    print(baseline, end="")
    print("Typed canister/store membership, moves, cross-module references and drift checks passed.")
    print(f"Two Wasm artifacts ready for the replica probe in {ARTIFACTS}.")


with tempfile.TemporaryDirectory(prefix="schema-owner-") as temp:
    probe(Path(temp))
