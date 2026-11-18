//! Cutover witness over the unchanged mapping entity; no host routing authority.
use super::{COPY_SOURCE, COPY_TARGET, CopyMapping, whole_declaration};
use crate::handles::Database;
use crate::{aggregate_store::open_jobs, runtime};
use ic_db::{Error, JobHandle, JobOutcome, QueryCursor, Timestamp};
use ic_storage::StableKey;

// Position bytes, sequence, source revision, examined rows, completed count.
type Reply = (Vec<u8>, u64, u64, u64, Option<u64>);

fn control(
    operation: u8,
    target: bool,
    bytes: &[u8],
    sequence: u64,
    work: u32,
) -> Result<Reply, Error> {
    let host = runtime();
    let (source_schema, target_schema, _) = whole_declaration()?;
    let db = Database::open(
        host.open_memory(
            &StableKey::parse(if target { COPY_TARGET } else { COPY_SOURCE }).unwrap(),
        )
        .unwrap(),
        if target { target_schema } else { source_schema },
    )?;
    let revision = db.revision::<CopyMapping>()?;
    let query = db.query::<CopyMapping>().work_limit(work as usize);
    if operation == 2 {
        let cursor = if bytes.is_empty() {
            None
        } else {
            Some(QueryCursor::from_bytes(bytes)?)
        };
        let query = query.limit(1);
        let page = match &cursor {
            Some(cursor) => query.after(cursor).fetch()?,
            None => query.fetch()?,
        };
        return Ok((
            page.next
                .map(|cursor| cursor.to_bytes())
                .unwrap_or_default(),
            0,
            revision,
            page.rows.len() as u64,
            None,
        ));
    }
    let mut jobs = open_jobs(&host);
    match operation {
        0 => {
            let handle = query.start_count(
                &mut jobs,
                Timestamp::from_millis(0),
                Timestamp::from_millis(100),
            )?;
            Ok((handle.to_bytes(), 0, revision, 0, None))
        }
        1 => {
            let handle = JobHandle::from_bytes(bytes)?;
            let step =
                query.resume_count(&mut jobs, &handle, sequence, Timestamp::from_millis(1))?;
            let count = match step.outcome {
                JobOutcome::Complete(count) => Some(count),
                JobOutcome::Running => None,
                _ => return Err(Error::InvalidJob),
            };
            Ok((
                vec![],
                step.sequence,
                step.source_revision,
                step.examined,
                count,
            ))
        }
        _ => Err(Error::InvalidQuery),
    }
}

/// Exercise retained aggregate custody during schema maintenance.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "maintenance_job",
    guard = crate::transport::admit::<146>,
    decode = crate::transport::decode::<_, 146>,
    encode = crate::transport::encode::<_, 146>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn maintenance_job(
    (operation, target, bytes, sequence, work): (u8, bool, Vec<u8>, u64, u32),
) -> Result<Reply, String> {
    control(operation, target, &bytes, sequence, work).map_err(|error| format!("{error:?}"))
}
