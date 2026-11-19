//! Frozen IcyDB time-policy regressions on the single typed commit path.
use ic_db::{Batch, Database, Entity, Error, Schema, Timestamp, entity};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

#[ic_db::newtype(item(prim = "Timestamp"))]
struct CreatedAt {}
#[ic_db::newtype(item(prim = "Timestamp"))]
struct UpdatedAt {}
type Changed = UpdatedAt;
#[ic_db::newtype(item(prim = "Int64"))]
struct Counter {}

#[entity(
    pk(field = "id"),
    index(field = "body", unique),
    index(field = "changed"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "body",
            value(item(prim = "Text", max_bytes = 32)),
            default = "ready"
        ),
        field(name = "born", value(item(is = "CreatedAt")), managed = "created_at"),
        field(
            name = "changed",
            value(item(is = "Changed", indirect)),
            managed = "updated_at"
        )
    )
)]
struct Entry {}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat64")), default = 0),
        field(name = "time", value(item(prim = "Timestamp")), managed = "created_at")
    )
)]
struct CreationOnly {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat64")), default = 0),
        field(name = "time", value(item(prim = "Timestamp")), managed = "updated_at")
    )
)]
struct UpdateOnly {}

#[test]
fn independent_roles_share_batch_time_and_deletes_need_no_clock() {
    let host = runtime(VectorMemory::default());
    let mut db = Database::create(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[CreationOnly::SCHEMA, UpdateOnly::SCHEMA]).unwrap(),
        [1; 16],
    )
    .unwrap();
    let mut batch = Batch::at(time(-1));
    let mut created = batch.insert_input(CreationOnly::insert().id(1)).unwrap();
    let mut updated = batch.insert_input(UpdateOnly::insert().id(1)).unwrap();
    assert_eq!(created.time, updated.time);
    db.commit(batch).unwrap();
    created.value = 1;
    updated.value = 1;
    let mut batch = Batch::at(time(10));
    batch.replace(&created).unwrap();
    batch.replace(&updated).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.get::<CreationOnly>(1).unwrap().unwrap().time, time(-1));
    assert_eq!(db.get::<UpdateOnly>(1).unwrap().unwrap().time, time(10));
    let mut batch = Batch::new();
    batch.delete::<CreationOnly>(1).unwrap();
    batch.delete::<UpdateOnly>(1).unwrap();
    db.commit(batch).unwrap();
    assert!(db.get::<CreationOnly>(1).unwrap().is_none());
    assert!(db.get::<UpdateOnly>(1).unwrap().is_none());
}

fn time(n: i64) -> Timestamp {
    Timestamp::from_millis(n)
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
fn one_batch_time_preserves_creation_and_updates_only_real_changes() {
    let backing = VectorMemory::default();
    let host = runtime(backing.clone());
    let mut db = Database::create(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Entry::SCHEMA]).unwrap(),
        [1; 16],
    )
    .unwrap();
    assert!(matches!(
        Batch::new().insert_input(Entry::insert().id(1)),
        Err(Error::MissingOperationTime)
    ));
    let mut batch = Batch::at(time(100));
    let first = batch.insert_input(Entry::insert().id(1)).unwrap();
    let second = batch
        .insert_input(Entry::insert().id(2).body("second".into()))
        .unwrap();
    assert_eq!(
        (
            first.born.0,
            first.changed.0,
            second.born.0,
            second.changed.0
        ),
        (time(100), time(100), time(100), time(100))
    );
    assert!(matches!(
        Batch::at(time(100)).insert(&first),
        Err(Error::ManagedField)
    ));
    db.commit(batch).unwrap();
    let mut updated = first;
    updated.body = "a longer replacement".into();
    let mut batch = Batch::at(time(200));
    batch.replace(&updated).unwrap();
    db.commit(batch).unwrap();
    let current = db.get::<Entry>(1).unwrap().unwrap();
    assert_eq!((current.born.0, current.changed.0), (time(100), time(200)));
    assert_eq!(
        db.index_prefix::<Entry>("changed", &[time(100).into()], 32)
            .unwrap(),
        vec![second]
    );
    assert_eq!(
        db.index_prefix::<Entry>("changed", &[time(200).into()], 32)
            .unwrap(),
        vec![current.clone()]
    );
    let before = backing.borrow().clone();
    for now in [0, 200, 300] {
        let mut batch = Batch::at(time(now));
        batch.replace(&current).unwrap();
        db.commit(batch).unwrap();
        assert_eq!(*backing.borrow(), before);
    }
    drop(db);
    let db = Database::open(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Entry::SCHEMA]).unwrap(),
    )
    .unwrap();
    assert_eq!(db.get::<Entry>(1).unwrap(), Some(current));
}

#[test]
fn ownership_regression_and_constraint_refusals_leave_the_complete_batch_unchanged() {
    let backing = VectorMemory::default();
    let host = runtime(backing.clone());
    let mut db = Database::create(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Entry::SCHEMA]).unwrap(),
        [1; 16],
    )
    .unwrap();
    let mut batch = Batch::at(time(100));
    let row = batch.insert_input(Entry::insert().id(1)).unwrap();
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    for mode in 0..4 {
        let mut candidate = row.clone();
        candidate.body = "changed".into();
        match mode {
            0 => candidate.born.0 = time(90),
            1 => candidate.changed.0 = time(101),
            _ => {}
        }
        let mut batch = Batch::at(time(if mode == 2 { 99 } else { 200 }));
        batch
            .insert_input(
                Entry::insert()
                    .id(2)
                    .body(if mode == 3 { "changed" } else { "other" }.into()),
            )
            .unwrap();
        batch.replace(&candidate).unwrap();
        let result = db.commit(batch);
        match mode {
            0 | 1 => assert!(matches!(result, Err(Error::ManagedField))),
            2 => assert!(matches!(result, Err(Error::TimestampRegression))),
            3 => assert!(matches!(result, Err(Error::UniqueViolation))),
            _ => unreachable!(),
        }
        assert_eq!(*backing.borrow(), before);
    }
    // Equal time is permitted for a real change; there is no hidden counter.
    let mut changed = row;
    changed.body = "equal time".into();
    let mut batch = Batch::at(time(100));
    batch.replace(&changed).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.get::<Entry>(1).unwrap(), Some(changed));
}

#[test]
fn corrupt_managed_order_refuses_reads_and_noop_replacement_without_repair() {
    let backing = VectorMemory::default();
    let host = runtime(backing.clone());
    let memory = || {
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap()
    };
    let schema = || Schema::new(&[Entry::SCHEMA]).unwrap();
    let mut db = Database::create(memory(), schema(), [1; 16]).unwrap();
    let mut batch = Batch::at(time(100));
    let row = batch.insert_input(Entry::insert().id(1)).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let mut corrupt = row.encode().unwrap();
    // Independent framing: u32 text length, five UTF-8 bytes, then born/changed i64.
    corrupt[9..17].copy_from_slice(&101_i64.to_le_bytes());
    assert!(matches!(Entry::decode(&corrupt), Err(Error::Corrupt)));
    let mut map = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(memory());
    let mut key = vec![2];
    key.extend(Entry::SCHEMA.id.to_be_bytes());
    key.extend(1_u64.to_be_bytes());
    map.insert(key, corrupt);
    drop(map);
    let before = backing.borrow().clone();
    let mut db = Database::open(memory(), schema()).unwrap();
    assert!(matches!(db.get::<Entry>(1), Err(Error::Corrupt)));
    let mut batch = Batch::at(time(200));
    batch.replace(&row).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn managed_policy_changes_identity_and_refuses_incompatible_reopen_intact() {
    let original = Schema::new(&[Entry::SCHEMA]).unwrap();
    let mut fields = Entry::SCHEMA.fields.to_vec();
    for field in &mut fields {
        field.managed = None;
    }
    let unowned = Box::leak(Box::new(ic_db::EntitySchema {
        fields: Box::leak(fields.into_boxed_slice()),
        ..*Entry::SCHEMA
    }));
    let different = Schema::new(&[unowned]).unwrap();
    assert_ne!(original.fingerprint(), different.fingerprint());
    let backing = VectorMemory::default();
    let host = runtime(backing.clone());
    drop(
        Database::create(
            host.open_memory(&StableKey::parse("test.db.v1").unwrap())
                .unwrap(),
            original,
            [1; 16],
        )
        .unwrap(),
    );
    let before = backing.borrow().clone();
    assert!(matches!(
        Database::open(
            host.open_memory(&StableKey::parse("test.db.v1").unwrap())
                .unwrap(),
            different
        ),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
    for mode in 0..5 {
        let mut fields = Entry::SCHEMA.fields.to_vec();
        let born = fields.iter_mut().find(|f| f.name == "born").unwrap();
        match mode {
            0 => born.optional = true,
            1 => born.value = ic_db::FieldType::Int64,
            2 => born.default = Some(&ic_db::LiteralDefault::Int(0)),
            3 => born.managed = Some(ic_db::ManagedTimestamp::UpdatedAt),
            4 => {
                use ic_db::Value;
                born.value = ic_db::FieldType::Named(ic_db::TypeLink {
                    name: Counter::NAME,
                    definition: Counter::definition,
                });
            }
            _ => unreachable!(),
        }
        let invalid = Box::leak(Box::new(ic_db::EntitySchema {
            fields: Box::leak(fields.into_boxed_slice()),
            ..*Entry::SCHEMA
        }));
        assert!(matches!(Schema::new(&[invalid]), Err(Error::InvalidSchema)));
    }
}
