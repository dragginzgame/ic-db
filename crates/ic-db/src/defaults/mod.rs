//! Literal insert policy metadata, encoded through the owned scalar codec.

#[cfg(test)]
mod tests;

use crate::{Error, FieldType, codec::Encoder};

/// A compiled literal used only when an insert input omits its field.
/// These are schema constants, not a dynamic query or mutation value model.
#[derive(Debug, PartialEq, Eq)]
pub enum LiteralDefault {
    /// Exact unsigned integer literal; must fit the declared unsigned width.
    Nat(u128),
    /// Exact signed integer literal; must fit the declared signed width.
    Int(i128),
    /// Boolean literal.
    Bool(bool),
    /// UTF-8 text within the declared byte limit.
    Text(&'static str),
    /// Raw bytes within the declared Blob byte limit.
    Blob(&'static [u8]),
}

impl LiteralDefault {
    pub(crate) fn encode(&self, value: FieldType) -> Result<Vec<u8>, Error> {
        let mut writer = Encoder::new();
        let invalid = |_| Error::InvalidSchema;
        match (self, value) {
            (Self::Nat(n), FieldType::Nat8) => writer.nat8((*n).try_into().map_err(invalid)?)?,
            (Self::Nat(n), FieldType::Nat16) => writer.nat16((*n).try_into().map_err(invalid)?)?,
            (Self::Nat(n), FieldType::Nat32) => writer.nat32((*n).try_into().map_err(invalid)?)?,
            (Self::Nat(n), FieldType::Nat64) => writer.nat64((*n).try_into().map_err(invalid)?)?,
            (Self::Nat(n), FieldType::Nat128) => writer.nat128(*n)?,
            (Self::Int(n), FieldType::Int8) => writer.int8((*n).try_into().map_err(invalid)?)?,
            (Self::Int(n), FieldType::Int16) => writer.int16((*n).try_into().map_err(invalid)?)?,
            (Self::Int(n), FieldType::Int32) => writer.int32((*n).try_into().map_err(invalid)?)?,
            (Self::Int(n), FieldType::Int64) => writer.int64((*n).try_into().map_err(invalid)?)?,
            (Self::Int(n), FieldType::Int128) => writer.int128(*n)?,
            (Self::Bool(b), FieldType::Bool) => writer.boolean(*b)?,
            (Self::Text(s), FieldType::Text { max_bytes }) if s.len() <= max_bytes as usize => {
                writer
                    .bytes(s.as_bytes())
                    .map_err(|_| Error::InvalidSchema)?;
            }
            (Self::Blob(bytes), FieldType::Blob { max_bytes })
                if bytes.len() <= max_bytes as usize =>
            {
                writer.bytes(bytes).map_err(|_| Error::InvalidSchema)?;
            }
            _ => return Err(Error::InvalidSchema),
        }

        Ok(writer.finish())
    }
}

/// Typed insert construction with omission distinct from explicit optional absence.
/// Generated inputs implement this once per entity; execution stays in `Batch`.
/// Managed fields have no input setters:
/// ```compile_fail
/// use ic_db::design::prelude::*;
/// #[entity(store = ic_db::testing::FixtureStore, pk(field="id"), fields(
///     field(name="id",value(item(prim="Nat64"))),
///     field(name="created",value(item(prim="Timestamp")),managed="created_at")
/// ))]
/// struct Entry {}
/// let input = Entry::insert().id(1).created(ic_db::Timestamp::from_millis(0));
/// ```
pub trait InsertInput: Sized {
    /// The fully materialized entity admitted through the ordinary write path.
    type Entity: crate::Entity;
    /// Resolve omitted fields without reading or mutating persistent state.
    /// # Errors
    /// Refuses a missing required field or managed time without a batch context.
    /// Field/row bounds are checked by batch insertion.
    fn into_entity(self) -> Result<Self::Entity, Error> {
        self.__into_entity(None)
    }

    /// Materialize generated input using the batch's captured time.
    #[doc(hidden)]
    fn __into_entity(self, time: Option<crate::Timestamp>) -> Result<Self::Entity, Error>;
}
