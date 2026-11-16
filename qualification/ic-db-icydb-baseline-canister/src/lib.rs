#![forbid(unsafe_code)]

//! Frozen IcyDB measurement host. These Wire exports are private test controls.
//! Startup is advanced explicitly by the harness; no Canic lifecycle is measured.

use ic_db_qualification_transport as transport;

#[cfg(feature = "envelope")]
mod envelope;

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

#[ic_platform_runtime::cdk::update(
    guard = "crate::transport::admit::<1>",
    decode_with = "crate::transport::decode::<_, 1>",
    encode_with = "crate::transport::encode::<_, 1>"
)]
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

#[ic_platform_runtime::cdk::update(
    guard = "crate::transport::admit::<2>",
    decode_with = "crate::transport::decode::<_, 2>",
    encode_with = "crate::transport::encode::<_, 2>"
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

#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<3>",
    decode_with = "crate::transport::decode::<_, 3>",
    encode_with = "crate::transport::encode::<_, 3>"
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

#[ic_platform_runtime::cdk::update(
    guard = "crate::transport::admit::<4>",
    decode_with = "crate::transport::decode::<_, 4>",
    encode_with = "crate::transport::encode::<_, 4>"
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

#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<5>",
    decode_with = "crate::transport::decode::<_, 5>",
    encode_with = "crate::transport::encode::<_, 5>"
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

#[cfg(feature = "fluent-comparison")]
#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<6>",
    decode_with = "crate::transport::decode::<_, 6>",
    encode_with = "crate::transport::encode::<_, 6>"
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

#[cfg(feature = "aggregate-comparison")]
#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<7>",
    decode_with = "crate::transport::decode::<_, 7>",
    encode_with = "crate::transport::encode::<_, 7>"
)]
fn grouped_aggregate(
    (operation, category, minimum): (u8, Option<u64>, u64),
) -> (Vec<(u64, u64)>, u64) {
    measure(|| {
        use icydb::{
            db::query::{FieldRef, count, max_by, min_by, sum},
            value::PublicValue,
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

#[cfg(feature = "count-comparison")]
#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<8>",
    decode_with = "crate::transport::decode::<_, 8>",
    encode_with = "crate::transport::encode::<_, 8>"
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
