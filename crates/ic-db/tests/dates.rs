//! Independent Date bytes, calendar domain and stored/indexed identity boundaries.

#[path = "support/raw_map.rs"]
pub mod raw_map;
use crate::raw_map::RawMap;
use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error, IndexValue};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory,
};
use std::collections::{BTreeMap, BTreeSet};

///
/// DateRow
/// Exercises indexed access by `zone`, `value`, `id` and `maybe` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["zone", "value", "id"]),
    index(field = "maybe", unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "maybe", value(opt, item(prim = "Date"))),
        field(name = "value", value(item(prim = "Date"))),
        field(name = "zone", value(item(prim = "Bool")))
    )
)]
struct DateRow {}

///
/// Ids
/// Stores up to 3 distinct `Date` values.
///

#[set(item(prim = "Date"), max_items = 3)]
struct Ids {}

///
/// Links
/// Maps up to 3 `Date` keys to optional `Date` values.
///

#[map(key(prim = "Date"), value(opt, item(prim = "Date")), max_items = 3)]
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
        field(name = "list", value(many, max_items = 3, item(prim = "Date"))),
        field(name = "map", value(item(is = "Links"))),
        field(name = "set", value(item(is = "Ids")))
    )
)]
struct Collections {}
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

fn values() -> [Date; 6] {
    [-719528, -365, -1, 0, 1, 2932896].map(|d| Date::try_from_days_since_epoch(d).unwrap())
}

#[test]
fn calendar_and_epoch_days_agree_with_an_independent_day_counter() {
    // Walk calendar months independently of the March-cycle implementation.
    let mut epoch_day = -719_528;
    for year in 0..=9999 {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        for (m, count) in [
            31,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ]
        .into_iter()
        .enumerate()
        {
            for day in 1..=count {
                let date = Date::try_from_days_since_epoch(epoch_day).unwrap();
                assert_eq!(
                    (date.year(), date.month(), date.day()),
                    (year, m as u8 + 1, day)
                );
                assert_eq!(Date::try_new(year, m as u8 + 1, day), Some(date));
                if year == 1970 && m == 0 && day == 1 {
                    assert_eq!(date, Date::EPOCH);
                }
                epoch_day += 1;
            }
            assert!(Date::try_new(year, m as u8 + 1, count + 1).is_none());
        }
    }
    assert_eq!(epoch_day - 1, Date::MAX.as_days_since_epoch());
    for text in [
        "0000-01-01",
        "1900-02-28",
        "2000-02-29",
        "1970-01-01",
        "9999-12-31",
    ] {
        assert_eq!(Date::parse(text).unwrap().to_string(), text);
    }
    for text in [
        "1900-02-29",
        "2100-02-29",
        "2024-04-31",
        "2024-1-01",
        "2024-01-01T00:00:00Z",
        "+024-01-01",
        "２０２４-01-01",
    ] {
        assert!(Date::parse(text).is_none());
    }
    for year in [-1, 10000, i32::MAX] {
        assert!(Date::try_new(year, 1, 1).is_none());
    }
    assert_eq!(
        Date::try_new(2000, 2, 28).unwrap().checked_add_days(1),
        Date::try_new(2000, 2, 29)
    );
    assert!(Date::MAX.checked_add_days(1).is_none());
    assert!(Date::MIN.checked_sub_days(1).is_none());
    assert!(Date::EPOCH.checked_add_days(i64::MAX).is_none());
    assert!(Date::EPOCH.checked_sub_days(i64::MIN).is_none());
}

#[test]
fn exact_date_bytes_and_invalid_stored_domains() {
    assert_eq!(
        Schema::new(&[DateRow::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "425c91779bd76c7a39be8d48ac42ab05c3658385b4b3159ccd63a4c23cb08cc1"
    );
    for value in values() {
        let row = DateRow {
            id: 7,
            maybe: Some(value),
            value,
            zone: true,
        };
        assert_eq!(DateRow::decode(&row.encode().unwrap()).unwrap(), row);
    }
    let row = DateRow {
        id: 7,
        maybe: None,
        value: Date::try_new(1969, 12, 31).unwrap(),
        zone: true,
    };
    let golden = vec![7, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 1];
    assert_eq!(row.encode().unwrap(), golden);
    for end in 0..golden.len() {
        assert!(matches!(
            DateRow::decode(&golden[..end]),
            Err(Error::Corrupt)
        ));
    }
    let mut excess = golden.clone();
    excess.push(0);
    assert!(matches!(DateRow::decode(&excess), Err(Error::Corrupt)));
    for invalid in [i32::MIN, -719529, 2932897, i32::MAX] {
        let mut bad = golden.clone();
        bad[9..13].copy_from_slice(&invalid.to_le_bytes());
        assert!(matches!(DateRow::decode(&bad), Err(Error::Corrupt)));
    }
}

#[test]
fn ordered_indexes_swaps_noops_refusals_and_reopen() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[DateRow::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [9; 16]).unwrap();
    let mut rows: Vec<_> = values()
        .into_iter()
        .enumerate()
        .map(|(i, value)| DateRow {
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
    let read = |db: &Database| {
        db.index_prefix::<DateRow>("zone,value,id", &[true.into()], 32)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(read(&db), vec![1, 2, 3, 4, 5, 6]);
    let page = db
        .query::<DateRow>()
        .using_index("zone,value,id", &[true.into()])
        .gt(DateRow::VALUE, Date::MAX.checked_sub_days(1).unwrap())
        .lte(DateRow::VALUE, Date::MAX)
        .order_desc(DateRow::ID)
        .fetch()
        .unwrap();
    assert_eq!(page.rows, [rows[5].clone()]);
    assert_eq!(page.examined, 1);

    for (op, field, value) in [
        (
            ic_db::Comparison::Ge,
            DateRow::ZONE,
            IndexValue::from(false),
        ),
        (ic_db::Comparison::Le, DateRow::ZONE, true.into()),
        (ic_db::Comparison::Ge, DateRow::VALUE, Date::MIN.into()),
        (ic_db::Comparison::Le, DateRow::VALUE, Date::MAX.into()),
    ] {
        let count = db
            .query::<DateRow>()
            .filter(field, op, value)
            .work_limit(1)
            .count()
            .unwrap();
        assert_eq!((count.value, count.examined, count.fetched), (6, 0, 0));
    }
    for (excluded, expected) in [(false, Some(1)), (true, None)] {
        let page = db
            .query::<DateRow>()
            .ne(DateRow::ZONE, excluded)
            .order_by(&[DateRow::VALUE, DateRow::ID])
            .limit(1)
            .work_limit(1)
            .fetch()
            .unwrap();
        assert_eq!(page.rows.first().map(|row| row.id), expected);
        assert_eq!(page.examined, usize::from(expected.is_some()));
    }
    for pair in values().windows(2) {
        assert!(pair[0] < pair[1]);
        let key = |v: Date| ic_db::__private::encode_index(&[(v.into(), false)], false).unwrap();
        assert_eq!(
            key(pair[0]),
            ((pair[0].as_days_since_epoch() as u32) ^ 0x80000000).to_be_bytes()
        );
        assert!(key(pair[0]) < key(pair[1]));
    }
    assert!(matches!(
        db.index_prefix::<DateRow>("maybe", &[IndexValue::Nat128(0)], 32),
        Err(Error::InvalidOperand)
    ));
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    db.commit(b).unwrap();
    assert_eq!(*backing.borrow(), before);
    let duplicate = DateRow {
        maybe: Some(Date::MAX),
        ..rows[0].clone()
    };
    let mut b = Batch::new();
    b.replace(&duplicate).unwrap();
    assert!(matches!(db.commit(b), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    rows[0].maybe = Some(Date::MAX);
    rows[5].maybe = Some(Date::MIN);
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    b.replace(&rows[5]).unwrap();
    db.commit(b).unwrap();
    assert_eq!(
        db.index_prefix::<DateRow>("maybe", &[Date::MAX.into()], 32)
            .unwrap()[0]
            .id,
        1
    );
    drop(db);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert_eq!(read(&db), vec![1, 2, 3, 4, 5, 6]);
    for r in rows {
        assert_eq!(db.get::<DateRow>(r.id).unwrap(), Some(r));
    }
    drop(db);
    let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
    let index = DateRow::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "zone,value,id")
        .unwrap();
    let mut physical = vec![3];
    physical.extend(DateRow::SCHEMA.id.to_be_bytes());
    physical.extend(index.id.to_be_bytes());
    physical.push(1);
    physical.extend([128, 44, 192, 160]);
    physical.extend(6_u64.to_be_bytes());
    physical.extend(6_u64.to_be_bytes());
    assert!(raw.get(&physical).is_some());
    let mut short = physical.clone();
    short[10..14].copy_from_slice(&[255; 4]);
    raw.remove(&physical);
    raw.insert(short, 6_u64.to_be_bytes().to_vec());
    drop(raw);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert!(matches!(
        db.index_prefix::<DateRow>("zone,value,id", &[true.into()], 32),
        Err(Error::Corrupt)
    ));
}

#[test]
fn collections_reject_duplicate_keys_before_materialization() {
    let row = Collections {
        id: 1,
        list: vec![Date::MAX, Date::MIN, Date::MAX],
        map: Links(BTreeMap::from([
            (Date::MIN, None),
            (Date::MAX, Some(Date::MIN)),
        ])),
        set: Ids(BTreeSet::from([Date::MIN, Date::MAX])),
    };
    let bytes = row.encode().unwrap();
    assert_eq!(Collections::decode(&bytes).unwrap(), row);
    assert_eq!(&bytes[12..16], &2932896_i32.to_le_bytes());
    assert_eq!(&bytes[16..20], &(-719528_i32).to_le_bytes());
    // Map begins at 24: count, four-byte key, absent tag, second key.
    let mut duplicate = bytes;
    let first = duplicate[28..32].to_vec();
    duplicate[33..37].copy_from_slice(&first);
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
    // Stored inspection must also validate a date outside every index.
    let mut invalid_date = row.encode().unwrap();
    invalid_date[12..16].copy_from_slice(&i32::MAX.to_le_bytes());
    raw.insert(key, invalid_date);
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
        field(name = "values", value(many, max_items = 256, item(prim = "Date")))
    )
)]
struct WideList {}
#[test]
fn lists_obey_the_shared_item_bound() {
    let row = WideList {
        id: 1,
        values: vec![Date::MAX; 256],
    };
    let bytes = row.encode().unwrap();
    assert_eq!(bytes.len(), 1036);
    assert_eq!(WideList::decode(&bytes).unwrap(), row);
    let mut oversized = row;
    oversized.values.push(Date::MAX);
    assert!(matches!(oversized.encode(), Err(Error::Limit)));
}
