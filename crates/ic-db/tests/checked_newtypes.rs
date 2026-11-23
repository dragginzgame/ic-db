//! Checked scalar contracts share the primitive representation across declarations.

use ic_db::design::prelude::*;
use ic_db::{Batch, Error, testing::Fixture};

#[newtype(
    item(prim = "Text", max_bytes = 200),
    validate(non_blank, max_chars = 50)
)]
struct CollectionName {}

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(field = "name", unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "name", value(item(is = "CollectionName")))
    )
)]
struct Collection {}

#[test]
fn stored_and_indexed_names_preserve_whitespace_and_escaped_nul() {
    let fixture = Fixture::new("test.names.v1");
    let schema = || Schema::new(&[Collection::SCHEMA]).unwrap();
    let mut db = fixture.create(schema(), [1; 16]).unwrap();
    let name = "  é\0 game  ";
    let row = Collection {
        id: 1,
        name: CollectionName::try_from(name.to_owned()).unwrap(),
    };
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let db = fixture.open(schema()).unwrap();
    assert_eq!(
        db.query::<Collection>()
            .eq(Collection::NAME, name)
            .try_one()
            .unwrap(),
        Some(row)
    );
    for invalid in [" ".to_owned(), "é".repeat(51)] {
        assert!(matches!(
            db.query::<Collection>()
                .eq(Collection::NAME, invalid.as_str())
                .fetch(),
            Err(Error::InvalidOperand)
        ));
        let mut bytes = 1_u64.to_le_bytes().to_vec();
        bytes.extend((invalid.len() as u32).to_le_bytes());
        bytes.extend(invalid.bytes());
        assert!(matches!(Collection::decode(&bytes), Err(Error::Corrupt)));
    }
}

#[test]
fn rule_order_has_one_identity_and_a_changed_limit_requires_transition() {
    macro_rules! schema {
        ($($rules:tt)*) => {{
            #[newtype(name = "CollectionName", item(prim = "Text", max_bytes = 200), validate($($rules)*))]
            struct Name {}
            #[entity(
                name = "Collection",
                store = ic_db::testing::FixtureStore,
                pk(field = "id"),
                index(field = "name", unique),
                fields(
                    field(name = "id", value(item(prim = "Nat64"))),
                    field(name = "name", value(item(is = "Name")))
                )
            )]
            struct Row {}
            Schema::new(&[Row::SCHEMA]).unwrap()
        }};
    }
    let fixture = Fixture::new("test.names.v1");
    let current = || Schema::new(&[Collection::SCHEMA]).unwrap();
    let mut db = fixture.create(current(), [1; 16]).unwrap();
    let mut batch = Batch::new();
    let row = Collection {
        id: 1,
        name: CollectionName::try_from("é".repeat(50)).unwrap(),
    };
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let reordered = schema!(max_chars = 50, non_blank);
    assert_eq!(current().fingerprint(), reordered.fingerprint());
    drop(fixture.open(reordered).unwrap());
    let tightened = schema!(non_blank, max_chars = 40);
    assert_ne!(current().fingerprint(), tightened.fingerprint());
    assert!(fixture.open(tightened).is_err());
    assert_eq!(
        fixture
            .open(current())
            .unwrap()
            .get::<Collection>(1)
            .unwrap(),
        Some(row)
    );
}

#[test]
fn describe_keeps_the_checked_text_rule_and_unique_index() {
    let schema = Schema::new(&[Collection::SCHEMA]).unwrap();
    let description = schema.describe(Some("Collection")).unwrap();
    assert_eq!(description.fingerprint, schema.fingerprint());
    let json = serde_json::to_value(description).unwrap();
    let entity = &json["entities"][0];
    assert_eq!(entity["primary_key"], "id");
    assert_eq!(
        entity["fields"][1]["value"]["Named"]["name"],
        "CollectionName"
    );
    assert_eq!(entity["indexes"][0]["unique"], true);
    assert_eq!(entity["indexes"][0]["fields"], serde_json::json!(["name"]));
    let checked = &json["types"][0]["body"]["Checked"];
    assert_eq!(
        checked["primitive"],
        serde_json::json!({"Text":{"max_bytes":200}})
    );
    assert_eq!(
        checked["rule"]["Text"],
        serde_json::json!({
            "max_bytes": 200, "non_blank": true, "max_chars": 50
        })
    );
    assert!(matches!(
        schema.describe(Some("Missing")),
        Err(Error::UnknownEntity)
    ));
}
