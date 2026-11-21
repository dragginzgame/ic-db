//! Measure reachable nested field paths while holding the schema and query executor fixed.
#![deny(unsafe_code)]

use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Entity};
use ic_db_qualification_transport as transport;
use ic_platform_runtime::raw::instruction_counter;
use ic_storage::{
    AllocationGrants, DefaultMemoryImpl, GenericAllocationPolicy, MemoryAuthority, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot, StableKey,
};

#[canister]
struct Owner {}

#[store(canister = Owner, key = "probe.rows.v1")]
struct Rows {}

#[record(fields(
    field(name = "a", value(item(prim = "Nat16"))),
    field(name = "aa", value(many, max_items = 8, item(prim = "Nat16"))),
    field(name = "b", value(item(prim = "Nat16"))),
    field(name = "c", value(item(prim = "Nat16"))),
    field(name = "d", value(item(prim = "Nat16"))),
    field(name = "e", value(item(prim = "Nat16"))),
    field(name = "f", value(item(prim = "Nat16"))),
    field(name = "g", value(item(prim = "Nat16"))),
    field(name = "h", value(item(prim = "Nat16")))
))]
struct Members {}

// Both builds use the same generated row codec. Only the physical index varies.
macro_rules! sample {
    ($($index:tt)*) => {
        #[entity(
            store = Rows,
            pk(field = "id"),
            $($index)*
            fields(
                field(name = "id", value(item(prim = "Nat64"))),
                field(name = "flat", value(item(prim = "Nat16"))),
                field(name = "one", value(item(is = "Members"))),
                field(name = "two", value(item(is = "Members"))),
                field(name = "three", value(item(is = "Members"))),
                field(name = "four", value(item(is = "Members")))
            )
        )]
        struct Sample {}
    };
}

#[cfg(feature = "flat-index-control")]
sample!(index(fields = ["flat"]),);
#[cfg(all(feature = "member-index-control", not(feature = "flat-index-control")))]
sample!(index(name = "by_value", fields = ["one.h"]),);
#[cfg(not(any(feature = "flat-index-control", feature = "member-index-control")))]
sample!();

fn members(a: u16) -> Members {
    Members {
        a,
        aa: vec![a; 8],
        b: a + 1,
        c: a + 2,
        d: a + 3,
        e: a + 4,
        f: a + 5,
        g: a + 6,
        h: a + 7,
    }
}

fn database(create: bool) -> Database {
    let mut host = MemoryRuntime::new(DefaultMemoryImpl::default()).unwrap();
    let key = "probe.rows.v1";
    host.bootstrap(
        &SealedDeclarationSnapshot::new(vec![MemoryRequest::new("probe", key).unwrap()]).unwrap(),
        &AllocationGrants::new(vec![MemoryAuthority::new("probe", "probe.").unwrap()]).unwrap(),
        &GenericAllocationPolicy,
    )
    .unwrap();
    let memory = host.open_memory(&StableKey::parse(key).unwrap()).unwrap();
    let schema = Schema::new(&[Sample::SCHEMA]).unwrap();
    if create {
        Database::create(memory, schema, [1; 16])
    } else {
        Database::open(memory, schema)
    }
    .unwrap()
}

// init
// Create the empty fixture store.
// guard  : allows lifecycle calls.
// limits : empty request, no reply.
#[ic_platform_runtime::init(runtime = ic_platform_runtime, guard = crate::transport::allow,
    decode = crate::transport::empty, max_bytes = 0, on_error = crate::transport::lifecycle_error)]
fn init((): ()) {
    database(true);
}

// write
// Insert, replace or delete the same batch of 24 rows and measure preparation through commit.
// guard  : admits qualification method 2.
// limits : 64 KiB frame, one instruction count in reply.
#[ic_platform_runtime::update(runtime = ic_platform_runtime, name = "write",
    guard = crate::transport::admit::<2>, decode = crate::transport::decode::<_, 2>,
    encode = crate::transport::encode::<_, 2>, max_bytes = 65_536, on_error = crate::transport::endpoint_error)]
fn write(mode: u8) -> u64 {
    let mut db = database(false);
    let start = instruction_counter();
    let mut batch = Batch::new();
    for id in 0..24_u16 {
        if mode == 2 {
            batch.delete::<Sample>(id.into()).unwrap();
            continue;
        }
        let flat = id + u16::from(mode);
        let row = Sample {
            id: id.into(),
            flat,
            one: members(flat),
            two: members(flat + 8),
            three: members(flat + 16),
            four: members(flat + 24),
        };
        match mode {
            0 => batch.insert(&row),
            1 => batch.replace(&row),
            _ => panic!("unknown write"),
        }
        .unwrap();
    }
    db.commit(batch).unwrap();
    instruction_counter() - start
}

macro_rules! paths {
    ($parent:ident; $($member:ident),+) => {
        [$(ic_db::field_path!(Sample::$parent, Members::$member)),+]
    };
}

fn path(i: u8) -> &'static str {
    if i == 32 {
        return Sample::FLAT;
    }
    let group = match (option_env!("IC_DB_PATH_COUNT"), i / 8) {
        (None | Some("1"), 0) if i == 0 => return ic_db::field_path!(Sample::ONE, Members::A),
        (Some("8" | "32"), 0) => &paths!(ONE; A,B,C,D,E,F,G,H),
        (Some("32"), 1) => &paths!(TWO; A,B,C,D,E,F,G,H),
        (Some("32"), 2) => &paths!(THREE; A,B,C,D,E,F,G,H),
        (Some("32"), 3) => &paths!(FOUR; A,B,C,D,E,F,G,H),
        _ => panic!("selector outside build tier"),
    };
    group[usize::from(i % 8)]
}

// measure
// Return matching IDs and instructions spent building and executing the fluent query.
// guard  : admits qualification method 1.
// limits : 64 KiB frame, at most eight returned IDs.
#[ic_platform_runtime::update(runtime = ic_platform_runtime, name = "measure",
    guard = crate::transport::admit::<1>, decode = crate::transport::decode::<_, 1>,
    encode = crate::transport::encode::<_, 1>, max_bytes = 65_536, on_error = crate::transport::endpoint_error)]
fn measure((selector, mode): (u8, u8)) -> (Vec<u64>, u64) {
    let db = database(false);
    let field = path(selector);
    let threshold = 16 + u16::from(selector % 32);
    let start = instruction_counter();
    let mut query = db.query::<Sample>().limit(8);
    if mode == 4 {
        query = query.gte(field, threshold + 6).order_asc(field);
    } else if mode == 3 {
        query = query.eq(field, threshold);
    } else if mode != 1 {
        query = query.gte(field, threshold);
    }
    if matches!(mode, 1 | 2) {
        query = query.order_desc(field);
    }
    let page = query.fetch().unwrap();
    let elapsed = instruction_counter() - start;
    (page.rows.into_iter().map(|row| row.id).collect(), elapsed)
}
