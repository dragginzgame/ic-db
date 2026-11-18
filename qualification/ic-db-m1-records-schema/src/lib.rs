#![expect(
    missing_docs,
    reason = "Synthetic schema declarations are private qualification inputs, not a product API."
)]
//! Shared authored schema: compiled once for host collection and again for the target.

use ic_db::design::prelude::{entity, enum_, list, map, newtype, record, set};

pub mod entries {
    use ic_db::design::prelude::entity;
    #[entity( pk(field="id", source="external"),
    index(fields=["category", "id"]),
    fields(field(name="id", value(item(prim="Nat64"))),
        field(name="category", value(item(prim="Nat64"))),
        field(name="amount", value(item(prim="Nat64")))))]
    pub struct Entry {}
}
pub use entries::Entry;

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
pub struct Document {}

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
pub struct WideNumbers {}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 1024))),
        field(name = "optional", value(opt, item(prim = "Blob", max_bytes = 32)))
    )
)]
pub struct BinaryObject {}

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
pub struct NarrowNumbers {}

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
pub struct IdentityRow {}

#[entity( pk(field="id"), index(fields=["next", "id"]),
    fields(field(name="id", value(item(prim="Nat64"))),
        field(name="next", value(opt, item(prim="Nat64")), references(entity="Self",index="next,id"))))]
pub struct OptionalLink {}

#[enum_(
    variant(name = "Ready"),
    variant(name = "Archived"),
    variant(name = "Draft")
)]
pub struct Lifecycle {}
#[entity(
    pk(field = "id"),
    index(fields = ["state", "id"]),
    index(field = "previous", unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "state", value(item(is = "Lifecycle"))),
        field(name = "previous", value(opt, item(is = "Lifecycle")))
    )
)]
pub struct EnumRow {}

#[set(item(is = "Lifecycle"), max_items = 3)]
pub struct LifecycleSet {}
#[map(key(is = "Lifecycle"), value(opt, item(prim = "Nat16")), max_items = 3)]
pub struct LifecycleCounts {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "counts", value(item(is = "LifecycleCounts"))),
        field(name = "states", value(item(is = "LifecycleSet")))
    )
)]
pub struct EnumCollectionsRow {}

#[record(fields(
    field(name = "label", value(item(prim = "Text", max_bytes = 8))),
    field(name = "x", value(item(prim = "Nat16")))
))]
pub struct Details {}
#[record(fields(
    field(name = "left", value(item(indirect, is = "NestedPayload"))),
    field(name = "right", value(item(indirect, is = "NestedPayload")))
))]
pub struct Branch {}
#[enum_(
    variant(name = "Data", value(item(is = "Details"))),
    variant(name = "Idle"),
    variant(name = "Loop", value(item(indirect, is = "Self"))),
    variant(name = "Maybe", value(opt, item(prim = "Nat64"))),
    variant(name = "Pair", value(item(is = "Branch")))
)]
pub struct NestedPayload {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "meta", value(opt, item(is = "Details"))),
        field(name = "payload", value(item(is = "NestedPayload")))
    )
)]
pub struct NestedRow {}

#[record(fields(field(name = "children", value(many, max_items = 4, item(is = "Self")))))]
pub struct ListNode {}

#[newtype(item(prim = "Nat16"))]
pub struct Sample {}

#[list(item(is = "Sample"), max_items = 3)]
pub struct Samples {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "labels",
            value(opt, many, max_items = 3, item(prim = "Text", max_bytes = 4))
        ),
        field(name = "numbers", value(item(is = "Samples"))),
        field(name = "trees", value(many, max_items = 4, item(is = "ListNode")))
    )
)]
pub struct ListRow {}

#[set(item(prim = "Int16"), max_items = 3)]
pub struct Scores {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "scores", value(item(is = "Scores")))
    )
)]
pub struct SetRow {}

#[map(
    key(prim = "Int16"),
    value(opt, item(prim = "Text", max_bytes = 4)),
    max_items = 3
)]
pub struct Rates {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "values", value(item(is = "Rates")))
    )
)]
pub struct MapRow {}

#[map(key(prim = "Nat16"), value(item(is = "Self")), max_items = 256)]
pub struct MapTree {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "tree", value(item(is = "MapTree")))
    )
)]
pub struct MapTreeRow {}

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
pub struct BigNumbers {}

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
pub struct FloatRow {}

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
pub struct AccountRow {}

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
pub struct UlidRow {}

#[entity(
    pk(field = "id"),
    index(fields = ["zone", "value", "id"]),
    index(field = "maybe", unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "maybe", value(opt, item(prim = "Date"))),
        field(name = "value", value(item(prim = "Date"))),
        field(name = "zone", value(item(prim = "Bool")))
    )
)]
pub struct DateRow {}

#[entity(
    pk(field="id"),
    index(fields=["zone","duration","id"]),
    index(fields=["zone","timestamp","id"]),
    index(field="delay",unique,present_only),
    index(field="maybe",unique,present_only),
    fields(
        field(name="delay",value(opt,item(prim="Duration"))),
        field(name="duration",value(item(prim="Duration"))),
        field(name="id",value(item(prim="Nat64"))),
        field(name="maybe",value(opt,item(prim="Timestamp"))),
        field(name="timestamp",value(item(prim="Timestamp"))),
        field(name="zone",value(item(prim="Bool")))
    )
)]
pub struct TimeRow {}

#[entity(pk(field="id"),index(fields=["zone","value","id"]),index(field="maybe",unique,present_only),fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="maybe",value(opt,item(prim="Decimal",scale=2))),
    field(name="value",value(item(prim="Decimal",scale=2))),
    field(name="zone",value(item(prim="Bool")))
))]
pub struct DecimalRow {}

#[entity(pk(field="id"),index(field="value",unique),index(fields=["owner","id"]),fields(
    field(name="id",value(item(prim="Ulid"))),
    field(name="owner",value(opt,item(prim="Nat64")),references(entity="Numbered")),
    field(name="value",value(item(prim="Nat64")))
))]
pub struct KeyRow {}
#[entity(pk(field="id"),index(fields=["target","id"]),fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="target",value(opt,item(prim="Ulid")),references(entity="KeyRow"))
))]
pub struct Numbered {}

#[entity(pk(field="id"),index(field="rank",unique),index(fields=["peer","id"]),fields(
    field(name="id",value(item(prim="Int128"))),
    field(name="peer",value(opt,item(prim="U256")),references(entity="WideKey")),
    field(name="rank",value(item(prim="Nat64")))
))]
pub struct SignedKey {}
#[entity(pk(field="id"),index(field="rank",unique),index(fields=["peer","id"]),fields(
    field(name="id",value(item(prim="U256"))),
    field(name="peer",value(opt,item(prim="Int128")),references(entity="SignedKey")),
    field(name="rank",value(item(prim="Nat64")))
))]
pub struct WideKey {}

#[entity(
    pk(field = "id"),
    index(field = "label", unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "count", value(item(prim = "Nat16")), default = 7),
        field(name = "enabled", value(item(prim = "Bool")), default = true),
        field(
            name = "label",
            value(opt, item(prim = "Text", max_bytes = 8)),
            default = "draft"
        ),
        field(
            name = "empty",
            value(item(prim = "Blob", max_bytes = 0)),
            default = b""
        ),
        field(
            name = "payload",
            value(opt, item(prim = "Blob", max_bytes = 2)),
            default = b"\x00\xff"
        )
    )
)]
pub struct DefaultRow {}

#[entity(
    pk(field = "id"),
    index(field = "body", unique),
    index(field = "updated"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "body",
            value(item(prim = "Text", max_bytes = 32)),
            default = "ready"
        ),
        field(
            name = "created",
            value(item(prim = "Timestamp")),
            managed = "created_at"
        ),
        field(
            name = "updated",
            value(item(prim = "Timestamp")),
            managed = "updated_at"
        )
    )
)]
pub struct ManagedRow {}

#[entity(pk(field="id"),index(field="rank",unique),index(fields=["next","id"]),fields(
    field(name="id",value(item(prim="Principal"))),
    field(name="next",value(opt,item(prim="Principal")),references(entity="Self")),
    field(name="rank",value(item(prim="Nat64")))
))]
pub struct PrincipalKey {}

#[entity(pk(field="id"),index(field="rank",unique),index(fields=["peer","id"]),fields(
    field(name="id",value(item(prim="Account"))),
    field(name="peer",value(opt,item(prim="Subaccount")),references(entity="SubaccountKey")),
    field(name="rank",value(item(prim="Nat64")))
))]
pub struct AccountKey {}

#[entity(pk(field="id"),index(field="rank",unique),index(fields=["peer","id"]),fields(
    field(name="id",value(item(prim="Subaccount"))),
    field(name="peer",value(opt,item(prim="Account")),references(entity="AccountKey")),
    field(name="rank",value(item(prim="Nat64")))
))]
pub struct SubaccountKey {}

// Synthetic maximum row/batch/index-fan-out witness, not an application schema.
#[entity(pk(field="id"),
    index(fields=["a","b","c","d"]),index(fields=["a","b","d","c"]),
    index(fields=["a","c","b","d"]),index(fields=["a","c","d","b"]),
    index(fields=["a","d","b","c"]),index(fields=["a","d","c","b"]),
    index(fields=["b","a","c","d"]),index(fields=["b","a","d","c"]),
    fields(
        field(name="id",value(item(prim="Nat64"))),
        field(name="a",value(item(prim="Text",max_bytes=1024))),
        field(name="b",value(item(prim="Text",max_bytes=1024))),
        field(name="c",value(item(prim="Text",max_bytes=1024))),
        field(name="d",value(item(prim="Text",max_bytes=1024))),
        field(name="padding",value(item(prim="Blob",max_bytes=4096)))
))]
pub struct EnvelopeRow {}

/// Collect this crate’s declarations and emit their target schema assembly.
/// # Errors
/// Refuses an invalid schema graph or target path.
#[cfg(feature = "build")]
pub fn write_schema(target: &str) -> Result<(), Box<dyn std::error::Error>> {
    ic_db::build::write_schema(module_path!(), target)
}
