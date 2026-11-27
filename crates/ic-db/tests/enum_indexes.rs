//! Unit-enum index identity, independent bytes and final-state integrity.

#[path = "support/raw_map.rs"]
pub mod raw_map;
#[path = "support/host.rs"]
mod test_host;
use crate::raw_map::RawMap;
use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error, IndexValue};
use ic_storage::{StableKey, VectorMemory};
use test_host::host;

///
/// State
/// Distinguishes the `Ready`, `Archived` and `Draft` variants.
///

#[enum_(
    variant(name = "Ready"),
    variant(name = "Archived"),
    variant(name = "Draft")
)]
struct State {}

///
/// Reordered
/// Distinguishes the `Draft`, `Ready` and `Archived` variants.
///

#[enum_(
    name = "State",
    variant(name = "Draft"),
    variant(name = "Ready"),
    variant(name = "Archived")
)]
struct Reordered {}

///
/// Changed
/// Distinguishes the `Archived`, `Draft` and `Stopped` variants.
///

#[enum_(
    name = "State",
    variant(name = "Archived"),
    variant(name = "Draft"),
    variant(name = "Stopped")
)]
struct Changed {}

///
/// Other
/// Distinguishes the `Archived`, `Draft` and `Ready` variants.
///

#[enum_(
    variant(name = "Archived"),
    variant(name = "Draft"),
    variant(name = "Ready")
)]
struct Other {}

///
/// Payload
/// Distinguishes the `Archived` and `Draft` variants.
///

#[enum_(
    variant(name = "Archived"),
    variant(name = "Draft", value(item(prim = "Nat8")))
)]
struct Payload {}

///
/// Row
/// Exercises indexed access by `zone`, `state`, `id`, `maybe` and `boxed` on rows keyed by
/// `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["zone", "state", "id"]),
    index(field = "maybe", unique, present_only),
    index(field = "boxed"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "zone", value(item(prim = "Bool"))),
        field(name = "state", value(item(is = "State"))),
        field(name = "maybe", value(opt, item(is = "State"))),
        field(name = "boxed", value(opt, item(indirect, is = "State")))
    )
)]
struct Row {}

///
/// States
/// Stores up to 3 distinct `State` values.
///

#[set(item(is = "State"), max_items = 3)]
struct States {}

///
/// Permissions
/// Stores `allowed`, `wanted` and `unrelated` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(field = "unrelated"),
    index(field = "wanted"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "allowed", value(item(is = "States"))),
        field(name = "wanted", value(item(is = "State"))),
        field(name = "unrelated", value(item(is = "Other")))
    )
)]
struct Permissions {}

#[test]
fn enum_membership_checks_schema_identity_before_comparing_ordinals() {
    use ic_db::Comparison::{Contains, Eq, Ne};
    let runtime = host(VectorMemory::default());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = Schema::new(&[Permissions::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema, [8; 16]).unwrap();
    let mut batch = Batch::new();
    for (id, wanted) in [(1, State::Ready), (2, State::Draft)] {
        batch
            .insert(&Permissions {
                id,
                allowed: States([State::Ready].into()),
                wanted,
                unrelated: if id == 1 { Other::Draft } else { Other::Ready },
            })
            .unwrap();
    }
    db.commit(batch).unwrap();

    let query = || db.query::<Permissions>();
    // The first and last ordinals span the unrelated index. Ready narrows wanted to one row.
    let by_wanted = || {
        query()
            .gte(Permissions::WANTED, &State::Ready)
            .work_limit(2)
    };
    for (filtered, expected) in [
        (by_wanted().lte(Permissions::UNRELATED, &Other::Ready), 1),
        (
            by_wanted().filter_in(Permissions::UNRELATED, &[&Other::Archived, &Other::Ready]),
            0,
        ),
    ] {
        let count = filtered.count_field(Permissions::ID).unwrap();
        assert_eq!(
            (count.value, count.examined, count.fetched),
            (expected, 1, 1)
        );
    }
    let page = query()
        .filter_fields("allowed", Contains, "wanted")
        .fetch()
        .unwrap();
    assert_eq!(page.rows.iter().map(|row| row.id).collect::<Vec<_>>(), [1]);
    assert!(matches!(
        query()
            .filter_fields("allowed", Contains, "unrelated")
            .count(),
        Err(Error::InvalidQuery)
    ));
    // Same logical enum under a reordered Rust declaration.
    for (comparison, expected) in [(Eq, 1), (Ne, 2)] {
        let rows = query()
            .filter(Permissions::WANTED, comparison, &Reordered::Ready)
            .fetch()
            .unwrap()
            .rows;
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            [expected]
        );
    }
    assert_eq!(
        query()
            .filter("allowed", Contains, &Reordered::Ready)
            .count()
            .unwrap()
            .value,
        2
    );
    // Archived has ordinal zero in all three definitions. Its identity still matters.
    for invalid in [
        IndexValue::from(&Other::Archived),
        (&Changed::Archived).into(),
        0_u8.into(),
    ] {
        assert!(matches!(
            query().filter(Permissions::WANTED, Eq, invalid).fetch(),
            Err(Error::InvalidOperand)
        ));
        assert!(matches!(
            query().filter("allowed", Contains, invalid).count(),
            Err(Error::InvalidOperand)
        ));
    }
}

fn row(id: u64, state: State) -> Row {
    Row {
        id,
        zone: true,
        state,
        maybe: Some(state),
        boxed: Some(Box::new(state)),
    }
}

fn schema() -> Schema {
    Schema::new(&[Row::SCHEMA]).unwrap()
}

#[test]
fn enum_keys_preserve_identity_order_uniqueness_and_reopen() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [13; 16]).unwrap();
    let mut rows = vec![
        row(3, State::Archived),
        row(2, State::Draft),
        row(1, State::Ready),
    ];
    let mut b = Batch::new();
    for r in rows.iter().rev() {
        b.insert(r).unwrap();
    }
    db.commit(b).unwrap();
    assert_eq!(
        db.index_prefix::<Row>("zone,state,id", &[true.into()], 32)
            .unwrap(),
        rows
    );
    // Same complete declared contract, independent Rust declaration order.
    assert_eq!(
        db.index_prefix::<Row>("maybe", &[(&Reordered::Draft).into()], 32)
            .unwrap(),
        [rows[1].clone()]
    );
    for value in [
        (&Other::Draft).into(),
        (&Changed::Draft).into(),
        1_u8.into(),
        IndexValue::Absent,
        IndexValue::UnitEnum {
            definition: State::definition(),
            ordinal: 3,
        },
        IndexValue::UnitEnum {
            definition: Payload::definition(),
            ordinal: 0,
        },
    ] {
        assert!(matches!(
            db.index_prefix::<Row>("maybe", &[value], 32),
            Err(Error::InvalidOperand)
        ));
    }
    let before = backing.borrow().clone();
    let mut duplicate = rows[0].clone();
    duplicate.maybe = rows[1].maybe;
    let mut b = Batch::new();
    b.replace(&duplicate).unwrap();
    assert!(matches!(db.commit(b), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    // Final-state uniqueness permits exchanges without a durable overlay.
    rows[0].maybe = Some(State::Ready);
    rows[2].maybe = Some(State::Archived);
    rows[1].boxed = None;
    let mut b = Batch::new();
    for r in &rows {
        b.replace(r).unwrap();
    }
    db.commit(b).unwrap();
    assert_eq!(
        db.index_prefix::<Row>("boxed", &[IndexValue::Absent], 32)
            .unwrap(),
        [rows[1].clone()]
    );
    assert_eq!(
        db.index_prefix::<Row>("boxed", &[(&State::Archived).into()], 32)
            .unwrap(),
        [rows[0].clone()]
    );
    // None through Ready spans the whole boxed index. The maybe index visits
    // just Draft and Ready; only the row with absent boxed state survives.
    let count = db
        .query::<Row>()
        .filter_in(Row::BOXED, &[None, Some(&State::Ready)])
        .gte(Row::MAYBE, &State::Draft)
        .work_limit(3)
        .count_field(Row::ID)
        .unwrap();
    assert_eq!((count.value, count.examined, count.fetched), (1, 2, 2));
    // Starting at the first present variant skips the absent key.
    let count = db
        .query::<Row>()
        .filter_in(Row::BOXED, &[&State::Archived, &State::Ready])
        .gte(Row::MAYBE, &State::Archived)
        .work_limit(3)
        .count_field(Row::ID)
        .unwrap();
    assert_eq!((count.value, count.examined, count.fetched), (2, 2, 2));
    drop(db);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert_eq!(
        db.index_prefix::<Row>("zone,state,id", &[true.into()], 32)
            .unwrap(),
        rows
    );
    drop(db);
    let raw = RawMap::load(runtime.open_memory(&slot).unwrap());
    let index = Row::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "zone,state,id")
        .unwrap();
    for (ordinal, r) in rows.iter().enumerate() {
        let mut key = vec![3];
        key.extend(Row::SCHEMA.id.to_be_bytes());
        key.extend(index.id.to_be_bytes());
        key.extend([1, ordinal as u8]);
        key.extend(r.id.to_be_bytes());
        key.extend(r.id.to_be_bytes());
        assert_eq!(raw.get(&key), Some(r.id.to_be_bytes().to_vec()));
    }
}

#[test]
fn enum_bounds_fix_the_ordered_index_prefix() {
    use ic_db::Comparison::{Ge, Gt, Le, Lt};
    let runtime = host(VectorMemory::default());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [13; 16]).unwrap();
    let mut batch = Batch::new();
    for (id, state) in [
        State::Ready,
        State::Archived,
        State::Draft,
        State::Ready,
        State::Archived,
        State::Draft,
    ]
    .into_iter()
    .enumerate()
    {
        let mut record = row(id as u64, state);
        record.maybe = (id < 3).then_some(state);
        batch.insert(&record).unwrap();
    }
    db.commit(batch).unwrap();
    let zone = [true.into()];
    for (lower, upper, expected) in [
        ((Le, State::Archived), None, [4, 1]),
        ((Ge, State::Ready), None, [3, 0]),
        ((Gt, State::Archived), Some((Lt, State::Ready)), [5, 2]),
        ((Ge, State::Draft), Some((Lt, State::Ready)), [5, 2]),
        ((Gt, State::Archived), Some((Le, State::Draft)), [5, 2]),
    ] {
        for hinted in [false, true] {
            let query = || {
                let mut q = db
                    .query::<Row>()
                    .eq(Row::ZONE, true)
                    .filter(Row::STATE, lower.0, &lower.1)
                    .order_desc(Row::ID)
                    .limit(1)
                    .work_limit(1);
                if let Some((op, state)) = &upper {
                    q = q.filter(Row::STATE, *op, state);
                }
                if hinted {
                    q.using_index("zone,state,id", &zone)
                } else {
                    q
                }
            };
            let first = query().fetch().unwrap();
            assert_eq!(first.rows[0].id, expected[0]);
            let second = query().after(&first.next.unwrap()).fetch().unwrap();
            assert_eq!(second.rows[0].id, expected[1]);
        }
    }
    for field in [Row::STATE, Row::MAYBE] {
        for query in [
            db.query::<Row>().lt(field, &State::Archived),
            db.query::<Row>().gt(field, &State::Ready),
            db.query::<Row>()
                .gt(field, &State::Archived)
                .lt(field, &State::Draft)
                .order_desc(Row::ID),
            db.query::<Row>()
                .lt(field, &State::Ready)
                .gt(field, &State::Draft)
                .order_desc(Row::ID),
        ] {
            let page = query.work_limit(1).fetch().unwrap();
            assert_eq!(page.examined, 0);
            assert!(page.rows.is_empty() && page.next.is_none());
        }
    }
    let present = db
        .query::<Row>()
        .lte(Row::MAYBE, &State::Archived)
        .fetch()
        .unwrap();
    assert_eq!(present.rows.iter().map(|r| r.id).collect::<Vec<_>>(), [1]);
    assert!(matches!(
        db.query::<Row>().gte(Row::STATE, &Other::Ready).fetch(),
        Err(Error::InvalidOperand)
    ));
    // Both Archived and Draft qualify, so this traversal cannot supply ID order.
    assert!(matches!(
        db.query::<Row>()
            .using_index("zone,state,id", &[])
            .eq(Row::ZONE, true)
            .lte(Row::STATE, &State::Draft)
            .order_desc(Row::ID)
            .fetch(),
        Err(Error::InvalidQuery)
    ));
}

#[test]
fn sparse_membership_and_corrupt_ordinals_refuse_without_mutation() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [14; 16]).unwrap();
    let mut r = row(1, State::Archived);
    r.maybe = None;
    r.boxed = None;
    let mut second = r.clone();
    second.id = 2;
    let mut b = Batch::new();
    b.insert(&r).unwrap();
    b.insert(&second).unwrap();
    db.commit(b).unwrap();
    assert!(
        db.index_prefix::<Row>("maybe", &[(&State::Archived).into()], 32)
            .unwrap()
            .is_empty()
    );
    // Alphabetical fields: boxed, id, maybe, state, zone.
    let golden = [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    assert_eq!(r.encode().unwrap(), golden);
    drop(db);
    let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
    let mut key = vec![2];
    key.extend(Row::SCHEMA.id.to_be_bytes());
    key.extend(1_u64.to_be_bytes());
    let mut bad = golden.to_vec();
    bad[10] = 3;
    raw.insert(key, bad);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(db.get::<Row>(1), Err(Error::Corrupt)));
    let mut b = Batch::new();
    b.delete::<Row>(1).unwrap();
    assert!(matches!(db.commit(b), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn complete_definition_controls_index_admission() {
    for definition in [Payload::definition(), Other::definition()] {
        let mut fields = Row::SCHEMA.fields.to_vec();
        let f = fields.iter_mut().find(|f| f.name == "state").unwrap();
        f.value = ic_db::FieldType::Named(if definition.name == "Payload" {
            ic_db::TypeLink {
                name: Payload::NAME,
                definition: Payload::definition,
            }
        } else {
            ic_db::TypeLink {
                name: Other::NAME,
                definition: Other::definition,
            }
        });
        let changed = Box::leak(Box::new(ic_db::EntitySchema {
            fields: Box::leak(fields.into_boxed_slice()),
            ..*Row::SCHEMA
        }));

        if definition.name == "Payload" {
            assert!(matches!(Schema::new(&[changed]), Err(Error::InvalidSchema)));
        } else {
            assert_ne!(
                Schema::new(&[changed]).unwrap().fingerprint(),
                schema().fingerprint()
            );
        }
    }
}

#[test]
fn malformed_or_wrong_owner_index_entries_refuse() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [15; 16]).unwrap();
    let r = row(1, State::Archived);
    let mut b = Batch::new();
    b.insert(&r).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let index = Row::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "zone,state,id")
        .unwrap();
    let mut prefix = vec![3];
    prefix.extend(Row::SCHEMA.id.to_be_bytes());
    prefix.extend(index.id.to_be_bytes());
    prefix.push(1);
    let mut original = prefix.clone();
    original.push(0);
    original.extend(1_u64.to_be_bytes());
    original.extend(1_u64.to_be_bytes());
    let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
    assert!(raw.remove(&original).is_some());
    drop(raw);
    // Invalid ordinal, valid but incorrect ordinal, truncated component and wrong owner.
    for (component, owner) in [(vec![3], 1_u64), (vec![1], 1), (vec![], 1), (vec![0], 2)] {
        let mut bad = prefix.clone();
        bad.extend(component);
        bad.extend(1_u64.to_be_bytes());
        bad.extend(1_u64.to_be_bytes());
        let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
        raw.insert(bad.clone(), owner.to_be_bytes().to_vec());
        drop(raw);
        let before = backing.borrow().clone();
        let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
        assert!(matches!(
            db.index_prefix::<Row>("zone,state,id", &[true.into()], 32),
            Err(Error::Corrupt)
        ));
        assert_eq!(*backing.borrow(), before);
        drop(db);
        let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
        raw.remove(&bad);
    }
}
