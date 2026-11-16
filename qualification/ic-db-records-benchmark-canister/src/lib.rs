#![forbid(unsafe_code)]
//! One-entity host matching the frozen IcyDB scalar workload, without fault controls.

#[cfg(feature = "count-profile")]
mod count_profile;

use ic_db_qualification_transport as transport;

use ic_db::{Batch, Database, Entity, Error, Schema, entity};
use ic_platform_runtime::cdk;
use ic_storage::{
    AllocationGrants, DefaultMemoryImpl, GenericAllocationPolicy, MemoryAuthority, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot, StableKey,
};

#[entity( pk(field = "id", source = "external"),
    index(fields = ["category", "id"]),
    fields(field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat64"))),
        field(name = "amount", value(item(prim = "Nat64")))))]
struct Entry {}

const KEY: &str = "m1.benchmark.records.v1";
type Row = (u64, u64, u64);

fn compiled() -> Schema {
    Schema::new(&[Entry::SCHEMA]).unwrap()
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
fn database() -> Database<DefaultMemoryImpl> {
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
    let start = cdk::api::instruction_counter();
    let result = operation();
    (result, cdk::api::instruction_counter() - start)
}

#[cdk::init]
fn initialize() {
    let host = runtime();
    // Fixed test identity only; this is not an application incarnation policy.
    Database::create(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
        compiled(),
        [41; 16],
    )
    .unwrap();
}

#[ic_platform_runtime::cdk::update(
    guard = "crate::transport::admit::<1>",
    decode_with = "crate::transport::decode::<_, 1>",
    encode_with = "crate::transport::encode::<_, 1>"
)]
fn advance_startup() -> (bool, u64) {
    measure(|| {
        let _ = database();
        true
    })
}

#[ic_platform_runtime::cdk::update(
    guard = "crate::transport::admit::<2>",
    decode_with = "crate::transport::decode::<_, 2>",
    encode_with = "crate::transport::encode::<_, 2>"
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

#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<3>",
    decode_with = "crate::transport::decode::<_, 3>",
    encode_with = "crate::transport::encode::<_, 3>"
)]
fn lookup(id: u64) -> (Option<Row>, u64) {
    measure(|| database().get::<Entry>(id).unwrap().as_ref().map(row))
}

#[ic_platform_runtime::cdk::update(
    guard = "crate::transport::admit::<4>",
    decode_with = "crate::transport::decode::<_, 4>",
    encode_with = "crate::transport::encode::<_, 4>"
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

#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<5>",
    decode_with = "crate::transport::decode::<_, 5>",
    encode_with = "crate::transport::encode::<_, 5>"
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
        Ok(query.fetch().unwrap().rows.iter().map(row).collect())
    })
}

// Output records only; no additional stored entity or index.
#[cfg(feature = "aggregate-comparison")]
#[ic_db::record(fields(field(name = "amount", value(item(prim = "Nat64")))))]
struct AmountOnly {}
#[cfg(feature = "aggregate-comparison")]
#[ic_db::record(fields(field(name = "category", value(item(prim = "Nat64")))))]
struct CategoryOnly {}

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

#[cfg(feature = "count-comparison")]
#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<8>",
    decode_with = "crate::transport::decode::<_, 8>",
    encode_with = "crate::transport::encode::<_, 8>"
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
        match query.count() {
            Ok(result) => Some(result.value),
            // Explicit work refusal, never a successful partial count.
            Err(Error::Limit) => None,
            Err(error) => panic!("unexpected count refusal: {error:?}"),
        }
    })
}
