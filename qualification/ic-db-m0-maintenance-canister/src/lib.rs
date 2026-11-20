#![deny(unsafe_code)]
// Platform narrowly permits the generated IC export attribute; handlers remain safe.

//! Fixed six-row copy/index migration, plus read-only controls for the M0 console.
//! No production manifest, query engine or general migration executor is implemented.

mod capacity;

use ic_db_qualification_transport as transport;

use ic_storage::{
    AllocationGrants, DefaultMemoryImpl, GenericAllocationPolicy, MemoryAuthority, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot, StableKey, adapter::StableBTreeMap,
};

// initialize
// Initialize the fixture’s stable state at installation.
// guard  : allow accepts lifecycle calls and installs the fixture panic hook.
// limits : 0 bytes request, no reply.
#[ic_platform_runtime::init(
    runtime = ic_platform_runtime,
    guard = crate::transport::allow,
    decode = crate::transport::empty,
    max_bytes = 0,
    on_error = crate::transport::lifecycle_error
)]
fn initialize((): ()) {
    let host = runtime();
    for slot in [0, 1, 3] {
        drop(Map::create(
            host.open_memory(&StableKey::parse(KEYS[slot]).unwrap())
                .unwrap(),
        ));
    }
    drop(Target::create(
        host.open_memory(&StableKey::parse(KEYS[2]).unwrap())
            .unwrap(),
    ));
    let mut control = map(&host, 0);
    for key in 0..=5 {
        control.insert(key, 0);
    }
    control.insert(FORMAT, 1);
    let mut source = map(&host, 1);
    for id in 1..=6 {
        source.insert(id, id * 10 + id % 2);
    }
}

// post_upgrade
// Trap during upgrade when the bad-upgrade fixture is installed.
// guard  : allow accepts lifecycle calls and installs the fixture panic hook.
// limits : 0 bytes request, no reply.
#[ic_platform_runtime::post_upgrade(
    runtime = ic_platform_runtime,
    guard = crate::transport::allow,
    decode = crate::transport::empty,
    max_bytes = 0,
    on_error = crate::transport::lifecycle_error
)]
fn post_upgrade((): ()) {
    if cfg!(feature = "bad-upgrade") {
        ic_platform_runtime::system::trap("M0 injected upgrade failure");
    }
}

// inspect
// Read the fixture’s persisted state for comparison across calls.
// guard  : validates the request frame for method 1.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "inspect",
    guard = crate::transport::admit::<1>,
    decode = crate::transport::decode::<_, 1>,
    encode = crate::transport::encode::<_, 1>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn inspect((): ()) -> Status {
    status(&runtime())
}

// schema
// Inspect the schema admitted by the installed fixture.
// guard  : validates the request frame for method 2.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "schema",
    guard = crate::transport::admit::<2>,
    decode = crate::transport::decode::<_, 2>,
    encode = crate::transport::encode::<_, 2>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn schema((): ()) -> (Status, String) {
    let description = if SCHEMA == 1 {
        "M0 SourceRow v1: id:u64 primary key; value:u64; IDs 1..6. No secondary indexes."
    } else {
        "M0 TargetRow v2: id:u64 primary key; normalized:u64 unique index; original:u64; 16-byte row encoding."
    };

    (status(&runtime()), description.to_string())
}

// Three candidates maximum per page, including residual nonmatches; IDs are bounded.

// query
// Read the requested bounded result from the local fixture.
// guard  : validates the request frame for method 3.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "query",
    guard = crate::transport::admit::<3>,
    decode = crate::transport::decode::<_, 3>,
    encode = crate::transport::encode::<_, 3>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn query((after, limit, minimum): (u64, u64, u64)) -> Result<Page, u8> {
    if after > 6 || !(1..=3).contains(&limit) {
        return Err(5);
    }
    let host = runtime();
    admitted(&map(&host, 0))?;
    let candidates: Vec<_> = if SCHEMA == 1 {
        map(&host, 1)
            .range((after + 1)..)
            .take(limit as usize)
            .map(|entry| (*entry.key(), entry.value()))
            .collect()
    } else {
        target(&host)
            .range((after + 1)..)
            .take(limit as usize)
            .map(|entry| (*entry.key(), normalized(entry.value())))
            .collect()
    };
    let scanned = candidates.len() as u64;
    // A full page may require one final empty call to establish completion.
    let next = if scanned == limit {
        candidates.last().map(|entry| entry.0)
    } else {
        None
    };

    Ok((
        candidates
            .into_iter()
            .filter(|(_, value)| *value >= minimum)
            .collect(),
        next,
        scanned,
    ))
}

// start, copy, publish, cleanup, cancel, cancel-cleanup, source-write, damage-manifest.
// Errors: missing manifest=1, incompatible schema=2, closed phase=3,
// unsupported transformation=4, invalid bounds=5, unique/validation refusal=6.

// command
// Advance the selected operation and return the resulting fixture state.
// guard  : validates the request frame for method 4.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "command",
    guard = crate::transport::admit::<4>,
    decode = crate::transport::decode::<_, 4>,
    encode = crate::transport::encode::<_, 4>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn command((operation, a, b, failure): (u8, u64, u64, u8)) -> Result<Status, u8> {
    let host = runtime();
    let mut control = map(&host, 0);
    let format = field(&control, FORMAT)?;
    let phase = field(&control, PHASE)?;
    if !matches!(format, 1 | 2) {
        return Err(2);
    }
    let mut source = map(&host, 1);
    let mut destination = target(&host);
    let mut index = map(&host, 3);
    match operation {
        0 => {
            if SCHEMA != 2 || format != 1 {
                return Err(2);
            }
            if phase != 0 || !destination.is_empty() || !index.is_empty() {
                return Err(3);
            }
            control.insert(PHASE, 1);
            fail_at(failure, 1);
            control.insert(TRANSFORMATION, TRANSFORM);
            control.insert(TARGET_FORMAT, 2);
            control.insert(CURSOR, 0);
        }
        1 => {
            supported(&control)?;
            if phase != 1 || format != 1 {
                return Err(3);
            }
            let rows: Vec<_> = source
                .range((field(&control, CURSOR)? + 1)..)
                .take(3)
                .map(|entry| (*entry.key(), encode(entry.value())))
                .collect();
            let pending: Vec<_> = rows.iter().take(2).copied().collect();
            for (position, (_, bytes)) in pending.iter().enumerate() {
                let key = normalized(*bytes);
                if index.contains_key(&key)
                    || pending[..position]
                        .iter()
                        .any(|(_, old)| normalized(*old) == key)
                {
                    return Err(6);
                }
            }
            for (id, bytes) in pending {
                destination.insert(id, bytes);
                fail_at(failure, 2);
                index.insert(normalized(bytes), id);
                fail_at(failure, 3);
                control.insert(CURSOR, id);
                fail_at(failure, 4);
            }
            if rows.len() <= 2 {
                control.insert(PHASE, 2);
            }
        }
        2 => {
            supported(&control)?;
            if phase != 2 || format != 1 {
                return Err(3);
            }
            // Full dependency validation is bounded by this fixture's six-row input.
            if source.len() != destination.len() || source.len() != index.len() {
                return Err(6);
            }
            for entry in source.iter() {
                let bytes = encode(entry.value());
                if destination.get(entry.key()) != Some(bytes)
                    || index.get(&normalized(bytes)) != Some(*entry.key())
                {
                    return Err(6);
                }
            }
            control.insert(FORMAT, 2);
            fail_at(failure, 5);
            control.insert(RETIRED, 1); // Recorded source allocation slot, never the live target.
            control.insert(PHASE, 4);
        }
        3 => {
            supported(&control)?;
            if phase != 4 || format != 2 || field(&control, RETIRED)? != 1 {
                return Err(3);
            }
            let ids: Vec<_> = source.iter().take(2).map(|entry| *entry.key()).collect();
            for id in ids {
                source.remove(&id);
                fail_at(failure, 6);
            }
            if source.is_empty() {
                for key in [PHASE, TRANSFORMATION, CURSOR, TARGET_FORMAT, RETIRED] {
                    control.insert(key, 0);
                }
                fail_at(failure, 7);
            }
        }
        4 => {
            supported(&control)?;
            if format != 1 || !matches!(phase, 1 | 2) {
                return Err(3);
            }
            control.insert(PHASE, 3);
        }
        5 => {
            supported(&control)?;
            if phase != 3 || format != 1 {
                return Err(3);
            }
            let rows: Vec<_> = destination
                .iter()
                .take(2)
                .map(|entry| (*entry.key(), entry.value()))
                .collect();
            for (id, bytes) in rows {
                destination.remove(&id);
                fail_at(failure, 6);
                index.remove(&normalized(bytes));
            }
            if destination.is_empty() {
                for key in [PHASE, TRANSFORMATION, CURSOR, TARGET_FORMAT, RETIRED] {
                    control.insert(key, 0);
                }
            }
        }
        6 => {
            admitted(&control)?;
            if SCHEMA != 1 || !(1..=6).contains(&a) || b > 100 {
                return Err(5);
            }
            source.insert(a, b);
        }
        7 => {
            // Deliberate corruption control for rejection tests, never exposed by the CLI.
            if a == 0 {
                control.remove(&FORMAT);
            } else {
                control.insert(FORMAT, a);
            }
        }
        _ => return Err(5),
    }
    drop((source, destination, index, control));

    Ok(status(&host))
}

type Map = StableBTreeMap<u64, u64, DefaultMemoryImpl>;
type Target = StableBTreeMap<u64, [u8; 16], DefaultMemoryImpl>;
// Executable schema, stored schema, phase, transformation, copy cursor,
// source/target/index lengths, physical pages.
type Status = [u64; 9];
type Page = (Vec<(u64, u64)>, Option<u64>, u64);
const KEYS: [&str; 4] = [
    "migration.control.v1",
    "migration.source.v1",
    "migration.target.v2",
    "migration.index.v2",
];
const SCHEMA: u64 = if cfg!(feature = "target") { 2 } else { 1 };
const TRANSFORM: u64 = if cfg!(feature = "changed-transform") {
    8
} else {
    7
};
const FORMAT: u64 = 0;
const PHASE: u64 = 1;
const TRANSFORMATION: u64 = 2;
const CURSOR: u64 = 3;
const TARGET_FORMAT: u64 = 4;
const RETIRED: u64 = 5;

// Phases: live, copying, ready, cancelling, published/cleaning.
fn runtime() -> MemoryRuntime<DefaultMemoryImpl> {
    let mut host = MemoryRuntime::new(DefaultMemoryImpl::default()).unwrap();
    let declarations = SealedDeclarationSnapshot::new(
        KEYS.iter()
            .map(|key| MemoryRequest::new("migration", key).unwrap())
            .collect(),
    )
    .unwrap();
    let grants = AllocationGrants::new(vec![
        MemoryAuthority::new("migration", "migration.").unwrap(),
    ])
    .unwrap();
    host.bootstrap(&declarations, &grants, &GenericAllocationPolicy)
        .unwrap();

    host
}

fn map(host: &MemoryRuntime<DefaultMemoryImpl>, slot: usize) -> Map {
    Map::load(
        host.open_memory(&StableKey::parse(KEYS[slot]).unwrap())
            .unwrap(),
    )
}

fn target(host: &MemoryRuntime<DefaultMemoryImpl>) -> Target {
    Target::load(
        host.open_memory(&StableKey::parse(KEYS[2]).unwrap())
            .unwrap(),
    )
}

fn field(control: &Map, key: u64) -> Result<u64, u8> {
    control.get(&key).ok_or(1)
}

fn admitted(control: &Map) -> Result<(), u8> {
    if !matches!(field(control, PHASE)?, 0 | 4) {
        return Err(3);
    }
    if field(control, FORMAT)? != SCHEMA {
        return Err(2);
    }

    Ok(())
}

fn supported(control: &Map) -> Result<(), u8> {
    if SCHEMA != 2
        || field(control, TRANSFORMATION)? != TRANSFORM
        || field(control, TARGET_FORMAT)? != 2
    {
        return Err(4);
    }

    Ok(())
}

fn encode(value: u64) -> [u8; 16] {
    let normalized = value / 10 + if TRANSFORM == 8 { 100 } else { 0 };
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&normalized.to_le_bytes());
    bytes[8..].copy_from_slice(&value.to_le_bytes());

    bytes
}

fn normalized(bytes: [u8; 16]) -> u64 {
    u64::from_le_bytes(bytes[..8].try_into().unwrap())
}

fn fail_at(selected: u8, boundary: u8) {
    if selected == boundary {
        ic_platform_runtime::system::trap("M0 injected maintenance failure");
    }
}

fn status(host: &MemoryRuntime<DefaultMemoryImpl>) -> Status {
    let control = map(host, 0);

    [
        SCHEMA,
        control.get(&FORMAT).unwrap_or(0),
        control.get(&PHASE).unwrap_or(0),
        control.get(&TRANSFORMATION).unwrap_or(0),
        control.get(&CURSOR).unwrap_or(0),
        map(host, 1).len(),
        target(host).len(),
        map(host, 3).len(),
        ic_storage::Memory::size(&ic_storage::DefaultMemoryImpl::default()),
    ]
}
