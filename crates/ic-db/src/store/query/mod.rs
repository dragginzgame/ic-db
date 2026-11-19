//! Bounded fluent traversal. Typed result construction stays outside shared execution.
mod aggregate;
mod distinct;
mod order;
mod range;
mod sort;
pub use aggregate::{
    Aggregate, GroupCount, GroupOrder, GroupValue,
    jobs::{AggregateJobs, GroupCursor, GroupPage, JobHandle, JobOutcome, JobStep},
};

use super::{Database, Map, prefix, row_key, validate_index_entry};
use crate::{
    Entity, EntitySchema, Error, FieldType, IndexSchema, IndexValue, TypeBody, TypeSchema, Value,
    codec::Decoder, key, primary::RowKey, types::TypeGraph,
};
use ic_storage::Memory;
use sha2::{Digest, Sha256};
use std::marker::PhantomData;

const MAX_TERMS: usize = 8;
const MAX_WORK: usize = 512;
const MAX_OUTPUT: usize = 128 * 1024;
const MAX_COMPARISON_KEY: usize = 4096;
const MAX_DISTINCT_VALUES: usize = 32;
const MAX_STATE_BYTES: usize = 128 * 1024;

/// Direction of one field in an explicit result order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    /// Smaller values first; absence precedes present values.
    Ascending,
    /// Larger values first; absence follows present values.
    Descending,
}

/// Exact comparison. Equality distinguishes absent values; ranges and pattern matches require presence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Comparison {
    /// Equal, including two absent values.
    Eq = 0,
    /// Not equal, including absent versus present.
    Ne = 1,
    /// Present and less than the present operand.
    Lt = 2,
    /// Present and less than or equal to the present operand.
    Le = 3,
    /// Present and greater than the present operand.
    Gt = 4,
    /// Present and greater than or equal to the present operand.
    Ge = 5,
    /// Present Text or Blob starts with an exact operand of the same type.
    /// Text uses UTF-8 bytes without normalization; Blob uses raw bytes.
    /// Empty prefixes match all present values, including empty values; other types refuse.
    StartsWith = 6,
    /// Present Text or Blob ends with an exact operand of the same type.
    /// Empty suffixes match all present values; text is not normalized.
    EndsWith = 7,
    /// A present list or set contains an equal scalar member, or a present Text
    /// or Blob contains the exact byte sequence of the same type.
    /// Empty substring needles match every present Text/Blob. Text uses UTF-8 without normalization.
    Contains = 8,
}

impl Comparison {
    fn is_relational(self) -> bool {
        matches!(
            self,
            Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge
        )
    }

    fn supports_scalar(self, field: FieldType) -> bool {
        self.is_relational()
            || matches!(
                key::operand_type(field),
                FieldType::Text { .. } | FieldType::Blob { .. }
            )
    }

    fn admit(self, field: FieldType, optional: bool, value: IndexValue<'_>) -> Result<(), Error> {
        value.admit(field, optional)?;
        if !self.supports_scalar(field) {
            return Err(Error::InvalidQuery);
        }
        if value == IndexValue::Absent && !matches!(self, Self::Eq | Self::Ne) {
            return Err(Error::InvalidOperand);
        }
        Ok(())
    }

    fn accepts(self, actual: Option<&[u8]>, operand: Option<&[u8]>, optional: bool) -> bool {
        if actual.is_none() {
            return self.accepts_absent(operand.is_none());
        }
        match self {
            Self::Eq => actual == operand,
            Self::Ne => actual != operand,
            Self::StartsWith | Self::EndsWith | Self::Contains => {
                // Compare escaped content, never the presence byte or terminator.
                // In particular, optional empty text must not match suffix U+0001.
                fn content(key: &[u8], optional: bool) -> Option<&[u8]> {
                    key.get(usize::from(optional)..)?.strip_suffix(&[0, 0])
                }
                actual
                    .and_then(|key| content(key, optional))
                    .zip(operand.and_then(|key| content(key, optional)))
                    .is_some_and(|(actual, operand)| match self {
                        // Prefixes start at a component boundary: escaping each
                        // byte preserves prefix identity for Text and Blob alike.
                        Self::StartsWith => actual.starts_with(operand),
                        // A suffix starting after 00 starts inside an escaped zero
                        // (00 FF), not at a raw-byte boundary. This matters for Blob
                        // operands beginning FF; valid UTF-8 cannot begin there.
                        Self::EndsWith => actual
                            .strip_suffix(operand)
                            .is_some_and(|prefix| prefix.last() != Some(&0)),
                        _ => contains_component(actual, operand),
                    })
            }
            _ if actual.is_none() || operand.is_none() => false,
            Self::Lt => actual < operand,
            Self::Le => actual <= operand,
            Self::Gt => actual > operand,
            Self::Ge => actual >= operand,
        }
    }

    // One side is absent. Equality/inequality are symmetric; execution,
    // presence-only filters and sparse membership share this rule.
    fn accepts_absent(self, other_absent: bool) -> bool {
        match self {
            Self::Eq => other_absent,
            Self::Ne => !other_absent,
            _ => false,
        }
    }
}

// Both slices are validated component contents with presence/terminator removed.
fn contains_component(actual: &[u8], needle: &[u8]) -> bool {
    let Some(start) = memchr::memmem::find(actual, needle) else {
        return false;
    };
    if start == 0 || actual[start - 1] != 0 {
        return true;
    }
    // Only a Blob needle starting FF can match inside an escaped zero (00 FF).
    // A valid overlapping match may follow. One raw search avoids repeatedly
    // scanning long prefixes. Each temporary is bounded by the row byte limit.
    fn raw(content: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(content.len().min(crate::codec::MAX_ROW_BYTES));
        let mut encoded = content.iter();
        while let Some(&byte) = encoded.next() {
            bytes.push(byte);
            if byte == 0 {
                encoded.next();
            }
        }
        bytes
    }
    memchr::memmem::find(&raw(actual), &raw(needle)).is_some()
}

/// A bounded logical position, bound to database, schema, query and direction.
/// It is not an authorization token or a snapshot. Callers may choose positions;
/// the database revalidates all claims and retains no trusted progress counters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryCursor {
    binding: [u8; 32],
    position: Vec<u8>,
    inclusive: bool,
}
impl QueryCursor {
    pub(super) const MAX_ENCODED_BYTES: usize = 34 + crate::MAX_INDEX_KEY_BYTES;

    /// Encode the bounded continuation for an application's transport.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![1, u8::from(self.inclusive)];
        bytes.extend(self.binding);
        bytes.extend(&self.position);
        bytes
    }

    /// Decode framing; execution additionally checks query identity and key shape.
    /// # Errors
    /// Refuses unsupported versions, flags and oversized or incomplete frames.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if !(35..=Self::MAX_ENCODED_BYTES).contains(&bytes.len()) || bytes[0] != 1 || bytes[1] > 1 {
            return Err(Error::InvalidCursor);
        }
        Ok(Self {
            binding: bytes[2..34].try_into().map_err(|_| Error::InvalidCursor)?,
            position: bytes[34..].to_vec(),
            inclusive: bytes[1] == 1,
        })
    }
}

/// A bounded page. Empty rows do not mean completion when `next` is present.
/// Ordinary queries are live; maintenance source pages read frozen input.
#[derive(Debug)]
pub struct QueryPage<T> {
    /// Matching results in the selected total order.
    pub rows: Vec<T>,
    /// Resume position; only None establishes completion.
    pub next: Option<QueryCursor>,
    /// Examined primary/index candidates, including residual nonmatches and retries.
    pub examined: usize,
    /// Rows fetched; tracked separately from index candidates.
    pub fetched: usize,
    /// Sum of returned encoded entity/projected-record bytes, excluding transport overhead.
    pub output_bytes: usize,
}

impl QueryPage<Vec<u8>> {
    #[inline(never)]
    pub(super) fn decode_entities<E: Entity>(self) -> Result<QueryPage<E>, Error> {
        Ok(QueryPage {
            rows: self
                .rows
                .iter()
                .map(|bytes| E::decode(bytes))
                .collect::<Result<_, _>>()?,
            next: self.next,
            examined: self.examined,
            fetched: self.fetched,
            output_bytes: self.output_bytes,
        })
    }

    #[inline(never)]
    fn decode_projection<R: Value>(self) -> Result<QueryPage<R>, Error> {
        Ok(QueryPage {
            rows: self
                .rows
                .iter()
                .map(|bytes| {
                    let mut reader = Decoder::new(bytes)?;
                    // Output records replace the entity root, not an extra nested value.
                    let row = R::decode_from(&mut reader)?;
                    reader.finish()?;
                    Ok(row)
                })
                .collect::<Result<_, Error>>()?,
            next: self.next,
            examined: self.examined,
            fetched: self.fetched,
            output_bytes: self.output_bytes,
        })
    }
}

/// A literal, row field or length threshold in a fluent predicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryOperand<'a> {
    /// Compare with this exact typed value.
    Literal(IndexValue<'a>),
    /// Compare the field's byte length (Text/Blob) or item count (list/set/map).
    Length(u32),
    /// Compare with another top-level field on the same row.
    Field(&'a str),
}

struct Predicate<'a> {
    field: &'a str,
    comparison: Comparison,
    operand: QueryOperand<'a>,
    // Consecutive alternatives form one clause; clauses are ANDed.
    or_next: bool,
}
struct Request<'a> {
    groups: aggregate::GroupRequest<'a>,
    predicates: Vec<Predicate<'a>>,
    index: Option<(&'a str, Vec<IndexValue<'a>>)>,
    order: Option<Vec<&'a str>>,
    order_descending: u8,
    descending: bool,
    distinct: bool,
    rows: usize,
    work: usize,
    bytes: usize,
    cursor: Option<&'a QueryCursor>,
    over_limit: bool,
}

impl Default for Request<'_> {
    fn default() -> Self {
        Self {
            groups: aggregate::GroupRequest::default(),
            predicates: Vec::new(),
            index: None,
            order: None,
            order_descending: 0,
            descending: false,
            distinct: false,
            rows: 32,
            work: 128,
            bytes: MAX_OUTPUT,
            cursor: None,
            over_limit: false,
        }
    }
}

// Request construction depends on operand shapes, not the entity's Rust type.
// Keep allocation and clause loops out of every typed builder instantiation.
impl<'a> Request<'a> {
    #[inline(never)]
    fn filter(&mut self, field: &'a str, comparison: Comparison, operand: QueryOperand<'a>) {
        if self.predicates.len() == MAX_TERMS {
            self.over_limit = true;
        } else {
            self.predicates.push(Predicate {
                field,
                comparison,
                operand,
                or_next: false,
            });
        }
    }

    #[inline(never)]
    fn or_clause(
        &mut self,
        alternatives: impl ExactSizeIterator<Item = (&'a str, Comparison, QueryOperand<'a>)>,
    ) {
        let count = alternatives.len();
        if count == 0 || count > MAX_TERMS - self.predicates.len() {
            self.over_limit = true;
        } else {
            self.predicates.extend(alternatives.enumerate().map(
                |(i, (field, comparison, operand))| Predicate {
                    field,
                    comparison,
                    operand,
                    or_next: i + 1 < count,
                },
            ));
        }
    }

    #[inline(never)]
    fn filter_not_in(&mut self, field: &'a str, values: &[IndexValue<'a>]) {
        if values.is_empty() || values.len() > MAX_TERMS - self.predicates.len() {
            self.over_limit = true;
            return;
        }
        for &value in values {
            self.filter(field, Comparison::Ne, QueryOperand::Literal(value));
        }
    }

    #[inline(never)]
    fn using_index(&mut self, name: &'a str, values: &[IndexValue<'a>]) {
        if values.len() > 4 {
            self.over_limit = true;
        } else {
            self.index = Some((name, values.to_vec()));
        }
    }

    #[inline(never)]
    fn order_by(&mut self, fields: &[&'a str]) {
        self.order_descending = 0;
        if fields.len() > 4 {
            self.over_limit = true;
        } else {
            self.order = Some(fields.to_vec());
        }
    }

    #[inline(never)]
    fn order_by_each(&mut self, fields: &[(&'a str, SortDirection)]) {
        if fields.len() > 4 {
            self.over_limit = true;
            return;
        }
        self.order = Some(Vec::with_capacity(fields.len()));
        self.order_descending = 0;
        for &(field, direction) in fields {
            self.append_order(field, direction);
        }
    }

    fn append_order(&mut self, field: &'a str, direction: SortDirection) {
        let fields = self.order.get_or_insert_with(Vec::new);
        if fields.len() == 4 {
            self.over_limit = true;
            return;
        }
        if direction == SortDirection::Descending {
            self.order_descending |= 1 << fields.len();
        }
        fields.push(field);
    }

    fn primary_descending(&self, entity: &EntitySchema) -> bool {
        self.order_descending != 0
            && self.order.as_ref().is_some_and(|fields| {
                fields.last() == Some(&entity.primary_key)
                    && self.order_descending & (1 << (fields.len() - 1)) != 0
            })
    }
}

// Maintenance gets only bounded primary traversal, not a second query frontend.
pub(super) fn source_page<M: Memory>(
    map: &Map<M>,
    entity: &EntitySchema,
    binding: [u8; 32],
    incarnation: [u8; 16],
    cursor: Option<&QueryCursor>,
    rows: usize,
    bytes: usize,
) -> Result<QueryPage<Vec<u8>>, Error> {
    let request = Request {
        rows,
        work: rows,
        bytes,
        cursor,
        ..Request::default()
    };
    let plan = Plan::admit(
        entity,
        &request,
        binding,
        incarnation,
        None,
        PlanOutput::Rows,
    )?;
    execute(map, entity, &request, &plan, None)
}

/// Fluent exact predicates over one entity. No callback executor is generated.
/// Ordering uses primary order, an explicit index, a proven automatic path or
/// bounded complete sorting. Work exhaustion never returns a sorted prefix.
pub struct Query<'a, E, M: Memory> {
    db: &'a Database<M>,
    request: Request<'a>,
    entity: PhantomData<E>,
}
impl<M: Memory> Database<M> {
    /// Begin a live primary-key-ordered query. Admission occurs on `fetch`.
    #[must_use]
    pub fn query<E: Entity>(&self) -> Query<'_, E, M> {
        Query {
            db: self,
            entity: PhantomData,
            request: Request::default(),
        }
    }
}
impl<'a, E: Entity, M: Memory> Query<'a, E, M> {
    /// Remove duplicate output records. Proven leading index tuples use live
    /// pages; identity projections need no duplicate tracking. Other projections scan to
    /// completion in one call, retaining at most 32 tuples within 128 KiB.
    /// That fallback preserves first-match traversal order, refuses cursors and
    /// returns `Limit` if work, result count or combined bytes cannot fit; it
    /// never returns a partial result. An individually oversized row returns
    /// `RowTooLarge`. Comparison keys are bounded to 4096 bytes.
    /// Non-identity sorted DISTINCT retains the best `limit` distinct tuples. Its order
    /// must lead with all varying projected fields. A mandatory literal equality
    /// fixes a field. Remove duplicates before applying `limit` and output bytes.
    #[must_use]
    pub fn distinct(mut self) -> Self {
        self.request.distinct = true;
        self
    }

    /// Add an AND term over a top-level field. Lists and sets support exact scalar membership with `Contains`.
    /// Eq/Ne with `IndexValue::Absent` also accept any optional stored field,
    /// including blobs and containers. Empty values are present, not absent.
    /// All operands are admitted before storage reads, including contradictory terms.
    /// On a primary traversal, mandatory Eq/Lt/Le/Gt/Ge primary-key comparisons
    /// narrow the scanned range. On an index path, those comparisons can narrow
    /// its first field after the admitted fixed prefix. Only examined rows
    /// undergo integrity checks; oversized derived bounds remain residual filters.
    /// Indexed StartsWith can seek that field's exact encoded Text/Blob prefix too.
    #[must_use]
    pub fn filter(
        mut self,
        field: &'a str,
        comparison: Comparison,
        value: impl Into<IndexValue<'a>>,
    ) -> Self {
        self.request
            .filter(field, comparison, QueryOperand::Literal(value.into()));
        self
    }

    /// Require an exact field value. Equivalent to `filter(field, Comparison::Eq, value)`.
    #[must_use]
    pub fn eq(self, field: &'a str, value: impl Into<IndexValue<'a>>) -> Self {
        self.filter(field, Comparison::Eq, value)
    }

    /// Require a different value, including absent versus present.
    #[must_use]
    pub fn ne(self, field: &'a str, value: impl Into<IndexValue<'a>>) -> Self {
        self.filter(field, Comparison::Ne, value)
    }

    /// Require a present field strictly below the supplied value.
    #[must_use]
    pub fn lt(self, field: &'a str, value: impl Into<IndexValue<'a>>) -> Self {
        self.filter(field, Comparison::Lt, value)
    }

    /// Require a present field at or below the supplied value.
    #[must_use]
    pub fn lte(self, field: &'a str, value: impl Into<IndexValue<'a>>) -> Self {
        self.filter(field, Comparison::Le, value)
    }

    /// Require a present field strictly above the supplied value.
    #[must_use]
    pub fn gt(self, field: &'a str, value: impl Into<IndexValue<'a>>) -> Self {
        self.filter(field, Comparison::Gt, value)
    }

    /// Require a present field at or above the supplied value.
    #[must_use]
    pub fn gte(self, field: &'a str, value: impl Into<IndexValue<'a>>) -> Self {
        self.filter(field, Comparison::Ge, value)
    }

    /// Compare two top-level fields on the same row. Lists and sets support Contains
    /// with a present scalar field of the element type. Text and Blob also
    /// support StartsWith, EndsWith and Contains with a present right-hand field.
    /// Types must match; Text and Blob bounds may differ within their respective families.
    /// Two absent values are equal. An ordered comparison requires two present values.
    #[must_use]
    pub fn filter_fields(mut self, left: &'a str, comparison: Comparison, right: &'a str) -> Self {
        self.request
            .filter(left, comparison, QueryOperand::Field(right));
        self
    }

    /// Compare UTF-8 bytes for Text, raw bytes for Blob, or the item count for a
    /// list, set or map. Accepts Eq, Ne, Lt, Le, Gt and Ge. An absent field
    /// matches Ne only; use Gt(0) for a present, nonempty value.
    #[must_use]
    pub fn filter_length(mut self, field: &'a str, comparison: Comparison, length: u32) -> Self {
        self.request
            .filter(field, comparison, QueryOperand::Length(length));
        self
    }

    /// Match rows whose optional field has a value, including an empty value.
    /// Required and unknown fields return `InvalidQuery` at admission.
    #[must_use]
    pub fn filter_present(self, field: &'a str) -> Self {
        self.filter(field, Comparison::Ne, IndexValue::Absent)
    }

    /// Match rows whose optional field has no value.
    /// Required and unknown fields return `InvalidQuery` at admission.
    #[must_use]
    pub fn filter_absent(self, field: &'a str) -> Self {
        self.filter(field, Comparison::Eq, IndexValue::Absent)
    }

    /// Match a present field inside an inclusive range, ANDed with other filters.
    /// Equivalent to `filter(field, Comparison::Ge, lower)` followed by
    /// `filter(field, Comparison::Le, upper)`, including cursor/job identity.
    /// Both exact operands are admitted and consume two of the eight comparisons.
    /// Absent operands refuse; absent fields never match. Reversed bounds produce
    /// no matches, without skipping admission of either operand or other filters.
    #[must_use]
    pub fn filter_between(
        self,
        field: &'a str,
        lower: impl Into<IndexValue<'a>>,
        upper: impl Into<IndexValue<'a>>,
    ) -> Self {
        self.filter(field, Comparison::Ge, lower)
            .filter(field, Comparison::Le, upper)
    }

    /// Match a present field strictly outside an inclusive range.
    /// Equivalent to one `filter_any` clause containing `Lt lower` then `Gt upper`,
    /// including cursor/job identity. Both exact operands consume filter terms.
    /// Absent operands refuse and absent fields never match; this is not a general
    /// negation of `filter_between`. Equal bounds exclude that value; reversed
    /// bounds match every present value, once. Uses the existing single traversal
    /// and work budget, without seeking separate ranges for the two alternatives.
    #[must_use]
    pub fn filter_not_between(
        self,
        field: &'a str,
        lower: impl Into<IndexValue<'a>>,
        upper: impl Into<IndexValue<'a>>,
    ) -> Self {
        self.filter_any(&[
            (field, Comparison::Lt, lower.into()),
            (field, Comparison::Gt, upper.into()),
        ])
    }

    /// Match `lower <= field && field <= upper` using three top-level fields of the row.
    /// Both comparisons require present values of compatible types. Equal endpoints
    /// are inclusive. Uses the same two comparison terms and continuation as the
    /// equivalent chained `filter_fields` calls.
    #[must_use]
    pub fn filter_between_fields(self, field: &'a str, lower: &'a str, upper: &'a str) -> Self {
        self.filter_fields(field, Comparison::Ge, lower)
            .filter_fields(field, Comparison::Le, upper)
    }

    /// Match `field < lower || field > upper` using fields of the same row.
    /// A missing bound makes only its own comparison false. A missing tested field
    /// never matches. Uses one OR clause and the same two comparison terms and
    /// continuation as the equivalent `filter_any_operands` call.
    #[must_use]
    pub fn filter_not_between_fields(self, field: &'a str, lower: &'a str, upper: &'a str) -> Self {
        self.filter_any_operands(&[
            (field, Comparison::Lt, QueryOperand::Field(lower)),
            (field, Comparison::Gt, QueryOperand::Field(upper)),
        ])
    }

    /// Add an OR clause of exact top-level comparisons, ANDed with other filters.
    /// Each clause must contain at least one comparison; all filters together
    /// may contain at most eight. Empty/oversized clauses refuse with `Error::Limit`
    /// at execution. Every operand is admitted, even if another alternative matches.
    /// Uses one ordered traversal, without union scans or duplicate rows. Alternatives
    /// do not establish fixed index prefixes; a one-term clause behaves like `filter`.
    #[must_use]
    pub fn filter_any(mut self, alternatives: &[(&'a str, Comparison, IndexValue<'a>)]) -> Self {
        self.request.or_clause(
            alternatives
                .iter()
                .map(|&(field, op, value)| (field, op, QueryOperand::Literal(value))),
        );
        self
    }

    /// Add one OR clause of literal, field-to-field or length comparisons.
    /// Each alternative follows its corresponding single-filter admission rules.
    /// All alternatives share the eight-comparison budget with the other filters.
    /// Literal-only clauses have the same cursor and job identity as `filter_any`.
    #[must_use]
    pub fn filter_any_operands(
        mut self,
        alternatives: &[(&'a str, Comparison, QueryOperand<'a>)],
    ) -> Self {
        self.request.or_clause(alternatives.iter().copied());
        self
    }

    /// Match a field against any supplied exact value, ANDed with other filters.
    /// Equivalent to `filter_any` with equality comparisons in the supplied order.
    /// Absence matches only an explicit `IndexValue::Absent` member on an optional field.
    /// Repeated members never repeat rows, but each counts toward the shared
    /// eight-comparison limit. Empty or oversized lists refuse with `Error::Limit`.
    /// Every member is admitted before reading; there is no coercion, sorting or
    /// deduplication of operands. Reordering members changes cursor/job identity.
    #[must_use]
    pub fn filter_in(mut self, field: &'a str, values: &[IndexValue<'a>]) -> Self {
        self.request.or_clause(
            values
                .iter()
                .copied()
                .map(|value| (field, Comparison::Eq, QueryOperand::Literal(value))),
        );
        self
    }

    /// Exclude every supplied exact value, ANDed with other filters.
    /// Equivalent to consecutive `filter(field, Comparison::Ne, value)` calls
    /// in the supplied order. Absent fields match unless absence is explicitly
    /// excluded. Empty lists refuse; all members, including duplicates, count
    /// toward the shared eight-comparison budget. Every operand is admitted.
    /// The equivalent inequality chain shares cursor/job identity; reordering
    /// the list changes that identity. No coercion or deduplication is performed.
    #[must_use]
    pub fn filter_not_in(mut self, field: &'a str, values: &[IndexValue<'a>]) -> Self {
        self.request.filter_not_in(field, values);
        self
    }

    /// Select index order and exact leading components (at most four).
    /// All rows matching this prefix are considered. Sparse indexes require
    /// every optional indexed component to be proven present by the prefix or
    /// filters. Every alternative in a proving OR clause must reject absence.
    #[must_use]
    pub fn using_index(mut self, name: &'a str, values: &[IndexValue<'a>]) -> Self {
        self.request.using_index(name, values);
        self
    }

    /// Append an ascending sort field. Earlier fields take precedence.
    /// The same field, work and continuation rules as [`Self::order_by_each`] apply.
    #[must_use]
    pub fn order_asc(mut self, field: &'a str) -> Self {
        self.request.append_order(field, SortDirection::Ascending);
        self
    }

    /// Append a descending sort field. An implicit primary-key tie-breaker
    /// stays ascending; append the primary key explicitly to choose its direction.
    #[must_use]
    pub fn order_desc(mut self, field: &'a str) -> Self {
        self.request.append_order(field, SortDirection::Descending);
        self
    }

    /// Replace the current order with 1..=4 distinct top-level ordered fields, then the primary key.
    /// An explicitly listed primary key must be last. All fields share one
    /// direction, reversed by `descending`. Prefer a complete ordered path.
    /// Otherwise scan to completion while retaining the best `limit` rows within
    /// 128 KiB of key/output buffers, then sort them. Non-identity DISTINCT retains
    /// the best `limit` distinct tuples. Work exhaustion refuses; no
    /// cursor is supported by this fallback. DISTINCT requires an identity result
    /// or an order led by all varying projected fields. Equality terms may fix
    /// leading index components. With `using_index`, require that path's order.
    #[must_use]
    pub fn order_by(mut self, fields: &[&'a str]) -> Self {
        self.request.order_by(fields);
        self
    }

    /// Replace the order with 1..=4 fields and their individual directions.
    /// An explicit primary key must be last; otherwise append it ascending.
    /// `descending()` reverses every direction, including that tie-breaker.
    /// The same path proof and complete-sort bounds as `order_by` apply.
    #[must_use]
    pub fn order_by_each(mut self, fields: &[(&'a str, SortDirection)]) -> Self {
        self.request.order_by_each(fields);
        self
    }

    /// Reverse the complete result order, including its unique tie-breaker.
    #[must_use]
    pub fn descending(mut self) -> Self {
        self.request.descending = true;
        self
    }

    /// Return at most 1..=32 matching rows in this page.
    /// Complete-collection DISTINCT refuses if the full result exceeds this cap.
    /// Complete sorting applies this cap after examining the entire input; it
    /// never relaxes the separate 32-matching-row retention bound.
    /// For grouped aggregates, cap the complete selected result instead; use
    /// `group_page` to set a retained grouped response's page size separately.
    #[must_use]
    pub fn limit(mut self, rows: usize) -> Self {
        self.request.rows = rows;
        self
    }

    /// Examine at most 1..=512 candidates, independently of the returned row limit.
    #[must_use]
    pub fn work_limit(mut self, candidates: usize) -> Self {
        self.request.work = candidates;
        self
    }

    /// Exercise output exhaustion with a small response in qualification fixtures.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    #[must_use]
    pub fn test_output_limit(mut self, bytes: usize) -> Self {
        self.request.bytes = bytes;
        self
    }

    /// Continue this query; page resource limits may change between calls.
    #[must_use]
    pub fn after(mut self, cursor: &'a QueryCursor) -> Self {
        self.request.cursor = Some(cursor);
        self
    }

    /// Admit and execute one page, then decode only its matching output rows.
    /// # Errors
    /// Refuses unsupported shapes, inexact operands, limits, invalid cursors and corruption.
    /// A selected output row larger than the whole byte allowance returns `RowTooLarge`.
    /// # Panics
    /// Supplier corruption traps without repair, as for direct record reads.
    pub fn fetch(self) -> Result<QueryPage<E>, Error> {
        fetch_rows(self.db, E::SCHEMA, self.request, None)?.decode_entities()
    }

    /// Return the first matching entity in query order, or `None` on a proven miss.
    /// Uses a one-row page. For partial pages with resumable progress use `fetch`.
    /// # Errors
    /// Returns the same admission and storage errors as `fetch`. Returns `Limit`
    /// if the work budget ends before a match or exhaustion is established.
    pub fn first(self) -> Result<Option<E>, Error> {
        let mut page = self.probe_matches(1)?;
        match page.rows.pop() {
            None if page.next.is_some() => Err(Error::Limit),
            result => Ok(result),
        }
    }

    /// Return the only match, or `None` when the query is exhausted without one.
    /// A valid row limit is overridden to probe for two matches. Applies to the
    /// whole query, so a continuation is refused.
    /// # Errors
    /// Returns `MultipleMatches` on a second match and `Limit` if the page is
    /// incomplete before cardinality is known. Ordinary fetch errors apply.
    pub fn try_one(self) -> Result<Option<E>, Error> {
        if self.request.cursor.is_some() {
            return Err(Error::InvalidQuery);
        }
        let mut page = self.probe_matches(2)?;
        if page.rows.len() == 2 {
            return Err(Error::MultipleMatches);
        }
        if page.next.is_some() {
            return Err(Error::Limit);
        }
        Ok(page.rows.pop())
    }

    fn probe_matches(self, rows: usize) -> Result<QueryPage<E>, Error> {
        if !(1..=32).contains(&self.request.rows) {
            return Err(Error::Limit);
        }
        self.limit(rows).fetch()
    }

    /// Project matching top-level fields into an existing `#[record]` type.
    /// Names, optionality and complete value definitions must match exactly.
    /// The record is an output shape, not a new stored entity or schema entry.
    /// Byte limits count projected bytes; all source fields still undergo validation.
    /// # Errors
    /// Refuses non-record/empty or incompatible output shapes, and the same failures
    /// as `fetch`. A continuation must use the same selected field shape.
    /// # Panics
    /// Supplier corruption traps without repair, as for direct record reads.
    pub fn fetch_as<R: Value>(self) -> Result<QueryPage<R>, Error> {
        let raw = fetch_rows(self.db, E::SCHEMA, self.request, Some(R::definition()))?;
        raw.decode_projection()
    }
}

// Reuse named-record metadata/codecs rather than a second value tree or a
// generic executor per output shape. The full transitive graph matters: named
// links compare by identity, which alone cannot establish codec compatibility.
fn admit_projection(entity: &EntitySchema, output: &TypeSchema) -> Result<u16, Error> {
    let TypeBody::Record(fields) = &output.body else {
        return Err(Error::InvalidQuery);
    };
    if fields.is_empty()
        || fields.len() > crate::schema::MAX_FIELDS
        || fields.windows(2).any(|w| w[0].name >= w[1].name)
    {
        return Err(Error::InvalidQuery);
    }
    let mut source = TypeGraph::default();
    let mut target = TypeGraph::default();
    let mut mask = 0;
    for field in *fields {
        let position = entity
            .fields
            .iter()
            .position(|f| f.name == field.name)
            .ok_or(Error::InvalidQuery)?;
        let stored = &entity.fields[position];
        if stored.value != field.value || stored.optional != field.optional {
            return Err(Error::InvalidQuery);
        }
        source
            .include(stored.value)
            .map_err(|_| Error::InvalidQuery)?;
        target
            .include(field.value)
            .map_err(|_| Error::InvalidQuery)?;
        mask |= 1 << position;
    }
    if !source.contains(&target) {
        return Err(Error::InvalidQuery);
    }
    Ok(mask)
}

struct Term {
    position: usize,
    comparison: Comparison,
    operand: TermOperand,
    or_next: bool,
}

enum TermOperand {
    Literal {
        bytes: Vec<u8>,
        absent: bool,
    },
    Member {
        item: FieldType,
        needle: MemberNeedle,
    },
    Field(usize),
    Length(u32),
}

enum MemberNeedle {
    Literal(Vec<u8>),
    Field(usize),
}

fn member_type(field: FieldType) -> Option<FieldType> {
    match key::operand_type(field) {
        FieldType::List { item, .. } => Some(*item),
        FieldType::Named(link) => match (link.definition)().body {
            TypeBody::Set { item, .. } => Some(item),
            _ => None,
        },
        _ => None,
    }
}

// inspect_fields has already validated every item, including the tail after a match.
fn contains_member(
    span: &[u8],
    optional: bool,
    item: FieldType,
    needle: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<bool, Error> {
    let mut reader = Decoder::new(span)?;
    if optional && !reader.boolean()? {
        return Ok(false);
    }
    for _ in 0..reader.nat32()? {
        scratch.clear();
        IndexValue::read(&mut reader, item)?.append_component(false, scratch);
        if scratch.as_slice() == needle {
            return Ok(true);
        }
    }
    Ok(false)
}

fn admit_ordered_fields(entity: &EntitySchema, mask: u16) -> Result<(), Error> {
    for (i, field) in entity.fields.iter().enumerate() {
        if mask & (1 << i) != 0 && !key::supports(field.value) {
            return Err(Error::InvalidQuery);
        }
    }
    Ok(())
}

// Grouping, distinct counts and collected DISTINCT share exact tuple equality.
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

// Size and copy already validated projection spans using the same field mask.
fn projection_size(spans: &[(u16, u16)], mask: u16) -> usize {
    spans
        .iter()
        .enumerate()
        .filter(|(i, _)| mask & (1 << i) != 0)
        .map(|(_, (start, end))| usize::from(end - start))
        .sum()
}

fn copy_projection(bytes: &[u8], spans: &[(u16, u16)], mask: u16, size: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(size);
    for (i, &(start, end)) in spans.iter().enumerate() {
        if mask & (1 << i) != 0 {
            output.extend_from_slice(&bytes[usize::from(start)..usize::from(end)]);
        }
    }
    output
}
#[derive(Clone, Copy)]
enum PlanOutput {
    Rows,
    Aggregate { grouped: bool },
}

struct Plan<'a> {
    primary: usize,
    index: Option<&'a IndexSchema>,
    prefix: Vec<u8>,
    fixed_components: usize,
    terms: Vec<Term>,
    selected: u16,
    // Non-orderable fields need only their validated optional tags, never key slots.
    span_fields: u16,
    binding: [u8; 32],
    projection: Option<u16>,
    distinct: distinct::Strategy,
    sort: Option<Vec<(usize, bool)>>,
    descending: bool,
}
impl<'a> Plan<'a> {
    fn use_index(&mut self, entity: &EntitySchema, path: order::IndexPath<'a>) {
        self.index = Some(path.index);
        self.prefix = path.prefix;
        self.fixed_components = path.fixed;
        self.selected |= entity.key_fields();
    }

    fn admit(
        entity: &'a EntitySchema,
        request: &Request<'_>,
        schema: [u8; 32],
        incarnation: [u8; 16],
        projection: Option<&TypeSchema>,
        output: PlanOutput,
    ) -> Result<Self, Error> {
        if request.over_limit
            || !(1..=32).contains(&request.rows)
            || !(1..=MAX_WORK).contains(&request.work)
            || !(1..=MAX_OUTPUT).contains(&request.bytes)
        {
            return Err(Error::Limit);
        }
        if !matches!(output, PlanOutput::Aggregate { grouped: true }) && !request.groups.is_empty()
        {
            return Err(Error::InvalidQuery);
        }
        // Bound literal encoding even when a declaration's maximum exceeds one row.
        for value in request
            .predicates
            .iter()
            .filter_map(|p| match &p.operand {
                QueryOperand::Literal(value) => Some(value),
                QueryOperand::Field(_) | QueryOperand::Length(_) => None,
            })
            .chain(request.index.iter().flat_map(|(_, values)| values.iter()))
        {
            if match value {
                IndexValue::Text(text) => text.len() > crate::codec::MAX_ROW_BYTES,
                IndexValue::Blob(bytes) => bytes.len() > crate::codec::MAX_ROW_BYTES,
                _ => false,
            } {
                return Err(Error::Limit);
            }
        }
        let primary = entity
            .fields
            .iter()
            .position(|f| f.name == entity.primary_key && !f.optional)
            .ok_or(Error::InvalidQuery)?;
        let mut plan = Self {
            primary,
            index: None,
            prefix: prefix(2, entity.id),
            fixed_components: 0,
            terms: Vec::new(),
            selected: 1 << primary,
            span_fields: 0,
            binding: [0; 32],
            projection: projection
                .map(|output| admit_projection(entity, output))
                .transpose()?,
            distinct: distinct::Strategy::Identity,
            sort: None,
            descending: request.descending ^ request.primary_descending(entity),
        };
        for predicate in &request.predicates {
            let position = entity
                .fields
                .iter()
                .position(|f| f.name == predicate.field)
                .ok_or(Error::InvalidQuery)?;
            let field = &entity.fields[position];
            let member = if predicate.comparison == Comparison::Contains {
                member_type(field.value)
            } else {
                None
            };
            let operand = match predicate.operand {
                QueryOperand::Length(length) => {
                    let supported = match key::operand_type(field.value) {
                        FieldType::Text { .. }
                        | FieldType::Blob { .. }
                        | FieldType::List { .. } => true,
                        FieldType::Named(link) => matches!(
                            (link.definition)().body,
                            TypeBody::Map { .. } | TypeBody::Set { .. }
                        ),
                        _ => false,
                    };
                    if !supported || !predicate.comparison.is_relational() {
                        return Err(Error::InvalidQuery);
                    }
                    plan.span_fields |= 1 << position;
                    TermOperand::Length(length)
                }
                QueryOperand::Field(name) => {
                    let right = entity
                        .fields
                        .iter()
                        .position(|f| f.name == name)
                        .ok_or(Error::InvalidQuery)?;
                    let right_type = entity.fields[right].value;
                    let left_type = member.unwrap_or(field.value);
                    let compatible = left_type == right_type
                        || matches!(
                            (left_type, right_type),
                            (FieldType::Text { .. }, FieldType::Text { .. })
                                | (FieldType::Blob { .. }, FieldType::Blob { .. })
                        );
                    if !compatible
                        || !key::supports(left_type)
                        || (member.is_none() && !predicate.comparison.supports_scalar(left_type))
                    {
                        return Err(Error::InvalidQuery);
                    }
                    plan.selected |= 1 << right;
                    if let Some(item) = member {
                        plan.span_fields |= 1 << position;
                        TermOperand::Member {
                            item,
                            needle: MemberNeedle::Field(right),
                        }
                    } else {
                        plan.selected |= 1 << position;
                        TermOperand::Field(right)
                    }
                }
                QueryOperand::Literal(value) => {
                    if let Some(item) = member {
                        if !key::supports(item) {
                            return Err(Error::InvalidQuery);
                        }
                        value.admit(item, false)?;
                        let mut bytes = Vec::new();
                        value.append_component(false, &mut bytes);
                        plan.span_fields |= 1 << position;
                        TermOperand::Member {
                            item,
                            needle: MemberNeedle::Literal(bytes),
                        }
                    } else {
                        let absent = value == IndexValue::Absent;
                        if key::supports(field.value) {
                            plan.selected |= 1 << position;
                        } else if absent
                            && matches!(predicate.comparison, Comparison::Eq | Comparison::Ne)
                        {
                            plan.span_fields |= 1 << position;
                        } else {
                            return Err(Error::InvalidQuery);
                        }
                        predicate
                            .comparison
                            .admit(field.value, field.optional, value)?;
                        let mut bytes = Vec::new();
                        value.append_component(field.optional, &mut bytes);
                        TermOperand::Literal { bytes, absent }
                    }
                }
            };
            plan.terms.push(Term {
                position,
                comparison: predicate.comparison,
                operand,
                or_next: predicate.or_next,
            });
        }
        // Admit every operand before selecting a path, even for empty or
        // contradictory queries. The scanner consumes this one proven choice.
        match order::select(entity, request, output)? {
            order::Path::Primary => {}
            order::Path::Index(path) => plan.use_index(entity, path),
            order::Path::Sort(fields) => {
                for &(position, _) in &fields {
                    plan.selected |= 1 << position;
                }
                plan.sort = Some(fields);
            }
        }
        if request.distinct {
            plan.distinct = distinct::admit(
                entity,
                request,
                plan.projection,
                plan.index,
                plan.fixed_components,
                plan.sort.is_some(),
            )?;
            if plan.distinct == distinct::Strategy::Complete {
                plan.selected |= plan.projection.ok_or(Error::InvalidQuery)?;
            }
        }
        let mut hash = Sha256::new();
        hash.update(b"ic-db:query:v1\0");
        hash.update(schema);
        hash.update(incarnation);
        hash.update([u8::from(request.descending)]);
        hash.update((plan.prefix.len() as u32).to_le_bytes());
        hash.update(&plan.prefix);
        for term in &plan.terms {
            hash.update([term.position as u8, term.comparison as u8]);
            match &term.operand {
                TermOperand::Literal { bytes, .. }
                | TermOperand::Member {
                    needle: MemberNeedle::Literal(bytes),
                    ..
                } => {
                    hash.update((bytes.len() as u32).to_le_bytes());
                    hash.update(bytes);
                }
                TermOperand::Length(length) => {
                    hash.update((u32::MAX - 1).to_le_bytes());
                    hash.update(length.to_le_bytes());
                }
                TermOperand::Field(right)
                | TermOperand::Member {
                    needle: MemberNeedle::Field(right),
                    ..
                } => {
                    // Outside the bounded literal length domain.
                    hash.update(u32::MAX.to_le_bytes());
                    hash.update([*right as u8]);
                }
            }
        }
        // Keep existing conjunction identities unchanged. A bounded clause mask
        // distinguishes OR grouping without persisting another predicate model.
        let clauses = plan
            .terms
            .iter()
            .enumerate()
            .fold(0_u8, |mask, (i, term)| mask | (u8::from(term.or_next) << i));
        if clauses != 0 {
            hash.update(b"or-clauses\0");
            hash.update([clauses]);
        }
        if let Some(order) = &request.order {
            hash.update(b"order\0");
            hash.update([order.len() as u8]);
            for name in order {
                // Field names were admitted by order selection above.
                hash.update([entity
                    .fields
                    .iter()
                    .position(|f| f.name == *name)
                    .ok_or(Error::InvalidQuery)? as u8]);
            }
        }
        if request.order_descending != 0 {
            hash.update(b"order-directions\0");
            hash.update([request.order_descending]);
        }
        // Preserve existing entity-result cursors. Projected shapes add a
        // domain-separated suffix; output Rust names do not change semantics.
        if let Some(mask) = plan.projection {
            hash.update(b"projection\0");
            hash.update(mask.to_le_bytes());
        }
        if request.distinct {
            hash.update(b"distinct\0");
        }
        plan.binding = hash.finalize().into();
        if let Some(cursor) = request.cursor {
            if cursor.binding != plan.binding {
                return Err(Error::InvalidCursor);
            }
            plan.validate_position(entity, &cursor.position)
                .map_err(|_| Error::InvalidCursor)?;
        }
        Ok(plan)
    }

    fn validate_position(&self, entity: &EntitySchema, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() > crate::MAX_INDEX_KEY_BYTES || !bytes.starts_with(&self.prefix) {
            return Err(Error::InvalidCursor);
        }
        if let Some(index) = self.index {
            let mut remaining = bytes.get(9..).ok_or(Error::InvalidCursor)?;
            for name in index.fields {
                consume_index_component(entity, index, name, &mut remaining)?;
            }
            if !index.unique {
                key::consume(&mut remaining, entity.key_type()?)?;
            }
            if !remaining.is_empty() {
                return Err(Error::InvalidCursor);
            }
        } else {
            RowKey::decode(
                bytes.get(5..).ok_or(Error::InvalidCursor)?,
                entity.key_type()?,
            )?;
        }
        Ok(())
    }

    fn cursor(&self, position: &[u8], inclusive: bool) -> QueryCursor {
        QueryCursor {
            binding: self.binding,
            position: position.to_vec(),
            inclusive,
        }
    }
}

// Cursor framing and DISTINCT equivalence boundaries use the same key decoder.
fn consume_index_component(
    entity: &EntitySchema,
    index: &IndexSchema,
    name: &str,
    remaining: &mut &[u8],
) -> Result<(), Error> {
    let field = entity
        .fields
        .iter()
        .find(|f| f.name == name)
        .ok_or(Error::InvalidQuery)?;
    if field.optional {
        let (&presence, rest) = remaining.split_first().ok_or(Error::InvalidCursor)?;
        *remaining = rest;
        match presence {
            0 if !index.present_only => return Ok(()),
            1 => {}
            _ => return Err(Error::InvalidCursor),
        }
    }
    key::consume(remaining, field.value)
}

// Admission, execution and request cleanup share one entity-independent boundary.
// Only conversion of the returned bytes remains in the typed public terminals.
#[inline(never)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "own the request so predicate cleanup stays outside entity-specialized terminals"
)]
fn fetch_rows<M: Memory>(
    db: &Database<M>,
    entity: &EntitySchema,
    request: Request<'_>,
    projection: Option<&TypeSchema>,
) -> Result<QueryPage<Vec<u8>>, Error> {
    db.schema.admits(entity)?;
    let plan = Plan::admit(
        entity,
        &request,
        db.schema.fingerprint(),
        db.incarnation,
        projection,
        PlanOutput::Rows,
    )?;
    execute(&db.map, entity, &request, &plan, None)
}

fn execute<M: Memory>(
    map: &Map<M>,
    entity: &EntitySchema,
    request: &Request<'_>,
    plan: &Plan<'_>,
    mut aggregate: Option<aggregate::Accumulator<'_>>,
) -> Result<QueryPage<Vec<u8>>, Error> {
    execute_inner(map, entity, request, plan, aggregate.as_mut(), None)
}

// Retained aggregates use the same traversal, but checkpoint at a work boundary.
// Reserve a maximum candidate before the next fetch, including index/owner keys.
const STEP_INPUT_BYTES: usize = 64 * 1024;
const CANDIDATE_BYTES: usize = crate::codec::MAX_ROW_BYTES + 2 * crate::MAX_INDEX_KEY_BYTES;

fn execute_inner<M: Memory>(
    map: &Map<M>,
    entity: &EntitySchema,
    request: &Request<'_>,
    plan: &Plan<'_>,
    mut aggregate: Option<&mut aggregate::Accumulator<'_>>,
    mut input_bytes: Option<&mut usize>,
) -> Result<QueryPage<Vec<u8>>, Error> {
    let mut page = QueryPage {
        rows: Vec::new(),
        next: None,
        examined: 0,
        fetched: 0,
        output_bytes: 0,
    };
    let Some(bounds) = range::bounds(entity, request, plan)? else {
        return Ok(page);
    };
    let terminal = range::terminal_key(&bounds, plan.descending);
    let mut entries = map.range((bounds.0.as_ref(), bounds.1.as_ref()));
    let mut collected = if plan.distinct == distinct::Strategy::Complete {
        Some(distinct::Collection::new(
            plan.projection.ok_or(Error::InvalidQuery)?,
        ))
    } else {
        None
    };
    let mut sorted = plan
        .sort
        .as_ref()
        .map(|_| sort::Collection::new(request, plan.distinct));
    let mut component = Vec::new();
    let mut right_component = Vec::new();
    // Each processed equivalence class is either wholly rejected or already
    // emitted. Inclusive byte retries point at an unreturned class instead.
    let mut previous = if let Some(cursor) = request.cursor.filter(|c| !c.inclusive) {
        distinct::boundary(entity, plan.index, plan.distinct, &cursor.position)?.map(<[u8]>::to_vec)
    } else {
        None
    };
    let primary_type = entity.fields[plan.primary].value;
    while let Some(entry) = if plan.descending {
        entries.next_back()
    } else {
        entries.next()
    } {
        // Retained jobs may have consumed their lifetime candidate allowance.
        // An empty range can still establish completion; refuse a next candidate
        // before reading its value or touching the accumulator.
        if request.work == 0 {
            return Err(Error::Limit);
        }
        page.examined += 1;
        let stored_key = entry.key();
        let mut read_bytes = stored_key.len();
        let (owner, bytes) = if plan.index.is_some() {
            let owner_bytes = entry.value();
            read_bytes += owner_bytes.len();
            let owner = RowKey::decode(&owner_bytes, primary_type)?;
            let bytes = map.get(&row_key(entity.id, owner)).ok_or(Error::Corrupt)?;
            (owner, bytes)
        } else {
            (
                RowKey::decode(&stored_key[5..], primary_type)?,
                entry.value(),
            )
        };
        if let Some(used) = input_bytes.as_deref_mut() {
            read_bytes += bytes.len();
            if read_bytes > CANDIDATE_BYTES {
                return Err(Error::Corrupt);
            }
            *used += read_bytes;
        }
        page.fetched += 1;
        let mut values = [IndexValue::Absent; crate::schema::MAX_FIELDS];
        let mut spans = [(0_u16, 0_u16); crate::schema::MAX_FIELDS];
        entity.inspect_fields(
            &bytes,
            plan.selected,
            &mut values,
            (plan.span_fields != 0
                || plan.projection.is_some()
                || aggregate.as_ref().is_some_and(|a| a.needs_spans()))
            .then_some(&mut spans),
        )?;
        if RowKey::from_index(values[plan.primary])? != owner {
            return Err(Error::Corrupt);
        }
        if plan.index.is_some() {
            validate_index_entry(entity, &entity.row_info(&values)?, owner, stored_key)?;
        }
        let mut accepts = |term: &Term| -> Result<bool, Error> {
            let value = values[term.position];
            component.clear();
            let (operand, optional) = match &term.operand {
                TermOperand::Literal { absent: true, .. }
                    if plan.span_fields & (1 << term.position) != 0 =>
                {
                    let absent = bytes[usize::from(spans[term.position].0)] == 0;
                    return Ok(term.comparison.accepts_absent(absent));
                }
                TermOperand::Member { item, needle } => {
                    let needle = match needle {
                        MemberNeedle::Literal(bytes) => bytes.as_slice(),
                        MemberNeedle::Field(right) => {
                            let value = values[*right];
                            if value == IndexValue::Absent {
                                return Ok(false);
                            }
                            right_component.clear();
                            value.append_component(false, &mut right_component);
                            &right_component
                        }
                    };
                    let (start, end) = spans[term.position];
                    return contains_member(
                        &bytes[usize::from(start)..usize::from(end)],
                        entity.fields[term.position].optional,
                        *item,
                        needle,
                        &mut component,
                    );
                }
                TermOperand::Length(length) => {
                    let (start, end) = spans[term.position];
                    let span = &bytes[usize::from(start)..usize::from(end)];
                    let actual =
                        crate::codec::validated_length(span, entity.fields[term.position].optional)
                            .map(u32::to_be_bytes);
                    return Ok(term.comparison.accepts(
                        actual.as_ref().map(|v| v.as_slice()),
                        Some(&length.to_be_bytes()),
                        false,
                    ));
                }
                TermOperand::Literal { bytes, absent } => (
                    (!absent).then_some(bytes.as_slice()),
                    entity.fields[term.position].optional,
                ),
                TermOperand::Field(right) => {
                    let rhs = values[*right];
                    right_component.clear();
                    rhs.append_component(false, &mut right_component);
                    (
                        (rhs != IndexValue::Absent).then_some(right_component.as_slice()),
                        false,
                    )
                }
            };
            value.append_component(optional, &mut component);
            Ok(term.comparison.accepts(
                (value != IndexValue::Absent).then_some(component.as_slice()),
                operand,
                optional,
            ))
        };
        let matches = 'matches: {
            'clause: for clause in plan.terms.split_inclusive(|term| !term.or_next) {
                for term in clause {
                    if accepts(term)? {
                        continue 'clause;
                    }
                }
                break 'matches false;
            }
            true
        };
        let boundary = distinct::boundary(entity, plan.index, plan.distinct, stored_key)?;
        let duplicate = boundary.is_some_and(|key| previous.as_deref() == Some(key));
        if matches && let Some(accumulator) = aggregate.as_mut() {
            accumulator.push(entity, &values, &bytes, &spans)?;
            if matches!(accumulator, aggregate::Accumulator::Exists(_)) {
                // One validated match proves existence, even at the work boundary.
                return Ok(page);
            }
        } else if matches && let Some(collection) = collected.as_mut() {
            collection.push(entity, request, &values, &bytes, &spans)?;
        } else if matches && let Some(collection) = sorted.as_mut() {
            collection.push(entity, plan, &values, &bytes, &spans)?;
        } else if matches && !duplicate {
            let size = plan
                .projection
                .map_or(bytes.len(), |mask| projection_size(&spans, mask));
            if size > request.bytes {
                return Err(Error::RowTooLarge);
            }
            if size > request.bytes - page.output_bytes {
                page.next = Some(plan.cursor(stored_key, true));
                return Ok(page);
            }
            page.output_bytes += size;
            if let Some(mask) = plan.projection {
                page.rows.push(copy_projection(&bytes, &spans, mask, size));
            } else {
                page.rows.push(bytes);
            }
            if let Some(boundary) = boundary {
                previous = Some(boundary.to_vec());
            }
        }
        if page.rows.len() == request.rows
            || page.examined == request.work
            || input_bytes
                .as_deref()
                .is_some_and(|used| *used > STEP_INPUT_BYTES - CANDIDATE_BYTES)
        {
            if terminal == Some(stored_key) {
                break;
            }
            if (aggregate.is_some() && input_bytes.is_none())
                || collected.is_some()
                || sorted.is_some()
            {
                // Completion has not been observed. Never expose partial reductions;
                // an exact-boundary scan may conservatively require more budget.
                return Err(Error::Limit);
            }
            page.next = Some(plan.cursor(stored_key, false));
            return Ok(page);
        }
    }
    if let Some(collection) = collected {
        page.output_bytes = collection.output_bytes;
        page.rows = collection.rows;
    }
    if let Some(collection) = sorted {
        collection.finish(request.bytes, &mut page)?;
    }
    Ok(page)
}
