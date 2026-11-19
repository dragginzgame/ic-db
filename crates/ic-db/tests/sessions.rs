//! A host hands independent helpers sessions on the same attached database.
use ic_db::{Batch, DatabaseSession, Entity, Error, Schema, entity, testing::Fixture};
use std::cell::RefCell;

#[entity(pk(field="id"), index(fields=["owner", "peer"], unique), fields(
    field(name="id", value(item(prim="Nat64"))),
    field(name="owner", value(item(prim="Nat64"))),
    field(name="peer", value(item(prim="Nat64"))),
    field(name="accepted", value(item(prim="Bool")))
))]
struct Friendship {}

thread_local! {
    static ATTACHED: RefCell<Option<DatabaseSession>> = const { RefCell::new(None) };
}

fn session() -> DatabaseSession {
    ATTACHED.with(|slot| slot.borrow().as_ref().unwrap().clone())
}

fn find(owner: u64, peer: u64) -> Result<Option<Friendship>, Error> {
    session()
        .read()?
        .query::<Friendship>()
        .eq(Friendship::OWNER, owner)
        .eq(Friendship::PEER, peer)
        .try_one()
}

#[test]
fn nested_helpers_share_one_attachment_and_commit_atomically() {
    let fixture = Fixture::new("test.friendships.v1");
    let db = fixture
        .create(Schema::new(&[Friendship::SCHEMA]).unwrap(), [1; 16])
        .unwrap();
    ATTACHED.with(|slot| assert!(slot.replace(Some(DatabaseSession::new(db))).is_none()));

    let outer = session();
    let mut row = Friendship {
        id: 1,
        owner: 10,
        peer: 20,
        accepted: false,
    };
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    outer.commit(batch).unwrap();

    row = session().get::<Friendship>(1).unwrap().unwrap();
    assert!(!find(10, 20).unwrap().unwrap().accepted);
    row.accepted = true;
    let update = || {
        let mut batch = Batch::new();
        batch.replace(&row).unwrap();
        batch
    };
    let read = outer.read().unwrap();
    let query = read
        .query::<Friendship>()
        .eq(Friendship::OWNER, 10_u64)
        .eq(Friendship::PEER, 20_u64);
    assert!(matches!(
        session().commit(update()),
        Err(Error::BorrowConflict)
    ));
    assert!(!query.try_one().unwrap().unwrap().accepted);
    drop(read);
    outer.commit(update()).unwrap();
    assert!(find(10, 20).unwrap().unwrap().accepted);

    let mut refused = Batch::new();
    row.accepted = false;
    refused.replace(&row).unwrap();
    row.id = 2;
    refused.insert(&row).unwrap();
    assert!(matches!(outer.commit(refused), Err(Error::UniqueViolation)));
    assert!(find(10, 20).unwrap().unwrap().accepted);
    assert!(session().get::<Friendship>(2).unwrap().is_none());

    let owner = ATTACHED.take().unwrap();
    let Err(owner) = owner.try_into_database() else {
        panic!("outer handle is still live")
    };
    drop(outer);
    let db = owner.try_into_database().ok().unwrap();
    assert_eq!(db.cardinality::<Friendship>().unwrap(), 1);
}
