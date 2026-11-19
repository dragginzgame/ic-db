//! Bounded ordered lists: independent count framing and shared row limits.

use ic_db::{Batch, Database, Entity, Error, Schema, entity, enum_, record};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

///
/// ListRow
/// Stores `labels` and `numbers` under the `id` primary key.
///

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

///
/// WiderListRow
/// Exercises an alternate `ListRow` declaration under the same logical identity.
///

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

///
/// Tree
/// Carries the `Self` value of `children`.
///

#[record(fields(field(name = "children", value(many, max_items = 4, item(is = "Self")))))]
struct Tree {}

///
/// Forest
/// Distinguishes the `Empty` and `Forest` variants.
///

#[enum_(
    variant(name = "Empty"),
    variant(name = "Forest", value(many, max_items = 4, item(is = "Tree")))
)]
struct Forest {}

///
/// ForestRow
/// Stores `trees` under the `id` primary key.
///

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "trees", value(item(is = "Forest")))
    )
)]
struct ForestRow {}

///
/// Siblings
/// Stores `a` and `b` under the `id` primary key.
///

#[entity(
    pk(field = "id"),
    fields(
        field(name = "a", value(many, max_items = 256, item(prim = "Nat8"))),
        field(name = "b", value(many, max_items = 256, item(prim = "Nat8"))),
        field(name = "id", value(item(prim = "Nat64")))
    )
)]
struct Siblings {}

///
/// Empty
/// Represents a record with no stored fields.
///

#[record(fields())]
struct Empty {}

///
/// EmptyItems
/// Stores `items` under the `id` primary key.
///

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "items", value(many, max_items = 256, item(is = "Empty")))
    )
)]
struct EmptyItems {}

///
/// EmptyOnly
/// Stores `items` under the `id` primary key.
///

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "items", value(many, max_items = 0, item(prim = "Bool")))
    )
)]
struct EmptyOnly {}

///
/// BlobList
/// Stores `items` under the `id` primary key.
///

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
        "edd3cd3e92634f85edde3e8ee997d28fbe88ffe20c5560d148174db1f5e8f18b"
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
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    let mut row_key = vec![2];
    row_key.extend(ListRow::SCHEMA.id.to_be_bytes());
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

///
/// StartedAt
/// Gives `Timestamp` a distinct schema identity.
///

#[ic_db::newtype(item(prim = "Timestamp"))]
struct StartedAt {}

///
/// ExpiresAt
/// Gives `Timestamp` a distinct schema identity.
///

#[ic_db::newtype(item(prim = "Timestamp"))]
struct ExpiresAt {}

///
/// Starts
/// Stores up to 2 ordered `StartedAt` values.
///

#[ic_db::list(item(is = "StartedAt"), max_items = 2)]
struct Starts {}

///
/// Event
/// Exercises indexed access by `started` on rows keyed by `id`.
///

#[ic_db::entity(
    pk(field = "id"),
    index(field = "started", unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "started", value(item(is = "StartedAt"))),
        field(name = "expires", value(item(is = "ExpiresAt"))),
        field(name = "history", value(item(is = "Starts"))),
        field(name = "previous", value(opt, item(is = "Starts")))
    )
)]
struct Event {}

///
/// WrongClock
/// Carries the `ExpiresAt` value of `started`.
///

#[record(fields(field(name = "started", value(item(is = "ExpiresAt")))))]
struct WrongClock {}

///
/// StartOnly
/// Carries the `StartedAt` value of `started`.
///

#[record(fields(field(name = "started", value(item(is = "StartedAt")))))]
struct StartOnly {}

#[test]
fn named_list_reuses_its_bound_and_scalar_wrappers_keep_identity_after_reopen() {
    use ic_db::{Comparison, Timestamp, Value};
    let at = |v| StartedAt(Timestamp::from_millis(v));
    let mut row = Event {
        id: 1,
        started: at(-3),
        expires: ExpiresAt(Timestamp::from_millis(7)),
        history: Starts(vec![at(5), at(5)]),
        previous: Some(Starts(vec![])),
    };
    let schema = || Schema::new(&[Event::SCHEMA]).unwrap();
    let definitions: Vec<_> = schema().types().map(|def| def.name).collect();
    assert_eq!(
        definitions,
        [ExpiresAt::NAME, StartedAt::NAME, Starts::NAME]
    );
    // Alphabetical fields: expires, history, id, previous, started.
    let mut bytes = 7_i64.to_le_bytes().to_vec();
    bytes.extend(2_u32.to_le_bytes());
    bytes.extend(5_i64.to_le_bytes().repeat(2));
    bytes.extend(1_u64.to_le_bytes());
    bytes.extend([1, 0, 0, 0, 0]);
    bytes.extend((-3_i64).to_le_bytes());
    assert_eq!(row.encode().unwrap(), bytes);
    bytes[8..12].copy_from_slice(&3_u32.to_le_bytes());
    assert!(matches!(Event::decode(&bytes), Err(Error::Corrupt)));

    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), schema(), [7; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
    let matches = db
        .query::<Event>()
        .using_index("started", &[(&at(-3)).into()])
        .filter("started", Comparison::Lt, &at(0))
        .fetch()
        .unwrap();
    assert_eq!(matches.rows, [row.clone()]);
    assert_eq!(
        db.query::<Event>().fetch_as::<StartOnly>().unwrap().rows,
        [StartOnly { started: at(-3) }]
    );
    assert!(matches!(
        db.query::<Event>().fetch_as::<WrongClock>(),
        Err(Error::InvalidQuery)
    ));
    // Both fields store timestamps. Only the StartedAt field is a valid
    // same-row needle for Starts, even when the collection is empty.
    for field in ["history", "previous"] {
        let query = || db.query::<Event>();
        assert!(matches!(
            query()
                .filter_fields(field, Comparison::Contains, "expires")
                .count(),
            Err(Error::InvalidQuery)
        ));
        assert_eq!(
            query()
                .filter_fields(field, Comparison::Contains, "started")
                .count()
                .unwrap()
                .value,
            0
        );
    }
    assert_eq!(
        db.query::<Event>()
            .filter("history", Comparison::Contains, &at(5))
            .count()
            .unwrap()
            .value,
        1
    );
    row.previous = Some(Starts(vec![at(0), at(1), at(2)]));
    assert!(matches!(row.encode(), Err(Error::Limit)));
    row.previous = None;
    row.history.0.push(at(9));
    assert!(matches!(row.encode(), Err(Error::Limit)));
    drop(db);
    let before = backing.borrow().clone();
    let changed = Schema::new(&[changed_bound::Event::SCHEMA]).unwrap();
    assert!(matches!(
        Database::open(runtime.open_memory(&key).unwrap(), changed),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
}

mod changed_bound {
    use super::{ExpiresAt, StartedAt};

    ///
    /// Starts
    /// Stores up to 3 ordered `StartedAt` values.
    ///

    #[ic_db::list(item(is = "StartedAt"), max_items = 3)]
    struct Starts {}

    ///
    /// Event
    /// Exercises indexed access by `started` on rows keyed by `id`.
    ///

    #[ic_db::entity(
        pk(field = "id"),
        index(field = "started", unique),
        fields(
            field(name = "id", value(item(prim = "Nat64"))),
            field(name = "started", value(item(is = "StartedAt"))),
            field(name = "expires", value(item(is = "ExpiresAt"))),
            field(name = "history", value(item(is = "Starts"))),
            field(name = "previous", value(opt, item(is = "Starts")))
        )
    )]
    pub(super) struct Event {}
}
