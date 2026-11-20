//! Installed product sums over 32 text groups with canceling Float32 and Float64 inputs.
#![deny(unsafe_code)]

use ic_db::design::prelude::*;
use ic_db::{AggregateJobs, Batch, Database, Entity, JobHandle, JobOutcome};
use ic_db_qualification_transport as transport;
use ic_storage::{
    AllocationGrants, DefaultMemoryImpl, GenericAllocationPolicy, MemoryAuthority, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot, StableKey,
};

#[canister]
struct Owner {}

#[store(canister = Owner, key = "probe.rows.v1")]
struct Rows {}

#[entity(store = Rows, pk(field = "id"), fields(
    field(name = "id", value(item(prim = "Nat64"))),
    field(name = "small", value(item(prim = "Float32"))),
    field(name = "label", value(item(prim = "Text", max_bytes = 256))),
    field(name = "value", value(item(prim = "Float64")))
))]
struct Sample {}

#[record(fields(field(name = "label", value(item(prim = "Text", max_bytes = 256)))))]
struct Label {}

#[record(fields(field(name = "value", value(item(prim = "Float64")))))]
struct Reading {}

#[record(fields(field(name = "small", value(item(prim = "Float32")))))]
struct SmallReading {}

const ROWS: &str = "probe.rows.v1";
const JOBS: &str = "probe.jobs.v1";
fn host() -> MemoryRuntime<DefaultMemoryImpl> {
    let mut host = MemoryRuntime::new(DefaultMemoryImpl::default()).unwrap();
    let requests = [ROWS, JOBS].map(|name| MemoryRequest::new("probe", name).unwrap());
    host.bootstrap(
        &SealedDeclarationSnapshot::new(requests.to_vec()).unwrap(),
        &AllocationGrants::new(vec![MemoryAuthority::new("probe", "probe.").unwrap()]).unwrap(),
        &GenericAllocationPolicy,
    )
    .unwrap();
    host
}
fn schema() -> Schema {
    Schema::new(&[Sample::SCHEMA]).unwrap()
}

// init
// Create the sample database and its retained aggregate job store.
// guard  : allow accepts lifecycle calls and installs the fixture panic hook.
// limits : 0 bytes request, no reply.
#[ic_platform_runtime::init(
    runtime = ic_platform_runtime,
    guard = crate::transport::allow,
    decode = crate::transport::empty,
    max_bytes = 0,
    on_error = crate::transport::lifecycle_error
)]
fn init((): ()) {
    let host = host();
    Database::create(
        host.open_memory(&StableKey::parse(ROWS).unwrap()).unwrap(),
        schema(),
        [1; 16],
    )
    .unwrap();
    AggregateJobs::create(
        host.open_memory(&StableKey::parse(JOBS).unwrap()).unwrap(),
        [2; 16],
    )
    .unwrap();
}

fn stores() -> (Database, AggregateJobs) {
    let host = host();
    (
        Database::open(
            host.open_memory(&StableKey::parse(ROWS).unwrap()).unwrap(),
            schema(),
        )
        .unwrap(),
        AggregateJobs::open(host.open_memory(&StableKey::parse(JOBS).unwrap()).unwrap()).unwrap(),
    )
}

// seed
// Insert one batch of 32 samples for the selected round (0 through 4).
// guard  : validates the request frame for method 1.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "seed",
    guard = crate::transport::admit::<1>,
    decode = crate::transport::decode::<_, 1>,
    encode = crate::transport::encode::<_, 1>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn seed((width, round): (u16, u8)) {
    assert!((2..=256).contains(&width) && round < 5);
    let (mut db, _) = stores();
    let mut batch = Batch::new();
    for group in 0..32_u64 {
        let value = match round {
            0 | 1 => f64::MAX,
            2 => f64::from_bits(group + 1),
            _ => -f64::MAX,
        };
        let small = match round {
            0 | 1 => f32::MAX,
            2 => f32::from_bits(group as u32 + 1),
            _ => -f32::MAX,
        };
        batch
            .insert(&Sample {
                id: u64::from(round) * 32 + group,
                label: format!("{group:0width$}", width = usize::from(width)),
                small: Float32::try_new(small).unwrap(),
                value: Float64::try_new(value).unwrap(),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
}

// start
// Start a scalar or grouped sum or average and return its job handle.
// guard  : validates the request frame for method 2.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "start",
    guard = crate::transport::admit::<2>,
    decode = crate::transport::decode::<_, 2>,
    encode = crate::transport::encode::<_, 2>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn start((grouped, average, small): (bool, bool, bool)) -> Vec<u8> {
    if small {
        start_job::<SmallReading>(grouped, average)
    } else {
        start_job::<Reading>(grouped, average)
    }
}

fn start_job<R: Value>(grouped: bool, average: bool) -> Vec<u8> {
    let (db, mut jobs) = stores();
    let q = db.query::<Sample>();
    let now = Timestamp::from_millis(0);
    let expires = Timestamp::from_millis(100);
    match (grouped, average) {
        (true, true) => q.start_group_avg::<Label, R>(&mut jobs, now, expires),
        (false, true) => q.start_avg::<R>(&mut jobs, now, expires),
        (true, false) => q.start_group_sum::<Label, R>(&mut jobs, now, expires),
        (false, false) => q.start_sum::<R>(&mut jobs, now, expires),
    }
    .unwrap()
    .to_bytes()
}

type Reply = (u64, u64, Option<Vec<(String, u64)>>);
// advance
// Examine up to the requested work limit and return progress or completed float results.
// guard  : validates the request frame for method 3.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "advance",
    guard = crate::transport::admit::<3>,
    decode = crate::transport::decode::<_, 3>,
    encode = crate::transport::encode::<_, 3>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn advance(
    (grouped, average, small, handle, sequence, work): (bool, bool, bool, Vec<u8>, u64, u16),
) -> Result<Reply, String> {
    if small {
        advance_job(
            grouped,
            average,
            &handle,
            sequence,
            work,
            |r: SmallReading| u64::from(r.small.get().to_bits()),
        )
    } else {
        advance_job(grouped, average, &handle, sequence, work, |r: Reading| {
            r.value.get().to_bits()
        })
    }
}

fn advance_job<R: Value>(
    grouped: bool,
    average: bool,
    handle: &[u8],
    sequence: u64,
    work: u16,
    bits: impl Fn(R) -> u64,
) -> Result<Reply, String> {
    let (db, mut jobs) = stores();
    let q = db.query::<Sample>().limit(32).work_limit(usize::from(work));
    let handle = JobHandle::from_bytes(handle).unwrap();
    let now = Timestamp::from_millis(1);
    let step = if grouped {
        let s = if average {
            q.resume_group_avg::<Label, R>(&mut jobs, &handle, sequence, now)
        } else {
            q.resume_group_sum::<Label, R>(&mut jobs, &handle, sequence, now)
        }
        .map_err(|e| format!("{e:?}"))?;
        (
            s.sequence,
            s.examined,
            match s.outcome {
                JobOutcome::Running => None,
                JobOutcome::Complete(page) => Some(
                    page.rows
                        .into_iter()
                        .map(|r| (r.key.label, bits(r.value.unwrap())))
                        .collect(),
                ),
                JobOutcome::Invalidated => return Err("Invalidated".into()),
                JobOutcome::Expired => return Err("Expired".into()),
            },
        )
    } else {
        let s = if average {
            q.resume_avg::<R>(&mut jobs, &handle, sequence, now)
        } else {
            q.resume_sum::<R>(&mut jobs, &handle, sequence, now)
        }
        .map_err(|e| format!("{e:?}"))?;
        (
            s.sequence,
            s.examined,
            match s.outcome {
                JobOutcome::Running => None,
                JobOutcome::Complete(value) => Some(vec![(String::new(), bits(value.unwrap()))]),
                JobOutcome::Invalidated => return Err("Invalidated".into()),
                JobOutcome::Expired => return Err("Expired".into()),
            },
        )
    };
    Ok(step)
}

// start_summary
// Start a Float64 summary for up to 32 groups and return its job handle.
// guard  : validates the request frame for method 4.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "start_summary",
    guard = crate::transport::admit::<4>,
    decode = crate::transport::decode::<_, 4>,
    encode = crate::transport::encode::<_, 4>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn start_summary((): ()) -> Vec<u8> {
    let (db, mut jobs) = stores();
    db.query::<Sample>()
        .limit(32)
        .filter_summary(ic_db::SummaryMeasure::Count, ic_db::Comparison::Eq, 5_u64)
        .order_groups(ic_db::GroupOrder::Summary(
            ic_db::SummaryMeasure::Average,
            ic_db::SortDirection::Descending,
        ))
        .start_group_summary::<Label, Reading, Reading>(
            &mut jobs,
            Timestamp::from_millis(0),
            Timestamp::from_millis(100),
        )
        .unwrap()
        .to_bytes()
}

type SummaryPage = (Vec<(String, u64, u64, u64)>, Option<Vec<u8>>);
type SummaryReply = (u64, u64, Option<SummaryPage>);

// advance_summary
// Advance within the requested work limit or return up to seven completed groups after the cursor.
// guard  : validates the request frame for method 5.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "advance_summary",
    guard = crate::transport::admit::<5>,
    decode = crate::transport::decode::<_, 5>,
    encode = crate::transport::encode::<_, 5>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn advance_summary(
    (handle, sequence, work, after): (Vec<u8>, u64, u16, Option<Vec<u8>>),
) -> SummaryReply {
    let (db, mut jobs) = stores();
    let cursor = after
        .as_deref()
        .map(ic_db::GroupCursor::from_bytes)
        .transpose()
        .unwrap();
    let step = db
        .query::<Sample>()
        .limit(32)
        .filter_summary(ic_db::SummaryMeasure::Count, ic_db::Comparison::Eq, 5_u64)
        .order_groups(ic_db::GroupOrder::Summary(
            ic_db::SummaryMeasure::Average,
            ic_db::SortDirection::Descending,
        ))
        .work_limit(usize::from(work))
        .group_page(7, cursor.as_ref())
        .resume_group_summary::<Label, Reading, Reading>(
            &mut jobs,
            &JobHandle::from_bytes(&handle).unwrap(),
            sequence,
            Timestamp::from_millis(1),
        )
        .unwrap();
    let page = match step.outcome {
        JobOutcome::Running => None,
        JobOutcome::Complete(page) => Some((
            page.rows
                .into_iter()
                .map(|row| {
                    (
                        row.key.label,
                        row.count,
                        row.sum.unwrap().value.get().to_bits(),
                        row.average.unwrap().value.get().to_bits(),
                    )
                })
                .collect(),
            page.next.map(|c| c.to_bytes()),
        )),
        other => panic!("unexpected summary outcome: {other:?}"),
    };
    (step.sequence, step.examined, page)
}
