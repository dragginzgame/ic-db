//! Private storage for generated text newtypes. Shared rule evaluation works on borrowed text.
use crate::{Error, NewtypeRule};

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, serde::Serialize)]
#[serde(transparent)]
pub struct CheckedText<const BYTES: u32, const NON_BLANK: bool, const CHARS: u32>(String);

impl<const B: u32, const N: bool, const C: u32> CheckedText<B, N, C> {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl<const B: u32, const N: bool, const C: u32> TryFrom<String> for CheckedText<B, N, C> {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Error> {
        NewtypeRule::Text {
            max_bytes: B,
            non_blank: N,
            max_chars: Some(C),
        }
        .check_text(&value)?;
        Ok(Self(value))
    }
}

impl<'de, const B: u32, const N: bool, const C: u32> serde::Deserialize<'de>
    for CheckedText<B, N, C>
{
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}
