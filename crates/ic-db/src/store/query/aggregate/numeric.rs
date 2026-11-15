//! Integer/decimal sums and averages over one exact total, without numeric coercion.
use super::super::Plan;
use super::{Aggregate, Completed, GroupValue, Query, Reducer};
use crate::{
    Decimal, Entity, EntitySchema, Error, FieldType, IndexValue, TypeBody, TypeSchema, U256, Value,
    ValueSchema, codec::Encoder,
};
use ethnum::I256;
use ic_storage::Memory;

impl<E: Entity, M: Memory> Query<'_, E, M> {
    /// Sum one integer or Decimal field into a single-field result record with the same field
    /// name and optionality. Its integer width may match or widen the source within
    /// the same signed/unsigned family. Unsigned results may widen to U256.
    /// Decimal results preserve the source's declared scale; coefficients accumulate
    /// exactly in I256 and must fit i128 at completion. No rescaling or rounding.
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
        let (plan, reducer) = self.numeric_plan::<R>(None, Operation::Sum)?;
        self.scalar_value(&plan, reducer)
    }
    /// Sum one integer or Decimal operand per exact grouping record, retaining absent keys
    /// and all-absent groups. R selects the same explicit result widening as sum.
    /// Group predicates use R's result type; e.g. Nat128 totals require u128 operands.
    /// Complete every group before filtering, ordering and output limits.
    /// # Errors
    /// Scalar sum shape/range errors and grouped query work/state/output bounds apply.
    /// Overflow in any group refuses the whole query, even if that group would be
    /// filtered out or fall beyond the output limit. Finalization is bounded to
    /// 32 groups and charged to the retained-state byte cap.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn group_sum<K: Value, R: Value>(self) -> Result<Aggregate<Vec<GroupValue<K, R>>>, Error> {
        let (plan, reducer) = self.numeric_plan::<R>(Some(K::definition()), Operation::Sum)?;
        self.group_values(&plan, reducer)
    }

    /// Average an integer or Decimal field into a single-field Decimal record.
    /// Preserve the field name and optionality; the result declares scale 0–28.
    /// Ignore absent operands, returning `None` for empty/all-absent input.
    /// Divide the exact total by the present count once, rounding to nearest with
    /// ties to even at the result scale. No floating-point conversion or scale fallback.
    /// # Errors
    /// Refuses nonnumeric inputs, non-Decimal results, changed optionality, cursors,
    /// DISTINCT, input ordering and group modifiers. Work exhaustion returns `Limit`;
    /// an unrepresentable rounded coefficient returns `ArithmeticOverflow`.
    /// Output-byte bounds match [`Self::sum`]. No partial average is returned.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn avg<R: Value>(self) -> Result<Aggregate<Option<R>>, Error> {
        let (plan, reducer) = self.numeric_plan::<R>(None, Operation::Average)?;
        self.scalar_value(&plan, reducer)
    }

    /// Average per group using [`Self::avg`]'s Decimal scale and rounding contract.
    /// Finalize all groups before filtering, ordering and output limits; these
    /// operations compare the rounded Decimal results. Retain all-absent groups.
    /// # Errors
    /// Scalar average admission/range errors and grouped work/state/output bounds
    /// apply. Overflow in an excluded group still refuses the whole query.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn group_avg<K: Value, R: Value>(self) -> Result<Aggregate<Vec<GroupValue<K, R>>>, Error> {
        let (plan, reducer) = self.numeric_plan::<R>(Some(K::definition()), Operation::Average)?;
        self.group_values(&plan, reducer)
    }

    // One admission path for synchronous, grouped and retained numeric terminals.
    pub(super) fn numeric_plan<R: Value>(
        &self,
        group: Option<&TypeSchema>,
        operation: Operation,
    ) -> Result<(Plan<'static>, Reducer), Error> {
        let mut plan = self.aggregate_plan(group, group.is_some())?;
        let state = NumericAggregate::admit(E::SCHEMA, R::definition(), operation)?;
        plan.selected |= 1 << state.position;
        Ok((plan, Reducer::Numeric(state)))
    }
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub(super) enum Operation {
    // Stable job-binding tags; preserve the original sum identity.
    Sum = 1,
    Average = 2,
}

#[derive(Clone)]
pub(in crate::store::query) struct NumericAggregate {
    pub(super) position: usize,
    source_scale: u32,
    output: ValueSchema,
    total: Total,
    count: u64,
    pub(super) operation: Operation,
}
#[derive(Clone)]
enum Total {
    Signed(I256),
    Unsigned(U256),
}
impl NumericAggregate {
    fn admit(
        entity: &EntitySchema,
        output: &TypeSchema,
        operation: Operation,
    ) -> Result<Self, Error> {
        let TypeBody::Record([result]) = output.body else {
            return Err(Error::InvalidQuery);
        };
        let (position, source) = entity
            .fields
            .iter()
            .enumerate()
            .find(|(_, field)| field.name == result.name)
            .ok_or(Error::InvalidQuery)?;
        if source.optional != result.optional {
            return Err(Error::InvalidQuery);
        }
        let source_scale = match source.value {
            FieldType::Decimal { scale } => scale,
            _ => 0,
        };
        let signed = if matches!(operation, Operation::Average) {
            if !matches!(result.value, FieldType::Decimal { scale: 0..=28 }) {
                return Err(Error::InvalidQuery);
            }
            match source.value {
                FieldType::Decimal { .. } => true,
                value => integer(value).ok_or(Error::InvalidQuery)?.0,
            }
        } else {
            match (source.value, result.value) {
                (FieldType::Decimal { scale: source }, FieldType::Decimal { scale: result })
                    if source == result =>
                {
                    true
                }
                (source, result) => {
                    let (signed, bits) = integer(source).ok_or(Error::InvalidQuery)?;
                    let (result_signed, result_bits) =
                        integer(result).ok_or(Error::InvalidQuery)?;
                    if signed != result_signed || bits > result_bits {
                        return Err(Error::InvalidQuery);
                    }
                    signed
                }
            }
        };
        Ok(Self {
            position,
            source_scale,
            output: ValueSchema {
                value: result.value,
                optional: result.optional,
            },
            total: if signed {
                Total::Signed(I256::ZERO)
            } else {
                Total::Unsigned(U256::ZERO)
            },
            count: 0,
            operation,
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
                    IndexValue::Decimal(n) => n
                        .coefficient_at_scale(self.source_scale)
                        .ok_or(Error::Corrupt)?,
                    _ => return Err(Error::Corrupt),
                };
                // Both synchronous and retained scans bound input counts; sums
                // remain checked independently of that resource proof.
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
        self.count = self.count.checked_add(1).ok_or(Error::ArithmeticOverflow)?;
        Ok(())
    }

    // The job binding supplies admitted type/scale/operation metadata. Store only
    // the exact accumulator and presence count, never a second schema or value AST.
    pub(super) fn checkpoint(&self) -> (u64, [u8; 32]) {
        (
            self.count,
            match self.total {
                Total::Signed(n) => n.to_be_bytes(),
                Total::Unsigned(n) => n.to_be_bytes(),
            },
        )
    }

    pub(super) fn restore(&mut self, count: u64, total: [u8; 32]) {
        self.count = count;
        self.total = match self.total {
            Total::Signed(_) => Total::Signed(I256::from_be_bytes(total)),
            Total::Unsigned(_) => Total::Unsigned(U256::from_be_bytes(total)),
        };
    }

    pub(super) fn retained_bytes(&self) -> usize {
        // Total payload plus the average's denominator. Fixed metadata remains
        // bounded by the group cap, as for count/extrema reducers.
        32 + if matches!(self.operation, Operation::Average) {
            8
        } else {
            0
        }
    }

    pub(super) fn output(&self) -> ValueSchema {
        self.output
    }

    pub(super) fn finish(mut self) -> Result<Completed, Error> {
        if self.count == 0 {
            return Ok(Completed::Value(None));
        }
        if matches!(self.operation, Operation::Average) {
            let FieldType::Decimal { scale } = self.output.value else {
                return Err(Error::Corrupt);
            };
            let numerator = match self.total {
                Total::Signed(n) => n,
                // A nonnegative integer total this large cannot produce a Decimal
                // coefficient fitting i128 even after division by the retained
                // job's maximum one million inputs (synchronous scans use 512).
                Total::Unsigned(n) => n.try_into().map_err(|_| Error::ArithmeticOverflow)?,
            };
            self.total = Total::Signed(rounded_average(
                numerator,
                self.count,
                self.source_scale,
                scale,
            )?);
        }
        let mut output = Encoder::new();
        let mut key = Vec::new();
        if self.output.optional {
            output.boolean(true)?;
        }
        // The ordinary owned codec supplies result bytes; checked conversion occurs
        // only after complete accumulation, never once per input or work chunk.
        macro_rules! encode {
            ($method:ident, $variant:ident, $value:expr) => {{
                let value = $value.try_into().map_err(|_| Error::ArithmeticOverflow)?;
                output.$method(value)?;
                IndexValue::$variant(value).append_component(self.output.optional, &mut key);
            }};
        }
        match (self.total, self.output.value) {
            (Total::Signed(n), FieldType::Int8) => encode!(int8, Int8, n),
            (Total::Signed(n), FieldType::Int16) => encode!(int16, Int16, n),
            (Total::Signed(n), FieldType::Int32) => encode!(int32, Int32, n),
            (Total::Signed(n), FieldType::Int64) => encode!(int64, Int64, n),
            (Total::Signed(n), FieldType::Int128) => encode!(int128, Int128, n),
            (Total::Signed(n), FieldType::Decimal { scale }) => {
                let coefficient = n.try_into().map_err(|_| Error::ArithmeticOverflow)?;
                let value = Decimal::try_new(coefficient, scale).ok_or(Error::Corrupt)?;
                output.decimal(value, scale)?;
                IndexValue::Decimal(value).append_component(self.output.optional, &mut key);
            }
            (Total::Unsigned(n), FieldType::Nat8) => encode!(nat8, Nat8, n),
            (Total::Unsigned(n), FieldType::Nat16) => encode!(nat16, Nat16, n),
            (Total::Unsigned(n), FieldType::Nat32) => encode!(nat32, Nat32, n),
            (Total::Unsigned(n), FieldType::Nat64) => encode!(nat64, Nat64, n),
            (Total::Unsigned(n), FieldType::Nat128) => encode!(nat128, Nat128, n),
            (Total::Unsigned(n), FieldType::U256) => {
                output.u256(n)?;
                IndexValue::U256(n).append_component(self.output.optional, &mut key);
            }
            _ => return Err(Error::Corrupt),
        }
        Ok(Completed::Value(Some((key, output.finish()))))
    }
}

fn rounded_average(
    total: I256,
    count: u64,
    source_scale: u32,
    result_scale: u32,
) -> Result<I256, Error> {
    // One million signed 128-bit inputs plus 28 decimal scale digits fit I256.
    // For unsigned totals, a scale-up overflow cannot hide a fitting i128 result:
    // division by at most one million cannot bring that magnitude into range.
    // The scale-down denominator and twice-remainder also fit comfortably.
    let factor = I256::from(10_i128.pow(source_scale.abs_diff(result_scale)));
    let (numerator, denominator) = if result_scale >= source_scale {
        (
            total.checked_mul(factor).ok_or(Error::ArithmeticOverflow)?,
            I256::from(count),
        )
    } else {
        (total, I256::from(count) * factor)
    };
    let quotient = numerator / denominator;
    let twice_remainder = (numerator % denominator).abs() * 2;
    let round = twice_remainder > denominator
        || (twice_remainder == denominator && quotient % 2 != I256::ZERO);
    Ok(if round {
        quotient
            + if numerator < I256::ZERO {
                I256::MINUS_ONE
            } else {
                I256::ONE
            }
    } else {
        quotient
    })
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
