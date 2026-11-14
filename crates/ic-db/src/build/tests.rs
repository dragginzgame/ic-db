use super::{Error, Registry};
use crate::{EntitySchema, FieldSchema, FieldType, IndexSchema, ReferenceSchema};

static PARENT: EntitySchema = EntitySchema {
    id: crate::schema::handle("Parent"),
    name: "Parent",
    fields: &[FieldSchema {
        default: None,
        managed: None,
        name: "id",
        value: FieldType::Nat64,
        optional: false,
        reference: None,
    }],
    primary_key: "id",
    indexes: &[],
};
static CHILD: EntitySchema = EntitySchema {
    id: crate::schema::handle("Child"),
    name: "Child",
    fields: &[
        FieldSchema {
            default: None,
            managed: None,
            name: "id",
            value: FieldType::Nat64,
            optional: false,
            reference: None,
        },
        FieldSchema {
            default: None,
            managed: None,
            name: "parent",
            value: FieldType::Nat64,
            optional: true,
            reference: Some(ReferenceSchema {
                entity_name: "Parent",
                entity: crate::schema::handle("Parent"),
                index: crate::schema::handle("parent"),
            }),
        },
    ],
    primary_key: "id",
    indexes: &[IndexSchema {
        id: crate::schema::handle("parent"),
        name: "parent",
        fields: &["parent"],
        unique: false,
        present_only: false,
    }],
};

#[test]
fn order_independent_generation_seals_and_preserves_the_complete_schema() {
    let mut left = Registry::default();
    left.register("host::schema::Child", &CHILD);
    left.register("host::schema::nested::Parent", &PARENT);
    let mut right = Registry::default();
    right.register("host::schema::nested::Parent", &PARENT);
    right.register("host::schema::Child", &CHILD);
    let output = left.generate("host::schema", "crate::schema").unwrap();
    assert_eq!(
        output,
        right.generate("host::schema", "crate::schema").unwrap()
    );
    assert_eq!(
        output,
        left.generate("host::schema", "crate::schema").unwrap()
    );
    assert!(output.contains("<crate::schema::nested::Parent as ::ic_db::Entity>::SCHEMA"));
    assert!(output.contains("<crate::schema::Child as ::ic_db::Entity>::SCHEMA"));
    left.register("host::schema::Late", &PARENT);
    assert!(matches!(
        left.generate("host::schema", "crate::schema"),
        Err(Error::LateRegistration)
    ));
    assert_eq!(left.nodes.len(), 2); // Late declarations cannot mutate the sealed snapshot.
}

#[test]
fn duplicates_and_missing_targets_refuse_before_emission() {
    for reverse in [false, true] {
        let mut registry = Registry::default();
        let mut nodes = [("host::schema::B", &CHILD), ("host::schema::A", &PARENT)];
        if reverse {
            nodes.reverse();
        }
        for (path, schema) in nodes {
            registry.register(path, schema);
            registry.register(path, schema);
        }
        assert!(matches!(
            registry.generate("host::schema", "crate::schema"),
            Err(Error::DuplicatePath("host::schema::A"))
        ));
    }
    let mut registry = Registry::default();
    registry.register("host::schema::Child", &CHILD);
    // A similarly prefixed module does not supply a missing target.
    registry.register("host::schema_extra::Parent", &PARENT);
    assert!(matches!(
        registry.generate("host::schema", "crate::schema"),
        Err(Error::Schema(crate::Error::InvalidSchema))
    ));
    assert!(matches!(
        registry.generate("host::missing", "crate::schema"),
        Err(Error::EmptyModule)
    ));
    registry.register("host::schema::Parent", &PARENT);
    registry.generate("host::schema", "crate::schema").unwrap();
    let mut registry = Registry::default();
    registry.register("host::schema::Parent", &PARENT);
    registry.register("host::schema::DifferentPathSameId", &PARENT);
    assert!(matches!(
        registry.generate("host::schema", "crate::schema"),
        Err(Error::Schema(crate::Error::InvalidSchema))
    ));
}
