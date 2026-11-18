//! Nested values use a finite compiled graph and one shared bounded row traversal.
use ic_db::{Batch, Database, Entity, Error, Schema, Value, entity, enum_, record};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

#[record(fields(
    field(name = "x", value(item(prim = "Nat16"))),
    field(name = "label", value(item(prim = "Text", max_bytes = 8)))
))]
struct Meta {}
#[record(fields(
    field(name = "right", value(item(indirect, is = "Event"))),
    field(name = "left", value(item(indirect, is = "Event")))
))]
struct Pair {}
#[enum_(
    variant(name = "Maybe", value(opt, item(prim = "Nat64"))),
    variant(name = "Idle"),
    variant(name = "Loop", value(item(indirect, is = "Self"))),
    variant(name = "Data", value(item(is = "Meta"))),
    variant(name = "Pair", value(item(is = "Pair")))
)]
struct Event {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "payload", value(item(is = "Event"))),
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "meta", value(opt, item(is = "Meta")))
    )
)]
struct Asset {}
#[record(fields(field(name = "next", value(opt, item(indirect, is = "Self")))))]
struct Node {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "node", value(item(is = "Node")))
    )
)]
struct NodeRow {}
#[record(fields(field(name = "next", value(item(indirect, is = "Self")))))]
struct Infinite {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(is = "Infinite")))
    )
)]
struct ImpossibleRow {}
#[enum_(variant(name = "Again", value(item(indirect, is = "Self"))))]
struct NoBase {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(is = "NoBase")))
    )
)]
struct ImpossibleEnumRow {}

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

fn asset(payload: Event) -> Asset {
    Asset {
        id: 7,
        meta: None,
        payload,
    }
}

fn loops(count: usize) -> Event {
    (0..count).fold(Event::Idle, |value, _| Event::Loop(Box::new(value)))
}

fn tree(depth: u8) -> Event {
    if depth == 0 {
        Event::Idle
    } else {
        Event::Pair(Pair {
            left: Box::new(tree(depth - 1)),
            right: Box::new(tree(depth - 1)),
        })
    }
}

#[test]
fn payloads_have_exact_bytes_and_no_hidden_materialization_rules() {
    let row = asset(Event::Data(Meta {
        label: "A".into(),
        x: 0x1234,
    }));
    // id, absent meta, Data ordinal, length-framed label, exact little-endian x.
    let golden = [7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, b'A', 0x34, 0x12];
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(Asset::decode(&golden).unwrap(), row);
    for end in 0..golden.len() {
        assert!(matches!(Asset::decode(&golden[..end]), Err(Error::Corrupt)));
    }
    let mut extra = golden.to_vec();
    extra.push(0);
    assert!(matches!(Asset::decode(&extra), Err(Error::Corrupt)));
    let mut invalid = golden;
    invalid[14] = 255;
    assert!(matches!(Asset::decode(&invalid), Err(Error::Corrupt)));
    invalid = golden;
    invalid[10] = 9;
    assert!(matches!(Asset::decode(&invalid), Err(Error::Corrupt)));
    invalid = golden;
    invalid[9] = 255;
    assert!(matches!(Asset::decode(&invalid), Err(Error::Corrupt)));
    assert!(matches!(
        asset(Event::Data(Meta {
            label: "123456789".into(),
            x: 1
        }))
        .encode(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        asset(Event::Data(Meta {
            label: "123456789".into(),
            x: 1
        }))
        .validate(),
        Err(Error::Limit)
    ));
    assert_eq!(
        asset(Event::Maybe(None)).encode().unwrap(),
        [7, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0]
    );
    assert_eq!(
        asset(Event::Idle).encode().unwrap(),
        [7, 0, 0, 0, 0, 0, 0, 0, 0, 1]
    );
    for value in [Event::Maybe(Some(u64::MAX)), tree(3), loops(3)] {
        let row = asset(value);
        assert_eq!(Asset::decode(&row.encode().unwrap()).unwrap(), row);
    }
}

#[test]
fn recursive_graphs_require_a_base_and_values_share_depth_and_node_bounds() {
    assert!(Schema::new(&[Asset::SCHEMA, NodeRow::SCHEMA]).is_ok());
    assert!(matches!(
        Schema::new(&[ImpossibleRow::SCHEMA]),
        Err(Error::InvalidSchema)
    ));
    assert!(matches!(
        Schema::new(&[ImpossibleEnumRow::SCHEMA]),
        Err(Error::InvalidSchema)
    ));
    for count in [0, 31] {
        let row = asset(loops(count));
        assert_eq!(Asset::decode(&row.encode().unwrap()).unwrap(), row);
    }
    assert!(matches!(asset(loops(32)).encode(), Err(Error::Limit)));
    assert!(matches!(asset(loops(32)).validate(), Err(Error::Limit)));
    // Independent byte producer bypasses the writer, exposing decode's own bound.
    let mut deep = vec![7, 0, 0, 0, 0, 0, 0, 0, 0];
    deep.extend([2; 32]);
    deep.push(1);
    assert!(matches!(Asset::decode(&deep), Err(Error::Corrupt)));
    let row = asset(tree(6));
    assert_eq!(Asset::decode(&row.encode().unwrap()).unwrap(), row);
    assert!(matches!(asset(tree(7)).encode(), Err(Error::Limit)));
    fn raw_tree(bytes: &mut Vec<u8>, depth: u8) {
        if depth == 0 {
            bytes.push(1);
        } else {
            bytes.push(4);
            raw_tree(bytes, depth - 1);
            raw_tree(bytes, depth - 1);
        }
    }
    let mut wide = vec![7, 0, 0, 0, 0, 0, 0, 0, 0];
    raw_tree(&mut wide, 7);
    assert!(wide.len() < 4096);
    assert!(matches!(Asset::decode(&wide), Err(Error::Corrupt)));
    let mut next = Node { next: None };
    for _ in 0..31 {
        next = Node {
            next: Some(Box::new(next)),
        };
    }
    let row = NodeRow { id: 1, node: next };
    assert_eq!(NodeRow::decode(&row.encode().unwrap()).unwrap(), row);
}

#[test]
fn persisted_nested_values_reopen_and_corrupt_unindexed_payloads_refuse_before_mutation() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let contract = || Schema::new(&[Asset::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [6; 16]).unwrap();
    let row = asset(Event::Data(Meta {
        label: "A".into(),
        x: 0x1234,
    }));
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
    assert_eq!(db.get::<Asset>(7).unwrap(), Some(row));
    let mut batch = Batch::new();
    batch.replace(&asset(loops(31))).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    let mut row_key = vec![2];
    row_key.extend(Asset::ID.to_be_bytes());
    row_key.extend(7_u64.to_be_bytes());
    let mut corrupt = vec![7, 0, 0, 0, 0, 0, 0, 0, 0];
    corrupt.extend([2; 32]);
    corrupt.push(1);
    raw.insert(row_key, corrupt);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let before = backing.borrow().clone();
    for delete in [true, false] {
        let mut batch = Batch::new();
        if delete {
            batch.delete::<Asset>(7).unwrap();
        } else {
            batch.replace(&asset(Event::Idle)).unwrap();
        }
        assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
        assert_eq!(*backing.borrow(), before);
    }
}

// Same source-independent names, with a changed leaf hidden behind a record link.
#[record(
    name = "Meta",
    fields(
        field(name = "label", value(item(prim = "Text", max_bytes = 8))),
        field(name = "x", value(item(prim = "Nat32")))
    )
)]
struct ChangedMeta {}
#[record(fields(field(name = "meta", value(item(is = "Meta")))))]
struct Container {}
#[record(
    name = "Container",
    fields(field(name = "meta", value(item(is = "ChangedMeta"))))
)]
struct ChangedContainer {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(is = "Container")))
    )
)]
struct Wrapped {}
#[entity(
    name = "Wrapped",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(is = "ChangedContainer")))
    )
)]
struct ChangedWrapped {}
#[test]
fn transitive_named_contracts_are_checked_at_typed_access_and_reopen() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let old = Schema::new(&[Wrapped::SCHEMA]).unwrap();
    let changed = || Schema::new(&[ChangedWrapped::SCHEMA]).unwrap();
    assert_ne!(old.fingerprint(), changed().fingerprint());
    let db = Database::create(runtime.open_memory(&key).unwrap(), old, [5; 16]).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.get::<ChangedWrapped>(1),
        Err(Error::UnknownEntity)
    ));
    drop(db);
    assert!(matches!(
        Database::open(runtime.open_memory(&key).unwrap(), changed()),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
    // Links do not recursively expand the static descriptor.
    assert_eq!(Container::definition().name, "Container");
}

#[test]
fn node_budget_is_shared_across_sibling_entity_fields() {
    let right = (0..18).fold(tree(4), |value, _| Event::Loop(Box::new(value)));
    // Pair=2 named nodes, left=190, right=46+18: exactly 256.
    let mut row = asset(Event::Pair(Pair {
        left: Box::new(tree(6)),
        right: Box::new(right),
    }));
    let exact = row.encode().unwrap();
    assert_eq!(Asset::decode(&exact).unwrap(), row);
    row.meta = Some(Meta {
        label: String::new(),
        x: 0,
    });
    assert!(matches!(row.encode(), Err(Error::Limit)));
    let mut over = exact;
    over.splice(8..9, [1, 0, 0, 0, 0, 0, 0]);
    assert!(matches!(Asset::decode(&over), Err(Error::Corrupt)));
}
#[enum_(variant(name = "Bytes", value(item(prim = "Blob", max_bytes = 4096))))]
struct BytesPayload {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(is = "BytesPayload")))
    )
)]
struct ByteRow {}
#[test]
fn payloads_share_the_whole_row_byte_envelope() {
    let row = ByteRow {
        id: 1,
        payload: BytesPayload::Bytes(vec![255; 4083]),
    };
    let bytes = row.encode().unwrap();
    assert_eq!(bytes.len(), 4096);
    assert_eq!(ByteRow::decode(&bytes).unwrap(), row);
    let row = ByteRow {
        id: 1,
        payload: BytesPayload::Bytes(vec![255; 4084]),
    };
    assert!(matches!(row.encode(), Err(Error::Limit)));
    let mut over = bytes;
    over.push(255);
    assert!(matches!(ByteRow::decode(&over), Err(Error::Corrupt)));
}
#[record(
    name = "Meta",
    fields(
        field(name = "label", value(item(prim = "Text", max_bytes = 8))),
        field(name = "x", value(item(prim = "Nat16")))
    )
)]
struct ReorderedMeta {}
#[record(
    name = "Container",
    fields(field(name = "meta", value(item(is = "ReorderedMeta"))))
)]
struct ReorderedContainer {}
#[entity(
    name = "Wrapped",
    pk(field = "id"),
    fields(
        field(name = "value", value(item(is = "ReorderedContainer"))),
        field(name = "id", value(item(prim = "Nat64")))
    )
)]
struct ReorderedWrapped {}
#[test]
fn source_renames_and_record_field_reordering_preserve_the_admitted_contract() {
    assert_eq!(
        Schema::new(&[Wrapped::SCHEMA]).unwrap().fingerprint(),
        Schema::new(&[ReorderedWrapped::SCHEMA])
            .unwrap()
            .fingerprint()
    );
    let row = Wrapped {
        id: 1,
        value: Container {
            meta: Meta {
                label: "A".into(),
                x: 3,
            },
        },
    };
    assert_eq!(
        ReorderedWrapped::decode(&row.encode().unwrap()).unwrap(),
        ReorderedWrapped {
            id: 1,
            value: ReorderedContainer {
                meta: ReorderedMeta {
                    label: "A".into(),
                    x: 3
                }
            }
        }
    );
}

#[record(fields())]
struct Empty {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "empty", value(item(is = "Empty"))),
        field(name = "id", value(item(prim = "Nat64")))
    )
)]
struct EmptyRow {}
#[test]
fn an_empty_record_has_a_finite_constructor_and_no_payload_bytes() {
    assert!(Schema::new(&[EmptyRow::SCHEMA]).is_ok());
    let row = EmptyRow {
        id: 7,
        empty: Empty {},
    };
    assert_eq!(row.encode().unwrap(), [7, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(EmptyRow::decode(&[7, 0, 0, 0, 0, 0, 0, 0]).unwrap(), row);
}
