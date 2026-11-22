//! Checked storage shared by named Principal declarations.

use crate::{Error, Principal};

#[doc(hidden)]
#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "Principal", into = "Principal")]
pub struct CheckedPrincipal(Principal);

impl TryFrom<Principal> for CheckedPrincipal {
    type Error = Error;

    fn try_from(value: Principal) -> Result<Self, Error> {
        if value == Principal::anonymous() {
            return Err(Error::InvalidOperand);
        }
        Ok(Self(value))
    }
}

impl From<CheckedPrincipal> for Principal {
    fn from(value: CheckedPrincipal) -> Self {
        value.0
    }
}
