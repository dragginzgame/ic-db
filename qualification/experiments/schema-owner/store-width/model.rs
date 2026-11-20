//! Single-store schema for the width experiment.

use ic_db::design::prelude::*;

/// The game canister.
#[canister]
pub struct Game {}

/// All rules share this allocation.
#[store(canister = Game, key = "probe.game.v1")]
pub struct Rules {}

/// Referenced rule at the start of the roster.
#[entity(
    store = Rules,
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
pub struct Ability {}

/// Dependent rule at the end of the roster.
#[entity(
    store = Rules,
    pk(field = "id"),
    index(field = "ability"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "ability", value(item(prim = "Nat64")), references(entity = "Ability"))
    )
)]
pub struct Spell {}

// The probe appends one invocation per independent declaration between Ability and Spell.
macro_rules! rule {
    ($name:ident) => {
        /// Independent game rule.
        #[entity(
                    store = Rules,
                    pk(field = "id"),
                    fields(field(name = "id", value(item(prim = "Nat64"))))
                )]
        pub struct $name {}
    };
}
