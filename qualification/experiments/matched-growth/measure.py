"""Build the same reachable workload for IC DB and frozen IcyDB in disposable fixtures."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]
QUAL = ROOT / "qualification"
CURRENT = "ic-db-entity-growth-canister"
FROZEN = "ic-db-icydb-baseline-canister"
# All eight use an admitted (category, id) range with ascending IDs.
FILTERS = [None, ("amount", "ge"), ("amount", "le"), ("amount", "eq"),
           ("amount", "gt"), ("amount", "lt"), ("id", "ge"), ("id", "le")]


def query(legacy, condition):
    if legacy:
        chain = 'database.query::<$entity>().unwrap().filter(FieldRef::new("category").eq(category))'
    else:
        chain = 'database.query::<$entity>().filter("category", Comparison::Eq, category)'
    if condition:
        field, op = condition
        value = "id" if field == "id" else "amount"
        if legacy:
            method = {"ge": "gte", "le": "lte"}.get(op, op)
            chain += f'.filter(FieldRef::new("{field}").{method}({value}))'
        else:
            chain += f'.filter("{field}", Comparison::{op.title()}, {value})'
    terminal = ('.order_by(asc("id")).limit(2).execute_live_page(None)' if legacy
                else '.order_by(&["id"]).limit(2).fetch()')
    return chain + terminal + '.unwrap().rows.iter().map(|v| (v.id,v.category,v.amount)).collect()'


def endpoint(legacy, count, shapes):
    # Use one typed call site per terminal and entity on both sides.
    if legacy:
        insert = '''let mut batch = database.trusted_typed_write_batch();
            batch.push($insert { id: WriteCell::Value(Id::from_key(id)),
                category: WriteCell::Value(category), amount: WriteCell::Value(amount) }).unwrap();
            batch.execute().unwrap();'''
        lookup = 'database.get::<$entity>(Id::from_key(id))'
        setup = 'let database = db().unwrap();'
        run = 'icydb::db::with_request_execution(|| { BODY })'
    else:
        insert = '''let mut batch = Batch::new();
            batch.insert(&$entity {id, category, amount}).unwrap(); database.commit(batch).unwrap();'''
        lookup = 'database.get::<$entity>(id)'
        setup = '''let memory = host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap();
            let mut database = Database::open(memory, schema_for_open()).unwrap();'''
        run = 'with_host(|host| { BODY })'
    arms = [
        f'0 => {{ {insert} Vec::new() }}',
        f'1 => {lookup}.unwrap().iter().map(|v| (v.id,v.category,v.amount)).collect()',
    ]
    arms.extend(f'{i+2} => {query(legacy, cond)}' for i, cond in enumerate(FILTERS[:shapes]))
    operations = 'match mode { ' + ',\n'.join(arms) + ', _ => panic!("mode") }'
    dispatch = ',\n'.join(f'{i} => operate!(Entry{i:02}, Entry{i:02}Insert)' for i in range(count))
    body = (
        setup + '\nmacro_rules! operate { ($entity:ident, $insert:ident) => {{'
        + operations + '}} }\nmatch selector {' + dispatch + ', _ => panic!("selector") }'
    )
    return '''
// exercise
// Run one runtime-selected typed operation for the growth comparison.
// guard  : admits fixture frame 2.
// limits : 64 KiB request and reply.
#[ic_platform_runtime::update(runtime = ic_platform_runtime, name = "exercise",
    guard = crate::transport::admit::<2>, decode = crate::transport::decode::<_,2>,
    encode = crate::transport::encode::<_,2>, max_bytes = 65_536,
    on_error = crate::transport::endpoint_error)]
fn exercise((selector,mode,(id,category,amount)): (u8,u8,Row)) -> Vec<Row> {
''' + run.replace("BODY", body) + '\n}\n'


def host_source(original, frozen, shapes):
    """Reuse each released fixture's actual startup, grants and schema-open logic."""
    if frozen:
        source = original[:original.index('// insert\n')]
        source = source.replace('use ic_db_icydb_baseline_schema::{Entry, EntryInsert, EntryReplace};',
                                'use ic_db_icydb_baseline_schema::*;')
        source = source[:source.index('fn row(')] + source[source.index('// advance_startup\n'):]
        if shapes:
            source += 'use icydb::db::query::{FieldRef, asc};\n'
        return source

    source = (original[:original.index('// exercise\n')]
              + original[original.index('const KEY:'):original.index('fn operate<E')]
              + original[original.index('/// Logical owner'):])
    source = source.replace('type Observation = (Option<Row>, Vec<Row>, Vec<Row>, u64, u8);', '')
    start = source.index('        fn dispatch(')
    source = source[:start] + source[source.index('\n    };', start):]
    if shapes:
        source += 'use ic_db::Comparison;\n'
    return source


def build(root, out):
    files = subprocess.check_output(
        ["git", "ls-files", "-z", "--", "Cargo*", "crates", "LICENSE.md", "README.md",
         "rust-toolchain.toml", "qualification"], cwd=ROOT,
    )
    for name in filter(None, files.decode().split("\0")):
        dest = root / name
        dest.parent.mkdir(exist_ok=True, parents=True)
        shutil.copyfile(ROOT / name, dest)
    workspace = root / "qualification"
    schema = workspace / "ic-db-icydb-baseline-schema/src/lib.rs"
    schema_source = schema.read_text()
    start = schema_source.index("/// Small rows")
    preamble = schema_source[:start]
    entity = schema_source[start:schema_source.index("/// Isolated")]

    current = (workspace / CURRENT / "src/lib.rs").read_text()
    legacy = (workspace / FROZEN / "src/lib.rs").read_text()
    for count in (1, 8, 32):
        declarations = (entity.replace("struct Entry", f"struct Entry{i:02}") for i in range(count))
        schema.write_text(preamble + "".join(declarations))
        for shapes in (0, 1, 8):
            for frozen, package in [(False, CURRENT), (True, FROZEN)]:
                features = []
                source = host_source(legacy if frozen else current, frozen, shapes)
                if not frozen:
                    selected = ['retained-host']
                    if count > 1:
                        selected.append(f'entities-{count}')
                    features = ['--features', ','.join(selected)]
                (workspace / package / 'src/lib.rs').write_text(source + endpoint(frozen, count, shapes))
                subprocess.run(
                    ["cargo", "build", "--offline", "--locked", "--release", "--target",
                     "wasm32-unknown-unknown", "-p", package, *features],
                    cwd=workspace, check=True,
                    env=dict(os.environ, CARGO_TARGET_DIR=str(QUAL / "target")),
                )
                artifact = package.replace("-", "_") + ".wasm"
                wasm = QUAL / "target/wasm32-unknown-unknown/release" / artifact
                dest = out / f'{"icydb" if frozen else "ic-db"}-{count}-{shapes}.wasm'
                shutil.copyfile(wasm, dest)
                print(dest.name, dest.stat().st_size, flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='icdb-matched-growth-') as tmp:
        build(Path(tmp), args.output.resolve())
