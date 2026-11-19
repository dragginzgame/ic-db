//! Finite domains, canonical zero, index order and stored corruption boundaries.
use ic_db::{
    Batch, Database, Entity, Error, Float32, Float64, IndexValue, Schema, entity, map, set,
};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
};

#[entity(
    pk(field="id"),
    index(fields=["zone","small","id"]),
    index(fields=["zone","value","id"]),
    index(field="maybe",unique,present_only),
    fields(
        field(name="id",value(item(prim="Nat64"))),
        field(name="maybe",value(opt,item(prim="Float64"))),
        field(name="small",value(item(prim="Float32"))),
        field(name="value",value(item(prim="Float64"))),
        field(name="zone",value(item(prim="Bool")))
    )
)]
struct FloatRow {}
#[set(item(prim = "Float32"), max_items = 3)]
struct Samples {}
#[map(
    key(prim = "Float64"),
    value(opt, item(prim = "Float32")),
    max_items = 3
)]
struct Measures {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "list", value(many, max_items = 3, item(prim = "Float64"))),
        field(name = "map", value(item(is = "Measures"))),
        field(name = "set", value(item(is = "Samples")))
    )
)]
struct Collections {}
fn f32v(value: f32) -> Float32 {
    Float32::try_new(value).unwrap()
}

fn f64v(value: f64) -> Float64 {
    Float64::try_new(value).unwrap()
}

fn values32() -> [f32; 9] {
    [
        -f32::MAX,
        -1.0,
        -f32::MIN_POSITIVE,
        -f32::from_bits(1),
        0.0,
        f32::from_bits(1),
        f32::MIN_POSITIVE,
        1.0,
        f32::MAX,
    ]
}

fn values64() -> [f64; 9] {
    [
        -f64::MAX,
        -1.0,
        -f64::MIN_POSITIVE,
        -f64::from_bits(1),
        0.0,
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        1.0,
        f64::MAX,
    ]
}

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

fn hash(value: impl Hash) -> u64 {
    let mut state = DefaultHasher::new();
    value.hash(&mut state);
    state.finish()
}

fn key(value: IndexValue<'_>) -> Vec<u8> {
    ic_db::__private::encode_index(&[(value, false)], false).unwrap()
}

#[test]
fn canonical_equality_hash_and_encoded_order_agree_over_the_finite_domain() {
    {
        let (a, b) = (f32v(-0.0), f32v(0.0));
        assert_eq!(a, b);
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
        assert_eq!(hash(a), hash(b));
        assert_eq!(a.get().to_bits(), 0);
        assert_eq!(key(a.into()), key(b.into()));
    }
    assert_eq!(HashSet::from([f64v(-0.0), f64v(0.0)]).len(), 1);
    assert_eq!(hash(f64v(-0.0)), hash(f64v(0.0)));
    assert_eq!(Float32::default(), f32v(0.0));
    assert_eq!(Float64::default(), f64v(0.0));
    for a in values32() {
        for b in values32() {
            assert_eq!(
                key(f32v(a).into()).cmp(&key(f32v(b).into())),
                a.partial_cmp(&b).unwrap()
            );
        }
    }
    for a in values64() {
        for b in values64() {
            assert_eq!(
                key(f64v(a).into()).cmp(&key(f64v(b).into())),
                a.partial_cmp(&b).unwrap()
            );
        }
    }
    // Literal IEEE sort keys, independently fixed at -1, zero and +1.
    assert_eq!(key(f32v(-1.0).into()), vec![0x40, 0x7f, 0xff, 0xff]);
    assert_eq!(key(f32v(0.0).into()), vec![0x80, 0, 0, 0]);
    assert_eq!(key(f32v(1.0).into()), vec![0xbf, 0x80, 0, 0]);
    assert_eq!(
        key(f64v(-1.0).into()),
        vec![0x40, 0x0f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
    );
    assert_eq!(key(f64v(0.0).into()), vec![0x80, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(key(f64v(1.0).into()), vec![0xbf, 0xf0, 0, 0, 0, 0, 0, 0]);
    for bits in [0x7f800000, 0xff800000, 0x7fc00000, 0x7f800001, 0xffc00001] {
        assert!(Float32::try_new(f32::from_bits(bits)).is_none());
    }
    for bits in [
        0x7ff0000000000000,
        0xfff0000000000000,
        0x7ff8000000000000,
        0x7ff0000000000001,
        0xfff8000000000001,
    ] {
        assert!(Float64::try_new(f64::from_bits(bits)).is_none());
    }
    // Deterministic raw-bit samples check the primitive comparator, not the key transform.
    let mut bits = 0x9382_58b7_02fa_7641_u64;
    for _ in 0..2048 {
        let old = bits;
        bits = bits.wrapping_mul(6364136223846793005).wrapping_add(1);
        if let (Some(a), Some(b)) = (
            Float64::try_new(f64::from_bits(old)),
            Float64::try_new(f64::from_bits(bits)),
        ) {
            assert_eq!(
                key(a.into()).cmp(&key(b.into())),
                a.get().partial_cmp(&b.get()).unwrap()
            );
        }
        if let (Some(a), Some(b)) = (
            Float32::try_new(f32::from_bits(old as u32)),
            Float32::try_new(f32::from_bits(bits as u32)),
        ) {
            assert_eq!(
                key(a.into()).cmp(&key(b.into())),
                a.get().partial_cmp(&b.get()).unwrap()
            );
        }
    }
}

#[test]
fn exact_rows_refuse_nonfinite_and_noncanonical_stored_values() {
    // Independent Node framing and SHA-256, not the product serializer.
    assert_eq!(
        Schema::new(&[FloatRow::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "cc54e10395bc01f03c4fac6ad075b400326d2b3ff81d427eacb3583c76c97a04"
    );
    let row = FloatRow {
        id: 7,
        maybe: None,
        small: f32v(1.5),
        value: f64v(-2.25),
        zone: true,
    };
    let golden = vec![
        7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0x3f, 0, 0, 0, 0, 0, 0, 0x02, 0xc0, 1,
    ];
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(FloatRow::decode(&golden).unwrap(), row);
    for (small, value) in values32().into_iter().zip(values64()) {
        let row = FloatRow {
            small: f32v(small),
            value: f64v(value),
            maybe: Some(f64v(value)),
            ..row.clone()
        };
        assert_eq!(FloatRow::decode(&row.encode().unwrap()).unwrap(), row);
    }
    for bits in [
        0x7f800000_u32,
        0xff800000,
        0x7fc00000,
        0x7f800001,
        0x80000000,
    ] {
        let mut bad = golden.clone();
        bad[9..13].copy_from_slice(&bits.to_le_bytes());
        assert!(matches!(FloatRow::decode(&bad), Err(Error::Corrupt)));
    }
    for bits in [
        0x7ff0000000000000_u64,
        0xfff0000000000000,
        0x7ff8000000000000,
        0x7ff0000000000001,
        0x8000000000000000,
    ] {
        let mut bad = golden.clone();
        bad[13..21].copy_from_slice(&bits.to_le_bytes());
        assert!(matches!(FloatRow::decode(&bad), Err(Error::Corrupt)));
    }
    for end in 0..golden.len() {
        assert!(matches!(
            FloatRow::decode(&golden[..end]),
            Err(Error::Corrupt)
        ));
    }
    let mut surplus = golden;
    surplus.push(0);
    assert!(matches!(FloatRow::decode(&surplus), Err(Error::Corrupt)));
    let a = FloatRow {
        small: f32v(-0.0),
        value: f64v(-0.0),
        maybe: Some(f64v(-0.0)),
        ..row
    };
    let b = FloatRow {
        small: f32v(0.0),
        value: f64v(0.0),
        maybe: Some(f64v(0.0)),
        ..a
    };
    assert_eq!(a.encode().unwrap(), b.encode().unwrap());
}

#[test]
fn indexes_preserve_full_domain_zero_uniqueness_and_final_state_swaps() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[FloatRow::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [7; 16]).unwrap();
    let mut rows: Vec<_> = values32()
        .into_iter()
        .zip(values64())
        .enumerate()
        .map(|(i, (small, value))| FloatRow {
            id: i as u64 + 1,
            maybe: Some(f64v(value)),
            small: f32v(small),
            value: f64v(value),
            zone: true,
        })
        .collect();
    let mut batch = Batch::new();
    for row in &rows {
        batch.insert(row).unwrap();
    }
    db.commit(batch).unwrap();
    for index in ["zone,small,id", "zone,value,id"] {
        assert_eq!(
            db.index_prefix::<FloatRow>(index, &[true.into()], 32)
                .unwrap()
                .into_iter()
                .map(|r| r.id)
                .collect::<Vec<_>>(),
            (1..=9).collect::<Vec<_>>()
        );
    }
    for wrong in [IndexValue::Nat64(0), f32v(0.0).into()] {
        assert!(matches!(
            db.index_prefix::<FloatRow>("zone,value,id", &[true.into(), wrong], 32),
            Err(Error::InvalidOperand)
        ));
    }
    assert!(matches!(
        db.index_prefix::<FloatRow>("zone,small,id", &[true.into(), f64v(0.0).into()], 32),
        Err(Error::InvalidOperand)
    ));
    let before = backing.borrow().clone();
    let zero = FloatRow {
        small: f32v(-0.0),
        value: f64v(-0.0),
        maybe: Some(f64v(-0.0)),
        ..rows[4].clone()
    };
    let mut batch = Batch::new();
    batch.replace(&zero).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(*backing.borrow(), before);
    let duplicate = FloatRow { id: 10, ..zero };
    let mut batch = Batch::new();
    batch.insert(&duplicate).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    rows[4].maybe = Some(f64v(f64::MAX));
    rows[8].maybe = Some(f64v(-0.0));
    let mut batch = Batch::new();
    batch.replace(&rows[4]).unwrap();
    batch.replace(&rows[8]).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(
        db.index_prefix::<FloatRow>("maybe", &[f64v(0.0).into()], 32)
            .unwrap()[0]
            .id,
        9
    );
    drop(db);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    for row in rows {
        assert_eq!(db.get::<FloatRow>(row.id).unwrap(), Some(row));
    }
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    let index = FloatRow::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "zone,value,id")
        .unwrap();
    let mut physical = vec![3];
    physical.extend(FloatRow::SCHEMA.id.to_be_bytes());
    physical.extend(index.id.to_be_bytes());
    physical.push(1);
    physical.extend([0xbf, 0xf0, 0, 0, 0, 0, 0, 0]);
    physical.extend(8_u64.to_be_bytes());
    physical.extend(8_u64.to_be_bytes());
    assert!(raw.get(&physical).is_some());
    raw.remove(&physical);
    physical[10..18].copy_from_slice(&[0xff, 0xf0, 0, 0, 0, 0, 0, 0]);
    raw.insert(physical, 8_u64.to_be_bytes().to_vec());
    drop(raw);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert!(matches!(
        db.index_prefix::<FloatRow>("zone,value,id", &[true.into()], 32),
        Err(Error::Corrupt)
    ));
}

#[test]
fn collection_keys_canonicalize_zero_and_inspection_checks_unindexed_payloads() {
    let row = Collections {
        id: 1,
        list: vec![f64v(-0.0), f64v(f64::from_bits(1)), f64v(f64::MAX)],
        map: Measures(BTreeMap::from([
            (f64v(0.0), None),
            (f64v(1.0), Some(f32v(1.0))),
        ])),
        set: Samples(BTreeSet::from([f32v(-0.0), f32v(0.0), f32v(1.0)])),
    };
    assert_eq!(row.set.0.len(), 2);
    assert_eq!(BTreeMap::from([(f64v(-0.0), 1), (f64v(0.0), 2)]).len(), 1);
    let bytes = row.encode().unwrap();
    assert_eq!(Collections::decode(&bytes).unwrap(), row);
    assert_eq!(&bytes[12..20], &[0; 8]);
    // Map follows three list values: count at 36, first key at 40; second at 49.
    let mut duplicate = bytes.clone();
    duplicate[49..57].fill(0);
    assert!(matches!(
        Collections::decode(&duplicate),
        Err(Error::Corrupt)
    ));
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[Collections::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [7; 16]).unwrap();
    let mut b = Batch::new();
    b.insert(&row).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    let mut rk = vec![2];
    rk.extend(Collections::SCHEMA.id.to_be_bytes());
    rk.extend(1_u64.to_be_bytes());
    let mut corrupt = bytes;
    corrupt[12..20].copy_from_slice(&0x7ff0000000000000_u64.to_le_bytes());
    raw.insert(rk, corrupt);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.delete::<Collections>(1).unwrap();
    assert!(matches!(db.commit(b), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}
