//! Exact integer accumulation and explicitly selected result width. No coercion engine.
use super::{Accumulator, Aggregate, Query, admit_output, decode_record, execute};
use crate::{
    Entity, EntitySchema, Error, FieldType, IndexValue, TypeBody, TypeSchema, U256, Value,
    ValueSchema, codec::Encoder,
};
use ethnum::I256;
use ic_storage::Memory;

impl<E: Entity, M: Memory> Query<'_, E, M> {
    /// Sum one integer field into a single-field result record with the same field
    /// name and optionality. Its integer width may match or widen the source within
    /// the same signed/unsigned family. Unsigned results may widen to U256.
    /// Empty/all-absent input returns `None`; present zero returns a record.
    /// Signed accumulation uses I256, so cancelling prefixes do not cause narrow
    /// overflow. Unsigned accumulation uses checked U256. The final total must fit
    /// the declared result width. No decimal/float conversion or rounding occurs.
    /// # Errors
    /// Refuses unsupported/narrower/cross-signedness result shapes, cursors, DISTINCT,
    /// input ordering and group modifiers. Work exhaustion returns `Limit` without
    /// a prefix total; numeric overflow returns `ArithmeticOverflow`. Output bytes
    /// count the completed result record, with `RowTooLarge` on overflow of that budget.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn sum<R: Value>(self) -> Result<Aggregate<Option<R>>, Error> {
        let mut plan = self.aggregate_plan(None, false)?;
        let mut sum = IntegerSum::admit(E::SCHEMA, R::definition())?;
        plan.selected |= 1 << sum.position;
        let page = execute(
            &self.db.map,
            E::SCHEMA,
            &self.request,
            &plan,
            Some(Accumulator::Sum(&mut sum)),
        )?;
        let mut output_bytes = 0;
        let value = sum
            .finish()?
            .map(|bytes| {
                admit_output(bytes.len(), self.request.bytes, &mut output_bytes)?;
                decode_record(&bytes)
            })
            .transpose()?;
        Ok(Aggregate {
            value,
            examined: page.examined,
            fetched: page.fetched,
            output_bytes,
        })
    }
}

pub(in crate::store::query) struct IntegerSum {
    position: usize,
    output: ValueSchema,
    total: Total,
    present: bool,
}
enum Total {
    Signed(I256),
    Unsigned(U256),
}
impl IntegerSum {
    fn admit(entity: &EntitySchema, output: &TypeSchema) -> Result<Self, Error> {
        let TypeBody::Record([result]) = output.body else {
            return Err(Error::InvalidQuery);
        };
        let (position, source) = entity
            .fields
            .iter()
            .enumerate()
            .find(|(_, field)| field.name == result.name)
            .ok_or(Error::InvalidQuery)?;
        let (signed, bits) = integer(source.value).ok_or(Error::InvalidQuery)?;
        let (result_signed, result_bits) = integer(result.value).ok_or(Error::InvalidQuery)?;
        if source.optional != result.optional || signed != result_signed || bits > result_bits {
            return Err(Error::InvalidQuery);
        }
        Ok(Self {
            position,
            output: ValueSchema {
                value: result.value,
                optional: result.optional,
            },
            total: if signed {
                Total::Signed(I256::ZERO)
            } else {
                Total::Unsigned(U256::ZERO)
            },
            present: false,
        })
    }

    pub(super) fn push(&mut self, values: &[IndexValue<'_>]) -> Result<(), Error> {
        let value = values[self.position];
        if value == IndexValue::Absent {
            return Ok(());
        }
        match &mut self.total {
            Total::Signed(total) => {
                let value = match value {
                    IndexValue::Int8(n) => i128::from(n),
                    IndexValue::Int16(n) => i128::from(n),
                    IndexValue::Int32(n) => i128::from(n),
                    IndexValue::Int64(n) => i128::from(n),
                    IndexValue::Int128(n) => n,
                    _ => return Err(Error::Corrupt),
                };
                // At most 512 inputs of at most 128 signed bits fit in I256.
                *total = total
                    .checked_add(I256::from(value))
                    .ok_or(Error::ArithmeticOverflow)?;
            }
            Total::Unsigned(total) => {
                let value = match value {
                    IndexValue::Nat8(n) => U256::from(n),
                    IndexValue::Nat16(n) => U256::from(n),
                    IndexValue::Nat32(n) => U256::from(n),
                    IndexValue::Nat64(n) => U256::from(n),
                    IndexValue::Nat128(n) => U256::from(n),
                    IndexValue::U256(n) => n,
                    _ => return Err(Error::Corrupt),
                };
                *total = total.checked_add(value).ok_or(Error::ArithmeticOverflow)?;
            }
        }
        self.present = true;
        Ok(())
    }

    fn finish(self) -> Result<Option<Vec<u8>>, Error> {
        if !self.present {
            return Ok(None);
        }
        let mut output = Encoder::new();
        if self.output.optional {
            output.boolean(true)?;
        }
        // The ordinary owned codec supplies result bytes; checked conversion occurs
        // only after complete accumulation, never once per input or work chunk.
        macro_rules! encode {
            ($method:ident, $value:expr) => {
                output.$method($value.try_into().map_err(|_| Error::ArithmeticOverflow)?)?
            };
        }
        match (self.total, self.output.value) {
            (Total::Signed(n), FieldType::Int8) => encode!(int8, n),
            (Total::Signed(n), FieldType::Int16) => encode!(int16, n),
            (Total::Signed(n), FieldType::Int32) => encode!(int32, n),
            (Total::Signed(n), FieldType::Int64) => encode!(int64, n),
            (Total::Signed(n), FieldType::Int128) => encode!(int128, n),
            (Total::Unsigned(n), FieldType::Nat8) => encode!(nat8, n),
            (Total::Unsigned(n), FieldType::Nat16) => encode!(nat16, n),
            (Total::Unsigned(n), FieldType::Nat32) => encode!(nat32, n),
            (Total::Unsigned(n), FieldType::Nat64) => encode!(nat64, n),
            (Total::Unsigned(n), FieldType::Nat128) => encode!(nat128, n),
            (Total::Unsigned(n), FieldType::U256) => output.u256(n)?,
            _ => return Err(Error::Corrupt),
        }
        Ok(Some(output.finish()))
    }
}

fn integer(value: FieldType) -> Option<(bool, u16)> {
    Some(match value {
        FieldType::Nat8 => (false, 8),
        FieldType::Int8 => (true, 8),
        FieldType::Nat16 => (false, 16),
        FieldType::Int16 => (true, 16),
        FieldType::Nat32 => (false, 32),
        FieldType::Int32 => (true, 32),
        FieldType::Nat64 => (false, 64),
        FieldType::Int64 => (true, 64),
        FieldType::Nat128 => (false, 128),
        FieldType::Int128 => (true, 128),
        FieldType::U256 => (false, 256),
        _ => return None,
    })
}
