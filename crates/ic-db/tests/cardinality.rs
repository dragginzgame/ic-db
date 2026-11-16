//! Maintained counts share the batch rollback and copy-publication boundaries.
use ic_db::{Batch, Database, Entity, Error, Schema, SchemaCopy, SchemaTransition, entity};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};
#[entity(
    name = "Row",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat8")))
    )
)]
struct Row {}
#[entity(
    name = "Row",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat16")))
    )
)]
struct Wide {}
#[entity(
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
struct Other {}
fn schema() -> Schema {
    Schema::new(&[Row::SCHEMA, Other::SCHEMA]).unwrap()
}
fn key(name: &str) -> StableKey {
    StableKey::parse(name).unwrap()
}
fn host(memory: VectorMemory) -> MemoryRuntime<VectorMemory> {
    let mut h = MemoryRuntime::new(memory).unwrap();
    h.bootstrap(
        &SealedDeclarationSnapshot::new(vec![
            MemoryRequest::new("test", "test.source.v1").unwrap(),
            MemoryRequest::new("test", "test.target.v1").unwrap(),
        ])
        .unwrap(),
        &AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap(),
        &GenericAllocationPolicy,
    )
    .unwrap();
    h
}
fn create(h: &MemoryRuntime<VectorMemory>) -> Database<VectorMemory> {
    Database::create(
        h.open_memory(&key("test.source.v1")).unwrap(),
        schema(),
        [1; 16],
    )
    .unwrap()
}
#[test]
fn counts_use_final_batch_deltas_and_preserve_noops() {
    let m = VectorMemory::default();
    let h = host(m.clone());
    let mut db = create(&h);
    assert_eq!(db.cardinality::<Row>().unwrap(), 0);
    let mut b = Batch::new();
    for id in 1..=3 {
        b.insert(&Row { id, value: 1 }).unwrap();
    }
    b.insert(&Other { id: 1 }).unwrap();
    db.commit(b).unwrap();
    assert_eq!(
        (
            db.cardinality::<Row>().unwrap(),
            db.cardinality::<Other>().unwrap()
        ),
        (3, 1)
    );
    let before = m.borrow().clone();
    let mut b = Batch::new();
    b.replace(&Row { id: 1, value: 1 }).unwrap();
    db.commit(b).unwrap();
    assert_eq!(*m.borrow(), before);
    let mut b = Batch::new();
    b.delete::<Row>(1).unwrap();
    b.insert(&Row { id: 4, value: 4 }).unwrap();
    b.replace(&Row { id: 2, value: 2 }).unwrap();
    db.commit(b).unwrap();
    assert_eq!(db.cardinality::<Row>().unwrap(), 3);
    let before = m.borrow().clone();
    let mut b = Batch::new();
    b.insert(&Row { id: 5, value: 1 }).unwrap();
    b.delete::<Row>(99).unwrap();
    assert!(matches!(db.commit(b), Err(Error::Missing)));
    assert_eq!(*m.borrow(), before);
    let mut b = Batch::new();
    b.delete::<Row>(2).unwrap();
    b.delete::<Row>(3).unwrap();
    b.delete::<Row>(4).unwrap();
    db.commit(b).unwrap();
    assert_eq!(db.cardinality::<Row>().unwrap(), 0);
    drop(db);
    let db = Database::open(h.open_memory(&key("test.source.v1")).unwrap(), schema()).unwrap();
    assert_eq!(db.cardinality::<Other>().unwrap(), 1);
}
#[test]
fn metadata_refusal_is_before_persistent_mutation() {
    for case in 0..4 {
        let m = VectorMemory::default();
        let h = host(m.clone());
        let mut db = create(&h);
        let mut b = Batch::new();
        b.insert(&Row { id: 1, value: 1 }).unwrap();
        db.commit(b).unwrap();
        drop(db);
        let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(
            h.open_memory(&key("test.source.v1")).unwrap(),
        )
        .unwrap();
        let mut k = vec![1];
        k.extend(Row::ID.to_be_bytes());
        match case {
            0 => {
                raw.remove(&k);
            }
            1 => {
                raw.insert(k, vec![0; 8]);
            }
            2 | 3 => {
                let mut v = 1_u64.to_le_bytes().to_vec();
                v.extend(if case == 2 { u64::MAX } else { 0 }.to_le_bytes());
                raw.insert(k, v);
            }
            _ => unreachable!(),
        }
        drop(raw);
        let before = m.borrow().clone();
        let mut db =
            Database::open(h.open_memory(&key("test.source.v1")).unwrap(), schema()).unwrap();
        let mut b = Batch::new();
        if case == 3 {
            b.delete::<Row>(1).unwrap();
        } else {
            b.insert(&Row { id: 2, value: 1 }).unwrap();
        }
        let result = db.commit(b);
        if case == 2 {
            assert!(matches!(result, Err(Error::Limit)));
        } else {
            assert!(matches!(result, Err(Error::Corrupt)));
        }
        assert_eq!(*m.borrow(), before);
        if case < 2 {
            assert!(matches!(db.cardinality::<Row>(), Err(Error::Corrupt)));
        }
    }
}
#[test]
fn old_format_refuses_intact_and_metadata_count_does_not_scan_rows() {
    let m = VectorMemory::default();
    let h = host(m.clone());
    let mut db = create(&h);
    let mut batch = Batch::new();
    batch.insert(&Row { id: 1, value: 1 }).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let mut raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(h.open_memory(&key("test.source.v1")).unwrap())
            .unwrap();
    let mut manifest = raw.get(&vec![0]).unwrap();
    assert_eq!(&manifest[..8], b"ICDBMF02");
    manifest[..8].copy_from_slice(b"ICDBMF01");
    raw.insert(vec![0], manifest.clone());
    drop(raw);
    let before = m.borrow().clone();
    assert!(matches!(
        Database::open(h.open_memory(&key("test.source.v1")).unwrap(), schema()),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*m.borrow(), before);
    // Restore the test's current-format manifest, then damage only its row bytes.
    let mut raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(h.open_memory(&key("test.source.v1")).unwrap())
            .unwrap();
    manifest[..8].copy_from_slice(b"ICDBMF02");
    raw.insert(vec![0], manifest);
    let mut row = vec![2];
    row.extend(Row::ID.to_be_bytes());
    row.extend(1_u64.to_be_bytes());
    raw.insert(row, vec![0]);
    drop(raw);
    let db = Database::open(h.open_memory(&key("test.source.v1")).unwrap(), schema()).unwrap();
    assert_eq!(db.cardinality::<Row>().unwrap(), 1);
    assert!(matches!(db.query::<Row>().count(), Err(Error::Corrupt)));
}
#[test]
fn copied_counts_resume_and_validate_before_publication() {
    copy_counts(false);
}
#[test]
fn mismatched_copy_counts_refuse_publication() {
    copy_counts(true);
}
fn copy_counts(corrupt_target: bool) {
    let m = VectorMemory::default();
    let h = host(m.clone());
    let mut db = create(&h);
    let mut b = Batch::new();
    for id in 1..=3 {
        b.insert(&Row { id, value: 1 }).unwrap();
    }
    db.commit(b).unwrap();
    let target = Schema::new(&[Wide::SCHEMA, Other::SCHEMA]).unwrap();
    let transition = SchemaTransition::new(&schema(), &target, "widen", 1, &[]).unwrap();
    let mut copy = db
        .begin_maintenance(&transition)
        .unwrap()
        .begin_copy(
            h.open_memory(&key("test.target.v1")).unwrap(),
            schema(),
            target.clone(),
            [2; 16],
        )
        .unwrap();
    assert_eq!(copy.next_entity(), Some("Other"));
    copy.copy_unchanged::<Other>(1, 128).unwrap();
    copy.copy_next::<Row, Wide>(1, 128, |r, _| {
        Ok(Wide {
            id: r.id,
            value: r.value.into(),
        })
    })
    .unwrap();
    drop(copy);
    let mut copy = SchemaCopy::resume(
        h.open_memory(&key("test.source.v1")).unwrap(),
        h.open_memory(&key("test.target.v1")).unwrap(),
        schema(),
        target.clone(),
        &transition,
    )
    .unwrap();
    while !copy
        .copy_next::<Row, Wide>(1, 128, |r, _| {
            Ok(Wide {
                id: r.id,
                value: r.value.into(),
            })
        })
        .unwrap()
        .complete
    {}
    if corrupt_target {
        drop(copy);
        let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(
            h.open_memory(&key("test.target.v1")).unwrap(),
        )
        .unwrap();
        let mut k = vec![1];
        k.extend(Wide::ID.to_be_bytes());
        let mut value = raw.get(&k).unwrap();
        value[8..].copy_from_slice(&2_u64.to_le_bytes());
        raw.insert(k, value);
        drop(raw);
        let before = m.borrow().clone();
        let mut copy = SchemaCopy::resume(
            h.open_memory(&key("test.source.v1")).unwrap(),
            h.open_memory(&key("test.target.v1")).unwrap(),
            schema(),
            target,
            &transition,
        )
        .unwrap();
        assert!(matches!(copy.validate_next(2), Err(Error::Corrupt)));
        assert_eq!(*m.borrow(), before);
        assert!(copy.publish().is_err());
        assert_eq!(*m.borrow(), before);
        return;
    }
    let mut ready = false;
    for _ in 0..32 {
        if copy.validate_next(2).unwrap() {
            ready = true;
            break;
        }
    }
    assert!(ready);
    let db = copy.publish().unwrap();
    assert_eq!(db.cardinality::<Wide>().unwrap(), 3);
    assert_eq!(db.cardinality::<Other>().unwrap(), 0);
    assert_eq!(db.query::<Wide>().count().unwrap().value, 3);
}
