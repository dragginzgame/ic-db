//! Reusable named values for application schemas.
//!
//! Import a value into the declaring module and refer to it with `item(is = "...")`.
//! Each store's compiled schema includes the definitions reachable from its entities.

mod non_anonymous_principal;

pub use non_anonymous_principal::NonAnonymousPrincipal;
