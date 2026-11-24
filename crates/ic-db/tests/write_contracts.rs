//! Write contracts at the schema and storage boundaries.

use ic_db::design::prelude::*;
use ic_db::{
    Batch, DatabaseSession, Error,
    testing::{Fixture, FixtureStore},
};

/// A thumbnail with an explicit encoded-row budget.
#[entity(
    store = FixtureStore,
    pk(field = "id"),
    max_row_bytes = 32768,
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 32768)))
    ),
    timestamps
)]
struct Thumbnail {}

/// A bounded list of references to thumbnails, with repeated references permitted.
#[entity(
    store = FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "images", value(opt, many, max_items = 4, item(prim = "Nat64")), references(entity = "Thumbnail"))
    )
)]
struct Gallery {}

#[test]
fn describe_reports_reference_targets_independently_of_live_counts() {
    let schema = Schema::new(&[Gallery::SCHEMA, Thumbnail::SCHEMA]).unwrap();
    let describe = || serde_json::to_value(schema.describe(Some("Gallery")).unwrap()).unwrap();
    let before = describe();
    let images = &before["entities"][0]["fields"][1];
    assert_eq!(images["reference"]["entity_name"], "Thumbnail");
    assert_eq!(images["optional"], true);
    assert_eq!(images["value"]["List"]["max_items"], 4);
    let fixture = Fixture::new("media.data.v1");
    let mut db = fixture.create(schema.clone(), [1; 16]).unwrap();
    assert_eq!(db.cardinality_by_name("Gallery").unwrap(), 0);
    let mut batch = Batch::new();
    batch
        .insert(&Gallery {
            id: 1,
            images: None,
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.cardinality_by_name("Gallery").unwrap(), 1);
    assert_eq!(describe(), before);
}

#[test]
fn list_references_follow_final_batch_state_and_large_rows_reopen() {
    let fixture = Fixture::new("media.data.v1");
    let schema = Schema::new(&[Gallery::SCHEMA, Thumbnail::SCHEMA]).unwrap();
    let session = DatabaseSession::new(fixture.create(schema.clone(), [1; 16]).unwrap());
    let mut batch = Batch::at_for_test(Timestamp::from_millis(100));
    let gallery = Gallery {
        id: 2,
        images: Some(vec![1, 1]),
    };
    batch.insert(&gallery).unwrap();
    let photo = batch
        .insert_input(Thumbnail::insert().id(1).payload(vec![7; 30_000]))
        .unwrap();
    session.commit(batch).unwrap();
    drop(session);
    let session = DatabaseSession::new(fixture.open(schema).unwrap());
    assert_eq!(session.get(photo.id()).unwrap(), Some(photo.clone()));
    assert!(matches!(
        session.delete(photo.id()),
        Err(Error::ReferenceViolation)
    ));
    let mut updated = photo.clone();
    updated.payload.resize(32768 - 28, 3);
    let stored = session.replace(&updated).unwrap();
    assert_eq!(stored.encode().unwrap().len(), 32768);
    assert_eq!(stored.created_at, photo.created_at);
    assert!(stored.updated_at > photo.updated_at);
    assert_eq!(session.get(stored.id()).unwrap(), Some(stored.clone()));
    assert_eq!(session.replace(&stored).unwrap(), stored);
    updated = stored;
    updated.payload.push(0);
    assert!(matches!(session.replace(&updated), Err(Error::Limit)));
    let invalid = Gallery {
        images: Some(vec![9]),
        ..gallery.clone()
    };
    assert!(matches!(
        session.replace(&invalid),
        Err(Error::ReferenceViolation)
    ));
    assert_eq!(session.get(gallery.id()).unwrap(), Some(gallery.clone()));
    let mut batch = Batch::new();
    batch.delete(photo.id()).unwrap();
    batch
        .replace(&Gallery {
            images: None,
            ..gallery
        })
        .unwrap();
    session.commit(batch).unwrap();
    let revision = session.read().unwrap().revision::<Thumbnail>().unwrap();
    session.delete(photo.id()).unwrap();
    assert_eq!(
        session.read().unwrap().revision::<Thumbnail>().unwrap(),
        revision
    );
}

#[test]
fn upsert_repoints_references_using_the_final_batch_state() {
    let schema = Schema::new(&[Gallery::SCHEMA, Thumbnail::SCHEMA]).unwrap();
    let fixture = Fixture::new("media.data.v1");
    let mut db = fixture.create(schema.clone(), [1; 16]).unwrap();
    let mut gallery = Gallery {
        id: 9,
        images: Some(vec![1]),
    };
    let mut batch = Batch::new();
    batch.upsert(&gallery).unwrap();
    let first = batch
        .insert_input(Thumbnail::insert().id(1).payload(vec![1]))
        .unwrap();
    assert_eq!(db.commit_returning(batch, gallery.id()).unwrap(), gallery);

    gallery.images = Some(vec![2]);
    let mut batch = Batch::new();
    batch.delete(first.id()).unwrap();
    batch.upsert(&gallery).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    assert_eq!(db.get::<Gallery>(9).unwrap().unwrap().images, Some(vec![1]));
    assert_eq!(db.get(first.id()).unwrap(), Some(first.clone()));

    let mut batch = Batch::new();
    batch.delete(first.id()).unwrap();
    batch.upsert(&gallery).unwrap();
    let second = batch
        .insert_input(Thumbnail::insert().id(2).payload(vec![2]))
        .unwrap();
    assert_eq!(db.commit_returning(batch, gallery.id()).unwrap(), gallery);
    drop(db);
    let mut db = fixture.open(schema).unwrap();
    assert_eq!(db.get(gallery.id()).unwrap(), Some(gallery));
    assert_eq!(db.get(first.id()).unwrap(), None);
    assert_eq!(db.get(second.id()).unwrap(), Some(second.clone()));
    let mut batch = Batch::new();
    batch.delete(second.id()).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
}

macro_rules! wide {
    ($($field:tt),*) => {
        #[entity(store = FixtureStore, pk(field = "id"), fields(
            field(name = "id", value(item(prim = "Nat64"))),
            $(field(name = $field, value(item(prim = "Nat8")), default = 0),)*
            field(name = "z", value(item(prim = "Nat8")))
        ))]
        struct Wide {}
    }
}

wide!(
    "a01", "a02", "a03", "a04", "a05", "a06", "a07", "a08", "a09", "a10", "a11", "a12", "a13",
    "a14", "a15", "a16", "a17", "a18", "a19", "a20", "a21", "a22", "a23", "a24", "a25", "a26",
    "a27", "a28", "a29", "a30"
);

/// Projection of field 32 in canonical order.
#[record(fields(field(name = "z", value(item(prim = "Nat8")))))]
struct Last {}

#[test]
fn projection_and_replacement_use_the_highest_field_bit() {
    let db = Fixture::new("wide.rows.v1")
        .create(Schema::new(&[Wide::SCHEMA]).unwrap(), [1; 16])
        .unwrap();
    let session = DatabaseSession::new(db);
    let mut row = session.insert_input(Wide::insert().id(1).z(9)).unwrap();
    row.z = 12;
    session.replace(&row).unwrap();
    let page = session.query::<Wide>().fetch_as::<Last>().unwrap();
    assert_eq!(page.rows, vec![Last { z: 12 }]);
}
