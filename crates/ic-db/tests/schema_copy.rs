//! Independent target rows/indexes, resumable progress and bounded cancellation.
use ic_db::{
    Batch, Database, Entity, Error, Maintenance, Schema, SchemaCopy, SchemaTransition, entity,
};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

#[entity(
    name = "Thing",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat8")))
    )
)]
struct Source {}
#[entity(
    name = "Thing",
    pk(field = "id"),
    index(field = "value", unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat16")))
    )
)]
struct Target {}

#[entity(
    name = "Thing",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "created",
            managed = "created_at",
            value(item(prim = "Timestamp"))
        )
    )
)]
struct ManagedSource {}

#[entity(
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
struct Other {}

fn source() -> Schema {
    Schema::new(&[Source::SCHEMA]).unwrap()
}
fn target() -> Schema {
    Schema::new(&[Target::SCHEMA]).unwrap()
}
fn transition(version: u32) -> SchemaTransition {
    SchemaTransition::new(&source(), &target(), "widen", version, &[7]).unwrap()
}
fn key(name: &str) -> StableKey {
    StableKey::parse(name).unwrap()
}
fn host(memory: VectorMemory) -> MemoryRuntime<VectorMemory> {
    let mut host = MemoryRuntime::new(memory).unwrap();
    host.bootstrap(
        &SealedDeclarationSnapshot::new(
            ["copy.source.v1", "copy.target.v1", "copy.other.v1"]
                .iter()
                .map(|name| MemoryRequest::new("copy", name).unwrap())
                .collect(),
        )
        .unwrap(),
        &AllocationGrants::new(vec![MemoryAuthority::new("copy", "copy.").unwrap()]).unwrap(),
        &GenericAllocationPolicy,
    )
    .unwrap();
    host
}
fn create(host: &MemoryRuntime<VectorMemory>) -> Maintenance<VectorMemory> {
    let mut db = Database::create(
        host.open_memory(&key("copy.source.v1")).unwrap(),
        source(),
        [1; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for value in 1..=5 {
        batch
            .insert(&Source {
                id: u64::from(value),
                value,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    db.begin_maintenance(&transition(1)).unwrap()
}
fn begin(host: &MemoryRuntime<VectorMemory>) -> SchemaCopy<VectorMemory> {
    create(host)
        .begin_copy(
            host.open_memory(&key("copy.target.v1")).unwrap(),
            source(),
            target(),
            [2; 16],
        )
        .unwrap()
}
fn resume(host: &MemoryRuntime<VectorMemory>) -> SchemaCopy<VectorMemory> {
    SchemaCopy::resume(
        host.open_memory(&key("copy.source.v1")).unwrap(),
        host.open_memory(&key("copy.target.v1")).unwrap(),
        source(),
        target(),
        &transition(1),
    )
    .unwrap()
}
// Match the consuming migration boundary, which also permits moving owned fields.
#[expect(clippy::needless_pass_by_value)]
fn transform(row: Source, inputs: &[u8]) -> Result<Target, Error> {
    Ok(Target {
        id: row.id,
        value: u16::from(row.value) + u16::from(inputs[0]),
    })
}
fn entries(host: &MemoryRuntime<VectorMemory>, name: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
    let raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(host.open_memory(&key(name)).unwrap()).unwrap();
    raw.iter()
        .map(|entry| (entry.key().clone(), entry.value()))
        .collect()
}

#[test]
fn copy_rebuilds_indexes_and_resumes_from_its_own_checkpoint() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let mut copy = begin(&runtime);
    let step = copy.copy_next::<Source, Target>(4, 18, transform).unwrap();
    assert_eq!((step.rows, step.complete), (2, false));
    drop(copy);
    let original = entries(&runtime, "copy.source.v1");
    for name in ["copy.source.v1", "copy.target.v1"] {
        assert!(matches!(
            Database::open(runtime.open_memory(&key(name)).unwrap(), source()),
            Err(Error::Maintenance)
        ));
        assert!(
            Maintenance::resume(runtime.open_memory(&key(name)).unwrap(), &transition(1)).is_err()
        );
    }
    drop(runtime);
    let runtime = host(memory);
    let mut copy = resume(&runtime);
    let step = copy.copy_next::<Source, Target>(4, 128, transform).unwrap();
    assert_eq!((step.rows, step.complete), (3, true));
    assert_eq!(
        copy.copy_next::<Source, Target>(4, 128, transform)
            .unwrap()
            .rows,
        0
    );
    drop(copy);
    assert_eq!(entries(&runtime, "copy.source.v1"), original);
    let stored = entries(&runtime, "copy.target.v1");
    let mut expected = Vec::new();
    for id in 1..=5_u64 {
        let value = id as u16 + 7;
        let mut row_key = vec![2];
        row_key.extend(Target::ID.to_be_bytes());
        row_key.extend(id.to_be_bytes());
        let mut row = id.to_le_bytes().to_vec();
        row.extend(value.to_le_bytes());
        expected.push((row_key, row));
        let mut index = vec![3];
        index.extend(Target::ID.to_be_bytes());
        index.extend(Target::SCHEMA.indexes[0].id.to_be_bytes());
        index.extend(value.to_be_bytes());
        expected.push((index, id.to_be_bytes().to_vec()));
    }
    expected.sort();
    assert_eq!(
        stored
            .iter()
            .filter(|(key, _)| key[0] >= 2)
            .cloned()
            .collect::<Vec<_>>(),
        expected
    );
    let mut copy = resume(&runtime);
    assert!(!copy.cancel_step(1).unwrap());
    assert!(matches!(
        copy.copy_next::<Source, Target>(1, 9, transform),
        Err(Error::InvalidTransition)
    ));
    drop(copy);
    let mut copy = resume(&runtime);
    let mut calls = 0;
    while !copy.cancel_step(2).unwrap() {
        calls += 1;
        assert!(calls < 10);
    }
    drop(copy);
    assert_eq!(entries(&runtime, "copy.target.v1").len(), 1); // ownership tombstone only
    let db = Database::open(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source(),
    )
    .unwrap();
    assert_eq!(db.revision::<Source>().unwrap(), 1);
    for id in 1..=5 {
        assert_eq!(db.get::<Source>(id).unwrap().unwrap().value, id as u8);
    }
}

#[test]
fn refusals_leave_rows_and_checkpoint_unchanged_and_can_cancel() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let mut copy = begin(&runtime);
    let before = memory.borrow().clone();
    assert!(
        copy.copy_next::<Source, Target>(2, 18, |row, _| {
            if row.id == 2 {
                Err(Error::Limit)
            } else {
                Ok(Target {
                    id: row.id,
                    value: 1,
                })
            }
        })
        .is_err()
    );
    assert_eq!(*memory.borrow(), before);
    assert!(matches!(
        copy.copy_next::<Source, Target>(1, 9, |row, _| Ok(Target {
            id: row.id + 1,
            value: 1
        })),
        Err(Error::InvalidTransition)
    ));
    assert_eq!(*memory.borrow(), before);
    copy.copy_next::<Source, Target>(1, 9, |row, _| {
        Ok(Target {
            id: row.id,
            value: 1,
        })
    })
    .unwrap();
    let before = memory.borrow().clone();
    assert!(
        copy.copy_next::<Source, Target>(1, 9, |row, _| Ok(Target {
            id: row.id,
            value: 1
        }))
        .is_err()
    );
    assert_eq!(*memory.borrow(), before); // unique collision with an earlier chunk
    drop(copy);
    let mut copy = resume(&runtime);
    assert!(
        copy.copy_next::<Source, Target>(1, 9, |row, _| Ok(Target {
            id: row.id,
            value: 1
        }))
        .is_err()
    );
    while !copy.cancel_step(2).unwrap() {}
    drop(copy);
    Database::open(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source(),
    )
    .unwrap();
}

#[test]
fn ownership_refuses_wrong_pairs_versions_and_existing_target_allocations() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    drop(begin(&runtime));
    let before = memory.borrow().clone();
    for (left, right, version) in [
        ("copy.target.v1", "copy.source.v1", 1),
        ("copy.source.v1", "copy.target.v1", 2),
        ("copy.source.v1", "copy.other.v1", 1),
    ] {
        assert!(
            SchemaCopy::resume(
                runtime.open_memory(&key(left)).unwrap(),
                runtime.open_memory(&key(right)).unwrap(),
                source(),
                target(),
                &transition(version)
            )
            .is_err()
        );
        assert_eq!(*memory.borrow(), before);
    }
    let mut copy = resume(&runtime);
    assert!(copy.cancel_step(32).unwrap());
    drop(copy);
    let db = Database::open(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source(),
    )
    .unwrap();
    let maintenance = db.begin_maintenance(&transition(1)).unwrap();
    let before = memory.borrow().clone();
    assert!(
        maintenance
            .begin_copy(
                runtime.open_memory(&key("copy.target.v1")).unwrap(),
                source(),
                target(),
                [3; 16]
            )
            .is_err()
    );
    assert_eq!(*memory.borrow(), before); // even an empty cancelled target is not fresh
    Maintenance::resume(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        &transition(1),
    )
    .unwrap()
    .cancel();
}

#[test]
fn unsupported_shapes_refuse_before_target_initialization() {
    for (source_schema, target_schema) in [
        (
            Schema::new(&[Source::SCHEMA, Other::SCHEMA]).unwrap(),
            Schema::new(&[Target::SCHEMA, Other::SCHEMA]).unwrap(),
        ),
        (Schema::new(&[ManagedSource::SCHEMA]).unwrap(), target()),
    ] {
        let memory = VectorMemory::default();
        let runtime = host(memory.clone());
        let transition =
            SchemaTransition::new(&source_schema, &target_schema, "unsupported", 1, &[]).unwrap();
        let db = Database::create(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            source_schema.clone(),
            [1; 16],
        )
        .unwrap();
        let maintenance = db.begin_maintenance(&transition).unwrap();
        let before = memory.borrow().clone();
        assert!(matches!(
            maintenance.begin_copy(
                runtime.open_memory(&key("copy.target.v1")).unwrap(),
                source_schema,
                target_schema,
                [2; 16]
            ),
            Err(Error::InvalidTransition)
        ));
        assert_eq!(*memory.borrow(), before);
        Maintenance::resume(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            &transition,
        )
        .unwrap()
        .cancel();
    }
}

#[test]
fn empty_source_completes_and_cancels_within_one_entry() {
    let runtime = host(VectorMemory::default());
    let db = Database::create(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source(),
        [1; 16],
    )
    .unwrap();
    let mut copy = db
        .begin_maintenance(&transition(1))
        .unwrap()
        .begin_copy(
            runtime.open_memory(&key("copy.target.v1")).unwrap(),
            source(),
            target(),
            [2; 16],
        )
        .unwrap();
    let step = copy.copy_next::<Source, Target>(1, 9, transform).unwrap();
    assert_eq!((step.rows, step.complete), (0, true));
    assert!(matches!(copy.cancel_step(0), Err(Error::Limit)));
    assert!(copy.cancel_step(1).unwrap());
    drop(copy);
    Database::open(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source(),
    )
    .unwrap();
}

#[test]
fn malformed_and_future_copy_checkpoints_refuse_intact() {
    for case in 0..3 {
        let memory = VectorMemory::default();
        let runtime = host(memory.clone());
        drop(begin(&runtime));
        let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(
            runtime.open_memory(&key("copy.target.v1")).unwrap(),
        )
        .unwrap();
        let mut frame = raw.get(&vec![0]).unwrap();
        match case {
            0 => {
                frame.pop();
            }
            1 => frame.push(0),
            _ => {
                let phase = frame.len() - 5;
                frame[phase] = 255;
            }
        }
        raw.insert(vec![0], frame);
        drop(raw);
        let before = memory.borrow().clone();
        assert!(
            Database::open(
                runtime.open_memory(&key("copy.target.v1")).unwrap(),
                target()
            )
            .is_err()
        );
        assert!(
            SchemaCopy::resume(
                runtime.open_memory(&key("copy.source.v1")).unwrap(),
                runtime.open_memory(&key("copy.target.v1")).unwrap(),
                source(),
                target(),
                &transition(1)
            )
            .is_err()
        );
        assert_eq!(*memory.borrow(), before);
    }
}

fn finish_copy(copy: &mut SchemaCopy<VectorMemory>) {
    for _ in 0..8 {
        if copy
            .copy_next::<Source, Target>(2, 18, transform)
            .unwrap()
            .complete
        {
            return;
        }
    }
    panic!("copy must make progress");
}

#[test]
fn publication_requires_resumable_validation_and_keeps_source_retired() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let copy = begin(&runtime);
    let before = memory.borrow().clone();
    assert!(matches!(copy.publish(), Err(Error::InvalidTransition)));
    assert_eq!(*memory.borrow(), before);
    let mut copy = resume(&runtime);
    assert!(matches!(
        copy.validate_next(2),
        Err(Error::InvalidTransition)
    ));
    finish_copy(&mut copy);
    let before = memory.borrow().clone();
    assert!(matches!(copy.publish(), Err(Error::InvalidTransition)));
    assert_eq!(*memory.borrow(), before);
    let source_entries = entries(&runtime, "copy.source.v1");
    for call in 0..32 {
        let mut copy = resume(&runtime);
        let ready = copy.validate_next(2).unwrap();
        assert!(matches!(
            copy.copy_next::<Source, Target>(2, 18, transform),
            Err(Error::InvalidTransition)
        ));
        drop(copy);
        assert!(
            Database::open(
                runtime.open_memory(&key("copy.target.v1")).unwrap(),
                target()
            )
            .is_err()
        );
        if ready {
            break;
        }
        assert!(call < 31, "validation must progress");
    }
    let mut db = resume(&runtime).publish().unwrap();
    assert_eq!(db.incarnation(), [2; 16]);
    assert_eq!(db.get::<Target>(1).unwrap().unwrap().value, 8);
    assert_eq!(
        db.index_prefix::<Target>("value", &[8_u16.into()], 2)
            .unwrap()[0]
            .id,
        1
    );
    let mut batch = Batch::new();
    batch.replace(&Target { id: 1, value: 100 }).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    assert_eq!(
        &entries(&runtime, "copy.source.v1")[1..],
        &source_entries[1..]
    );
    let before = memory.borrow().clone();
    assert!(matches!(
        Database::open(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            source()
        ),
        Err(Error::Maintenance)
    ));
    assert!(
        Database::open(
            runtime.open_memory(&key("copy.target.v1")).unwrap(),
            source()
        )
        .is_err()
    );
    assert!(
        Maintenance::resume(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            &transition(1)
        )
        .is_err()
    );
    assert!(
        SchemaCopy::resume(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            runtime.open_memory(&key("copy.target.v1")).unwrap(),
            source(),
            target(),
            &transition(1)
        )
        .is_err()
    );
    assert_eq!(*memory.borrow(), before);
    let db = Database::open(
        runtime.open_memory(&key("copy.target.v1")).unwrap(),
        target(),
    )
    .unwrap();
    assert_eq!(db.get::<Target>(1).unwrap().unwrap().value, 100);
}

#[test]
fn incomplete_or_inconsistent_collections_never_validate_or_publish() {
    for case in 0..10 {
        let memory = VectorMemory::default();
        let runtime = host(memory.clone());
        let mut copy = begin(&runtime);
        finish_copy(&mut copy);
        drop(copy);
        let name = if case >= 8 {
            "copy.source.v1"
        } else {
            "copy.target.v1"
        };
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key(name)).unwrap())
                .unwrap();
        let row = raw.keys().find(|key| key[0] == 2).unwrap();
        let index = raw.keys().find(|key| key[0] == 3);
        match case {
            0 | 8 => {
                raw.remove(&row);
            }
            1 => {
                raw.remove(&index.unwrap());
            }
            2 => {
                raw.insert(index.unwrap(), 5_u64.to_be_bytes().to_vec());
            }
            3 => {
                let mut stale = index.unwrap();
                stale[9..].copy_from_slice(&999_u16.to_be_bytes());
                raw.insert(stale, 1_u64.to_be_bytes().to_vec());
            }
            4 => {
                raw.insert(row, vec![0]);
            }
            5 => {
                raw.insert(vec![4], vec![0]);
            }
            6 => {
                let revision = raw.keys().find(|key| key[0] == 1).unwrap();
                raw.remove(&revision);
            }
            7 => {
                let mut extra = row;
                extra[5..].copy_from_slice(&99_u64.to_be_bytes());
                let mut bytes = 99_u64.to_le_bytes().to_vec();
                bytes.extend(999_u16.to_le_bytes());
                raw.insert(extra, bytes);
            }
            _ => {
                let mut bytes = 99_u64.to_le_bytes().to_vec();
                bytes.push(1);
                raw.insert(row, bytes);
            }
        }
        drop(raw);
        let mut copy = resume(&runtime);
        for call in 0..32 {
            let before = memory.borrow().clone();
            match copy.validate_next(2) {
                Err(_) => {
                    assert_eq!(*memory.borrow(), before);
                    break;
                }
                Ok(ready) => {
                    assert!(!ready, "corruption case {case} validated");
                    assert!(call < 31);
                }
            }
        }
        let before = memory.borrow().clone();
        assert!(matches!(copy.publish(), Err(Error::InvalidTransition)));
        assert_eq!(*memory.borrow(), before);
        let mut copy = resume(&runtime);
        for call in 0..32 {
            if copy.cancel_step(2).unwrap() {
                break;
            }
            assert!(call < 31);
        }
    }
}
