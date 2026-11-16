//! Bounded mixed-history witness. These private controls are not product endpoints.
use crate::{Backing, MemoryRuntime, runtime};
use ic_db::{
    AggregateJobs, Batch, Comparison, Database, Entity, Error, JobHandle, JobOutcome,
    RetiredSource, Schema, SchemaCopy, SchemaTransition, Timestamp,
};
use ic_storage::{RuntimeMemory, StableKey};

pub(super) const KEYS: [&str; 3] = [
    "m1.mixed.source.v1",
    "m1.mixed.target.v1",
    "m1.mixed.jobs.v1",
];

#[ic_db::entity(
    pk(field = "id"),
    index(field = "code", unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "code", value(item(prim = "Nat64")))
    )
)]
struct Parent {}
#[ic_db::entity(pk(field="id"), index(fields=["parent","id"]), index(fields=["score","id"]), fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="parent",value(item(prim="Nat64")),references(entity="Parent",index="parent,id")),
    field(name="score",value(item(prim="Nat64")))
))]
struct Item {}
#[ic_db::entity(name="Item", pk(field="id"), index(fields=["parent","id"]), index(fields=["score","id"]), fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="parent",value(item(prim="Nat64")),references(entity="Parent",index="parent,id")),
    field(name="score",value(item(prim="Nat128")))
))]
struct TargetItem {}
#[ic_db::entity(pk(field="id"), index(fields=["item","id"]), fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="item",value(item(prim="Nat64")),references(entity="Item",index="item,id"))
))]
struct Link {}
#[ic_db::record(fields(field(name = "parent", value(item(prim = "Nat64")))))]
struct ParentKey {}

fn schema(target: bool) -> Schema {
    Schema::new(&[
        Parent::SCHEMA,
        if target {
            TargetItem::SCHEMA
        } else {
            Item::SCHEMA
        },
        Link::SCHEMA,
    ])
    .unwrap()
}
fn memory(host: &MemoryRuntime<Backing>, slot: usize) -> RuntimeMemory<Backing> {
    host.open_memory(&StableKey::parse(KEYS[slot]).unwrap())
        .unwrap()
}
fn open(host: &MemoryRuntime<Backing>, target: bool) -> Result<Database<Backing>, Error> {
    Database::open(memory(host, usize::from(target)), schema(target))
}
pub(super) fn initialize(host: &MemoryRuntime<Backing>) {
    Database::create(memory(host, 0), schema(false), [21; 16]).unwrap();
    AggregateJobs::create(memory(host, 2), [23; 16]).unwrap();
}
fn refusal(error: Error) -> u8 {
    match error {
        Error::Exists => 1,
        Error::Missing => 2,
        Error::DuplicateTarget => 3,
        Error::UniqueViolation => 4,
        Error::ReferenceViolation => 5,
        Error::Limit => 6,
        Error::Maintenance => 7,
        other => panic!("unexpected mixed-history error: {other:?}"),
    }
}
// Entity, operation (insert/replace/delete/read-modify), key, value, score/delta.
type Command = (u8, u8, u64, u64, u64);
fn write(target: bool, commands: Vec<Command>) -> Result<(), Error> {
    let host = runtime();
    let mut db = open(&host, target)?;
    let mut batch = Batch::new();
    for (entity, operation, id, value, score) in commands {
        match (entity, operation) {
            (0, 0) => {
                batch.insert(&Parent { id, code: value })?;
            }
            (0, 1) => {
                batch.replace(&Parent { id, code: value })?;
            }
            (0, 2) => {
                batch.delete::<Parent>(id)?;
            }
            (1, 2) if target => {
                batch.delete::<TargetItem>(id)?;
            }
            (1, 2) => {
                batch.delete::<Item>(id)?;
            }
            (1, 0 | 1 | 3) if target => {
                let row = if operation == 3 {
                    let mut row = db.get::<TargetItem>(id)?.ok_or(Error::Missing)?;
                    row.score = row
                        .score
                        .checked_add(u128::from(score))
                        .ok_or(Error::Limit)?;
                    row
                } else {
                    TargetItem {
                        id,
                        parent: value,
                        score: u128::from(score),
                    }
                };
                if operation == 0 {
                    batch.insert(&row)?;
                } else {
                    batch.replace(&row)?;
                }
            }
            (1, 0 | 1 | 3) => {
                let row = if operation == 3 {
                    let mut row = db.get::<Item>(id)?.ok_or(Error::Missing)?;
                    row.score = row.score.checked_add(score).ok_or(Error::Limit)?;
                    row
                } else {
                    Item {
                        id,
                        parent: value,
                        score,
                    }
                };
                if operation == 0 {
                    batch.insert(&row)?;
                } else {
                    batch.replace(&row)?;
                }
            }
            (2, 0) => {
                batch.insert(&Link { id, item: value })?;
            }
            (2, 1) => {
                batch.replace(&Link { id, item: value })?;
            }
            (2, 2) => {
                batch.delete::<Link>(id)?;
            }
            _ => panic!("invalid private fixture command"),
        }
    }
    db.commit(batch)
}
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "mixed_write",
    guard = crate::transport::admit::<241>,
    decode = crate::transport::decode::<_,241>,
    encode = crate::transport::encode::<_,241>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn mixed_write((target, commands): (bool, Vec<Command>)) -> (Result<(), u8>, u64) {
    let start = ic_platform_runtime::raw::instruction_counter();
    let result = write(target, commands).map_err(refusal);
    (
        result,
        ic_platform_runtime::raw::instruction_counter() - start,
    )
}
// Independent driver compares all fields, not a digest produced by this writer.
type Snapshot = (
    Vec<(u64, u64)>,
    Vec<(u64, u64, u128)>,
    Vec<(u64, u64)>,
    [u64; 3],
    [u64; 3],
    Vec<u64>,
    Vec<(u64, u64)>,
    Vec<u64>,
);
fn read(target: bool, parent: u64, minimum: u64) -> Result<Snapshot, Error> {
    let host = runtime();
    let db = open(&host, target)?;
    let parents = db.query::<Parent>().limit(32).fetch()?;
    let links = db.query::<Link>().limit(32).fetch()?;
    assert!(parents.next.is_none() && links.next.is_none());
    // Both branches execute the same admitted operations against their compiled types.
    macro_rules! items {
        ($ty:ty, $minimum:expr) => {{
            let rows = db.query::<$ty>().limit(32).fetch()?;
            let indexed = db
                .query::<$ty>()
                .using_index("parent,id", &[parent.into()])
                .filter("score", Comparison::Ge, $minimum)
                .order_by(&["parent", "id"])
                .limit(32)
                .fetch()?;
            assert!(rows.next.is_none() && indexed.next.is_none());
            let ordered = db
                .query::<$ty>()
                .using_index("score,id", &[])
                .order_by(&["score", "id"])
                .limit(32)
                .fetch()?;
            assert!(ordered.next.is_none());
            let groups = db
                .query::<$ty>()
                .group_count::<ParentKey>()?
                .value
                .into_iter()
                .map(|row| (row.key.parent, row.count))
                .collect();
            (
                rows.rows
                    .into_iter()
                    .map(|row| (row.id, row.parent, u128::from(row.score)))
                    .collect(),
                db.cardinality::<$ty>()?,
                db.revision::<$ty>()?,
                indexed.rows.into_iter().map(|row| row.id).collect(),
                groups,
                ordered.rows.into_iter().map(|row| row.id).collect(),
            )
        }};
    }
    let (items, count, revision, indexed, groups, ordered) = if target {
        items!(TargetItem, u128::from(minimum))
    } else {
        items!(Item, minimum)
    };
    Ok((
        parents
            .rows
            .into_iter()
            .map(|row| (row.id, row.code))
            .collect(),
        items,
        links
            .rows
            .into_iter()
            .map(|row| (row.id, row.item))
            .collect(),
        [
            db.cardinality::<Parent>()?,
            count,
            db.cardinality::<Link>()?,
        ],
        [db.revision::<Parent>()?, revision, db.revision::<Link>()?],
        indexed,
        groups,
        ordered,
    ))
}
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "mixed_read",
    guard = crate::transport::admit::<242>,
    decode = crate::transport::decode::<_,242>,
    encode = crate::transport::encode::<_,242>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn mixed_read((target, parent, minimum): (bool, u64, u64)) -> (Result<Snapshot, u8>, u64) {
    let start = ic_platform_runtime::raw::instruction_counter();
    let result = read(target, parent, minimum).map_err(refusal);
    (
        result,
        ic_platform_runtime::raw::instruction_counter() - start,
    )
}
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "mixed_job",
    guard = crate::transport::admit::<243>,
    decode = crate::transport::decode::<_,243>,
    encode = crate::transport::encode::<_,243>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn mixed_job((operation, bytes, sequence): (u8, Vec<u8>, u64)) -> (Vec<u8>, u64, u8, Option<u64>) {
    let host = runtime();
    let mut jobs = AggregateJobs::open(memory(&host, 2)).unwrap();
    if operation == 2 {
        jobs.cancel(&JobHandle::from_bytes(&bytes).unwrap())
            .unwrap();
        return (bytes, sequence, 4, None);
    }
    let db = open(&host, false).unwrap();
    if operation == 0 {
        let handle = db
            .query::<Item>()
            .start_count(
                &mut jobs,
                Timestamp::from_millis(0),
                Timestamp::from_millis(10000),
            )
            .unwrap();
        return (handle.to_bytes(), 0, 0, None);
    }
    assert_eq!(operation, 1);
    let step = db
        .query::<Item>()
        .work_limit(2)
        .resume_count(
            &mut jobs,
            &JobHandle::from_bytes(&bytes).unwrap(),
            sequence,
            Timestamp::from_millis(1),
        )
        .unwrap();
    let (state, value) = match step.outcome {
        JobOutcome::Running => (0, None),
        JobOutcome::Complete(value) => (1, Some(value)),
        JobOutcome::Invalidated => (2, None),
        JobOutcome::Expired => (3, None),
    };
    (bytes, step.sequence, state, value)
}
fn migrate(operation: u8) -> Result<(u64, bool), Error> {
    let host = runtime();
    let source = schema(false);
    let target = schema(true);
    let transition = SchemaTransition::new(&source, &target, "mixed-score-v2", 1, &[])?;
    if operation == 0 {
        open(&host, false)?
            .begin_maintenance(&transition)?
            .begin_copy(memory(&host, 1), source, target, [22; 16])?;
        return Ok((0, false));
    }
    if operation == 4 {
        return Ok((
            0,
            RetiredSource::resume(memory(&host, 0), &transition, [21; 16], [22; 16])?
                .cleanup_step(2)?,
        ));
    }
    let mut copy = SchemaCopy::resume(
        memory(&host, 0),
        memory(&host, 1),
        source,
        target,
        &transition,
    )?;
    match operation {
        1 => {
            let step = match copy.next_entity() {
                Some("Item") => copy.copy_next::<Item, TargetItem>(2, 4096, |row, _| {
                    Ok(TargetItem {
                        id: row.id,
                        parent: row.parent,
                        score: u128::from(row.score) + 1000,
                    })
                })?,
                Some("Link") => copy.copy_unchanged::<Link>(2, 4096)?,
                Some("Parent") => copy.copy_unchanged::<Parent>(2, 4096)?,
                None => return Ok((0, true)),
                other => panic!("unexpected entity: {other:?}"),
            };
            Ok((step.rows as u64, copy.next_entity().is_none()))
        }
        2 => Ok((0, copy.validate_next(2)?)),
        3 => {
            copy.publish()?;
            Ok((0, true))
        }
        _ => panic!("invalid migration control"),
    }
}
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "mixed_migrate",
    guard = crate::transport::admit::<244>,
    decode = crate::transport::decode::<_,244>,
    encode = crate::transport::encode::<_,244>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn mixed_migrate(operation: u8) -> (u64, bool, u64) {
    let start = ic_platform_runtime::raw::instruction_counter();
    let (rows, complete) = migrate(operation).unwrap();
    (
        rows,
        complete,
        ic_platform_runtime::raw::instruction_counter() - start,
    )
}
