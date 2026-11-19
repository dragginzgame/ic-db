#![deny(unsafe_code)]
#![cfg_attr(
    not(target_arch = "wasm32"),
    expect(
        dead_code,
        reason = "Platform emits the roots for this private cdylib only on Wasm; native builds type-check the same handlers"
    )
)]
// Platform narrowly permits the generated IC export attribute; handlers remain safe.
//! Local host for the actual M1 record library. Wire exports are private test
//! controls; production endpoints and authorization belong to the Canic adapter.

use ic_db_qualification_transport as transport;
use ic_platform_runtime::system;

#[cfg(test)]
mod tests;

mod account_keys;
#[cfg(any(feature = "aggregate-jobs", feature = "retained-console"))]
mod aggregate_store;
mod console;
mod defaults;
mod enum_collections;
mod envelope;
mod generation;
#[cfg(feature = "fault-injection")]
mod handles;
#[cfg(not(feature = "fault-injection"))]
use ic_db as handles;
#[cfg(feature = "aggregate-jobs")]
mod jobs;
#[cfg(feature = "maintenance")]
mod maintenance;
mod managed;
#[cfg(feature = "maintenance-faults")]
mod migration_capacity;
#[cfg(feature = "maintenance-faults")]
mod migration_references;
#[cfg(feature = "mixed-lifecycle")]
mod mixed_lifecycle;
mod principal_keys;
mod queries;

use crate::handles::Database;
use ic_db::{Batch, Entity, FieldType, Principal};

// Treat framed unit as one reply value for the generic encoder hook.
type EmptyReply = ();
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

use ic_db_m1_records_schema as schema;
use schema::{
    BinaryObject, Document, Entry, IdentityRow, NarrowNumbers, OptionalLink, WideNumbers,
};

const KEY: &str = "m1.records.v1";
type Status = ([u8; 32], [u8; 16], u64, u64);

include!(concat!(env!("OUT_DIR"), "/schema.rs"));

fn runtime() -> std::rc::Rc<MemoryRuntime<Backing>> {
    // Host custody survives calls; Database handles and leases remain local.
    // Upgrade clears this heap state and the next access re-admits the runtime.
    thread_local! {
        static HOST: std::rc::Rc<MemoryRuntime<Backing>> = std::rc::Rc::new(new_runtime());
    }
    HOST.with(std::rc::Rc::clone)
}

fn new_runtime() -> MemoryRuntime<Backing> {
    let mut host = MemoryRuntime::new(Backing::default()).unwrap();
    let groups: &[&[&str]] = &[
        &[KEY],
        #[cfg(feature = "maintenance")]
        &[
            maintenance::COPY_SOURCE,
            maintenance::COPY_TARGET,
            maintenance::COPY_NEXT,
            maintenance::COPY_ROUTE,
        ],
        #[cfg(any(feature = "aggregate-jobs", feature = "retained-console"))]
        &[aggregate_store::KEY],
        #[cfg(feature = "retained-console")]
        &[console::migration::SOURCE, console::migration::TARGET],
        #[cfg(feature = "mixed-lifecycle")]
        &mixed_lifecycle::KEYS,
    ];
    let requests = groups
        .iter()
        .flat_map(|keys| keys.iter())
        .map(|&key| MemoryRequest::new("m1", key).unwrap())
        .collect();
    let declarations = SealedDeclarationSnapshot::new(requests).unwrap();
    let grants = AllocationGrants::new(vec![MemoryAuthority::new("m1", "m1.").unwrap()]).unwrap();
    host.bootstrap(&declarations, &grants, &GenericAllocationPolicy)
        .unwrap();
    host
}

fn open(host: &MemoryRuntime<Backing>) -> Database {
    Database::open(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
        compiled(),
    )
    .unwrap()
}

/// Initialize the fixture’s stable state at installation.
#[ic_platform_runtime::init(
    runtime = ic_platform_runtime,
    guard = crate::transport::allow,
    decode = crate::transport::empty,
    max_bytes = 0,
    on_error = crate::transport::lifecycle_error
)]
fn initialize((): ()) {
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
    #[cfg(any(feature = "aggregate-jobs", feature = "retained-console"))]
    aggregate_store::initialize(&host);
    #[cfg(feature = "mixed-lifecycle")]
    mixed_lifecycle::initialize(&host);
}

fn status(db: &Database) -> Status {
    (
        db.schema().fingerprint(),
        db.incarnation(),
        db.revision::<Entry>().unwrap(),
        ic_storage::Memory::size(&DefaultMemoryImpl::default()),
    )
}

/// Read the fixture’s persisted state for comparison across calls.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "inspect",
    guard = crate::transport::admit::<1>,
    decode = crate::transport::decode::<_, 1>,
    encode = crate::transport::encode::<_, 1>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn inspect((): ()) -> Status {
    let host = runtime();
    let db = open(&host);
    status(&db)
}
// Private fixture transport: entity identity, fields and indexes, without terminal layout.
type FieldView = (String, String, bool, String, Option<(String, String)>);
type IndexView = (String, Vec<String>, bool, bool);
type EntityView = (String, u32, String, Vec<FieldView>, Vec<IndexView>);
type TypeView = (String, String, Vec<(String, String, bool, String)>);

/// Inspect the schema admitted by the installed fixture.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "schema",
    guard = crate::transport::admit::<2>,
    decode = crate::transport::decode::<_, 2>,
    encode = crate::transport::encode::<_, 2>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn schema(selected: Option<String>) -> (Status, Vec<EntityView>, Vec<TypeView>) {
    let host = runtime();
    let db = open(&host);
    let mut included_types = std::collections::BTreeSet::new();
    let entities = db
        .schema()
        .entities()
        .iter()
        .filter(|entity| selected.as_deref().is_none_or(|name| name == entity.name))
        .map(|entity| {
            let fields = entity
                .fields
                .iter()
                .map(|field| {
                    include_type_names(field.value, &mut included_types);
                    let (kind, mut info) = type_view(field.value);
                    if let Some(default) = field.default {
                        let value = match default {
                            ic_db::LiteralDefault::Nat(v) => v.to_string(),
                            ic_db::LiteralDefault::Int(v) => v.to_string(),
                            ic_db::LiteralDefault::Bool(v) => v.to_string(),
                            ic_db::LiteralDefault::Text(v) => format!("{v:?}"),
                            ic_db::LiteralDefault::Blob(v) => console::hex(v),
                        };
                        info = joined_info(info, &format!("default {value}"));
                    }
                    if let Some(managed) = field.managed {
                        info = joined_info(
                            info,
                            match managed {
                                ic_db::ManagedTimestamp::CreatedAt => "managed created_at",
                                ic_db::ManagedTimestamp::UpdatedAt => "managed updated_at",
                            },
                        );
                    }
                    (
                        field.name.into(),
                        kind,
                        field.optional,
                        info,
                        field.reference.map(|reference| {
                            (
                                {
                                    let target = db
                                        .schema()
                                        .entities()
                                        .iter()
                                        .find(|target| target.id == reference.entity)
                                        .unwrap();
                                    format!("{}.{}", target.name, target.primary_key)
                                },
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
    let types = db
        .schema()
        .types()
        .filter(|definition| selected.is_none() || included_types.contains(definition.name))
        .map(|definition| {
            let (kind, members) = match &definition.body {
                ic_db::TypeBody::Wrapped(value) => {
                    let label = if matches!(value, FieldType::List { .. }) {
                        "List"
                    } else {
                        "Newtype"
                    };
                    let (kind, info) = type_view(*value);
                    (label, vec![("value".into(), kind, false, info)])
                }
                ic_db::TypeBody::Map {
                    key,
                    value,
                    max_items,
                } => {
                    let (key_type, key_info) = type_view(*key);
                    let (value_type, value_info) = type_view(value.value);
                    (
                        "Map",
                        vec![
                            (
                                "key".into(),
                                key_type,
                                false,
                                joined_info(format!("map max {max_items} entries"), &key_info),
                            ),
                            ("value".into(), value_type, value.optional, value_info),
                        ],
                    )
                }
                ic_db::TypeBody::Set { item, max_items } => {
                    let (kind, info) = type_view(*item);
                    (
                        "Set",
                        vec![(
                            "element".into(),
                            kind,
                            false,
                            joined_info(format!("max {max_items} items"), &info),
                        )],
                    )
                }
                ic_db::TypeBody::Record(fields) => (
                    "Record",
                    fields
                        .iter()
                        .map(|f| {
                            let (kind, info) = type_view(f.value);
                            (f.name.into(), kind, f.optional, info)
                        })
                        .collect(),
                ),
                ic_db::TypeBody::Enum(variants) => (
                    "Enum",
                    variants
                        .iter()
                        .map(|v| {
                            let (kind, info) = v.payload.map_or_else(
                                || ("-".into(), String::new()),
                                |p| type_view(p.value),
                            );
                            (
                                v.name.into(),
                                kind,
                                v.payload.is_some_and(|p| p.optional),
                                info,
                            )
                        })
                        .collect(),
                ),
            };
            (definition.name.into(), kind.into(), members)
        })
        .collect();
    (status(&db), entities, types)
}

// Presentation selection over an already admitted graph. Names terminate recursive
// definitions; this neither validates nor reconstructs the database schema.
fn include_type_names(value: FieldType, names: &mut std::collections::BTreeSet<&'static str>) {
    match value {
        FieldType::List { item, .. } => include_type_names(*item, names),
        FieldType::Named(link) if names.insert(link.name) => match &(link.definition)().body {
            ic_db::TypeBody::Wrapped(inner) => include_type_names(*inner, names),
            ic_db::TypeBody::Record(fields) => {
                for field in *fields {
                    include_type_names(field.value, names);
                }
            }
            ic_db::TypeBody::Enum(variants) => {
                for variant in *variants {
                    if let Some(payload) = variant.payload {
                        include_type_names(payload.value, names);
                    }
                }
            }
            ic_db::TypeBody::Map { key, value, .. } => {
                include_type_names(*key, names);
                include_type_names(value.value, names);
            }
            ic_db::TypeBody::Set { item, .. } => include_type_names(*item, names),
        },
        _ => {}
    }
}

/// Apply the selected mutation to a fixture entry.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "write",
    guard = crate::transport::admit::<4>,
    decode = crate::transport::decode::<_, 4>,
    encode = crate::transport::encode::<_, 4>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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
        crate::system::trap("M1 application trap after successful database commit");
    }
    if mode == 3 {
        return Err(4);
    }
    Ok(())
}

/// List entry IDs and amounts for the supplied category.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "category",
    guard = crate::transport::admit::<5>,
    decode = crate::transport::decode::<_, 5>,
    encode = crate::transport::encode::<_, 5>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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

/// Read the fixture document and its entity revision.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "document",
    guard = crate::transport::admit::<7>,
    decode = crate::transport::decode::<_, 7>,
    encode = crate::transport::encode::<_, 7>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn document((): ()) -> (DocumentValue, u64) {
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

/// Replace the fixture document with the supplied fields.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "replace_document",
    guard = crate::transport::admit::<8>,
    decode = crate::transport::decode::<_, 8>,
    encode = crate::transport::encode::<_, 8>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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

/// Look up the entry reference stored on the requested document.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "document_reference",
    guard = crate::transport::admit::<9>,
    decode = crate::transport::decode::<_, 9>,
    encode = crate::transport::encode::<_, 9>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn document_reference(id: u64) -> Option<u64> {
    let host = runtime();
    open(&host)
        .get::<Document>(id)
        .unwrap()
        .map(|row| row.entry_id)
}

/// Apply the selected cross-record reference scenario as one batch.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "reference_batch",
    guard = crate::transport::admit::<10>,
    decode = crate::transport::decode::<_, 10>,
    encode = crate::transport::encode::<_, 10>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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

/// Read the wide integer fields and their revision.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "wide_numbers",
    guard = crate::transport::admit::<11>,
    decode = crate::transport::decode::<_, 11>,
    encode = crate::transport::encode::<_, 11>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn wide_numbers((): ()) -> (WideValue, u64) {
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

/// Replace the fixture’s wide numbers fields with optional rollback injection.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "replace_wide_numbers",
    guard = crate::transport::admit::<12>,
    decode = crate::transport::decode::<_, 12>,
    encode = crate::transport::encode::<_, 12>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn replace_wide_numbers(
    ((signed, unsigned, optional_signed, optional_unsigned), trap): (WideValue, bool),
) -> EmptyReply {
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
        crate::system::trap("M1 wide-number application trap");
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

/// Read the narrow integer fields and their revision.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "narrow_numbers",
    guard = crate::transport::admit::<16>,
    decode = crate::transport::decode::<_, 16>,
    encode = crate::transport::encode::<_, 16>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn narrow_numbers((): ()) -> (NarrowValue, u64) {
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

/// Replace the fixture’s narrow numbers fields with optional rollback injection.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "replace_narrow_numbers",
    guard = crate::transport::admit::<17>,
    decode = crate::transport::decode::<_, 17>,
    encode = crate::transport::encode::<_, 17>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn replace_narrow_numbers((value, trap): (NarrowValue, bool)) -> EmptyReply {
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
        crate::system::trap("M1 narrow-number application trap");
    }
}

// Private decoder probe: transport carries opaque bytes, never decodes a row.

/// Validate the supplied bytes with the `BinaryObject` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "binary_decodes",
    guard = crate::transport::admit::<15>,
    decode = crate::transport::decode::<_, 15>,
    encode = crate::transport::encode::<_, 15>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn binary_decodes(bytes: ic_wire::ByteBuf) -> bool {
    BinaryObject::decode(bytes.as_ref()).is_ok()
}

/// Read the stored blob fields and their revision.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "binary_object",
    guard = crate::transport::admit::<13>,
    decode = crate::transport::decode::<_, 13>,
    encode = crate::transport::encode::<_, 13>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn binary_object((): ()) -> (BinaryValue, u64) {
    let host = runtime();
    let db = open(&host);
    let row = db.get::<BinaryObject>(1).unwrap().unwrap();
    (
        (row.payload.into(), row.optional.map(Into::into)),
        db.revision::<BinaryObject>().unwrap(),
    )
}

/// Replace the fixture’s binary object fields with optional rollback injection.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "replace_binary_object",
    guard = crate::transport::admit::<14>,
    decode = crate::transport::decode::<_, 14>,
    encode = crate::transport::encode::<_, 14>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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
        crate::system::trap("M1 binary application trap");
    }
    Ok(())
}

// Bytes cross the test transport; Principal admission and persistence belong to
// their respective owners, never Wire's representation of an identity.
type PrincipalValue = (ic_wire::ByteBuf, Option<ic_wire::ByteBuf>);

/// Read the stored principal fields and their revision.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "principal_record",
    guard = crate::transport::admit::<18>,
    decode = crate::transport::decode::<_, 18>,
    encode = crate::transport::encode::<_, 18>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn principal_record((): ()) -> (PrincipalValue, u64) {
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

/// Replace the fixture’s principal record fields with optional rollback injection.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "replace_principal_record",
    guard = crate::transport::admit::<19>,
    decode = crate::transport::decode::<_, 19>,
    encode = crate::transport::encode::<_, 19>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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
        crate::system::trap("M1 principal application trap");
    }
    Ok(())
}

/// Validate the supplied bytes with the `IdentityRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "principal_decodes",
    guard = crate::transport::admit::<20>,
    decode = crate::transport::decode::<_, 20>,
    encode = crate::transport::encode::<_, 20>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn principal_decodes(bytes: ic_wire::ByteBuf) -> bool {
    IdentityRow::decode(bytes.as_ref()).is_ok()
}

// Private qualification controls; product endpoints remain adapter-owned.
type IndexedWrite = (u64, ic_wire::ByteBuf, i64);

/// Apply the supplied indexed writes with optional rollback injection.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "indexed_batch",
    guard = crate::transport::admit::<21>,
    decode = crate::transport::decode::<_, 21>,
    encode = crate::transport::encode::<_, 21>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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
        crate::system::trap("M1 indexed batch application trap");
    }
    Ok(())
}

type IndexedRead = (Vec<u64>, Vec<(i64, u64)>);

/// Inspect the fixture indexes for the supplied principal.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "typed_indexes",
    guard = crate::transport::admit::<22>,
    decode = crate::transport::decode::<_, 22>,
    encode = crate::transport::encode::<_, 22>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
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

/// Populate the published/title index with the supplied records.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "text_index_batch",
    guard = crate::transport::admit::<23>,
    decode = crate::transport::decode::<_, 23>,
    encode = crate::transport::encode::<_, 23>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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
        crate::system::trap("M1 text index application trap");
    }
    Ok(())
}

/// List indexed titles and IDs under the selected published/title prefix.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "text_index",
    guard = crate::transport::admit::<24>,
    decode = crate::transport::decode::<_, 24>,
    encode = crate::transport::encode::<_, 24>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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

/// Apply the supplied optional-index mutations as a batch.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "optional_index_batch",
    guard = crate::transport::admit::<25>,
    decode = crate::transport::decode::<_, 25>,
    encode = crate::transport::encode::<_, 25>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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
        crate::system::trap("M1 optional index application trap");
    }
    Ok(())
}

/// Inspect optional index entries for the supplied value or absence.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "optional_indexes",
    guard = crate::transport::admit::<26>,
    decode = crate::transport::decode::<_, 26>,
    encode = crate::transport::encode::<_, 26>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
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

/// Apply optional reference changes and deletions in one batch.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "optional_reference_batch",
    guard = crate::transport::admit::<27>,
    decode = crate::transport::decode::<_, 27>,
    encode = crate::transport::encode::<_, 27>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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
        crate::system::trap("M1 optional reference application trap");
    }
    Ok(())
}

/// Read optional links and reverse lookups for the selected target.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "optional_references",
    guard = crate::transport::admit::<28>,
    decode = crate::transport::decode::<_, 28>,
    encode = crate::transport::encode::<_, 28>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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

// Private enum qualification controls. Transport uses ordinals solely to exercise
// the typed database boundary; it does not define the persisted enum codec.

/// Exercise bounded enum encoding and storage for the requested fixture case.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "enum_write",
    guard = crate::transport::admit::<29>,
    decode = crate::transport::decode::<_, 29>,
    encode = crate::transport::encode::<_, 29>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn enum_write((mode, state, previous): (u8, u8, Option<u8>)) -> Result<(Vec<u8>, u64), u8> {
    fn lifecycle(ordinal: u8) -> Result<schema::Lifecycle, ic_db::Error> {
        let bytes = [ordinal];
        let mut reader = ic_db::__private::Decoder::new(&bytes)?;
        reader.named()
    }
    use schema::EnumRow;
    let host = runtime();
    let mut db = open(&host);
    if mode != 0 {
        let row = EnumRow {
            id: 1,
            state: lifecycle(state).map_err(|_| 1_u8)?,
            previous: previous.map(lifecycle).transpose().map_err(|_| 1_u8)?,
        };
        let mut batch = Batch::new();
        if mode == 1 {
            batch.insert(&row)
        } else {
            batch.replace(&row)
        }
        .map_err(|_| 2_u8)?;
        db.commit(batch).map_err(|_| 2_u8)?;
        if mode == 3 {
            crate::system::trap("after enum replacement");
        }
    }
    Ok((
        db.get::<EnumRow>(1)
            .unwrap()
            .map(|row| {
                assert_eq!(
                    db.index_prefix::<EnumRow>("state,id", &[(&row.state).into()], 3)
                        .unwrap(),
                    std::slice::from_ref(&row)
                );
                for value in [
                    schema::Lifecycle::Archived,
                    schema::Lifecycle::Draft,
                    schema::Lifecycle::Ready,
                ] {
                    let expected = if row.previous == Some(value) {
                        vec![row.clone()]
                    } else {
                        vec![]
                    };
                    assert_eq!(
                        db.index_prefix::<EnumRow>("previous", &[(&value).into()], 3)
                            .unwrap(),
                        expected
                    );
                    if row.state != value {
                        assert!(
                            db.index_prefix::<EnumRow>("state,id", &[(&value).into()], 3)
                                .unwrap()
                                .is_empty()
                        );
                    }
                }
                row.encode().unwrap()
            })
            .unwrap_or_default(),
        db.revision::<EnumRow>().unwrap(),
    ))
}

/// Validate the supplied bytes with the `EnumRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "enum_decode",
    guard = crate::transport::admit::<30>,
    decode = crate::transport::decode::<_, 30>,
    encode = crate::transport::encode::<_, 30>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn enum_decode(bytes: Vec<u8>) -> bool {
    schema::EnumRow::decode(&bytes).is_ok()
}

// Inspection projection only: names and constraints stay separate from terminal layout.
fn type_view(value: FieldType) -> (String, String) {
    match value {
        FieldType::List { item, max_items } => {
            let (kind, info) = type_view(*item);
            let item_info = if info.is_empty() {
                info
            } else {
                format!("item {info}")
            };
            (
                format!("List<{kind}>"),
                joined_info(format!("max {max_items} items"), &item_info),
            )
        }
        FieldType::Named(link) => (link.name.into(), String::new()),
        FieldType::Text { max_bytes } => ("Text".into(), format!("max {max_bytes} bytes")),
        FieldType::Blob { max_bytes } => ("Blob".into(), format!("max {max_bytes} bytes")),
        FieldType::Principal => ("Principal".into(), "max 29 bytes".into()),
        FieldType::Subaccount => ("Subaccount".into(), "32 bytes".into()),
        FieldType::Date => ("Date".into(), "4 bytes; days; 0000–9999".into()),
        FieldType::Timestamp => ("Timestamp".into(), "8 bytes; Unix milliseconds".into()),
        FieldType::Decimal { scale } => {
            ("Decimal".into(), format!("17 bytes; scale {scale}; exact"))
        }
        FieldType::Duration => (
            "Duration".into(),
            "8 bytes; nonnegative milliseconds".into(),
        ),
        FieldType::Ulid => ("Ulid".into(), "16 bytes".into()),
        FieldType::Account => (
            "Account".into(),
            "owner max 29 bytes; subaccount 32 bytes".into(),
        ),
        scalar => (format!("{scalar:?}"), String::new()),
    }
}

fn joined_info(bound: String, nested: &str) -> String {
    if bound.is_empty() {
        nested.to_owned()
    } else if nested.is_empty() {
        bound
    } else {
        format!("{bound}; {nested}")
    }
}

/// Exercise bounded nested encoding and storage for the requested fixture case.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "nested_write",
    guard = crate::transport::admit::<31>,
    decode = crate::transport::decode::<_, 31>,
    encode = crate::transport::encode::<_, 31>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn nested_write((mode, shape, size): (u8, u8, u8)) -> Result<(Vec<u8>, u64), u8> {
    use schema::{Branch, Details, NestedPayload, NestedRow};
    fn tree(depth: u8) -> NestedPayload {
        if depth == 0 {
            NestedPayload::Idle
        } else {
            NestedPayload::Pair(Branch {
                left: Box::new(tree(depth - 1)),
                right: Box::new(tree(depth - 1)),
            })
        }
    }
    let host = runtime();
    let mut db = open(&host);
    if mode != 0 {
        if size > 33 || (shape == 2 && size > 7) {
            return Err(1);
        }
        let payload = match shape {
            0 => NestedPayload::Data(Details {
                label: "x".repeat(size.into()),
                x: 0x1234,
            }),
            1 => (0..size).fold(NestedPayload::Idle, |value, _| {
                NestedPayload::Loop(Box::new(value))
            }),
            2 => tree(size),
            _ => return Err(1),
        };
        let row = NestedRow {
            id: 1,
            meta: None,
            payload,
        };
        let mut batch = Batch::new();
        if mode == 1 {
            batch.insert(&row)
        } else {
            batch.replace(&row)
        }
        .map_err(|_| 1_u8)?;
        db.commit(batch).map_err(|_| 2_u8)?;
        if mode == 3 {
            crate::system::trap("after nested replacement");
        }
    }
    Ok((
        db.get::<NestedRow>(1)
            .unwrap()
            .map(|row| row.encode().unwrap())
            .unwrap_or_default(),
        db.revision::<NestedRow>().unwrap(),
    ))
}

/// Validate the supplied bytes with the `NestedRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "nested_decodes",
    guard = crate::transport::admit::<32>,
    decode = crate::transport::decode::<_, 32>,
    encode = crate::transport::encode::<_, 32>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn nested_decodes(bytes: Vec<u8>) -> bool {
    schema::NestedRow::decode(&bytes).is_ok()
}

/// Exercise bounded list encoding and storage for the requested fixture case.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "list_write",
    guard = crate::transport::admit::<33>,
    decode = crate::transport::decode::<_, 33>,
    encode = crate::transport::encode::<_, 33>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn list_write((mode, shape, size): (u8, u8, u8)) -> Result<(Vec<u8>, u64), u8> {
    use schema::{ListNode, ListRow, Sample, Samples};
    fn tree(depth: u8) -> ListNode {
        ListNode {
            children: if depth == 0 {
                vec![]
            } else {
                vec![tree(depth - 1), tree(depth - 1)]
            },
        }
    }
    let host = runtime();
    let mut db = open(&host);
    if mode != 0 {
        if size > 33 || (shape == 3 && size > 7) {
            return Err(1);
        }
        let mut row = ListRow {
            id: 1,
            labels: None,
            numbers: Samples(vec![]),
            trees: vec![],
        };
        match shape {
            0 => row.numbers = Samples(vec![Sample(258); usize::from(size)]),
            1 => {
                row.trees =
                    vec![
                        (0..size).fold(ListNode { children: vec![] }, |child, _| ListNode {
                            children: vec![child],
                        }),
                    ]
            }
            2 => row.labels = Some(vec!["x".repeat(size.into())]),
            3 => row.trees = vec![tree(size)],
            _ => return Err(1),
        }
        let mut batch = Batch::new();
        if mode == 1 {
            batch.insert(&row)
        } else {
            batch.replace(&row)
        }
        .map_err(|_| 1_u8)?;
        db.commit(batch).map_err(|_| 2_u8)?;
        if mode == 3 {
            crate::system::trap("after list replacement");
        }
    }
    Ok((
        db.get::<ListRow>(1)
            .unwrap()
            .map(|row| row.encode().unwrap())
            .unwrap_or_default(),
        db.revision::<ListRow>().unwrap(),
    ))
}

/// Validate the supplied bytes with the `ListRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "list_decodes",
    guard = crate::transport::admit::<34>,
    decode = crate::transport::decode::<_, 34>,
    encode = crate::transport::encode::<_, 34>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn list_decodes(bytes: Vec<u8>) -> bool {
    schema::ListRow::decode(&bytes).is_ok()
}

/// Exercise bounded set encoding and storage for the requested fixture case.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "set_write",
    guard = crate::transport::admit::<35>,
    decode = crate::transport::decode::<_, 35>,
    encode = crate::transport::encode::<_, 35>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn set_write((mode, reverse, size): (u8, bool, u8)) -> Result<(Vec<u8>, u64), u8> {
    use schema::{Scores, SetRow};
    let host = runtime();
    let mut db = open(&host);
    if mode != 0 {
        let mut values: Vec<i16> = [-1, 0, 256, 257].into_iter().take(size.into()).collect();
        if reverse {
            values.reverse();
        }
        let row = SetRow {
            id: 1,
            scores: Scores(values.into_iter().collect()),
        };
        let mut batch = Batch::new();
        if mode == 1 {
            batch.insert(&row)
        } else {
            batch.replace(&row)
        }
        .map_err(|_| 1_u8)?;
        db.commit(batch).map_err(|_| 2_u8)?;
        if mode == 3 {
            crate::system::trap("after set replacement");
        }
    }
    Ok((
        db.get::<SetRow>(1)
            .unwrap()
            .map(|row| row.encode().unwrap())
            .unwrap_or_default(),
        db.revision::<SetRow>().unwrap(),
    ))
}

/// Validate the supplied bytes with the `SetRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "set_decodes",
    guard = crate::transport::admit::<36>,
    decode = crate::transport::decode::<_, 36>,
    encode = crate::transport::encode::<_, 36>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn set_decodes(bytes: Vec<u8>) -> bool {
    schema::SetRow::decode(&bytes).is_ok()
}

/// Exercise bounded map encoding and storage for the requested fixture case.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "map_write",
    guard = crate::transport::admit::<37>,
    decode = crate::transport::decode::<_, 37>,
    encode = crate::transport::encode::<_, 37>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn map_write((mode, shape): (u8, u8)) -> Result<(Vec<u8>, u64), u8> {
    use schema::{MapRow, Rates};
    let host = runtime();
    let mut db = open(&host);
    if mode != 0 {
        let mut entries = vec![
            (256, Some("é".into())),
            (-1, None),
            (0, Some(String::new())),
        ];
        match shape {
            0 => {}
            1 => entries.reverse(),
            2 => entries.push((1, None)),
            3 => entries[0].1 = Some("12345".into()),
            4 => entries.clear(),
            _ => return Err(1),
        }
        let row = MapRow {
            id: 1,
            values: Rates(entries.into_iter().collect()),
        };
        let mut batch = Batch::new();
        if mode == 1 {
            batch.insert(&row)
        } else {
            batch.replace(&row)
        }
        .map_err(|_| 1_u8)?;
        db.commit(batch).map_err(|_| 2_u8)?;
        if mode == 3 {
            crate::system::trap("after map replacement");
        }
    }
    Ok((
        db.get::<MapRow>(1)
            .unwrap()
            .map(|row| row.encode().unwrap())
            .unwrap_or_default(),
        db.revision::<MapRow>().unwrap(),
    ))
}

/// Validate the supplied bytes with the `MapRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "map_decodes",
    guard = crate::transport::admit::<38>,
    decode = crate::transport::decode::<_, 38>,
    encode = crate::transport::encode::<_, 38>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn map_decodes(bytes: Vec<u8>) -> bool {
    schema::MapRow::decode(&bytes).is_ok()
}

/// Encode a recursive map of the requested shape and size.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "map_tree_encode",
    guard = crate::transport::admit::<39>,
    decode = crate::transport::decode::<_, 39>,
    encode = crate::transport::encode::<_, 39>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn map_tree_encode((star, size): (bool, u16)) -> Result<Vec<u8>, u8> {
    use schema::{MapTree, MapTreeRow};
    if (star && size > 256) || (!star && size > 32) {
        return Err(1);
    }
    let tree = if star {
        MapTree((0..size).map(|key| (key, MapTree::default())).collect())
    } else {
        (0..size).fold(MapTree::default(), |child, _| MapTree([(0, child)].into()))
    };
    MapTreeRow { id: 1, tree }.encode().map_err(|_| 1)
}

/// Validate the supplied bytes with the `MapTreeRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "map_tree_decodes",
    guard = crate::transport::admit::<40>,
    decode = crate::transport::decode::<_, 40>,
    encode = crate::transport::encode::<_, 40>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn map_tree_decodes(bytes: Vec<u8>) -> bool {
    schema::MapTreeRow::decode(&bytes).is_ok()
}

type WideRow = (u64, Option<[u8; 32]>, [u8; 32]);

/// Apply the supplied U256 rows in one batch with optional rollback injection.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "u256_batch",
    guard = crate::transport::admit::<41>,
    decode = crate::transport::decode::<_, 41>,
    encode = crate::transport::encode::<_, 41>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn u256_batch((rows, trap): (Vec<WideRow>, bool)) -> Result<(), u8> {
    use schema::BigNumbers;
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for (id, maybe, value) in rows {
        let row = BigNumbers {
            id,
            maybe: maybe.map(ic_db::U256::from_be_bytes),
            value: ic_db::U256::from_be_bytes(value),
            zone: true,
        };
        if db.get::<BigNumbers>(id).map_err(|_| 1_u8)?.is_some() {
            batch.replace(&row)
        } else {
            batch.insert(&row)
        }
        .map_err(|_| 1_u8)?;
    }
    db.commit(batch).map_err(|_| 2_u8)?;
    if trap {
        crate::system::trap("after U256 batch");
    }
    Ok(())
}

/// Read the stored U256 rows and their revision.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "u256_rows",
    guard = crate::transport::admit::<42>,
    decode = crate::transport::decode::<_, 42>,
    encode = crate::transport::encode::<_, 42>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn u256_rows((): ()) -> (Vec<(u64, Vec<u8>)>, u64) {
    use schema::BigNumbers;
    let host = runtime();
    let db = open(&host);
    (
        db.index_prefix::<BigNumbers>("zone,value,id", &[true.into()], 32)
            .unwrap()
            .into_iter()
            .map(|row| (row.id, row.encode().unwrap()))
            .collect(),
        db.revision::<BigNumbers>().unwrap(),
    )
}

/// Look up matching row IDs for the supplied U256 value.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "u256_lookup",
    guard = crate::transport::admit::<43>,
    decode = crate::transport::decode::<_, 43>,
    encode = crate::transport::encode::<_, 43>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn u256_lookup(value: [u8; 32]) -> Vec<u64> {
    let host = runtime();
    open(&host)
        .index_prefix::<schema::BigNumbers>(
            "zone,value,id",
            &[true.into(), ic_db::U256::from_be_bytes(value).into()],
            32,
        )
        .unwrap()
        .into_iter()
        .map(|row| row.id)
        .collect()
}

/// Validate the supplied bytes with the `BigNumbers` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "u256_decodes",
    guard = crate::transport::admit::<44>,
    decode = crate::transport::decode::<_, 44>,
    encode = crate::transport::encode::<_, 44>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn u256_decodes(bytes: Vec<u8>) -> bool {
    schema::BigNumbers::decode(&bytes).is_ok()
}

type FloatInput = (u64, Option<u64>, u32, u64);

/// Apply the supplied float rows in one batch with optional rollback injection.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "float_batch",
    guard = crate::transport::admit::<45>,
    decode = crate::transport::decode::<_, 45>,
    encode = crate::transport::encode::<_, 45>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn float_batch((rows, trap): (Vec<FloatInput>, bool)) -> Result<(), u8> {
    use ic_db::{Float32, Float64};
    use schema::FloatRow;
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for (id, maybe, small, value) in rows {
        let maybe = maybe
            .map(|bits| Float64::try_new(f64::from_bits(bits)).ok_or(1_u8))
            .transpose()?;
        let row = FloatRow {
            id,
            maybe,
            zone: true,
            small: Float32::try_new(f32::from_bits(small)).ok_or(1_u8)?,
            value: Float64::try_new(f64::from_bits(value)).ok_or(1_u8)?,
        };
        if db.get::<FloatRow>(id).map_err(|_| 1_u8)?.is_some() {
            batch.replace(&row)
        } else {
            batch.insert(&row)
        }
        .map_err(|_| 1_u8)?;
    }
    db.commit(batch).map_err(|_| 2_u8)?;
    if trap {
        crate::system::trap("after float batch");
    }
    Ok(())
}

/// Read the stored float rows and their revision.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "float_rows",
    guard = crate::transport::admit::<46>,
    decode = crate::transport::decode::<_, 46>,
    encode = crate::transport::encode::<_, 46>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn float_rows(small: bool) -> (Vec<(u64, Vec<u8>)>, u64) {
    use schema::FloatRow;
    let host = runtime();
    let db = open(&host);
    let index = if small {
        "zone,small,id"
    } else {
        "zone,value,id"
    };
    (
        db.index_prefix::<FloatRow>(index, &[true.into()], 32)
            .unwrap()
            .into_iter()
            .map(|row| (row.id, row.encode().unwrap()))
            .collect(),
        db.revision::<FloatRow>().unwrap(),
    )
}

/// Look up matching row IDs for the supplied float value.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "float_lookup",
    guard = crate::transport::admit::<47>,
    decode = crate::transport::decode::<_, 47>,
    encode = crate::transport::encode::<_, 47>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn float_lookup(bits: u64) -> Result<Vec<u64>, u8> {
    let value = ic_db::Float64::try_new(f64::from_bits(bits)).ok_or(1_u8)?;
    let host = runtime();
    Ok(open(&host)
        .index_prefix::<schema::FloatRow>("maybe", &[value.into()], 32)
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect())
}

/// Validate the supplied bytes with the `FloatRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "float_decodes",
    guard = crate::transport::admit::<48>,
    decode = crate::transport::decode::<_, 48>,
    encode = crate::transport::encode::<_, 48>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn float_decodes(bytes: Vec<u8>) -> bool {
    schema::FloatRow::decode(&bytes).is_ok()
}

type AccountInput = (Vec<u8>, Option<[u8; 32]>);
type AccountRowInput = (u64, Option<AccountInput>, [u8; 32], AccountInput);
fn admitted_account((owner, subaccount): AccountInput) -> Result<ic_db::Account, u8> {
    Ok(ic_db::Account {
        owner: Principal::try_from_slice(&owner).map_err(|_| 1_u8)?,
        subaccount,
    })
}

/// Apply the supplied account rows in one batch with optional rollback injection.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "account_batch",
    guard = crate::transport::admit::<49>,
    decode = crate::transport::decode::<_, 49>,
    encode = crate::transport::encode::<_, 49>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn account_batch((rows, trap): (Vec<AccountRowInput>, bool)) -> Result<(), u8> {
    use schema::AccountRow;
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for (id, maybe, sub, value) in rows {
        let row = AccountRow {
            id,
            maybe: maybe.map(admitted_account).transpose()?,
            sub,
            value: admitted_account(value)?,
            zone: true,
        };
        if db.get::<AccountRow>(id).map_err(|_| 1_u8)?.is_some() {
            batch.replace(&row)
        } else {
            batch.insert(&row)
        }
        .map_err(|_| 1_u8)?;
    }
    db.commit(batch).map_err(|_| 2_u8)?;
    if trap {
        crate::system::trap("after account batch");
    }
    Ok(())
}

/// Read the stored account rows and their revision.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "account_rows",
    guard = crate::transport::admit::<50>,
    decode = crate::transport::decode::<_, 50>,
    encode = crate::transport::encode::<_, 50>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn account_rows(sub: bool) -> (Vec<(u64, Vec<u8>)>, u64) {
    use schema::AccountRow;
    let host = runtime();
    let db = open(&host);
    let index = if sub { "zone,sub,id" } else { "zone,value,id" };
    (
        db.index_prefix::<AccountRow>(index, &[true.into()], 32)
            .unwrap()
            .into_iter()
            .map(|r| (r.id, r.encode().unwrap()))
            .collect(),
        db.revision::<AccountRow>().unwrap(),
    )
}

/// Look up matching row IDs for the supplied account value.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "account_lookup",
    guard = crate::transport::admit::<51>,
    decode = crate::transport::decode::<_, 51>,
    encode = crate::transport::encode::<_, 51>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn account_lookup(value: AccountInput) -> Result<Vec<u64>, u8> {
    let value = admitted_account(value)?;
    let host = runtime();
    Ok(open(&host)
        .index_prefix::<schema::AccountRow>("maybe", &[value.into()], 32)
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect())
}

/// Validate the supplied bytes with the `AccountRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "account_decodes",
    guard = crate::transport::admit::<52>,
    decode = crate::transport::decode::<_, 52>,
    encode = crate::transport::encode::<_, 52>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn account_decodes(bytes: Vec<u8>) -> bool {
    schema::AccountRow::decode(&bytes).is_ok()
}

type UlidInput = (u64, Option<[u8; 16]>, [u8; 16]);

/// Apply the supplied ULID rows in one batch with optional rollback injection.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "ulid_batch",
    guard = crate::transport::admit::<53>,
    decode = crate::transport::decode::<_, 53>,
    encode = crate::transport::encode::<_, 53>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn ulid_batch((rows, trap): (Vec<UlidInput>, bool)) -> Result<(), u8> {
    use schema::UlidRow;
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for (id, maybe, value) in rows {
        let row = UlidRow {
            id,
            maybe: maybe.map(ic_db::Ulid::from_bytes),
            value: ic_db::Ulid::from_bytes(value),
            zone: true,
        };
        if db.get::<UlidRow>(id).map_err(|_| 1_u8)?.is_some() {
            batch.replace(&row)
        } else {
            batch.insert(&row)
        }
        .map_err(|_| 1_u8)?;
    }
    db.commit(batch).map_err(|_| 2_u8)?;
    if trap {
        crate::system::trap("after Ulid batch");
    }
    Ok(())
}

/// Read the stored ULID rows and their revision.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "ulid_rows",
    guard = crate::transport::admit::<54>,
    decode = crate::transport::decode::<_, 54>,
    encode = crate::transport::encode::<_, 54>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn ulid_rows((): ()) -> (Vec<(u64, Vec<u8>)>, u64) {
    use schema::UlidRow;
    let host = runtime();
    let db = open(&host);
    (
        db.index_prefix::<UlidRow>("zone,value,id", &[true.into()], 32)
            .unwrap()
            .into_iter()
            .map(|row| (row.id, row.encode().unwrap()))
            .collect(),
        db.revision::<UlidRow>().unwrap(),
    )
}

/// Look up matching row IDs for the supplied ULID value.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "ulid_lookup",
    guard = crate::transport::admit::<55>,
    decode = crate::transport::decode::<_, 55>,
    encode = crate::transport::encode::<_, 55>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn ulid_lookup(value: [u8; 16]) -> Vec<u64> {
    let host = runtime();
    open(&host)
        .index_prefix::<schema::UlidRow>(
            "zone,value,id",
            &[true.into(), ic_db::Ulid::from_bytes(value).into()],
            32,
        )
        .unwrap()
        .into_iter()
        .map(|row| row.id)
        .collect()
}

/// Validate the supplied bytes with the `UlidRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "ulid_decodes",
    guard = crate::transport::admit::<56>,
    decode = crate::transport::decode::<_, 56>,
    encode = crate::transport::encode::<_, 56>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn ulid_decodes(bytes: Vec<u8>) -> bool {
    schema::UlidRow::decode(&bytes).is_ok()
}

/// Parse a ULID and return its formatted text and bytes.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "ulid_text",
    guard = crate::transport::admit::<57>,
    decode = crate::transport::decode::<_, 57>,
    encode = crate::transport::encode::<_, 57>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn ulid_text(text: String) -> Result<(String, [u8; 16]), u8> {
    let value = text.parse::<ic_db::Ulid>().map_err(|_| 1_u8)?;
    Ok((value.to_string(), value.to_bytes()))
}

type DateInput = (u64, Option<i32>, i32);

/// Apply the supplied date rows in one batch with optional rollback injection.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "date_batch",
    guard = crate::transport::admit::<58>,
    decode = crate::transport::decode::<_, 58>,
    encode = crate::transport::encode::<_, 58>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn date_batch((rows, trap): (Vec<DateInput>, bool)) -> Result<(), u8> {
    use schema::DateRow;
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for (id, maybe, value) in rows {
        let row = DateRow {
            id,
            maybe: match maybe {
                Some(days) => Some(ic_db::Date::try_from_days_since_epoch(days).ok_or(1_u8)?),
                None => None,
            },
            value: ic_db::Date::try_from_days_since_epoch(value).ok_or(1_u8)?,
            zone: true,
        };
        if db.get::<DateRow>(id).map_err(|_| 1_u8)?.is_some() {
            batch.replace(&row)
        } else {
            batch.insert(&row)
        }
        .map_err(|_| 1_u8)?;
    }
    db.commit(batch).map_err(|_| 2_u8)?;
    if trap {
        crate::system::trap("after Date batch");
    }
    Ok(())
}

/// Read the stored date rows and their revision.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "date_rows",
    guard = crate::transport::admit::<59>,
    decode = crate::transport::decode::<_, 59>,
    encode = crate::transport::encode::<_, 59>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn date_rows((): ()) -> (Vec<(u64, Vec<u8>)>, u64) {
    use schema::DateRow;
    let host = runtime();
    let db = open(&host);
    (
        db.index_prefix::<DateRow>("zone,value,id", &[true.into()], 32)
            .unwrap()
            .into_iter()
            .map(|row| (row.id, row.encode().unwrap()))
            .collect(),
        db.revision::<DateRow>().unwrap(),
    )
}

/// Look up matching row IDs for the supplied date value.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "date_lookup",
    guard = crate::transport::admit::<60>,
    decode = crate::transport::decode::<_, 60>,
    encode = crate::transport::encode::<_, 60>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn date_lookup(value: i32) -> Result<Vec<u64>, u8> {
    let host = runtime();
    Ok(open(&host)
        .index_prefix::<schema::DateRow>(
            "zone,value,id",
            &[
                true.into(),
                ic_db::Date::try_from_days_since_epoch(value)
                    .ok_or(1_u8)?
                    .into(),
            ],
            32,
        )
        .unwrap()
        .into_iter()
        .map(|row| row.id)
        .collect())
}

/// Validate the supplied bytes with the `DateRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "date_decodes",
    guard = crate::transport::admit::<61>,
    decode = crate::transport::decode::<_, 61>,
    encode = crate::transport::encode::<_, 61>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn date_decodes(bytes: Vec<u8>) -> bool {
    schema::DateRow::decode(&bytes).is_ok()
}

type TimeInput = (u64, Option<u64>, u64, Option<i64>, i64);

/// Apply the supplied time rows in one batch with optional rollback injection.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "time_batch",
    guard = crate::transport::admit::<62>,
    decode = crate::transport::decode::<_, 62>,
    encode = crate::transport::encode::<_, 62>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn time_batch((rows, trap): (Vec<TimeInput>, bool)) -> Result<(), u8> {
    use schema::TimeRow;
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for (id, delay, duration, maybe, timestamp) in rows {
        let row = TimeRow {
            zone: true,
            id,
            delay: delay.map(ic_db::Duration::from_millis),
            duration: ic_db::Duration::from_millis(duration),
            maybe: maybe.map(ic_db::Timestamp::from_millis),
            timestamp: ic_db::Timestamp::from_millis(timestamp),
        };
        if db.get::<TimeRow>(id).map_err(|_| 1_u8)?.is_some() {
            batch.replace(&row)
        } else {
            batch.insert(&row)
        }
        .map_err(|_| 1_u8)?;
    }
    db.commit(batch).map_err(|_| 2_u8)?;
    if trap {
        crate::system::trap("after time batch");
    }
    Ok(())
}

/// Read the stored time rows and their revision.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "time_rows",
    guard = crate::transport::admit::<63>,
    decode = crate::transport::decode::<_, 63>,
    encode = crate::transport::encode::<_, 63>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn time_rows(duration: bool) -> (Vec<(u64, Vec<u8>)>, u64) {
    use schema::TimeRow;
    let host = runtime();
    let db = open(&host);
    let index = if duration {
        "zone,duration,id"
    } else {
        "zone,timestamp,id"
    };
    (
        db.index_prefix::<TimeRow>(index, &[true.into()], 32)
            .unwrap()
            .into_iter()
            .map(|r| (r.id, r.encode().unwrap()))
            .collect(),
        db.revision::<TimeRow>().unwrap(),
    )
}

/// Validate the supplied bytes with the `TimeRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "time_decodes",
    guard = crate::transport::admit::<64>,
    decode = crate::transport::decode::<_, 64>,
    encode = crate::transport::encode::<_, 64>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn time_decodes(bytes: Vec<u8>) -> bool {
    schema::TimeRow::decode(&bytes).is_ok()
}

/// Parse the supplied duration and timestamp strings.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "time_text",
    guard = crate::transport::admit::<65>,
    decode = crate::transport::decode::<_, 65>,
    encode = crate::transport::encode::<_, 65>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn time_text((duration, timestamp): (String, String)) -> Result<(u64, i64), u8> {
    Ok((
        ic_db::Duration::parse_flexible(&duration)
            .map_err(|_| 1_u8)?
            .as_millis(),
        ic_db::Timestamp::parse_flexible(&timestamp)
            .map_err(|_| 2_u8)?
            .as_millis(),
    ))
}

/// Exercise conversions and arithmetic on the supplied time operands.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "time_math",
    guard = crate::transport::admit::<66>,
    decode = crate::transport::decode::<_, 66>,
    encode = crate::transport::encode::<_, 66>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn time_math((micros, nanos, duration): (i64, i64, u64)) -> (i64, i64, i64, i64) {
    let duration = ic_db::Duration::from_millis(duration);
    (
        ic_db::Timestamp::from_micros(micros).as_millis(),
        ic_db::Timestamp::from_nanos(nanos).as_millis(),
        (ic_db::Timestamp::MIN + duration).as_millis(),
        (ic_db::Timestamp::MAX - duration).as_millis(),
    )
}

// Strings are private transport inputs; the database receives exact Decimal values.
type DecimalInput = (u64, Option<String>, String);

/// Apply the supplied decimal rows in one batch with optional rollback injection.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "decimal_batch",
    guard = crate::transport::admit::<67>,
    decode = crate::transport::decode::<_, 67>,
    encode = crate::transport::encode::<_, 67>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn decimal_batch((rows, trap): (Vec<DecimalInput>, bool)) -> Result<(), u8> {
    use schema::DecimalRow;
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for (id, maybe, value) in rows {
        let row = DecimalRow {
            id,
            maybe: maybe.map(|v| v.parse()).transpose().map_err(|_| 1_u8)?,
            value: value.parse().map_err(|_| 1_u8)?,
            zone: true,
        };
        if db.get::<DecimalRow>(id).map_err(|_| 1_u8)?.is_some() {
            batch.replace(&row)
        } else {
            batch.insert(&row)
        }
        .map_err(|_| 1_u8)?;
    }
    db.commit(batch).map_err(|_| 2_u8)?;
    if trap {
        crate::system::trap("after decimal batch");
    }
    Ok(())
}

/// Read the stored decimal rows and their revision.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "decimal_rows",
    guard = crate::transport::admit::<68>,
    decode = crate::transport::decode::<_, 68>,
    encode = crate::transport::encode::<_, 68>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn decimal_rows((): ()) -> (Vec<(u64, Vec<u8>)>, u64) {
    use schema::DecimalRow;
    let host = runtime();
    let db = open(&host);
    (
        db.index_prefix::<DecimalRow>("zone,value,id", &[true.into()], 32)
            .unwrap()
            .into_iter()
            .map(|r| (r.id, r.encode().unwrap()))
            .collect(),
        db.revision::<DecimalRow>().unwrap(),
    )
}

/// Validate the supplied bytes with the `DecimalRow` row decoder.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "decimal_decodes",
    guard = crate::transport::admit::<69>,
    decode = crate::transport::decode::<_, 69>,
    encode = crate::transport::encode::<_, 69>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn decimal_decodes(bytes: Vec<u8>) -> bool {
    schema::DecimalRow::decode(&bytes).is_ok()
}

type KeyInput = ([u8; 16], Option<u64>, u64);
type NumberedInput = (u64, Option<[u8; 16]>);
type KeyBatchInput = (
    Vec<KeyInput>,
    Vec<NumberedInput>,
    Vec<[u8; 16]>,
    Vec<u64>,
    bool,
);

/// Apply ULID-key changes, links and deletions in one batch.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "key_batch",
    guard = crate::transport::admit::<70>,
    decode = crate::transport::decode::<_, 70>,
    encode = crate::transport::encode::<_, 70>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn key_batch((rows, links, deletes, numbered_deletes, trap): KeyBatchInput) -> Result<(), u8> {
    use schema::{KeyRow, Numbered};
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for (id, owner, value) in rows {
        let row = KeyRow {
            id: ic_db::Ulid::from_bytes(id),
            owner,
            value,
        };
        if db.get::<KeyRow>(row.id).map_err(|_| 1_u8)?.is_some() {
            batch.replace(&row)
        } else {
            batch.insert(&row)
        }
        .map_err(|_| 1_u8)?;
    }
    for (id, target) in links {
        let row = Numbered {
            id,
            target: target.map(ic_db::Ulid::from_bytes),
        };
        if db.get::<Numbered>(id).map_err(|_| 1_u8)?.is_some() {
            batch.replace(&row)
        } else {
            batch.insert(&row)
        }
        .map_err(|_| 1_u8)?;
    }
    for id in deletes {
        batch
            .delete::<KeyRow>(ic_db::Ulid::from_bytes(id))
            .map_err(|_| 1_u8)?;
    }
    for id in numbered_deletes {
        batch.delete::<Numbered>(id).map_err(|_| 1_u8)?;
    }
    db.commit(batch).map_err(|_| 2_u8)?;
    if trap {
        crate::system::trap("after ULID primary-key batch");
    }
    Ok(())
}

/// Page ULID-keyed rows after the supplied position.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "key_page",
    guard = crate::transport::admit::<71>,
    decode = crate::transport::decode::<_, 71>,
    encode = crate::transport::encode::<_, 71>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn key_page((after, limit): (Option<[u8; 16]>, u8)) -> (Vec<KeyInput>, Option<[u8; 16]>, u64) {
    use schema::KeyRow;
    let host = runtime();
    let db = open(&host);
    let page = db
        .page::<KeyRow>(after.map(ic_db::Ulid::from_bytes), usize::from(limit))
        .unwrap();
    (
        page.rows
            .into_iter()
            .map(|r| (r.id.to_bytes(), r.owner, r.value))
            .collect(),
        page.next_after.map(ic_db::Ulid::to_bytes),
        db.revision::<KeyRow>().unwrap(),
    )
}

/// Read a ULID-keyed row and its indexed lookup results.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "key_lookup",
    guard = crate::transport::admit::<72>,
    decode = crate::transport::decode::<_, 72>,
    encode = crate::transport::encode::<_, 72>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn key_lookup((id, value): ([u8; 16], u64)) -> (Option<KeyInput>, Vec<KeyInput>) {
    use schema::KeyRow;
    let host = runtime();
    let db = open(&host);
    let view = |r: KeyRow| (r.id.to_bytes(), r.owner, r.value);
    (
        db.get::<KeyRow>(ic_db::Ulid::from_bytes(id))
            .unwrap()
            .map(view),
        db.query::<KeyRow>()
            .eq("value", value)
            .work_limit(1)
            .first()
            .unwrap()
            .into_iter()
            .map(view)
            .collect(),
    )
}

type SignedInput = (i128, Option<[u8; 32]>, u64);
type WideInput = ([u8; 32], Option<i128>, u64);
type IntegerKeyBatch = (
    Vec<SignedInput>,
    Vec<WideInput>,
    Vec<i128>,
    Vec<[u8; 32]>,
    bool,
);

/// Apply signed and wide primary-key changes in one batch.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "integer_key_batch",
    guard = crate::transport::admit::<73>,
    decode = crate::transport::decode::<_, 73>,
    encode = crate::transport::encode::<_, 73>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn integer_key_batch(
    (signed, wide, delete_signed, delete_wide, trap): IntegerKeyBatch,
) -> Result<(), u8> {
    use schema::{SignedKey, WideKey};
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for (id, peer, rank) in signed {
        let row = SignedKey {
            id,
            peer: peer.map(ic_db::U256::from_be_bytes),
            rank,
        };
        if db.get::<SignedKey>(id).map_err(|_| 1_u8)?.is_some() {
            batch.replace(&row)
        } else {
            batch.insert(&row)
        }
        .map_err(|_| 1_u8)?;
    }
    for (id, peer, rank) in wide {
        let row = WideKey {
            id: ic_db::U256::from_be_bytes(id),
            peer,
            rank,
        };
        if db.get::<WideKey>(row.id).map_err(|_| 1_u8)?.is_some() {
            batch.replace(&row)
        } else {
            batch.insert(&row)
        }
        .map_err(|_| 1_u8)?;
    }
    for id in delete_signed {
        batch.delete::<SignedKey>(id).map_err(|_| 1_u8)?;
    }
    for id in delete_wide {
        batch
            .delete::<WideKey>(ic_db::U256::from_be_bytes(id))
            .map_err(|_| 1_u8)?;
    }
    db.commit(batch).map_err(|_| 2_u8)?;
    if trap {
        crate::system::trap("after integer-key batch");
    }
    Ok(())
}

/// Page signed integer primary keys after the supplied position.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "signed_key_page",
    guard = crate::transport::admit::<74>,
    decode = crate::transport::decode::<_, 74>,
    encode = crate::transport::encode::<_, 74>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn signed_key_page((after, limit): (Option<i128>, u8)) -> (Vec<SignedInput>, Option<i128>, u64) {
    use schema::SignedKey;
    let host = runtime();
    let db = open(&host);
    let page = db.page::<SignedKey>(after, usize::from(limit)).unwrap();
    (
        page.rows
            .into_iter()
            .map(|r| (r.id, r.peer.map(ic_db::U256::to_be_bytes), r.rank))
            .collect(),
        page.next_after,
        db.revision::<SignedKey>().unwrap(),
    )
}

/// Page wide integer primary keys after the supplied position.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "wide_key_page",
    guard = crate::transport::admit::<75>,
    decode = crate::transport::decode::<_, 75>,
    encode = crate::transport::encode::<_, 75>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn wide_key_page(
    (after, limit): (Option<[u8; 32]>, u8),
) -> (Vec<WideInput>, Option<[u8; 32]>, u64) {
    use schema::WideKey;
    let host = runtime();
    let db = open(&host);
    let page = db
        .page::<WideKey>(after.map(ic_db::U256::from_be_bytes), usize::from(limit))
        .unwrap();
    (
        page.rows
            .into_iter()
            .map(|r| (r.id.to_be_bytes(), r.peer, r.rank))
            .collect(),
        page.next_after.map(ic_db::U256::to_be_bytes),
        db.revision::<WideKey>().unwrap(),
    )
}

/// Look up signed-key and wide-key rows by rank.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "integer_key_lookup",
    guard = crate::transport::admit::<76>,
    decode = crate::transport::decode::<_, 76>,
    encode = crate::transport::encode::<_, 76>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn integer_key_lookup(rank: u64) -> (Vec<SignedInput>, Vec<WideInput>) {
    use schema::{SignedKey, WideKey};
    let host = runtime();
    let db = open(&host);
    (
        db.index_prefix::<SignedKey>("rank", &[rank.into()], 32)
            .unwrap()
            .into_iter()
            .map(|r| (r.id, r.peer.map(ic_db::U256::to_be_bytes), r.rank))
            .collect(),
        db.index_prefix::<WideKey>("rank", &[rank.into()], 32)
            .unwrap()
            .into_iter()
            .map(|r| (r.id.to_be_bytes(), r.peer, r.rank))
            .collect(),
    )
}
