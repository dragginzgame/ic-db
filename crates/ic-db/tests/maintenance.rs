//! Durable source freezing, exact identity admission and pre-copy cancellation.
use ic_db::{
    Batch, Database, Entity, Error, Maintenance, QueryCursor, Schema, SchemaTransition, entity,
};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

#[entity(
    name = "Thing",
    pk(field = "id"),
    index(field = "value"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat8")))
    )
)]
struct Source {}
#[entity(
    name = "Thing",
    pk(field = "id"),
    index(field = "value"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat16")))
    )
)]
struct Target {}

fn source() -> Schema {
    Schema::new(&[Source::SCHEMA]).unwrap()
}
fn target() -> Schema {
    Schema::new(&[Target::SCHEMA]).unwrap()
}
fn identity(name: &str, version: u32, inputs: &[u8]) -> SchemaTransition {
    SchemaTransition::new(&source(), &target(), name, version, inputs).unwrap()
}
fn key() -> StableKey {
    StableKey::parse("test.db.v1").unwrap()
}
fn host(memory: VectorMemory) -> MemoryRuntime<VectorMemory> {
    let mut runtime = MemoryRuntime::new(memory).unwrap();
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
fn create(runtime: &MemoryRuntime<VectorMemory>) -> Database<VectorMemory> {
    let mut db = Database::create(runtime.open_memory(&key()).unwrap(), source(), [7; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&Source { id: 1, value: 42 }).unwrap();
    db.commit(batch).unwrap();
    db
}
fn entries(runtime: &MemoryRuntime<VectorMemory>) -> Vec<(Vec<u8>, Vec<u8>)> {
    let raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key()).unwrap()).unwrap();
    raw.iter()
        .map(|entry| (entry.key().clone(), entry.value()))
        .collect()
}

#[test]
fn frozen_source_pages_resume_without_skipping_byte_retries_or_mutating_storage() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let mut db = create(&runtime);
    let mut batch = Batch::new();
    for id in 2..=5 {
        batch.insert(&Source { id, value: 42 }).unwrap();
    }
    db.commit(batch).unwrap();
    let ordinary = db.query::<Source>().limit(1).fetch().unwrap().next.unwrap();
    let transition = identity("widen-value", 1, &[2]);
    let maintenance = db.begin_maintenance(&transition).unwrap();
    let frozen = memory.borrow().clone();
    assert!(matches!(
        maintenance.source_page::<Source>(&source(), Some(&ordinary), 2, 18),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        maintenance.source_page::<Target>(&target(), None, 2, 18),
        Err(Error::IncompatibleSchema)
    ));
    assert!(
        maintenance
            .source_page::<Target>(&source(), None, 2, 18)
            .is_err()
    );
    for (rows, bytes) in [(0, 18), (33, 18), (1, 0), (1, 128 * 1024 + 1)] {
        assert!(matches!(
            maintenance.source_page::<Source>(&source(), None, rows, bytes),
            Err(Error::Limit)
        ));
    }
    assert!(matches!(
        maintenance.source_page::<Source>(&source(), None, 2, 8),
        Err(Error::RowTooLarge)
    ));
    // Each encoded row is exactly nine bytes; row three is examined but retried.
    let first = maintenance
        .source_page::<Source>(&source(), None, 4, 18)
        .unwrap();
    assert_eq!(
        first.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(
        (first.examined, first.fetched, first.output_bytes),
        (3, 3, 18)
    );
    let cursor = QueryCursor::from_bytes(&first.next.unwrap().to_bytes()).unwrap();
    drop(maintenance);
    let maintenance =
        Maintenance::resume(runtime.open_memory(&key()).unwrap(), &transition).unwrap();
    let second = maintenance
        .source_page::<Source>(&source(), Some(&cursor), 3, 27)
        .unwrap();
    assert_eq!(
        second.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        [3, 4, 5]
    );
    assert!(second.rows.iter().all(|row| row.value == 42));
    let end = maintenance
        .source_page::<Source>(&source(), second.next.as_ref(), 1, 9)
        .unwrap();
    assert!(end.rows.is_empty() && end.next.is_none());
    assert_eq!(end.examined, 0);
    assert_eq!(*memory.borrow(), frozen);
    maintenance.cancel();
    let db = Database::open(runtime.open_memory(&key()).unwrap(), source()).unwrap();
    assert!(matches!(
        db.query::<Source>().after(&cursor).fetch(),
        Err(Error::InvalidCursor)
    ));
}

#[test]
fn source_cursors_bind_transition_database_and_content_revision() {
    let runtime = host(VectorMemory::default());
    let transition = identity("widen-value", 1, &[2]);
    let maintenance = create(&runtime).begin_maintenance(&transition).unwrap();
    let cursor = maintenance
        .source_page::<Source>(&source(), None, 1, 9)
        .unwrap()
        .next
        .unwrap();
    maintenance.cancel();
    for changed in [
        identity("other", 1, &[2]),
        identity("widen-value", 2, &[2]),
        identity("widen-value", 1, &[3]),
    ] {
        let db = Database::open(runtime.open_memory(&key()).unwrap(), source()).unwrap();
        let maintenance = db.begin_maintenance(&changed).unwrap();
        assert!(matches!(
            maintenance.source_page::<Source>(&source(), Some(&cursor), 1, 9),
            Err(Error::InvalidCursor)
        ));
        maintenance.cancel();
    }
    let mut db = Database::open(runtime.open_memory(&key()).unwrap(), source()).unwrap();
    let mut batch = Batch::new();
    batch.replace(&Source { id: 1, value: 42 }).unwrap();
    db.commit(batch).unwrap(); // A no-op leaves input unchanged.
    let maintenance = db.begin_maintenance(&transition).unwrap();
    assert!(
        maintenance
            .source_page::<Source>(&source(), Some(&cursor), 1, 9)
            .unwrap()
            .next
            .is_none()
    );
    maintenance.cancel();
    let mut db = Database::open(runtime.open_memory(&key()).unwrap(), source()).unwrap();
    for value in [43, 42] {
        let mut batch = Batch::new();
        batch.replace(&Source { id: 1, value }).unwrap();
        db.commit(batch).unwrap();
    }
    let maintenance = db.begin_maintenance(&transition).unwrap();
    assert!(matches!(
        maintenance.source_page::<Source>(&source(), Some(&cursor), 1, 9),
        Err(Error::InvalidCursor)
    ));
    let other = host(VectorMemory::default());
    let mut db = Database::create(other.open_memory(&key()).unwrap(), source(), [8; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&Source { id: 1, value: 42 }).unwrap();
    db.commit(batch).unwrap();
    let other = db.begin_maintenance(&transition).unwrap();
    assert!(matches!(
        other.source_page::<Source>(&source(), Some(&cursor), 1, 9),
        Err(Error::InvalidCursor)
    ));
}

#[test]
fn corrupt_source_rows_and_revisions_refuse_without_repair() {
    for (tag, payload) in [
        (1, vec![0; 7]),
        (2, vec![0; 8]),
        (2, [2_u64.to_le_bytes().as_slice(), &[42]].concat()),
    ] {
        let memory = VectorMemory::default();
        let runtime = host(memory.clone());
        let transition = identity("widen-value", 1, &[]);
        drop(create(&runtime).begin_maintenance(&transition).unwrap());
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key()).unwrap())
                .unwrap();
        let mut stored_key = vec![tag];
        stored_key.extend(Source::ID.to_be_bytes());
        if tag == 2 {
            stored_key.extend(1_u64.to_be_bytes());
        }
        raw.insert(stored_key, payload);
        drop(raw);
        let before = memory.borrow().clone();
        let maintenance =
            Maintenance::resume(runtime.open_memory(&key()).unwrap(), &transition).unwrap();
        assert!(
            maintenance
                .source_page::<Source>(&source(), None, 2, 18)
                .is_err()
        );
        assert_eq!(*memory.borrow(), before);
    }
}

#[test]
fn freeze_closes_access_and_cancellation_restores_source_rows_indexes_and_revision() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let db = create(&runtime);
    let revision = db.revision::<Source>().unwrap();
    drop(db);
    let original = entries(&runtime);
    let transition = identity("widen-value", 1, &[2, 0]);
    let db = Database::open(runtime.open_memory(&key()).unwrap(), source()).unwrap();
    let maintenance = db.begin_maintenance(&transition).unwrap();
    assert_eq!(maintenance.transition(), &transition);
    assert_eq!(maintenance.incarnation(), [7; 16]);
    assert!(runtime.open_memory(&key()).is_err()); // exclusive supplier custody
    drop(maintenance);
    let frozen = entries(&runtime);
    assert_eq!(frozen[1..], original[1..]); // independent complete row/index/revision oracle
    let mut expected = b"ICDBMT01".to_vec();
    expected.extend(source().fingerprint());
    expected.extend(target().fingerprint());
    expected.extend([7; 16]);
    expected.push(0);
    expected.extend(1_u32.to_le_bytes());
    expected.extend(11_u32.to_le_bytes());
    expected.extend(b"widen-value");
    expected.extend(2_u32.to_le_bytes());
    expected.extend([2, 0]);
    assert_eq!(frozen[0], (vec![0], expected));
    let before = memory.borrow().clone();
    for schema in [source(), target()] {
        assert!(matches!(
            Database::open(runtime.open_memory(&key()).unwrap(), schema),
            Err(Error::Maintenance)
        ));
    }
    for changed in [
        identity("different", 1, &[2, 0]),
        identity("widen-value", 2, &[2, 0]),
        identity("widen-value", 1, &[0, 2]),
        SchemaTransition::new(&target(), &source(), "widen-value", 1, &[2, 0]).unwrap(),
    ] {
        assert!(matches!(
            Maintenance::resume(runtime.open_memory(&key()).unwrap(), &changed),
            Err(Error::InvalidTransition)
        ));
    }
    assert_eq!(*memory.borrow(), before);
    drop(runtime);
    let runtime = host(memory);
    Maintenance::resume(runtime.open_memory(&key()).unwrap(), &transition)
        .unwrap()
        .cancel();
    assert_eq!(entries(&runtime), original);
    assert!(matches!(
        Database::open(runtime.open_memory(&key()).unwrap(), target()),
        Err(Error::IncompatibleSchema)
    ));
    let mut db = Database::open(runtime.open_memory(&key()).unwrap(), source()).unwrap();
    assert_eq!(db.revision::<Source>().unwrap(), revision);
    assert_eq!(db.get::<Source>(1).unwrap().unwrap().value, 42);
    let mut batch = Batch::new();
    batch.replace(&Source { id: 1, value: 43 }).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.revision::<Source>().unwrap(), revision + 1);
}

#[test]
fn transition_bounds_and_wrong_source_refuse_before_mutation() {
    for (name, version, inputs) in [("", 1, vec![]), ("x", 0, vec![]), ("x", 1, vec![0; 257])] {
        assert!(matches!(
            SchemaTransition::new(&source(), &target(), name, version, &inputs),
            Err(Error::InvalidTransition)
        ));
    }
    assert!(SchemaTransition::new(&source(), &target(), &"é".repeat(33), 1, &[]).is_err());
    assert!(SchemaTransition::new(&source(), &source(), "x", 1, &[]).is_err());
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let db = create(&runtime);
    let before = memory.borrow().clone();
    let wrong = SchemaTransition::new(&target(), &source(), "x", 1, &[]).unwrap();
    assert!(matches!(
        db.begin_maintenance(&wrong),
        Err(Error::InvalidTransition)
    ));
    assert!(matches!(
        Maintenance::resume(runtime.open_memory(&key()).unwrap(), &identity("x", 1, &[])),
        Err(Error::InvalidTransition)
    ));
    assert_eq!(*memory.borrow(), before);
    let transition = identity(&"é".repeat(32), u32::MAX, &[255; 256]);
    let db = Database::open(runtime.open_memory(&key()).unwrap(), source()).unwrap();
    drop(db.begin_maintenance(&transition).unwrap());
    assert_eq!(entries(&runtime)[0].1.len(), 421);
    let resumed = Maintenance::resume(runtime.open_memory(&key()).unwrap(), &transition).unwrap();
    assert_eq!(resumed.transition().name(), "é".repeat(32));
    assert_eq!(resumed.transition().version(), u32::MAX);
    assert_eq!(resumed.transition().inputs(), &[255; 256]);
    assert_eq!(resumed.transition().source(), source().fingerprint());
    assert_eq!(resumed.transition().target(), target().fingerprint());
    resumed.cancel();
}

#[test]
fn malformed_or_future_maintenance_records_never_reopen_or_cancel() {
    let runtime = host(VectorMemory::default());
    let transition = identity("widen-value", 1, &[]);
    drop(create(&runtime).begin_maintenance(&transition).unwrap());
    let valid = entries(&runtime)[0].1.clone();
    let mut cases = Vec::new();
    for end in 8..valid.len() {
        cases.push(valid[..end].to_vec());
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    cases.push(trailing);
    for (offset, value) in [(88, 1), (93, 255), (97, 255)] {
        let mut bytes = valid.clone();
        bytes[offset] = value;
        cases.push(bytes);
    }
    for range in [72..88, 89..93] {
        let mut bytes = valid.clone();
        bytes[range].fill(0);
        cases.push(bytes);
    }
    let mut same = valid;
    same.copy_within(8..40, 40);
    cases.push(same);
    for bytes in cases {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key()).unwrap())
                .unwrap();
        raw.insert(vec![0], bytes);
        drop(raw);
        let before = entries(&runtime);
        assert!(Database::open(runtime.open_memory(&key()).unwrap(), source()).is_err());
        assert!(Maintenance::resume(runtime.open_memory(&key()).unwrap(), &transition).is_err());
        assert_eq!(entries(&runtime), before);
    }
}
