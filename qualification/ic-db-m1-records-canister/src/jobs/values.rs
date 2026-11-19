//! Variable-size winners through the existing typed engine and job store.
use super::{EntryId, open_jobs, runtime};
use crate::{
    open,
    schema::{BinaryObject, Document, EnvelopeRow},
};
use ic_db::{Batch, GroupCursor, GroupOrder, JobHandle, JobOutcome, Timestamp};

#[ic_db::record(fields(field(name = "id", value(item(prim = "Nat64")))))]
struct Id {}
#[ic_db::record(fields(
    field(name = "id", value(item(prim = "Nat64"))),
    field(name = "a", value(item(prim = "Text", max_bytes = 1024)))
))]
struct IdText {}
#[ic_db::record(fields(field(name = "a", value(item(prim = "Text", max_bytes = 1024)))))]
struct Text {}
#[ic_db::record(fields(field(name = "note", value(opt, item(prim = "Text", max_bytes = 64)))))]
struct Note {}
#[ic_db::record(fields(field(name = "payload", value(item(prim = "Blob", max_bytes = 1024)))))]
struct Payload {}

type WriteRequest = (Vec<(u64, u64, Option<String>)>, Vec<(u64, String)>);

/// Populate the value and text inputs for retained aggregates.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "job_values_write",
    guard = crate::transport::admit::<118>,
    decode = crate::transport::decode::<_, 118>,
    encode = crate::transport::encode::<_, 118>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn job_values_write((documents, texts): WriteRequest) -> Result<(), String> {
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    let result = (|| {
        for (id, entry_id, note) in documents {
            let row = Document {
                id,
                entry_id,
                note,
                title: String::new(),
                published: false,
                score: 0,
                previous_score: None,
                priority: None,
            };
            if db.get::<Document>(id)?.is_some() {
                batch.replace(&row)?;
            } else {
                batch.insert(&row)?;
            }
        }
        for (id, a) in texts {
            let row = EnvelopeRow {
                id,
                a,
                b: String::new(),
                c: String::new(),
                d: String::new(),
                padding: Vec::new(),
            };
            if db.get::<EnvelopeRow>(id)?.is_some() {
                batch.replace(&row)?;
            } else {
                batch.insert(&row)?;
            }
        }
        db.commit(batch)
    })();
    result.map_err(|e: ic_db::Error| e.to_string())
}

// 0/1: optional-text min/max; 2: wide text; 3: Blob max. These are compiled
// fixture operations, not a schema-driven dynamic frontend.

/// Start the selected retained value aggregate.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "job_values_start",
    guard = crate::transport::admit::<119>,
    decode = crate::transport::decode::<_, 119>,
    encode = crate::transport::encode::<_, 119>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn job_values_start((operation, limit): (u8, u32)) -> Result<Vec<u8>, String> {
    let host = runtime();
    let db = open(&host);
    let mut jobs = open_jobs(&host);
    let now = Timestamp::from_millis(0);
    let expires = Timestamp::from_millis(100);
    let query = db
        .query::<Document>()
        .limit(limit as usize)
        .order_groups(GroupOrder::AggregateDescending);
    let result = match operation {
        0 => query.start_group_min::<EntryId, Note>(&mut jobs, now, expires),
        1 => query.start_group_max::<EntryId, Note>(&mut jobs, now, expires),
        2 => db
            .query::<EnvelopeRow>()
            .limit(limit as usize)
            .start_group_max::<IdText, Text>(&mut jobs, now, expires),
        3 => db
            .query::<BinaryObject>()
            .limit(limit as usize)
            .start_group_max::<Id, Payload>(&mut jobs, now, expires),
        _ => Err(ic_db::Error::InvalidQuery),
    };
    result
        .map(|handle| handle.to_bytes())
        .map_err(|e| e.to_string())
}

type AdvanceRequest = (Vec<u8>, u8, u32, u64, u32, u32, Option<Vec<u8>>, u32);
type Response = (u64, u8, Vec<(u64, Option<Vec<u8>>)>, Option<Vec<u8>>, u64);

/// Advance the value aggregate and measure the step.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "job_values_advance",
    guard = crate::transport::admit::<149>,
    decode = crate::transport::decode_measured::<_, 149>,
    encode = crate::transport::encode::<_, 149>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn job_values_advance(
    (request, counter): crate::transport::Measured<AdvanceRequest>,
) -> (Result<Response, String>, u64) {
    let start = counter();
    let result = advance(request).map_err(|e| e.to_string());
    (result, counter() - start)
}

fn advance(
    (bytes, operation, limit, sequence, work, size, after, output_bytes): AdvanceRequest,
) -> Result<Response, ic_db::Error> {
    let host = runtime();
    let db = open(&host);
    let mut jobs = open_jobs(&host);
    let handle = JobHandle::from_bytes(&bytes)?;
    let cursor = after.as_deref().map(GroupCursor::from_bytes).transpose()?;
    let now = Timestamp::from_millis(1);
    let query = db
        .query::<Document>()
        .limit(limit as usize)
        .order_groups(GroupOrder::AggregateDescending)
        .work_limit(work as usize)
        .test_output_limit(output_bytes as usize)
        .group_page(size as usize, cursor.as_ref());
    // Map already-completed typed results to passive bytes for the independent
    // native oracle. No fixture-side comparison, sorting or aggregation.
    match operation {
        0 | 1 => {
            let step = if operation == 0 {
                query.resume_group_min::<EntryId, Note>(&mut jobs, &handle, sequence, now)?
            } else {
                query.resume_group_max::<EntryId, Note>(&mut jobs, &handle, sequence, now)?
            };
            Ok(response(step, |page| {
                (
                    page.rows
                        .into_iter()
                        .map(|row| {
                            (
                                row.key.entry_id,
                                row.value.and_then(|v| v.note).map(String::into_bytes),
                            )
                        })
                        .collect::<Vec<_>>(),
                    page.next,
                )
            }))
        }
        2 => Ok(response(
            db.query::<EnvelopeRow>()
                .limit(limit as usize)
                .work_limit(work as usize)
                .test_output_limit(output_bytes as usize)
                .group_page(size as usize, cursor.as_ref())
                .resume_group_max::<IdText, Text>(&mut jobs, &handle, sequence, now)?,
            |page| {
                (
                    page.rows
                        .into_iter()
                        .map(|row| (row.key.id, row.value.map(|v| v.a.into_bytes())))
                        .collect::<Vec<_>>(),
                    page.next,
                )
            },
        )),
        3 => Ok(response(
            db.query::<BinaryObject>()
                .limit(limit as usize)
                .work_limit(work as usize)
                .test_output_limit(output_bytes as usize)
                .group_page(size as usize, cursor.as_ref())
                .resume_group_max::<Id, Payload>(&mut jobs, &handle, sequence, now)?,
            |page| {
                (
                    page.rows
                        .into_iter()
                        .map(|row| (row.key.id, row.value.map(|v| v.payload)))
                        .collect(),
                    page.next,
                )
            },
        )),
        _ => Err(ic_db::Error::InvalidQuery),
    }
}

fn response<T>(
    step: ic_db::JobStep<T>,
    map: impl FnOnce(T) -> (Vec<(u64, Option<Vec<u8>>)>, Option<GroupCursor>),
) -> Response {
    let (phase, rows, next) = match step.outcome {
        JobOutcome::Running => (0, vec![], None),
        JobOutcome::Complete(value) => {
            let (rows, next) = map(value);
            (1, rows, next.map(|c| c.to_bytes()))
        }
        JobOutcome::Invalidated => (2, vec![], None),
        JobOutcome::Expired => (3, vec![], None),
    };
    (step.sequence, phase, rows, next, step.examined)
}
