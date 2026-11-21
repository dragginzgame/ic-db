//! Exact integer, decimal and finite float totals, with rounding at completion.

use super::float_sum::{Binary32Sum, Binary64Sum};
use super::{
    super::Plan, Aggregate, Completed, GroupOrder, GroupSummaries, GroupSummary, GroupValue, Query,
    Reducer, SummaryMeasure,
};
use crate::{
    Decimal, Entity, EntitySchema, Error, FieldType, IndexValue, TypeBody, TypeSchema, Value,
    ValueSchema, codec::Encoder,
};
use ethnum::{I256, U256};
use ic_storage::Memory;

impl<E: Entity, M: Memory> Query<'_, E, M> {
    /// Sum one integer, Decimal or finite float field into a single-field result record with the same field
    /// name and optionality. Scalar newtype inputs use their declared primitive.
    /// The result may preserve the exact input newtype. A primitive integer result may widen within
    /// the same signed/unsigned family. Unsigned results may widen to U256.
    /// Decimal results preserve the source's declared scale; coefficients accumulate
    /// exactly in I256 and must fit i128 at completion. No rescaling or rounding.
    /// Float32 and Float64 sum finite values exactly, rounding once to nearest with ties to even.
    /// Empty/all-absent input returns `None`; present zero returns a record.
    /// Signed accumulation uses I256, so cancelling prefixes do not cause narrow
    /// overflow. Unsigned accumulation uses checked U256. The final total must fit
    /// the declared result width. Integer and Decimal sums retain their exact result types.
    /// # Errors
    /// Refuses unsupported/narrower/cross-signedness result shapes, cursors, DISTINCT,
    /// input ordering and group modifiers. Work exhaustion returns `Limit` without
    /// a prefix total; numeric overflow returns `ArithmeticOverflow`. Output bytes
    /// count the completed result record, with `RowTooLarge` on overflow of that budget.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn sum<R: Value>(self) -> Result<Aggregate<Option<R>>, Error> {
        observe!(query, {
            let (plan, reducer) = self.numeric_plan::<R>(None, Operation::Sum)?;

            self.scalar_value(plan, reducer)
        })
    }

    /// Sum one integer, Decimal or finite float operand per exact grouping record, retaining absent keys
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
        observe!(query, {
            let (plan, reducer) = self.numeric_plan::<R>(Some(K::definition()), Operation::Sum)?;

            self.group_values(plan, reducer)
        })
    }

    /// Average present values into a single-field record with matching name and optionality.
    /// Float inputs preserve their width. Integer and Decimal inputs produce Decimal at the
    /// declared result scale (0–28). A named result preserves the source newtype.
    /// Divide the exact total by the present count and round once, nearest with
    /// ties to even. Empty and all-absent inputs return `None`.
    /// # Errors
    /// Refuses unsupported input/result pairs, changed optionality, cursors,
    /// DISTINCT, input ordering and group modifiers. Work exhaustion returns `Limit`;
    /// an unrepresentable rounded coefficient returns `ArithmeticOverflow`.
    /// Output-byte bounds match [`Self::sum`]. No partial average is returned.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn avg<R: Value>(self) -> Result<Aggregate<Option<R>>, Error> {
        observe!(query, {
            let (plan, reducer) = self.numeric_plan::<R>(None, Operation::Average)?;

            self.scalar_value(plan, reducer)
        })
    }

    /// Average per group using [`Self::avg`]'s result type and rounding contract.
    /// Finalize all groups before filtering, ordering and output limits; these
    /// operations compare the rounded results. Retain all-absent groups.
    /// # Errors
    /// Scalar average admission/range errors and grouped work/state/output bounds
    /// apply. Overflow in an excluded group still refuses the whole query.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn group_avg<K: Value, R: Value>(self) -> Result<Aggregate<Vec<GroupValue<K, R>>>, Error> {
        observe!(query, {
            let (plan, reducer) =
                self.numeric_plan::<R>(Some(K::definition()), Operation::Average)?;

            self.group_values(plan, reducer)
        })
    }

    /// Compute present count, sum and average for one numeric field in one scan.
    /// S and A name the same operand and follow the result rules of sum and avg.
    /// All-absent groups have count zero and two absent results. Supports key order or an explicit summary measure
    /// in either direction. The row limit applies after every group is checked.
    /// # Errors
    /// Refuses unnamed group predicates, unspecified aggregate-value ordering and paging. Ordinary
    /// grouped numeric type, overflow, work, state and output bounds apply.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn group_summary<K: Value, S: Value, A: Value>(
        self,
    ) -> Result<GroupSummaries<K, S, A>, Error> {
        observe!(query, {
            let (mut plan, initial, sum) = self.summary_plan::<K, S, A>()?;
            super::super::order::select_unique_aggregate_path(E::SCHEMA, &self.request, &mut plan)?;
            super::group_output(
                self.grouped(&plan, initial, Some(sum))?,
                &self.request,
                decode_summary,
            )
        })
    }

    pub(super) fn summary_plan<K: Value, S: Value, A: Value>(
        &self,
    ) -> Result<(Plan<'static>, Reducer, ValueSchema), Error> {
        let request = &self.request.groups;
        let order = request.order.unwrap_or_default();
        if !matches!(
            order,
            GroupOrder::Key | GroupOrder::KeyDescending | GroupOrder::Summary(..)
        ) {
            return Err(Error::InvalidQuery);
        }
        let (plan, initial) = self.numeric_plan::<A>(Some(K::definition()), Operation::Average)?;
        let sum = NumericAggregate::admit(E::SCHEMA, S::definition(), Operation::Sum)?;
        let Reducer::Numeric(average) = &initial else {
            return Err(Error::Corrupt);
        };
        if sum.position != average.position {
            return Err(Error::InvalidQuery);
        }
        Ok((plan, initial, sum.output))
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

pub(super) fn decode_summary<K: Value, S: Value, A: Value>(
    key: &[u8],
    result: Completed,
) -> Result<GroupSummary<K, S, A>, Error> {
    let Completed::Summary {
        count,
        sum,
        average,
        ..
    } = result
    else {
        return Err(Error::Corrupt);
    };
    Ok(GroupSummary {
        key: super::decode_record(key)?,
        count,
        sum: sum.as_deref().map(super::decode_record).transpose()?,
        average: average.as_deref().map(super::decode_record).transpose()?,
    })
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
    Float32(Box<Binary32Sum>),
    Float64(Box<Binary64Sum>),
    Signed(I256),
    Unsigned(U256),
}
impl Total {
    fn integer_zero(signed: bool) -> Self {
        if signed {
            Self::Signed(I256::ZERO)
        } else {
            Self::Unsigned(U256::ZERO)
        }
    }
}

type EncodedValue = (Vec<u8>, Vec<u8>);

impl NumericAggregate {
    fn admit(
        entity: &EntitySchema,
        output: &TypeSchema,
        operation: Operation,
    ) -> Result<Self, Error> {
        let TypeBody::Record([result]) = output.body else {
            return Err(Error::InvalidQuery);
        };
        let position = entity
            .field_position(result.name)
            .ok_or(Error::InvalidQuery)?;
        let source = &entity.fields[position];
        if source.optional != result.optional {
            return Err(Error::InvalidQuery);
        }
        let input = crate::key::operand_type(source.value);
        // A named result keeps the exact stored field contract, as in a projection.
        if matches!(result.value, FieldType::Named(_)) {
            super::admit_projection(entity, output)?;
        }
        let result_type = crate::key::operand_type(result.value);
        let source_scale = match input {
            FieldType::Decimal { scale } => scale,
            _ => 0,
        };
        let total = match (operation, input, result_type) {
            (_, FieldType::Float32, FieldType::Float32) => Total::Float32(Box::default()),
            (_, FieldType::Float64, FieldType::Float64) => Total::Float64(Box::default()),
            (
                Operation::Sum,
                FieldType::Decimal { scale: from },
                FieldType::Decimal { scale: to },
            ) if from == to => Total::Signed(I256::ZERO),
            (
                Operation::Average,
                FieldType::Decimal { .. },
                FieldType::Decimal { scale: 0..=28 },
            ) => Total::Signed(I256::ZERO),
            (Operation::Average, source, FieldType::Decimal { scale: 0..=28 }) => {
                Total::integer_zero(integer(source).ok_or(Error::InvalidQuery)?.0)
            }
            (Operation::Average, _, _) => return Err(Error::InvalidQuery),
            (Operation::Sum, source, result) => {
                let (signed, bits) = integer(source).ok_or(Error::InvalidQuery)?;
                let (result_signed, result_bits) = integer(result).ok_or(Error::InvalidQuery)?;
                if signed != result_signed || bits > result_bits {
                    return Err(Error::InvalidQuery);
                }
                Total::integer_zero(signed)
            }
        };
        Ok(Self {
            position,
            source_scale,
            output: ValueSchema {
                value: result.value,
                optional: result.optional,
            },
            total,
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
            Total::Float32(total) => {
                let IndexValue::Float32(value) = value else {
                    return Err(Error::Corrupt);
                };
                total.add(value);
            }
            Total::Float64(total) => {
                let IndexValue::Float64(value) = value else {
                    return Err(Error::Corrupt);
                };
                total.add(value);
            }
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
                    IndexValue::U256(n) => n.into(),
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
    pub(super) fn with_checkpoint<T>(&self, write: impl FnOnce(u64, &[u8]) -> T) -> T {
        match &self.total {
            Total::Signed(n) => write(self.count, &n.to_be_bytes()),
            Total::Unsigned(n) => write(self.count, &n.to_be_bytes()),
            Total::Float32(sum) => write(self.count, sum.checkpoint().as_flattened()),
            Total::Float64(sum) => write(self.count, sum.checkpoint().as_flattened()),
        }
    }

    pub(super) fn restore(&mut self, count: u64, bytes: &[u8]) -> Result<(), Error> {
        match &mut self.total {
            Total::Signed(n) => {
                *n = I256::from_be_bytes(bytes.try_into().map_err(|_| Error::Corrupt)?)
            }
            Total::Unsigned(n) => {
                *n = U256::from_be_bytes(bytes.try_into().map_err(|_| Error::Corrupt)?)
            }
            Total::Float32(sum) => sum.restore(count, bytes)?,
            Total::Float64(sum) => sum.restore(count, bytes)?,
        }
        self.count = count;
        Ok(())
    }

    pub(super) fn total_bytes(&self) -> usize {
        match self.total {
            Total::Float32(_) => Binary32Sum::BYTES,
            Total::Float64(_) => Binary64Sum::BYTES,
            Total::Signed(_) | Total::Unsigned(_) => 32,
        }
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.total_bytes()
            + if matches!(self.operation, Operation::Average) {
                8
            } else {
                0
            }
    }

    pub(super) fn output(&self) -> ValueSchema {
        self.output
    }

    pub(super) fn finish_summary(
        self,
        output: ValueSchema,
        selection: &super::Selection,
    ) -> Result<(Completed, bool), Error> {
        let (sum_key, sum) = self
            .encode_result(
                output,
                Operation::Sum,
                selection.needs_summary_key(SummaryMeasure::Sum),
            )?
            .unzip();
        let (avg_key, average) = self
            .encode_result(
                self.output,
                Operation::Average,
                selection.needs_summary_key(SummaryMeasure::Average),
            )?
            .unzip();
        let count_key = self.count.to_be_bytes();
        let accepted = selection.accepts(|measure| match measure {
            Some(SummaryMeasure::Count) => Some(&count_key),
            Some(SummaryMeasure::Sum) => sum_key.as_deref(),
            Some(SummaryMeasure::Average) => avg_key.as_deref(),
            None => None,
        });
        let sort_key = match selection.order {
            GroupOrder::Summary(SummaryMeasure::Sum, _) => sum_key,
            GroupOrder::Summary(SummaryMeasure::Average, _) => avg_key,
            _ => None,
        };
        Ok((
            Completed::Summary {
                count: self.count,
                sum: sum.map(Vec::into_boxed_slice),
                average: average.map(Vec::into_boxed_slice),
                sort_key: sort_key.map(Vec::into_boxed_slice),
            },
            accepted,
        ))
    }

    pub(super) fn finish(self, needs_key: bool) -> Result<Completed, Error> {
        self.encode_result(self.output, self.operation, needs_key)
            .map(Completed::Value)
    }

    fn encode_result(
        &self,
        schema: ValueSchema,
        operation: Operation,
        needs_key: bool,
    ) -> Result<Option<EncodedValue>, Error> {
        if self.count == 0 {
            return Ok(None);
        }
        let result_type = crate::key::operand_type(schema.value);
        let rounded;
        let total =
            if let (Operation::Average, FieldType::Decimal { scale }) = (operation, result_type) {
                let numerator = match self.total {
                    Total::Float32(_) | Total::Float64(_) => return Err(Error::Corrupt),
                    Total::Signed(n) => n,
                    // A nonnegative integer total this large cannot produce a Decimal
                    // coefficient fitting i128 even after division by the retained
                    // job's maximum one million inputs (synchronous scans use 512).
                    Total::Unsigned(n) => n.try_into().map_err(|_| Error::ArithmeticOverflow)?,
                };
                rounded = Total::Signed(rounded_average(
                    numerator,
                    self.count,
                    self.source_scale,
                    scale,
                )?);
                &rounded
            } else {
                &self.total
            };
        let mut output = Encoder::new();
        if schema.optional {
            output.boolean(true)?;
        }
        // The ordinary owned codec supplies result bytes; checked conversion occurs
        // only after complete accumulation, never once per input or work chunk.
        macro_rules! encode {
            ($method:ident, $variant:ident, $value:expr) => {{
                let value = $value.try_into().map_err(|_| Error::ArithmeticOverflow)?;
                output.$method(value)?;
                IndexValue::$variant(value)
            }};
        }
        let divisor = match operation {
            Operation::Sum => 1,
            Operation::Average => self.count,
        };
        let value = match (total, result_type) {
            (Total::Float32(sum), FieldType::Float32) => {
                let value = sum.finish(divisor)?;
                output.float32(value)?;
                IndexValue::Float32(value)
            }
            (Total::Float64(sum), FieldType::Float64) => {
                let value = sum.finish(divisor)?;
                output.float64(value)?;
                IndexValue::Float64(value)
            }
            (Total::Signed(n), FieldType::Int8) => encode!(int8, Int8, *n),
            (Total::Signed(n), FieldType::Int16) => encode!(int16, Int16, *n),
            (Total::Signed(n), FieldType::Int32) => encode!(int32, Int32, *n),
            (Total::Signed(n), FieldType::Int64) => encode!(int64, Int64, *n),
            (Total::Signed(n), FieldType::Int128) => encode!(int128, Int128, *n),
            (Total::Signed(n), FieldType::Decimal { scale }) => {
                let coefficient = (*n).try_into().map_err(|_| Error::ArithmeticOverflow)?;
                let value = Decimal::try_new(coefficient, scale).ok_or(Error::Corrupt)?;
                output.decimal(value, scale)?;
                IndexValue::Decimal(value)
            }
            (Total::Unsigned(n), FieldType::Nat8) => encode!(nat8, Nat8, *n),
            (Total::Unsigned(n), FieldType::Nat16) => encode!(nat16, Nat16, *n),
            (Total::Unsigned(n), FieldType::Nat32) => encode!(nat32, Nat32, *n),
            (Total::Unsigned(n), FieldType::Nat64) => encode!(nat64, Nat64, *n),
            (Total::Unsigned(n), FieldType::Nat128) => encode!(nat128, Nat128, *n),
            (Total::Unsigned(n), FieldType::U256) => {
                let value = (*n).into();
                output.u256(value)?;
                IndexValue::U256(value)
            }
            _ => return Err(Error::Corrupt),
        };
        let mut key = Vec::new();
        if needs_key {
            value.append_component(schema.optional, &mut key);
        }

        Ok(Some((key, output.finish())))
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
