//! Public JSON projection of the local schema inspection reply.

use ic_db_cli::{EntityView, FieldView, IndexView, TypeView, lowercase_hex};
use serde::Serialize;

///
/// Document
/// Top-level output of schema --json.
///
#[derive(Serialize)]
pub(super) struct Document<'a> {
    source: &'static str,
    schema_fingerprint: String,
    entities: Vec<Entity<'a>>,
    types: &'a [TypeView],
}

impl<'a> Document<'a> {
    pub(super) fn new(
        fingerprint: &[u8; 32],
        entities: &'a [EntityView],
        types: &'a [TypeView],
    ) -> Self {
        Self {
            source: "local_fixture",
            schema_fingerprint: lowercase_hex(fingerprint, ""),
            entities: entities.iter().map(Entity::from).collect(),
            types,
        }
    }
}

///
/// Entity
/// Selected entity and its public field and index descriptions.
///
#[derive(Serialize)]
struct Entity<'a> {
    name: &'a str,
    primary_key: &'a str,
    fields: Vec<Field<'a>>,
    indexes: &'a [IndexView],
}

impl<'a> From<&'a EntityView> for Entity<'a> {
    fn from(view: &'a EntityView) -> Self {
        Self {
            name: &view.name,
            primary_key: &view.primary,
            fields: view.fields.iter().map(Field::from).collect(),
            indexes: &view.indexes,
        }
    }
}

///
/// Field
/// Field metadata with the type and reference names exposed by the CLI.
///
#[derive(Serialize)]
struct Field<'a> {
    name: &'a str,
    r#type: &'a str,
    optional: bool,
    info: &'a str,
    reference: Option<&'a ic_db_cli::ReferenceView>,
}

impl<'a> From<&'a FieldView> for Field<'a> {
    fn from(view: &'a FieldView) -> Self {
        Self {
            name: &view.name,
            r#type: &view.kind,
            optional: view.optional,
            info: &view.info,
            reference: view.reference.as_ref(),
        }
    }
}
