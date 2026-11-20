//! Installed product sums over 32 text groups with canceling Float64 inputs.
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
    field(name = "label", value(item(prim = "Text", max_bytes = 256))),
    field(name = "value", value(item(prim = "Float64")))
))]
struct Sample {}

#[record(fields(field(name = "label", value(item(prim = "Text", max_bytes = 256)))))]
struct Label {}

#[record(fields(field(name = "value", value(item(prim = "Float64")))))]
struct Reading {}

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

#[ic_platform_runtime::init(runtime = ic_platform_runtime, guard = crate::transport::allow,
    decode = crate::transport::empty, max_bytes = 0, on_error = crate::transport::lifecycle_error)]
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

// seed: insert one 32-row batch, bounded to five rounds.
#[ic_platform_runtime::update(runtime = ic_platform_runtime, name = "seed",
    guard = crate::transport::admit::<1>, decode = crate::transport::decode::<_, 1>,
    encode = crate::transport::encode::<_, 1>, max_bytes = 65_536, on_error = crate::transport::endpoint_error)]
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
        batch
            .insert(&Sample {
                id: u64::from(round) * 32 + group,
                label: format!("{group:0width$}", width = usize::from(width)),
                value: Float64::try_new(value).unwrap(),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
}

#[ic_platform_runtime::update(runtime = ic_platform_runtime, name = "start",
    guard = crate::transport::admit::<2>, decode = crate::transport::decode::<_, 2>,
    encode = crate::transport::encode::<_, 2>, max_bytes = 65_536, on_error = crate::transport::endpoint_error)]
fn start(grouped: bool) -> Vec<u8> {
    let (db, mut jobs) = stores();
    let q = db.query::<Sample>();
    let now = Timestamp::from_millis(0);
    let expires = Timestamp::from_millis(100);
    if grouped {
        q.start_group_sum::<Label, Reading>(&mut jobs, now, expires)
    } else {
        q.start_sum::<Reading>(&mut jobs, now, expires)
    }
    .unwrap()
    .to_bytes()
}

type Reply = (u64, u64, Option<Vec<(String, u64)>>);
#[ic_platform_runtime::update(runtime = ic_platform_runtime, name = "advance",
    guard = crate::transport::admit::<3>, decode = crate::transport::decode::<_, 3>,
    encode = crate::transport::encode::<_, 3>, max_bytes = 65_536, on_error = crate::transport::endpoint_error)]
fn advance((grouped, handle, sequence, work): (bool, Vec<u8>, u64, u16)) -> Result<Reply, String> {
    let (db, mut jobs) = stores();
    let q = db.query::<Sample>().limit(32).work_limit(usize::from(work));
    let handle = JobHandle::from_bytes(&handle).unwrap();
    let now = Timestamp::from_millis(1);
    let step = if grouped {
        let s = q
            .resume_group_sum::<Label, Reading>(&mut jobs, &handle, sequence, now)
            .map_err(|e| format!("{e:?}"))?;
        (
            s.sequence,
            s.examined,
            match s.outcome {
                JobOutcome::Running => None,
                JobOutcome::Complete(page) => Some(
                    page.rows
                        .into_iter()
                        .map(|r| (r.key.label, r.value.unwrap().value.get().to_bits()))
                        .collect(),
                ),
                _ => panic!("unexpected phase"),
            },
        )
    } else {
        let s = q
            .resume_sum::<Reading>(&mut jobs, &handle, sequence, now)
            .map_err(|e| format!("{e:?}"))?;
        (
            s.sequence,
            s.examined,
            match s.outcome {
                JobOutcome::Running => None,
                JobOutcome::Complete(value) => {
                    Some(vec![(String::new(), value.unwrap().value.get().to_bits())])
                }
                _ => panic!("unexpected phase"),
            },
        )
    };
    Ok(step)
}
