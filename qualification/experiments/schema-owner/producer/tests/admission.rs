//! Reopen rows written using the pre-existing schema admission path.

use ic_db::{Batch, Entity, Schema, testing::Fixture};
use ic_db_schema_owner_producer::{
    compiled,
    model::{hub::User, shard::Entry},
};

#[test]
fn existing_rows_reopen_under_the_producer_export() {
    let fixture = Fixture::new("probe.account.v1");
    let schema = Schema::new(&[User::SCHEMA, Entry::SCHEMA]).unwrap();
    assert_eq!(schema.fingerprint(), compiled().fingerprint());
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
        .open(compiled())
        .unwrap()
        .get::<User>(42)
        .unwrap()
        .unwrap();
    assert_eq!(stored.encode().unwrap(), user.encode().unwrap());
}
