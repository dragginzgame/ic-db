//! Full-width U256 bytes, ordering, uniqueness and corruption boundaries.
use ic_db::{Batch, Database, Entity, Error, IndexValue, Schema, U256, entity, map, set};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};
use std::collections::{BTreeMap, BTreeSet};
#[entity(
    pk(field="id"),
    index(fields=["zone","value","id"]),
    index(field="maybe",unique,present_only),
    fields(field(name="id",value(item(prim="Nat64"))),
    field(name="maybe",value(opt,item(prim="U256"))),
    field(name="value",value(item(prim="U256"))),
    field(name="zone",value(item(prim="Bool"))))
)]
struct BigNumbers {}
#[entity(
    name="BigNumbers",pk(field="id"),
    index(fields=["zone","value","id"]),
    index(field="maybe",unique,present_only),
    fields(field(name="id",value(item(prim="Nat64"))),
    field(name="maybe",value(opt,item(prim="Nat128"))),
    field(name="value",value(item(prim="Nat128"))),
    field(name="zone",value(item(prim="Bool"))))
)]
struct Narrower {}
#[set(item(prim = "U256"), max_items = 3)]
struct Values {}
#[map(key(prim = "U256"), value(item(prim = "U256")), max_items = 3)]
struct Pairs {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "list", value(many, max_items = 3, item(prim = "U256"))),
        field(name = "pairs", value(item(is = "Pairs"))),
        field(name = "set", value(item(is = "Values")))
    )
)]
struct Collections {}
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
fn values() -> [U256; 7] {
    [
        U256::ZERO,
        U256::ONE,
        U256::from(u128::MAX),
        U256::from_words(1, 0),
        U256::from_words(1 << 127, 0),
        U256::MAX,
        U256::from_words(
            0x0102030405060708090a0b0c0d0e0f10,
            0x1112131415161718191a1b1c1d1e1f20,
        ),
    ]
}
#[test]
fn exact_bytes_and_schema_do_not_narrow_large_integers() {
    // Independently framed schema bytes, hashed with Node SHA-256.
    assert_eq!(
        Schema::new(&[BigNumbers::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "f68531ad0f79c99125a0f3152b4199eaf82ffbe6df177006101ee563afc60788"
    );
    for value in values() {
        let row = BigNumbers {
            zone: true,
            id: 7,
            maybe: Some(value),
            value,
        };
        assert_eq!(BigNumbers::decode(&row.encode().unwrap()).unwrap(), row);
    }
    let row = BigNumbers {
        zone: true,
        id: 7,
        maybe: None,
        value: values()[6],
    };
    let mut golden = vec![7, 0, 0, 0, 0, 0, 0, 0, 0];
    golden.extend(1_u8..=32);
    golden.push(1);
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(BigNumbers::decode(&golden).unwrap(), row);
    for end in 0..golden.len() {
        assert!(matches!(
            BigNumbers::decode(&golden[..end]),
            Err(Error::Corrupt)
        ));
    }
    let mut bad = golden;
    bad.push(0);
    assert!(matches!(BigNumbers::decode(&bad), Err(Error::Corrupt)));
    let absent = BigNumbers {
        zone: true,
        id: 7,
        maybe: None,
        value: U256::ZERO,
    };
    let present = BigNumbers {
        zone: true,
        id: 7,
        maybe: Some(U256::ZERO),
        value: U256::ZERO,
    };
    assert_ne!(absent.encode().unwrap(), present.encode().unwrap());
    let mut bad = present.encode().unwrap();
    bad[8] = 2;
    assert!(matches!(BigNumbers::decode(&bad), Err(Error::Corrupt)));
    let maximum = "115792089237316195423570985008687907853269984665640564039457584007913129639935";
    assert_eq!(maximum.parse::<U256>().unwrap(), U256::MAX);
    assert_eq!(U256::MAX.to_string(), maximum);
    assert!(format!("{maximum}0").parse::<U256>().is_err());
    assert!("-1".parse::<U256>().is_err());
    assert_eq!(U256::MAX.checked_add(U256::ONE), None);
    assert_ne!(
        Schema::new(&[BigNumbers::SCHEMA]).unwrap().fingerprint(),
        Schema::new(&[Narrower::SCHEMA]).unwrap().fingerprint()
    );
}
#[test]
fn secondary_keys_order_the_full_domain_and_preserve_final_state_uniqueness() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[BigNumbers::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), schema(), [6; 16]).unwrap();
    let mut rows: Vec<_> = values()
        .into_iter()
        .enumerate()
        .map(|(n, value)| BigNumbers {
            zone: true,
            id: n as u64 + 1,
            maybe: Some(value),
            value,
        })
        .collect();
    rows.push(BigNumbers {
        zone: true,
        id: 8,
        maybe: None,
        value: U256::from_words(1, 0),
    });
    let mut b = Batch::new();
    for row in &rows {
        b.insert(row).unwrap();
    }
    db.commit(b).unwrap();
    let ordered = |db: &Database<VectorMemory>| {
        db.index_prefix::<BigNumbers>("zone,value,id", &[IndexValue::Bool(true)], 32)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ordered(&db), vec![1, 2, 3, 4, 8, 7, 5, 6]);
    assert_eq!(
        db.index_prefix::<BigNumbers>(
            "zone,value,id",
            &[
                IndexValue::Bool(true),
                IndexValue::from(U256::from_words(1, 0))
            ],
            32
        )
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect::<Vec<_>>(),
        vec![4, 8]
    );
    assert!(matches!(
        db.index_prefix::<BigNumbers>(
            "zone,value,id",
            &[IndexValue::Bool(true), IndexValue::Nat128(0)],
            32
        ),
        Err(Error::InvalidOperand)
    ));
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    db.commit(b).unwrap();
    assert_eq!(*backing.borrow(), before);
    let mut duplicate = rows[0].clone();
    duplicate.maybe = Some(U256::MAX);
    let mut b = Batch::new();
    b.replace(&duplicate).unwrap();
    assert!(db.commit(b).is_err());
    assert_eq!(*backing.borrow(), before);
    let first = rows[0].maybe;
    rows[0].maybe = rows[5].maybe;
    rows[5].maybe = first;
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    b.replace(&rows[5]).unwrap();
    db.commit(b).unwrap();
    assert_eq!(
        db.index_prefix::<BigNumbers>("maybe", &[IndexValue::from(U256::MAX)], 32)
            .unwrap()[0]
            .id,
        1
    );
    drop(db);
    let db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
    assert_eq!(ordered(&db), vec![1, 2, 3, 4, 8, 7, 5, 6]);
    for row in rows {
        assert_eq!(db.get::<BigNumbers>(row.id).unwrap(), Some(row));
    }
    drop(db);
    let before = backing.borrow().clone();
    assert!(matches!(
        Database::open(
            runtime.open_memory(&key).unwrap(),
            Schema::new(&[Narrower::SCHEMA]).unwrap()
        ),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
    // Inspect the physical ordered components independently of the typed reader.
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    let index = BigNumbers::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "zone,value,id")
        .unwrap();
    let mut physical = vec![3];
    physical.extend(BigNumbers::ID.to_be_bytes());
    physical.extend(index.id.to_be_bytes());
    physical.push(1);
    physical.extend([255; 32]);
    physical.extend(6_u64.to_be_bytes());
    physical.extend(6_u64.to_be_bytes());
    assert!(raw.get(&physical).is_some());
    // A noncanonical component length must not be accepted as a U256 index entry.
    let mut short = physical.clone();
    short.remove(10);
    raw.remove(&physical);
    raw.insert(short, 6_u64.to_be_bytes().to_vec());
    drop(raw);
    let db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
    assert!(matches!(
        db.index_prefix::<BigNumbers>("zone,value,id", &[IndexValue::Bool(true)], 32),
        Err(Error::Corrupt)
    ));
}
#[test]
fn collections_keep_high_bits_and_validate_unindexed_payloads() {
    let row = Collections {
        id: 1,
        list: vec![U256::MAX, U256::ZERO, U256::MAX],
        pairs: Pairs(BTreeMap::from([
            (U256::from_words(1, 0), U256::MAX),
            (U256::ONE, U256::ZERO),
        ])),
        set: Values(BTreeSet::from([
            U256::MAX,
            U256::ZERO,
            U256::from_words(1, 0),
        ])),
    };
    let bytes = row.encode().unwrap();
    assert_eq!(Collections::decode(&bytes).unwrap(), row);
    // List: 4-byte count + 3 exact 32-byte values; map starts at offset 108.
    assert_eq!(&bytes[12..44], &[255; 32]);
    assert_eq!(&bytes[44..76], &[0; 32]);
    let mut duplicate = bytes;
    let first = duplicate[112..144].to_vec();
    duplicate[176..208].copy_from_slice(&first);
    assert!(matches!(
        Collections::decode(&duplicate),
        Err(Error::Corrupt)
    ));
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[Collections::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), schema(), [6; 16]).unwrap();
    let mut b = Batch::new();
    b.insert(&row).unwrap();
    db.commit(b).unwrap();
    let mut b = Batch::new();
    b.replace(&row).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    let mut rk = vec![2];
    rk.extend(Collections::ID.to_be_bytes());
    rk.extend(1_u64.to_be_bytes());
    raw.insert(rk, duplicate);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.delete::<Collections>(1).unwrap();
    assert!(matches!(db.commit(b), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "values", value(many, max_items = 128, item(prim = "U256")))
    )
)]
struct WideList {}
#[test]
fn full_width_values_respect_the_row_byte_budget() {
    let mut row = WideList {
        id: 1,
        values: vec![U256::MAX; 127],
    };
    let bytes = row.encode().unwrap();
    assert_eq!(bytes.len(), 4076);
    assert_eq!(WideList::decode(&bytes).unwrap(), row);
    row.values.push(U256::ZERO);
    assert!(matches!(row.encode(), Err(Error::Limit)));
    let mut oversized = bytes;
    oversized[8..12].copy_from_slice(&128_u32.to_le_bytes());
    oversized.extend([0; 32]);
    assert!(matches!(WideList::decode(&oversized), Err(Error::Corrupt)));
}
