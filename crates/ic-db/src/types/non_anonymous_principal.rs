//! Checked identity for fields that require a non-anonymous principal.

use ic_db::design::prelude::*;

/// A principal other than the IC anonymous principal. This value alone grants no authority.
/// ```compile_fail
/// # use ic_db::{NonAnonymousPrincipal, Principal};
/// let invalid = NonAnonymousPrincipal(Principal::anonymous());
/// ```
#[newtype(item(prim = "Principal"), validate(non_anonymous))]
#[derive(Copy, Hash)]
pub struct NonAnonymousPrincipal {}
