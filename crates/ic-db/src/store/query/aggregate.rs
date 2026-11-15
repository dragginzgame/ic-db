//! Bounded aggregates over the shared scanner and exact reducers, without row history.
pub(super) mod jobs;
mod numeric;

use super::{
    Comparison, MAX_TERMS, Plan, Query, QueryPage, admit_projection, copy_projection, execute,
};
use crate::{
    Entity, EntitySchema, Error, IndexValue, TypeSchema, Value, ValueSchema, codec::Decoder, key,
};
use ic_storage::Memory;
use std::collections::{BTreeMap, BTreeSet};

const MAX_COMPARISON_KEY: usize = 4096;
const MAX_GROUPS: usize = 32;
const MAX_DISTINCT_VALUES: usize = 32;
const MAX_STATE_BYTES: usize = 128 * 1024;

/// Completed-group ordering. Aggregate ties always use ascending canonical keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GroupOrder {
    /// Ascending group-key tuple, in canonical field-name order (the default).
    #[default]
    Key,
    /// Ascending aggregate value; absent values precede present values.
    AggregateAscending,
    /// Descending aggregate value; absent values follow present values.
    AggregateDescending,
}

#[derive(Default)]
pub(super) struct GroupRequest<'a> {
    predicates: Vec<(Comparison, IndexValue<'a>)>,
    order: Option<GroupOrder>,
}
impl GroupRequest<'_> {
    pub(super) fn is_empty(&self) -> bool {
        self.predicates.is_empty() && self.order.is_none()
    }

    fn admit(&self, entity: &EntitySchema, reducer: &Reducer) -> Result<Selection, Error> {
        let predicates = self.predicates.iter().map(|&(comparison, value)| {
            if matches!(value, IndexValue::Text(text) if text.len() > crate::codec::MAX_ROW_BYTES) {
                return Err(Error::Limit);
            }
            let key = match reducer {
                Reducer::Count { .. } | Reducer::Distinct { .. } => match value {
                    IndexValue::Nat64(value) => Some(value.to_be_bytes().to_vec()),
                    _ => return Err(Error::InvalidOperand),
                },
                _ => {
                    let field = match reducer {
                        Reducer::Extremum { position, .. } => {
                            let field = &entity.fields[*position];
                            ValueSchema { value: field.value, optional: field.optional }
                        }
                        Reducer::Numeric(state) => state.output(),
                        Reducer::Count { .. } | Reducer::Distinct { .. } => return Err(Error::Corrupt),
                    };
                    // Even a required input field can have an absent aggregate result.
                    value.admit(field.value, true)?;
                    if value == IndexValue::Absent {
                        if !matches!(comparison, Comparison::Eq | Comparison::Ne) {
                            return Err(Error::InvalidOperand);
                        }
                        None
                    } else {
                        let mut key = Vec::new();
                        value.append_component(field.optional, &mut key);
                        if key.len() > MAX_COMPARISON_KEY { return Err(Error::Limit); }
                        Some(key)
                    }
                }
            };
            Ok((comparison, key))
        }).collect::<Result<_, Error>>()?;
        Ok(Selection {
            predicates,
            order: self.order.unwrap_or_default(),
        })
    }
}

struct Selection {
    predicates: Vec<(Comparison, Option<Vec<u8>>)>,
    order: GroupOrder,
}

/// A completed aggregate and its execution costs. No partial result or cursor.
#[derive(Debug)]
pub struct Aggregate<T> {
    /// Completed value over all matching input rows.
    pub value: T,
    /// Examined primary/index candidates, including residual nonmatches.
    pub examined: usize,
    /// Fetched and validated source rows.
    pub fetched: usize,
    /// Encoded result bytes, excluding transport framing.
    pub output_bytes: usize,
}

/// One complete group. Absent optional key values belong to a group too.
#[derive(Debug)]
pub struct GroupCount<K> {
    /// Projected grouping tuple in an ordinary named record.
    pub key: K,
    /// Matching rows, or present operand values for `group_count_field`.
    pub count: u64,
}

/// One complete group's aggregate value, preserving all-absent groups.
#[derive(Debug)]
pub struct GroupValue<K, R> {
    /// Projected grouping tuple, including optional absent keys.
    pub key: K,
    /// Single-field result record, or `None` when the group has no present operand.
    pub value: Option<R>,
}

impl<'a, E: Entity, M: Memory> Query<'a, E, M> {
    /// Add an AND predicate on the completed aggregate value, before group ordering
    /// and output limits. At most eight terms. Counts require Nat64 operands;
    /// extrema require the source type; sums require the declared result type.
    /// Absent values support Eq/Ne only.
    /// Only grouped terminals accept this modifier, even for empty input.
    #[must_use]
    pub fn filter_groups(
        mut self,
        comparison: Comparison,
        value: impl Into<IndexValue<'a>>,
    ) -> Self {
        if self.request.groups.predicates.len() == MAX_TERMS {
            self.request.over_limit = true;
        } else {
            self.request
                .groups
                .predicates
                .push((comparison, value.into()));
        }
        self
    }

    /// Order completed groups before applying output limits. Only grouped
    /// terminals accept this modifier. The last call selects the order.
    #[must_use]
    pub fn order_groups(mut self, order: GroupOrder) -> Self {
        self.request.groups.order = Some(order);
        self
    }

    /// Minimum present value of an exact single-field record projection.
    /// Supports ordered scalars/unit enums without requiring an index. The result
    /// preserves the declared type and optionality; empty/all-absent input returns
    /// `None`. `limit` never truncates input. Only the winning record's bytes count
    /// toward `byte_limit`; an absent result occupies zero payload bytes.
    /// # Errors
    /// Refuses multi-field projections, unsupported types, cursors, DISTINCT and
    /// explicit ordering. Work exhaustion or a comparison key beyond 4096 bytes
    /// returns `Limit`; an oversized winning result returns `RowTooLarge`.
    /// Ordinary projection/query errors apply. Completion must precede `work_limit`.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn min<R: Value>(self) -> Result<Aggregate<Option<R>>, Error> {
        self.extremum(false)
    }

    /// Maximum present value, with the same types, bounds and empty-input behavior
    /// as [`Self::min`]. Equal operands retain the first encountered value.
    /// # Errors
    /// The same projection, resource and query errors as [`Self::min`].
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn max<R: Value>(self) -> Result<Aggregate<Option<R>>, Error> {
        self.extremum(true)
    }

    fn extremum<R: Value>(self, maximum: bool) -> Result<Aggregate<Option<R>>, Error> {
        let plan = self.aggregate_plan(Some(R::definition()), false)?;
        let reducer = Reducer::extremum(
            E::SCHEMA,
            plan.projection.ok_or(Error::InvalidQuery)?,
            maximum,
        )?;
        self.scalar_value(&plan, reducer)
    }

    fn scalar_value<R: Value>(
        self,
        plan: &Plan<'_>,
        mut reducer: Reducer,
    ) -> Result<Aggregate<Option<R>>, Error> {
        let page = execute(
            &self.db.map,
            E::SCHEMA,
            &self.request,
            plan,
            Some(Accumulator::Scalar(&mut reducer)),
        )?;
        let mut output_bytes = 0;
        let value = reducer
            .finish()?
            .into_value()?
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

    /// Minimum present operand per group. `K` is an exact grouping record; `R`
    /// is an exact single-field operand record. They may overlap. All-absent
    /// groups retain `None`; empty input yields no groups. No index is required.
    /// Groups default to ascending canonical field-name tuple order;
    /// `filter_groups` and `order_groups` select completed results. `limit` selects
    /// completed groups, never input rows. Output bytes are the key record plus
    /// the present winning record; transport option framing is excluded.
    /// At most 32 groups and 128 KiB of retained keys, records and reducer payloads
    /// are admitted. Every comparison key is bounded to 4096 bytes. Winner growth
    /// is charged, and replacement releases the previous winner's charge.
    /// # Errors
    /// The projection, work and shape refusals of [`Self::min`] apply. Group or
    /// retained-state exhaustion returns `Limit`. An oversized output group
    /// returns `RowTooLarge`; combined output exhaustion returns `Limit`.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn group_min<K: Value, R: Value>(self) -> Result<Aggregate<Vec<GroupValue<K, R>>>, Error> {
        self.group_extrema(false)
    }

    /// Maximum present operand per group, with the contract of [`Self::group_min`].
    /// # Errors
    /// The same projection, work, state, shape and output refusals as grouped minimum.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn group_max<K: Value, R: Value>(self) -> Result<Aggregate<Vec<GroupValue<K, R>>>, Error> {
        self.group_extrema(true)
    }

    fn group_extrema<K: Value, R: Value>(
        self,
        maximum: bool,
    ) -> Result<Aggregate<Vec<GroupValue<K, R>>>, Error> {
        let mut plan = self.aggregate_plan(Some(K::definition()), true)?;
        let operand = admit_projection(E::SCHEMA, R::definition())?;
        let initial = Reducer::extremum(E::SCHEMA, operand, maximum)?;
        plan.selected |= operand;
        self.group_values(&plan, initial)
    }

    fn group_values<K: Value, R: Value>(
        self,
        plan: &Plan<'_>,
        initial: Reducer,
    ) -> Result<Aggregate<Vec<GroupValue<K, R>>>, Error> {
        let (groups, page) = self.grouped(plan, initial)?;
        let (value, output_bytes) = value_output(groups, self.request.rows, self.request.bytes)?;
        Ok(Aggregate {
            value,
            examined: page.examined,
            fetched: page.fetched,
            output_bytes,
        })
    }

    /// Count all matching rows in one synchronous execution. Empty input yields zero.
    /// `limit` does not limit input; the scalar output occupies eight bytes.
    /// Completion must be observed before `work_limit` is reached; an exact-boundary
    /// scan can conservatively refuse. Use `start_count`/`resume_count` for retained work.
    /// # Errors
    /// Refuses cursors, DISTINCT, explicit ordering and the ordinary query failures.
    /// Work exhaustion returns `Limit`, never a partial count. Too few output bytes
    /// returns `RowTooLarge`.
    /// # Panics
    /// Supplier corruption traps without repair, as for ordinary query execution.
    pub fn count(self) -> Result<Aggregate<u64>, Error> {
        self.scalar_count(None)
    }

    /// Count present values of a top-level field, including non-scalar fields.
    /// Empty input and all-absent input yield zero; present empty values count.
    /// Limits, refusals and corruption traps are the same as [`Self::count`].
    /// # Errors
    /// Unknown fields refuse even for empty input. Ordinary count errors apply.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn count_field(self, field: &str) -> Result<Aggregate<u64>, Error> {
        self.scalar_count(Some(field))
    }

    /// Count distinct projected tuples, including absent values, without an index.
    /// Retain at most 32 distinct comparison keys, each at most 4096 bytes, within
    /// the shared 128-KiB state bound. No projected records are retained. `limit`
    /// never truncates this count. The scalar result occupies eight bytes.
    /// # Errors
    /// Ordinary count/projection errors and distinct-state exhaustion refuse.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn count_distinct<K: Value>(self) -> Result<Aggregate<u64>, Error> {
        if self.request.bytes < 8 {
            return Err(Error::RowTooLarge);
        }
        let plan = self.aggregate_plan(Some(K::definition()), false)?;
        let reducer = Reducer::distinct(plan.projection.ok_or(Error::InvalidQuery)?);
        self.scalar_count_value(&plan, reducer)
    }

    fn scalar_count(self, field: Option<&str>) -> Result<Aggregate<u64>, Error> {
        if self.request.bytes < 8 {
            return Err(Error::RowTooLarge);
        }
        let plan = self.aggregate_plan(None, false)?;
        let reducer = self.count_reducer(field)?;
        self.scalar_count_value(&plan, reducer)
    }

    fn scalar_count_value(
        self,
        plan: &Plan<'_>,
        mut reducer: Reducer,
    ) -> Result<Aggregate<u64>, Error> {
        let page = execute(
            &self.db.map,
            E::SCHEMA,
            &self.request,
            plan,
            Some(Accumulator::Scalar(&mut reducer)),
        )?;
        Ok(Aggregate {
            value: reducer.finish()?.into_count()?,
            examined: page.examined,
            fetched: page.fetched,
            output_bytes: 8,
        })
    }

    /// Count matching rows grouped by an exact named-record projection.
    /// Keys support ordered scalars/unit enums, including optional absence; no index
    /// is required. Results default to ascending tuple order in canonical field-name order.
    /// `filter_groups` and `order_groups` select completed results before output limits.
    /// `limit` selects the first completed groups (1..=32), never input rows.
    /// `byte_limit` bounds key-record bytes plus eight count bytes per group.
    /// Empty input yields no groups. At most 32 groups and 128 KiB of key/output
    /// state may be retained, independently of the requested output limit;
    /// each comparison key is capped at 4096 bytes, independently of index-key limits.
    /// # Errors
    /// Refuses unsupported key types, cursors, DISTINCT and explicit ordering.
    /// Work, group or retained-state exhaustion returns `Limit`, never partial groups.
    /// Completion must be observed before `work_limit`; a boundary scan can refuse.
    /// Ordinary projection/query integrity and byte-limit errors also apply.
    /// # Panics
    /// Supplier corruption traps without repair, as for ordinary query execution.
    pub fn group_count<K: Value>(self) -> Result<Aggregate<Vec<GroupCount<K>>>, Error> {
        self.group_counts(None)
    }

    /// Count present operand values within each projected group. Groups whose
    /// operands are all absent remain in the output with count zero. The operand
    /// may be any top-level stored field and need not belong to the group key.
    /// Bounds and ordering are the same as [`Self::group_count`].
    /// # Errors
    /// Unknown operands and ordinary grouped-count errors refuse.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn group_count_field<K: Value>(
        self,
        field: &str,
    ) -> Result<Aggregate<Vec<GroupCount<K>>>, Error> {
        self.group_counts(Some(field))
    }

    /// Count distinct projected `R` tuples within each projected `K` group.
    /// Absence is a tuple component, as for [`Self::count_distinct`]; an all-absent
    /// group has one distinct tuple, not zero. Projections may overlap. No index
    /// is required. Complete input before group filtering/order and output limits.
    /// Retain at most 32 groups and 32 distinct tuples per group, with one shared
    /// 128-KiB state allowance and a 4096-byte cap per comparison key.
    /// # Errors
    /// Refuses unsupported projections, cursors, DISTINCT and explicit input order.
    /// Work, cardinality or byte exhaustion returns `Limit`, never partial counts.
    /// Ordinary grouped-count output and integrity errors apply.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn group_count_distinct<K: Value, R: Value>(
        self,
    ) -> Result<Aggregate<Vec<GroupCount<K>>>, Error> {
        let mut plan = self.aggregate_plan(Some(K::definition()), true)?;
        let operand = admit_projection(E::SCHEMA, R::definition())?;
        admit_ordered_fields(E::SCHEMA, operand)?;
        plan.selected |= operand;
        self.group_count_values(&plan, Reducer::distinct(operand))
    }

    fn group_counts<K: Value>(
        self,
        field: Option<&str>,
    ) -> Result<Aggregate<Vec<GroupCount<K>>>, Error> {
        let plan = self.aggregate_plan(Some(K::definition()), true)?;
        let reducer = self.count_reducer(field)?;
        self.group_count_values(&plan, reducer)
    }

    fn group_count_values<K: Value>(
        self,
        plan: &Plan<'_>,
        reducer: Reducer,
    ) -> Result<Aggregate<Vec<GroupCount<K>>>, Error> {
        let (groups, page) = self.grouped(plan, reducer)?;
        let (value, output_bytes) = count_output(groups, self.request.rows, self.request.bytes)?;
        Ok(Aggregate {
            value,
            examined: page.examined,
            fetched: page.fetched,
            output_bytes,
        })
    }

    fn count_reducer(&self, field: Option<&str>) -> Result<Reducer, Error> {
        // Presence needs only the validated tag, not an ordered scalar slot.
        let presence = field
            .map(|name| {
                E::SCHEMA
                    .fields
                    .iter()
                    .enumerate()
                    .find(|(_, f)| f.name == name)
                    .map(|(i, f)| f.optional.then_some(i))
                    .ok_or(Error::InvalidQuery)
            })
            .transpose()?
            .flatten();
        Ok(Reducer::Count { presence, total: 0 })
    }

    fn grouped(
        &self,
        plan: &Plan<'_>,
        initial: Reducer,
    ) -> Result<(Groups, QueryPage<Vec<u8>>), Error> {
        let mut groups = self.admit_groups(plan, initial)?;
        let page = execute(
            &self.db.map,
            E::SCHEMA,
            &self.request,
            plan,
            Some(Accumulator::Groups(&mut groups)),
        )?;
        Ok((groups, page))
    }

    fn admit_groups(&self, plan: &Plan<'_>, initial: Reducer) -> Result<Groups, Error> {
        Ok(Groups {
            selection: self.request.groups.admit(E::SCHEMA, &initial)?,
            mask: plan.projection.ok_or(Error::InvalidQuery)?,
            initial,
            rows: BTreeMap::new(),
            state_bytes: 0,
        })
    }

    fn aggregate_plan(
        &self,
        output: Option<&TypeSchema>,
        group_output: bool,
    ) -> Result<Plan<'static>, Error> {
        if self.request.cursor.is_some()
            || self.request.distinct
            || self.request.order.is_some()
            || self.request.descending
        {
            return Err(Error::InvalidQuery);
        }
        self.db.schema.admits(E::SCHEMA)?;
        let mut plan = Plan::admit(
            E::SCHEMA,
            &self.request,
            self.db.schema.fingerprint(),
            self.db.incarnation,
            output,
            group_output,
        )?;
        if let Some(mask) = plan.projection {
            admit_ordered_fields(E::SCHEMA, mask)?;
            plan.selected |= mask;
        }
        Ok(plan)
    }
}

fn admit_ordered_fields(entity: &EntitySchema, mask: u16) -> Result<(), Error> {
    for (i, field) in entity.fields.iter().enumerate() {
        if mask & (1 << i) != 0 && !key::supports(field.value) {
            return Err(Error::InvalidQuery);
        }
    }
    Ok(())
}

fn comparison_key(
    entity: &EntitySchema,
    mask: u16,
    values: &[IndexValue<'_>],
) -> Result<Vec<u8>, Error> {
    let mut key = Vec::new();
    for (i, field) in entity.fields.iter().enumerate() {
        if mask & (1 << i) != 0 {
            values[i].append_component(field.optional, &mut key);
            if key.len() > MAX_COMPARISON_KEY {
                return Err(Error::Limit);
            }
        }
    }
    Ok(key)
}

fn value_output<K: Value, R: Value>(
    groups: Groups,
    rows: usize,
    bytes: usize,
) -> Result<(Vec<GroupValue<K, R>>, usize), Error> {
    let mut output_bytes = 0;
    let value = groups
        .into_output()?
        .into_iter()
        .take(rows)
        .map(|(key, reducer)| {
            let value = reducer.into_value()?;
            admit_output(
                key.len() + value.as_ref().map_or(0, Vec::len),
                bytes,
                &mut output_bytes,
            )?;
            Ok(GroupValue {
                key: decode_record(&key)?,
                value: value.as_deref().map(decode_record).transpose()?,
            })
        })
        .collect::<Result<_, Error>>()?;
    Ok((value, output_bytes))
}

fn count_output<K: Value>(
    groups: Groups,
    rows: usize,
    bytes: usize,
) -> Result<(Vec<GroupCount<K>>, usize), Error> {
    let mut output_bytes = 0;
    let value = groups
        .into_output()?
        .into_iter()
        .take(rows)
        .map(|(key, reducer)| {
            admit_output(key.len() + 8, bytes, &mut output_bytes)?;
            Ok(GroupCount {
                key: decode_record(&key)?,
                count: reducer.into_count()?,
            })
        })
        .collect::<Result<_, Error>>()?;
    Ok((value, output_bytes))
}

fn decode_record<R: Value>(bytes: &[u8]) -> Result<R, Error> {
    let mut reader = Decoder::new(bytes)?;
    let value = R::decode_from(&mut reader)?;
    reader.finish()?;
    Ok(value)
}

fn admit_output(size: usize, limit: usize, used: &mut usize) -> Result<(), Error> {
    if size > limit {
        return Err(Error::RowTooLarge);
    }
    if size > limit - *used {
        return Err(Error::Limit);
    }
    *used += size;
    Ok(())
}

// Closed choices over delivered reducers and one grouping table, not user extensions.
pub(super) enum Accumulator<'a> {
    Scalar(&'a mut Reducer),
    Groups(&'a mut Groups),
}
impl Accumulator<'_> {
    pub(super) fn needs_spans(&self) -> bool {
        !matches!(
            self,
            Self::Scalar(
                Reducer::Numeric(_)
                    | Reducer::Count { presence: None, .. }
                    | Reducer::Distinct { .. }
            )
        )
    }

    pub(super) fn push(
        &mut self,
        entity: &EntitySchema,
        values: &[IndexValue<'_>],
        bytes: &[u8],
        spans: &[(u16, u16)],
    ) -> Result<(), Error> {
        match self {
            Self::Scalar(state) => state.push(entity, values, bytes, spans, MAX_STATE_BYTES),
            Self::Groups(state) => state.push(entity, values, bytes, spans),
        }
    }
}

#[derive(Clone)]
pub(super) enum Reducer {
    Numeric(numeric::NumericAggregate),
    Distinct {
        mask: u16,
        seen: BTreeSet<Vec<u8>>,
    },
    Count {
        presence: Option<usize>,
        total: u64,
    },
    Extremum {
        position: usize,
        maximum: bool,
        selected: Option<(Vec<u8>, Vec<u8>)>,
    },
}
impl Reducer {
    fn distinct(mask: u16) -> Self {
        Self::Distinct {
            mask,
            seen: BTreeSet::new(),
        }
    }

    fn extremum(entity: &EntitySchema, mask: u16, maximum: bool) -> Result<Self, Error> {
        if mask.count_ones() != 1 {
            return Err(Error::InvalidQuery);
        }
        let position = mask.trailing_zeros() as usize;
        if !key::supports(entity.fields[position].value) {
            return Err(Error::InvalidQuery);
        }
        Ok(Self::Extremum {
            position,
            maximum,
            selected: None,
        })
    }

    fn retained_bytes(&self) -> usize {
        match self {
            Self::Numeric(state) => state.retained_bytes(),
            Self::Count { .. } => 8,
            Self::Distinct { seen, .. } => 8 + seen.iter().map(Vec::capacity).sum::<usize>(),
            Self::Extremum { selected, .. } => selected
                .as_ref()
                .map_or(0, |(key, bytes)| key.capacity() + bytes.capacity()),
        }
    }

    fn finish(self) -> Result<Completed, Error> {
        match self {
            Self::Count { total, .. } => Ok(Completed::Count(total)),
            Self::Distinct { seen, .. } => Ok(Completed::Count(seen.len() as u64)),
            Self::Extremum { selected, .. } => Ok(Completed::Value(selected)),
            Self::Numeric(state) => state.finish(),
        }
    }

    fn push(
        &mut self,
        entity: &EntitySchema,
        values: &[IndexValue<'_>],
        bytes: &[u8],
        spans: &[(u16, u16)],
        allowance: usize,
    ) -> Result<(), Error> {
        match self {
            Self::Numeric(state) => state.push(values)?,
            Self::Distinct { mask, seen } => {
                let key = comparison_key(entity, *mask, values)?;
                if !seen.contains(&key) {
                    if seen.len() == MAX_DISTINCT_VALUES
                        || 8 + seen.iter().map(Vec::capacity).sum::<usize>() + key.capacity()
                            > allowance
                    {
                        return Err(Error::Limit);
                    }
                    seen.insert(key);
                }
            }
            Self::Count { presence, total } => {
                *total += u64::from(presence.is_none_or(|i| bytes[usize::from(spans[i].0)] != 0));
            }
            Self::Extremum {
                position,
                maximum,
                selected,
            } => {
                let value = values[*position];
                if value == IndexValue::Absent {
                    return Ok(());
                }
                let mut key = Vec::new();
                value.append_component(entity.fields[*position].optional, &mut key);
                if key.len() > MAX_COMPARISON_KEY {
                    return Err(Error::Limit);
                }
                if selected
                    .as_ref()
                    .is_none_or(|(old, _)| if *maximum { key > *old } else { key < *old })
                {
                    let (start, end) = spans[*position];
                    if key.capacity() + usize::from(end - start) > allowance {
                        return Err(Error::Limit);
                    }
                    // Copy only a winner, after admitting its replacement charge.
                    let output = bytes[usize::from(start)..usize::from(end)].to_vec();
                    if key.capacity() + output.capacity() > allowance {
                        return Err(Error::Limit);
                    }
                    *selected = Some((key, output));
                }
            }
        }
        Ok(())
    }
}

// Completed results cannot be advanced. All numeric range checks precede selection.
enum Completed {
    Count(u64),
    Value(Option<(Vec<u8>, Vec<u8>)>),
}
impl Completed {
    // Counts need only eight stack bytes; extrema borrow their retained ordered key.
    fn result_key<'a>(&'a self, scratch: &'a mut [u8; 8]) -> Option<&'a [u8]> {
        match self {
            Self::Count(total) => {
                *scratch = total.to_be_bytes();
                Some(scratch)
            }
            Self::Value(selected) => selected.as_ref().map(|(key, _)| key.as_slice()),
        }
    }

    fn retained_bytes(&self) -> usize {
        match self {
            Self::Count(_) => 8,
            Self::Value(value) => value
                .as_ref()
                .map_or(0, |(key, bytes)| key.capacity() + bytes.capacity()),
        }
    }
    fn into_count(self) -> Result<u64, Error> {
        match self {
            Self::Count(count) => Ok(count),
            _ => Err(Error::Corrupt),
        }
    }
    fn into_value(self) -> Result<Option<Vec<u8>>, Error> {
        match self {
            Self::Value(value) => Ok(value.map(|(_, bytes)| bytes)),
            _ => Err(Error::Corrupt),
        }
    }
}

pub(super) struct Groups {
    selection: Selection,
    mask: u16,
    // Never advanced: each new group receives its own empty reducer state.
    initial: Reducer,
    rows: BTreeMap<Vec<u8>, (Vec<u8>, Reducer)>,
    state_bytes: usize,
}
impl Groups {
    fn into_output(self) -> Result<Vec<(Vec<u8>, Completed)>, Error> {
        // Accumulation has completed. Output filtering cannot relax input/state bounds.
        let mut state_bytes = self.state_bytes;
        let mut rows = self
            .rows
            .into_iter()
            .map(|(key, (record, reducer))| {
                let previous = reducer.retained_bytes();
                let result = reducer.finish()?;
                state_bytes = state_bytes - previous + result.retained_bytes();
                if state_bytes > MAX_STATE_BYTES {
                    return Err(Error::Limit);
                }
                Ok((key, (record, result)))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        rows.retain(|(_, (_, result))| {
            let mut scratch = [0; 8];
            let actual = result.result_key(&mut scratch);
            self.selection
                .predicates
                .iter()
                .all(|(comparison, operand)| {
                    comparison.accepts(actual.cmp(&operand.as_deref()), actual.is_none())
                })
        });
        if self.selection.order != GroupOrder::Key {
            rows.sort_unstable_by(|(ka, (_, a)), (kb, (_, b))| {
                let (mut sa, mut sb) = ([0; 8], [0; 8]);
                let order = a.result_key(&mut sa).cmp(&b.result_key(&mut sb));
                let order = if self.selection.order == GroupOrder::AggregateDescending {
                    order.reverse()
                } else {
                    order
                };
                order.then_with(|| ka.cmp(kb))
            });
        }
        Ok(rows.into_iter().map(|(_, row)| row).collect())
    }

    fn push(
        &mut self,
        entity: &EntitySchema,
        values: &[IndexValue<'_>],
        bytes: &[u8],
        spans: &[(u16, u16)],
    ) -> Result<(), Error> {
        let key = comparison_key(entity, self.mask, values)?;
        let mut size = 0;
        for (i, span) in spans.iter().enumerate() {
            if self.mask & (1 << i) != 0 {
                size += usize::from(span.1 - span.0);
            }
        }
        if let Some((_, reducer)) = self.rows.get_mut(&key) {
            let previous = reducer.retained_bytes();
            reducer.push(
                entity,
                values,
                bytes,
                spans,
                MAX_STATE_BYTES - self.state_bytes + previous,
            )?;
            self.state_bytes = self.state_bytes - previous + reducer.retained_bytes();
            return Ok(());
        }
        let base = key.capacity() + size;
        let available = MAX_STATE_BYTES - self.state_bytes;
        if self.rows.len() == MAX_GROUPS || base + self.initial.retained_bytes() > available {
            return Err(Error::Limit);
        }
        let mut reducer = self.initial.clone();
        reducer.push(entity, values, bytes, spans, available - base)?;
        let output = copy_projection(bytes, spans, self.mask, size);
        // Map/set nodes and fixed bookkeeping are independently bounded by the
        // group and per-group distinct cardinality caps.
        let retained = key.capacity() + output.capacity() + reducer.retained_bytes();
        if retained > available {
            return Err(Error::Limit);
        }
        self.state_bytes += retained;
        self.rows.insert(key, (output, reducer));
        Ok(())
    }
}
