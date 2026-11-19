//! Maps preserve scalar key identity without overwriting corrupt stored entries.
use ic_db::{Batch, Database, Entity, Error, Schema, entity, map};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};
use std::collections::BTreeMap;

#[map(
    key(prim = "Int16"),
    value(opt, item(prim = "Text", max_bytes = 4)),
    max_items = 3
)]
struct Rates {}
#[map(
    name = "Rates",
    key(prim = "Int16"),
    value(opt, item(prim = "Text", max_bytes = 4)),
    max_items = 4
)]
struct WiderRates {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "values", value(item(is = "Rates")))
    )
)]
struct MapRow {}
#[entity(
    name = "MapRow",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "values", value(item(is = "WiderRates")))
    )
)]
struct WiderRow {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "values", value(opt, item(is = "Rates")))
    )
)]
struct OptionalRow {}
#[map(
    key(prim = "Nat16"),
    value(many, max_items = 256, item(prim = "Nat8")),
    max_items = 256
)]
struct Lists {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "values", value(item(is = "Lists")))
    )
)]
struct ListMapRow {}
#[map(key(prim = "Nat16"), value(item(is = "Self")), max_items = 256)]
struct Tree {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "tree", value(item(is = "Tree")))
    )
)]
struct TreeRow {}
#[map(
    key(prim = "Nat8"),
    value(item(prim = "Blob", max_bytes = 4096)),
    max_items = 2
)]
struct Blobs {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "values", value(item(is = "Blobs")))
    )
)]
struct BlobRow {}
#[map(key(prim = "Bool"), value(item(prim = "Nat8")), max_items = 0)]
struct Empty {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "values", value(item(is = "Empty")))
    )
)]
struct EmptyRow {}

fn sample() -> MapRow {
    MapRow {
        id: 7,
        values: Rates(BTreeMap::from([
            (256, Some("é".into())),
            (-1, None),
            (0, Some("".into())),
        ])),
    }
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

#[test]
fn canonical_map_bytes_distinguish_missing_absent_and_empty() {
    let row = sample();
    // Independent Node contract framing and SHA-256.
    assert_eq!(
        Schema::new(&[MapRow::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "b06aaaf0dfbba0ac0a9e422d6116ee0f89cf2f4a5097500b57b519cd1c187f05"
    );
    let golden = [
        7, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 255, 255, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 2, 0, 0, 0,
        195, 169,
    ];
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(MapRow::decode(&golden).unwrap(), row);
    let rebuilt = MapRow {
        id: 7,
        values: Rates(
            row.values
                .0
                .iter()
                .rev()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
        ),
    };
    assert_eq!(rebuilt.encode().unwrap(), golden);
    for end in 0..golden.len() {
        assert!(matches!(
            MapRow::decode(&golden[..end]),
            Err(Error::Corrupt)
        ));
    }
    let mut duplicate = golden;
    duplicate[15..17].copy_from_slice(&[255, 255]);
    let mut descending = golden;
    descending[12..14].copy_from_slice(&[0, 0]);
    descending[15..17].copy_from_slice(&[255, 255]);
    let mut huge = golden;
    huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut invalid = golden;
    invalid[30] = 255;
    let mut presence = golden;
    presence[14] = 2;
    for bad in [duplicate, descending, huge, invalid, presence] {
        assert!(matches!(MapRow::decode(&bad), Err(Error::Corrupt)));
    }
    let mut trailing = golden.to_vec();
    trailing.push(0);
    assert!(matches!(MapRow::decode(&trailing), Err(Error::Corrupt)));
    let mut over = sample();
    over.values.0.insert(1, None);
    assert!(matches!(over.encode(), Err(Error::Limit)));
    over.values.0.remove(&1);
    over.values.0.insert(0, Some("12345".into()));
    assert!(matches!(over.encode(), Err(Error::Limit)));
    let absent = OptionalRow {
        id: 7,
        values: None,
    };
    let empty = OptionalRow {
        id: 7,
        values: Some(Rates::default()),
    };
    let entry = OptionalRow {
        id: 7,
        values: Some(Rates(BTreeMap::from([(0, None)]))),
    };
    assert_ne!(absent.encode().unwrap(), empty.encode().unwrap());
    assert_ne!(empty.encode().unwrap(), entry.encode().unwrap());
    for row in [absent, empty, entry] {
        assert_eq!(OptionalRow::decode(&row.encode().unwrap()).unwrap(), row);
    }
    let mut zero = EmptyRow {
        id: 1,
        values: Empty::default(),
    };
    assert!(zero.encode().is_ok());
    zero.values.0.insert(false, 0);
    assert!(matches!(zero.encode(), Err(Error::Limit)));
}

#[test]
fn map_values_share_collection_depth_node_and_byte_bounds() {
    let mut row = ListMapRow {
        id: 1,
        values: Lists(BTreeMap::from([(0, vec![1; 255])])),
    };
    let bytes = row.encode().unwrap();
    assert_eq!(ListMapRow::decode(&bytes).unwrap(), row);
    row.values.0.get_mut(&0).unwrap().push(1);
    assert!(matches!(row.encode(), Err(Error::Limit)));
    let mut over = bytes;
    over[14..18].copy_from_slice(&256_u32.to_le_bytes());
    over.push(1);
    assert!(matches!(ListMapRow::decode(&over), Err(Error::Corrupt)));
    fn chain(n: usize) -> Tree {
        (0..n).fold(Tree::default(), |child, _| {
            Tree(BTreeMap::from([(0, child)]))
        })
    }
    for tree in [
        chain(31),
        Tree((0..255).map(|i| (i, Tree::default())).collect()),
    ] {
        let row = TreeRow { id: 1, tree };
        assert_eq!(TreeRow::decode(&row.encode().unwrap()).unwrap(), row);
    }
    for tree in [
        chain(32),
        Tree((0..256).map(|i| (i, Tree::default())).collect()),
    ] {
        assert!(matches!(
            TreeRow { id: 1, tree }.encode(),
            Err(Error::Limit)
        ));
    }
    let mut deep = 1_u64.to_le_bytes().to_vec();
    for _ in 0..32 {
        deep.extend([1, 0, 0, 0, 0, 0]);
    }
    deep.extend([0; 4]);
    assert!(matches!(TreeRow::decode(&deep), Err(Error::Corrupt)));
    assert!(Schema::new(&[TreeRow::SCHEMA]).is_ok());
    let mut blobs = BlobRow {
        id: 1,
        values: Blobs(BTreeMap::from([(0, vec![0; 2037]), (1, vec![255; 2037])])),
    };
    let bytes = blobs.encode().unwrap();
    assert_eq!(bytes.len(), 4096);
    assert_eq!(BlobRow::decode(&bytes).unwrap(), blobs);
    blobs.values.0.get_mut(&1).unwrap().push(1);
    assert!(matches!(blobs.encode(), Err(Error::Limit)));
    let mut over = bytes;
    over.push(0);
    assert!(matches!(BlobRow::decode(&over), Err(Error::Corrupt)));
}

#[test]
fn persisted_keys_cannot_be_overwritten_by_decode_or_repair_writes() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[MapRow::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), schema(), [5; 16]).unwrap();
    let row = sample();
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
    assert_eq!(db.get::<MapRow>(7).unwrap(), Some(row.clone()));
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
    row_key.extend(MapRow::SCHEMA.id.to_be_bytes());
    row_key.extend(7_u64.to_be_bytes());
    let mut duplicate = row.encode().unwrap();
    duplicate[15..17].copy_from_slice(&[255, 255]);
    let mut descending = row.encode().unwrap();
    descending[12..14].copy_from_slice(&[0, 0]);
    descending[15..17].copy_from_slice(&[255, 255]);
    let mut invalid = row.encode().unwrap();
    invalid[30] = 255;
    for bad in [duplicate, descending, invalid] {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
        raw.insert(row_key.clone(), bad);
        drop(raw);
        let mut db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
        let before = backing.borrow().clone();
        assert!(matches!(db.get::<MapRow>(7), Err(Error::Corrupt)));
        let mut b = Batch::new();
        b.delete::<MapRow>(7).unwrap();
        assert!(matches!(db.commit(b), Err(Error::Corrupt)));
        assert_eq!(*backing.borrow(), before);
        let mut b = Batch::new();
        b.replace(&row).unwrap();
        assert!(matches!(db.commit(b), Err(Error::Corrupt)));
        assert_eq!(*backing.borrow(), before);
    }
}
