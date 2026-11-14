//! Bounded ordered lists: independent count framing and shared row limits.
use ic_db::{Batch, Database, Entity, Error, Schema, entity, enum_, record};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "labels",
            value(opt, many, max_items = 3, item(prim = "Text", max_bytes = 4))
        ),
        field(name = "numbers", value(many, max_items = 3, item(prim = "Nat16")))
    )
)]
struct ListRow {}
#[entity(
    name = "ListRow",
    pk(field = "id"),
    fields(
        field(name = "numbers", value(many, max_items = 4, item(prim = "Nat16"))),
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "labels",
            value(opt, many, max_items = 3, item(prim = "Text", max_bytes = 4))
        )
    )
)]
struct WiderListRow {}
#[record(fields(field(name = "children", value(many, max_items = 4, item(is = "Self")))))]
struct Tree {}
#[enum_(
    variant(name = "Empty"),
    variant(name = "Forest", value(many, max_items = 4, item(is = "Tree")))
)]
struct Forest {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "trees", value(item(is = "Forest")))
    )
)]
struct ForestRow {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "a", value(many, max_items = 256, item(prim = "Nat8"))),
        field(name = "b", value(many, max_items = 256, item(prim = "Nat8"))),
        field(name = "id", value(item(prim = "Nat64")))
    )
)]
struct Siblings {}
#[record(fields())]
struct Empty {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "items", value(many, max_items = 256, item(is = "Empty")))
    )
)]
struct EmptyItems {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "items", value(many, max_items = 0, item(prim = "Bool")))
    )
)]
struct EmptyOnly {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "items",
            value(many, max_items = 2, item(prim = "Blob", max_bytes = 4096))
        )
    )
)]
struct BlobList {}
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
fn list_order_duplicates_absence_and_element_bounds_are_exact() {
    let row = ListRow {
        id: 7,
        labels: None,
        numbers: vec![258, 1, 258],
    };
    // Independent Node framing and SHA-256, including list item/count bounds.
    assert_eq!(
        Schema::new(&[ListRow::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "44d48520a751a80c6d8327b3d7bd3677fcbe33b25335f20a66fee88d9e81e1fb"
    );
    let golden = [7, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 2, 1, 1, 0, 2, 1];
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(ListRow::decode(&golden).unwrap(), row);
    for end in 0..golden.len() {
        assert!(matches!(
            ListRow::decode(&golden[..end]),
            Err(Error::Corrupt)
        ));
    }
    let mut extra = golden.to_vec();
    extra.push(0);
    assert!(matches!(ListRow::decode(&extra), Err(Error::Corrupt)));
    let mut over = golden;
    over[9..13].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(ListRow::decode(&over), Err(Error::Corrupt)));
    let empty = ListRow {
        id: 7,
        labels: Some(vec![]),
        numbers: vec![],
    };
    assert_eq!(
        empty.encode().unwrap(),
        [7, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_ne!(
        empty.encode().unwrap(),
        ListRow {
            id: 7,
            labels: None,
            numbers: vec![]
        }
        .encode()
        .unwrap()
    );
    let ordered = ListRow {
        id: 7,
        labels: Some(vec!["é".into(), "".into(), "é".into()]),
        numbers: vec![],
    };
    assert_eq!(
        ListRow::decode(&ordered.encode().unwrap()).unwrap(),
        ordered
    );
    for row in [
        ListRow {
            id: 7,
            labels: None,
            numbers: vec![0; 4],
        },
        ListRow {
            id: 7,
            labels: Some(vec!["12345".into()]),
            numbers: vec![],
        },
    ] {
        assert!(matches!(row.encode(), Err(Error::Limit)));
        assert!(matches!(row.validate(), Err(Error::Limit)));
    }
    let mut malformed = vec![7, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 255];
    malformed.extend([0; 4]);
    assert!(matches!(ListRow::decode(&malformed), Err(Error::Corrupt)));
    assert!(Schema::new(&[EmptyOnly::SCHEMA]).is_ok());
    assert!(
        EmptyOnly {
            id: 0,
            items: vec![]
        }
        .encode()
        .is_ok()
    );
    assert!(matches!(
        EmptyOnly {
            id: 0,
            items: vec![false]
        }
        .encode(),
        Err(Error::Limit)
    ));
}
#[test]
fn lists_share_total_item_depth_and_structured_value_bounds() {
    // Individually legal items must still fit the whole framed row.
    let mut blobs = BlobList {
        id: 1,
        items: vec![vec![255; 2038], vec![0; 2038]],
    };
    let bytes = blobs.encode().unwrap();
    assert_eq!(bytes.len(), 4096);
    assert_eq!(BlobList::decode(&bytes).unwrap(), blobs);
    blobs.items[1].push(0);
    assert!(matches!(blobs.encode(), Err(Error::Limit)));
    let mut over = bytes;
    over[2054..2058].copy_from_slice(&2039_u32.to_le_bytes());
    over.push(0);
    assert!(matches!(BlobList::decode(&over), Err(Error::Corrupt)));
    let row = Siblings {
        id: 1,
        a: vec![1; 128],
        b: vec![2; 128],
    };
    let bytes = row.encode().unwrap();
    assert_eq!(Siblings::decode(&bytes).unwrap(), row);
    let row = Siblings {
        id: 1,
        a: vec![1; 128],
        b: vec![2; 129],
    };
    assert!(matches!(row.encode(), Err(Error::Limit)));
    // Two individually legal counts exceed the row-wide count before b is decoded.
    let mut over = bytes;
    over[132..136].copy_from_slice(&129_u32.to_le_bytes());
    over.insert(136, 2);
    assert!(matches!(Siblings::decode(&over), Err(Error::Corrupt)));
    let row = EmptyItems {
        id: 1,
        items: (0..255).map(|_| Empty {}).collect(),
    };
    let bytes = row.encode().unwrap();
    assert_eq!(bytes.len(), 12);
    assert_eq!(EmptyItems::decode(&bytes).unwrap(), row);
    let row = EmptyItems {
        id: 1,
        items: (0..256).map(|_| Empty {}).collect(),
    };
    assert!(matches!(row.encode(), Err(Error::Limit)));
    let mut over = bytes;
    over[8..12].copy_from_slice(&256_u32.to_le_bytes());
    assert!(matches!(EmptyItems::decode(&over), Err(Error::Corrupt)));
    assert!(Schema::new(&[ForestRow::SCHEMA]).is_ok());
    fn chain(n: usize) -> Tree {
        (0..n).fold(Tree { children: vec![] }, |child, _| Tree {
            children: vec![child],
        })
    }
    // Enum + forest list + fifteen record/list levels reaches depth 32.
    for n in [0, 14] {
        let row = ForestRow {
            id: 1,
            trees: Forest::Forest(vec![chain(n)]),
        };
        assert_eq!(ForestRow::decode(&row.encode().unwrap()).unwrap(), row);
    }
    assert!(matches!(
        ForestRow {
            id: 1,
            trees: Forest::Forest(vec![chain(15)])
        }
        .encode(),
        Err(Error::Limit)
    ));
    let mut over = vec![1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0];
    for _ in 0..15 {
        over.extend(1_u32.to_le_bytes());
    }
    over.extend(0_u32.to_le_bytes());
    assert!(matches!(ForestRow::decode(&over), Err(Error::Corrupt)));
}
#[test]
fn persisted_list_changes_reopen_and_corruption_refuses_before_mutation() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let contract = || Schema::new(&[ListRow::SCHEMA]).unwrap();
    assert_ne!(
        contract().fingerprint(),
        Schema::new(&[WiderListRow::SCHEMA]).unwrap().fingerprint()
    );
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [4; 16]).unwrap();
    let row = ListRow {
        id: 7,
        labels: Some(vec!["hi".into()]),
        numbers: vec![1, 2],
    };
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(*backing.borrow(), before);
    drop(db);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(db.get::<ListRow>(7).unwrap(), Some(row));
    let changed = ListRow {
        id: 7,
        labels: None,
        numbers: vec![2, 1],
    };
    let mut batch = Batch::new();
    batch.replace(&changed).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.revision::<ListRow>().unwrap(), 2);
    drop(db);
    let before = backing.borrow().clone();
    assert!(matches!(
        Database::open(
            runtime.open_memory(&key).unwrap(),
            Schema::new(&[WiderListRow::SCHEMA]).unwrap()
        ),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap()).unwrap();
    let mut row_key = vec![2];
    row_key.extend(ListRow::ID.to_be_bytes());
    row_key.extend(7_u64.to_be_bytes());
    let mut corrupt = changed.encode().unwrap();
    corrupt[9..13].copy_from_slice(&4_u32.to_le_bytes());
    raw.insert(row_key, corrupt);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.delete::<ListRow>(7).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}
