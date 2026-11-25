//! A host hands independent helpers sessions on the same attached database.

use ic_db::design::prelude::*;
use ic_db::{Batch, DatabaseSession, Error, testing::Fixture};
use std::cell::RefCell;

///
/// Friendship
/// Exercises indexed access by `owner` and `peer` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["owner", "peer"], unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "owner", value(item(prim = "Nat64"))),
        field(name = "peer", value(item(prim = "Nat64"))),
        field(name = "accepted", value(item(prim = "Bool")))
    )
)]
struct Friendship {}

thread_local! {
    static ATTACHED: RefCell<Option<DatabaseSession>> = const { RefCell::new(None) };
}

fn session() -> DatabaseSession {
    ATTACHED.with(|slot| slot.borrow().as_ref().unwrap().clone())
}

fn find(owner: u64, peer: u64) -> Result<Option<Friendship>, Error> {
    session()
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

///
/// Other
/// Exercises a row containing only its `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
struct Other {}

fn attached() -> DatabaseSession {
    DatabaseSession::new(
        Fixture::new("test.friendships.v1")
            .create(Schema::new(&[Friendship::SCHEMA]).unwrap(), [1; 16])
            .unwrap(),
    )
}

#[test]
fn session_queries_observe_execution_time_and_preserve_page_and_point_rules() {
    let db = attached();
    let writer = db.clone();
    let q = db
        .query::<Friendship>()
        .eq(Friendship::OWNER, 11_u64)
        .eq(Friendship::PEER, 20_u64);
    let bulk = db
        .clone()
        .get_many::<Friendship>(&[2, 99, 2])
        .select(|row| row.owner);
    for (id, owner) in [(1, 10), (2, 11)] {
        writer
            .insert(&Friendship {
                id,
                owner,
                peer: 20,
                accepted: false,
            })
            .unwrap();
    }
    assert_eq!(bulk.fetch().unwrap(), [Some(11), None, Some(11)]);
    assert_eq!(q.work_limit(1).try_one().unwrap().unwrap().id, 2);
    let unique = |owner, peer| db.get_unique(&FriendshipByOwnerAndPeer { owner, peer });
    assert_eq!(unique(10, 20).unwrap().unwrap().id, 1);
    assert_eq!(unique(11, 20).unwrap().unwrap().id, 2);
    assert!(unique(20, 11).unwrap().is_none());
    assert!(matches!(
        db.query::<Friendship>()
            .eq(Friendship::PEER, 20_u64)
            .try_one(),
        Err(Error::MultipleMatches)
    ));
    let page = db.query::<Friendship>().limit(1).fetch().unwrap();
    assert_eq!(page.rows[0].id, 1);
    let cursor = page.next.unwrap();
    let next = db.query::<Friendship>().after(&cursor).fetch().unwrap();
    assert_eq!(next.rows[0].id, 2);
    assert!(matches!(
        db.query::<Friendship>()
            .eq(Friendship::OWNER, 10_u64)
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        db.query::<Friendship>().work_limit(0).fetch(),
        Err(Error::Limit)
    ));
}

#[test]
fn single_mutations_match_batches_including_refusals_and_index_effects() {
    let direct = attached();
    let batched = attached();
    let mut row = Friendship {
        id: 1,
        owner: 10,
        peer: 20,
        accepted: false,
    };
    macro_rules! check {
        ($method:ident $(::<$entity:ty>)? ($arg:expr), $outcome:pat) => {{
            let actual = direct.$method $(::<$entity>)? ($arg).map(|_| ());
            let mut batch = Batch::new();
            let expected = batch.$method $(::<$entity>)? ($arg).and_then(|()| batched.commit(batch));
            assert!(matches!(actual, $outcome), "{actual:?}");
            assert!(matches!(expected, $outcome), "{expected:?}");
            assert_eq!(direct.get::<Friendship>(1).unwrap(), batched.get::<Friendship>(1).unwrap());
            assert_eq!(direct.read().unwrap().revision::<Friendship>().unwrap(), batched.read().unwrap().revision::<Friendship>().unwrap());
            for owner in [10, 11] {
                let key = FriendshipByOwnerAndPeer { owner, peer: 20 };
                assert_eq!(direct.get_unique(&key).unwrap(), batched.get_unique(&key).unwrap());
            }
        }};
    }
    check!(insert(&row), Ok(()));
    check!(insert(&row), Err(Error::Exists));
    row.owner = 11;
    check!(replace(&row), Ok(()));
    let duplicate = Friendship { id: 2, ..row };
    check!(insert(&duplicate), Err(Error::UniqueViolation));
    {
        let _guards = (direct.read().unwrap(), batched.read().unwrap());
        check!(delete::<Friendship>(1), Err(Error::BorrowConflict));
    }
    check!(delete::<Friendship>(1), Ok(()));
    check!(delete::<Friendship>(1), Ok(()));
    check!(replace(&row), Err(Error::Missing));
    check!(upsert(&row), Ok(()));
    row.owner = 10;
    check!(upsert(&row), Ok(()));
    assert_eq!(direct.upsert(&row).unwrap(), row);
    check!(
        upsert(&Friendship { id: 2, ..row }),
        Err(Error::UniqueViolation)
    );
    check!(delete::<Other>(1), Err(Error::UnknownEntity));
}

#[test]
fn query_execution_refuses_an_active_commit_and_recovers() {
    use ic_db::design::prelude::*;
    use ic_db::{
        __private::{Decoder, Encoder},
        TypeSchema,
    };

    ///
    /// Payload
    /// Carries the `Nat8` value of `value`.
    ///

    #[record(fields(field(name = "value", value(item(prim = "Nat8")))))]
    struct Payload {}
    #[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
    struct Probe(Payload);
    thread_local! {
        static DURING_ADMISSION: RefCell<Option<DatabaseSession>> = const { RefCell::new(None) };
    }
    impl Value for Probe {
        const NAME: &'static str = Payload::NAME;
        fn definition() -> &'static TypeSchema {
            if let Some(session) = DURING_ADMISSION.take() {
                assert!(matches!(
                    session.get_many::<Friendship>(&[1]).fetch(),
                    Err(Error::BorrowConflict)
                ));
                assert!(matches!(
                    session.query::<Friendship>().fetch(),
                    Err(Error::BorrowConflict)
                ));
                assert!(matches!(
                    session.get_unique(&FriendshipByOwnerAndPeer {
                        owner: 11,
                        peer: 20
                    }),
                    Err(Error::BorrowConflict)
                ));
            }

            Payload::definition()
        }

        fn encode_into(&self, writer: &mut Encoder) -> Result<(), Error> {
            self.0.encode_into(writer)
        }

        fn decode_from(reader: &mut Decoder<'_>) -> Result<Self, Error> {
            Payload::decode_from(reader).map(Self)
        }
    }

    ///
    /// Checked
    /// Stores `probe` under the `id` primary key.
    ///

    #[entity(
        store = ic_db::testing::FixtureStore,
        pk(field = "id"),
        fields(
            field(name = "id", value(item(prim = "Nat64"))),
            field(name = "probe", value(item(is = "Probe")))
        )
    )]
    struct Checked {}
    let db = DatabaseSession::new(
        Fixture::new("test.reentrant.v1")
            .create(
                Schema::new(&[Friendship::SCHEMA, Checked::SCHEMA]).unwrap(),
                [1; 16],
            )
            .unwrap(),
    );
    let mut batch = Batch::new();
    batch
        .insert(&Checked {
            id: 1,
            probe: Probe(Payload { value: 1 }),
        })
        .unwrap();
    DURING_ADMISSION.set(Some(db.clone()));
    db.commit(batch).unwrap();
    assert!(DURING_ADMISSION.take().is_none());
    assert_eq!(db.query::<Checked>().try_one().unwrap().unwrap().id, 1);
}

#[test]
fn named_unique_keys_use_declared_order_types_and_sparse_membership() {
    use ic_db::Principal;

    ///
    /// Route
    /// Exercises indexed access by `alias` on rows keyed by `id`.
    ///

    #[entity(
        store = ic_db::testing::FixtureStore,
        pk(field = "id"),
        index(name = "account", fields = ["issuer", "subject"], unique),
        index(field = "alias", unique, present_only),
        fields(
            field(name = "id", value(item(prim = "Nat64"))),
            field(name = "issuer", value(item(prim = "Principal"))),
            field(name = "subject", value(item(prim = "Principal"))),
            field(name = "alias", value(opt, item(prim = "Text", max_bytes = 8)))
        )
    )]
    struct Route {}
    let db = DatabaseSession::new(
        Fixture::new("test.routes.v1")
            .create(Schema::new(&[Route::SCHEMA]).unwrap(), [1; 16])
            .unwrap(),
    );
    let subject = Principal::from_slice(&[9]);
    for id in [1_u8, 2] {
        db.insert(&Route {
            id: id.into(),
            issuer: Principal::from_slice(&[id]),
            subject,
            alias: (id == 1).then(|| String::from("first")),
        })
        .unwrap();
    }
    let key = |id| RouteByIssuerAndSubject {
        subject,
        issuer: Principal::from_slice(&[id]),
    };
    let direct = db.read().unwrap();
    assert_eq!(direct.get_unique(&key(2)).unwrap().unwrap().id, 2);
    assert!(direct.get_unique(&key(3)).unwrap().is_none());
    assert!(matches!(
        direct.get_unique(&RouteByAlias {
            alias: "exceeds bound".into()
        }),
        Err(Error::Limit)
    ));
    for id in [1_u8, 2] {
        assert_eq!(db.get_unique(&key(id)).unwrap().unwrap().id, u64::from(id));
    }
    assert!(db.get_unique(&key(3)).unwrap().is_none());
    assert_eq!(
        db.get_unique(&RouteByAlias {
            alias: "first".into()
        })
        .unwrap()
        .unwrap()
        .id,
        1
    );
    assert!(
        db.get_unique(&RouteByAlias {
            alias: String::new()
        })
        .unwrap()
        .is_none()
    );
    assert!(matches!(
        db.get_unique(&RouteByAlias {
            alias: "exceeds bound".into()
        }),
        Err(Error::Limit)
    ));
    drop(direct);
    let input = |id: u8| {
        Route::insert()
            .id(u64::from(id))
            .issuer(Principal::from_slice(&[id]))
            .subject(subject)
    };
    assert!(matches!(
        db.insert_if_absent::<RouteByAlias>(input(3)),
        Ok(ic_db::InsertOutcome::Inserted(_))
    ));
    assert!(matches!(
        db.insert_if_absent::<RouteByAlias>(input(4).alias(Some("first".into()))),
        Ok(ic_db::InsertOutcome::AlreadyPresent)
    ));
}

///
/// Discovery
/// Records one wallet/project association and its independent receipt.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["wallet", "project"], unique),
    index(field = "receipt", unique),
    index(field = "parent"),
    fields(
        field(name = "id", value(item(prim = "Ulid"))),
        field(name = "wallet", value(item(prim = "Nat64"))),
        field(name = "project", value(item(prim = "Nat64"))),
        field(name = "receipt", value(item(prim = "Nat64"))),
        field(name = "note", value(item(prim = "Text", max_bytes = 8))),
        field(name = "parent", value(opt, item(prim = "Ulid")), references(entity = "Discovery"))
    ),
    timestamps
)]
struct Discovery {}

#[test]
fn conditional_insert_ignores_only_the_selected_unique_conflict() {
    use ic_db::InsertOutcome::{AlreadyPresent, Inserted};

    let fixture = Fixture::new("test.discovery.v1");
    let db = DatabaseSession::new(
        fixture
            .create(Schema::new(&[Discovery::SCHEMA]).unwrap(), [1; 16])
            .unwrap(),
    );
    let input = |project, receipt, note: &str| {
        Discovery::insert()
            .wallet(1)
            .project(project)
            .receipt(receipt)
            .note(note.to_owned())
    };
    let insert = |input| db.insert_if_absent::<DiscoveryByWalletAndProject>(input);
    let Inserted(original) = insert(input(2, 3, "first")).unwrap() else {
        panic!("expected a new row")
    };
    assert!(matches!(insert(input(4, 5, "other")), Ok(Inserted(_))));
    let revision = db.read().unwrap().revision::<Discovery>().unwrap();
    assert_eq!(insert(input(2, 6, "changed")).unwrap(), AlreadyPresent);
    assert!(matches!(
        insert(input(2, 5, "conflict")),
        Err(Error::UniqueViolation)
    ));
    assert!(matches!(
        insert(input(7, 5, "conflict")),
        Err(Error::UniqueViolation)
    ));
    assert!(matches!(
        insert(input(2, 6, "repeat").id(original.id)),
        Err(Error::Exists)
    ));
    assert!(matches!(
        insert(input(2, 6, "too long for note")),
        Err(Error::Limit)
    ));
    assert!(matches!(
        insert(input(2, 6, "repeat").parent(Some(Ulid::MIN))),
        Err(Error::ReferenceViolation)
    ));
    assert_eq!(
        db.read().unwrap().revision::<Discovery>().unwrap(),
        revision
    );
    assert_eq!(db.read().unwrap().cardinality::<Discovery>().unwrap(), 2);
    assert_eq!(db.get::<Discovery>(original.id()).unwrap(), Some(original));
}
