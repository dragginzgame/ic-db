//! Exact millisecond values across storage, indexes, collections and reopen.

use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error, IndexValue};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};
use std::collections::{BTreeMap, BTreeSet};

///
/// TimeRow
/// Exercises indexed access by `zone`, `duration`, `id`, `timestamp`, `delay` and `maybe` on
/// rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["zone", "duration", "id"]),
    index(fields = ["zone", "timestamp", "id"]),
    index(field = "delay", unique, present_only),
    index(field = "maybe", unique, present_only),
    fields(
        field(name = "delay", value(opt, item(prim = "Duration"))),
        field(name = "duration", value(item(prim = "Duration"))),
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "maybe", value(opt, item(prim = "Timestamp"))),
        field(name = "timestamp", value(item(prim = "Timestamp"))),
        field(name = "zone", value(item(prim = "Bool")))
    )
)]
pub(crate) struct TimeRow {}

///
/// Moments
/// Stores up to 3 distinct `Timestamp` values.
///

#[set(item(prim = "Timestamp"), max_items = 3)]
struct Moments {}

///
/// Deadlines
/// Maps up to 3 `Duration` keys to optional `Timestamp` values.
///

#[map(
    key(prim = "Duration"),
    value(opt, item(prim = "Timestamp")),
    max_items = 3
)]
struct Deadlines {}

///
/// Collections
/// Stores `list`, `map` and `set` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "list", value(many, max_items = 3, item(prim = "Duration"))),
        field(name = "map", value(item(is = "Deadlines"))),
        field(name = "set", value(item(is = "Moments")))
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

fn rows() -> Vec<TimeRow> {
    let mut rows: Vec<_> = [
        (0, i64::MIN),
        (1, -1),
        (i64::MAX as u64, 0),
        (1_u64 << 63, 1),
        (u64::MAX, i64::MAX),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, (d, t))| TimeRow {
        zone: true,
        id: n as u64 + 1,
        delay: Some(Duration::from_millis(d)),
        duration: Duration::from_millis(d),
        maybe: Some(Timestamp::from_millis(t)),
        timestamp: Timestamp::from_millis(t),
    })
    .collect();
    rows.push(TimeRow {
        zone: true,
        id: 6,
        delay: None,
        duration: Duration::ZERO,
        maybe: None,
        timestamp: Timestamp::EPOCH,
    });

    rows
}

#[test]
fn independent_bytes_full_domains_and_absence() {
    assert_eq!(
        Schema::new(&[TimeRow::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "747450a00cd3026b5fc4bd3154ba484ed7c3240b314de61ba6069df85e6a9d0b"
    );
    for row in rows() {
        let mut bytes = vec![u8::from(row.delay.is_some())];
        if let Some(d) = row.delay {
            bytes.extend(d.as_millis().to_le_bytes());
        }
        bytes.extend(row.duration.as_millis().to_le_bytes());
        bytes.extend(row.id.to_le_bytes());
        bytes.push(u8::from(row.maybe.is_some()));
        if let Some(t) = row.maybe {
            bytes.extend(t.as_millis().to_le_bytes());
        }
        bytes.extend(row.timestamp.as_millis().to_le_bytes());
        bytes.push(1);
        assert_eq!(row.encode().unwrap(), bytes);
        assert_eq!(TimeRow::decode(&bytes).unwrap(), row);
        for end in 0..bytes.len() {
            assert!(matches!(
                TimeRow::decode(&bytes[..end]),
                Err(Error::Corrupt)
            ));
        }
        let mut excess = bytes.clone();
        excess.push(0);
        assert!(matches!(TimeRow::decode(&excess), Err(Error::Corrupt)));
        let mut bad = bytes;
        bad[0] = 2;
        assert!(matches!(TimeRow::decode(&bad), Err(Error::Corrupt)));
    }
    let row = rows().pop().unwrap();
    let present = TimeRow {
        zone: true,
        delay: Some(Duration::ZERO),
        maybe: Some(Timestamp::EPOCH),
        ..row
    };
    assert_ne!(row.encode().unwrap(), present.encode().unwrap());
    // Exact operand bytes: full unsigned duration; timestamp sign is biased.
    let key = |value| ic_db::__private::encode_index(&[(value, false)], false).unwrap();
    assert_eq!(key(Duration::MAX.into()), [255; 8]);
    assert_eq!(key(Timestamp::MIN.into()), [0; 8]);
    assert_eq!(key(Timestamp::EPOCH.into()), [128, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(key(Timestamp::MAX.into()), [255; 8]);
    for pair in rows()[..5].windows(2) {
        assert!(pair[0].duration < pair[1].duration);
        assert!(pair[0].timestamp < pair[1].timestamp);
        assert!(key(pair[0].duration.into()) < key(pair[1].duration.into()));
        assert!(key(pair[0].timestamp.into()) < key(pair[1].timestamp.into()));
    }
}

#[test]
fn exact_indexes_final_state_swaps_and_reopen() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[TimeRow::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [10; 16]).unwrap();
    let mut rows = rows();
    let mut b = Batch::new();
    for row in rows.iter().rev() {
        b.insert(row).unwrap();
    }
    db.commit(b).unwrap();
    let read = |db: &Database, index| {
        db.index_prefix::<TimeRow>(index, &[true.into()], 32)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(read(&db, "zone,duration,id"), vec![1, 6, 2, 3, 4, 5]);
    assert_eq!(read(&db, "zone,timestamp,id"), vec![1, 2, 3, 6, 4, 5]);
    for (index, value) in [
        ("delay", IndexValue::Nat64(0)),
        ("maybe", IndexValue::Int64(0)),
        ("maybe", Duration::ZERO.into()),
    ] {
        assert!(matches!(
            db.index_prefix::<TimeRow>(index, &[value], 32),
            Err(Error::InvalidOperand)
        ));
    }
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    db.commit(b).unwrap();
    assert_eq!(*backing.borrow(), before);
    for duplicate in [
        TimeRow {
            zone: true,
            delay: Some(Duration::MAX),
            ..rows[0].clone()
        },
        TimeRow {
            zone: true,
            maybe: Some(Timestamp::MAX),
            ..rows[0].clone()
        },
    ] {
        let mut b = Batch::new();
        b.replace(&duplicate).unwrap();
        assert!(matches!(db.commit(b), Err(Error::UniqueViolation)));
        assert_eq!(*backing.borrow(), before);
    }
    rows[0].delay = Some(Duration::MAX);
    rows[0].maybe = Some(Timestamp::MAX);
    rows[4].delay = Some(Duration::ZERO);
    rows[4].maybe = Some(Timestamp::MIN);
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    b.replace(&rows[4]).unwrap();
    db.commit(b).unwrap();
    for (index, value) in [
        ("delay", Duration::MAX.into()),
        ("maybe", Timestamp::MAX.into()),
    ] {
        assert_eq!(
            db.index_prefix::<TimeRow>(index, &[value], 32).unwrap()[0].id,
            1
        );
    }
    drop(db);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    for row in rows {
        assert_eq!(db.get::<TimeRow>(row.id).unwrap(), Some(row));
    }
    assert_eq!(read(&db, "zone,duration,id"), vec![1, 6, 2, 3, 4, 5]);
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    let index = TimeRow::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "zone,timestamp,id")
        .unwrap();
    let mut physical = vec![3];
    physical.extend(TimeRow::SCHEMA.id.to_be_bytes());
    physical.extend(index.id.to_be_bytes());
    physical.push(1);
    physical.extend([0; 8]);
    physical.extend(1_u64.to_be_bytes());
    physical.extend(1_u64.to_be_bytes());
    assert!(raw.get(&physical).is_some());
    let mut short = physical.clone();
    short.remove(10);
    raw.remove(&physical);
    raw.insert(short, 1_u64.to_be_bytes().to_vec());
    drop(raw);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert!(matches!(
        db.index_prefix::<TimeRow>("zone,timestamp,id", &[true.into()], 32),
        Err(Error::Corrupt)
    ));
}

#[test]
fn collection_order_and_unindexed_validation() {
    let row = Collections {
        id: 1,
        list: vec![Duration::MAX, Duration::ZERO, Duration::MAX],
        map: Deadlines(BTreeMap::from([
            (Duration::ZERO, None),
            (Duration::MAX, Some(Timestamp::MIN)),
        ])),
        set: Moments(BTreeSet::from([Timestamp::MAX, Timestamp::MIN])),
    };
    let bytes = row.encode().unwrap();
    assert_eq!(Collections::decode(&bytes).unwrap(), row);
    assert_eq!(&bytes[12..20], &[255; 8]);
    // Map count at 36, first eight-byte key at 40, absent tag, second key at 49.
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
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [10; 16]).unwrap();
    let mut b = Batch::new();
    b.insert(&row).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    let mut key = vec![2];
    key.extend(Collections::SCHEMA.id.to_be_bytes());
    key.extend(1_u64.to_be_bytes());
    let mut truncated = bytes;
    truncated.pop();
    raw.insert(key, truncated);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.delete::<Collections>(1).unwrap();
    assert!(matches!(db.commit(b), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn current_timestamp_uses_unix_milliseconds() {
    let before = std::time::SystemTime::now();
    let actual = Timestamp::now().as_millis();
    let after = std::time::SystemTime::now();
    let millis = |instant: std::time::SystemTime| {
        i64::try_from(
            instant
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
    };
    assert!((millis(before)..=millis(after)).contains(&actual));
}
