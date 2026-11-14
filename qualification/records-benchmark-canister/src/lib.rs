#![forbid(unsafe_code)]
//! One-entity host matching the frozen IcyDB scalar workload, without fault controls.

use ic_db::{Batch, Database, Entity, Error, Schema, entity};
use ic_platform_runtime::cdk;
use ic_storage::{
    AllocationGrants, DefaultMemoryImpl, GenericAllocationPolicy, MemoryAuthority, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot, StableKey,
};

#[entity(id = 1, pk(field = "id", source = "external"),
    index(id = 1, fields = ["category", "id"]),
    fields(field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat64"))),
        field(name = "amount", value(item(prim = "Nat64")))))]
struct Entry {}

const KEY: &str = "m1.benchmark.records.v1";
type Row = (u64, u64, u64);

fn compiled() -> Schema {
    Schema::new(&[Entry::SCHEMA]).unwrap()
}
fn runtime() -> MemoryRuntime<DefaultMemoryImpl> {
    let mut host = MemoryRuntime::new(DefaultMemoryImpl::default()).unwrap();
    let declarations =
        SealedDeclarationSnapshot::new(vec![MemoryRequest::new("m1", KEY).unwrap()]).unwrap();
    let grants = AllocationGrants::new(vec![MemoryAuthority::new("m1", "m1.").unwrap()]).unwrap();
    host.bootstrap(&declarations, &grants, &GenericAllocationPolicy)
        .unwrap();
    host
}
fn database() -> Database<DefaultMemoryImpl> {
    let host = runtime();
    Database::open(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
        compiled(),
    )
    .unwrap()
}
fn row(value: &Entry) -> Row {
    (value.id, value.category, value.amount)
}

// Include runtime construction, schema admission, database work and result mapping.
// Wire decode/encode is outside this interval, as in the frozen IcyDB host.
fn measure<T>(operation: impl FnOnce() -> T) -> (T, u64) {
    let start = cdk::api::instruction_counter();
    let result = operation();
    (result, cdk::api::instruction_counter() - start)
}

#[cdk::init]
fn initialize() {
    let host = runtime();
    // Fixed test identity only; this is not an application incarnation policy.
    Database::create(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
        compiled(),
        [41; 16],
    )
    .unwrap();
}

#[ic_wire_endpoint::update(id = 1)]
fn advance_startup() -> (bool, u64) {
    measure(|| {
        let _ = database();
        true
    })
}

#[ic_wire_endpoint::update(id = 2)]
fn insert((id, category, amount): Row) -> (bool, u64) {
    measure(|| {
        let mut database = database();
        let mut batch = Batch::new();
        batch
            .insert(&Entry {
                id,
                category,
                amount,
            })
            .unwrap();
        match database.commit(batch) {
            Ok(()) => true,
            Err(Error::Exists) => false,
            Err(error) => panic!("unexpected insert refusal: {error:?}"),
        }
    })
}

#[ic_wire_endpoint::query(id = 3)]
fn lookup(id: u64) -> (Option<Row>, u64) {
    measure(|| database().get::<Entry>(id).unwrap().as_ref().map(row))
}

#[ic_wire_endpoint::update(id = 4)]
fn read_modify_write((id, category, amount): Row) -> (Option<Row>, u64) {
    measure(|| {
        let mut database = database();
        let mut current = database.get::<Entry>(id).unwrap()?;
        current.category = category;
        current.amount = amount;
        let mut batch = Batch::new();
        batch.replace(&current).unwrap();
        database.commit(batch).unwrap();
        Some(row(&current))
    })
}

#[ic_wire_endpoint::query(id = 5)]
fn listing(category: u64) -> (Vec<Row>, u64) {
    // The index yields complete ID order within one category. This measures the
    // delivered direct read, not a fluent planner that IC DB has yet to implement.
    measure(|| {
        database()
            .index_prefix::<Entry>(1, &[category.into()], 10)
            .unwrap()
            .iter()
            .map(row)
            .collect()
    })
}
