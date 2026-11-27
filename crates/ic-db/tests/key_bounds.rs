//! Complete encoded-key admission, independent of row and field bounds.

#[path = "support/allocations.rs"]
mod allocations;
#[path = "support/raw_map.rs"]
pub mod raw_map;
use crate::raw_map::RawMap;
use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error};
use ic_storage::{MemoryRuntime, StableKey, VectorMemory};

///
/// TextKey
/// Exercises indexed access by `text` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(field = "text"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "text", value(item(prim = "Text", max_bytes = 4096))),
        field(name = "padding", value(item(prim = "Blob", max_bytes = 4096)))
    )
)]
struct TextKey {}

///
/// Compound
/// Exercises indexed access by `a` and `b` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(field = "a"),
    index(fields = ["a", "b"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "a", value(item(prim = "Text", max_bytes = 4096))),
        field(name = "b", value(item(prim = "Text", max_bytes = 4096)))
    )
)]
struct Compound {}

///
/// Sparse
/// Exercises indexed access by `a` and `b` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["a", "b"], unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "a", value(item(prim = "Text", max_bytes = 4096))),
        field(name = "b", value(opt, item(prim = "Text", max_bytes = 4096)))
    )
)]
struct Sparse {}

///
/// Wide
/// Exercises indexed access by `text` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(field = "text"),
    fields(
        field(name = "id", value(item(prim = "Subaccount"))),
        field(name = "text", value(item(prim = "Text", max_bytes = 4096)))
    )
)]
struct Wide {}

fn host(backing: VectorMemory) -> MemoryRuntime<VectorMemory> {
    allocations::host_with(backing, &["test.keys.v1"])
}

#[test]
fn complete_keys_count_escaping_and_owner_before_any_mutation() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.keys.v1").unwrap();
    let schema = || Schema::new(&[TextKey::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [1; 16]).unwrap();
    // Literal frame: namespace 9 + text 493 + terminator 2 + Nat64 owner 8 = 512.
    let mut row = TextKey {
        id: 1,
        text: "x".repeat(493),
        padding: vec![0; 3000],
    };
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(
        db.index_prefix::<TextKey>("text", &[row.text.as_str().into()], 1)
            .unwrap(),
        [row.clone()]
    );
    for text in ["x".repeat(494), "\0".repeat(247)] {
        let before = backing.borrow().clone();
        let mut batch = Batch::new();
        batch
            .insert(&TextKey {
                id: 2,
                text: "valid".into(),
                padding: vec![],
            })
            .unwrap();
        batch
            .replace(&TextKey {
                text: text.clone(),
                ..row.clone()
            })
            .unwrap();
        assert!(matches!(db.commit(batch), Err(Error::Limit)));
        assert!(matches!(
            db.index_prefix::<TextKey>("text", &[text.as_str().into()], 1),
            Err(Error::Limit)
        ));
        assert_eq!(*backing.borrow(), before);
    }
    row.text = format!("{}a", "\0".repeat(246)); // 492 escaped bytes + one ordinary byte.
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(*backing.borrow(), before);
    drop(db);
    let raw = RawMap::load(runtime.open_memory(&slot).unwrap());
    let mut prefix = vec![3];
    prefix.extend(TextKey::SCHEMA.id.to_be_bytes());
    let keys: Vec<_> = raw
        .range(prefix.clone()..)
        .take_while(|e| e.key().starts_with(&prefix))
        .map(|e| e.key().0.clone())
        .collect();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].len(), 512);
    drop(raw);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert_eq!(db.get::<TextKey>(1).unwrap(), Some(row));
}

#[test]
fn combined_fields_wide_owners_and_sparse_membership_share_the_cap() {
    let runtime = host(VectorMemory::default());
    let slot = StableKey::parse("test.keys.v1").unwrap();
    let schema = Schema::new(&[Compound::SCHEMA, Wide::SCHEMA, Sparse::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema, [1; 16]).unwrap();
    let mut row = Compound {
        id: 1,
        a: "a".repeat(245),
        b: "b".repeat(246),
    };
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(
        db.index_prefix::<Compound>("a,b", &[row.a.as_str().into(), row.b.as_str().into()], 1)
            .unwrap(),
        [row.clone()]
    );
    row.b.push('b');
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Limit)));
    assert!(matches!(
        db.index_prefix::<Compound>("a,b", &[row.a.as_str().into(), row.b.as_str().into()], 1),
        Err(Error::Limit)
    ));
    // A leading prefix must also leave room for the unknown b terminator and owner.
    assert!(matches!(
        db.index_prefix::<Compound>("a,b", &["a".repeat(492).as_str().into()], 1),
        Err(Error::Limit)
    ));
    let mut wide = Wide {
        id: [7; 32],
        text: "x".repeat(469),
    };
    let mut batch = Batch::new();
    batch.insert(&wide).unwrap();
    db.commit(batch).unwrap();
    wide.text.push('x');
    let mut batch = Batch::new();
    batch.replace(&wide).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Limit)));
    assert!(matches!(
        db.index_prefix::<Wide>("text", &[wide.text.as_str().into()], 1),
        Err(Error::Limit)
    ));
    let sparse = Sparse {
        id: 1,
        a: "x".repeat(3000),
        b: None,
    };
    let mut batch = Batch::new();
    batch.insert(&sparse).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.get::<Sparse>(1).unwrap(), Some(sparse.clone()));
    let mut batch = Batch::new();
    batch
        .replace(&Sparse {
            b: Some(String::new()),
            ..sparse
        })
        .unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Limit)));
}

#[test]
fn no_op_cannot_admit_an_overlong_stored_after_image() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.keys.v1").unwrap();
    let schema = || Schema::new(&[TextKey::SCHEMA]).unwrap();
    drop(Database::create(runtime.open_memory(&slot).unwrap(), schema(), [1; 16]).unwrap());
    let row = TextKey {
        id: 1,
        text: "x".repeat(494),
        padding: vec![],
    };
    let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
    let mut key = vec![2];
    key.extend(TextKey::SCHEMA.id.to_be_bytes());
    key.extend(1_u64.to_be_bytes());
    raw.insert(key, row.encode().unwrap());
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Limit)));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn selected_index_read_checks_unrelated_complete_key_bound() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.keys.v1").unwrap();
    let schema = || Schema::new(&[Compound::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [1; 16]).unwrap();
    let mut row = Compound {
        id: 1,
        a: "x".into(),
        b: "y".repeat(490),
    };
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(
        db.query::<Compound>()
            .using_index("a", &["x".into()])
            .count()
            .unwrap()
            .value,
        1
    );
    drop(db);

    // Keep the selected "a" entry valid. Only the unrelated "a,b" key now
    // exceeds 512 bytes (namespace 9 + a 3 + b 493 + owner 8).
    row.b.push('y');
    let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
    let key = [
        &[2][..],
        &Compound::SCHEMA.id.to_be_bytes(),
        &row.id.to_be_bytes(),
    ]
    .concat();
    assert!(raw.insert(key, row.encode().unwrap()).is_some());
    drop(raw);
    let before = backing.borrow().clone();
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert!(matches!(
        db.query::<Compound>()
            .using_index("a", &["x".into()])
            .count_validated(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.index_prefix::<Compound>("a", &["x".into()], 1),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
}

///
/// Labels
/// Holds bounded text and raw bytes for two member indexes.
///
#[record(fields(
    field(name = "text", value(item(prim = "Text", max_bytes = 512))),
    field(name = "blob", value(item(prim = "Blob", max_bytes = 512)))
))]
struct Labels {}

///
/// LabeledRow
/// Exercises both member indexes at the complete physical key limit.
///
#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(field = "labels.text"),
    index(field = "labels.blob"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "labels", value(item(is = "Labels")))
    )
)]
struct LabeledRow {}

#[test]
fn nested_text_and_blob_keys_charge_escaped_bytes_before_committing() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.keys.v1").unwrap();
    let memory = || runtime.open_memory(&slot).unwrap();
    let schema = Schema::new(&[LabeledRow::SCHEMA]).unwrap();
    let mut db = Database::create(memory(), schema.clone(), [2; 16]).unwrap();
    // 9 header + 246 escaped zeros (492) + 1 nonzero + 2 terminator + 8 owner.
    let mut blob = vec![0; 246];
    blob.push(255);
    let row = LabeledRow {
        id: 1,
        labels: Labels {
            text: format!("{}x", "\0".repeat(246)),
            blob,
        },
    };
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let mut db = Database::open(memory(), schema).unwrap();
    for text in [true, false] {
        let before = backing.borrow().clone();
        let mut oversized = row.clone();
        if text {
            oversized.labels.text.push('x');
        } else {
            oversized.labels.blob.push(255);
        }
        let mut batch = Batch::new();
        batch
            .insert(&LabeledRow {
                id: 2,
                labels: Labels {
                    text: String::new(),
                    blob: Vec::new(),
                },
            })
            .unwrap();
        batch.replace(&oversized).unwrap();
        assert!(matches!(db.commit(batch), Err(Error::Limit)));
        assert_eq!(*backing.borrow(), before);
    }
    let mut batch = Batch::new();
    batch.delete::<LabeledRow>(1).unwrap();
    db.commit(batch).unwrap();
    for (path, value) in [
        (
            ic_db::field_path!(LabeledRow::LABELS, Labels::TEXT),
            ic_db::IndexValue::from(&row.labels.text),
        ),
        (
            ic_db::field_path!(LabeledRow::LABELS, Labels::BLOB),
            ic_db::IndexValue::from(&row.labels.blob),
        ),
    ] {
        assert_eq!(
            db.query::<LabeledRow>()
                .eq(path, value)
                .count()
                .unwrap()
                .value,
            0
        );
    }
}
