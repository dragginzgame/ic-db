//! Unit-enum index identity, independent bytes and final-state integrity.
use ic_db::{Batch, Database, Entity, Error, IndexValue, Schema, Value, entity, enum_};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

#[enum_(
    variant(name = "Ready"),
    variant(name = "Archived"),
    variant(name = "Draft")
)]
struct State {}
#[enum_(
    name = "State",
    variant(name = "Draft"),
    variant(name = "Ready"),
    variant(name = "Archived")
)]
struct Reordered {}
#[enum_(
    name = "State",
    variant(name = "Archived"),
    variant(name = "Draft"),
    variant(name = "Stopped")
)]
struct Changed {}
#[enum_(
    variant(name = "Archived"),
    variant(name = "Draft"),
    variant(name = "Ready")
)]
struct Other {}
#[enum_(
    variant(name = "Archived"),
    variant(name = "Draft", value(item(prim = "Nat8")))
)]
struct Payload {}
#[entity(pk(field="id"),index(fields=["zone","state","id"]),index(field="maybe",unique,present_only),index(field="boxed"),fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="zone",value(item(prim="Bool"))),
    field(name="state",value(item(is="State"))),
    field(name="maybe",value(opt,item(is="State"))),
    field(name="boxed",value(opt,item(indirect,is="State")))
))]
struct Row {}

fn host(backing: VectorMemory) -> MemoryRuntime<VectorMemory> {
    let mut host = MemoryRuntime::new(backing).unwrap();
    let requests =
        SealedDeclarationSnapshot::new(vec![MemoryRequest::new("test", "test.db.v1").unwrap()])
            .unwrap();
    let grants =
        AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap();
    host.bootstrap(&requests, &grants, &GenericAllocationPolicy)
        .unwrap();
    host
}
fn row(id: u64, state: State) -> Row {
    Row {
        id,
        zone: true,
        state,
        maybe: Some(state),
        boxed: Some(Box::new(state)),
    }
}
fn schema() -> Schema {
    Schema::new(&[Row::SCHEMA]).unwrap()
}

#[test]
fn enum_keys_preserve_identity_order_uniqueness_and_reopen() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [13; 16]).unwrap();
    let mut rows = vec![
        row(3, State::Archived),
        row(2, State::Draft),
        row(1, State::Ready),
    ];
    let mut b = Batch::new();
    for r in rows.iter().rev() {
        b.insert(r).unwrap();
    }
    db.commit(b).unwrap();
    assert_eq!(
        db.index_prefix::<Row>("zone,state,id", &[true.into()], 32)
            .unwrap(),
        rows
    );
    // Same complete declared contract, independent Rust declaration order.
    assert_eq!(
        db.index_prefix::<Row>("maybe", &[(&Reordered::Draft).into()], 32)
            .unwrap(),
        [rows[1].clone()]
    );
    for value in [
        (&Other::Draft).into(),
        (&Changed::Draft).into(),
        1_u8.into(),
        IndexValue::Absent,
        IndexValue::UnitEnum {
            definition: State::definition(),
            ordinal: 3,
        },
        IndexValue::UnitEnum {
            definition: Payload::definition(),
            ordinal: 0,
        },
    ] {
        assert!(matches!(
            db.index_prefix::<Row>("maybe", &[value], 32),
            Err(Error::InvalidOperand)
        ));
    }
    let before = backing.borrow().clone();
    let mut duplicate = rows[0].clone();
    duplicate.maybe = rows[1].maybe;
    let mut b = Batch::new();
    b.replace(&duplicate).unwrap();
    assert!(matches!(db.commit(b), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    // Final-state uniqueness permits exchanges without a durable overlay.
    rows[0].maybe = Some(State::Ready);
    rows[2].maybe = Some(State::Archived);
    rows[1].boxed = None;
    let mut b = Batch::new();
    for r in &rows {
        b.replace(r).unwrap();
    }
    db.commit(b).unwrap();
    assert_eq!(
        db.index_prefix::<Row>("boxed", &[IndexValue::Absent], 32)
            .unwrap(),
        [rows[1].clone()]
    );
    assert_eq!(
        db.index_prefix::<Row>("boxed", &[(&State::Archived).into()], 32)
            .unwrap(),
        [rows[0].clone()]
    );
    drop(db);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert_eq!(
        db.index_prefix::<Row>("zone,state,id", &[true.into()], 32)
            .unwrap(),
        rows
    );
    drop(db);
    let raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    let index = Row::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "zone,state,id")
        .unwrap();
    for (ordinal, r) in rows.iter().enumerate() {
        let mut key = vec![3];
        key.extend(Row::ID.to_be_bytes());
        key.extend(index.id.to_be_bytes());
        key.extend([1, ordinal as u8]);
        key.extend(r.id.to_be_bytes());
        key.extend(r.id.to_be_bytes());
        assert_eq!(raw.get(&key), Some(r.id.to_be_bytes().to_vec()));
    }
}

#[test]
fn sparse_membership_and_corrupt_ordinals_refuse_without_mutation() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [14; 16]).unwrap();
    let mut r = row(1, State::Archived);
    r.maybe = None;
    r.boxed = None;
    let mut second = r.clone();
    second.id = 2;
    let mut b = Batch::new();
    b.insert(&r).unwrap();
    b.insert(&second).unwrap();
    db.commit(b).unwrap();
    assert!(
        db.index_prefix::<Row>("maybe", &[(&State::Archived).into()], 32)
            .unwrap()
            .is_empty()
    );
    // Alphabetical fields: boxed, id, maybe, state, zone.
    let golden = [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    assert_eq!(r.encode().unwrap(), golden);
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    let mut key = vec![2];
    key.extend(Row::ID.to_be_bytes());
    key.extend(1_u64.to_be_bytes());
    let mut bad = golden.to_vec();
    bad[10] = 3;
    raw.insert(key, bad);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(db.get::<Row>(1), Err(Error::Corrupt)));
    let mut b = Batch::new();
    b.delete::<Row>(1).unwrap();
    assert!(matches!(db.commit(b), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn complete_definition_controls_index_admission() {
    for definition in [Payload::definition(), Other::definition()] {
        let mut fields = Row::SCHEMA.fields.to_vec();
        let f = fields.iter_mut().find(|f| f.name == "state").unwrap();
        f.value = ic_db::FieldType::Named(if definition.name == "Payload" {
            ic_db::TypeLink {
                name: Payload::NAME,
                definition: Payload::definition,
            }
        } else {
            ic_db::TypeLink {
                name: Other::NAME,
                definition: Other::definition,
            }
        });
        let changed = Box::leak(Box::new(ic_db::EntitySchema {
            fields: Box::leak(fields.into_boxed_slice()),
            ..*Row::SCHEMA
        }));
        if definition.name == "Payload" {
            assert!(matches!(Schema::new(&[changed]), Err(Error::InvalidSchema)));
        } else {
            assert_ne!(
                Schema::new(&[changed]).unwrap().fingerprint(),
                schema().fingerprint()
            );
        }
    }
}

#[test]
fn malformed_or_wrong_owner_index_entries_refuse() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [15; 16]).unwrap();
    let r = row(1, State::Archived);
    let mut b = Batch::new();
    b.insert(&r).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let index = Row::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "zone,state,id")
        .unwrap();
    let mut prefix = vec![3];
    prefix.extend(Row::ID.to_be_bytes());
    prefix.extend(index.id.to_be_bytes());
    prefix.push(1);
    let mut original = prefix.clone();
    original.push(0);
    original.extend(1_u64.to_be_bytes());
    original.extend(1_u64.to_be_bytes());
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    assert!(raw.remove(&original).is_some());
    drop(raw);
    // Invalid ordinal, valid but incorrect ordinal, truncated component and wrong owner.
    for (component, owner) in [(vec![3], 1_u64), (vec![1], 1), (vec![], 1), (vec![0], 2)] {
        let mut bad = prefix.clone();
        bad.extend(component);
        bad.extend(1_u64.to_be_bytes());
        bad.extend(1_u64.to_be_bytes());
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
        raw.insert(bad.clone(), owner.to_be_bytes().to_vec());
        drop(raw);
        let before = backing.borrow().clone();
        let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
        assert!(matches!(
            db.index_prefix::<Row>("zone,state,id", &[true.into()], 32),
            Err(Error::Corrupt)
        ));
        assert_eq!(*backing.borrow(), before);
        drop(db);
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
        raw.remove(&bad);
    }
}
