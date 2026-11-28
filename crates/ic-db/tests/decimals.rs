//! Exact decimal invariants, with independent widened-integer and byte oracles.

#[path = "support/fingerprint.rs"]
mod fingerprint;
#[path = "support/raw_map.rs"]
pub mod raw_map;
#[path = "support/host.rs"]
mod test_host;
use crate::raw_map::RawMap;
use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error, IndexValue};
use ic_storage::{StableKey, VectorMemory};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    str::FromStr,
};
use test_host::host;

///
/// DecimalRow
/// Exercises indexed access by `zone`, `value`, `id` and `maybe` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["zone", "value", "id"]),
    index(field = "maybe", unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "maybe", value(opt, item(prim = "Decimal", scale = 2))),
        field(name = "value", value(item(prim = "Decimal", scale = 2))),
        field(name = "zone", value(item(prim = "Bool")))
    )
)]
pub(crate) struct DecimalRow {}

///
/// Prices
/// Stores up to 3 distinct `Decimal` values.
///

#[set(item(prim = "Decimal", scale = 2), max_items = 3)]
struct Prices {}

///
/// Flags
/// Maps up to 3 `Decimal` keys to `Bool` values.
///

#[map(
    key(prim = "Decimal", scale = 2),
    value(item(prim = "Bool")),
    max_items = 3
)]
struct Flags {}

///
/// Collections
/// Stores `list`, `map` and `set` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "list",
            value(many, max_items = 3, item(prim = "Decimal", scale = 2))
        ),
        field(name = "map", value(item(is = "Flags"))),
        field(name = "set", value(item(is = "Prices")))
    )
)]
struct Collections {}
fn d(value: i128, scale: u32) -> Decimal {
    Decimal::try_new(value, scale).unwrap()
}

fn row(id: u64, coefficient: i128) -> DecimalRow {
    DecimalRow {
        id,
        maybe: Some(d(coefficient, 2)),
        value: d(coefficient, 2),
        zone: true,
    }
}

fn bytes(row: &DecimalRow) -> Vec<u8> {
    let mut b = row.id.to_le_bytes().to_vec();
    b.push(u8::from(row.maybe.is_some()));
    if let Some(v) = row.maybe {
        b.extend(v.coefficient_at_scale(2).unwrap().to_be_bytes());
        b.push(2);
    }
    b.extend(row.value.coefficient_at_scale(2).unwrap().to_be_bytes());
    b.extend([2, 1]);

    b
}

#[test]
fn exact_value_text_hash_and_order() {
    let one = d(1, 0);
    assert_eq!(HashSet::from([one, d(10, 1), d(100, 2)]).len(), 1);
    assert_eq!(BTreeSet::from([one, d(10, 1), d(100, 2)]).len(), 1);
    assert_eq!(d(0, 28), Decimal::ZERO);
    assert_eq!(Decimal::try_new(10, 29), Some(d(1, 28)));
    assert_eq!(Decimal::try_new(1, 29), None);
    for (s, canonical) in [
        (" +001.00 ", "1"),
        ("-.5", "-0.5"),
        ("1.", "1"),
        ("-0.00", "0"),
        (
            "0.0000000000000000000000000001",
            "0.0000000000000000000000000001",
        ),
    ] {
        assert_eq!(Decimal::from_str(s).unwrap().to_string(), canonical);
    }
    for s in [
        "",
        ".",
        "-",
        "1e2",
        "1..2",
        "1 2",
        "0.00000000000000000000000000001",
        "170141183460469231731687303715884105728",
    ] {
        assert!(s.parse::<Decimal>().is_err(), "{s}");
    }
    for v in [
        Decimal::MIN,
        Decimal::MAX,
        d(i128::MIN, 28),
        d(i128::MAX, 28),
    ] {
        assert_eq!(v.to_string().parse::<Decimal>().unwrap(), v);
    }
    assert_eq!(one.coefficient_at_scale(2), Some(100));
    assert_eq!(d(101, 3).coefficient_at_scale(2), None);
    assert_eq!(Decimal::MAX.coefficient_at_scale(2), None);
    assert_eq!(one.coefficient_at_scale(29), None);
    // Independent I256 alignment, including the i128 overflow fallback and
    // IcyDB's high-mantissa regression family. No production comparator reuse.
    let values: Vec<_> = [
        i128::MIN,
        i128::MIN + 1,
        -1234567890123456789,
        -100,
        -11,
        -10,
        -1,
        0,
        1,
        10,
        11,
        100,
        1234567890123456789,
        i128::MAX - 1,
        i128::MAX,
    ]
    .into_iter()
    .flat_map(|m| [0, 1, 2, 18, 28].map(|s| d(m, s)))
    .collect();
    let key = |v: Decimal| {
        ic_db::__private::encode_index(
            &[ic_db::__private::IndexComponent {
                value: v.into(),
                optional: false,
            }],
            false,
        )
        .unwrap()
    };
    for &a in &values {
        for &b in &values {
            let wide = |v: Decimal| {
                ethnum::I256::from(v.mantissa()) * ethnum::I256::from(10_i128.pow(28 - v.scale()))
            };
            let expected = wide(a).cmp(&wide(b));
            assert_eq!(a.cmp(&b), expected, "{a}, {b}");
            assert_eq!(key(a).cmp(&key(b)), expected, "{a}, {b}");
        }
    }
    assert_eq!(key(one), vec![2, 128, 0, 0, 0, b'1', 0]);
    assert_eq!(key(d(-1, 0)), vec![0, 127, 255, 255, 255, !b'1', 255]);
    assert_eq!(key(Decimal::ZERO), vec![1]);
}

#[test]
fn fixed_scale_bytes_and_corruption() {
    for coefficient in [i128::MIN, -100, -1, 0, 1, 100, i128::MAX] {
        let row = row(1, coefficient);
        let mut b = 1_u64.to_le_bytes().to_vec();
        b.push(1);
        b.extend(coefficient.to_be_bytes());
        b.push(2);
        b.extend(coefficient.to_be_bytes());
        b.extend([2, 1]);
        assert_eq!(row.encode().unwrap(), b);
        assert_eq!(DecimalRow::decode(&b).unwrap(), row);
        for end in 0..b.len() {
            assert!(matches!(DecimalRow::decode(&b[..end]), Err(Error::Corrupt)));
        }
        for scale in [0, 1, 3, 28, 29, 255] {
            let mut corrupt = b.clone();
            corrupt[25] = scale;
            assert!(matches!(DecimalRow::decode(&corrupt), Err(Error::Corrupt)));
        }
        let mut corrupt = b;
        corrupt.push(0);
        assert!(matches!(DecimalRow::decode(&corrupt), Err(Error::Corrupt)));
    }
    for value in [d(1, 3), Decimal::MAX] {
        let mut bad = row(1, 0);
        bad.value = value;
        assert!(matches!(bad.validate(), Err(Error::InvalidOperand)));
        assert!(matches!(bad.encode(), Err(Error::InvalidOperand)));
    }
    let row = row(1, 100);
    let mut b = bytes(&row);
    // Numerically equivalent persisted 1.0 still violates authored scale 2.
    b[9..25].copy_from_slice(&10_i128.to_be_bytes());
    b[25] = 1;
    assert!(matches!(DecimalRow::decode(&b), Err(Error::Corrupt)));
    fingerprint::check(
        Schema::new(&[DecimalRow::SCHEMA]).unwrap().fingerprint(),
        "ad9ce8031e66e01e253b2f2f3d1fb51b5722dd104ba08ffc35723fe44744a1a1",
    );
    let absent = DecimalRow { maybe: None, ..row };
    let zero = DecimalRow {
        maybe: Some(Decimal::ZERO),
        ..row
    };
    assert_ne!(absent.encode().unwrap(), zero.encode().unwrap());
    assert_eq!(
        DecimalRow::decode(&absent.encode().unwrap()).unwrap(),
        absent
    );
    // Codec boundary scales include zero and 28; no rounding at either edge.
    for scale in [0, 28] {
        for coefficient in [i128::MIN, 0, i128::MAX] {
            let mut writer = ic_db::__private::Encoder::new();
            writer.decimal(d(coefficient, scale), scale).unwrap();
            let mut expected = coefficient.to_be_bytes().to_vec();
            expected.push(scale as u8);
            assert_eq!(writer.finish(), expected);
            let mut reader = ic_db::__private::Decoder::new(&expected).unwrap();
            assert_eq!(reader.decimal(scale).unwrap(), d(coefficient, scale));
            reader.finish().unwrap();
        }
    }
    let mut changed = DecimalRow::SCHEMA.fields.to_vec();
    changed[2].value = ic_db::FieldType::Decimal { scale: 3 };
    let changed = Box::leak(Box::new(ic_db::EntitySchema {
        fields: Box::leak(changed.into_boxed_slice()),
        ..*DecimalRow::SCHEMA
    }));
    assert_ne!(
        Schema::new(&[changed]).unwrap().fingerprint(),
        Schema::new(&[DecimalRow::SCHEMA]).unwrap().fingerprint()
    );
}

#[test]
fn uniqueness_reopen_and_key_integrity() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[DecimalRow::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [11; 16]).unwrap();
    let mut rows: Vec<_> = [i128::MIN, -100, -1, 0, 1, 100, i128::MAX]
        .into_iter()
        .enumerate()
        .map(|(n, v)| row(n as u64 + 1, v))
        .collect();
    let mut b = Batch::new();
    for r in rows.iter().rev() {
        b.insert(r).unwrap();
    }
    db.commit(b).unwrap();
    let read = |db: &Database| {
        db.index_prefix::<DecimalRow>("zone,value,id", &[true.into()], 32)
            .unwrap()
    };
    assert_eq!(read(&db), rows);
    assert_eq!(
        db.index_prefix::<DecimalRow>("maybe", &[d(10, 1).into()], 32)
            .unwrap()[0]
            .id,
        6
    );
    for value in [IndexValue::Nat64(1), d(1, 3).into(), Decimal::MAX.into()] {
        assert!(matches!(
            db.index_prefix::<DecimalRow>("maybe", &[value], 32),
            Err(Error::InvalidOperand)
        ));
    }
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.replace(&rows[5]).unwrap();
    db.commit(b).unwrap();
    assert_eq!(*backing.borrow(), before);
    let mut duplicate = rows[0].clone();
    duplicate.maybe = Some(d(1000, 3));
    let mut b = Batch::new();
    b.replace(&duplicate).unwrap();
    assert!(matches!(db.commit(b), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    let left = rows[0].maybe;
    rows[0].maybe = rows[6].maybe;
    rows[6].maybe = left;
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    b.replace(&rows[6]).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert_eq!(read(&db), rows);
    drop(db);
    let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
    let index = DecimalRow::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "zone,value,id")
        .unwrap();
    let mut prefix = vec![3];
    prefix.extend(DecimalRow::SCHEMA.id.to_be_bytes());
    prefix.extend(index.id.to_be_bytes());
    prefix.push(1);
    let mut original = prefix.clone();
    original.extend([2, 128, 0, 0, 0, b'1', 0]);
    original.extend(6_u64.to_be_bytes());
    original.extend(6_u64.to_be_bytes());
    assert!(raw.remove(&original).is_some());
    drop(raw);
    // Equivalent noncanonical 1.00, a leading zero, invalid sign, exponent
    // overflow, fractional precision beyond the field, and coefficient overflow.
    let mut overflow = vec![2, 128, 0, 0, 38];
    overflow.extend([b'9'; 39]);
    overflow.push(0);
    for component in [
        vec![2, 128, 0, 0, 0, b'1', b'0', b'0', 0],
        vec![2, 128, 0, 0, 0, b'0', b'1', 0],
        vec![3],
        vec![2, 255, 255, 255, 255, b'1', 0],
        vec![2, 127, 255, 255, 253, b'1', 0],
        overflow,
    ] {
        let mut bad = prefix.clone();
        bad.extend(component);
        bad.extend(6_u64.to_be_bytes());
        bad.extend(6_u64.to_be_bytes());
        let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
        raw.insert(bad.clone(), 6_u64.to_be_bytes().to_vec());
        drop(raw);
        let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
        assert!(matches!(
            db.index_prefix::<DecimalRow>("zone,value,id", &[true.into()], 32),
            Err(Error::Corrupt)
        ));
        drop(db);
        let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
        raw.remove(&bad);
    }
}

#[test]
fn collections_and_unindexed_scale_are_checked() {
    let row = Collections {
        id: 1,
        list: vec![d(1, 0), d(100, 2)],
        map: Flags(BTreeMap::from([(d(1, 0), true), (d(2, 0), false)])),
        set: Prices(BTreeSet::from([d(1, 0), d(10, 1), d(-1, 0)])),
    };
    let bytes = row.encode().unwrap();
    assert_eq!(Collections::decode(&bytes).unwrap(), row);
    assert_eq!(row.set.0.len(), 2);
    // id + list count + two decimals + map count => first key offset 50.
    let mut duplicate = bytes.clone();
    duplicate[68..85].copy_from_slice(&bytes[50..67]);
    assert!(matches!(
        Collections::decode(&duplicate),
        Err(Error::Corrupt)
    ));
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[Collections::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [11; 16]).unwrap();
    let mut b = Batch::new();
    b.insert(&row).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
    let mut key = vec![2];
    key.extend(Collections::SCHEMA.id.to_be_bytes());
    key.extend(1_u64.to_be_bytes());
    let mut corrupt = bytes;
    corrupt[28] = 1;
    raw.insert(key, corrupt);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.delete::<Collections>(1).unwrap();
    assert!(matches!(db.commit(b), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}
