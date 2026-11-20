#![deny(unsafe_code)]
// Platform narrowly permits the generated IC export attribute; handlers remain safe.

//! Frozen IcyDB measurement host. These Wire exports are private test controls.
//! Startup is advanced explicitly by the harness; no Canic lifecycle is measured.

use ic_db_qualification_transport as transport;

use ic_db_icydb_baseline_schema::{Entry, EntryInsert, EntryReplace};
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
    let start = ic_platform_runtime::raw::instruction_counter();
    let result = icydb::db::with_request_execution(operation);
    (
        result,
        ic_platform_runtime::raw::instruction_counter() - start,
    )
}

// advance_startup
// Measure one startup step and report whether the database is ready.
// guard  : validates the request frame for method 1.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "advance_startup",
    guard = crate::transport::admit::<1>,
    decode = crate::transport::decode::<_, 1>,
    encode = crate::transport::encode::<_, 1>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn advance_startup((): ()) -> (bool, u64) {
    let start = ic_platform_runtime::raw::instruction_counter();
    // Deliberately fixed test entropy, never a production identity source.
    icydb::db::provide_boot_entropy([41; 32]).unwrap();
    let ready =
        icydb::db::with_request_execution(|| __icydb_generated::startup_driver_attempt().unwrap());
    if ready {
        __icydb_host::RECOVERY_PENDING.set(false);
    }
    (
        ready,
        ic_platform_runtime::raw::instruction_counter() - start,
    )
}

// insert
// Insert a benchmark row and report its instruction cost.
// guard  : validates the request frame for method 2.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "insert",
    guard = crate::transport::admit::<2>,
    decode = crate::transport::decode::<_, 2>,
    encode = crate::transport::encode::<_, 2>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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

// lookup
// Look up a benchmark row by ID and report its instruction cost.
// guard  : validates the request frame for method 3.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "lookup",
    guard = crate::transport::admit::<3>,
    decode = crate::transport::decode::<_, 3>,
    encode = crate::transport::encode::<_, 3>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn lookup(id: u64) -> (Option<Row>, u64) {
    measure(|| {
        db().unwrap()
            .get::<Entry>(Id::from_key(id))
            .unwrap()
            .as_ref()
            .map(row)
    })
}

// read_modify_write
// Measure the complete journey from lookup through replacement.
// guard  : validates the request frame for method 4.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "read_modify_write",
    guard = crate::transport::admit::<4>,
    decode = crate::transport::decode::<_, 4>,
    encode = crate::transport::encode::<_, 4>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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

// listing
// Return up to ten category rows in ascending ID order and report instruction cost.
// guard  : validates the request frame for method 5.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "listing",
    guard = crate::transport::admit::<5>,
    decode = crate::transport::decode::<_, 5>,
    encode = crate::transport::encode::<_, 5>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
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

// keyset_listing
// Return up to two matching rows after the supplied ID and report instruction cost.
// guard  : validates the request frame for method 11.
// limits : 64 KiB request, 64 KiB reply.
#[cfg(feature = "keyset-comparison")]
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "keyset_listing",
    guard = crate::transport::admit::<11>,
    decode = crate::transport::decode::<_, 11>,
    encode = crate::transport::encode::<_, 11>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn keyset_listing((after, category): (Option<u64>, u64)) -> (Vec<Row>, u64) {
    measure(|| {
        use icydb::db::query::{FieldRef, asc};
        let db = db().unwrap();
        let mut query = db
            .query::<Entry>()
            .unwrap()
            .filter(FieldRef::new("category").eq(category))
            .filter(FieldRef::new("amount").gte(10_u64))
            .order_by(asc("id"))
            .limit(2);
        if let Some(id) = after {
            query = query.filter(FieldRef::new("id").gt(id));
        }
        let page = query.execute_live_page(None).unwrap();
        assert!(page.rows.len() == 2 || page.continuation.is_none());
        page.rows.iter().map(row).collect()
    })
}

// fluent_listing
// Return the filtered rows up to the requested limit or an error code, with instruction cost.
// guard  : validates the request frame for method 6.
// limits : 64 KiB request, 64 KiB reply.
#[cfg(feature = "fluent-comparison")]
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_listing",
    guard = crate::transport::admit::<6>,
    decode = crate::transport::decode::<_, 6>,
    encode = crate::transport::encode::<_, 6>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_listing(
    (mode, category, minimum, limit): (u8, u64, u64, u32),
) -> (Result<Vec<Row>, u16>, u64) {
    measure(|| {
        use icydb::db::query::{FieldRef, asc, desc};
        let database = db().unwrap();
        let mut query = database
            .query::<Entry>()
            .unwrap()
            .filter(FieldRef::new("amount").gte(minimum))
            .limit(limit);
        if mode != 2 {
            query = query.filter(FieldRef::new("category").eq(category));
        } else {
            query = query.filter(FieldRef::new("category").gte(0_u64));
        }
        query = match mode {
            0 => query.order_by(asc("id")),
            1 => query.order_by(desc("id")),
            2 => query.order_by(asc("category")).order_by(asc("id")),
            3 => query.order_by(desc("amount")).order_by(asc("id")),
            _ => panic!("unsupported comparison shape"),
        };
        match query.execute_live_page(None) {
            Ok(page) => Ok(page.rows.iter().map(row).collect()),
            Err(icydb::db::TypedOperationError::Database(error)) => Err(error.code().raw()),
            Err(error) => panic!("unexpected typed adapter refusal: {error}"),
        }
    })
}

// grouped_aggregate
// Measure the selected aggregate with a four-group limit and a 64 KiB group budget.
// guard  : validates the request frame for method 7.
// limits : 64 KiB request, 64 KiB reply.
#[cfg(feature = "aggregate-comparison")]
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "grouped_aggregate",
    guard = crate::transport::admit::<7>,
    decode = crate::transport::decode::<_, 7>,
    encode = crate::transport::encode::<_, 7>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn grouped_aggregate(
    (operation, category, minimum): (u8, Option<u64>, u64),
) -> (Vec<(u64, u64)>, u64) {
    measure(|| {
        use icydb::{
            db::query::{FieldRef, count, max_by, min_by, sum},
            value::PublicValue,
        };
        // The paired IC DB control selects an index explicitly. Legacy already
        // selects its category index; retain that same public query for both arms.
        #[cfg(feature = "query-matrix")]
        let operation = {
            assert!(operation < 8);
            operation % 4
        };
        let aggregate = match operation {
            0 => count(),
            1 => sum("amount"),
            2 => min_by("amount"),
            3 => max_by("amount"),
            _ => panic!("unsupported comparison aggregate"),
        };
        let database = db().unwrap();
        // Public typed fluent admission; no SQL, trusted fallback or client reducer.
        let output = database
            .query::<Entry>()
            .unwrap()
            .filter(match category {
                Some(value) => FieldRef::new("category").eq(value),
                None => FieldRef::new("category").gte(0_u64),
            })
            .filter(FieldRef::new("amount").gte(minimum))
            .group_by("category")
            .aggregate(aggregate)
            .grouped_limits(4, 65_536)
            .execute_grouped()
            .unwrap();
        assert!(output.next_cursor.is_none());
        output
            .rows
            .iter()
            .map(|row| {
                let key = match row.group_key() {
                    [key] => match key.as_public() {
                        PublicValue::Nat64(value) => *value,
                        value => panic!("unexpected group key: {value:?}"),
                    },
                    _ => panic!("unexpected group key shape"),
                };
                let value = match row.aggregate_values() {
                    [value] => match value.as_public() {
                        PublicValue::Nat64(value) => *value,
                        // Legacy SUM produces Decimal. Reject rounding or range loss.
                        PublicValue::Decimal(value) if operation == 1 => value.to_u64().unwrap(),
                        value => panic!("unexpected aggregate value: {value:?}"),
                    },
                    _ => panic!("unexpected aggregate output shape"),
                };
                (key, value)
            })
            .collect()
    })
}

// count_rows
// Return the exact whole-table or category count and its instruction cost.
// guard  : validates the request frame for method 8.
// limits : 64 KiB request, 64 KiB reply.
#[cfg(feature = "count-comparison")]
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "count_rows",
    guard = crate::transport::admit::<8>,
    decode = crate::transport::decode::<_, 8>,
    encode = crate::transport::encode::<_, 8>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn count_rows(category: Option<u64>) -> (Option<u64>, u64) {
    measure(|| {
        let database = db().unwrap();
        let mut query = database.query::<Entry>().unwrap();
        if let Some(category) = category {
            query = query.filter(icydb::db::query::FieldRef::new("category").eq(category));
        }
        // Maintained cardinality metadata; no scan fallback or substituted reducer.
        Some(query.execute_exact_count().unwrap())
    })
}

// category_exists
// Check whether the benchmark category has an owner and measure the lookup.
// guard  : validates the request frame for method 10.
// limits : 64 KiB request, 64 KiB reply.
#[cfg(feature = "exists-comparison")]
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "category_exists",
    guard = crate::transport::admit::<10>,
    decode = crate::transport::decode::<_, 10>,
    encode = crate::transport::encode::<_, 10>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn category_exists(category: u64) -> (bool, u64) {
    measure(|| {
        db().unwrap()
            .query::<Entry>()
            .unwrap()
            .filter(icydb::db::query::FieldRef::new("category").eq(category))
            .execute_exact_count()
            .unwrap()
            > 0
    })
}
