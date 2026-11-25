//! Reused executable metadata never stands in for current durable admission.

#[path = "support/raw_map.rs"]
pub mod raw_map;
use crate::raw_map::RawMap;
use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory,
};

///
/// Payload
/// Carries the `Nat64` value of `amount`.
///

#[record(fields(field(name = "amount", value(item(prim = "Nat64")))))]
struct Payload {}

///
/// Entry
/// Stores `payload` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(is = "Payload")))
    )
)]
struct Entry {}

///
/// Other
/// Exercises a row containing only its `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
struct Other {}

#[test]
fn retained_schema_rechecks_each_allocation_and_each_open() {
    let backing = VectorMemory::default();
    let mut runtime = MemoryRuntime::new(backing.clone()).unwrap();
    let names = ["test.first.v1", "test.second.v1"];
    runtime
        .bootstrap(
            &SealedDeclarationSnapshot::new(
                names
                    .iter()
                    .map(|name| MemoryRequest::new("test", name).unwrap())
                    .collect(),
            )
            .unwrap(),
            &AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap(),
            &GenericAllocationPolicy,
        )
        .unwrap();
    let schema = Schema::new(&[Entry::SCHEMA, Other::SCHEMA]).unwrap();
    for (name, identity) in names.iter().zip([1_u8, 2]) {
        let key = StableKey::parse(name).unwrap();
        let mut db = Database::create(
            runtime.open_memory(&key).unwrap(),
            schema.clone(),
            [identity; 16],
        )
        .unwrap();
        let mut batch = Batch::new();
        batch
            .insert(&Entry {
                id: 1,
                payload: Payload {
                    amount: u64::from(identity),
                },
            })
            .unwrap();
        db.commit(batch).unwrap();
    }
    for (name, identity) in names.iter().zip([1_u8, 2]) {
        let key = StableKey::parse(name).unwrap();
        let db = Database::open(runtime.open_memory(&key).unwrap(), schema.clone()).unwrap();
        assert_eq!(db.incarnation(), [identity; 16]);
        assert_eq!(
            db.get::<Entry>(1).unwrap().unwrap().payload.amount,
            u64::from(identity)
        );
        assert_eq!(db.revision::<Entry>().unwrap(), 1);
        assert_eq!(
            db.schema()
                .types()
                .map(|value| value.name)
                .collect::<Vec<_>>(),
            ["Payload"]
        );
    }

    let key = StableKey::parse(names[0]).unwrap();
    let mut revision_key = vec![1];
    revision_key.extend(Entry::SCHEMA.id.to_be_bytes());
    for failure in 0..4 {
        let mut raw = RawMap::load(runtime.open_memory(&key).unwrap());
        let manifest = raw.get(&[0]).unwrap();
        let revision = raw.get(&revision_key).unwrap();
        match failure {
            0 => {
                let mut foreign = manifest.clone();
                foreign[8] ^= 1;
                raw.insert(vec![0], foreign);
            }
            1 => {
                raw.remove(&revision_key);
            }
            2 => {
                raw.insert(revision_key.clone(), vec![0; 7]);
            }
            3 => {
                let mut exhausted = revision.clone();
                exhausted[..8].copy_from_slice(&u64::MAX.to_le_bytes());
                raw.insert(revision_key.clone(), exhausted);
            }
            _ => unreachable!(),
        }
        drop(raw);
        let before = backing.borrow().clone();
        let result = Database::open(runtime.open_memory(&key).unwrap(), schema.clone());
        if failure == 0 {
            assert!(matches!(result, Err(Error::IncompatibleSchema)));
        } else {
            let mut db = result.unwrap();
            // Ordinary row reads need no revision; explicit revision reads do.
            let row = db.get::<Entry>(1).unwrap().unwrap();
            assert_eq!(row.payload.amount, 1);
            if failure == 3 {
                assert_eq!(db.revision::<Entry>().unwrap(), u64::MAX);
            } else {
                assert!(matches!(db.revision::<Entry>(), Err(Error::Corrupt)));
            }
            let refused = |result| {
                assert!(matches!(
                    (failure, result),
                    (3, Err(Error::Limit)) | (1 | 2, Err(Error::Corrupt))
                ));
            };
            // Validate even a no-op, but an exhausted valid revision need not
            // advance if the row is unchanged.
            let mut no_op = Batch::new();
            no_op.replace(&row).unwrap();
            if failure == 3 {
                db.commit(no_op).unwrap();
            } else {
                refused(db.commit(no_op));
            }
            let mut change = Batch::new();
            change
                .replace(&Entry {
                    id: 1,
                    payload: Payload { amount: 9 },
                })
                .unwrap();
            refused(db.commit(change));
            let mut delete = Batch::new();
            delete.delete::<Entry>(1).unwrap();
            refused(db.commit(delete));
            // Earlier healthy mutations in a mixed batch cannot escape refusal.
            let mut mixed = Batch::new();
            mixed.insert(&Other { id: failure }).unwrap();
            mixed
                .insert(&Entry {
                    id: 2,
                    payload: Payload { amount: 2 },
                })
                .unwrap();
            refused(db.commit(mixed));
            assert_eq!(*backing.borrow(), before);
            // An unrelated counter does not prevent this entity's valid write.
            let mut healthy = Batch::new();
            healthy.insert(&Other { id: failure }).unwrap();
            db.commit(healthy).unwrap();
            assert_eq!(db.revision::<Other>().unwrap(), failure);
            assert!(db.get::<Other>(failure).unwrap().is_some());
        }
        if failure == 0 {
            assert_eq!(*backing.borrow(), before);
        }
        let mut raw = RawMap::load(runtime.open_memory(&key).unwrap());
        raw.insert(vec![0], manifest);
        raw.insert(revision_key.clone(), revision);
    }
    let db = Database::open(runtime.open_memory(&key).unwrap(), schema).unwrap();
    assert_eq!(db.get::<Entry>(1).unwrap().unwrap().payload.amount, 1);
}
