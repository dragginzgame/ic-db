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
    use ic_db::{CanisterSession, CompiledCanister, Database, DatabaseSession};
    use ic_db_schema_owner_producer::model::{
        AuditEvent, AuditEventByRevision, Hub, Revision, Shard,
    };
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
        let db = CanisterSession::<Hub>::try_attach(|store| {
            let memory = runtime
                .open_memory(&StableKey::parse(store.key).unwrap())
                .unwrap();
            let incarnation = if store.key == accounts.key {
                [1; 16]
            } else {
                [2; 16]
            };
            Database::create(memory, (store.schema)(), incarnation).map(DatabaseSession::new)
        })
        .unwrap();
        let events = db.query::<AuditEvent>();
        let mut batch = db.batch();
        batch.insert_input(User::insert().id(42)).unwrap();
        batch.insert(&Entry { id: 42 }).unwrap();
        assert!(db.get::<User>(42).unwrap().is_none());
        assert_eq!(batch.commit_returning::<User>(42).unwrap().id, 42);
        let mut refused = db.batch();
        refused.delete::<Entry>(42).unwrap();
        refused.insert(&User { id: 42 }).unwrap();
        assert!(matches!(refused.commit(), Err(ic_db::Error::Exists)));
        let event = db
            .insert_input(AuditEvent::insert().id(42).revision(Revision(9)))
            .unwrap();
        assert_eq!(event.revision, Revision(9));
        assert!(matches!(
            db.insert_input(AuditEvent::insert().id(42).revision(Revision(10))),
            Err(ic_db::Error::Exists)
        ));
        assert_eq!(events.try_one().unwrap().unwrap().revision, Revision(9));
        let mut batch = db.batch();
        batch
            .upsert(&AuditEvent {
                id: 7,
                revision: Revision(70),
            })
            .unwrap();
        assert_eq!(
            batch.commit_returning::<AuditEvent>(7).unwrap().revision,
            Revision(70)
        );
        let stored = db
            .upsert(&AuditEvent {
                id: 7,
                revision: Revision(71),
            })
            .unwrap();
        assert_eq!(db.get::<AuditEvent>(7).unwrap(), Some(stored));
    }
    let runtime = open_runtime();
    assert!(matches!(
        Database::open(
            runtime.open_memory(&accounts_key).unwrap(),
            (audit.schema)()
        ),
        Err(ic_db::Error::IncompatibleSchema)
    ));
    let reopen = |store: &ic_db::CompiledStore| {
        let memory = runtime
            .open_memory(&StableKey::parse(store.key).unwrap())
            .unwrap();
        Database::open(memory, (store.schema)()).map(DatabaseSession::new)
    };
    let failed = CanisterSession::<Hub>::try_attach(|store| {
        if store.key == audit.key {
            return Err(ic_db::Error::Maintenance);
        }
        reopen(store)
    });
    assert!(matches!(failed, Err(ic_db::Error::Maintenance)));
    let db = CanisterSession::<Hub>::try_attach(reopen).unwrap();
    assert_eq!(db.get::<Entry>(42).unwrap().unwrap().id, 42);
    assert_eq!(db.query::<User>().try_one().unwrap().unwrap().id, 42);
    assert_eq!(
        db.get::<AuditEvent>(42).unwrap().unwrap().revision,
        Revision(9)
    );
    db.replace(&AuditEvent {
        id: 42,
        revision: Revision(10),
    })
    .unwrap();
    let key = AuditEventByRevision {
        revision: Revision(10),
    };
    assert_eq!(db.get_unique(&key).unwrap().unwrap().id, 42);
    assert!(matches!(
        db.insert_if_absent::<AuditEventByRevision>(
            AuditEvent::insert().id(1).revision(Revision(10))
        ),
        Ok(ic_db::InsertOutcome::AlreadyPresent)
    ));
    db.clone().delete::<User>(42).unwrap();
    assert!(db.get::<User>(42).unwrap().is_none());
    let keys = [42, 1, 42];
    assert!(
        db.get_many::<User>(&keys)
            .fetch()
            .unwrap()
            .iter()
            .all(Option::is_none)
    );
    assert_eq!(
        db.get_many::<AuditEvent>(&keys)
            .select(|row| row.revision)
            .fetch()
            .unwrap(),
        [Some(Revision(10)), None, Some(Revision(10))]
    );
    let revisions: Vec<_> = db
        .get_many::<AuditEvent>(&keys.map(ic_db::Id::<AuditEvent>::from_key))
        .fetch()
        .unwrap()
        .into_iter()
        .map(|event| event.map(|event| event.revision))
        .collect();
    assert_eq!(revisions, [Some(Revision(10)), None, Some(Revision(10))]);
    assert!(matches!(
        db.get_many::<AuditEvent>(&[42; 33]).fetch(),
        Err(ic_db::Error::Limit)
    ));
    assert_eq!(
        db.get::<AuditEvent>(42).unwrap().unwrap().revision,
        Revision(10)
    );
}

#[test]
fn single_store_root_reopens_its_original_key_and_row() {
    use ic_db::{CanisterSession, CompiledCanister, DatabaseSession};
    use ic_db_schema_owner_producer::model::{Revision, Shard, ShardRecord};

    let [store] = Shard::STORES else {
        panic!("expected one shard store")
    };
    let fixture = Fixture::new(store.key);
    let reopen = || {
        CanisterSession::<Shard>::try_attach(|store| {
            fixture.open((store.schema)()).map(DatabaseSession::new)
        })
    };
    assert!(reopen().is_err());
    let session = CanisterSession::<Shard>::try_attach(|store| {
        fixture
            .create((store.schema)(), [3; 16])
            .map(DatabaseSession::new)
    })
    .unwrap();
    session
        .insert(&ShardRecord {
            id: 42,
            revision: Revision(7),
        })
        .unwrap();
    drop(session);
    let stored = reopen().unwrap().get::<ShardRecord>(42).unwrap().unwrap();
    assert_eq!(stored.revision, Revision(7));
}
