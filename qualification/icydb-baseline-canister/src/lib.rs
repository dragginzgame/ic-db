#![forbid(unsafe_code)]

//! Frozen IcyDB measurement host. These Wire exports are private test controls.
//! Startup is advanced explicitly by the harness; no Canic lifecycle is measured.

use ic_db_icydb_baseline_schema::{Entry, EntryInsert, EntryReplace};
use ic_platform_runtime::cdk;
use icydb::{db::WriteCell, types::Id};

icydb::__icydb_start_actor!();

// The harness explicitly drives recovery between calls. Record wakeups so a
// measurement cannot silently continue after a recovery-triggering failure.
mod __icydb_host {
    use std::cell::Cell;
    thread_local! {
        pub(super) static RECOVERY_PENDING: Cell<bool> = const { Cell::new(false) };
    }
    // Generated native test code installs its own wakeup stub.
    #[cfg(not(all(test, not(target_arch = "wasm32"))))]
    pub(crate) fn ensure_startup_watchdog_scheduled_immediately() {
        RECOVERY_PENDING.set(true);
    }
}

fn icydb_storage_grants() -> Result<icydb::db::AllocationGrants, icydb::db::AdmissionError> {
    icydb::db::AllocationGrants::new(vec![icydb::db::MemoryAuthority::new(
        "icydb_icdb_baseline",
        "icydb.icdb_baseline.",
    )?])
}

type Row = (u64, u64, u64);

fn row(value: &Entry) -> Row {
    (value.id, value.category, value.amount)
}

// Includes session creation, request accounting and result conversion. Excludes
// Wire decode/encode. The driver separately records complete update cycle cost.
fn measure<T>(operation: impl FnOnce() -> T) -> (T, u64) {
    assert!(!__icydb_host::RECOVERY_PENDING.get(), "drive startup first");
    let start = cdk::api::instruction_counter();
    let result = icydb::db::with_request_execution(operation);
    (result, cdk::api::instruction_counter() - start)
}

#[ic_wire_endpoint::update(id = 1)]
fn advance_startup() -> (bool, u64) {
    let start = cdk::api::instruction_counter();
    // Deliberately fixed test entropy, never a production identity source.
    icydb::db::provide_boot_entropy([41; 32]).unwrap();
    let ready =
        icydb::db::with_request_execution(|| __icydb_generated::startup_driver_attempt().unwrap());
    if ready {
        __icydb_host::RECOVERY_PENDING.set(false);
    }
    (ready, cdk::api::instruction_counter() - start)
}

#[ic_wire_endpoint::update(id = 2)]
fn insert((id, category, amount): Row) -> (bool, u64) {
    measure(|| {
        let database = db().unwrap();
        let mut batch = database.trusted_typed_write_batch();
        batch
            .push(EntryInsert {
                id: WriteCell::Value(Id::from_key(id)),
                category: WriteCell::Value(category),
                amount: WriteCell::Value(amount),
            })
            .unwrap();
        batch.execute().is_ok()
    })
}

#[ic_wire_endpoint::query(id = 3)]
fn lookup(id: u64) -> (Option<Row>, u64) {
    measure(|| {
        db().unwrap()
            .get::<Entry>(Id::from_key(id))
            .unwrap()
            .as_ref()
            .map(row)
    })
}

#[ic_wire_endpoint::update(id = 4)]
fn read_modify_write((id, category, amount): Row) -> (Option<Row>, u64) {
    measure(|| {
        let database = db().unwrap();
        let mut current = database.get::<Entry>(Id::from_key(id)).unwrap()?;
        current.amount = amount;
        current.category = category;
        let mut batch = database.trusted_typed_write_batch();
        batch
            .push(EntryReplace {
                id: Id::from_key(id),
                category: WriteCell::Value(current.category),
                amount: WriteCell::Value(current.amount),
            })
            .unwrap();
        batch.execute().unwrap();
        Some(row(&current))
    })
}

#[ic_wire_endpoint::query(id = 5)]
fn listing(category: u64) -> (Vec<Row>, u64) {
    measure(|| {
        db().unwrap()
            .query::<Entry>()
            .unwrap()
            .filter(icydb::db::query::FieldRef::new("category").eq(category))
            .order_by(icydb::db::query::asc("id"))
            .limit(10)
            .execute_live_page(None)
            .unwrap()
            .rows
            .iter()
            .map(row)
            .collect()
    })
}
