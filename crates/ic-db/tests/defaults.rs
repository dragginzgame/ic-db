//! Omission/default semantics on the real typed and persistent write path.

use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error, FieldSchema, FieldType, InsertInput, LiteralDefault};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory,
};

///
/// Entry
/// Exercises indexed access by `label` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(field = "label", unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "count", value(item(prim = "Nat16")), default = 7),
        field(name = "enabled", value(item(prim = "Bool")), default = true),
        field(name = "label", value(opt, item(prim = "Text", max_bytes = 8)), default = "draft"),
        field(name = "note", value(opt, item(prim = "Text", max_bytes = 8))),
        field(name = "signed", value(item(prim = "Int8")), default = -128)
    )
)]
struct Entry {}

///
/// Policy
/// Stores `value` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Policy",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat16")), default = 7)
    )
)]
struct Policy {}

///
/// Reordered
/// Exercises an alternate `Policy` declaration under the same logical identity.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Policy",
    pk(field = "id"),
    fields(
        field(name = "value", value(item(prim = "Nat16")), default = 0x7),
        field(name = "id", value(item(prim = "Nat64")))
    )
)]
struct Reordered {}

///
/// Changed
/// Exercises an alternate `Policy` declaration under the same logical identity.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Policy",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat16")), default = 8)
    )
)]
struct Changed {}

///
/// Absent
/// Exercises an alternate `Policy` declaration under the same logical identity.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Policy",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat16")))
    )
)]
struct Absent {}

///
/// Extremes
/// Exercises stored representations across 10 fields sharing the primary key `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "u8", value(item(prim = "Nat8")), default = 255),
        field(name = "u16", value(item(prim = "Nat16")), default = 65535),
        field(name = "u32", value(item(prim = "Nat32")), default = 4294967295),
        field(name = "u64", value(item(prim = "Nat64")), default = 18446744073709551615),
        field(name = "u128", value(item(prim = "Nat128")), default = 340282366920938463463374607431768211455),
        field(name = "i8", value(item(prim = "Int8")), default = -128),
        field(name = "i16", value(item(prim = "Int16")), default = -32768),
        field(name = "i32", value(item(prim = "Int32")), default = -2147483648),
        field(name = "i64", value(item(prim = "Int64")), default = -9223372036854775808),
        field(name = "i128", value(item(prim = "Int128")), default = -170141183460469231731687303715884105728)
    )
)]
struct Extremes {}

#[test]
fn blob_defaults_preserve_omission_absence_uniqueness_and_reopen() {
    ///
    /// Binary
    /// Exercises indexed access by `payload` on rows keyed by `id`.
    ///

    #[entity(
        store = ic_db::testing::FixtureStore,
        pk(field = "id"),
        index(field = "payload", unique, present_only),
        fields(
            field(name = "id", value(item(prim = "Nat64"))),
            field(
                name = "empty",
                value(item(prim = "Blob", max_bytes = 0)),
                default = b""
            ),
            field(
                name = "payload",
                value(opt, item(prim = "Blob", max_bytes = 2)),
                default = b"\x00\xff"
            )
        )
    )]
    struct Binary {}
    let backing = VectorMemory::default();
    let host = runtime(backing.clone());
    let schema = || Schema::new(&[Binary::SCHEMA]).unwrap();
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(host.open_memory(&key).unwrap(), schema(), [3; 16]).unwrap();
    let mut batch = Batch::new();
    let defaulted = batch.insert_input(Binary::insert().id(1)).unwrap();
    assert!(defaulted.empty.is_empty());
    assert_eq!(defaulted.payload, Some(vec![0, 255]));
    let absent = batch
        .insert_input(Binary::insert().id(2).payload(None))
        .unwrap();
    let empty = batch
        .insert_input(Binary::insert().id(3).payload(Some(vec![])))
        .unwrap();
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.insert_input(Binary::insert().id(4)).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut replacement = defaulted;
    replacement.payload = None;
    let mut batch = Batch::new();
    batch.replace(&replacement).unwrap();
    db.commit(batch).unwrap();
    drop((db, host));
    let host = runtime(backing.clone());
    let db = Database::open(host.open_memory(&key).unwrap(), schema()).unwrap();
    assert_eq!(
        db.page::<Binary>(None, 32).unwrap().rows,
        [replacement, absent, empty]
    );
    let before = backing.borrow().clone();
    let mut fields = Binary::SCHEMA.fields.to_vec();
    fields
        .iter_mut()
        .find(|f| f.name == "payload")
        .unwrap()
        .default = Some(&LiteralDefault::Blob(b"\xff\x00"));
    let changed = Box::leak(Box::new(ic_db::EntitySchema {
        fields: Box::leak(fields.into_boxed_slice()),
        ..*Binary::SCHEMA
    }));
    let changed = Schema::new(&[changed]).unwrap();
    assert_ne!(changed.fingerprint(), schema().fingerprint());
    drop(db);
    assert!(matches!(
        Database::open(host.open_memory(&key).unwrap(), changed),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn literal_defaults_preserve_every_integer_width_without_host_narrowing() {
    Schema::new(&[Extremes::SCHEMA]).unwrap();
    let row = Extremes::insert().id(1).into_entity().unwrap();
    assert_eq!(
        (row.u8, row.u16, row.u32, row.u64, row.u128),
        (u8::MAX, u16::MAX, u32::MAX, u64::MAX, u128::MAX)
    );
    assert_eq!(
        (row.i8, row.i16, row.i32, row.i64, row.i128),
        (i8::MIN, i16::MIN, i32::MIN, i64::MIN, i128::MIN)
    );
    assert_eq!(Extremes::decode(&row.encode().unwrap()).unwrap(), row);
}

fn runtime(backing: VectorMemory) -> MemoryRuntime<VectorMemory> {
    let mut runtime = MemoryRuntime::new(backing).unwrap();
    runtime
        .bootstrap(
            &SealedDeclarationSnapshot::new(vec![
                MemoryRequest::new("test", "test.db.v1").unwrap(),
            ])
            .unwrap(),
            &AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap(),
            &GenericAllocationPolicy,
        )
        .unwrap();

    runtime
}

#[test]
fn omission_and_explicit_values_share_existing_constraints_and_replacements() {
    let backing = VectorMemory::default();
    let host = runtime(backing.clone());
    let mut db = Database::create(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Entry::SCHEMA]).unwrap(),
        [1; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    assert!(matches!(
        batch.insert_input(Entry::insert()),
        Err(Error::MissingField("id"))
    ));
    assert!(matches!(
        batch.insert_input(Entry::insert().id(99).label(Some("too long!".into()))),
        Err(Error::Limit)
    ));
    let defaulted = batch.insert_input(Entry::insert().id(1)).unwrap();
    assert_eq!(
        (
            defaulted.count,
            defaulted.enabled,
            defaulted.label.as_deref(),
            defaulted.note.as_deref(),
            defaulted.signed
        ),
        (7, true, Some("draft"), None, -128)
    );
    let explicit = batch
        .insert_input(
            Entry::insert()
                .id(2)
                .count(0)
                .enabled(false)
                .label(None)
                .note(Some(String::new()))
                .signed(0),
        )
        .unwrap();
    assert_eq!(
        (
            explicit.count,
            explicit.enabled,
            explicit.label.as_deref(),
            explicit.note.as_deref(),
            explicit.signed
        ),
        (0, false, None, Some(""), 0)
    );
    db.commit(batch).unwrap();
    assert_eq!(
        db.page::<Entry>(None, 32).unwrap().rows,
        vec![defaulted.clone(), explicit.clone()]
    );
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.insert_input(Entry::insert().id(3)).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut replacement = defaulted;
    replacement.count = 0;
    replacement.enabled = false;
    replacement.label = None;
    replacement.signed = 0;
    let mut batch = Batch::new();
    batch.replace(&replacement).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.get::<Entry>(1).unwrap(), Some(replacement.clone()));
    let revision = db.revision::<Entry>().unwrap();
    let mut batch = Batch::new();
    batch.replace(&replacement).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.revision::<Entry>().unwrap(), revision);
    assert_eq!(
        Entry::decode(&explicit.encode().unwrap()).unwrap(),
        explicit
    );
    drop(db);
    drop(host);
    let host = runtime(backing);
    let db = Database::open(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Entry::SCHEMA]).unwrap(),
    )
    .unwrap();
    assert_eq!(db.get::<Entry>(1).unwrap(), Some(replacement));
}

#[test]
fn default_identity_is_canonical_and_changed_policy_refuses_intact() {
    let fingerprint = Schema::new(&[Policy::SCHEMA]).unwrap().fingerprint();
    assert_eq!(
        Schema::new(&[Reordered::SCHEMA]).unwrap().fingerprint(),
        fingerprint
    );
    assert_ne!(
        Schema::new(&[Changed::SCHEMA]).unwrap().fingerprint(),
        fingerprint
    );
    assert_ne!(
        Schema::new(&[Absent::SCHEMA]).unwrap().fingerprint(),
        fingerprint
    );
    let backing = VectorMemory::default();
    let host = runtime(backing.clone());
    let mut db = Database::create(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Policy::SCHEMA]).unwrap(),
        [2; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    batch.insert_input(Policy::insert().id(1)).unwrap();
    db.commit(batch).unwrap();
    let mut batch = Batch::new();
    batch.insert_input(Changed::insert().id(2)).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::UnknownEntity)));
    drop(db);
    let before = backing.borrow().clone();
    assert!(matches!(
        Database::open(
            host.open_memory(&StableKey::parse("test.db.v1").unwrap())
                .unwrap(),
            Schema::new(&[Changed::SCHEMA]).unwrap()
        ),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
    let db = Database::open(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Reordered::SCHEMA]).unwrap(),
    )
    .unwrap();
    assert_eq!(db.get::<Reordered>(1).unwrap().unwrap().value, 7);
}

#[test]
fn hand_authored_metadata_cannot_bypass_default_type_and_range_admission() {
    for default in [
        &LiteralDefault::Nat(65536),
        &LiteralDefault::Int(7),
        &LiteralDefault::Bool(true),
        &LiteralDefault::Text("7"),
    ] {
        let fields = Box::leak(Box::new([
            FieldSchema {
                name: "id",
                value: FieldType::Nat64,
                optional: false,
                reference: None,
                default: None,
                managed: None,
            },
            FieldSchema {
                name: "value",
                value: FieldType::Nat16,
                optional: false,
                reference: None,
                default: Some(default),
                managed: None,
            },
        ]));
        let schema = Box::leak(Box::new(ic_db::EntitySchema {
            fields,
            ..*Policy::SCHEMA
        }));
        assert!(matches!(Schema::new(&[schema]), Err(Error::InvalidSchema)));
    }
}
