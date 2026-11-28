//! Exact millisecond values across storage, indexes, collections and reopen.

#[path = "support/allocations.rs"]
mod allocations;
#[path = "support/fingerprint.rs"]
mod fingerprint;
#[path = "support/raw_map.rs"]
pub mod raw_map;
use crate::raw_map::RawMap;
use ic_db::design::prelude::*;
use ic_db::{AggregateJobs, Batch, Database, Error, IndexValue, JobOutcome};
use ic_storage::{MemoryRuntime, StableKey, VectorMemory};
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
    allocations::host_with(backing, &["test.db.v1", "test.jobs.v1"])
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
    fingerprint::check(
        Schema::new(&[TimeRow::SCHEMA]).unwrap().fingerprint(),
        "49eac412fcdf225a26d5d5df7ae277cb3c19fe10f8d0b9e3e932a92aecd467c8",
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
    let key = |value| {
        ic_db::__private::encode_index(
            &[ic_db::__private::IndexComponent {
                value,
                optional: false,
            }],
            false,
        )
        .unwrap()
    };
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
    let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
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
    let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
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

/// Elapsed delay projected in milliseconds.
#[record(fields(field(name = "delay", value(opt, item(prim = "Duration")))))]
struct Delay {}

/// Partition of elapsed-time observations.
#[record(fields(field(name = "zone", value(item(prim = "Bool")))))]
struct Zone {}

/// Same field name and width as Delay, with different units.
#[record(fields(field(name = "delay", value(opt, item(prim = "Nat64")))))]
struct RawDelay {}

#[test]
fn duration_totals_round_once_and_survive_job_reopen() {
    for (left, right, sum, average) in [
        (0, 1, Some(1), 0),
        (1, 2, Some(3), 2),
        (u64::MAX - 1, u64::MAX, None, u64::MAX - 1),
    ] {
        let runtime = host(VectorMemory::default());
        let slot = StableKey::parse("test.db.v1").unwrap();
        let job_slot = StableKey::parse("test.jobs.v1").unwrap();
        let schema = Schema::new(&[TimeRow::SCHEMA]).unwrap();
        let mut db =
            Database::create(runtime.open_memory(&slot).unwrap(), schema, [1; 16]).unwrap();
        let mut jobs =
            AggregateJobs::create(runtime.open_memory(&job_slot).unwrap(), [2; 16]).unwrap();
        let mut batch = Batch::new();
        for (row, delay) in rows().into_iter().zip([Some(left), None, Some(right)]) {
            batch
                .insert(&TimeRow {
                    delay: delay.map(Duration::from_millis),
                    ..row
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
        let query = || db.query::<TimeRow>();
        let actual_sum = query()
            .sum::<Delay>()
            .map(|a| a.value.unwrap().delay.unwrap().as_millis());
        match sum {
            Some(value) => assert_eq!(actual_sum.unwrap(), value),
            None => assert!(matches!(actual_sum, Err(Error::ArithmeticOverflow))),
        }
        let expected = Some(Duration::from_millis(average));
        assert_eq!(
            query().avg::<Delay>().unwrap().value.unwrap().delay,
            expected
        );

        let groups = query().group_avg::<Zone, Delay>().unwrap().value;
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].value.as_ref().unwrap().delay, expected);
        assert!(matches!(
            query().sum::<RawDelay>(),
            Err(Error::InvalidQuery)
        ));

        let now = Timestamp::EPOCH;
        let handle = query()
            .start_avg::<Delay>(&mut jobs, now, Timestamp::from_millis(100))
            .unwrap();
        let first = query()
            .work_limit(1)
            .resume_avg::<Delay>(&mut jobs, &handle, 0, now)
            .unwrap();
        assert!(matches!(first.outcome, JobOutcome::Running));
        drop(jobs);
        let mut jobs = AggregateJobs::open(runtime.open_memory(&job_slot).unwrap()).unwrap();
        let step = query()
            .resume_avg::<Delay>(&mut jobs, &handle, first.sequence, now)
            .unwrap();
        let JobOutcome::Complete(value) = step.outcome else {
            panic!("expected completed average")
        };
        assert_eq!(value.unwrap().delay, expected);
    }
}

#[test]
fn utc_calendar_day_uses_timestamp_index_bounds() {
    let fixture = ic_db::testing::Fixture::new("test.day.v1");
    let mut db = fixture
        .create(Schema::new(&[TimeRow::SCHEMA]).unwrap(), [1; 16])
        .unwrap();
    for (date, start) in [
        (Date::MIN, -62_167_219_200_000_i64),
        (Date::parse("1969-12-31").unwrap(), -86_400_000),
        (Date::parse("2024-02-29").unwrap(), 1_709_164_800_000),
        (Date::MAX, 253_402_214_400_000),
    ] {
        let day = date.utc_timestamp_range();
        let end = start + 86_400_000;
        assert_eq!((day.start.as_millis(), day.end.as_millis()), (start, end));
        let mut batch = Batch::new();
        for (id, instant) in [Some(start - 1), Some(start), Some(end - 1), Some(end), None]
            .into_iter()
            .enumerate()
        {
            let row = TimeRow {
                id: id as u64,
                zone: true,
                delay: None,
                duration: Duration::ZERO,
                timestamp: Timestamp::EPOCH,
                maybe: instant.map(Timestamp::from_millis),
            };
            batch.upsert(&row).unwrap();
        }
        db.commit(batch).unwrap();
        let page = db
            .query::<TimeRow>()
            .filter_range(TimeRow::MAYBE, day.clone())
            .order_asc(TimeRow::MAYBE)
            .work_limit(3)
            .fetch()
            .unwrap();
        assert_eq!(page.rows.iter().map(|r| r.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(page.examined, 2);
        assert!(page.next.is_none());

        let first = db
            .query::<TimeRow>()
            .gte(TimeRow::MAYBE, day.start)
            .lt(TimeRow::MAYBE, day.end)
            .order_asc(TimeRow::MAYBE)
            .limit(1)
            .fetch()
            .unwrap();
        assert_eq!(first.rows[0].id, 1);
        let second = db
            .query::<TimeRow>()
            .filter_range(TimeRow::MAYBE, day)
            .order_asc(TimeRow::MAYBE)
            .limit(1)
            .after(first.next.as_ref().unwrap())
            .fetch()
            .unwrap();
        assert_eq!(second.rows[0].id, 2);
    }
}

#[test]
fn one_millisecond_ranges_fix_compound_order_and_open_gaps_skip_reads() {
    let fixture = ic_db::testing::Fixture::new("test.tick.v1");
    let mut db = fixture
        .create(Schema::new(&[TimeRow::SCHEMA]).unwrap(), [1; 16])
        .unwrap();
    let mut batch = Batch::new();
    for row in rows() {
        batch.insert(&row).unwrap();
    }
    db.commit(batch).unwrap();

    for (field, index, low, high, expected) in [
        (
            TimeRow::TIMESTAMP,
            "zone,timestamp,id",
            Timestamp::EPOCH.into(),
            Timestamp::from_millis(1).into(),
            [6, 3],
        ),
        (
            TimeRow::DURATION,
            "zone,duration,id",
            IndexValue::from(Duration::ZERO),
            Duration::from_millis(1).into(),
            [6, 1],
        ),
    ] {
        let page = db
            .query::<TimeRow>()
            .using_index(index, &[true.into()])
            .filter_range(field, low..high)
            .order_desc(TimeRow::ID)
            .fetch()
            .unwrap();
        assert_eq!(page.rows.iter().map(|r| r.id).collect::<Vec<_>>(), expected);
        assert_eq!(page.examined, 2);
        let empty = db
            .query::<TimeRow>()
            .gt(field, low)
            .lt(field, high)
            .work_limit(1)
            .count()
            .unwrap();
        assert_eq!((empty.value, empty.examined, empty.fetched), (0, 0, 0));
    }
}

#[test]
fn time_extremes_count_required_rows_but_exclude_absent_optional_fields() {
    use ic_db::Comparison::{Ge, Gt, Le, Lt};
    let fixture = ic_db::testing::Fixture::new("test.time.v1");
    let mut db = fixture
        .create(Schema::new(&[TimeRow::SCHEMA]).unwrap(), [1; 16])
        .unwrap();
    let mut batch = Batch::new();
    for row in rows() {
        batch.insert(&row).unwrap();
    }
    db.commit(batch).unwrap();
    for (required, optional, low, high) in [
        (
            TimeRow::TIMESTAMP,
            TimeRow::MAYBE,
            IndexValue::from(Timestamp::MIN),
            Timestamp::MAX.into(),
        ),
        (
            TimeRow::DURATION,
            TimeRow::DELAY,
            Duration::MIN.into(),
            Duration::MAX.into(),
        ),
    ] {
        for (op, bound) in [(Ge, low), (Le, high)] {
            let all = db
                .query::<TimeRow>()
                .filter(required, op, bound)
                .work_limit(1)
                .count()
                .unwrap();
            assert_eq!((all.value, all.examined, all.fetched), (6, 0, 0));
            assert_eq!(
                db.query::<TimeRow>()
                    .filter(optional, op, bound)
                    .count()
                    .unwrap()
                    .value,
                5
            );
        }
        for (op, bound) in [(Lt, low), (Gt, high)] {
            let none = db
                .query::<TimeRow>()
                .filter(optional, op, bound)
                .work_limit(1)
                .count()
                .unwrap();
            assert_eq!((none.value, none.examined, none.fetched), (0, 0, 0));
        }
    }
}
