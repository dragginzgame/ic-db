//! Reopen rows written using the pre-existing schema admission path.

use ic_db::{Batch, Entity, Schema, testing::Fixture};
use ic_db_schema_owner_producer::{
    model::Accounts,
    model::{hub::User, shard::Entry},
};

#[test]
fn existing_rows_reopen_under_the_producer_export() {
    let fixture = Fixture::new("probe.account.v1");
    let schema = Schema::new(&[User::SCHEMA, Entry::SCHEMA]).unwrap();
    assert_eq!(schema.fingerprint(), Accounts::compiled().fingerprint());
    let user = User { id: 42 };
    let mut db = fixture.create(schema, [7; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&user).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    assert!(matches!(
        fixture.open(Schema::new(&[User::SCHEMA]).unwrap()),
        Err(ic_db::Error::IncompatibleSchema)
    ));
    let stored = fixture
        .open(Accounts::compiled())
        .unwrap()
        .get::<User>(42)
        .unwrap()
        .unwrap();
    assert_eq!(stored.encode().unwrap(), user.encode().unwrap());
}

#[test]
fn host_reopens_only_the_selected_canisters_allocations() {
    use ic_db::{CompiledCanister, Database};
    use ic_db_schema_owner_producer::model::{AuditEvent, Hub, Revision, Shard};
    use ic_storage::{
        AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
        SealedDeclarationSnapshot, StableKey, VectorMemory,
    };

    assert_eq!(Shard::STORES.len(), 1);
    assert_eq!(Shard::STORES[0].key, "probe.shard.v1");
    let [accounts, audit] = Hub::STORES else {
        panic!("hub must own exactly two stores")
    };
    assert_eq!(
        [accounts.key, audit.key],
        ["probe.account.v1", "probe.audit.v1"]
    );
    let backing = VectorMemory::default();
    let requests = Hub::STORES
        .iter()
        .map(|store| MemoryRequest::new("hub", store.key).unwrap())
        .collect();
    let declarations = SealedDeclarationSnapshot::new(requests).unwrap();
    let grants =
        AllocationGrants::new(vec![MemoryAuthority::new("hub", "probe.").unwrap()]).unwrap();
    let open_runtime = || {
        let mut runtime = MemoryRuntime::new(backing.clone()).unwrap();
        runtime
            .bootstrap(&declarations, &grants, &GenericAllocationPolicy)
            .unwrap();
        runtime
    };
    let accounts_key = StableKey::parse(accounts.key).unwrap();
    let audit_key = StableKey::parse(audit.key).unwrap();
    {
        let runtime = open_runtime();
        assert!(
            runtime
                .open_memory(&StableKey::parse(Shard::STORES[0].key).unwrap())
                .is_err()
        );
        assert!(
            Database::open(runtime.open_memory(&audit_key).unwrap(), (audit.schema)()).is_err()
        );
        let mut users = Database::create(
            runtime.open_memory(&accounts_key).unwrap(),
            (accounts.schema)(),
            [1; 16],
        )
        .unwrap();
        let mut events = Database::create(
            runtime.open_memory(&audit_key).unwrap(),
            (audit.schema)(),
            [2; 16],
        )
        .unwrap();
        let mut batch = Batch::new();
        batch.insert(&User { id: 42 }).unwrap();
        users.commit(batch).unwrap();
        let mut batch = Batch::new();
        batch
            .insert(&AuditEvent {
                id: 42,
                revision: Revision(9),
            })
            .unwrap();
        events.commit(batch).unwrap();
    }
    let runtime = open_runtime();
    assert!(matches!(
        Database::open(
            runtime.open_memory(&accounts_key).unwrap(),
            (audit.schema)()
        ),
        Err(ic_db::Error::IncompatibleSchema)
    ));
    let users = Database::open(
        runtime.open_memory(&accounts_key).unwrap(),
        (accounts.schema)(),
    )
    .unwrap();
    let events =
        Database::open(runtime.open_memory(&audit_key).unwrap(), (audit.schema)()).unwrap();
    assert_eq!(users.get::<User>(42).unwrap().unwrap().id, 42);
    assert_eq!(
        events.get::<AuditEvent>(42).unwrap().unwrap().revision,
        Revision(9)
    );
    assert!(events.get::<User>(42).is_err());
}
