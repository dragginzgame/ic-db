//! Supplied ULID keys retain typed identity through shared storage and references.
use ic_db::{Batch, Database, Entity, Error, Schema, Ulid, entity};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

#[entity(pk(field="id"),index(field="value",unique),index(fields=["owner","id"]),fields(
    field(name="id",value(item(prim="Ulid"))),
    field(name="owner",value(opt,item(prim="Nat64")),references(entity="Numbered")),
    field(name="value",value(item(prim="Nat64")))
))]
pub(crate) struct KeyRow {}
#[entity(pk(field="id"),index(fields=["target","id"]),fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="target",value(opt,item(prim="Ulid")),references(entity="KeyRow"))
))]
pub(crate) struct Numbered {}
#[entity(
    pk(field = "id"),
    index(field = "target"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "target",
            value(item(prim = "Nat64")),
            references(entity = "KeyRow")
        )
    )
)]
struct WrongReference {}
fn host(backing: VectorMemory) -> MemoryRuntime<VectorMemory> {
    let mut host = MemoryRuntime::new(backing).unwrap();
    let req =
        SealedDeclarationSnapshot::new(vec![MemoryRequest::new("test", "test.db.v1").unwrap()])
            .unwrap();
    let grants =
        AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap();
    host.bootstrap(&req, &grants, &GenericAllocationPolicy)
        .unwrap();
    host
}

fn schema() -> Schema {
    Schema::new(&[KeyRow::SCHEMA, Numbered::SCHEMA]).unwrap()
}

fn rows() -> Vec<KeyRow> {
    [
        0,
        u64::MAX as u128,
        1_u128 << 64,
        (1_u128 << 80) - 1,
        1_u128 << 80,
        u128::MAX,
    ]
    .into_iter()
    .enumerate()
    .map(|(n, id)| KeyRow {
        id: Ulid::from_u128(id),
        owner: None,
        value: n as u64 + 1,
    })
    .collect()
}

fn physical_row(id: Ulid) -> Vec<u8> {
    let mut b = vec![2];
    b.extend(KeyRow::ID.to_be_bytes());
    b.extend(id.to_bytes());
    b
}

#[test]
fn typed_keys_pages_unique_exchange_and_final_references() {
    assert!(matches!(
        Schema::new(&[WrongReference::SCHEMA, KeyRow::SCHEMA, Numbered::SCHEMA]),
        Err(Error::InvalidSchema)
    ));
    assert_eq!(
        schema().fingerprint().map(|b| format!("{b:02x}")).concat(),
        "dae66c10fc4a1b336ae176dd2fe86aa55dd94edf0f48575ea5c60a55051b19f2"
    );
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [12; 16]).unwrap();
    let mut rows = rows();
    rows[0].owner = Some(1);
    let mut batch = Batch::new();
    for row in rows.iter().rev() {
        batch.insert(row).unwrap();
    }
    batch
        .insert(&Numbered {
            id: 1,
            target: Some(Ulid::MIN),
        })
        .unwrap();
    batch
        .insert(&Numbered {
            id: 2,
            target: None,
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.get::<KeyRow>(Ulid::MIN).unwrap(), Some(rows[0].clone()));
    assert_eq!(db.get::<KeyRow>(Ulid::MAX).unwrap(), Some(rows[5].clone()));
    let first = db.page::<KeyRow>(None, 3).unwrap();
    assert_eq!(first.rows, rows[..3]);
    assert_eq!(first.next_after, Some(rows[2].id));
    let next = db.page::<KeyRow>(first.next_after, 3).unwrap();
    assert_eq!(next.rows, rows[3..]);
    assert_eq!(next.next_after, Some(Ulid::MAX));
    assert!(
        db.page::<KeyRow>(next.next_after, 3)
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(
        db.index_prefix::<KeyRow>("value", &[6_u64.into()], 1)
            .unwrap(),
        vec![rows[5].clone()]
    );
    assert_eq!(
        db.index_prefix::<KeyRow>("owner,id", &[ic_db::IndexValue::Absent], 32)
            .unwrap(),
        rows[1..]
    );
    assert_eq!(
        db.index_prefix::<Numbered>("target,id", &[Ulid::MIN.into()], 32)
            .unwrap()[0]
            .id,
        1
    );
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    db.commit(b).unwrap();
    assert_eq!(*backing.borrow(), before);
    let mut b = Batch::new();
    b.delete::<KeyRow>(Ulid::MIN).unwrap();
    assert!(matches!(db.commit(b), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut b = Batch::new();
    b.insert(&Numbered {
        id: 3,
        target: Some(Ulid::from_u128(7)),
    })
    .unwrap();
    assert!(matches!(db.commit(b), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut b = Batch::new();
    b.delete::<Numbered>(1).unwrap();
    assert!(matches!(db.commit(b), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut duplicate = rows[0].clone();
    duplicate.value = rows[5].value;
    let mut b = Batch::new();
    b.replace(&duplicate).unwrap();
    assert!(matches!(db.commit(b), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    b.delete::<KeyRow>(Ulid::MIN).unwrap();
    assert!(matches!(db.commit(b), Err(Error::DuplicateTarget)));
    assert_eq!(*backing.borrow(), before);
    rows[0].value = 6;
    rows[5].value = 1;
    let mut b = Batch::new();
    b.replace(&rows[5]).unwrap();
    b.replace(&rows[0]).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert_eq!(db.page::<KeyRow>(None, 32).unwrap().rows, rows);
    assert_eq!(
        db.index_prefix::<KeyRow>("value", &[1_u64.into()], 1)
            .unwrap()[0]
            .id,
        Ulid::MAX
    );
    // Delete/repoint against the complete final state, including the mixed-key cycle.
    let mut b = Batch::new();
    b.delete::<KeyRow>(Ulid::MIN).unwrap();
    b.delete::<Numbered>(1).unwrap();
    db.commit(b).unwrap();
    assert!(db.get::<KeyRow>(Ulid::MIN).unwrap().is_none());
    assert_eq!(db.revision::<KeyRow>().unwrap(), 3);
    // An absent reference does not point to the all-zero ULID.
    assert!(db.get::<Numbered>(2).unwrap().unwrap().target.is_none());
}

#[test]
fn physical_bytes_and_wrong_owner_refuse_without_mutation() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [12; 16]).unwrap();
    let row = rows().pop().unwrap();
    let mut b = Batch::new();
    b.insert(&row).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    let mut expected = vec![255; 16];
    expected.push(0);
    expected.extend(6_u64.to_le_bytes());
    assert_eq!(raw.get(&physical_row(Ulid::MAX)).unwrap(), expected);
    let unique = KeyRow::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "value")
        .unwrap();
    let mut index = vec![3];
    index.extend(KeyRow::ID.to_be_bytes());
    index.extend(unique.id.to_be_bytes());
    index.extend(6_u64.to_be_bytes());
    assert_eq!(raw.get(&index).unwrap(), vec![255; 16]);
    let nonunique = KeyRow::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "owner,id")
        .unwrap();
    let mut full = vec![3];
    full.extend(KeyRow::ID.to_be_bytes());
    full.extend(nonunique.id.to_be_bytes());
    full.push(0);
    full.extend([255; 16]);
    full.extend([255; 16]);
    assert_eq!(raw.get(&full).unwrap(), vec![255; 16]);
    drop(raw);
    for owner in [vec![255; 15], vec![255; 17], vec![0; 16]] {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
        raw.insert(index.clone(), owner);
        drop(raw);
        let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
        let before = backing.borrow().clone();
        assert!(matches!(
            db.index_prefix::<KeyRow>("value", &[6_u64.into()], 1),
            Err(Error::Corrupt)
        ));
        let mut b = Batch::new();
        b.delete::<KeyRow>(Ulid::MAX).unwrap();
        assert!(matches!(db.commit(b), Err(Error::Corrupt)));
        assert_eq!(*backing.borrow(), before);
        drop(db);
    }
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    raw.insert(index, vec![255; 16]);
    raw.remove(&physical_row(Ulid::MAX));
    drop(raw);
    let original = physical_row(Ulid::MAX);
    let mut trailing = original.clone();
    trailing.push(0);
    for bad in [original[..20].to_vec(), trailing, physical_row(Ulid::MIN)] {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
        raw.insert(bad.clone(), expected.clone());
        drop(raw);
        let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
        assert!(matches!(db.page::<KeyRow>(None, 32), Err(Error::Corrupt)));
        drop(db);
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
        raw.remove(&bad);
    }
}

#[entity(
    name = "Identity",
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
struct NumericIdentity {}
#[entity(
    name = "Identity",
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Ulid"))))
)]
struct UlidIdentity {}
#[test]
fn changing_primary_type_refuses_existing_data_intact() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let old = || Schema::new(&[NumericIdentity::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), old(), [12; 16]).unwrap();
    let mut b = Batch::new();
    b.insert(&NumericIdentity { id: u64::MAX }).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let before = backing.borrow().clone();
    assert!(matches!(
        Database::open(
            runtime.open_memory(&slot).unwrap(),
            Schema::new(&[UlidIdentity::SCHEMA]).unwrap()
        ),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), old()).unwrap();
    assert_eq!(
        db.get::<NumericIdentity>(u64::MAX).unwrap(),
        Some(NumericIdentity { id: u64::MAX })
    );
}
