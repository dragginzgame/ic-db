#![forbid(unsafe_code)]
//! Local host for the actual M1 record library. Wire exports are private test
//! controls; production endpoints and authorization belong to the Canic adapter.

use ic_db::{Batch, Database, Entity, FieldType, Schema, entity};
use ic_platform_runtime::cdk;
use ic_storage::{
    AllocationGrants, DefaultMemoryImpl, GenericAllocationPolicy, MemoryAuthority, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot, StableKey,
};

#[cfg(feature = "fault-injection")]
mod faults;
#[cfg(feature = "fault-injection")]
type Backing = faults::FaultMemory;
#[cfg(not(feature = "fault-injection"))]
type Backing = DefaultMemoryImpl;

#[entity(id=1, pk(field="id", source="external"),
    index(id=1, fields=["category", "id"]),
    fields(field(name="id", value(item(prim="Nat64"))),
        field(name="category", value(item(prim="Nat64"))),
        field(name="amount", value(item(prim="Nat64")))))]
struct Entry {}

#[entity(
    id = 2,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "title", value(item(prim = "Text", max_bytes = 32))),
        field(name = "note", value(opt, item(prim = "Text", max_bytes = 64))),
        field(name = "published", value(item(prim = "Bool"))),
        field(name = "priority", value(opt, item(prim = "Nat64")))
    )
)]
struct Document {}

const KEY: &str = "m1.records.v1";
type Status = ([u8; 32], [u8; 16], u64, u64);
type QueryPage = (Vec<(u64, u64)>, Option<u64>, u64);

fn compiled() -> Schema {
    Schema::new(&[Entry::SCHEMA, Document::SCHEMA]).unwrap()
}
fn runtime() -> MemoryRuntime<Backing> {
    let mut host = MemoryRuntime::new(Backing::default()).unwrap();
    let declarations =
        SealedDeclarationSnapshot::new(vec![MemoryRequest::new("m1", KEY).unwrap()]).unwrap();
    let grants = AllocationGrants::new(vec![MemoryAuthority::new("m1", "m1.").unwrap()]).unwrap();
    host.bootstrap(&declarations, &grants, &GenericAllocationPolicy)
        .unwrap();
    host
}
fn open(host: &MemoryRuntime<Backing>) -> Database<Backing> {
    Database::open(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
        compiled(),
    )
    .unwrap()
}
#[cdk::init]
fn initialize() {
    let host = runtime();
    // Fixed fixture identity only. A real host supplies a fresh database incarnation.
    let mut db = Database::create(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
        compiled(),
        [1; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for id in 1..=6 {
        batch
            .insert(&Entry {
                id,
                category: id % 2,
                amount: id * 10 + id % 2,
            })
            .unwrap();
    }
    batch
        .insert(&Document {
            id: 1,
            title: "First document".into(),
            note: None,
            published: false,
            priority: Some(0),
        })
        .unwrap();
    db.commit(batch).unwrap();
}
fn status(db: &Database<Backing>, host: &MemoryRuntime<Backing>) -> Status {
    (
        db.schema().fingerprint(),
        db.incarnation(),
        db.revision(1).unwrap(),
        host.diagnostics().unwrap().physical_pages,
    )
}
#[ic_wire_endpoint::query(id = 1)]
fn inspect() -> Status {
    let host = runtime();
    let db = open(&host);
    status(&db, &host)
}
// Private fixture transport: entity identity, fields and indexes, without terminal layout.
type FieldView = (String, String, bool, Option<u32>);
type IndexView = (u32, Vec<String>, bool);
type EntityView = (String, u32, String, Vec<FieldView>, Vec<IndexView>);
#[ic_wire_endpoint::query(id = 2)]
fn schema() -> (Status, Vec<EntityView>) {
    let host = runtime();
    let db = open(&host);
    let entities = db
        .schema()
        .entities()
        .iter()
        .map(|entity| {
            let fields = entity
                .fields
                .iter()
                .map(|field| {
                    let (kind, limit) = match field.value {
                        FieldType::Nat64 => ("Nat64", None),
                        FieldType::Bool => ("Bool", None),
                        FieldType::Text { max_bytes } => ("Text", Some(max_bytes)),
                    };
                    (field.name.into(), kind.into(), field.optional, limit)
                })
                .collect();
            let indexes = entity
                .indexes
                .iter()
                .map(|index| {
                    (
                        index.id,
                        index.fields.iter().map(|name| (*name).into()).collect(),
                        index.unique,
                    )
                })
                .collect();
            (
                entity.name.into(),
                entity.id,
                entity.primary_key.into(),
                fields,
                indexes,
            )
        })
        .collect();
    (status(&db, &host), entities)
}
#[ic_wire_endpoint::query(id = 3)]
fn query((after, limit, minimum): (u64, u64, u64)) -> Result<QueryPage, u8> {
    if !(1..=3).contains(&limit) {
        return Err(1);
    }
    let host = runtime();
    let db = open(&host);
    let page = db
        .page::<Entry>(Some(after), limit as usize)
        .map_err(|_| 2)?;
    let scanned = page.rows.len() as u64;
    // This fixture's residual filter transforms only the obtained bounded page;
    // the database continuation remains after the last examined row, not last match.
    Ok((
        page.rows
            .into_iter()
            .filter(|row| row.amount >= minimum)
            .map(|row| (row.id, row.amount))
            .collect(),
        page.next_after,
        scanned,
    ))
}
#[ic_wire_endpoint::update(id = 4)]
fn write((id, category, amount, mode): (u64, u64, u64, u8)) -> Result<(), u8> {
    if mode > 3 {
        return Err(1);
    }
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    let row = Entry {
        id,
        category,
        amount,
    };
    if mode == 1 {
        batch.insert(&row).map_err(|_| 2)?;
    } else {
        batch.replace(&row).map_err(|_| 2)?;
    }
    db.commit(batch).map_err(|_| 3)?;
    if mode == 2 {
        cdk::api::trap("M1 application trap after successful database commit");
    }
    if mode == 3 {
        return Err(4);
    }
    Ok(())
}
#[ic_wire_endpoint::query(id = 5)]
fn category(category: u64) -> Vec<(u64, u64)> {
    let host = runtime();
    let db = open(&host);
    db.index_prefix::<Entry>(1, &[category], 32)
        .unwrap()
        .into_iter()
        .map(|row| (row.id, row.amount))
        .collect()
}

type DocumentValue = (String, Option<String>, bool, Option<u64>);
#[ic_wire_endpoint::query(id = 7)]
fn document() -> (DocumentValue, u64) {
    let host = runtime();
    let db = open(&host);
    let row = db.get::<Document>(1).unwrap().unwrap();
    (
        (row.title, row.note, row.published, row.priority),
        db.revision(2).unwrap(),
    )
}
#[ic_wire_endpoint::update(id = 8)]
fn replace_document((title, note, published, priority): DocumentValue) -> Result<(), u8> {
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    batch
        .replace(&Document {
            id: 1,
            title,
            note,
            published,
            priority,
        })
        .map_err(|_| 1)?;
    db.commit(batch).map_err(|_| 2)
}
