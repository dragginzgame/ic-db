//! Shared declarations used by the host build and target.
use ic_db::entity;

pub(crate) mod destination {
    use super::*;
    #[entity(
        pk(field = "id"),
        fields(field(name = "id", value(item(prim = "Nat64"))))
    )]
    pub struct Target {}
}
pub(crate) mod source {
    use super::*;
    use crate::schema::destination::Target as ImportedTarget;
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
pub(crate) mod cycle {
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
