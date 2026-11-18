//! Feature-selected product-job witness, sharing the ordinary records and supplier.
mod values;

use super::{
    Backing,
    aggregate_store::open_jobs,
    faults, open, runtime,
    schema::{Document, Entry},
};
use ic_db::{
    Batch, Comparison, Database, GroupOrder, JobHandle, JobOutcome, JobStep, Query, Timestamp,
};

#[ic_db::record(fields(field(name = "amount", value(item(prim = "Nat64")))))]
struct Amount {}
#[ic_db::record(fields(
    field(name = "id", value(item(prim = "Nat64"))),
    field(name = "amount", value(item(prim = "Nat64")))
))]
struct IdAmount {}
#[ic_db::record(fields(field(name = "amount", value(item(prim = "Decimal", scale = 2)))))]
struct AverageAmount {}
#[ic_db::record(fields(field(name = "category", value(item(prim = "Nat64")))))]
struct Category {}
#[ic_db::record(fields(field(name = "entry_id", value(item(prim = "Nat64")))))]
struct EntryId {}

fn query(db: &Database<Backing>, operation: u8) -> Query<'_, Entry, Backing> {
    let query = db.query::<Entry>();
    if operation == 3 {
        query
            .filter_groups_any(&[(Comparison::Ge, 2_u64.into())])
            .order_groups(GroupOrder::AggregateDescending)
    } else if operation == 14 {
        query.filter_groups_any(&[
            (Comparison::Eq, 0_u64.into()),
            (Comparison::Eq, 3_u64.into()),
        ])
    } else if operation == 15 {
        query
            .filter_groups(Comparison::Eq, 0_u64)
            .filter_groups(Comparison::Eq, 3_u64)
    } else if operation == 18 {
        query.filter_fields("id", Comparison::Gt, "category")
    } else if operation == 19 {
        query.filter_fields("id", Comparison::Gt, "amount")
    } else if matches!(operation, 12 | 13) {
        query.order_groups(GroupOrder::AggregateDescending)
    } else {
        query
    }
}

type Response = (u64, u64, u64, u64, u8, Option<String>, u64);
type AdvanceRequest = (Vec<u8>, u8, u64, u32, i64, u64, bool);
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
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "job_start",
    guard = crate::transport::admit::<110>,
    decode = crate::transport::decode::<_, 110>,
    encode = crate::transport::encode::<_, 110>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
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
        0 | 18 | 19 => query.start_count(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        3 | 14 | 15 => query.start_group_count::<Category>(
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
        6 => db.query::<Document>().start_count_field(
            "note",
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        7 => db.query::<Document>().start_group_count_field::<EntryId>(
            "note",
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        8 => query.start_count_distinct::<Category>(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        9 => query.start_group_count_distinct::<Category, Category>(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        16 => query.start_group_count_distinct::<Category, Amount>(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        17 => query.start_group_count_distinct::<Category, IdAmount>(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        10 => query.start_min::<Amount>(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        11 => query.start_max::<Amount>(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        12 => query.start_group_min::<Category, Amount>(
            &mut jobs,
            Timestamp::from_millis(now),
            Timestamp::from_millis(expires),
        ),
        13 => query.start_group_max::<Category, Amount>(
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
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "job_advance",
    guard = crate::transport::admit::<111>,
    decode = crate::transport::decode::<_, 111>,
    encode = crate::transport::encode::<_, 111>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn job_advance(
    (bytes, operation, sequence, work, now, fault, trap_after): AdvanceRequest,
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
        10 => query
            .resume_min::<Amount>(&mut jobs, &handle, sequence, Timestamp::from_millis(now))
            .map(|s| progress(s, |value| value.map(|r| r.amount.to_string()))),
        11 => query
            .resume_max::<Amount>(&mut jobs, &handle, sequence, Timestamp::from_millis(now))
            .map(|s| progress(s, |value| value.map(|r| r.amount.to_string()))),
        0 | 18 | 19 => query
            .resume_count(&mut jobs, &handle, sequence, Timestamp::from_millis(now))
            .map(|s| progress(s, |n| Some(n.to_string()))),
        3 | 14 | 15 => query
            .resume_group_count::<Category>(
                &mut jobs,
                &handle,
                sequence,
                Timestamp::from_millis(now),
            )
            .map(|s| {
                progress(s, |rows| {
                    Some(
                        rows.rows
                            .into_iter()
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
                        rows.rows
                            .into_iter()
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
                        rows.rows
                            .into_iter()
                            .map(|row| {
                                format!("{}:{}", row.key.category, row.value.unwrap().amount)
                            })
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                })
            }),
        6 => db
            .query::<Document>()
            .work_limit(work as usize)
            .resume_count_field(
                "note",
                &mut jobs,
                &handle,
                sequence,
                Timestamp::from_millis(now),
            )
            .map(|s| progress(s, |count| Some(count.to_string()))),
        7 => db
            .query::<Document>()
            .work_limit(work as usize)
            .resume_group_count_field::<EntryId>(
                "note",
                &mut jobs,
                &handle,
                sequence,
                Timestamp::from_millis(now),
            )
            .map(|s| {
                progress(s, |rows| {
                    Some(
                        rows.rows
                            .into_iter()
                            .map(|row| format!("{}:{}", row.key.entry_id, row.count))
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                })
            }),
        8 => query
            .resume_count_distinct::<Category>(
                &mut jobs,
                &handle,
                sequence,
                Timestamp::from_millis(now),
            )
            .map(|s| progress(s, |count| Some(count.to_string()))),
        9 | 16 | 17 => (if operation == 9 {
            query.resume_group_count_distinct::<Category, Category>(
                &mut jobs,
                &handle,
                sequence,
                Timestamp::from_millis(now),
            )
        } else if operation == 16 {
            query.resume_group_count_distinct::<Category, Amount>(
                &mut jobs,
                &handle,
                sequence,
                Timestamp::from_millis(now),
            )
        } else {
            query.resume_group_count_distinct::<Category, IdAmount>(
                &mut jobs,
                &handle,
                sequence,
                Timestamp::from_millis(now),
            )
        })
        .map(|s| {
            progress(s, |rows| {
                Some(
                    rows.rows
                        .into_iter()
                        .map(|row| format!("{}:{}", row.key.category, row.count))
                        .collect::<Vec<_>>()
                        .join(","),
                )
            })
        }),
        _ => Err(ic_db::Error::InvalidQuery),
    };
    let writes = faults::finish_job_write();
    if trap_after {
        ic_platform_runtime::system::trap("after successful aggregate advance");
    }
    result
        .map(|(sequence, revision, examined, bytes, phase, value)| {
            (sequence, revision, examined, bytes, phase, value, writes)
        })
        .map_err(|e| e.to_string())
}

// Measurements wrap the same fixture operations, including reopen and refusals.
// Transport decoding/encoding is outside the instruction interval; balance
// deltas in the driver include the whole update. No product telemetry is added.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "job_advance_cost",
    guard = crate::transport::admit::<115>,
    decode = crate::transport::decode::<_, 115>,
    encode = crate::transport::encode::<_, 115>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn job_advance_cost(request: AdvanceRequest) -> (Result<Response, String>, u64) {
    let start = ic_platform_runtime::raw::instruction_counter();
    let result = job_advance(request);
    (
        result,
        ic_platform_runtime::raw::instruction_counter() - start,
    )
}

#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "job_page_cost",
    guard = crate::transport::admit::<116>,
    decode = crate::transport::decode::<_, 116>,
    encode = crate::transport::encode::<_, 116>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn job_page_cost(request: PageRequest) -> (Result<PageResponse, String>, u64) {
    let start = ic_platform_runtime::raw::instruction_counter();
    let result = job_page(request);
    (
        result,
        ic_platform_runtime::raw::instruction_counter() - start,
    )
}
// operation: 0 cancel; 1 cleanup; 2 mutate source; 3 same-value replacement;
// 4 unrelated entity mutation; 5 mutate source then trap.
type ControlRequest = (u8, Vec<u8>, i64, u32);
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "job_control",
    guard = crate::transport::admit::<112>,
    decode = crate::transport::decode::<_, 112>,
    encode = crate::transport::encode::<_, 112>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn job_control((operation, bytes, now, limit): ControlRequest) -> Result<u64, String> {
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
                ic_platform_runtime::system::trap("after source mutation");
            }
            result
        }
        _ => return Err("invalid operation".into()),
    }
    .map_err(|e| e.to_string())
}

// Observe the existing lifecycle operations and inject rollback faults only in
// this qualification host. Cleanup policy stays in AggregateJobs.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "job_control_cost",
    guard = crate::transport::admit::<117>,
    decode = crate::transport::decode::<_, 117>,
    encode = crate::transport::encode::<_, 117>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn job_control_cost(
    (request, fault, trap_after): (ControlRequest, u64, bool),
) -> (Result<u64, String>, u64, u64) {
    let start = ic_platform_runtime::raw::instruction_counter();
    faults::arm_job_write(fault);
    let result = job_control(request);
    let writes = faults::finish_job_write();
    if trap_after {
        ic_platform_runtime::system::trap("after aggregate lifecycle operation");
    }
    (
        result,
        ic_platform_runtime::raw::instruction_counter() - start,
        writes,
    )
}

// Private qualification endpoint; product endpoints remain adapter-owned.
type PageRequest = (Vec<u8>, u64, Option<Vec<u8>>, u32, u32, i64, u64, bool);
type PageResponse = (u64, u8, Vec<(u64, u64)>, Option<Vec<u8>>, u64);
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "job_page",
    guard = crate::transport::admit::<113>,
    decode = crate::transport::decode::<_, 113>,
    encode = crate::transport::encode::<_, 113>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn job_page(
    (handle, sequence, after, size, bytes, now, fault, trap_after): PageRequest,
) -> Result<PageResponse, String> {
    let host = runtime();
    let db = open(&host);
    let mut jobs = open_jobs(&host);
    let handle = JobHandle::from_bytes(&handle).map_err(|e| e.to_string())?;
    let cursor = after
        .as_deref()
        .map(ic_db::GroupCursor::from_bytes)
        .transpose()
        .map_err(|e| e.to_string())?;
    faults::arm_job_write(fault);
    let result = query(&db, 3)
        .group_page(size as usize, cursor.as_ref())
        .byte_limit(bytes as usize)
        .resume_group_count::<Category>(&mut jobs, &handle, sequence, Timestamp::from_millis(now));
    faults::finish_job_write();
    if trap_after {
        ic_platform_runtime::system::trap("after successful group page");
    }
    let step = result.map_err(|e| e.to_string())?;
    let (phase, rows, next) = match step.outcome {
        JobOutcome::Running => (0, vec![], None),
        JobOutcome::Complete(page) => (
            1,
            page.rows
                .into_iter()
                .map(|row| (row.key.category, row.count))
                .collect(),
            page.next.map(|c| c.to_bytes()),
        ),
        JobOutcome::Invalidated => (2, vec![], None),
        JobOutcome::Expired => (3, vec![], None),
    };
    Ok((step.sequence, phase, rows, next, step.source_revision))
}

// Grouped extrema use the same query/result page boundary, with optional winners.
type ExtremumRequest = (Vec<u8>, u8, u64, u32, Option<Vec<u8>>, u32, i64, u64, bool);
type ExtremumResponse = (u64, u8, Vec<(u64, Option<u64>)>, Option<Vec<u8>>, u64, u64);
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "job_group_extremum",
    guard = crate::transport::admit::<114>,
    decode = crate::transport::decode::<_, 114>,
    encode = crate::transport::encode::<_, 114>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn job_group_extremum(
    (handle, operation, sequence, work, after, size, now, fault, trap_after): ExtremumRequest,
) -> Result<ExtremumResponse, String> {
    let host = runtime();
    let db = open(&host);
    let mut jobs = open_jobs(&host);
    let handle = JobHandle::from_bytes(&handle).map_err(|e| e.to_string())?;
    let cursor = after
        .as_deref()
        .map(ic_db::GroupCursor::from_bytes)
        .transpose()
        .map_err(|e| e.to_string())?;
    let query = query(&db, operation)
        .work_limit(work as usize)
        .group_page(size as usize, cursor.as_ref());
    faults::arm_job_write(fault);
    let result = match operation {
        12 => query.resume_group_min::<Category, Amount>(
            &mut jobs,
            &handle,
            sequence,
            Timestamp::from_millis(now),
        ),
        13 => query.resume_group_max::<Category, Amount>(
            &mut jobs,
            &handle,
            sequence,
            Timestamp::from_millis(now),
        ),
        _ => Err(ic_db::Error::InvalidQuery),
    };
    faults::finish_job_write();
    if trap_after {
        ic_platform_runtime::system::trap("after grouped extremum advance");
    }
    let step = result.map_err(|e| e.to_string())?;
    let (phase, rows, next) = match step.outcome {
        JobOutcome::Running => (0, vec![], None),
        JobOutcome::Complete(page) => (
            1,
            page.rows
                .into_iter()
                .map(|row| (row.key.category, row.value.map(|v| v.amount)))
                .collect(),
            page.next.map(|cursor| cursor.to_bytes()),
        ),
        JobOutcome::Invalidated => (2, vec![], None),
        JobOutcome::Expired => (3, vec![], None),
    };
    Ok((
        step.sequence,
        phase,
        rows,
        next,
        step.source_revision,
        step.examined,
    ))
}
