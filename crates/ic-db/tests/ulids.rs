//! Independent ULID bytes, canonical text and stored/indexed identity boundaries.

#[path = "support/raw_map.rs"]
pub mod raw_map;
#[path = "support/host.rs"]
mod test_host;
use crate::raw_map::RawMap;
use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error, IndexValue};
use ic_storage::{StableKey, VectorMemory};
use std::collections::{BTreeMap, BTreeSet};
use test_host::host;

///
/// UlidRow
/// Exercises indexed access by `zone`, `value`, `id` and `maybe` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["zone", "value", "id"]),
    index(field = "maybe", unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "maybe", value(opt, item(prim = "Ulid"))),
        field(name = "value", value(item(prim = "Ulid"))),
        field(name = "zone", value(item(prim = "Bool")))
    )
)]
struct UlidRow {}

///
/// Ids
/// Stores up to 3 distinct `Ulid` values.
///

#[set(item(prim = "Ulid"), max_items = 3)]
struct Ids {}

///
/// Links
/// Maps up to 3 `Ulid` keys to optional `Ulid` values.
///

#[map(key(prim = "Ulid"), value(opt, item(prim = "Ulid")), max_items = 3)]
struct Links {}

///
/// Collections
/// Stores `list`, `map` and `set` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "list", value(many, max_items = 3, item(prim = "Ulid"))),
        field(name = "map", value(item(is = "Links"))),
        field(name = "set", value(item(is = "Ids")))
    )
)]
struct Collections {}

fn values() -> [Ulid; 6] {
    // Includes randomness overflow into the next millisecond and the top bit.
    [
        0,
        1,
        (1_u128 << 80) - 1,
        1_u128 << 80,
        1_u128 << 127,
        u128::MAX,
    ]
    .map(Ulid::from_u128)
}

#[test]
fn canonical_text_exact_bytes_and_distinct_schema_identity() {
    assert_eq!(
        Schema::new(&[UlidRow::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "941dcdaab8ea9a0c46d1305ebc35a8a54ee1aef816c5ac951402bb01c2aced60"
    );
    let known = "01ARZ3NDEKTSV4RRFFQ69G5FAV".parse::<Ulid>().unwrap();
    assert_eq!(
        known.to_bytes(),
        [
            1, 86, 62, 58, 181, 211, 214, 118, 76, 97, 239, 185, 147, 2, 189, 91
        ]
    );
    assert_eq!(known.timestamp().as_millis(), 1_469_922_850_259);
    assert_eq!(Ulid::MIN.timestamp(), Timestamp::EPOCH);
    assert_eq!(Ulid::MAX.timestamp().as_millis(), (1_i64 << 48) - 1);
    assert_eq!(format!("{known:?}"), "\"01ARZ3NDEKTSV4RRFFQ69G5FAV\"");
    assert_eq!(Ulid::MIN.to_string(), "00000000000000000000000000");
    assert_eq!(Ulid::MAX.to_string(), "7ZZZZZZZZZZZZZZZZZZZZZZZZZ");
    for invalid in [
        "01arz3ndektsv4rrffq69g5fav",
        "80000000000000000000000000",
        "0000000000000000000000000I",
        "",
        " 01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "not-a-ulid",
    ] {
        assert!(invalid.parse::<Ulid>().is_err(), "{invalid}");
    }
    for value in values() {
        assert_eq!(value.to_string().parse::<Ulid>().unwrap(), value);
        let row = UlidRow {
            id: 7,
            maybe: Some(value),
            value,
            zone: true,
        };
        assert_eq!(UlidRow::decode(&row.encode().unwrap()).unwrap(), row);
    }
    let row = UlidRow {
        id: 7,
        maybe: None,
        value: Ulid::from_bytes(std::array::from_fn(|i| i as u8 + 1)),
        zone: true,
    };
    let mut golden = vec![7, 0, 0, 0, 0, 0, 0, 0, 0];
    golden.extend(1_u8..=16);
    golden.push(1);
    assert_eq!(row.encode().unwrap(), golden);
    for end in 0..golden.len() {
        assert!(matches!(
            UlidRow::decode(&golden[..end]),
            Err(Error::Corrupt)
        ));
    }
    let mut excess = golden;
    excess.push(0);
    assert!(matches!(UlidRow::decode(&excess), Err(Error::Corrupt)));
    let present = UlidRow {
        maybe: Some(Ulid::MIN),
        ..row
    };
    assert_ne!(row.encode().unwrap(), present.encode().unwrap());
    let mut invalid = present.encode().unwrap();
    invalid[8] = 2;
    assert!(matches!(UlidRow::decode(&invalid), Err(Error::Corrupt)));
    // Nat128 is not the same schema type, even though both occupy 16 bytes.
    let mut fields = UlidRow::SCHEMA.fields.to_vec();
    fields.iter_mut().find(|f| f.name == "value").unwrap().value = ic_db::FieldType::Nat128;
    let other = ic_db::EntitySchema {
        fields: Box::leak(fields.into_boxed_slice()),
        ..*UlidRow::SCHEMA
    };
    assert_ne!(
        Schema::new(&[UlidRow::SCHEMA]).unwrap().fingerprint(),
        Schema::new(&[Box::leak(Box::new(other))])
            .unwrap()
            .fingerprint()
    );
}

#[test]
fn ordered_indexes_swaps_noops_refusals_and_reopen() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[UlidRow::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [9; 16]).unwrap();
    let mut rows: Vec<_> = values()
        .into_iter()
        .enumerate()
        .map(|(i, value)| UlidRow {
            id: i as u64 + 1,
            maybe: Some(value),
            value,
            zone: true,
        })
        .collect();
    let mut batch = Batch::new();
    for r in rows.iter().rev() {
        batch.insert(r).unwrap();
    }
    db.commit(batch).unwrap();
    let page = db
        .query::<UlidRow>()
        .eq(UlidRow::ZONE, true)
        .order_asc(UlidRow::VALUE)
        .select(|row| row.value)
        .fetch()
        .unwrap();
    assert_eq!(page.rows, values());
    let read = |db: &Database| {
        db.index_prefix::<UlidRow>("zone,value,id", &[true.into()], 32)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(read(&db), vec![1, 2, 3, 4, 5, 6]);
    for pair in values().windows(2) {
        assert!(pair[0] < pair[1]);
        let key = |v: Ulid| {
            ic_db::__private::encode_index(
                &[ic_db::__private::IndexComponent {
                    value: v.into(),
                    optional: false,
                }],
                false,
            )
            .unwrap()
        };
        assert_eq!(key(pair[0]), pair[0].to_bytes());
        assert!(key(pair[0]) < key(pair[1]));
    }
    assert!(matches!(
        db.index_prefix::<UlidRow>("maybe", &[IndexValue::Nat128(0)], 32),
        Err(Error::InvalidOperand)
    ));
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    db.commit(b).unwrap();
    assert_eq!(*backing.borrow(), before);
    let duplicate = UlidRow {
        maybe: Some(Ulid::MAX),
        ..rows[0].clone()
    };
    let mut b = Batch::new();
    b.replace(&duplicate).unwrap();
    assert!(matches!(db.commit(b), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    rows[0].maybe = Some(Ulid::MAX);
    rows[5].maybe = Some(Ulid::MIN);
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    b.replace(&rows[5]).unwrap();
    db.commit(b).unwrap();
    assert_eq!(
        db.index_prefix::<UlidRow>("maybe", &[Ulid::MAX.into()], 32)
            .unwrap()[0]
            .id,
        1
    );
    drop(db);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert_eq!(read(&db), vec![1, 2, 3, 4, 5, 6]);
    for r in rows {
        assert_eq!(db.get::<UlidRow>(r.id).unwrap(), Some(r));
    }
    drop(db);
    let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
    let index = UlidRow::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "zone,value,id")
        .unwrap();
    let mut physical = vec![3];
    physical.extend(UlidRow::SCHEMA.id.to_be_bytes());
    physical.extend(index.id.to_be_bytes());
    physical.push(1);
    physical.extend([255; 16]);
    physical.extend(6_u64.to_be_bytes());
    physical.extend(6_u64.to_be_bytes());
    assert!(raw.get(&physical).is_some());
    let mut short = physical.clone();
    short.remove(10);
    raw.remove(&physical);
    raw.insert(short, 6_u64.to_be_bytes().to_vec());
    drop(raw);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert!(matches!(
        db.index_prefix::<UlidRow>("zone,value,id", &[true.into()], 32),
        Err(Error::Corrupt)
    ));
}

#[test]
fn collections_reject_duplicate_keys_before_materialization() {
    let row = Collections {
        id: 1,
        list: vec![Ulid::MAX, Ulid::MIN, Ulid::MAX],
        map: Links(BTreeMap::from([
            (Ulid::MIN, None),
            (Ulid::MAX, Some(Ulid::MIN)),
        ])),
        set: Ids(BTreeSet::from([Ulid::MIN, Ulid::MAX])),
    };
    let bytes = row.encode().unwrap();
    assert_eq!(Collections::decode(&bytes).unwrap(), row);
    assert_eq!(&bytes[12..28], &[255; 16]);
    assert_eq!(&bytes[28..44], &[0; 16]);
    // Map begins at 60: count, 16-byte key, absent tag, second key.
    let mut duplicate = bytes;
    duplicate[81..97].fill(0);
    assert!(matches!(
        Collections::decode(&duplicate),
        Err(Error::Corrupt)
    ));
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[Collections::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [9; 16]).unwrap();
    let mut b = Batch::new();
    b.insert(&row).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
    let mut key = vec![2];
    key.extend(Collections::SCHEMA.id.to_be_bytes());
    key.extend(1_u64.to_be_bytes());
    raw.insert(key, duplicate);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.delete::<Collections>(1).unwrap();
    assert!(matches!(db.commit(b), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}

///
/// WideList
/// Stores `values` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "values", value(many, max_items = 256, item(prim = "Ulid")))
    )
)]
struct WideList {}
#[test]
fn row_byte_limit_admits_255_values_but_refuses_256() {
    let mut row = WideList {
        id: 1,
        values: vec![Ulid::MAX; 255],
    };
    let bytes = row.encode().unwrap();
    assert_eq!(bytes.len(), 4092);
    assert_eq!(WideList::decode(&bytes).unwrap(), row);
    row.values.push(Ulid::MAX);
    assert!(matches!(row.encode(), Err(Error::Limit)));
    let mut bad = bytes;
    bad[8..12].copy_from_slice(&256_u32.to_le_bytes());
    bad.extend([255; 16]);
    assert!(matches!(WideList::decode(&bad), Err(Error::Corrupt)));
}

#[test]
fn full_ulid_time_counts_respect_field_presence() {
    let fixture = ic_db::testing::Fixture::new("test.ulid.v1");
    let mut db = fixture
        .create(Schema::new(&[UlidRow::SCHEMA]).unwrap(), [1; 16])
        .unwrap();
    let mut batch = Batch::new();
    for (id, value) in values().into_iter().enumerate() {
        batch
            .insert(&UlidRow {
                id: id as u64,
                value,
                maybe: (id != 1).then_some(value),
                zone: true,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let all_time = Timestamp::MIN..Timestamp::MAX;
    let required = db
        .query::<UlidRow>()
        .filter_ulid_time(UlidRow::VALUE, all_time.clone())
        .work_limit(1)
        .count()
        .unwrap();
    assert_eq!(
        (required.value, required.examined, required.fetched),
        (6, 0, 0)
    );
    let optional = db
        .query::<UlidRow>()
        .filter_ulid_time(UlidRow::MAYBE, all_time)
        .count()
        .unwrap();
    assert_eq!(
        (optional.value, optional.examined, optional.fetched),
        (5, 5, 0)
    );
    let partial = db
        .query::<UlidRow>()
        .filter_ulid_time(UlidRow::MAYBE, Timestamp::EPOCH..Timestamp::from_millis(1))
        .count()
        .unwrap();
    assert_eq!(partial.value, 2);
}
