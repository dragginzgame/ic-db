//! Shared authored schema: compiled once for host collection and again for the target.

use ic_db::entity;

pub(crate) mod entries {
    use ic_db::entity;
    #[entity( pk(field="id", source="external"),
    index(fields=["category", "id"]),
    fields(field(name="id", value(item(prim="Nat64"))),
        field(name="category", value(item(prim="Nat64"))),
        field(name="amount", value(item(prim="Nat64")))))]
    pub(crate) struct Entry {}
}
pub(crate) use entries::Entry;

#[entity(
    pk(field = "id"),
    index(fields = ["entry_id", "score"]),
    index(fields = ["published", "title"]),
    index(fields = ["entry_id", "priority"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "entry_id", value(item(prim = "Nat64")), references(entity = "Entry", index="entry_id,score")),
        field(name = "title", value(item(prim = "Text", max_bytes = 32))),
        field(name = "note", value(opt, item(prim = "Text", max_bytes = 64))),
        field(name = "published", value(item(prim = "Bool"))),
        field(name = "score", value(item(prim = "Int64"))),
        field(name = "previous_score", value(opt, item(prim = "Int64"))),
        field(name = "priority", value(opt, item(prim = "Nat64")))
    )
)]
pub(crate) struct Document {}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "signed", value(item(prim = "Int128"))),
        field(name = "unsigned", value(item(prim = "Nat128"))),
        field(name = "optional_signed", value(opt, item(prim = "Int128"))),
        field(name = "optional_unsigned", value(opt, item(prim = "Nat128")))
    )
)]
pub(crate) struct WideNumbers {}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 1024))),
        field(name = "optional", value(opt, item(prim = "Blob", max_bytes = 32)))
    )
)]
pub(crate) struct BinaryObject {}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "signed8", value(item(prim = "Int8"))),
        field(name = "signed16", value(item(prim = "Int16"))),
        field(name = "signed32", value(item(prim = "Int32"))),
        field(name = "unsigned8", value(item(prim = "Nat8"))),
        field(name = "unsigned16", value(item(prim = "Nat16"))),
        field(name = "unsigned32", value(item(prim = "Nat32"))),
        field(name = "optional_signed8", value(opt, item(prim = "Int8"))),
        field(name = "optional_signed16", value(opt, item(prim = "Int16"))),
        field(name = "optional_signed32", value(opt, item(prim = "Int32"))),
        field(name = "optional_unsigned8", value(opt, item(prim = "Nat8"))),
        field(name = "optional_unsigned16", value(opt, item(prim = "Nat16"))),
        field(name = "optional_unsigned32", value(opt, item(prim = "Nat32"))),
    )
)]
pub(crate) struct NarrowNumbers {}

#[entity(
    pk(field = "id"),
    index(field = "owner", unique),
    index(field = "optional", unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "owner", value(item(prim = "Principal"))),
        field(name = "optional", value(opt, item(prim = "Principal")))
    )
)]
pub(crate) struct IdentityRow {}

#[entity( pk(field="id"), index(fields=["next", "id"]),
    fields(field(name="id", value(item(prim="Nat64"))),
        field(name="next", value(opt, item(prim="Nat64")), references(entity="Self",index="next,id"))))]
pub(crate) struct OptionalLink {}
