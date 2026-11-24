#![deny(unsafe_code)]
// Platform narrowly permits the generated IC export attribute; handlers remain safe.
//! Scalar benchmark host. The ranked listing feature adds four layouts for the SQLite comparison.

#[cfg(feature = "count-profile")]
mod count_profile;
#[cfg(feature = "ranked-listing")]
mod ranked_listing;

use ic_db_qualification_transport as transport;

use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error};
use ic_storage::{
    AllocationGrants, DefaultMemoryImpl, GenericAllocationPolicy, MemoryAuthority, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot, StableKey,
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
    // Fixed test identity only; this is not an application incarnation policy.
    Database::create(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
        compiled(),
        [41; 16],
    )
    .unwrap();
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
    measure(|| {
        let _ = database();

        true
    })
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
        let mut database = database();
        let mut batch = Batch::new();
        batch
            .insert(&Entry {
                id,
                category,
                amount,
            })
            .unwrap();

        match database.commit(batch) {
            Ok(()) => true,
            Err(Error::Exists) => false,
            Err(error) => panic!("unexpected insert refusal: {error:?}"),
        }
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
    measure(|| database().get::<Entry>(id).unwrap().as_ref().map(row))
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
        let mut database = database();
        let mut current = database.get::<Entry>(id).unwrap()?;
        current.category = category;
        current.amount = amount;
        let mut batch = Batch::new();
        batch.replace(&current).unwrap();
        database.commit(batch).unwrap();

        Some(row(&current))
    })
}

// listing
// List rows in the requested category for the engine comparison.
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
    // The index yields complete ID order within one category. This measures the
    // direct read. The optional fluent comparison below measures planner work.
    measure(|| {
        database()
            .index_prefix::<Entry>("category,id", &[category.into()], 10)
            .unwrap()
            .iter()
            .map(row)
            .collect()
    })
}

// keyset_listing
// Read two matches after the last emitted ID on fixed benchmark input.
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
fn keyset_listing((after, category, hinted): (Option<u64>, u64, bool)) -> (Vec<Row>, u64) {
    measure(|| {
        let db = database();
        let prefix = [category.into()];
        let query = db.query::<Entry>();
        let query = if hinted {
            query.using_index("category,id", &prefix)
        } else {
            query.eq("category", category)
        };
        let mut query = query
            .filter("amount", ic_db::Comparison::Ge, 10_u64)
            .limit(2)
            .work_limit(512);
        if let Some(id) = after {
            query = query.filter("id", ic_db::Comparison::Gt, id);
        }
        let page = query.fetch().unwrap();
        assert!(page.rows.len() == 2 || page.next.is_none());

        page.rows.iter().map(row).collect()
    })
}

// fluent_listing
// Run the selected filtered listing and report its result and instruction cost.
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
        use ic_db::{Comparison, SortDirection};
        let db = database();
        let mut query = db
            .query::<Entry>()
            .filter("amount", Comparison::Ge, minimum)
            .limit(limit as usize);
        if mode != 2 {
            query = query.filter("category", Comparison::Eq, category);
        } else {
            query = query.filter("category", Comparison::Ge, 0_u64);
        }
        query = match mode {
            0 => query.order_by(&["id"]),
            1 => query.order_by(&["id"]).descending(),
            2 => query.order_by(&["category", "id"]),
            3 => query.order_by_each(&[
                ("amount", SortDirection::Descending),
                ("id", SortDirection::Ascending),
            ]),
            _ => panic!("unsupported comparison shape"),
        };
        #[cfg(feature = "query-matrix")]
        let query = query.work_limit(512);

        Ok(query.fetch().unwrap().rows.iter().map(row).collect())
    })
}

// grouped_aggregate
// Measure the requested aggregate over matching benchmark entries.
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
        use ic_db::Comparison;
        let db = database();
        let query = db
            .query::<Entry>()
            .filter(
                "category",
                if category.is_some() {
                    Comparison::Eq
                } else {
                    Comparison::Ge
                },
                category.unwrap_or(0),
            )
            .filter("amount", Comparison::Ge, minimum);
        #[cfg(feature = "query-matrix")]
        let query = query.work_limit(512);
        #[cfg(feature = "query-matrix")]
        let (query, operation) = {
            assert!(operation < 8);
            let query = if operation >= 4 {
                query.using_index("category,id", &[])
            } else {
                query
            };

            (query, operation % 4)
        };
        if operation == 0 {
            return query
                .group_count::<CategoryOnly>()
                .unwrap()
                .value
                .into_iter()
                .map(|row| (row.key.category, row.count))
                .collect();
        }
        let result = match operation {
            1 => query.group_sum::<CategoryOnly, AmountOnly>(),
            2 => query.group_min::<CategoryOnly, AmountOnly>(),
            3 => query.group_max::<CategoryOnly, AmountOnly>(),
            _ => panic!("unsupported comparison aggregate"),
        }
        .unwrap();

        result
            .value
            .into_iter()
            .map(|row| (row.key.category, row.value.unwrap().amount))
            .collect()
    })
}

// count_rows
// Count all benchmark rows or the selected category under the work limit.
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
        let db = database();
        if category.is_none() {
            return Some(db.cardinality::<Entry>().unwrap());
        }
        let mut query = db.query::<Entry>().work_limit(512);
        if let Some(category) = category {
            query = query.using_index("category,id", &[category.into()]).filter(
                "category",
                ic_db::Comparison::Eq,
                category,
            );
        }

        match query.count_validated() {
            Ok(result) => Some(result.value),
            // Explicit work refusal, never a successful partial count.
            Err(Error::Limit) => None,
            Err(error) => panic!("unexpected count refusal: {error:?}"),
        }
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
        database()
            .query::<Entry>()
            .filter("category", ic_db::Comparison::Eq, category)
            .work_limit(1)
            .exists()
            .unwrap()
            .value
    })
}

// filtered_exists
// Check for a category member with an exact unindexed amount within a candidate budget.
// guard  : validates the request frame for method 12.
// limits : 64 KiB request, 64 KiB reply.
#[cfg(feature = "exists-comparison")]
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "filtered_exists",
    guard = crate::transport::admit::<12>,
    decode = crate::transport::decode::<_, 12>,
    encode = crate::transport::encode::<_, 12>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn filtered_exists((category, amount, work): (u64, u64, u16)) -> (Option<(bool, u64, u64)>, u64) {
    measure(|| {
        match database()
            .query::<Entry>()
            .eq(Entry::CATEGORY, category)
            .eq(Entry::AMOUNT, amount)
            .work_limit(usize::from(work))
            .exists()
        {
            Ok(result) => Some((result.value, result.examined as u64, result.fetched as u64)),
            Err(Error::Limit) => None,
            Err(error) => panic!("unexpected existence refusal: {error:?}"),
        }
    })
}

///
/// Entry
/// Exercises indexed access by `category` and `id` on rows keyed by `id`.
///

#[entity(
    store = crate::RecordsStore,
    pk(field = "id"),
    index(fields = ["category", "id"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat64"))),
        field(name = "amount", value(item(prim = "Nat64")))
    )
)]
struct Entry {}

const KEY: &str = <RecordsStore as ic_db::Store>::KEY;
type Row = (u64, u64, u64);

fn compiled() -> Schema {
    Schema::new(&[
        Entry::SCHEMA,
        #[cfg(feature = "ranked-listing")]
        ranked_listing::ByTime::SCHEMA,
        #[cfg(feature = "ranked-listing")]
        ranked_listing::ByProject::SCHEMA,
        #[cfg(feature = "ranked-listing")]
        ranked_listing::ByPrice::SCHEMA,
        #[cfg(feature = "ranked-listing")]
        ranked_listing::ByPriceAndTime::SCHEMA,
    ])
    .unwrap()
}

fn runtime() -> MemoryRuntime<DefaultMemoryImpl> {
    let mut host = MemoryRuntime::new(DefaultMemoryImpl::default()).unwrap();
    let declarations =
        SealedDeclarationSnapshot::new(vec![MemoryRequest::new("m1", KEY).unwrap()]).unwrap();
    let grants = AllocationGrants::new(vec![MemoryAuthority::new("m1", "m1.").unwrap()]).unwrap();
    host.bootstrap(&declarations, &grants, &GenericAllocationPolicy)
        .unwrap();

    host
}

fn database() -> Database {
    let host = runtime();

    Database::open(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
        compiled(),
    )
    .unwrap()
}

fn row(value: &Entry) -> Row {
    (value.id, value.category, value.amount)
}

// Include runtime construction, schema admission, database work and result mapping.
// Wire decode/encode is outside this interval, as in the frozen IcyDB host.
fn measure<T>(operation: impl FnOnce() -> T) -> (T, u64) {
    let start = ic_platform_runtime::raw::instruction_counter();
    let result = operation();

    (
        result,
        ic_platform_runtime::raw::instruction_counter() - start,
    )
}

// Output records only; no additional stored entity or index.
///
/// AmountOnly
/// Carries the `Nat64` value of `amount`.
///

#[cfg(feature = "aggregate-comparison")]
#[record(fields(field(name = "amount", value(item(prim = "Nat64")))))]
struct AmountOnly {}

///
/// CategoryOnly
/// Carries the `Nat64` value of `category`.
///

#[cfg(feature = "aggregate-comparison")]
#[record(fields(field(name = "category", value(item(prim = "Nat64")))))]
struct CategoryOnly {}

/// Logical owner of the RecordsStore allocation.
#[canister]
pub struct ProbeCanister {}

/// Store using the existing m1.benchmark.records.v1 allocation key.
#[store(canister = ProbeCanister, key = "m1.benchmark.records.v1")]
pub struct RecordsStore {}
