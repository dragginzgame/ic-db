"""Build matched release Wasm variants of the reachable eight-shape query fixture."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
QUAL = ROOT / "qualification"
PACKAGE = "ic-db-entity-growth-canister"
OUT = Path(sys.argv[1]).resolve()
OUT.mkdir(parents=True, exist_ok=True)


def payload(width, distinct):
    if width == 0:
        return """
#[newtype(item(prim = "Nat64"))]
struct Payload {}
impl Payload {
    fn from_number(n: u64) -> Self { Self(n) }
    fn number(&self) -> u64 { self.0 }
}
"""
    leaves = [f"Leaf{i:02}" for i in range(width if distinct else 1)]
    declarations = [f'#[newtype(item(prim = "Nat64"))] struct {leaf} {{}}' for leaf in leaves]
    choices = [(f"V{i:02}", leaves[i if distinct else 0]) for i in range(width)]
    variants = ", ".join(f'variant(name = "{v}", value(item(is = "{t}")))' for v, t in choices)
    declarations.append(f"#[enum_({variants})] struct Payload {{}}")
    construct = ", ".join(f"{i} => Self::{v}({t}(n))" for i, (v, t) in enumerate(choices))
    extract = ", ".join(f"Self::{v}(x) => x.0" for v, _ in choices)
    declarations.append(f"""
impl Payload {{
    fn from_number(n: u64) -> Self {{
        match n % {width} {{ {construct}, _ => unreachable!() }}
    }}
    fn number(&self) -> u64 {{ match self {{ {extract} }} }}
}}
""")
    return "\n".join(declarations)


def add_payload(source, definition):
    edits = {
        'field(name = "amount", value(item(prim = "Nat64")))':
            'field(name = "amount", value(item(prim = "Nat64"))),\n'
            '                field(name = "payload", value(item(is = "Payload")))',
        "amount: input.2 }": "amount: input.2, payload: Payload::from_number(input.2) }",
        "(row.id, row.category, row.amount)":
            "{ assert_eq!(row.amount, row.payload.number()); (row.id, row.category, row.amount) }",
    }
    for before, after in edits.items():
        if source.count(before) != 1:
            raise ValueError(f"growth fixture changed at {before!r}")
        source = source.replace(before, after)
    return source + definition


def measure(root):
    # Preserve workspace membership, paths, lock and release profile. Only the
    # temporary benchmark crate is edited. Every variant builds at the same path.
    files = subprocess.check_output(
        ["git", "ls-files", "-z", "--", "Cargo*", "README.md", "LICENSE.md",
         "rust-toolchain.toml", "crates", "qualification"], cwd=ROOT)
    for name in files.decode().split("\0"):
        if name:
            dest = root / name
            dest.parent.mkdir(exist_ok=True, parents=True)
            shutil.copyfile(ROOT / name, dest)
    lib = root / "qualification" / PACKAGE / "src/lib.rs"
    original = lib.read_text()
    cases = [(f"{shape}-{n}", n, definition)
             for shape, definition in [("base", None), ("shared", payload(0, False))]
             for n in (1, 8, 32)]
    cases.extend((f"enum-{width}-{kind}", 1, payload(width, kind == "distinct"))
                 for width in (8, 64) for kind in ("shared", "distinct"))
    for label, entities, definition in cases:
        lib.write_text(original if definition is None else add_payload(original, definition))
        features = ["retained-host", "fluent-shapes"]
        if entities > 1:
            features.append(f"entities-{entities}")
        subprocess.run(["cargo", "build", "--locked", "--release", "--target",
                        "wasm32-unknown-unknown", "-p", PACKAGE, "--features", ",".join(features)],
                       cwd=root / "qualification", check=True,
                       env=dict(os.environ, CARGO_TARGET_DIR=str(QUAL / "target")))
        wasm = QUAL / "target/wasm32-unknown-unknown/release/ic_db_entity_growth_canister.wasm"
        shutil.copyfile(wasm, OUT / f"{label}.wasm")
        print(label, wasm.stat().st_size, flush=True)
    (OUT / "variants.tsv").write_text("".join(f"{label}\t{n}\n" for label, n, _ in cases))


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="ic-db-named-growth-") as directory:
        measure(Path(directory))
