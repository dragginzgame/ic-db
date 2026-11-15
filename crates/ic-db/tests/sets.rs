//! Canonical scalar sets: logical order, strict corruption refusal, shared limits.
use ic_db::{Batch, Database, Entity, Error, Schema, entity, set};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};
use std::collections::BTreeSet;
#[set(item(prim = "Int16"), max_items = 3)]
struct Scores {}
#[set(name = "Scores", item(prim = "Int16"), max_items = 4)]
struct WiderScores {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "scores", value(item(is = "Scores")))
    )
)]
struct SetRow {}
#[entity(
    name = "SetRow",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "scores", value(item(is = "WiderScores")))
    )
)]
struct WiderRow {}
#[set(item(prim = "Nat16"), max_items = 256)]
struct Large {}
#[set(item(prim = "Blob", max_bytes = 4096), max_items = 2)]
struct Blobs {}
#[set(item(prim = "Bool"), max_items = 0)]
struct Empty {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "a", value(item(is = "Large"))),
        field(name = "b", value(many, max_items = 256, item(prim = "Nat16"))),
        field(name = "empty", value(opt, item(is = "Empty")))
    )
)]
struct Mixed {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "items", value(item(is = "Blobs")))
    )
)]
struct BlobRow {}
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
#[test]
fn order_and_count_are_validated_before_materialization() {
    let row = SetRow {
        id: 7,
        scores: Scores(BTreeSet::from([256, -1, 0])),
    };
    // Independent Node contract framing and SHA-256, not the product serializer.
    assert_eq!(
        Schema::new(&[SetRow::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "486936ccde04e7d35ebeae22524df9ce34319ff3a6d3a14b6403aff817632ba4"
    );
    let golden = [7, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 255, 255, 0, 0, 0, 1];
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(SetRow::decode(&golden).unwrap(), row);
    let mut reverse = row.clone();
    reverse.scores.0 = [0, -1, 256].into_iter().collect();
    assert_eq!(reverse.encode().unwrap(), golden);
    for end in 0..golden.len() {
        assert!(matches!(
            SetRow::decode(&golden[..end]),
            Err(Error::Corrupt)
        ));
    }
    for suffix in [[255, 255, 255, 255, 0, 1], [0, 0, 255, 255, 0, 1]] {
        let mut bad = golden;
        bad[12..].copy_from_slice(&suffix);
        assert!(matches!(SetRow::decode(&bad), Err(Error::Corrupt)));
    }
    let mut bad = golden.to_vec();
    bad.push(0);
    assert!(matches!(SetRow::decode(&bad), Err(Error::Corrupt)));
    let mut bad = golden;
    bad[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(SetRow::decode(&bad), Err(Error::Corrupt)));
    let mut over = row;
    over.scores.0.insert(1);
    assert!(matches!(over.encode(), Err(Error::Limit)));
}
#[test]
fn set_and_list_counts_share_one_row_budget() {
    let mut row = Mixed {
        id: 1,
        a: Large((0..128).collect()),
        b: (0..128).collect(),
        empty: None,
    };
    let bytes = row.encode().unwrap();
    assert_eq!(Mixed::decode(&bytes).unwrap(), row);
    row.b.push(128);
    assert!(matches!(row.encode(), Err(Error::Limit)));
    row.b.pop();
    row.empty = Some(Empty::default());
    let present = row.encode().unwrap();
    assert_ne!(present, bytes);
    assert_eq!(Mixed::decode(&present).unwrap(), row);
    row.empty.as_mut().unwrap().0.insert(false);
    assert!(matches!(row.encode(), Err(Error::Limit)));
    let mut blobs = BlobRow {
        id: 1,
        items: Blobs(BTreeSet::from([vec![0; 2038], vec![255; 2038]])),
    };
    let bytes = blobs.encode().unwrap();
    assert_eq!(bytes.len(), 4096);
    assert_eq!(BlobRow::decode(&bytes).unwrap(), blobs);
    blobs.items.0.remove(&vec![255; 2038]);
    blobs.items.0.insert(vec![255; 2039]);
    assert!(matches!(blobs.encode(), Err(Error::Limit)));
    let mut bad = bytes;
    bad.push(0);
    assert!(matches!(BlobRow::decode(&bad), Err(Error::Corrupt)));
}
#[test]
fn stored_duplicates_refuse_mutation_and_schema_changes_refuse_reopen() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[SetRow::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), schema(), [4; 16]).unwrap();
    let row = SetRow {
        id: 7,
        scores: Scores(BTreeSet::from([-1, 0, 256])),
    };
    let mut b = Batch::new();
    b.insert(&row).unwrap();
    db.commit(b).unwrap();
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.replace(&row).unwrap();
    db.commit(b).unwrap();
    assert_eq!(*backing.borrow(), before);
    drop(db);
    let db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
    assert_eq!(db.get::<SetRow>(7).unwrap(), Some(row.clone()));
    drop(db);
    assert!(matches!(
        Database::open(
            runtime.open_memory(&key).unwrap(),
            Schema::new(&[WiderRow::SCHEMA]).unwrap()
        ),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut row_key = vec![2];
    row_key.extend(SetRow::ID.to_be_bytes());
    row_key.extend(7_u64.to_be_bytes());
    for suffix in [[255, 255, 255, 255, 0, 1], [0, 0, 255, 255, 0, 1]] {
        let mut corrupt = row.encode().unwrap();
        corrupt[12..].copy_from_slice(&suffix);
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap())
                .unwrap();
        raw.insert(row_key.clone(), corrupt);
        drop(raw);
        let mut db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
        let before = backing.borrow().clone();
        assert!(matches!(db.get::<SetRow>(7), Err(Error::Corrupt)));
        let mut b = Batch::new();
        b.delete::<SetRow>(7).unwrap();
        assert!(matches!(db.commit(b), Err(Error::Corrupt)));
        assert_eq!(*backing.borrow(), before);
        let mut b = Batch::new();
        b.replace(&row).unwrap();
        assert!(matches!(db.commit(b), Err(Error::Corrupt)));
        assert_eq!(*backing.borrow(), before);
    }
}
#[set(item(prim = "Nat8"), max_items = 2)]
struct Nat8Set {}
#[set(item(prim = "Nat16"), max_items = 2)]
struct Nat16Set {}
#[set(item(prim = "Nat32"), max_items = 2)]
struct Nat32Set {}
#[set(item(prim = "Nat64"), max_items = 2)]
struct Nat64Set {}
#[set(item(prim = "Nat128"), max_items = 2)]
struct Nat128Set {}
#[set(item(prim = "Int8"), max_items = 2)]
struct Int8Set {}
#[set(item(prim = "Int16"), max_items = 2)]
struct Int16Set {}
#[set(item(prim = "Int32"), max_items = 2)]
struct Int32Set {}
#[set(item(prim = "Int64"), max_items = 2)]
struct Int64Set {}
#[set(item(prim = "Int128"), max_items = 2)]
struct Int128Set {}
#[set(item(prim = "Bool"), max_items = 2)]
struct BoolSet {}
#[set(item(prim = "Principal"), max_items = 2)]
struct PrincipalSet {}
#[set(item(prim = "Text", max_bytes = 4), max_items = 2)]
struct TextSet {}
#[set(item(prim = "Blob", max_bytes = 4), max_items = 2)]
struct BlobSet {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "snat8", value(item(is = "Nat8Set"))),
        field(name = "snat16", value(item(is = "Nat16Set"))),
        field(name = "snat32", value(item(is = "Nat32Set"))),
        field(name = "snat64", value(item(is = "Nat64Set"))),
        field(name = "snat128", value(item(is = "Nat128Set"))),
        field(name = "sint8", value(item(is = "Int8Set"))),
        field(name = "sint16", value(item(is = "Int16Set"))),
        field(name = "sint32", value(item(is = "Int32Set"))),
        field(name = "sint64", value(item(is = "Int64Set"))),
        field(name = "sint128", value(item(is = "Int128Set"))),
        field(name = "sbool", value(item(is = "BoolSet"))),
        field(name = "sprincipal", value(item(is = "PrincipalSet"))),
        field(name = "stext", value(item(is = "TextSet"))),
        field(name = "sblob", value(item(is = "BlobSet")))
    )
)]
struct AllSets {}
#[test]
fn scalar_set_order_agrees_with_borrowed_stored_inspection() {
    let row = AllSets {
        id: 1,
        snat8: Nat8Set(BTreeSet::from([0, u8::MAX])),
        snat16: Nat16Set(BTreeSet::from([255, 256])),
        snat32: Nat32Set(BTreeSet::from([0, u32::MAX])),
        snat64: Nat64Set(BTreeSet::from([0, u64::MAX])),
        snat128: Nat128Set(BTreeSet::from([0, u128::MAX])),
        sint8: Int8Set(BTreeSet::from([i8::MIN, i8::MAX])),
        sint16: Int16Set(BTreeSet::from([-1, 0])),
        sint32: Int32Set(BTreeSet::from([i32::MIN, i32::MAX])),
        sint64: Int64Set(BTreeSet::from([i64::MIN, i64::MAX])),
        sint128: Int128Set(BTreeSet::from([i128::MIN, i128::MAX])),
        sbool: BoolSet(BTreeSet::from([false, true])),
        sprincipal: PrincipalSet(BTreeSet::from([
            ic_db::Principal::from_slice(&[255]),
            ic_db::Principal::from_slice(&[0, 0]),
        ])),
        stext: TextSet(BTreeSet::from(["".to_string(), "é".to_string()])),
        sblob: BlobSet(BTreeSet::from([vec![0, 0], vec![255]])),
    };
    let bytes = row.encode().unwrap();
    assert_eq!(AllSets::decode(&bytes).unwrap(), row);
    // Independent frames: blobs use bytes-first order; principals use length-first.
    assert_eq!(
        &bytes[8..23],
        &[2, 0, 0, 0, 2, 0, 0, 0, 0, 0, 1, 0, 0, 0, 255]
    );
    assert!(bytes.ends_with(&[
        2, 0, 0, 0, 1, 0, 0, 0, 255, 2, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 195,
        169,
    ]));
    let backing = VectorMemory::default();
    let runtime = host(backing);
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(
        runtime.open_memory(&key).unwrap(),
        Schema::new(&[AllSets::SCHEMA]).unwrap(),
        [8; 16],
    )
    .unwrap();
    let mut b = Batch::new();
    b.insert(&row).unwrap();
    db.commit(b).unwrap();
    let mut b = Batch::new();
    b.delete::<AllSets>(1).unwrap();
    db.commit(b).unwrap();
    assert_eq!(db.get::<AllSets>(1).unwrap(), None);
}
