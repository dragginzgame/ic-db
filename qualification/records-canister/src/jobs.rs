//! Feature-selected product-job witness, sharing the ordinary records and supplier.
use super::{
    Backing, MemoryRuntime, StableKey, faults, open, runtime,
    schema::{Document, Entry},
};
use ic_db::{
    AggregateJobs, Batch, Comparison, Database, GroupOrder, JobHandle, JobOutcome, JobStep, Query,
    Timestamp,
};

pub(super) const KEY: &str = "m1.aggregate_jobs.v1";
#[ic_db::record(fields(field(name = "amount", value(item(prim = "Nat64")))))]
struct Amount {}
#[ic_db::record(fields(field(name = "amount", value(item(prim = "Decimal", scale = 2)))))]
struct AverageAmount {}
#[ic_db::record(fields(field(name = "category", value(item(prim = "Nat64")))))]
struct Category {}

fn query(db: &Database<Backing>, operation: u8) -> Query<'_, Entry, Backing> {
    let query = db.query::<Entry>();
    if operation == 3 {
        query
            .filter_groups(Comparison::Ge, 2_u64)
            .order_groups(GroupOrder::AggregateDescending)
    } else {
        query
    }
}

pub(super) fn initialize(host: &MemoryRuntime<Backing>) {
    AggregateJobs::create(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
        [3; 16],
    )
    .unwrap();
}
fn open_jobs(host: &MemoryRuntime<Backing>) -> AggregateJobs<Backing> {
    AggregateJobs::open(host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap()).unwrap()
}
type Response = (u64, u64, u64, u64, u8, Option<String>, u64);
type Progress = (u64, u64, u64, u64, u8, Option<String>);
fn progress<T>(step: JobStep<T>, display: impl FnOnce(T) -> Option<String>) -> Progress {
    let (phase, value) = match step.outcome {
        JobOutcome::Running => (0, None),
        JobOutcome::Complete(value) => (1, display(value)),
        JobOutcome::Invalidated => (2, None),
        JobOutcome::Expired => (3, None),
    };
    (
        step.sequence,
        step.source_revision,
        step.examined,
        step.input_bytes,
        phase,
        value,
    )
}
#[ic_platform_runtime::cdk::update(
    guard = "crate::transport::admit::<110>",
    decode_with = "crate::transport::decode::<_, 110>",
    encode_with = "crate::transport::encode::<_, 110>"
)]
fn job_start((operation, now, expires, fault): (u8, i64, i64, u64)) -> Result<Vec<u8>, String> {
    let host = runtime();
    let db = open(&host);
    let mut jobs = open_jobs(&host);
    let query = query(&db, operation);
    faults::arm_job_write(fault);
    let result = match operation {
        1 => query.start_sum::<Amount>(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        2 => query.start_avg::<AverageAmount>(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        0 => query.start_count(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        3 => query.start_group_count::<Category>(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        4 => query.start_group_sum::<Category, Amount>(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        5 => query.start_group_avg::<Category, AverageAmount>(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        _ => Err(ic_db::Error::InvalidQuery),
    };
    faults::finish_job_write();
    result
        .map(|handle| handle.to_bytes())
        .map_err(|e| e.to_string())
}
#[ic_platform_runtime::cdk::update(
    guard = "crate::transport::admit::<111>",
    decode_with = "crate::transport::decode::<_, 111>",
    encode_with = "crate::transport::encode::<_, 111>"
)]
fn job_advance(
    (bytes, operation, sequence, work, now, fault, trap_after): (
        Vec<u8>,
        u8,
        u64,
        u32,
        i64,
        u64,
        bool,
    ),
) -> Result<Response, String> {
    let host = runtime();
    let db = open(&host);
    let mut jobs = open_jobs(&host);
    let handle = JobHandle::from_bytes(&bytes).map_err(|e| e.to_string())?;
    let query = query(&db, operation).work_limit(work as usize);
    faults::arm_job_write(fault);
    let result = match operation {
        1 => query
            .resume_sum::<Amount>(&mut jobs, &handle, sequence, Timestamp::from_millis(now))
            .map(|s| progress(s, |value| value.map(|r| r.amount.to_string()))),
        2 => query
            .resume_avg::<AverageAmount>(&mut jobs, &handle, sequence, Timestamp::from_millis(now))
            .map(|s| progress(s, |value| value.map(|r| r.amount.to_string()))),
        0 => query
            .resume_count(&mut jobs, &handle, sequence, Timestamp::from_millis(now))
            .map(|s| progress(s, |n| Some(n.to_string()))),
        3 => query
            .resume_group_count::<Category>(
                &mut jobs,
                &handle,
                sequence,
                Timestamp::from_millis(now),
            )
            .map(|s| {
                progress(s, |rows| {
                    Some(
                        rows.into_iter()
                            .map(|row| format!("{}:{}", row.key.category, row.count))
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                })
            }),
        4 => query
            .resume_group_sum::<Category, Amount>(
                &mut jobs,
                &handle,
                sequence,
                Timestamp::from_millis(now),
            )
            .map(|s| {
                progress(s, |rows| {
                    Some(
                        rows.into_iter()
                            .map(|row| {
                                format!("{}:{}", row.key.category, row.value.unwrap().amount)
                            })
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                })
            }),
        5 => query
            .resume_group_avg::<Category, AverageAmount>(
                &mut jobs,
                &handle,
                sequence,
                Timestamp::from_millis(now),
            )
            .map(|s| {
                progress(s, |rows| {
                    Some(
                        rows.into_iter()
                            .map(|row| {
                                format!("{}:{}", row.key.category, row.value.unwrap().amount)
                            })
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                })
            }),
        _ => Err(ic_db::Error::InvalidQuery),
    };
    let writes = faults::finish_job_write();
    if trap_after {
        ic_platform_runtime::cdk::api::trap("after successful aggregate advance");
    }
    result
        .map(|(sequence, revision, examined, bytes, phase, value)| {
            (sequence, revision, examined, bytes, phase, value, writes)
        })
        .map_err(|e| e.to_string())
}
// operation: 0 cancel; 1 cleanup; 2 mutate source; 3 same-value replacement;
// 4 unrelated entity mutation; 5 mutate source then trap.
#[ic_platform_runtime::cdk::update(
    guard = "crate::transport::admit::<112>",
    decode_with = "crate::transport::decode::<_, 112>",
    encode_with = "crate::transport::encode::<_, 112>"
)]
fn job_control((operation, bytes, now, limit): (u8, Vec<u8>, i64, u32)) -> Result<u64, String> {
    let host = runtime();
    let mut jobs = open_jobs(&host);
    match operation {
        0 => jobs
            .cancel(&JobHandle::from_bytes(&bytes).map_err(|e| e.to_string())?)
            .map(|()| 0),
        1 => jobs
            .cleanup_expired(Timestamp::from_millis(now), limit as usize)
            .map(|n| n as u64),
        2..=5 => {
            let mut db = open(&host);
            let mut batch = Batch::new();
            if operation == 4 {
                let mut row = db.get::<Document>(1).unwrap().unwrap();
                row.published = !row.published;
                batch.replace(&row).unwrap();
            } else {
                let mut row = db.get::<Entry>(1).unwrap().unwrap();
                if operation != 3 {
                    row.amount += 1;
                }
                batch.replace(&row).unwrap();
            }
            let result = db.commit(batch).map(|()| db.revision::<Entry>().unwrap());
            if operation == 5 {
                ic_platform_runtime::cdk::api::trap("after source mutation");
            }
            result
        }
        _ => return Err("invalid operation".into()),
    }
    .map_err(|e| e.to_string())
}
