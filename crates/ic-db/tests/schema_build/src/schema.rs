//! Shared declarations used by the host build and target.

use ic_db::entity;

pub(crate) mod destination {
    use super::*;

    ///
    /// Target
    /// Provides the destination for cross-module references.
    ///

    #[entity(
        pk(field = "id"),
        fields(field(name = "id", value(item(prim = "Nat64"))))
    )]
    pub struct Target {}
}
pub(crate) mod source {
    use super::*;
    use crate::schema::destination::Target as ImportedTarget;

    ///
    /// Link
    /// Refers to a destination entity through a Rust import alias.
    ///

    #[entity(
        name = "SourceLink",
        pk(field = "id"),
        index(field = "target"),
        fields(
            field(name = "id", value(item(prim = "Nat64"))),
            field(name = "target", value(opt, item(prim = "Nat64")), references(entity = "ImportedTarget"))
        )
    )]
    pub struct Link {}
}
pub(crate) mod cycle {
    use super::*;

    ///
    /// Link
    /// Models an optional self-reference resolved during schema assembly.
    ///

    #[entity(
        name = "CycleLink",
        pk(field = "id"),
        index(field = "next"),
        fields(
            field(name = "id", value(item(prim = "Nat64"))),
            field(name = "next", value(opt, item(prim = "Nat64")), references(entity = "Self"))
        )
    )]
    pub struct Link {}
}
