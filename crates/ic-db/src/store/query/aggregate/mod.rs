//! Bounded aggregates over the shared scanner and exact reducers, without row history.

mod float_sum;
pub(super) mod jobs;
mod numeric;

use super::distinct::DistinctKeys;
use super::{
    Comparison, MAX_COMPARISON_KEY, MAX_DISTINCT_VALUES, MAX_STATE_BYTES, MAX_TERMS, Plan, Query,
    QueryPage, admit_ordered_fields, admit_output, admit_projection, comparison_key,
    copy_projection, execute, projection_size,
};
use crate::{
    Entity, EntitySchema, Error, FieldType, IndexValue, TypeSchema, Value, ValueSchema,
    codec::Decoder, key,
};
use ic_storage::Memory;
use std::collections::BTreeMap;

const MAX_GROUPS: usize = 32;

/// Numeric result used to order completed summary groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SummaryMeasure {
    /// Number of present operands.
    Count = 0,
    /// Completed sum.
    Sum = 1,
    /// Average after rounding to the declared result type.
    Average = 2,
}

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
    /// Descending group-key tuple, in canonical field-name order; absence sorts last.
    KeyDescending,
    /// Retained DISTINCT keys in first matching input order.
    FirstOccurrence,
    /// Order a grouped numeric summary by one completed measure.
    Summary(SummaryMeasure, super::SortDirection),
}

struct GroupPredicate<T> {
    measure: Option<SummaryMeasure>,
    comparison: Comparison,
    operand: T,
    or_next: bool,
}

#[derive(Default)]
pub(super) struct GroupRequest<'a> {
    predicates: Vec<GroupPredicate<IndexValue<'a>>>,
    order: Option<GroupOrder>,
    page: Option<jobs::pages::PageRequest<'a>>,
}
impl GroupRequest<'_> {
    pub(super) fn is_empty(&self) -> bool {
        self.predicates.is_empty() && self.order.is_none() && self.page.is_none()
    }

    fn admit(
        &self,
        entity: &EntitySchema,
        reducer: &Reducer,
        summary_sum: Option<ValueSchema>,
    ) -> Result<Selection, Error> {
        if self
            .page
            .is_some_and(|page| !(1..=MAX_GROUPS).contains(&page.size))
        {
            return Err(Error::Limit);
        }
        let field = match reducer {
            Reducer::Keys(_) => {
                let order = self.order.unwrap_or_default();
                if !self.predicates.is_empty()
                    || !matches!(
                        order,
                        GroupOrder::Key | GroupOrder::KeyDescending | GroupOrder::FirstOccurrence
                    )
                {
                    return Err(Error::InvalidQuery);
                }

                return Ok(Selection {
                    predicates: Vec::new(),
                    order,
                    optional: false,
                });
            }
            Reducer::Count { .. } | Reducer::Distinct { .. } => ValueSchema {
                value: FieldType::Nat64,
                optional: false,
            },
            Reducer::Extremum { position, .. } => {
                let field = &entity.fields[*position];

                ValueSchema {
                    value: field.value,
                    optional: field.optional,
                }
            }
            Reducer::Numeric(state) => state.output(),
        };
        if self.order == Some(GroupOrder::FirstOccurrence) {
            return Err(Error::InvalidQuery);
        }
        // Extrema/numeric results may be absent even with required input fields.
        let absent_result = !matches!(reducer, Reducer::Count { .. } | Reducer::Distinct { .. });
        let predicates = self
            .predicates
            .iter()
            .map(
                |&GroupPredicate {
                     comparison,
                     operand: value,
                     or_next,
                     measure,
                 }| {
                    let (field, absent_result) = match (measure, summary_sum) {
                        (None, None) => (field, absent_result),
                        (Some(SummaryMeasure::Count), Some(_)) => (
                            ValueSchema {
                                value: FieldType::Nat64,
                                optional: false,
                            },
                            false,
                        ),
                        (Some(SummaryMeasure::Sum), Some(sum)) => (sum, true),
                        (Some(SummaryMeasure::Average), Some(_)) => (field, true),
                        _ => return Err(Error::InvalidQuery),
                    };
                    if match value {
                        IndexValue::Text(text) => text.len() > crate::codec::MAX_ROW_BYTES,
                        IndexValue::Blob(bytes) => bytes.len() > crate::codec::MAX_ROW_BYTES,
                        _ => false,
                    } {
                        return Err(Error::Limit);
                    }
                    comparison.admit(field.value, absent_result, value)?;
                    let key = if value == IndexValue::Absent {
                        None
                    } else {
                        let mut key = Vec::new();
                        value.append_component(field.optional, &mut key);
                        if key.len() > MAX_COMPARISON_KEY {
                            return Err(Error::Limit);
                        }

                        Some(key)
                    };

                    Ok(GroupPredicate {
                        measure,
                        comparison,
                        operand: key,
                        or_next,
                    })
                },
            )
            .collect::<Result<_, Error>>()?;

        Ok(Selection {
            predicates,
            order: self.order.unwrap_or_default(),
            optional: field.optional,
        })
    }
}

struct Selection {
    predicates: Vec<GroupPredicate<Option<Vec<u8>>>>,
    order: GroupOrder,
    // Physical comparison-key framing, distinct from an absent aggregate result.
    optional: bool,
}

impl Selection {
    fn needs_summary_key(&self, measure: SummaryMeasure) -> bool {
        matches!(self.order, GroupOrder::Summary(ordered, _) if ordered == measure)
            || self
                .predicates
                .iter()
                .any(|term| term.measure == Some(measure))
    }

    fn accepts<'a>(&self, key: impl Fn(Option<SummaryMeasure>) -> Option<&'a [u8]>) -> bool {
        self.predicates
            .split_inclusive(|term| !term.or_next)
            .all(|clause| {
                clause.iter().any(|term| {
                    term.comparison.accepts(
                        key(term.measure),
                        term.operand.as_deref(),
                        self.optional && term.measure != Some(SummaryMeasure::Count),
                    )
                })
            })
    }
}

/// A completed aggregate and its execution costs. No partial result or cursor.
#[derive(Debug)]
pub struct Aggregate<T> {
    /// Completed value over all matching input rows.
    pub value: T,
    /// Examined primary/index candidates, including residual nonmatches and missing point probes.
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

/// Completed numeric summaries and the work of their shared scan.
pub type GroupSummaries<K, S, A> = Aggregate<Vec<GroupSummary<K, S, A>>>;

/// A page of completed retained numeric summaries.
pub type GroupSummaryPage<K, S, A> = jobs::GroupPage<GroupSummary<K, S, A>>;

/// Count, sum and average of one numeric operand in a completed group.
#[derive(Debug)]
pub struct GroupSummary<K, S, A> {
    /// Grouping record.
    pub key: K,
    /// Number of present operands (the average denominator).
    pub count: u64,
    /// Exact sum converted to the declared sum result.
    pub sum: Option<S>,
    /// Average rounded once to the declared average result.
    pub average: Option<A>,
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
    /// Page completed retained groups, returning at most 1..=32 rows per response.
    /// `limit` still caps the complete selected result and binds the job. Page size
    /// and byte allowance may change. Supply None for the first page; continuation
    /// is valid only for a completed job. Only grouped resume terminals accept this.
    #[must_use]
    pub fn group_page(mut self, size: usize, after: Option<&'a jobs::GroupCursor>) -> Self {
        self.request.groups.page = Some(jobs::pages::PageRequest { size, after });

        self
    }

    /// Add an AND predicate on the completed aggregate value, before group ordering
    /// and output limits. At most eight terms. Counts require Nat64 operands;
    /// extrema require the source type; sums require the declared result type.
    /// Absent values support Eq/Ne only. Text and Blob extrema also accept StartsWith,
    /// EndsWith and Contains.
    /// Only grouped terminals accept this modifier, even for empty input.
    #[must_use]
    pub fn filter_groups(self, comparison: Comparison, value: impl Into<IndexValue<'a>>) -> Self {
        self.add_group_clause(std::iter::once((None, comparison, value.into())))
    }

    /// Require a completed summary measure to satisfy the comparison before ordering or limiting.
    /// Calls are ANDed. Count uses Nat64; sum and average use their declared result types.
    #[must_use]
    pub fn filter_summary(
        self,
        measure: SummaryMeasure,
        comparison: Comparison,
        value: impl Into<IndexValue<'a>>,
    ) -> Self {
        self.add_group_clause(std::iter::once((Some(measure), comparison, value.into())))
    }

    /// Require at least one comparison on a completed count, sum or rounded average.
    /// Alternatives can name different measures. Separate clauses are ANDed.
    /// A clause must be nonempty and all group predicates share the eight-term limit.
    #[must_use]
    pub fn filter_summary_any(
        self,
        alternatives: &[(SummaryMeasure, Comparison, IndexValue<'a>)],
    ) -> Self {
        self.add_group_clause(
            alternatives
                .iter()
                .map(|&(measure, comparison, value)| (Some(measure), comparison, value)),
        )
    }

    /// Add an OR clause on completed aggregate values, ANDed with other group
    /// filters. The clause must be nonempty; all group filters together permit
    /// eight comparisons. Every operand is admitted before reading any rows.
    /// Uses `filter_groups`' exact type and absence rules. All groups must finish
    /// and validate before selection, ordering and output limits. Clause boundaries
    /// bind retained jobs; a singleton clause is equivalent to `filter_groups`.
    #[must_use]
    pub fn filter_groups_any(self, alternatives: &[(Comparison, IndexValue<'a>)]) -> Self {
        self.add_group_clause(
            alternatives
                .iter()
                .map(|&(comparison, value)| (None, comparison, value)),
        )
    }

    fn add_group_clause(
        mut self,
        terms: impl ExactSizeIterator<Item = (Option<SummaryMeasure>, Comparison, IndexValue<'a>)>,
    ) -> Self {
        let count = terms.len();
        if count == 0 || count > MAX_TERMS - self.request.groups.predicates.len() {
            self.request.over_limit = true;
        } else {
            self.request.groups.predicates.extend(terms.enumerate().map(
                |(i, (measure, comparison, operand))| GroupPredicate {
                    measure,
                    comparison,
                    operand,
                    or_next: i + 1 != count,
                },
            ));
        }

        self
    }

    /// Order completed groups before applying output limits. Retained DISTINCT
    /// also accepts key order and first-occurrence order. The last call selects the order.
    #[must_use]
    pub fn order_groups(mut self, order: GroupOrder) -> Self {
        self.request.groups.order = Some(order);

        self
    }

    /// Minimum present value of an exact single-field record projection.
    /// Supports ordered scalars/unit enums without requiring an index. The result
    /// preserves the declared type and optionality; empty/all-absent input returns
    /// `None`. `limit` never truncates input. Only the winning record's bytes count
    /// toward the engine output bound; an absent result occupies zero payload bytes.
    /// # Errors
    /// Refuses multi-field projections, unsupported types, cursors, DISTINCT and
    /// explicit ordering. Work exhaustion or a comparison key beyond 4096 bytes
    /// returns `Limit`; an oversized winning result returns `RowTooLarge`.
    /// Ordinary projection/query errors apply. Completion must precede `work_limit`.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn min<R: Value>(self) -> Result<Aggregate<Option<R>>, Error> {
        observe!(query, { self.extremum(false) })
    }

    /// Maximum present value, with the same types, bounds and empty-input behavior
    /// as [`Self::min`]. Equal operands retain the first encountered value.
    /// # Errors
    /// The same projection, resource and query errors as [`Self::min`].
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn max<R: Value>(self) -> Result<Aggregate<Option<R>>, Error> {
        observe!(query, { self.extremum(true) })
    }

    fn extremum<R: Value>(self, maximum: bool) -> Result<Aggregate<Option<R>>, Error> {
        let (plan, reducer) = self.extremum_plan::<R>(maximum)?;

        self.scalar_value(plan, reducer)
    }

    fn extremum_plan<R: Value>(&self, maximum: bool) -> Result<(Plan<'a>, Reducer), Error> {
        let plan = self.aggregate_plan(Some(R::definition()), false)?;
        let reducer = Reducer::extremum(
            E::SCHEMA,
            plan.projection.ok_or(Error::InvalidQuery)?,
            maximum,
        )?;

        Ok((plan, reducer))
    }

    fn scalar_value<R: Value>(
        self,
        mut plan: Plan<'_>,
        mut reducer: Reducer,
    ) -> Result<Aggregate<Option<R>>, Error> {
        super::order::select_unique_aggregate_path(E::SCHEMA, &self.request, &mut plan)?;
        let page = execute(
            &self.db.read()?.map,
            E::SCHEMA,
            &self.request,
            &plan,
            Some(Accumulator::Scalar(&mut reducer)),
        )?;
        let (value, output_bytes) = scalar_output(reducer, self.request.bytes)?;

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
        observe!(query, { self.group_extrema(false) })
    }

    /// Maximum present operand per group, with the contract of [`Self::group_min`].
    /// # Errors
    /// The same projection, work, state, shape and output refusals as grouped minimum.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn group_max<K: Value, R: Value>(self) -> Result<Aggregate<Vec<GroupValue<K, R>>>, Error> {
        observe!(query, { self.group_extrema(true) })
    }

    fn group_extrema<K: Value, R: Value>(
        self,
        maximum: bool,
    ) -> Result<Aggregate<Vec<GroupValue<K, R>>>, Error> {
        let (plan, initial) = self.group_extremum_plan::<K, R>(maximum)?;

        self.group_values(plan, initial)
    }

    fn group_extremum_plan<K: Value, R: Value>(
        &self,
        maximum: bool,
    ) -> Result<(Plan<'a>, Reducer), Error> {
        let mut plan = self.aggregate_plan(Some(K::definition()), true)?;
        let operand = admit_projection(E::SCHEMA, R::definition())?;
        let initial = Reducer::extremum(E::SCHEMA, operand, maximum)?;
        plan.selected |= operand;

        Ok((plan, initial))
    }

    fn group_values<K: Value, R: Value>(
        self,
        mut plan: Plan<'_>,
        initial: Reducer,
    ) -> Result<Aggregate<Vec<GroupValue<K, R>>>, Error> {
        super::order::select_unique_aggregate_path(E::SCHEMA, &self.request, &mut plan)?;
        let (groups, page) = self.grouped(&plan, initial, None)?;
        let (value, output_bytes) = group_output(
            groups,
            self.request.rows,
            self.request.bytes,
            decode_group_value,
        )?;

        Ok(Aggregate {
            value,
            examined: page.examined,
            fetched: page.fetched,
            output_bytes,
        })
    }

    /// Whether at least one row matches, without materializing an entity.
    /// Can seek a mandatory equality prefix and evaluate remaining filters on its owner rows.
    /// Stops after validating the first matching row; later rows are not inspected.
    /// Returns false only after observing the end of the selected input.
    /// `limit` does not truncate input. The boolean result occupies one byte.
    /// # Errors
    /// Refuses cursors, DISTINCT, explicit ordering and group modifiers, as for
    /// `count`. Exhausting `work_limit` without a match returns `Limit`, never
    /// false; an exact-boundary no-match scan can conservatively refuse.
    /// Ordinary admission and examined-row integrity errors still apply.
    /// # Panics
    /// Supplier corruption traps without repair, as for ordinary query execution.
    pub fn exists(self) -> Result<Aggregate<bool>, Error> {
        observe!(query, {
            let mut plan = self.aggregate_plan(None, false)?;
            self.select_aggregate_row_path(&mut plan)?;
            let mut found = false;
            let page = execute(
                &self.db.read()?.map,
                E::SCHEMA,
                &self.request,
                &plan,
                Some(Accumulator::Exists(&mut found)),
            )?;

            Ok(Aggregate {
                value: found,
                examined: page.examined,
                fetched: page.fetched,
                output_bytes: 1,
            })
        })
    }

    /// Count all matching rows in one synchronous execution. Empty input yields zero.
    /// Uses maintained cardinality for unfiltered input and physical index keys for
    /// fully covered index ranges or primary-key comparison ranges. A partially covered
    /// equality prefix narrows the owner rows checked by the remaining predicates.
    /// Membership counts can probe distinct complete primary or secondary keys
    /// when the fixed prefix and final component prove every filter and the probes fit. Use
    /// [`Self::count_validated`] to check each visited row and selected index owner.
    /// `limit` does not limit input; the scalar output occupies eight bytes.
    /// Completion must be established within `work_limit`; an exact-boundary
    /// scan can conservatively refuse. Use `start_count`/`resume_count` for retained work.
    /// # Errors
    /// Refuses cursors, DISTINCT, explicit ordering and the ordinary query failures.
    /// Work exhaustion returns `Limit`, never a partial count. Too few output bytes
    /// returns `RowTooLarge`.
    /// # Panics
    /// Supplier corruption traps without repair, as for ordinary query execution.
    pub fn count(self) -> Result<Aggregate<u64>, Error> {
        observe!(query, {
            if self.request.bytes < 8 {
                return Err(Error::RowTooLarge);
            }
            let mut plan = self.aggregate_plan(None, false)?;
            if self.request.predicates.is_empty() && self.request.index.is_none() {
                return Ok(Aggregate {
                    value: self.db.read()?.cardinality::<E>()?,
                    examined: 0,
                    fetched: 0,
                    output_bytes: 8,
                });
            }
            if plan.index.is_none()
                && let Some(count) = self.count_members(&plan)?
            {
                return Ok(count);
            }
            if super::order::select_filtered_path(E::SCHEMA, &self.request, &mut plan, true)? {
                return self.count_keys(&plan);
            }
            if plan.index.is_some()
                && let Some(count) = self.count_members(&plan)?
            {
                return Ok(count);
            }
            let reducer = self.count_reducer(None)?;

            self.scalar_count_value(&plan, reducer)
        })
    }

    /// Count by validating each visited row, including its selected index entry.
    /// # Errors
    /// Returns the same admission and resource errors as [`Self::count`], and
    /// reports corruption encountered during traversal.
    pub fn count_validated(self) -> Result<Aggregate<u64>, Error> {
        observe!(query, { self.scalar_count(None) })
    }

    /// Count physical entries in the range selected by `using_index` and its
    /// leading operands. This reads neither index values nor owner rows. A
    /// dangling index entry counts as one. Use [`Self::count_validated`] to validate rows.
    /// Like other scalar counts, the row limit does not truncate the input.
    /// # Errors
    /// Requires an explicit index. Row predicates, ordering, cursors, grouping
    /// and DISTINCT refuse. Visiting the inclusive terminal key proves completion,
    /// even on the last allowed visit. Exhausting the work budget earlier returns
    /// `Limit`; fewer than eight output bytes returns `RowTooLarge`.
    /// # Panics
    /// Malformed supplier storage can trap during traversal.
    pub fn count_index_entries(self) -> Result<Aggregate<u64>, Error> {
        observe!(query, {
            if self.request.index.is_none() || !self.request.predicates.is_empty() {
                return Err(Error::InvalidQuery);
            }
            if self.request.bytes < 8 {
                return Err(Error::RowTooLarge);
            }
            let plan = self.aggregate_plan(None, false)?;

            self.count_keys(&plan)
        })
    }

    fn count_members(&self, plan: &Plan<'_>) -> Result<Option<Aggregate<u64>>, Error> {
        let Some(members) = super::range::point_count_members(E::SCHEMA, plan, &self.request)
        else {
            return Ok(None);
        };
        let db = self.db.read()?;
        let mut count = 0;
        for member in &members {
            let key = crate::store_key::StoreKey::new([plan.prefix.as_slice(), *member].concat())?;
            count += u64::from(db.map.contains_key(&key));
        }

        Ok(Some(Aggregate {
            value: count,
            examined: members.len(),
            fetched: 0,
            output_bytes: 8,
        }))
    }

    fn count_keys(&self, plan: &Plan<'_>) -> Result<Aggregate<u64>, Error> {
        let bounds = super::range::bounds(E::SCHEMA, &self.request, plan)?;
        let mut examined = 0;
        if let Some(range) = bounds {
            let terminal = super::range::terminal_key(&range, false);
            let db = self.db.read()?;
            let entries = db.map.range((range.0.as_ref(), range.1.as_ref()));
            for entry in entries.take(self.request.work) {
                examined += 1;
                if examined == self.request.work && terminal.is_none_or(|end| end != entry.key()) {
                    return Err(Error::Limit);
                }
            }
        }

        Ok(Aggregate {
            value: examined as u64,
            examined,
            fetched: 0,
            output_bytes: 8,
        })
    }

    /// Count present values of a top-level field, including non-scalar fields.
    /// Empty input and all-absent input yield zero; present empty values count.
    /// Limits, refusals and corruption traps are the same as [`Self::count`].
    /// # Errors
    /// Unknown fields refuse even for empty input. Ordinary count errors apply.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn count_field(self, field: &str) -> Result<Aggregate<u64>, Error> {
        observe!(query, { self.scalar_count(Some(field)) })
    }

    /// Count distinct projected tuples, including absent values. An indexed filter can narrow the input.
    /// Retain at most 32 distinct comparison keys, each at most 4096 bytes, within
    /// the shared 128-KiB state bound. No projected records are retained. `limit`
    /// never truncates this count. The scalar result occupies eight bytes.
    /// # Errors
    /// Ordinary count/projection errors and distinct-state exhaustion refuse.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn count_distinct<K: Value>(self) -> Result<Aggregate<u64>, Error> {
        observe!(query, {
            if self.request.bytes < 8 {
                return Err(Error::RowTooLarge);
            }
            let (mut plan, reducer) = self.distinct_plan::<K>()?;
            self.select_aggregate_row_path(&mut plan)?;

            self.scalar_count_value(&plan, reducer)
        })
    }

    fn distinct_plan<K: Value>(&self) -> Result<(Plan<'static>, Reducer), Error> {
        let plan = self.aggregate_plan(Some(K::definition()), false)?;
        let reducer = Reducer::distinct(plan.projection.ok_or(Error::InvalidQuery)?);

        Ok((plan, reducer))
    }

    fn select_aggregate_row_path(&self, plan: &mut Plan<'static>) -> Result<(), Error> {
        if plan.index.is_none() {
            super::order::select_filtered_path(E::SCHEMA, &self.request, plan, false)?;
        }

        Ok(())
    }

    fn scalar_count(self, field: Option<&str>) -> Result<Aggregate<u64>, Error> {
        if self.request.bytes < 8 {
            return Err(Error::RowTooLarge);
        }
        let mut plan = self.aggregate_plan(None, false)?;
        self.select_aggregate_row_path(&mut plan)?;
        let reducer = self.count_reducer(field)?;

        self.scalar_count_value(&plan, reducer)
    }

    fn scalar_count_value(
        self,
        plan: &Plan<'_>,
        mut reducer: Reducer,
    ) -> Result<Aggregate<u64>, Error> {
        let page = execute(
            &self.db.read()?.map,
            E::SCHEMA,
            &self.request,
            plan,
            Some(Accumulator::Scalar(&mut reducer)),
        )?;

        Ok(Aggregate {
            value: reducer.finish(false)?.into_count()?,
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
    /// The engine output allowance bounds key-record bytes plus eight count bytes per group.
    /// Empty input yields no groups. At most 32 groups and 128 KiB of key/output
    /// state may be retained, independently of the requested output limit;
    /// each comparison key is capped at 4096 bytes, independently of index-key limits.
    /// # Errors
    /// Refuses unsupported key types, cursors, DISTINCT and explicit ordering.
    /// Work, group or retained-state exhaustion returns `Limit`, never partial groups.
    /// Completion must be established within `work_limit`; a boundary scan can refuse.
    /// Ordinary projection/query integrity and byte-limit errors also apply.
    /// # Panics
    /// Supplier corruption traps without repair, as for ordinary query execution.
    pub fn group_count<K: Value>(self) -> Result<Aggregate<Vec<GroupCount<K>>>, Error> {
        observe!(query, { self.group_counts(None) })
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
        observe!(query, { self.group_counts(Some(field)) })
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
        observe!(query, {
            let (plan, reducer) = self.group_distinct_plan::<K, R>()?;

            self.group_count_values(plan, reducer)
        })
    }

    fn group_distinct_plan<K: Value, R: Value>(&self) -> Result<(Plan<'static>, Reducer), Error> {
        let mut plan = self.aggregate_plan(Some(K::definition()), true)?;
        let operand = admit_projection(E::SCHEMA, R::definition())?;
        admit_ordered_fields(E::SCHEMA, operand)?;
        plan.selected |= operand;

        Ok((plan, Reducer::distinct(operand)))
    }

    fn group_counts<K: Value>(
        self,
        field: Option<&str>,
    ) -> Result<Aggregate<Vec<GroupCount<K>>>, Error> {
        let plan = self.aggregate_plan(Some(K::definition()), true)?;
        let reducer = self.count_reducer(field)?;

        self.group_count_values(plan, reducer)
    }

    fn group_count_values<K: Value>(
        self,
        mut plan: Plan<'static>,
        reducer: Reducer,
    ) -> Result<Aggregate<Vec<GroupCount<K>>>, Error> {
        self.select_aggregate_row_path(&mut plan)?;
        let (groups, page) = self.grouped(&plan, reducer, None)?;
        let (value, output_bytes) = group_output(
            groups,
            self.request.rows,
            self.request.bytes,
            decode_group_count,
        )?;

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
            .map(|name| E::SCHEMA.field_position(name).ok_or(Error::InvalidQuery))
            .transpose()?
            .filter(|&i| E::SCHEMA.fields[i].optional);

        Ok(Reducer::Count { presence, total: 0 })
    }

    fn grouped(
        &self,
        plan: &Plan<'_>,
        initial: Reducer,
        summary_sum: Option<ValueSchema>,
    ) -> Result<(Groups, QueryPage<Vec<u8>>), Error> {
        if self.request.groups.page.is_some() {
            return Err(Error::InvalidQuery);
        }
        let mut groups = self.admit_groups(plan, initial, summary_sum)?;
        let page = execute(
            &self.db.read()?.map,
            E::SCHEMA,
            &self.request,
            plan,
            Some(Accumulator::Groups(&mut groups)),
        )?;

        Ok((groups, page))
    }

    fn admit_groups(
        &self,
        plan: &Plan<'_>,
        initial: Reducer,
        summary_sum: Option<ValueSchema>,
    ) -> Result<Groups, Error> {
        if summary_sum.is_none()
            && matches!(self.request.groups.order, Some(GroupOrder::Summary(..)))
        {
            return Err(Error::InvalidQuery);
        }
        Ok(Groups {
            selection: self
                .request
                .groups
                .admit(E::SCHEMA, &initial, summary_sum)?,
            summary_sum,
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
            || ((self.request.order.is_some() || self.request.descending)
                && self.request.groups.order != Some(GroupOrder::FirstOccurrence))
        {
            return Err(Error::InvalidQuery);
        }
        let db = self.db.read()?;
        db.schema.admits(E::SCHEMA)?;
        let mut plan = Plan::admit(
            E::SCHEMA,
            &self.request,
            db.schema.fingerprint(),
            db.incarnation,
            output,
            super::PlanOutput::Aggregate {
                grouped: group_output,
            },
        )?;
        if let Some(mask) = plan.projection {
            admit_ordered_fields(E::SCHEMA, mask)?;
            plan.selected |= mask;
        }

        Ok(plan)
    }
}

fn scalar_output<R: Value>(reducer: Reducer, limit: usize) -> Result<(Option<R>, usize), Error> {
    let Some(bytes) = reducer.finish(false)?.into_value()? else {
        return Ok((None, 0));
    };
    let mut used = 0;
    admit_output(bytes.len(), limit, &mut used)?;

    Ok((Some(decode_record(&bytes)?), used))
}

fn group_output<T>(
    groups: Groups,
    rows: usize,
    bytes: usize,
    decode: fn(&[u8], Completed) -> Result<T, Error>,
) -> Result<(Vec<T>, usize), Error> {
    let mut output_bytes = 0;
    let value = groups
        .into_output(rows)?
        .into_iter()
        .map(|(key, result)| {
            admit_output(key.len() + result.encoded_len(), bytes, &mut output_bytes)?;

            decode(&key, result)
        })
        .collect::<Result<_, Error>>()?;

    Ok((value, output_bytes))
}

fn decode_group_count<K: Value>(key: &[u8], result: Completed) -> Result<GroupCount<K>, Error> {
    Ok(GroupCount {
        key: decode_record(key)?,
        count: result.into_count()?,
    })
}

fn decode_group_value<K: Value, R: Value>(
    key: &[u8],
    result: Completed,
) -> Result<GroupValue<K, R>, Error> {
    let value = result.into_value()?;

    Ok(GroupValue {
        key: decode_record(key)?,
        value: value.as_deref().map(decode_record).transpose()?,
    })
}

fn decode_record<R: Value>(bytes: &[u8]) -> Result<R, Error> {
    let mut reader = Decoder::new(bytes)?;
    let value = R::decode_from(&mut reader)?;
    reader.finish()?;

    Ok(value)
}

// Closed choices over delivered reducers and one grouping table, not user extensions.
pub(super) enum Accumulator<'a> {
    // Synchronous existence can complete on the first matching row; no job state.
    Exists(&'a mut bool),
    Scalar(&'a mut Reducer),
    Groups(&'a mut Groups),
}
impl Accumulator<'_> {
    pub(super) fn needs_spans(&self) -> bool {
        !matches!(
            self,
            Self::Exists(_)
                | Self::Scalar(
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
        scratch: &mut Vec<u8>,
    ) -> Result<(), Error> {
        match self {
            Self::Exists(found) => {
                **found = true;

                Ok(())
            }
            Self::Scalar(state) => {
                state.push(entity, values, bytes, spans, scratch, MAX_STATE_BYTES)
            }
            Self::Groups(state) => state.push(entity, values, bytes, spans, scratch),
        }
    }
}

#[derive(Clone)]
pub(super) enum Reducer {
    Keys(u8),
    Numeric(numeric::NumericAggregate),
    Distinct {
        mask: u16,
        seen: DistinctKeys,
        key_bytes: usize,
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
            seen: DistinctKeys::new(),
            key_bytes: 0,
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
            Self::Keys(_) => 0,
            Self::Numeric(state) => state.retained_bytes(),
            Self::Count { .. } => 8,
            Self::Distinct { key_bytes, .. } => 8 + key_bytes,
            Self::Extremum { selected, .. } => selected
                .as_ref()
                .map_or(0, |(key, bytes)| key.capacity() + bytes.capacity()),
        }
    }

    fn finish(self, needs_numeric_key: bool) -> Result<Completed, Error> {
        match self {
            Self::Keys(_) => Ok(Completed::Key),
            Self::Count { total, .. } => Ok(Completed::Count(total)),
            Self::Distinct { seen, .. } => Ok(Completed::Count(seen.len() as u64)),
            Self::Extremum { selected, .. } => Ok(Completed::Value(selected)),
            Self::Numeric(state) => state.finish(needs_numeric_key),
        }
    }

    fn push(
        &mut self,
        entity: &EntitySchema,
        values: &[IndexValue<'_>],
        bytes: &[u8],
        spans: &[(u16, u16)],
        scratch: &mut Vec<u8>,
        allowance: usize,
    ) -> Result<(), Error> {
        match self {
            Self::Keys(_) => {}
            Self::Numeric(state) => state.push(values)?,
            Self::Distinct {
                mask,
                seen,
                key_bytes,
            } => {
                comparison_key(entity, *mask, values, scratch)?;
                if let Err(position) =
                    seen.binary_search_by(|key| key.as_ref().cmp(scratch.as_slice()))
                {
                    let size = scratch.len();
                    if seen.len() == MAX_DISTINCT_VALUES || 8 + *key_bytes + size > allowance {
                        return Err(Error::Limit);
                    }
                    seen.insert(position, Box::from(scratch.as_slice()));
                    *key_bytes += size;
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
                scratch.clear();
                value.append_component(entity.fields[*position].optional, scratch);
                if scratch.len() > MAX_COMPARISON_KEY {
                    return Err(Error::Limit);
                }
                if selected.as_ref().is_none_or(|(old, _)| {
                    if *maximum {
                        scratch.as_slice() > old.as_slice()
                    } else {
                        scratch.as_slice() < old.as_slice()
                    }
                }) {
                    let (start, end) = spans[*position];
                    let output = &bytes[usize::from(start)..usize::from(end)];
                    if let Some((old_key, old_output)) = selected
                        && old_key.len() == scratch.len()
                        && old_output.len() == output.len()
                    {
                        // Equal-sized replacement uses the already charged buffers.
                        old_key.copy_from_slice(scratch);
                        old_output.copy_from_slice(output);
                        return Ok(());
                    }
                    if scratch.len() + output.len() > allowance {
                        return Err(Error::Limit);
                    }
                    // Copy only a winner, after admitting its replacement charge.
                    let key = scratch.as_slice().to_vec();
                    let output = output.to_vec();
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
    Summary {
        count: u64,
        sum: Option<Box<[u8]>>,
        average: Option<Box<[u8]>>,
        sort_key: Option<Box<[u8]>>,
    },
    Key,
    Count(u64),
    Value(Option<(Vec<u8>, Vec<u8>)>),
}
impl Completed {
    fn encoded_len(&self) -> usize {
        match self {
            Self::Summary { sum, average, .. } => {
                8 + sum.as_deref().map_or(0, <[u8]>::len)
                    + average.as_deref().map_or(0, <[u8]>::len)
            }
            Self::Key => 0,
            Self::Count(_) => 8,
            Self::Value(value) => value.as_ref().map_or(0, |(_, bytes)| bytes.len()),
        }
    }
    // Counts need only eight stack bytes; extrema borrow their retained ordered key.
    fn result_key<'a>(&'a self, scratch: &'a mut [u8; 8]) -> Option<&'a [u8]> {
        match self {
            Self::Summary { sort_key, .. } => sort_key.as_deref(),
            Self::Key => None,
            Self::Count(total) => {
                *scratch = total.to_be_bytes();

                Some(scratch)
            }
            Self::Value(selected) => selected.as_ref().map(|(key, _)| key.as_slice()),
        }
    }

    fn retained_bytes(&self) -> usize {
        match self {
            Self::Summary { sort_key, .. } => {
                self.encoded_len() + sort_key.as_deref().map_or(0, <[u8]>::len)
            }
            Self::Key => 0,
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
    summary_sum: Option<ValueSchema>,
    selection: Selection,
    mask: u16,
    // Never advanced: each new group receives its own empty reducer state.
    initial: Reducer,
    rows: BTreeMap<Box<[u8]>, (Vec<u8>, Reducer)>,
    state_bytes: usize,
}
impl Groups {
    fn max_keys(&self) -> usize {
        if matches!(self.initial, Reducer::Keys(_)) {
            usize::from(u8::MAX)
        } else {
            MAX_GROUPS
        }
    }

    // Ordinals are assigned densely on first insertion or while restoring the checkpoint.
    fn first_occurrences(&self) -> Result<Vec<&[u8]>, Error> {
        let mut ordered = vec![&[][..]; self.rows.len()];
        for (bytes, reducer) in self.rows.values() {
            let Reducer::Keys(position) = reducer else {
                return Err(Error::Corrupt);
            };
            ordered[usize::from(*position)] = bytes;
        }
        Ok(ordered)
    }

    fn into_output(self, limit: usize) -> Result<Vec<(Vec<u8>, Completed)>, Error> {
        // Accumulation has completed. Output filtering cannot relax input/state bounds.
        let mut state_bytes = self.state_bytes;
        let key_order = matches!(
            self.selection.order,
            GroupOrder::Key | GroupOrder::KeyDescending
        );
        let bounded_rank = !key_order && limit < self.rows.len();
        let compare = |a: &Completed, b: &Completed| {
            let (mut sa, mut sb) = ([0; 8], [0; 8]);
            let order = a.result_key(&mut sa).cmp(&b.result_key(&mut sb));
            if matches!(
                self.selection.order,
                GroupOrder::AggregateDescending
                    | GroupOrder::Summary(_, super::SortDirection::Descending)
            ) {
                order.reverse()
            } else {
                order
            }
        };
        let retain = limit.min(self.rows.len());
        let mut rows = Vec::with_capacity(retain);
        let mut oldest = 0;
        let needs_value_key = !self.selection.predicates.is_empty()
            || matches!(
                self.selection.order,
                GroupOrder::AggregateAscending | GroupOrder::AggregateDescending
            );
        for (record, reducer) in self.rows.into_values() {
            let previous = reducer.retained_bytes();
            let (result, accepted) = if let Some(sum) = self.summary_sum {
                let Reducer::Numeric(state) = reducer else {
                    return Err(Error::Corrupt);
                };
                state.finish_summary(sum, &self.selection)?
            } else {
                let result = reducer.finish(needs_value_key)?;
                let mut scratch = [0; 8];
                let key = result.result_key(&mut scratch);
                let accepted = self.selection.accepts(|_| key);
                (result, accepted)
            };
            state_bytes = state_bytes - previous + result.retained_bytes();
            if state_bytes > MAX_STATE_BYTES {
                return Err(Error::Limit);
            }
            if accepted {
                if bounded_rank {
                    // Insert after equal values to keep canonical key order across ties.
                    let position =
                        rows.partition_point(|(_, value)| !compare(value, &result).is_gt());
                    if position < retain {
                        if rows.len() == retain {
                            rows.pop();
                        }
                        rows.insert(position, (record, result));
                    }
                } else if rows.len() < retain {
                    rows.push((record, result));
                } else if self.selection.order == GroupOrder::KeyDescending {
                    // The newest qualifying key displaces the smallest retained key.
                    rows[oldest] = (record, result);
                    oldest = (oldest + 1) % retain;
                }
            }
        }
        if self.selection.order == GroupOrder::KeyDescending {
            rows.rotate_left(oldest);
            rows.reverse();
        } else if !key_order && !bounded_rank {
            // Stable sorting preserves the map's ascending key order on equal results.
            rows.sort_by(|(_, a), (_, b)| compare(a, b));
        }

        Ok(rows)
    }

    fn push(
        &mut self,
        entity: &EntitySchema,
        values: &[IndexValue<'_>],
        bytes: &[u8],
        spans: &[(u16, u16)],
        scratch: &mut Vec<u8>,
    ) -> Result<(), Error> {
        comparison_key(entity, self.mask, values, scratch)?;
        let available = MAX_STATE_BYTES - self.state_bytes;
        if let Some((_, reducer)) = self.rows.get_mut(scratch.as_slice()) {
            let previous = reducer.retained_bytes();
            reducer.push(entity, values, bytes, spans, scratch, available + previous)?;
            self.state_bytes = self.state_bytes - previous + reducer.retained_bytes();
            return Ok(());
        }

        let count = self.rows.len();
        let size = projection_size(spans, self.mask);
        let base = scratch.len() + size;
        if count == self.max_keys() || base + self.initial.retained_bytes() > available {
            return Err(Error::Limit);
        }
        // The reducer can reuse scratch as soon as the new group's key is owned.
        let key = Box::<[u8]>::from(scratch.as_slice());
        let mut reducer = self.initial.clone();
        if let Reducer::Keys(position) = &mut reducer {
            *position = count as u8;
        }
        reducer.push(entity, values, bytes, spans, scratch, available - base)?;
        let output = copy_projection(bytes, spans, self.mask, size);
        // Map/set nodes and fixed bookkeeping are independently bounded by the
        // group and per-group distinct cardinality caps.
        let retained = key.len() + output.capacity() + reducer.retained_bytes();
        if retained > available {
            return Err(Error::Limit);
        }
        self.state_bytes += retained;
        self.rows.insert(key, (output, reducer));

        Ok(())
    }
}
