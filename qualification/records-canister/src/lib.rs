#![forbid(unsafe_code)]
//! Local host for the actual M1 record library. Wire exports are private test
//! controls; production endpoints and authorization belong to the Canic adapter.

use ic_db::{Batch, Database, Entity, FieldType, Principal};
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

mod schema;
use schema::{
    BinaryObject, Document, Entry, IdentityRow, NarrowNumbers, OptionalLink, WideNumbers,
};

const KEY: &str = "m1.records.v1";
type Status = ([u8; 32], [u8; 16], u64, u64);
type QueryPage = (Vec<(u64, u64)>, Option<u64>, u64);

include!(concat!(env!("OUT_DIR"), "/schema.rs"));
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
    batch
        .insert(&IdentityRow {
            id: 1,
            owner: Principal::anonymous(),
            optional: None,
        })
        .unwrap();
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
            entry_id: 1,
            title: "First document".into(),
            note: None,
            published: false,
            score: -10,
            previous_score: None,
            priority: Some(0),
        })
        .unwrap();
    batch
        .insert(&WideNumbers {
            id: 1,
            signed: i128::MIN,
            unsigned: u128::MAX,
            optional_signed: None,
            optional_unsigned: Some(0),
        })
        .unwrap();
    batch
        .insert(&BinaryObject {
            id: 1,
            payload: vec![0, 255, 128],
            optional: None,
        })
        .unwrap();
    batch
        .insert(&NarrowNumbers {
            id: 1,
            signed8: i8::MIN,
            signed16: i16::MIN,
            signed32: i32::MIN,
            unsigned8: u8::MAX,
            unsigned16: u16::MAX,
            unsigned32: u32::MAX,
            optional_signed8: None,
            optional_signed16: None,
            optional_signed32: None,
            optional_unsigned8: None,
            optional_unsigned16: None,
            optional_unsigned32: None,
        })
        .unwrap();
    db.commit(batch).unwrap();
}
fn status(db: &Database<Backing>, host: &MemoryRuntime<Backing>) -> Status {
    (
        db.schema().fingerprint(),
        db.incarnation(),
        db.revision::<Entry>().unwrap(),
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
type FieldView = (String, String, bool, Option<u32>, Option<(u32, String)>);
type IndexView = (String, Vec<String>, bool, bool);
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
                        FieldType::Nat8 => ("Nat8", None),
                        FieldType::Nat16 => ("Nat16", None),
                        FieldType::Nat32 => ("Nat32", None),
                        FieldType::Int8 => ("Int8", None),
                        FieldType::Int16 => ("Int16", None),
                        FieldType::Int32 => ("Int32", None),
                        FieldType::Nat64 => ("Nat64", None),
                        FieldType::Int64 => ("Int64", None),
                        FieldType::Nat128 => ("Nat128", None),
                        FieldType::Int128 => ("Int128", None),
                        FieldType::Bool => ("Bool", None),
                        FieldType::Principal => ("Principal", Some(29)),
                        FieldType::Text { max_bytes } => ("Text", Some(max_bytes)),
                        FieldType::Blob { max_bytes } => ("Blob", Some(max_bytes)),
                    };
                    (
                        field.name.into(),
                        kind.into(),
                        field.optional,
                        limit,
                        field.reference.map(|reference| {
                            (
                                reference.entity,
                                entity
                                    .indexes
                                    .iter()
                                    .find(|index| index.id == reference.index)
                                    .unwrap()
                                    .name
                                    .into(),
                            )
                        }),
                    )
                })
                .collect();
            let indexes = entity
                .indexes
                .iter()
                .map(|index| {
                    (
                        index.name.into(),
                        index.fields.iter().map(|name| (*name).into()).collect(),
                        index.unique,
                        index.present_only,
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
    db.index_prefix::<Entry>("category,id", &[category.into()], 32)
        .unwrap()
        .into_iter()
        .map(|row| (row.id, row.amount))
        .collect()
}

type DocumentValue = (String, Option<String>, bool, Option<u64>, i64, Option<i64>);
#[ic_wire_endpoint::query(id = 7)]
fn document() -> (DocumentValue, u64) {
    let host = runtime();
    let db = open(&host);
    let row = db.get::<Document>(1).unwrap().unwrap();
    (
        (
            row.title,
            row.note,
            row.published,
            row.priority,
            row.score,
            row.previous_score,
        ),
        db.revision::<Document>().unwrap(),
    )
}
#[ic_wire_endpoint::update(id = 8)]
fn replace_document(
    (title, note, published, priority, score, previous_score): DocumentValue,
) -> Result<(), u8> {
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    batch
        .replace(&Document {
            id: 1,
            entry_id: 1,
            title,
            note,
            published,
            score,
            previous_score,
            priority,
        })
        .map_err(|_| 1)?;
    db.commit(batch).map_err(|_| 2)
}

#[ic_wire_endpoint::query(id = 9)]
fn document_reference(id: u64) -> Option<u64> {
    let host = runtime();
    open(&host)
        .get::<Document>(id)
        .unwrap()
        .map(|row| row.entry_id)
}

#[ic_wire_endpoint::update(id = 10)]
fn reference_batch(mode: u8) -> Result<(), u8> {
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    match mode {
        0 | 3 => {
            let mut row = db.get::<Document>(1).unwrap().unwrap();
            row.entry_id = if mode == 0 { 99 } else { 2 };
            batch.replace(&row).unwrap();
            if mode == 3 {
                batch.delete::<Entry>(1).unwrap();
            }
        }
        1 | 2 => {
            batch.delete::<Entry>(1).unwrap();
            if mode == 2 {
                batch.delete::<Document>(1).unwrap();
            }
        }
        4 => {
            batch
                .insert(&Document {
                    id: 2,
                    entry_id: 7,
                    title: "Dependent first".into(),
                    note: None,
                    published: false,
                    score: 0,
                    previous_score: Some(-1),
                    priority: None,
                })
                .unwrap();
            batch
                .insert(&Entry {
                    id: 7,
                    category: 0,
                    amount: 70,
                })
                .unwrap();
        }
        5 | 6 => {
            batch.delete::<Entry>(7).unwrap();
            if mode == 6 {
                batch.delete::<Document>(2).unwrap();
            }
        }
        _ => return Err(2),
    }
    db.commit(batch).map_err(|error| {
        if matches!(error, ic_db::Error::ReferenceViolation) {
            1
        } else {
            2
        }
    })
}

type WideValue = (i128, u128, Option<i128>, Option<u128>);
#[ic_wire_endpoint::query(id = 11)]
fn wide_numbers() -> (WideValue, u64) {
    let host = runtime();
    let db = open(&host);
    let row = db.get::<WideNumbers>(1).unwrap().unwrap();
    (
        (
            row.signed,
            row.unsigned,
            row.optional_signed,
            row.optional_unsigned,
        ),
        db.revision::<WideNumbers>().unwrap(),
    )
}
#[ic_wire_endpoint::update(id = 12)]
fn replace_wide_numbers(
    ((signed, unsigned, optional_signed, optional_unsigned), trap): (WideValue, bool),
) {
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    batch
        .replace(&WideNumbers {
            id: 1,
            signed,
            unsigned,
            optional_signed,
            optional_unsigned,
        })
        .unwrap();
    db.commit(batch).unwrap();
    if trap {
        cdk::api::trap("M1 wide-number application trap");
    }
}

type BinaryValue = (ic_wire::ByteBuf, Option<ic_wire::ByteBuf>);
type NarrowRequired = (i8, i16, i32, u8, u16, u32);
type NarrowOptional = (
    Option<i8>,
    Option<i16>,
    Option<i32>,
    Option<u8>,
    Option<u16>,
    Option<u32>,
);
type NarrowValue = (NarrowRequired, NarrowOptional);

#[ic_wire_endpoint::query(id = 16)]
fn narrow_numbers() -> (NarrowValue, u64) {
    let host = runtime();
    let db = open(&host);
    let row = db.get::<NarrowNumbers>(1).unwrap().unwrap();
    (
        (
            (
                row.signed8,
                row.signed16,
                row.signed32,
                row.unsigned8,
                row.unsigned16,
                row.unsigned32,
            ),
            (
                row.optional_signed8,
                row.optional_signed16,
                row.optional_signed32,
                row.optional_unsigned8,
                row.optional_unsigned16,
                row.optional_unsigned32,
            ),
        ),
        db.revision::<NarrowNumbers>().unwrap(),
    )
}

#[ic_wire_endpoint::update(id = 17)]
fn replace_narrow_numbers((value, trap): (NarrowValue, bool)) {
    let (
        (signed8, signed16, signed32, unsigned8, unsigned16, unsigned32),
        (
            optional_signed8,
            optional_signed16,
            optional_signed32,
            optional_unsigned8,
            optional_unsigned16,
            optional_unsigned32,
        ),
    ) = value;
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    batch
        .replace(&NarrowNumbers {
            id: 1,
            signed8,
            signed16,
            signed32,
            unsigned8,
            unsigned16,
            unsigned32,
            optional_signed8,
            optional_signed16,
            optional_signed32,
            optional_unsigned8,
            optional_unsigned16,
            optional_unsigned32,
        })
        .unwrap();
    db.commit(batch).unwrap();
    if trap {
        cdk::api::trap("M1 narrow-number application trap");
    }
}

// Private decoder probe: transport carries opaque bytes, never decodes a row.
#[ic_wire_endpoint::query(id = 15)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "endpoint decoding owns the request; the database decoder borrows its bytes"
)]
fn binary_decodes(bytes: ic_wire::ByteBuf) -> bool {
    BinaryObject::decode(bytes.as_ref()).is_ok()
}

#[ic_wire_endpoint::query(id = 13)]
fn binary_object() -> (BinaryValue, u64) {
    let host = runtime();
    let db = open(&host);
    let row = db.get::<BinaryObject>(1).unwrap().unwrap();
    (
        (row.payload.into(), row.optional.map(Into::into)),
        db.revision::<BinaryObject>().unwrap(),
    )
}
#[ic_wire_endpoint::update(id = 14)]
fn replace_binary_object(((payload, optional), trap): (BinaryValue, bool)) -> Result<(), u8> {
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    batch
        .replace(&BinaryObject {
            id: 1,
            payload: payload.into_vec(),
            optional: optional.map(ic_wire::ByteBuf::into_vec),
        })
        .map_err(|_| 1)?;
    db.commit(batch).map_err(|_| 2)?;
    if trap {
        cdk::api::trap("M1 binary application trap");
    }
    Ok(())
}

// Bytes cross the test transport; Principal admission and persistence belong to
// their respective owners, never Wire's representation of an identity.
type PrincipalValue = (ic_wire::ByteBuf, Option<ic_wire::ByteBuf>);
#[ic_wire_endpoint::query(id = 18)]
fn principal_record() -> (PrincipalValue, u64) {
    let host = runtime();
    let db = open(&host);
    let row = db.get::<IdentityRow>(1).unwrap().unwrap();
    (
        (
            row.owner.as_slice().to_vec().into(),
            row.optional.map(|p| p.as_slice().to_vec().into()),
        ),
        db.revision::<IdentityRow>().unwrap(),
    )
}
#[ic_wire_endpoint::update(id = 19)]
fn replace_principal_record(((owner, optional), trap): (PrincipalValue, bool)) -> Result<(), u8> {
    let row = IdentityRow {
        id: 1,
        owner: Principal::try_from_slice(owner.as_ref()).map_err(|_| 1)?,
        optional: optional
            .as_ref()
            .map(|p| Principal::try_from_slice(p.as_ref()))
            .transpose()
            .map_err(|_| 1)?,
    };
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    if trap {
        cdk::api::trap("M1 principal application trap");
    }
    Ok(())
}
#[ic_wire_endpoint::query(id = 20)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "endpoint owns transport input; database decoder borrows it"
)]
fn principal_decodes(bytes: ic_wire::ByteBuf) -> bool {
    IdentityRow::decode(bytes.as_ref()).is_ok()
}

// Private qualification controls; product endpoints remain adapter-owned.
type IndexedWrite = (u64, ic_wire::ByteBuf, i64);
#[ic_wire_endpoint::update(id = 21)]
fn indexed_batch((rows, trap): (Vec<IndexedWrite>, bool)) -> Result<(), u8> {
    let host = runtime();
    let mut db = open(&host);
    let document = db.get::<Document>(1).unwrap().unwrap();
    let mut batch = Batch::new();
    for (id, owner, score) in rows {
        let identity = IdentityRow {
            id,
            owner: Principal::try_from_slice(owner.as_ref()).map_err(|_| 1)?,
            optional: None,
        };
        let changed = Document {
            id,
            score,
            ..document.clone()
        };
        if db.get::<IdentityRow>(id).unwrap().is_some() {
            batch.replace(&identity).map_err(|_| 2)?;
        } else {
            batch.insert(&identity).map_err(|_| 2)?;
        }
        if db.get::<Document>(id).unwrap().is_some() {
            batch.replace(&changed).map_err(|_| 2)?;
        } else {
            batch.insert(&changed).map_err(|_| 2)?;
        }
    }
    db.commit(batch).map_err(|_| 3)?;
    if trap {
        cdk::api::trap("M1 indexed batch application trap");
    }
    Ok(())
}

type IndexedRead = (Vec<u64>, Vec<(i64, u64)>);
#[ic_wire_endpoint::query(id = 22)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "endpoint owns transport input"
)]
fn typed_indexes(owner: ic_wire::ByteBuf) -> Result<IndexedRead, u8> {
    let principal = Principal::try_from_slice(owner.as_ref()).map_err(|_| 1)?;
    let host = runtime();
    let db = open(&host);
    Ok((
        db.index_prefix::<IdentityRow>("owner", &[principal.into()], 32)
            .unwrap()
            .into_iter()
            .map(|row| row.id)
            .collect(),
        db.index_prefix::<Document>("entry_id,score", &[1_u64.into()], 32)
            .unwrap()
            .into_iter()
            .map(|row| (row.score, row.id))
            .collect(),
    ))
}

#[ic_wire_endpoint::update(id = 23)]
fn text_index_batch((rows, trap): (Vec<(u64, String, bool)>, bool)) -> Result<(), u8> {
    let host = runtime();
    let mut db = open(&host);
    let template = db.get::<Document>(1).unwrap().unwrap();
    let mut batch = Batch::new();
    for (id, title, published) in rows {
        let row = Document {
            id,
            title,
            published,
            ..template.clone()
        };
        if db.get::<Document>(id).unwrap().is_some() {
            batch.replace(&row).map_err(|_| 1)?;
        } else {
            batch.insert(&row).map_err(|_| 1)?;
        }
    }
    db.commit(batch).map_err(|_| 2)?;
    if trap {
        cdk::api::trap("M1 text index application trap");
    }
    Ok(())
}

#[ic_wire_endpoint::query(id = 24)]
fn text_index((published, title): (bool, Option<String>)) -> Result<Vec<(String, u64)>, u8> {
    let host = runtime();
    let db = open(&host);
    let mut prefix = vec![published.into()];
    if let Some(title) = title.as_deref() {
        prefix.push(title.into());
    }
    db.index_prefix::<Document>("published,title", &prefix, 32)
        .map(|rows| rows.into_iter().map(|row| (row.title, row.id)).collect())
        .map_err(|_| 1)
}

type OptionalWrites = Vec<(u64, Option<ic_wire::ByteBuf>, Option<u64>)>;
type OptionalRead = (Result<Vec<u64>, u8>, Vec<(Option<u64>, u64)>);
#[ic_wire_endpoint::update(id = 25)]
fn optional_index_batch((rows, trap): (OptionalWrites, bool)) -> Result<(), u8> {
    let host = runtime();
    let mut db = open(&host);
    let template = db.get::<Document>(1).unwrap().unwrap();
    let mut batch = Batch::new();
    for (id, optional, priority) in rows {
        // Stable test-only identities, distinct from each other and the initial [4].
        let identity = IdentityRow {
            id,
            owner: Principal::try_from_slice(&id.to_be_bytes()).unwrap(),
            optional: optional
                .as_ref()
                .map(|v| Principal::try_from_slice(v.as_ref()))
                .transpose()
                .map_err(|_| 1)?,
        };
        let document = Document {
            id,
            priority,
            ..template.clone()
        };
        if db.get::<IdentityRow>(id).unwrap().is_some() {
            batch.replace(&identity)
        } else {
            batch.insert(&identity)
        }
        .map_err(|_| 2)?;
        if db.get::<Document>(id).unwrap().is_some() {
            batch.replace(&document)
        } else {
            batch.insert(&document)
        }
        .map_err(|_| 2)?;
    }
    db.commit(batch).map_err(|_| 3)?;
    if trap {
        cdk::api::trap("M1 optional index application trap");
    }
    Ok(())
}

#[ic_wire_endpoint::query(id = 26)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "endpoint owns transport input"
)]
fn optional_indexes(value: Option<ic_wire::ByteBuf>) -> OptionalRead {
    let host = runtime();
    let db = open(&host);
    let operand = value
        .as_ref()
        .map(|v| Principal::try_from_slice(v.as_ref()).map(ic_db::IndexValue::from))
        .transpose();
    let owners = operand.map_err(|_| 1).and_then(|v| {
        db.index_prefix::<IdentityRow>("optional", &[v.unwrap_or(ic_db::IndexValue::Absent)], 32)
            .map(|rows| rows.into_iter().map(|row| row.id).collect())
            .map_err(|_| 2)
    });
    let priorities = db
        .index_prefix::<Document>("entry_id,priority", &[1_u64.into()], 32)
        .unwrap()
        .into_iter()
        .map(|row| (row.priority, row.id))
        .collect();
    (owners, priorities)
}

// Optional relationships share the product batch; these are private probe controls.
type OptionalLinks = Vec<(u64, Option<u64>)>;
#[ic_wire_endpoint::update(id = 27)]
fn optional_reference_batch(
    (rows, deletes, trap): (OptionalLinks, Vec<u64>, bool),
) -> Result<(), u8> {
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for id in deletes {
        batch.delete::<OptionalLink>(id).map_err(|_| 1)?;
    }
    for (id, next) in rows {
        let row = OptionalLink { id, next };
        if db.get::<OptionalLink>(id).unwrap().is_some() {
            batch.replace(&row)
        } else {
            batch.insert(&row)
        }
        .map_err(|_| 1)?;
    }
    db.commit(batch).map_err(|_| 2)?;
    if trap {
        cdk::api::trap("M1 optional reference application trap");
    }
    Ok(())
}

#[ic_wire_endpoint::query(id = 28)]
fn optional_references(target: Option<u64>) -> (OptionalLinks, Vec<u64>, u64) {
    let host = runtime();
    let db = open(&host);
    let rows = db
        .page::<OptionalLink>(None, 32)
        .unwrap()
        .rows
        .into_iter()
        .map(|row| (row.id, row.next))
        .collect();
    let dependants = db
        .index_prefix::<OptionalLink>(
            "next,id",
            &[target
                .map(ic_db::IndexValue::from)
                .unwrap_or(ic_db::IndexValue::Absent)],
            32,
        )
        .unwrap()
        .into_iter()
        .map(|row| row.id)
        .collect();
    (rows, dependants, db.revision::<OptionalLink>().unwrap())
}
