#![deny(unsafe_code)]
// Platform narrowly permits the generated IC export attribute; handlers remain safe.

//! Disposable M0 fixture: real Storage collections with explicit failure boundaries.
//! Wire exports are test controls, not product endpoints or a Canic replacement.

mod batch;
mod entrypoints;
mod reads;

use ic_db_qualification_transport as transport;

use ic_storage::{
    AllocationGrants, DefaultMemoryImpl, GenericAllocationPolicy, MemoryAuthority, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot, StableKey, adapter::StableBTreeMap,
};

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
        let memory = host.open_memory(&StableKey::parse(name).unwrap()).unwrap();
        drop(Map::create(memory));
    }
}

// Mode 6 intentionally violates the candidate contract: it returns Err after a row write.
// This negative control proves that an application error is not a rollback instruction.
// mutate
// Exercise rollback at the selected row or mapping write boundary.
// guard  : validates the request frame for method 1.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "mutate",
    guard = crate::transport::admit::<1>,
    decode = crate::transport::decode::<_, 1>,
    encode = crate::transport::encode::<_, 1>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn mutate((mode, id, principal): (u8, u64, u64)) -> Result<(), u8> {
    let host = runtime();
    let rows = open(&host, NAMES[0]);
    let mut unique = open(&host, NAMES[1]);
    let mut order = open(&host, NAMES[2]);
    if mode == 1 || rows.contains_key(&id) || unique.contains_key(&principal) {
        return Err(1);
    }
    // Force a new physical bucket and directory publication, not just row changes.
    let (rows, pages) = rows.allocated_pages();
    let (mut rows, reserved) = rows.reserve_pages(pages + 9);
    reserved.unwrap();
    fail_at(mode, 2);
    rows.insert(id, principal);
    fail_at(mode, 3);
    if mode == 6 {
        return Err(6);
    }
    unique.insert(principal, id);
    fail_at(mode, 4);
    order.insert(id, principal);
    fail_at(mode, 5);
    drop((rows, unique, order, host));
    // A successful database operation followed by an application trap still rolls back.
    fail_at(mode, 7);
    if mode == 8 {
        // Earlier complete batch followed by an application-level refusal.
        return Err(8);
    }

    Ok(())
}

// inspect
// Read the fixture’s persisted state for comparison across calls.
// guard  : validates the request frame for method 2.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "inspect",
    guard = crate::transport::admit::<2>,
    decode = crate::transport::decode::<_, 2>,
    encode = crate::transport::encode::<_, 2>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn inspect((): ()) -> Snapshot {
    let host = runtime();
    let entries = |name| {
        open(&host, name)
            .iter()
            .map(|entry| (*entry.key(), entry.value()))
            .collect()
    };
    let rows = entries(NAMES[0]);
    let unique = entries(NAMES[1]);
    let order = entries(NAMES[2]);

    (
        rows,
        unique,
        order,
        [
            host.committed_generation().unwrap(),
            ic_storage::Memory::size(&DefaultMemoryImpl::default()),
        ],
    )
}

type Map = StableBTreeMap<u64, u64, DefaultMemoryImpl>;
type Entries = Vec<(u64, u64)>;
type Snapshot = (Entries, Entries, Entries, [u64; 2]);

const NAMES: [&str; 3] = ["m0.rows.v1", "m0.unique.v1", "m0.order.v1"];

fn runtime() -> MemoryRuntime<DefaultMemoryImpl> {
    let mut host = MemoryRuntime::new(DefaultMemoryImpl::default()).unwrap();
    let declarations = SealedDeclarationSnapshot::new(
        NAMES
            .iter()
            .map(|name| MemoryRequest::new("m0", name).unwrap())
            .collect(),
    )
    .unwrap();
    let grants = AllocationGrants::new(vec![MemoryAuthority::new("m0", "m0.").unwrap()]).unwrap();
    host.bootstrap(&declarations, &grants, &GenericAllocationPolicy)
        .unwrap();

    host
}

fn open(host: &MemoryRuntime<DefaultMemoryImpl>, name: &str) -> Map {
    Map::load(host.open_memory(&StableKey::parse(name).unwrap()).unwrap())
}

fn fail_at(selected: u8, boundary: u8) {
    if selected == boundary {
        ic_platform_runtime::system::trap("M0 injected commit failure");
    }
}
