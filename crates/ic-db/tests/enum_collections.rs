//! Enum order agrees across collections, row bytes and secondary keys.

use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};
use std::collections::{BTreeMap, BTreeSet};

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
/// States
/// Stores up to 3 distinct `State` values.
///

#[set(item(is = "State"), max_items = 3)]
struct States {}

///
/// Counts
/// Maps up to 3 `State` keys to optional `Nat16` values.
///

#[map(key(is = "State"), value(opt, item(prim = "Nat16")), max_items = 3)]
struct Counts {}

///
/// Row
/// Stores `counts` and `states` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "counts", value(item(is = "Counts"))),
        field(name = "states", value(item(is = "States")))
    )
)]
struct Row {}

///
/// Nested
/// Stores `sets` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "sets", value(many, max_items = 64, item(is = "States")))
    )
)]
struct Nested {}

///
/// Payload
/// Distinguishes the `Empty` and `Data` variants.
///

#[enum_(
    variant(name = "Empty"),
    variant(name = "Data", value(item(prim = "Nat8")))
)]
#[derive(PartialOrd, Ord)]
struct Payload {}

///
/// InvalidSet
/// Stores up to 3 distinct `Payload` values.
///

#[set(item(is = "Payload"), max_items = 3)]
struct InvalidSet {}

///
/// InvalidMap
/// Maps up to 3 `Payload` keys to `Bool` values.
///

#[map(key(is = "Payload"), value(item(prim = "Bool")), max_items = 3)]
struct InvalidMap {}

///
/// BadSetRow
/// Stores `value` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(is = "InvalidSet")))
    )
)]
struct BadSetRow {}

///
/// BadMapRow
/// Stores `value` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(is = "InvalidMap")))
    )
)]
struct BadMapRow {}

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

fn row() -> Row {
    Row {
        id: 1,
        counts: Counts(BTreeMap::from([
            (State::Ready, Some(256)),
            (State::Archived, None),
        ])),
        states: States(BTreeSet::from([State::Ready, State::Archived])),
    }
}

fn golden() -> Vec<u8> {
    // counts: two entries (Archived/absent, Ready/256); id; two set ordinals.
    vec![
        2, 0, 0, 0, 0, 0, 2, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 2,
    ]
}

#[test]
fn order_and_transitive_identity_are_shared() {
    assert_eq!(
        BTreeSet::from([State::Ready, State::Archived, State::Draft])
            .into_iter()
            .collect::<Vec<_>>(),
        [State::Archived, State::Draft, State::Ready]
    );
    assert_eq!(State::definition(), Reordered::definition());
    for (value, ordinal) in [(State::Archived, 0), (State::Draft, 1), (State::Ready, 2)] {
        assert_eq!(
            ic_db::__private::encode_index(&[((&value).into(), false)], false),
            Some(vec![ordinal])
        );
    }
    let schema = Schema::new(&[Row::SCHEMA]).unwrap();
    assert_eq!(
        schema.types().map(|t| t.name).collect::<Vec<_>>(),
        ["Counts", "State", "States"]
    );
    assert_eq!(row().encode().unwrap(), golden());
    assert_eq!(Row::decode(&golden()).unwrap(), row());
    let mut no_entry = row();
    no_entry.counts.0.remove(&State::Archived);
    assert_ne!(no_entry.encode().unwrap(), golden());
    // Deriving Rust order does not grant payload enums a database ordering contract.
    assert!(matches!(
        Schema::new(&[BadSetRow::SCHEMA]),
        Err(Error::InvalidSchema)
    ));
    assert!(matches!(
        Schema::new(&[BadMapRow::SCHEMA]),
        Err(Error::InvalidSchema)
    ));
}

#[test]
fn corrupt_collections_refuse_before_materialization_or_mutation() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[Row::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [18; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&row()).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert_eq!(db.get::<Row>(1).unwrap(), Some(row()));
    drop(db);
    let mut corruptions = Vec::new();
    for (offset, value) in [(6, 0), (4, 2), (6, 255), (23, 0), (22, 2), (23, 255)] {
        let mut bytes = golden();
        bytes[offset] = value;
        corruptions.push(bytes);
    }
    let mut excess = golden();
    excess[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    corruptions.push(excess);
    for bytes in corruptions {
        assert!(matches!(Row::decode(&bytes), Err(Error::Corrupt)));
        let mut key = vec![2];
        key.extend(Row::SCHEMA.id.to_be_bytes());
        key.extend(1_u64.to_be_bytes());
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
        raw.insert(key, bytes);
        drop(raw);
        let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
        let before = backing.borrow().clone();
        for delete in [false, true] {
            let mut batch = Batch::new();
            if delete {
                batch.delete::<Row>(1).unwrap();
            } else {
                batch.replace(&row()).unwrap();
            }
            assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
            assert_eq!(*backing.borrow(), before);
        }
    }
}

#[test]
fn enum_keys_count_toward_the_shared_structured_budget() {
    let set = States(BTreeSet::from([
        State::Archived,
        State::Draft,
        State::Ready,
    ]));
    let mut row = Nested {
        id: 1,
        sets: vec![set.clone(); 63],
    };
    let bytes = row.encode().unwrap();
    assert_eq!(Nested::decode(&bytes).unwrap(), row);
    row.sets.push(set);
    // 64 sets + 192 enums + the outer list = 257 nodes, but exactly 256 items.
    assert!(matches!(row.encode(), Err(Error::Limit)));
    let mut bytes = vec![1, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 0];
    for _ in 0..64 {
        bytes.extend([3, 0, 0, 0, 0, 1, 2]);
    }
    assert!(matches!(Nested::decode(&bytes), Err(Error::Corrupt)));
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[Nested::SCHEMA]).unwrap();
    let db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [19; 16]).unwrap();
    drop(db);
    let mut key = vec![2];
    key.extend(Nested::SCHEMA.id.to_be_bytes());
    key.extend(1_u64.to_be_bytes());
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    raw.insert(key, bytes);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.delete::<Nested>(1).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}

///
/// Rank
/// Gives `Nat16` a distinct schema identity.
///

#[newtype(item(prim = "Nat16"))]
struct Rank {}

///
/// Flags
/// Maps up to 2 `Rank` keys to `Bool` values.
///

#[map(key(is = "Rank"), value(item(prim = "Bool")), max_items = 2)]
struct Flags {}

///
/// Ranks
/// Stores up to 2 distinct `Rank` values.
///

#[set(item(is = "Rank"), max_items = 2)]
struct Ranks {}

///
/// Ranked
/// Stores `flags` and `ranks` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "flags", value(item(is = "Flags"))),
        field(name = "ranks", value(item(is = "Ranks")))
    )
)]
struct Ranked {}

#[test]
fn wrapped_collection_keys_keep_numeric_order_on_typed_and_borrowed_reads() {
    let row = Ranked {
        id: 1,
        flags: Flags(BTreeMap::from([(Rank(256), true), (Rank(255), false)])),
        ranks: Ranks(BTreeSet::from([Rank(256), Rank(255)])),
    };
    // Stored u16s are little-endian, so raw byte order would reverse these keys.
    let bytes = [
        2, 0, 0, 0, 255, 0, 0, 0, 1, 1, // flags
        1, 0, 0, 0, 0, 0, 0, 0, // id
        2, 0, 0, 0, 255, 0, 0, 1, // ranks
    ];
    assert_eq!(row.encode().unwrap(), bytes);
    let schema = || Schema::new(&[Ranked::SCHEMA]).unwrap();
    assert_eq!(
        schema().types().map(|ty| ty.name).collect::<Vec<_>>(),
        ["Flags", "Rank", "Ranks"]
    );
    let runtime = host(VectorMemory::default());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [20; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert_eq!(db.get::<Ranked>(1).unwrap(), Some(row));
    drop(db);

    for second_key in [7, 24] {
        for invalid in [255_u16, 254] {
            let mut corrupt = bytes;
            corrupt[second_key..second_key + 2].copy_from_slice(&invalid.to_le_bytes());
            assert!(matches!(Ranked::decode(&corrupt), Err(Error::Corrupt)));
            let mut key = vec![2];
            key.extend(Ranked::SCHEMA.id.to_be_bytes());
            key.extend(1_u64.to_be_bytes());
            let mut raw =
                StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
            raw.insert(key, corrupt.to_vec());
            drop(raw);
            let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
            let mut batch = Batch::new();
            batch.delete::<Ranked>(1).unwrap();
            assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
        }
    }
}
