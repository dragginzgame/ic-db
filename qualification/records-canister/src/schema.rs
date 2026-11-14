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

#[ic_db::enum_(
    variant(name = "Ready"),
    variant(name = "Archived"),
    variant(name = "Draft")
)]
pub(crate) struct Lifecycle {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "state", value(item(is = "Lifecycle"))),
        field(name = "previous", value(opt, item(is = "Lifecycle")))
    )
)]
pub(crate) struct EnumRow {}

#[ic_db::record(fields(
    field(name = "label", value(item(prim = "Text", max_bytes = 8))),
    field(name = "x", value(item(prim = "Nat16")))
))]
pub(crate) struct Details {}
#[ic_db::record(fields(
    field(name = "left", value(item(indirect, is = "NestedPayload"))),
    field(name = "right", value(item(indirect, is = "NestedPayload")))
))]
pub(crate) struct Branch {}
#[ic_db::enum_(
    variant(name = "Data", value(item(is = "Details"))),
    variant(name = "Idle"),
    variant(name = "Loop", value(item(indirect, is = "Self"))),
    variant(name = "Maybe", value(opt, item(prim = "Nat64"))),
    variant(name = "Pair", value(item(is = "Branch")))
)]
pub(crate) struct NestedPayload {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "meta", value(opt, item(is = "Details"))),
        field(name = "payload", value(item(is = "NestedPayload")))
    )
)]
pub(crate) struct NestedRow {}

#[ic_db::record(fields(field(name = "children", value(many, max_items = 4, item(is = "Self")))))]
pub(crate) struct ListNode {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "labels",
            value(opt, many, max_items = 3, item(prim = "Text", max_bytes = 4))
        ),
        field(name = "numbers", value(many, max_items = 3, item(prim = "Nat16"))),
        field(name = "trees", value(many, max_items = 4, item(is = "ListNode")))
    )
)]
pub(crate) struct ListRow {}

#[ic_db::set(item(prim = "Int16"), max_items = 3)]
pub(crate) struct Scores {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "scores", value(item(is = "Scores")))
    )
)]
pub(crate) struct SetRow {}

#[ic_db::map(
    key(prim = "Int16"),
    value(opt, item(prim = "Text", max_bytes = 4)),
    max_items = 3
)]
pub(crate) struct Rates {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "values", value(item(is = "Rates")))
    )
)]
pub(crate) struct MapRow {}

#[ic_db::map(key(prim = "Nat16"), value(item(is = "Self")), max_items = 256)]
pub(crate) struct MapTree {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "tree", value(item(is = "MapTree")))
    )
)]
pub(crate) struct MapTreeRow {}

#[entity(
    pk(field = "id"),
    index(fields = ["zone", "value", "id"]),
    index(field = "maybe", unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "maybe", value(opt, item(prim = "U256"))),
        field(name = "value", value(item(prim = "U256"))),
        field(name = "zone", value(item(prim = "Bool")))
    )
)]
pub(crate) struct BigNumbers {}

#[entity(
    pk(field="id"),
    index(fields=["zone","small","id"]),
    index(fields=["zone","value","id"]),
    index(field="maybe",unique,present_only),
    fields(
        field(name="id",value(item(prim="Nat64"))),
        field(name="maybe",value(opt,item(prim="Float64"))),
        field(name="small",value(item(prim="Float32"))),
        field(name="value",value(item(prim="Float64"))),
        field(name="zone",value(item(prim="Bool")))
    )
)]
pub(crate) struct FloatRow {}

#[entity(
    pk(field="id"),
    index(fields=["zone","value","id"]),
    index(fields=["zone","sub","id"]),
    index(field="maybe",unique,present_only),
    fields(
        field(name="id",value(item(prim="Nat64"))),
        field(name="maybe",value(opt,item(prim="Account"))),
        field(name="sub",value(item(prim="Subaccount"))),
        field(name="value",value(item(prim="Account"))),
        field(name="zone",value(item(prim="Bool")))
    )
)]
pub(crate) struct AccountRow {}

#[entity(
    pk(field = "id"),
    index(fields = ["zone", "value", "id"]),
    index(field = "maybe", unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "maybe", value(opt, item(prim = "Ulid"))),
        field(name = "value", value(item(prim = "Ulid"))),
        field(name = "zone", value(item(prim = "Bool")))
    )
)]
pub(crate) struct UlidRow {}
