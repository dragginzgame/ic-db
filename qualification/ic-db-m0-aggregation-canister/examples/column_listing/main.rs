//! Matched canister builds for one paginated listing with four projection shapes.
#![deny(unsafe_code)]

use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Entity, QueryCursor, QueryPage};
use ic_db_qualification_transport as transport;
use ic_platform_runtime::raw::instruction_counter;
use ic_storage::{
    AllocationGrants, DefaultMemoryImpl, GenericAllocationPolicy, MemoryAuthority, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot, StableKey,
};

#[canister]
struct Host {}

#[store(canister = Host, key = "listing.rows.v1")]
struct Rows {}

#[entity(store = Rows, pk(field = "id"), index(fields = ["phase", "rank", "id"]),
    fields(
        field(name = "id", value(item(prim = "Ulid"))),
        field(name = "phase", value(item(prim = "Bool"))),
        field(name = "rank", value(item(prim = "Nat16"))),
        field(name = "score", value(item(prim = "Nat16"))),
        field(name = "ready", value(item(prim = "Bool"))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 512)))
    )
)]
struct Listing {}

#[record(fields(field(name = "id", value(item(prim = "Ulid")))))]
struct IdOnly {}

fn database(create: bool) -> Database {
    let mut host = MemoryRuntime::new(DefaultMemoryImpl::default()).unwrap();
    let key = "listing.rows.v1";
    host.bootstrap(
        &SealedDeclarationSnapshot::new(vec![MemoryRequest::new("listing", key).unwrap()]).unwrap(),
        &AllocationGrants::new(vec![MemoryAuthority::new("listing", "listing.").unwrap()]).unwrap(),
        &GenericAllocationPolicy,
    )
    .unwrap();
    let memory = host.open_memory(&StableKey::parse(key).unwrap()).unwrap();
    let schema = Schema::new(&[Listing::SCHEMA]).unwrap();
    if create {
        Database::create(memory, schema, [1; 16])
    } else {
        Database::open(memory, schema)
    }
    .unwrap()
}

// init
// Install 24 rows with repeated ranks, a residual predicate and a 512-byte payload.
// guard  : allows lifecycle calls.
// limits : empty request and no reply.
#[ic_platform_runtime::init(runtime = ic_platform_runtime, guard = crate::transport::allow,
    decode = crate::transport::empty, max_bytes = 0, on_error = crate::transport::lifecycle_error)]
fn init((): ()) {
    let mut db = database(true);
    let mut batch = Batch::new();
    for n in 0..24_u16 {
        let rank = (7 * n % 24) / 3;
        batch
            .insert(&Listing {
                id: Ulid::from_u128(n.into()),
                phase: n % 4 != 3,
                rank,
                score: n * 11,
                ready: rank >= 2 && n % 5 != 2,
                payload: vec![n as u8; 512],
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
}

type Row = (u64, Option<(u16, u16)>);
type Reply = (Vec<Row>, Option<Vec<u8>>, u32, u32, u32, u64);

fn row(id: Ulid, fields: Option<(u16, u16)>) -> Row {
    (
        u64::try_from(u128::from_be_bytes(id.to_bytes())).unwrap(),
        fields,
    )
}

fn finish<T>(page: QueryPage<T>, start: u64, display: impl Fn(T) -> Row) -> Reply {
    std::hint::black_box(&page);
    let instructions = instruction_counter() - start;
    (
        page.rows.into_iter().map(display).collect(),
        page.next.map(|cursor| cursor.to_bytes()),
        page.examined as u32,
        page.fetched as u32,
        page.output_bytes as u32,
        instructions,
    )
}

// listing
// Measure fluent construction through typed result decoding, after reopening the store.
// guard  : validates the request frame for method 1.
// limits : 64 KiB request, at most two rows and one continuation in reply.
#[ic_platform_runtime::query(runtime = ic_platform_runtime, name = "listing", guard = crate::transport::admit::<1>,
    decode = crate::transport::decode::<_, 1>, encode = crate::transport::encode::<_, 1>,
    max_bytes = 65_536, on_error = crate::transport::endpoint_error)]
fn listing((mode, sparse, after): (u8, bool, Option<Vec<u8>>)) -> Reply {
    let db = database(false);
    let cursor = after.map(|b| QueryCursor::from_bytes(&b).unwrap());
    let start = instruction_counter();
    let mut query = db
        .query::<Listing>()
        .eq(Listing::PHASE, true)
        .order_by(&[Listing::RANK, Listing::ID])
        .limit(2)
        .work_limit(4);
    if sparse {
        query = query.eq(Listing::READY, true);
    }
    let query = query.after(cursor.as_ref());
    match (option_env!("IC_DB_LISTING_SHAPE"), mode) {
        (None | Some("entity"), _) | (Some("records" | "with-scalar" | "all"), 0) => {
            finish(query.fetch().unwrap(), start, |r| {
                row(r.id, Some((r.score, r.rank)))
            })
        }
        (Some("named"), _) | (Some("records" | "with-scalar" | "all"), 1) => {
            finish(query.fetch_as::<IdOnly>().unwrap(), start, |r| {
                row(r.id, None)
            })
        }
        (Some("scalar"), _) | (Some("with-scalar" | "all"), 2) => {
            finish(query.select(|r| r.id).fetch().unwrap(), start, |id| {
                row(id, None)
            })
        }
        (Some("tuple"), _) | (Some("all"), 3) => finish(
            query.select(|r| (r.id, r.score, r.rank)).fetch().unwrap(),
            start,
            |(id, score, rank)| row(id, Some((score, rank))),
        ),
        _ => panic!("unknown listing shape"),
    }
}
