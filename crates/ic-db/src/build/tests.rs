use super::{Error, Registry, StoreDeclaration};
use crate::{EntitySchema, FieldSchema, FieldType, IndexSchema, ReferenceSchema};

const STORE: &str = "host::schema::Data";
fn registry_fixture() -> Registry {
    Registry {
        stores: vec![StoreDeclaration {
            path: STORE,
            canister: "host::schema::App",
            key: "test.data.v1",
        }],
        canisters: vec!["host::schema::App"],
        ..Registry::default()
    }
}

static PARENT: EntitySchema = EntitySchema {
    max_row_bytes: crate::codec::DEFAULT_ROW_BYTES,
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
    max_row_bytes: crate::codec::DEFAULT_ROW_BYTES,
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
    let mut left = registry_fixture();
    left.register("host::schema::Child", STORE, &CHILD);
    left.register("host::schema::nested::Parent", STORE, &PARENT);
    let mut right = registry_fixture();
    right.register("host::schema::nested::Parent", STORE, &PARENT);
    right.register("host::schema::Child", STORE, &CHILD);
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
    left.register("host::schema::Late", STORE, &PARENT);
    assert!(matches!(
        left.generate("host::schema", "crate::schema"),
        Err(Error::LateRegistration)
    ));
    assert_eq!(left.nodes.len(), 2); // Late declarations cannot mutate the sealed snapshot.
}

#[test]
fn duplicates_and_missing_targets_refuse_before_emission() {
    for reverse in [false, true] {
        let mut registry = registry_fixture();
        let mut nodes = [("host::schema::B", &CHILD), ("host::schema::A", &PARENT)];
        if reverse {
            nodes.reverse();
        }
        for (path, schema) in nodes {
            registry.register(path, STORE, schema);
            registry.register(path, STORE, schema);
        }
        assert!(matches!(
            registry.generate("host::schema", "crate::schema"),
            Err(Error::DuplicatePath("host::schema::A"))
        ));
    }
    let mut registry = registry_fixture();
    registry.register("host::schema::Child", STORE, &CHILD);
    // A similarly prefixed module does not supply a missing target.
    registry.register("host::schema_extra::Parent", STORE, &PARENT);
    let error = registry
        .generate("host::schema", "crate::schema")
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Child.parent references Parent, which is missing from the selected schema module host::schema"
    );
    assert!(matches!(
        registry.generate("host::missing", "crate::schema"),
        Err(Error::EmptyModule)
    ));
    registry.register("host::schema::Parent", STORE, &PARENT);
    registry.generate("host::schema", "crate::schema").unwrap();
    let mut registry = registry_fixture();
    registry.register("host::schema::Parent", STORE, &PARENT);
    registry.register("host::schema::DifferentPathSameId", STORE, &PARENT);
    assert!(matches!(
        registry.generate("host::schema", "crate::schema"),
        Err(Error::Ownership(message)) if message.contains("both declare entity Parent")
    ));
}

#[test]
fn mismatched_reference_identifies_both_fields_and_key_types() {
    static ULID_PARENT: EntitySchema = EntitySchema {
        fields: &[FieldSchema {
            value: FieldType::Ulid,
            ..PARENT.fields[0]
        }],
        ..PARENT
    };
    let mut registry = registry_fixture();
    registry.register("host::schema::Child", STORE, &CHILD);
    registry.register("host::schema::Parent", STORE, &ULID_PARENT);
    let error = registry
        .generate("host::schema", "crate::schema")
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Child.parent has type Nat64 but references Parent.id, which requires Ulid"
    );
    assert!(matches!(
        crate::Schema::new(&[&CHILD, &ULID_PARENT]),
        Err(crate::Error::InvalidSchema)
    ));
}
