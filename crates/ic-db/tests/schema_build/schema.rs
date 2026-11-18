//! Shared declarations used by the host build and target.
use ic_db::entity;

pub(super) mod destination {
    use super::*;
    #[entity(
        pk(field = "id"),
        fields(field(name = "id", value(item(prim = "Nat64"))))
    )]
    pub struct Target {}
}
pub(super) mod source {
    use super::*;
    use super::destination::Target as ImportedTarget;
    #[entity(
        name = "SourceLink",
        pk(field = "id"),
        index(field = "target"),
        fields(
            field(name = "id", value(item(prim = "Nat64"))),
            field(
                name = "target",
                value(opt, item(prim = "Nat64")),
                references(entity = "ImportedTarget")
            )
        )
    )]
    pub struct Link {}
}
pub(super) mod cycle {
    use super::*;
    #[entity(
        name = "CycleLink",
        pk(field = "id"),
        index(field = "next"),
        fields(
            field(name = "id", value(item(prim = "Nat64"))),
            field(
                name = "next",
                value(opt, item(prim = "Nat64")),
                references(entity = "Self")
            )
        )
    )]
    pub struct Link {}
}
