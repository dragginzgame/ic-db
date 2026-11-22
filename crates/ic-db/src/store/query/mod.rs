//! Bounded fluent traversal. Typed result construction stays outside shared execution.

mod aggregate;
mod distinct;
mod order;
mod probe;
mod range;
mod sort;

pub use aggregate::{
    Aggregate, GroupCount, GroupOrder, GroupSummaries, GroupSummary, GroupSummaryPage, GroupValue,
    SummaryMeasure,
    jobs::{AggregateJobs, GroupCursor, GroupPage, JobHandle, JobOutcome, JobStep},
};

use super::{
    Database, ENTITY_PREFIX_BYTES, INDEX_PREFIX_BYTES, Map, prefix, row_key, validate_index_entry,
};
use crate::{
    Entity, EntitySchema, Error, FieldType, IndexSchema, IndexValue, TypeBody, TypeSchema, Value,
    codec::Decoder, key, primary::RowKey, types::TypeGraph,
};
use ic_storage::Memory;
use sha2::{Digest, Sha256};
use std::{
    cell::{Ref, RefCell},
    marker::PhantomData,
    ops::Deref,
    rc::Rc,
};

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
    fn supports_record_member(self) -> bool {
        self.is_relational() || matches!(self, Self::StartsWith | Self::EndsWith)
    }

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
                    crate::key::bytes_prefix(key.get(usize::from(optional)..)?)
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

    // Residual predicates have typed operands. Only non-byte ordering needs an encoded key.
    fn accepts_values(self, left: IndexValue<'_>, right: IndexValue<'_>) -> Option<bool> {
        match self {
            Self::Eq => Some(equal_values(left, right)),
            Self::Ne => Some(!equal_values(left, right)),
            _ => Some(self.accepts_bytes(scalar_bytes(left)?, scalar_bytes(right)?)),
        }
    }

    // Text and Blob operands borrowed from rows or fixed query values are already unframed.
    fn accepts_bytes(self, actual: &[u8], operand: &[u8]) -> bool {
        match self {
            Self::StartsWith => actual.starts_with(operand),
            Self::EndsWith => actual.ends_with(operand),
            Self::Contains => memchr::memmem::find(actual, operand).is_some(),
            _ => self.accepts(Some(actual), Some(operand), false),
        }
    }

    fn accepts_length(self, actual: Option<u32>, threshold: u32) -> bool {
        let encoded = actual.map(u32::to_be_bytes);

        self.accepts(
            encoded.as_ref().map(|v| v.as_slice()),
            Some(&threshold.to_be_bytes()),
            false,
        )
    }

    // These byte-length predicates admit exactly one present scalar value.
    fn empty_length_value(self, threshold: u32, field: FieldType) -> Option<IndexValue<'static>> {
        if !matches!((self, threshold), (Self::Eq | Self::Le, 0) | (Self::Lt, 1)) {
            return None;
        }
        match key::operand_type(field) {
            FieldType::Text { .. } => Some(IndexValue::Text("")),
            FieldType::Blob { .. } => Some(IndexValue::Blob(&[])),
            _ => None,
        }
    }

    // A result is uniform only when every integer in the inclusive interval agrees.
    fn uniform_length_result(self, min: u32, max: u32, n: u32) -> Option<bool> {
        match self {
            Self::Eq | Self::Ne if min == max || n < min || n > max => {
                Some(self.accepts_length(Some(min), n))
            }
            Self::Lt | Self::Ge if max < n || min >= n => Some(self.accepts_length(Some(min), n)),
            Self::Le | Self::Gt if max <= n || min > n => Some(self.accepts_length(Some(min), n)),
            _ => None,
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
    resume: Resume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Resume {
    After = 0,
    Retry = 1,
    AfterUnemittedClass = 2,
}
impl QueryCursor {
    pub(super) const MAX_ENCODED_BYTES: usize = 34 + crate::MAX_INDEX_KEY_BYTES;

    /// Encode the bounded continuation for an application's transport.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![1, self.resume as u8];
        bytes.extend(self.binding);
        bytes.extend(&self.position);

        bytes
    }

    /// Decode framing; execution additionally checks query identity and key shape.
    /// # Errors
    /// Refuses unsupported versions, flags and oversized or incomplete frames.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if !(35..=Self::MAX_ENCODED_BYTES).contains(&bytes.len()) || bytes[0] != 1 {
            return Err(Error::InvalidCursor);
        }

        Ok(Self {
            binding: bytes[2..34].try_into().map_err(|_| Error::InvalidCursor)?,
            position: bytes[34..].to_vec(),
            resume: match bytes[1] {
                0 => Resume::After,
                1 => Resume::Retry,
                2 => Resume::AfterUnemittedClass,
                _ => return Err(Error::InvalidCursor),
            },
        })
    }
}

impl serde::Serialize for QueryCursor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_bytes().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for QueryCursor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Frame;

        impl<'de> serde::de::Visitor<'de> for Frame {
            type Value = QueryCursor;

            fn expecting(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                fmt.write_str("a bounded IC DB query cursor frame")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<QueryCursor, A::Error> {
                let size = seq.size_hint().unwrap_or(0);
                if size > QueryCursor::MAX_ENCODED_BYTES {
                    return Err(serde::de::Error::custom(Error::InvalidCursor));
                }
                let mut frame = Vec::with_capacity(size);
                while let Some(byte) = seq.next_element()? {
                    if frame.len() == QueryCursor::MAX_ENCODED_BYTES {
                        return Err(serde::de::Error::custom(Error::InvalidCursor));
                    }
                    frame.push(byte);
                }
                QueryCursor::from_bytes(&frame).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_seq(Frame)
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
    /// Compare with another field or supported direct record member on the same row.
    Field(&'a str),
}

struct Predicate<'a> {
    field: &'a str,
    comparison: Comparison,
    operand: QueryOperand<'a>,
    // Consecutive alternatives form one clause; clauses are ANDed.
    or_next: bool,
}

impl<'a> Predicate<'a> {
    // The only field value that can satisfy this predicate, when known.
    fn fixed_equality(&self, entity: &EntitySchema) -> Option<(&'a str, IndexValue<'a>)> {
        if let QueryOperand::Length(n) = self.operand {
            let empty = self
                .comparison
                .empty_length_value(n, entity.index_field(self.field)?.value)?;
            return Some((self.field, empty));
        }
        let QueryOperand::Literal(value) = self.operand else {
            return None;
        };
        // Ordered comparisons exclude absence, including on optional fields.
        let fixed = match (self.comparison, value) {
            (Comparison::Eq, _) => value,
            (Comparison::Ne, IndexValue::Bool(excluded))
                if entity.field_is_required(self.field) =>
            {
                IndexValue::Bool(!excluded)
            }
            (Comparison::Le, _) if scalar_bytes(value).is_some_and(<[u8]>::is_empty) => value,
            (Comparison::Gt, IndexValue::Bool(false))
            | (Comparison::Ge, IndexValue::Bool(true)) => IndexValue::Bool(true),
            (Comparison::Lt, IndexValue::Bool(true))
            | (Comparison::Le, IndexValue::Bool(false)) => IndexValue::Bool(false),
            _ => return None,
        };
        Some((self.field, fixed))
    }

    // Each of these predicates matches exactly the rows where the field is present.
    fn tests_presence(&self, entity: &EntitySchema, operand: &TermOperand) -> bool {
        match (self.comparison, self.operand) {
            (Comparison::Ne, QueryOperand::Literal(IndexValue::Absent))
            | (Comparison::Ge, QueryOperand::Literal(IndexValue::Bool(false)))
            | (Comparison::Le, QueryOperand::Literal(IndexValue::Bool(true))) => true,
            (_, QueryOperand::Length(_)) => {
                self.present_length_result(entity) == Some(true)
                    && self.comparison != Comparison::Ne
            }
            (
                Comparison::Le
                | Comparison::Ge
                | Comparison::StartsWith
                | Comparison::EndsWith
                | Comparison::Contains,
                QueryOperand::Field(right),
            ) => right == self.field,
            (
                Comparison::Ge
                | Comparison::StartsWith
                | Comparison::EndsWith
                | Comparison::Contains,
                QueryOperand::Literal(needle),
            ) => {
                // An empty Text/Blob collection member still requires a matching element.
                !matches!(operand, TermOperand::Member { .. })
                    && scalar_bytes(needle).is_some_and(<[u8]>::is_empty)
            }
            _ => false,
        }
    }

    // Result for every present length in the declared inclusive range, if uniform.
    fn present_length_result(&self, entity: &EntitySchema) -> Option<bool> {
        let QueryOperand::Length(n) = self.operand else {
            return None;
        };
        let max = length_limit(entity.index_field(self.field)?.value)?;
        self.comparison.uniform_length_result(0, max, n)
    }

    fn constant_comparison(&self, entity: &EntitySchema) -> Option<bool> {
        if let QueryOperand::Literal(value) = self.operand {
            // Absence also fails these comparisons at the edges of the present domain.
            return match (self.comparison, value) {
                (Comparison::Lt, IndexValue::Bool(false))
                | (Comparison::Gt, IndexValue::Bool(true)) => Some(false),
                (Comparison::Lt, _) if scalar_bytes(value).is_some_and(<[u8]>::is_empty) => {
                    Some(false)
                }
                _ => None,
            };
        }
        if matches!(self.operand, QueryOperand::Length(_)) {
            let result = self.present_length_result(entity)?;
            return (result == self.comparison.accepts_absent(false)
                || entity.field_is_required(self.field))
            .then_some(result);
        }
        if !matches!(self.operand, QueryOperand::Field(right) if right == self.field) {
            return None;
        }
        match self.comparison {
            Comparison::Eq => Some(true),
            Comparison::Ne | Comparison::Lt | Comparison::Gt => Some(false),
            _ => None,
        }
    }
}

struct Request<'a> {
    groups: aggregate::GroupRequest<'a>,
    predicates: Vec<Predicate<'a>>,
    index: Option<(&'a str, Vec<IndexValue<'a>>)>,
    order: Option<Vec<&'a str>>,
    order_descending: u8,
    descending: bool,
    distinct: bool,
    single_result_probe: bool,
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
            single_result_probe: false,
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
    // A disjunction fixes a field only when every alternative fixes it to the same value.
    fn fixed_equalities<'r>(
        &'r self,
        entity: &'r EntitySchema,
    ) -> impl Iterator<Item = (&'a str, IndexValue<'a>)> + 'r {
        self.clauses().filter_map(move |clause| {
            let (predicate, rest) = clause.split_first()?;
            if let Some(equality) = predicate.fixed_equality(entity) {
                return rest
                    .iter()
                    .all(|other| other.fixed_equality(entity) == Some(equality))
                    .then_some(equality);
            }
            // Two mandatory inclusive bounds on the same literal fix one value.
            // Leave both admitted terms intact for evaluation and cursor binding.
            let ([], Comparison::Ge, QueryOperand::Literal(lower)) =
                (rest, predicate.comparison, predicate.operand)
            else {
                return None;
            };
            self.clauses()
                .any(|clause| {
                    let [upper] = clause else { return false };
                    upper.comparison == Comparison::Le
                        && upper.field == predicate.field
                        && upper.operand == QueryOperand::Literal(lower)
                })
                .then_some((predicate.field, lower))
        })
    }

    fn clauses(&self) -> impl Iterator<Item = &[Predicate<'a>]> {
        self.predicates.split_inclusive(|p| !p.or_next)
    }

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
    fn filter_not_in(
        &mut self,
        field: &'a str,
        values: impl ExactSizeIterator<Item = IndexValue<'a>>,
    ) {
        if values.len() == 0 || values.len() > MAX_TERMS - self.predicates.len() {
            self.over_limit = true;

            return;
        }
        for value in values {
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

enum QuerySource<'a, M: Memory> {
    Direct(&'a Database<M>),
    Session(Rc<RefCell<Database<M>>>),
}

enum QueryRead<'a, M: Memory> {
    Direct(&'a Database<M>),
    Session(Ref<'a, Database<M>>),
}

impl<M: Memory> Deref for QueryRead<'_, M> {
    type Target = Database<M>;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Direct(db) => db,
            Self::Session(db) => db,
        }
    }
}

impl<M: Memory> QuerySource<'_, M> {
    fn read(&self) -> Result<QueryRead<'_, M>, Error> {
        match self {
            Self::Direct(db) => Ok(QueryRead::Direct(db)),
            Self::Session(db) => db
                .try_borrow()
                .map(QueryRead::Session)
                .map_err(|_| Error::BorrowConflict),
        }
    }
}

/// Fluent exact predicates over one entity. No callback executor is generated.
/// Ordering uses primary order, an explicit index, a proven automatic path or
/// bounded complete sorting. Work exhaustion never returns a sorted prefix.
pub struct Query<'a, E, M: Memory> {
    db: QuerySource<'a, M>,
    request: Request<'a>,
    entity: PhantomData<E>,
}
impl<M: Memory> Database<M> {
    /// Begin a live primary-key-ordered query. Admission occurs on `fetch`.
    #[must_use]
    pub fn query<E: Entity>(&self) -> Query<'_, E, M> {
        Query {
            db: QuerySource::Direct(self),
            entity: PhantomData,
            request: Request::default(),
        }
    }
}
impl<'a, E: Entity, M: Memory> Query<'a, E, M> {
    pub(crate) fn from_session(db: Rc<RefCell<Database<M>>>) -> Self {
        Self {
            db: QuerySource::Session(db),
            request: Request::default(),
            entity: PhantomData,
        }
    }

    /// Remove duplicate output records. Proven leading index tuples use live
    /// pages. Projecting the primary key or the varying fields of a unique key covering every match
    /// needs no duplicate tracking. Other projections scan to
    /// completion in one call, retaining at most 32 tuples within 128 KiB.
    /// That fallback preserves first-match traversal order, refuses cursors and
    /// returns `Limit` if work, result count or combined bytes cannot fit; it
    /// never returns a partial result. An individually oversized row returns
    /// `RowTooLarge`. Comparison keys are bounded to 4096 bytes.
    /// For enumeration across calls, [`Self::start_distinct`] and [`Self::resume_distinct`]
    /// retain up to 255 tuples within a 16 KiB job frame, then page the completed result.
    /// Non-identity sorted DISTINCT retains the best `limit` distinct tuples. Its order
    /// must lead with all varying projected fields. A mandatory literal equality
    /// fixes a field. Remove duplicates before applying `limit` and output bytes.
    #[must_use]
    pub fn distinct(mut self) -> Self {
        self.request.distinct = true;

        self
    }

    /// Add an AND term. A top-level list or set supports exact scalar membership with `Contains`.
    /// A direct scalar record member selected by [`crate::field_path!`] accepts
    /// Eq, Ne, Lt, Le, Gt and Ge against a literal. Text and Blob members
    /// also accept `StartsWith` and `EndsWith` with an operand of the same type.
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

    /// Compare fields on the same row. Eq, Ne, Lt, Le, Gt and Ge also accept
    /// direct scalar record members on either side, as do StartsWith and EndsWith for Text and Blob.
    /// Top-level lists and sets support Contains
    /// with a present scalar field or direct record member of the element type. Text and Blob also
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
    /// list, set or map. A direct Text or Blob record member also supports byte
    /// length through [`crate::field_path!`]. Accepts Eq, Ne, Lt, Le, Gt and Ge. An absent field
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

    /// Add an OR clause of literal comparisons, ANDed with other filters.
    /// Alternatives may compare different direct scalar members of a record.
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
    /// Include `None` in a slice of optional values to match an absent field.
    /// When the field is the next traversal key component, seek between the smallest
    /// and largest member, including all tied owners. Interior candidates still consume work.
    /// Repeated members never repeat rows, but each counts toward the shared
    /// eight-comparison limit. Empty or oversized lists refuse with `Error::Limit`.
    /// Every member is admitted before reading; there is no coercion, sorting or
    /// deduplication of operands. Reordering members changes cursor/job identity.
    #[must_use]
    pub fn filter_in(
        mut self,
        field: &'a str,
        values: &[impl Copy + Into<IndexValue<'a>>],
    ) -> Self {
        self.request.or_clause(
            values
                .iter()
                .copied()
                .map(|value| (field, Comparison::Eq, QueryOperand::Literal(value.into()))),
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
    pub fn filter_not_in(
        mut self,
        field: &'a str,
        values: &[impl Copy + Into<IndexValue<'a>>],
    ) -> Self {
        self.request
            .filter_not_in(field, values.iter().copied().map(Into::into));

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

    /// Replace the current order with 1..=4 distinct ordered scalar fields, then the primary key.
    /// A direct record member can be selected with [`crate::field_path!`].
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
        observe!(query, { self.entity_page() })
    }

    fn entity_page(self) -> Result<QueryPage<E>, Error> {
        fetch_rows(&*self.db.read()?, E::SCHEMA, self.request, None)?.decode_entities()
    }

    /// Return the first matching entity in query order, or `None` on a proven miss.
    /// Uses a one-row page. For partial pages with resumable progress use `fetch`.
    /// # Errors
    /// Returns the same admission and storage errors as `fetch`. Returns `Limit`
    /// if the work budget ends before a match or exhaustion is established.
    pub fn first(self) -> Result<Option<E>, Error> {
        self.first_with(Self::entity_page)
    }

    /// Return the first projected record in query order.
    /// # Errors
    /// Applies the projection checks of [`Self::fetch_as`] and the work bound of [`Self::first`].
    pub fn first_as<R: Value>(self) -> Result<Option<R>, Error> {
        self.first_with(Self::projected_page::<R>)
    }

    fn first_with<R>(
        self,
        fetch: impl FnOnce(Self) -> Result<QueryPage<R>, Error>,
    ) -> Result<Option<R>, Error> {
        observe!(query, {
            let page = fetch(self.probe_matches(1)?)?;
            if page.rows.is_empty() && page.next.is_some() {
                return Err(Error::Limit);
            }
            Ok(page)
        })
        .map(|mut page| page.rows.pop())
    }

    /// Return the only match, or `None` when the query is exhausted without one.
    /// A valid row limit is overridden to probe for two matches. Applies to the
    /// whole query, so a continuation is refused.
    /// # Errors
    /// Returns `MultipleMatches` on a second match and `Limit` if the page is
    /// incomplete before cardinality is known. Ordinary fetch errors apply.
    pub fn try_one(self) -> Result<Option<E>, Error> {
        self.try_one_with(Self::entity_page)
    }

    /// Return the sole projected result, or `None` on a proven miss.
    /// With DISTINCT, a literal equality or fixed index prefix covering every
    /// projected field lets an ordered traversal finish at the first match.
    /// # Errors
    /// Applies the projection checks of [`Self::fetch_as`] and the cardinality checks of [`Self::try_one`].
    pub fn try_one_as<R: Value>(self) -> Result<Option<R>, Error> {
        self.try_one_with(Self::projected_page::<R>)
    }

    fn try_one_with<R>(
        self,
        fetch: impl FnOnce(Self) -> Result<QueryPage<R>, Error>,
    ) -> Result<Option<R>, Error> {
        observe!(query, {
            if self.request.cursor.is_some() {
                return Err(Error::InvalidQuery);
            }
            let page = fetch(self.probe_matches(2)?)?;
            if page.rows.len() == 2 {
                return Err(Error::MultipleMatches);
            }
            if page.next.is_some() {
                return Err(Error::Limit);
            }

            Ok(page)
        })
        .map(|mut page| page.rows.pop())
    }

    fn probe_matches(mut self, rows: usize) -> Result<Self, Error> {
        if !(1..=32).contains(&self.request.rows) {
            return Err(Error::Limit);
        }

        self.request.single_result_probe = true;

        Ok(self.limit(rows))
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
        observe!(query, { self.projected_page() })
    }

    fn projected_page<R: Value>(self) -> Result<QueryPage<R>, Error> {
        let raw = fetch_rows(
            &*self.db.read()?,
            E::SCHEMA,
            self.request,
            Some(R::definition()),
        )?;

        raw.decode_projection()
    }
}

// Reuse named-record metadata/codecs rather than a second value tree or a
// generic executor per output shape. The full transitive graph matters: named
// links compare by identity, which alone cannot establish codec compatibility.
fn admit_projection(entity: &EntitySchema, output: &TypeSchema) -> Result<u32, Error> {
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
            .field_position(field.name)
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
    tests_presence: bool,
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
    Nested {
        member: crate::member::RecordMember,
        literal: Vec<u8>,
        absent: bool,
    },
    Field {
        right: usize,
        left_member: Option<crate::member::RecordMember>,
        right_member: Option<crate::member::RecordMember>,
    },
    Length {
        member: Option<crate::member::RecordMember>,
        threshold: u32,
    },
}

enum MemberNeedle {
    Literal(Vec<u8>),
    Field(usize, Option<crate::member::RecordMember>),
}

// Operands have already been type-checked. Text and Blob literals carry their
// original bytes, before the durable key codec escapes embedded zeros.
fn scalar_bytes(value: IndexValue<'_>) -> Option<&[u8]> {
    match value {
        IndexValue::Text(text) => Some(text.as_bytes()),
        IndexValue::Blob(bytes) => Some(bytes),
        _ => None,
    }
}

fn equal_values(left: IndexValue<'_>, right: IndexValue<'_>) -> bool {
    // Admission already checked both enum contracts.
    match (left, right) {
        (IndexValue::UnitEnum { ordinal: a, .. }, IndexValue::UnitEnum { ordinal: b, .. }) => {
            a == b
        }
        _ => left == right,
    }
}

fn length_limit(field: FieldType) -> Option<u32> {
    match key::operand_type(field) {
        FieldType::Text { max_bytes } | FieldType::Blob { max_bytes } => Some(max_bytes),
        FieldType::List { max_items, .. } => Some(max_items),
        FieldType::Named(link) => match (link.definition)().body {
            TypeBody::Map { max_items, .. } | TypeBody::Set { max_items, .. } => Some(max_items),
            _ => None,
        },
        _ => None,
    }
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
    needle: IndexValue<'_>,
) -> Result<bool, Error> {
    let mut reader = Decoder::new(span)?;
    if optional && !reader.boolean()? {
        return Ok(false);
    }
    for _ in 0..reader.nat32()? {
        let value = IndexValue::read(&mut reader, item)?;
        if equal_values(value, needle) {
            return Ok(true);
        }
    }

    Ok(false)
}

// Admission sets bits only for fields in this entity. Consuming the lowest
// bit first preserves the canonical field order of projected records and keys.
fn projection_positions(mut mask: u32) -> impl Iterator<Item = usize> {
    std::iter::from_fn(move || {
        if mask == 0 {
            return None;
        }
        let position = mask.trailing_zeros() as usize;
        mask &= mask - 1;
        Some(position)
    })
}

fn admit_ordered_fields(entity: &EntitySchema, mask: u32) -> Result<(), Error> {
    for i in projection_positions(mask) {
        if !key::supports(entity.fields[i].value) {
            return Err(Error::InvalidQuery);
        }
    }

    Ok(())
}

// Grouping, distinct counts and collected DISTINCT share exact tuple equality.
fn comparison_key(
    entity: &EntitySchema,
    mask: u32,
    values: &[IndexValue<'_>],
    scratch: &mut Vec<u8>,
) -> Result<(), Error> {
    build_comparison_key(entity, mask, scratch, |i, _| Ok(values[i]))
}

fn build_comparison_key<'a>(
    entity: &EntitySchema,
    mask: u32,
    key: &mut Vec<u8>,
    mut value: impl FnMut(usize, &crate::FieldSchema) -> Result<IndexValue<'a>, Error>,
) -> Result<(), Error> {
    key.clear();
    for i in projection_positions(mask) {
        let field = &entity.fields[i];
        value(i, field)?.append_component(field.optional, key);
        if key.len() > MAX_COMPARISON_KEY {
            return Err(Error::Limit);
        }
    }

    Ok(())
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
fn projection_size(spans: &[(u16, u16)], mask: u32) -> usize {
    projection_positions(mask)
        .map(|i| {
            let (start, end) = spans[i];
            usize::from(end - start)
        })
        .sum()
}

fn copy_projection(bytes: &[u8], spans: &[(u16, u16)], mask: u32, size: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(size);
    for i in projection_positions(mask) {
        let (start, end) = spans[i];
        output.extend_from_slice(&bytes[usize::from(start)..usize::from(end)]);
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
    selected: u32,
    // Non-orderable fields need only their validated optional tags, never key slots.
    span_fields: u32,
    binding: [u8; 32],
    projection: Option<u32>,
    distinct: distinct::Strategy,
    sort: Option<Vec<order::SortField>>,
    descending: bool,
}
impl<'a> Plan<'a> {
    fn clauses(&self) -> impl Iterator<Item = &[Term]> {
        self.terms.split_inclusive(|term| !term.or_next)
    }

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
            .field_position(entity.primary_key)
            .filter(|&i| !entity.fields[i].optional)
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
            let (position, record_member) = crate::member::resolve_field(entity, predicate.field)?;
            let field = &entity.fields[position];
            let member = if predicate.comparison == Comparison::Contains {
                member_type(field.value)
            } else {
                None
            };
            let operand = match (predicate.operand, record_member) {
                (QueryOperand::Literal(value), Some(member)) => {
                    if !predicate.comparison.supports_record_member() {
                        return Err(Error::InvalidQuery);
                    }
                    let leaf = member.field();
                    predicate.comparison.admit(
                        leaf.value,
                        field.optional || leaf.optional,
                        value,
                    )?;
                    let mut literal = Vec::new();
                    value.append_component(false, &mut literal);
                    plan.span_fields |= 1 << position;
                    TermOperand::Nested {
                        member,
                        literal,
                        absent: value == IndexValue::Absent,
                    }
                }
                (QueryOperand::Length(length), member) => {
                    let selected = member.map_or(field.value, |m| m.field().value);
                    if length_limit(selected).is_none() || !predicate.comparison.is_relational() {
                        return Err(Error::InvalidQuery);
                    }
                    plan.span_fields |= 1 << position;

                    TermOperand::Length {
                        member,
                        threshold: length,
                    }
                }
                (QueryOperand::Field(name), left_member) => {
                    let (right, right_member) = crate::member::resolve_field(entity, name)?;
                    if (left_member.is_some() || right_member.is_some())
                        && member.is_none()
                        && !predicate.comparison.supports_record_member()
                    {
                        return Err(Error::InvalidQuery);
                    }
                    let right_type =
                        right_member.map_or(entity.fields[right].value, |m| m.field().value);
                    let left_type =
                        left_member.map_or(member.unwrap_or(field.value), |m| m.field().value);
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
                    if right_member.is_some() {
                        plan.span_fields |= 1 << right;
                    } else {
                        plan.selected |= 1 << right;
                    }

                    if let Some(item) = member {
                        plan.span_fields |= 1 << position;

                        TermOperand::Member {
                            item,
                            needle: MemberNeedle::Field(right, right_member),
                        }
                    } else {
                        if left_member.is_some() {
                            plan.span_fields |= 1 << position;
                        } else {
                            plan.selected |= 1 << position;
                        }
                        TermOperand::Field {
                            right,
                            left_member,
                            right_member,
                        }
                    }
                }
                (QueryOperand::Literal(value), None) => {
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
                tests_presence: predicate.tests_presence(entity, &operand),
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
            order::Path::Index(path, descending) => {
                plan.use_index(entity, path);
                plan.descending = request.descending ^ descending;
            }
            order::Path::Sort(fields) => {
                order::select_filtered_path(entity, request, &mut plan, None)?;
                for field in &fields {
                    if field.member.is_some() {
                        plan.span_fields |= 1 << field.position;
                    } else {
                        plan.selected |= 1 << field.position;
                    }
                }
                plan.sort = Some(fields);
            }
        }
        if request.distinct {
            plan.distinct = distinct::admit(entity, request, &plan)?;
            if let distinct::Strategy::Complete { key_fields } = plan.distinct {
                plan.selected |= key_fields;
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
                TermOperand::Nested {
                    member,
                    literal,
                    absent,
                } => {
                    hash.update((u32::MAX - 2).to_le_bytes());
                    hash.update([member.position() as u8, u8::from(*absent)]);
                    hash.update((literal.len() as u32).to_le_bytes());
                    hash.update(literal);
                }
                TermOperand::Literal { bytes, .. }
                | TermOperand::Member {
                    needle: MemberNeedle::Literal(bytes),
                    ..
                } => {
                    hash.update((bytes.len() as u32).to_le_bytes());
                    hash.update(bytes);
                }
                TermOperand::Length { member, threshold } => {
                    if let Some(member) = member {
                        hash.update((u32::MAX - 3).to_le_bytes());
                        hash.update([member.position() as u8]);
                    } else {
                        hash.update((u32::MAX - 1).to_le_bytes());
                    }
                    hash.update(threshold.to_le_bytes());
                }
                TermOperand::Field {
                    right,
                    left_member: None,
                    right_member: None,
                }
                | TermOperand::Member {
                    needle: MemberNeedle::Field(right, None),
                    ..
                } => {
                    // Outside the bounded literal length domain.
                    hash.update(u32::MAX.to_le_bytes());
                    hash.update([*right as u8]);
                }
                TermOperand::Member {
                    needle: MemberNeedle::Field(right, Some(member)),
                    ..
                } => {
                    hash.update((u32::MAX - 4).to_le_bytes());
                    hash.update([u8::MAX, *right as u8, member.position() as u8]);
                }
                TermOperand::Field {
                    right,
                    left_member,
                    right_member,
                } => {
                    hash.update((u32::MAX - 4).to_le_bytes());
                    let slot = |member: &Option<crate::member::RecordMember>| {
                        member.map_or(u8::MAX, |m| m.position() as u8)
                    };
                    hash.update([slot(left_member), *right as u8, slot(right_member)]);
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
                if let Some(position) = entity.field_position(name) {
                    hash.update([position as u8]);
                } else {
                    // A top-level position is at most 31. Frame each admitted nested path.
                    hash.update([u8::MAX, name.len() as u8]);
                    hash.update(name.as_bytes());
                }
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
            plan.validate_position(entity, cursor)
                .map_err(|_| Error::InvalidCursor)?;
        }

        Ok(plan)
    }

    fn validate_position(&self, entity: &EntitySchema, cursor: &QueryCursor) -> Result<(), Error> {
        if cursor.resume == Resume::AfterUnemittedClass
            && !matches!(self.distinct, distinct::Strategy::Adjacent(_))
        {
            return Err(Error::InvalidCursor);
        }
        let bytes = cursor.position.as_slice();
        if bytes.len() > crate::MAX_INDEX_KEY_BYTES || !bytes.starts_with(&self.prefix) {
            return Err(Error::InvalidCursor);
        }
        if let Some(index) = self.index {
            let mut remaining = bytes
                .get(INDEX_PREFIX_BYTES..)
                .ok_or(Error::InvalidCursor)?;
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
                bytes
                    .get(ENTITY_PREFIX_BYTES..)
                    .ok_or(Error::InvalidCursor)?,
                entity.key_type()?,
            )?;
        }

        Ok(())
    }

    fn cursor(&self, position: &[u8], resume: Resume) -> QueryCursor {
        QueryCursor {
            binding: self.binding,
            position: position.to_vec(),
            resume,
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
    let field = entity.index_field(name).ok_or(Error::InvalidQuery)?;

    key::consume_component(remaining, field, index.present_only, Error::InvalidCursor)
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
    execute_inner(
        map,
        entity,
        request,
        plan,
        range::bounds(entity, request, plan)?,
        aggregate.as_mut(),
        None,
    )
}

// Retained aggregates use the same traversal, but checkpoint at a work boundary.
// Reserve a maximum candidate before the next fetch, including index/owner keys.
const STEP_INPUT_BYTES: usize = 64 * 1024;
const fn candidate_byte_limit(row_bytes: usize) -> usize {
    row_bytes + 2 * crate::MAX_INDEX_KEY_BYTES
}

fn execute_inner<M: Memory>(
    map: &Map<M>,
    entity: &EntitySchema,
    request: &Request<'_>,
    plan: &Plan<'_>,
    bounds: Option<range::Bounds>,
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
    let Some(bounds) = bounds else {
        return Ok(page);
    };
    let terminal = range::terminal_key(&bounds, plan.descending);
    let mut entries = map.range((bounds.0.as_ref(), bounds.1.as_ref()));
    let mut collected = if let distinct::Strategy::Complete { key_fields } = plan.distinct {
        Some(distinct::Collection::new(
            plan.projection.ok_or(Error::InvalidQuery)?,
            key_fields,
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
    // Only suppress a class already emitted by this traversal.
    let mut previous = if let Some(cursor) = request.cursor.filter(|c| c.resume == Resume::After) {
        distinct::boundary(entity, plan.index, plan.distinct, &cursor.position)?.map(<[u8]>::to_vec)
    } else {
        None
    };
    let candidate_bytes = candidate_byte_limit(entity.max_row_bytes);
    let primary_type = entity.fields[plan.primary].value;
    let needs_spans = plan.index.is_some()
        || plan.span_fields != 0
        || match &aggregate {
            Some(accumulator) => accumulator.needs_spans(),
            None => plan.projection.is_some(),
        };
    // Inspection overwrites every declared field's span, including absent fields.
    let mut spans = [(0_u16, 0_u16); crate::schema::MAX_FIELDS];
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
                RowKey::decode(&stored_key[ENTITY_PREFIX_BYTES..], primary_type)?,
                entry.value(),
            )
        };
        if let Some(used) = input_bytes.as_deref_mut() {
            read_bytes += bytes.len();
            if read_bytes > candidate_bytes {
                return Err(Error::Corrupt);
            }
            *used += read_bytes;
        }
        page.fetched += 1;
        let mut values = [IndexValue::Absent; crate::schema::MAX_FIELDS];
        entity.inspect_fields(
            &bytes,
            plan.selected,
            &mut values,
            needs_spans.then_some(&mut spans),
        )?;
        if RowKey::from_index(values[plan.primary])? != owner {
            return Err(Error::Corrupt);
        }
        if plan.index.is_some() {
            validate_index_entry(
                entity,
                &entity.row_info(&values, &bytes, &spans)?,
                owner,
                stored_key,
            )?;
        }
        let read = |position: usize, member: Option<crate::member::RecordMember>| {
            if let Some(member) = member {
                let (start, end) = spans[position];
                member.read(
                    &bytes[usize::from(start)..usize::from(end)],
                    entity.fields[position].optional,
                )
            } else {
                Ok(values[position])
            }
        };
        let mut accepts = |term: &Term, supplied: &QueryOperand<'_>| -> Result<bool, Error> {
            let mut value = values[term.position];
            component.clear();
            let (operand, optional) = match &term.operand {
                TermOperand::Nested {
                    member,
                    literal,
                    absent,
                } => {
                    value = read(term.position, Some(*member))?;
                    if *absent {
                        return Ok(term.comparison.accepts_absent(value == IndexValue::Absent));
                    }
                    (Some(literal.as_slice()), false)
                }
                TermOperand::Literal { absent: true, .. } => {
                    let absent = if plan.span_fields & (1 << term.position) != 0 {
                        bytes[usize::from(spans[term.position].0)] == 0
                    } else {
                        value == IndexValue::Absent
                    };

                    return Ok(term.comparison.accepts_absent(absent));
                }
                TermOperand::Member { item, needle } => {
                    let needle = match (needle, supplied) {
                        (MemberNeedle::Literal(_), QueryOperand::Literal(value)) => *value,
                        (MemberNeedle::Field(right, member), QueryOperand::Field(_)) => {
                            read(*right, *member)?
                        }
                        _ => return Err(Error::Corrupt),
                    };
                    if needle == IndexValue::Absent {
                        return Ok(false);
                    }
                    let (start, end) = spans[term.position];

                    return contains_member(
                        &bytes[usize::from(start)..usize::from(end)],
                        entity.fields[term.position].optional,
                        *item,
                        needle,
                    );
                }
                TermOperand::Length { member, threshold } => {
                    let (start, end) = spans[term.position];
                    let span = &bytes[usize::from(start)..usize::from(end)];
                    let optional = entity.fields[term.position].optional;
                    let actual = if let Some(member) = member {
                        match member.read(span, optional)? {
                            IndexValue::Absent => None,
                            value => Some(scalar_bytes(value).ok_or(Error::Corrupt)?.len() as u32),
                        }
                    } else {
                        crate::codec::validated_length(span, optional)
                    };

                    return Ok(term.comparison.accepts_length(actual, *threshold));
                }
                TermOperand::Literal {
                    bytes,
                    absent: false,
                } => (
                    Some(bytes.as_slice()),
                    entity.fields[term.position].optional,
                ),
                TermOperand::Field {
                    right,
                    left_member,
                    right_member,
                } => {
                    value = read(term.position, *left_member)?;
                    let rhs = read(*right, *right_member)?;
                    if value == IndexValue::Absent || rhs == IndexValue::Absent {
                        return Ok(term.comparison.accepts_absent(value == rhs));
                    }
                    if let Some(matches) = term.comparison.accepts_values(value, rhs) {
                        return Ok(matches);
                    }
                    right_component.clear();
                    rhs.append_component(false, &mut right_component);

                    (Some(right_component.as_slice()), false)
                }
            };
            if let QueryOperand::Literal(right) = supplied
                && let Some(matches) = term.comparison.accepts_values(value, *right)
            {
                return Ok(matches);
            }
            value.append_component(optional, &mut component);

            Ok(term.comparison.accepts(
                (value != IndexValue::Absent).then_some(component.as_slice()),
                operand,
                optional,
            ))
        };
        let matches = 'matches: {
            let mut clause_matches = false;
            for (term, predicate) in plan.terms.iter().zip(&request.predicates) {
                clause_matches = clause_matches || accepts(term, &predicate.operand)?;
                if !term.or_next {
                    if !clause_matches {
                        break 'matches false;
                    }
                    clause_matches = false;
                }
            }

            true
        };
        let boundary = distinct::boundary(entity, plan.index, plan.distinct, stored_key)?;
        let duplicate = boundary.is_some_and(|key| previous.as_deref() == Some(key));
        if matches && let Some(accumulator) = aggregate.as_mut() {
            accumulator.push(entity, &values, &bytes, &spans, &mut component)?;
            if matches!(accumulator, aggregate::Accumulator::Exists(_)) {
                // One validated match proves existence, even at the work boundary.
                return Ok(page);
            }
        } else if matches && let Some(collection) = collected.as_mut() {
            collection.push(entity, request, &values, &bytes, &spans, &mut component)?;
            // This path emits first occurrences in traversal order. A single-result
            // terminal has its answer after one class (first) or two (non-unique).
            if request.single_result_probe && collection.rows.len() == request.rows {
                break;
            }
        } else if matches && let Some(collection) = sorted.as_mut() {
            collection.push(entity, plan, &values, &bytes, &spans, &mut component)?;
        } else if matches && !duplicate {
            let size = plan
                .projection
                .map_or(bytes.len(), |mask| projection_size(&spans, mask));
            if size > request.bytes {
                return Err(Error::RowTooLarge);
            }
            if size > request.bytes - page.output_bytes {
                page.next = Some(plan.cursor(stored_key, Resume::Retry));

                return Ok(page);
            }
            page.output_bytes += size;
            if let Some(mask) = plan.projection {
                page.rows.push(copy_projection(&bytes, &spans, mask, size));
            } else {
                page.rows.push(bytes);
            }
            if plan.distinct == distinct::Strategy::FirstMatch {
                return Ok(page);
            }
            if let Some(boundary) = boundary {
                previous = Some(boundary.to_vec());
            }
        }
        if page.rows.len() == request.rows
            || page.examined == request.work
            || input_bytes
                .as_deref()
                .is_some_and(|used| *used > STEP_INPUT_BYTES - candidate_bytes)
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
            let resume = if boundary.is_some_and(|key| previous.as_deref() != Some(key)) {
                Resume::AfterUnemittedClass
            } else {
                Resume::After
            };
            page.next = Some(plan.cursor(stored_key, resume));

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
