#![deny(unsafe_code)]
// Platform narrowly permits the generated IC export attribute; handlers remain safe.

//! One fixed, bounded grouped sum to probe revision invalidation and durable progress.
//! This fixture is not a query API, schema admission implementation or job framework.

use ic_db_qualification_transport as transport;

use ic_storage::{
    AllocationGrants, DefaultMemoryImpl, GenericAllocationPolicy, MemoryAuthority, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot, StableKey, adapter::StableBTreeMap,
};

type Map = StableBTreeMap<u64, u64, DefaultMemoryImpl>;
// Phase, job ID, sequence, captured revision, current revision, completed output.
type Response = (u64, u64, u64, u64, u64, Vec<(u64, u64)>);
const NAMES: [&str; 2] = ["aggregate.rows.v1", "aggregate.state.v1"];
const REVISION: u64 = 0;
const JOB: u64 = 1;
const SOURCE: u64 = 2;
const CURSOR: u64 = 3;
const SEQUENCE: u64 = 4;
const PHASE: u64 = 5;
const CONTRACT: u64 = 6;
const SUM: u64 = 7;
const COUNT: u64 = 10;
const RUNNING: u64 = 1;
const COMPLETE: u64 = 2;
const INVALIDATED: u64 = 3;

fn runtime() -> MemoryRuntime<DefaultMemoryImpl> {
    let mut host = MemoryRuntime::new(DefaultMemoryImpl::default()).unwrap();
    let declarations = SealedDeclarationSnapshot::new(
        NAMES
            .iter()
            .map(|name| MemoryRequest::new("aggregate", name).unwrap())
            .collect(),
    )
    .unwrap();
    let grants = AllocationGrants::new(vec![
        MemoryAuthority::new("aggregate", "aggregate.").unwrap(),
    ])
    .unwrap();
    host.bootstrap(&declarations, &grants, &GenericAllocationPolicy)
        .unwrap();
    host
}

fn open(host: &MemoryRuntime<DefaultMemoryImpl>, name: &str) -> Map {
    Map::load(host.open_memory(&StableKey::parse(name).unwrap()).unwrap())
}

/// Initialize the fixture’s stable state at installation.
#[ic_platform_runtime::init(
    runtime = ic_platform_runtime,
    guard = crate::transport::allow,
    decode = crate::transport::empty,
    max_bytes = 0,
    on_error = crate::transport::lifecycle_error
)]
fn initialize((): ()) {
    let host = runtime();
    for name in NAMES {
        drop(Map::create(
            host.open_memory(&StableKey::parse(name).unwrap()).unwrap(),
        ));
    }
    let mut rows = open(&host, NAMES[0]);
    for id in 1..=12 {
        rows.insert(id, id * 10);
    }
    let mut state = open(&host, NAMES[1]);
    for key in 0..=12 {
        state.insert(key, 0);
    }
    // One explicit fixed schema/query/algorithm contract for this disposable fixture.
    state.insert(CONTRACT, 1);
}

fn get(state: &Map, key: u64) -> u64 {
    state.get(&key).unwrap()
}

fn fail_at(selected: u8, boundary: u8) {
    if selected == boundary {
        ic_platform_runtime::system::trap("M0 injected aggregation failure");
    }
}

fn response(state: &Map) -> Response {
    let mut output = Vec::new();
    if get(state, PHASE) == COMPLETE {
        // Finalization visits at most three groups. HAVING and ORDER BY see full sums.
        for group in 0..3 {
            let sum = get(state, SUM + group);
            if get(state, COUNT + group) > 0 && sum > 150 {
                output.push((group, sum));
            }
        }
        output.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    }
    (
        get(state, PHASE),
        get(state, JOB),
        get(state, SEQUENCE),
        get(state, SOURCE),
        get(state, REVISION),
        output,
    )
}

// Commands: start, advance(job, sequence), put(id, value), cancel(job), delete(id).
// Errors: input bound, wrong job, future sequence, not running, capacity, contract.

/// Advance the selected operation and return the resulting fixture state.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "command",
    guard = crate::transport::admit::<1>,
    decode = crate::transport::decode::<_, 1>,
    encode = crate::transport::encode::<_, 1>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn command((operation, a, b, failure): (u8, u64, u64, u8)) -> Result<Response, u8> {
    let host = runtime();
    let mut rows = open(&host, NAMES[0]);
    let mut state = open(&host, NAMES[1]);
    if get(&state, CONTRACT) != 1 {
        return Err(6);
    }
    match operation {
        0 => {
            if get(&state, PHASE) == RUNNING {
                return Err(5);
            }
            let job = get(&state, JOB).checked_add(1).ok_or(5)?;
            for key in [
                CURSOR,
                SEQUENCE,
                SUM,
                SUM + 1,
                SUM + 2,
                COUNT,
                COUNT + 1,
                COUNT + 2,
            ] {
                state.insert(key, 0);
            }
            state.insert(JOB, job);
            state.insert(SOURCE, get(&state, REVISION));
            state.insert(PHASE, RUNNING);
        }
        1 => {
            if a != get(&state, JOB) {
                return Err(2);
            }
            let sequence = get(&state, SEQUENCE);
            if b > sequence {
                return Err(3);
            }
            if get(&state, PHASE) == COMPLETE || get(&state, PHASE) == INVALIDATED {
                return Ok(response(&state));
            }
            if get(&state, PHASE) != RUNNING {
                return Err(4);
            }
            if get(&state, SOURCE) != get(&state, REVISION) {
                state.insert(PHASE, INVALIDATED);
                return Ok(response(&state));
            }
            // Replaying an acknowledged or lost-response request cannot advance twice.
            if b < sequence {
                return Ok(response(&state));
            }
            // Three processed rows plus one bounded lookahead to establish exhaustion.
            let candidates: Vec<_> = rows
                .range((get(&state, CURSOR) + 1)..)
                .take(4)
                .map(|entry| (*entry.key(), entry.value()))
                .collect();
            let mut sums = [get(&state, SUM), get(&state, SUM + 1), get(&state, SUM + 2)];
            let mut counts = [
                get(&state, COUNT),
                get(&state, COUNT + 1),
                get(&state, COUNT + 2),
            ];
            let mut cursor = get(&state, CURSOR);
            for &(id, value) in candidates.iter().take(3) {
                let group = (id % 3) as usize;
                sums[group] = sums[group].checked_add(value).unwrap();
                counts[group] += 1;
                cursor = id;
            }
            for group in 0..3 {
                state.insert(SUM + group, sums[group as usize]);
                fail_at(failure, 1);
                state.insert(COUNT + group, counts[group as usize]);
            }
            fail_at(failure, 2);
            state.insert(CURSOR, cursor);
            fail_at(failure, 3);
            state.insert(SEQUENCE, sequence + 1);
            if candidates.len() <= 3 {
                state.insert(PHASE, COMPLETE);
            }
        }
        2 | 4 => {
            if !(1..=12).contains(&a) || b > 1000 {
                return Err(1);
            }
            let revision = get(&state, REVISION).checked_add(1).ok_or(5)?;
            if operation == 2 {
                rows.insert(a, b);
            } else {
                rows.remove(&a);
            }
            fail_at(failure, 1);
            state.insert(REVISION, revision);
            fail_at(failure, 2);
        }
        3 => {
            if a != get(&state, JOB) {
                return Err(2);
            }
            // Fixed-size slot cleanup is bounded and cannot affect the source revision.
            for key in [
                SOURCE,
                CURSOR,
                SEQUENCE,
                PHASE,
                SUM,
                SUM + 1,
                SUM + 2,
                COUNT,
                COUNT + 1,
                COUNT + 2,
            ] {
                state.insert(key, 0);
            }
        }
        _ => return Err(1),
    }
    Ok(response(&state))
}

/// Read the fixture’s persisted state for comparison across calls.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "inspect",
    guard = crate::transport::admit::<2>,
    decode = crate::transport::decode::<_, 2>,
    encode = crate::transport::encode::<_, 2>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn inspect((): ()) -> Response {
    let host = runtime();
    response(&open(&host, NAMES[1]))
}
