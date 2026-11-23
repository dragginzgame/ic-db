//! Bounded mixed-history witness. These private controls are not product endpoints.

use crate::{
    Backing, MemoryRuntime,
    handles::{AggregateJobs, Database, RetiredSource, SchemaCopy},
    runtime,
};
use ic_db::design::prelude::*;
use ic_db::{
    Batch, Column, Comparison, CopyStatus, Error, JobHandle, JobOutcome, SchemaTransition,
    SelectColumns,
};
use ic_db_qualification_transport::mixed::{
    Command, CountAction, CountOutcome, CountReply, CountRequest, ItemPage, Query, QueryReply,
    Reply, Snapshot, Update,
};
use ic_storage::{RuntimeMemory, StableKey};

// Drive one explicit maintenance step for the installed qualification workload.

// mixed_migrate
// Run one migration operation in the mixed-history fixture.
// guard  : validates the request frame for method 244.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "mixed_migrate",
    guard = crate::transport::admit::<244>,
    decode = crate::transport::decode_measured::<_,244>,
    encode = crate::transport::encode::<_,244>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn mixed_migrate((operation, counter): crate::transport::Measured<u8>) -> (u64, bool, u64) {
    let start = counter();
    let (rows, complete) = migrate(operation).unwrap();

    (rows, complete, counter() - start)
}

mod endpoints {
    // Mount the fixture’s inspection, reads and updates through Canic.
    canic::mount_db_endpoints! {
        protocol = crate::mixed_lifecycle::protocol,
        authorize = crate::mixed_lifecycle::endpoints::authorize,
        inspect = crate::mixed_lifecycle::inspect,
        query = crate::mixed_lifecycle::endpoints::query,
        write = crate::mixed_lifecycle::endpoints::apply_update,
    }

    fn query(_: ic_db::Principal, request: super::Query) -> super::QueryReply {
        super::query(
            request,
            crate::transport::instruction_meter().expect("installed Canic endpoint"),
        )
    }

    fn apply_update(_: ic_db::Principal, request: super::Update, time_ns: u64) -> super::Reply {
        super::apply_update(
            request,
            time_ns,
            crate::transport::instruction_meter().expect("installed Canic endpoint"),
        )
    }

    // Only the fixture canister itself may invoke the mounted operations.
    fn authorize(caller: ic_db::Principal, _: canic::db::Operation) -> Result<(), String> {
        if caller == ic_platform_runtime::system::canister_self() {
            Ok(())
        } else {
            Err("mixed workload caller denied".into())
        }
    }
}

pub(super) const KEYS: [&str; 3] = [
    <MixedStore as ic_db::Store>::KEY,
    "m1.mixed.target.v1",
    "m1.mixed.jobs.v1",
];

///
/// Parent
/// Exercises indexed access by `code` on rows keyed by `id`.
///

#[entity(
    store = self::MixedStore,
    pk(field = "id"),
    index(field = "code", unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "code", value(item(prim = "Nat64")))
    )
)]
struct Parent {}

///
/// Item
/// Exercises references to `Parent` on rows keyed by `id`.
///

#[entity(
    store = self::MixedStore,
    pk(field = "id"),
    index(fields = ["parent", "id"]),
    index(fields = ["score", "id"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "parent", value(item(prim = "Nat64")), references(entity = "Parent", index = "parent,id")),
        field(name = "score", value(item(prim = "Nat64")))
    )
)]
struct Item {}

///
/// TargetItem
/// Exercises references to `Parent` on rows keyed by `id`.
///

#[entity(
    store = self::MixedStore,
    name = "Item",
    pk(field = "id"),
    index(fields = ["parent", "id"]),
    index(fields = ["score", "id"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "parent", value(item(prim = "Nat64")), references(entity = "Parent", index = "parent,id")),
        field(name = "score", value(item(prim = "Nat128")))
    )
)]
struct TargetItem {}

///
/// Link
/// Exercises references to `Item` on rows keyed by `id`.
///

#[entity(
    store = self::MixedStore,
    pk(field = "id"),
    index(fields = ["item", "id"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "item", value(item(prim = "Nat64")), references(entity = "Item", index = "item,id"))
    )
)]
struct Link {}

///
/// ParentKey
/// Carries the `Nat64` value of `parent`.
///

#[record(fields(field(name = "parent", value(item(prim = "Nat64")))))]
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

fn open(host: &MemoryRuntime<Backing>, target: bool) -> Result<Database, Error> {
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
        Error::InvalidCursor => 8,
        other => panic!("unexpected mixed-history error: {other:?}"),
    }
}

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
// Application-owned protocol for this installed workload. Canic owns the guards,
// framed entrypoints and reply encoding. Only the fixture's own principal is admitted.
mod protocol {
    pub(super) const QUERY: u64 = 242;
    pub(super) const WRITE: u64 = 241;
    pub(super) type QueryRequest = super::Query;
    pub(super) type QueryReply = super::QueryReply;
    pub(super) type WriteRequest = super::Update;
    pub(super) type WriteReply = super::Reply;
}

// Inspect the source store declared by this host, including while it is frozen.
fn inspect(_: ic_db::Principal, (): ()) -> canic::db::inspection::Response {
    use canic::db::inspection::{Admission, Response};

    let source = schema(false);
    let fingerprint = source.fingerprint();
    let admission = match runtime().open_memory(&StableKey::parse(KEYS[0]).unwrap()) {
        Ok(memory) => match Database::open(memory, source) {
            Ok(db) => Admission::Readable {
                incarnation: db.incarnation(),
            },
            Err(Error::Maintenance) => Admission::Maintenance,
            Err(Error::IncompatibleSchema) => Admission::Incompatible,
            Err(_) => Admission::Unavailable,
        },
        Err(_) => Admission::Unavailable,
    };
    Response {
        schema: fingerprint,
        admission,
    }
}

// Commit a batch or operate on a retained count.
fn apply_update(request: Update, time_ns: u64, counter: fn() -> u64) -> Reply {
    match request {
        Update::Batch(target, commands) => {
            let start = counter();
            let result = write(target, commands).map_err(refusal);

            Reply::Batch(result, counter() - start)
        }
        Update::ReplaceManaged { returning, body } => {
            let start = counter();
            let result = replace_managed(returning, body)
                .map(crate::managed::view)
                .map_err(refusal);

            Reply::Managed(result, counter() - start)
        }
        Update::Count(request) => {
            let start = counter();
            let result = count(request, time_ns).map_err(|error| format!("{error:?}"));

            Reply::Count(result, counter() - start)
        }
    }
}

// Both paths update the same pair and return the first row with its database-assigned time.
fn replace_managed(returning: bool, body: String) -> Result<crate::schema::ManagedRow, Error> {
    use crate::schema::ManagedRow;

    let mut db = crate::open(&runtime());
    let mut first = db.get::<ManagedRow>(1_u64)?.ok_or(Error::Missing)?;
    let mut second = db.get::<ManagedRow>(2_u64)?.ok_or(Error::Missing)?;
    first.body = body;
    second.body = "ready".into();
    let mut batch = Batch::new();
    batch.replace(&first)?;
    batch.replace(&second)?;
    if returning {
        db.commit_returning(batch, first.id())
    } else {
        db.commit(batch)?;
        db.get::<ManagedRow>(first.id)?.ok_or(Error::Missing)
    }
}

fn count(request: CountRequest, time_ns: u64) -> Result<CountReply, Error> {
    let now_ms = (time_ns / 1_000_000) as i64;
    let host = runtime();
    let mut jobs = AggregateJobs::open(memory(&host, 2))?;

    match request {
        CountRequest::Run(target, action) => {
            let db = open(&host, target)?;

            if target {
                run_count::<TargetItem>(&db, &mut jobs, action, now_ms)
            } else {
                run_count::<Item>(&db, &mut jobs, action, now_ms)
            }
        }
        CountRequest::CleanupExpired(limit) => {
            let removed =
                jobs.cleanup_expired(Timestamp::from_millis(now_ms), usize::from(limit))?;
            Ok(CountReply::Removed(removed as u64))
        }
        CountRequest::Cancel(bytes) => {
            jobs.cancel(&JobHandle::from_bytes(&bytes)?)?;

            Ok(CountReply::Cancelled)
        }
    }
}

fn run_count<E: Entity>(
    db: &Database,
    jobs: &mut AggregateJobs,
    action: CountAction,
    now_ms: i64,
) -> Result<CountReply, Error> {
    match action {
        CountAction::Start | CountAction::StartGrouped => {
            let now = Timestamp::from_millis(now_ms);
            let expires = Timestamp::from_millis(now_ms + 3_600_000);
            let query = db.query::<E>();
            let handle = if matches!(action, CountAction::StartGrouped) {
                query.start_group_count::<ParentKey>(jobs, now, expires)?
            } else {
                query.start_count(jobs, now, expires)?
            };

            Ok(CountReply::Started(handle.to_bytes()))
        }
        CountAction::Resume(bytes, sequence, work) => {
            let handle = JobHandle::from_bytes(&bytes)?;
            let step = db.query::<E>().work_limit(usize::from(work)).resume_count(
                jobs,
                &handle,
                sequence,
                Timestamp::from_millis(now_ms),
            )?;
            let outcome = match step.outcome {
                JobOutcome::Running => CountOutcome::Running,
                JobOutcome::Complete(value) => CountOutcome::Complete(value),
                JobOutcome::Invalidated => CountOutcome::Invalidated,
                JobOutcome::Expired => CountOutcome::Expired,
            };

            Ok(CountReply::Advanced(step.sequence, outcome))
        }
        CountAction::ResumeGrouped(bytes, sequence, work) => {
            let handle = JobHandle::from_bytes(&bytes)?;
            let step = db
                .query::<E>()
                .work_limit(usize::from(work))
                .resume_group_count::<ParentKey>(
                    jobs,
                    &handle,
                    sequence,
                    Timestamp::from_millis(now_ms),
                )?;
            let outcome = match step.outcome {
                JobOutcome::Running => CountOutcome::Running,
                JobOutcome::Complete(page) => CountOutcome::Groups(
                    page.rows
                        .into_iter()
                        .map(|r| (r.key.parent, r.count))
                        .collect(),
                ),
                JobOutcome::Invalidated => CountOutcome::Invalidated,
                JobOutcome::Expired => CountOutcome::Expired,
            };
            Ok(CountReply::GroupAdvanced(
                step.sequence,
                step.examined,
                outcome,
            ))
        }
    }
}

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
// Read a full fixture snapshot or one bounded item page.
fn query(request: Query, counter: fn() -> u64) -> QueryReply {
    let start = counter();

    match request {
        Query::Snapshot(target, parent, minimum) => {
            let result = read(target, parent, minimum).map_err(refusal);

            QueryReply::Snapshot(result, counter() - start)
        }
        Query::Items {
            target,
            parent,
            minimum,
            work,
            after,
        } => {
            let work = work.unwrap_or(8);
            let result = open(&runtime(), target)
                .and_then(|db| {
                    if target {
                        item_page::<TargetItem>(
                            &db,
                            parent,
                            u128::from(minimum).into(),
                            work,
                            after.as_deref(),
                            |r| r.id,
                        )
                    } else {
                        item_page::<Item>(
                            &db,
                            parent,
                            minimum.into(),
                            work,
                            after.as_deref(),
                            |r| r.id,
                        )
                    }
                })
                .map_err(refusal);

            QueryReply::Items(result, counter() - start)
        }
    }
}

fn item_page<E: SelectColumns<Key = u64>>(
    db: &Database,
    parent: u64,
    minimum: ic_db::IndexValue<'_>,
    work: u16,
    after: Option<&[u8]>,
    id: impl FnOnce(E::Columns) -> Column<E, u64>,
) -> Result<ItemPage, Error> {
    let cursor = after.map(ic_db::QueryCursor::from_bytes).transpose()?;
    let page = db
        .query::<E>()
        .eq(Item::PARENT, parent)
        .filter(Item::SCORE, Comparison::Ge, minimum)
        .order_by(&[Item::PARENT, Item::ID])
        .work_limit(usize::from(work))
        .limit(2)
        .after_optional(cursor.as_ref())
        .select(id)
        .fetch()?;

    Ok(ItemPage {
        ids: page.rows,
        next: page.next.map(|cursor| cursor.to_bytes()),
        examined: page.examined as u64,
    })
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
            let step = match copy.status() {
                CopyStatus::Copying("Item") => {
                    copy.copy_next::<Item, TargetItem>(2, 4096, |row, _| {
                        Ok(TargetItem {
                            id: row.id,
                            parent: row.parent,
                            score: u128::from(row.score) + 1000,
                        })
                    })?
                }
                CopyStatus::Copying("Link") => copy.copy_unchanged::<Link>(2, 4096)?,
                CopyStatus::Copying("Parent") => copy.copy_unchanged::<Parent>(2, 4096)?,
                CopyStatus::Validating | CopyStatus::ReadyToPublish => return Ok((0, true)),
                _ => return Err(Error::InvalidTransition),
            };

            Ok((step.rows as u64, step.complete))
        }
        2 => Ok((0, copy.validate_next(2)?)),
        3 => {
            copy.publish()?;

            Ok((0, true))
        }
        _ => panic!("invalid migration control"),
    }
}

/// Store using the existing m1.mixed.source.v1 allocation key.
#[store(canister = records_schema::RecordsCanister, key = "m1.mixed.source.v1")]
pub struct MixedStore {}
