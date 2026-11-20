//! Measure a single game store before and after an executable upgrade.

use accounts::model::{Ability, Game, Spell};
use ic_db::{Batch, CompiledCanister, Database, Error};
use ic_db_qualification_transport as transport;
use ic_platform_runtime::raw::instruction_counter;
use ic_storage::{
    AllocationGrants, DefaultMemoryImpl, GenericAllocationPolicy, MemoryAuthority, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot, StableKey,
};

fn measured<T>(work: impl FnOnce() -> T) -> (T, u64) {
    let start = instruction_counter();
    let result = work();
    (result, instruction_counter() - start)
}

// exercise
// Create and seed the store, or reopen and check restricted deletion after upgrade.
// guard  : validates the method 1 request frame.
// limits : 128 bytes request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "exercise",
    guard = crate::transport::admit::<1>,
    decode = crate::transport::decode::<_, 1>,
    encode = crate::transport::encode::<_, 1>,
    max_bytes = 128,
    on_error = crate::transport::endpoint_error
)]
fn exercise(create: bool) -> (u64, [u64; 6]) {
    let [store] = Game::STORES else {
        panic!("expected one store")
    };
    let (schema, admission) = measured(|| (store.schema)());
    let width = schema.entities().len() as u64;
    assert_eq!(schema.entities().first().unwrap().name, "Ability");
    assert_eq!(schema.entities().last().unwrap().name, "Spell");
    let mut host = MemoryRuntime::new(DefaultMemoryImpl::default()).unwrap();
    let declarations =
        SealedDeclarationSnapshot::new(vec![MemoryRequest::new("game", store.key).unwrap()])
            .unwrap();
    let grants =
        AllocationGrants::new(vec![MemoryAuthority::new("game", "probe.").unwrap()]).unwrap();
    host.bootstrap(&declarations, &grants, &GenericAllocationPolicy)
        .unwrap();
    let memory = host
        .open_memory(&StableKey::parse(store.key).unwrap())
        .unwrap();

    if create {
        let (mut db, creation) = measured(|| Database::create(memory, schema, [1; 16]).unwrap());
        let mut batch = Batch::new();
        batch.insert(&Spell { id: 9, ability: 7 }).unwrap();
        batch.insert(&Ability { id: 7 }).unwrap();
        let ((), commit) = measured(|| db.commit(batch).unwrap());
        return (width, [admission, creation, commit, 0, 0, 0]);
    }

    let (mut db, reopen) = measured(|| Database::open(memory, schema).unwrap());
    let (spell, lookup) = measured(|| db.get::<Spell>(9).unwrap().unwrap());
    assert_eq!(spell.ability, 7);
    let mut delete = Batch::new();
    delete.delete::<Ability>(7).unwrap();
    let (refusal, restricted) = measured(|| db.commit(delete));
    assert!(matches!(refusal, Err(Error::ReferenceViolation)));
    assert_eq!(db.get::<Ability>(7).unwrap().unwrap().id, 7);

    let mut delete = Batch::new();
    delete.delete::<Ability>(7).unwrap();
    delete.delete::<Spell>(9).unwrap();
    let ((), cleanup) = measured(|| db.commit(delete).unwrap());
    assert!(db.get::<Spell>(9).unwrap().is_none());
    assert!(db.get::<Ability>(7).unwrap().is_none());
    (width, [admission, reopen, 0, lookup, restricted, cleanup])
}
