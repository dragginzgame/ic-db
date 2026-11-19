#![deny(unsafe_code)]
// Platform narrowly permits the generated IC export attribute; handlers remain safe.
//! Controlled entity-count growth with one endpoint and identical typed operations.
//! Optional fluent shapes and result maps isolate application composition growth.

use ic_db::{Batch, Database, Entity, Schema, entity};
use ic_db_qualification_transport as transport;
use ic_storage::{
    AllocationGrants, DefaultMemoryImpl, GenericAllocationPolicy, MemoryAuthority, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot, StableKey,
};

const KEY: &str = "m1.growth.records.v1";
#[cfg(feature = "fluent")]
mod fluent;
type Row = (u64, u64, u64);
type Observation = (Option<Row>, Vec<Row>, Vec<Row>, u64, u8);

// The experiment retains executable metadata and the supplier's host runtime,
// never a Database, collection lease, manifest, incarnation or revision.
#[cfg(feature = "retained-host")]
thread_local! {
    static HOST: MemoryRuntime<DefaultMemoryImpl> = runtime();
    static SCHEMA: Schema = compiled();
}

fn with_host<R>(operation: impl FnOnce(&MemoryRuntime<DefaultMemoryImpl>) -> R) -> R {
    #[cfg(feature = "retained-host")]
    {
        HOST.with(operation)
    }
    #[cfg(not(feature = "retained-host"))]
    {
        operation(&runtime())
    }
}

fn schema_for_open() -> Schema {
    #[cfg(feature = "retained-host")]
    {
        SCHEMA.with(Clone::clone)
    }
    #[cfg(not(feature = "retained-host"))]
    {
        compiled()
    }
}

// The same declaration list owns schema membership and runtime-selected typed
// reachability. Numeric selectors belong only to this private fixture transport.
macro_rules! declare_entity {
    ($name:ident) => {
        #[cfg(feature = "second-index")]
        declare_entity!(@fields $name, index(fields = ["amount", "id"]),);
        #[cfg(not(feature = "second-index"))]
        declare_entity!(@fields $name,);
    };
    (@fields $name:ident, $($extra:tt)*) => {
        #[entity(pk(field = "id"),
            index(fields = ["category", "id"]), $($extra)*
            fields(field(name = "id", value(item(prim = "Nat64"))),
                field(name = "category", value(item(prim = "Nat64"))),
                field(name = "amount", value(item(prim = "Nat64")))))]
        struct $name {}
    };
}
macro_rules! entities {
    ($base:ident = $first:literal, $(#[$guard:meta] { $($name:ident = $selector:literal,)+ })+) => {
        entities!(@entries
            $base = $first,
            $($(#[$guard] $name = $selector,)+)+
        );
    };
    (@entries $( $(#[$configuration:meta])? $name:ident = $selector:literal, )+) => {
        $(
            $(#[$configuration])?
            declare_entity!($name);
        )*
        fn compiled() -> Schema {
            Schema::new(&[$($(#[$configuration])? $name::SCHEMA),*]).unwrap()
        }

        fn dispatch(db: &mut Database, selector: u8, mode: u8, input: Row) -> Observation {
            match selector {
                $(
                    $(#[$configuration])?
                    $selector => operate(db, mode, input,
                        &$name { id: input.0, category: input.1, amount: input.2 },
                        |row: &$name| (row.id, row.category, row.amount)),
                )*
                _ => panic!("entity selector outside this fixture"),
            }
        }
    };
}
entities!(
    Entry00 = 0,
    #[cfg(feature = "entities-8")]
    {
        Entry01 = 1,
        Entry02 = 2,
        Entry03 = 3,
        Entry04 = 4,
        Entry05 = 5,
        Entry06 = 6,
        Entry07 = 7,
    }
    #[cfg(feature = "entities-32")]
    {
        Entry08 = 8,
        Entry09 = 9,
        Entry10 = 10,
        Entry11 = 11,
        Entry12 = 12,
        Entry13 = 13,
        Entry14 = 14,
        Entry15 = 15,
        Entry16 = 16,
        Entry17 = 17,
        Entry18 = 18,
        Entry19 = 19,
        Entry20 = 20,
        Entry21 = 21,
        Entry22 = 22,
        Entry23 = 23,
        Entry24 = 24,
        Entry25 = 25,
        Entry26 = 26,
        Entry27 = 27,
        Entry28 = 28,
        Entry29 = 29,
        Entry30 = 30,
        Entry31 = 31,
    }
);

fn runtime() -> MemoryRuntime<DefaultMemoryImpl> {
    let mut host = MemoryRuntime::new(DefaultMemoryImpl::default()).unwrap();
    let declarations =
        SealedDeclarationSnapshot::new(vec![MemoryRequest::new("m1", KEY).unwrap()]).unwrap();
    let grants = AllocationGrants::new(vec![MemoryAuthority::new("m1", "m1.").unwrap()]).unwrap();
    host.bootstrap(&declarations, &grants, &GenericAllocationPolicy)
        .unwrap();
    host
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
    with_host(|host| {
        Database::create(
            host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
            schema_for_open(),
            [53; 16],
        )
        .unwrap();
    });
}

fn operate<E: Entity<Key = u64>>(
    db: &mut Database,
    mode: u8,
    input: Row,
    row: &E,
    project: impl Fn(&E) -> Row,
) -> Observation {
    #[cfg(feature = "fluent")]
    if mode >= 10 {
        return fluent::operate::<E>(db, mode, input, project);
    }
    let mut batch = Batch::new();
    match mode {
        0 => {} // Read only, invoked as an update to retain one fixed export.
        1 => batch.insert(row).unwrap(),
        2 | 4 => batch.replace(row).unwrap(),
        3 => batch.delete::<E>(input.0).unwrap(),
        _ => panic!("unsupported fixture operation"),
    }
    if mode != 0 {
        db.commit(batch).unwrap();
    }
    if mode == 4 {
        ic_platform_runtime::system::trap("after retained-host commit");
    }
    (
        db.get::<E>(input.0).unwrap().as_ref().map(&project),
        db.page::<E>(None, 2)
            .unwrap()
            .rows
            .iter()
            .map(&project)
            .collect(),
        db.index_prefix::<E>("category,id", &[input.1.into()], 2)
            .unwrap()
            .iter()
            .map(project)
            .collect(),
        db.revision::<E>().unwrap(),
        u8::try_from(db.schema().entities().len()).unwrap(),
    )
}

/// Measure the selected entity-growth workload and return its observations.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "exercise",
    guard = crate::transport::admit::<1>,
    decode = crate::transport::decode::<_, 1>,
    encode = crate::transport::encode::<_, 1>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn exercise((selector, mode, row): (u8, u8, Row)) -> (Observation, [u64; 4], u64) {
    let start = ic_platform_runtime::raw::instruction_counter();
    with_host(|host| {
        let bootstrapped = ic_platform_runtime::raw::instruction_counter();
        let memory = host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap();
        let leased = ic_platform_runtime::raw::instruction_counter();
        let schema = schema_for_open();
        let described = ic_platform_runtime::raw::instruction_counter();
        let mut db = Database::open(memory, schema).unwrap();
        let opened = ic_platform_runtime::raw::instruction_counter();
        let result = dispatch(&mut db, selector, mode, row);
        let finished = ic_platform_runtime::raw::instruction_counter();
        (
            result,
            [
                bootstrapped - start,
                leased - bootstrapped,
                described - leased,
                opened - described,
            ],
            finished - opened,
        )
    })
}
