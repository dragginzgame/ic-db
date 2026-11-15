//! Bounded fluent traversal. Typed result construction stays outside shared execution.
mod aggregate;
mod distinct;
mod order;
mod range;
pub use aggregate::jobs::{AggregateJobs, GroupCursor, GroupPage, JobHandle, JobOutcome, JobStep};
pub use aggregate::{Aggregate, GroupCount, GroupOrder, GroupValue};

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

/// Exact comparison. Equality distinguishes absent values; ranges and text matches require presence.
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
    /// Present text starts with the exact UTF-8 operand, without normalization.
    /// Empty prefixes match all present text, including empty text; other types refuse.
    StartsWith = 6,
    /// Present text ends with the exact UTF-8 operand, without normalization.
    /// Empty suffixes match all present text, including empty text; other types refuse.
    EndsWith = 7,
    /// Present text contains the exact UTF-8 operand, without normalization.
    /// Empty needles match all present text; this is not collection membership.
    Contains = 8,
}

impl Comparison {
    fn admit(self, field: FieldType, optional: bool, value: IndexValue<'_>) -> Result<(), Error> {
        value.admit(field, optional)?;
        if matches!(self, Self::StartsWith | Self::EndsWith | Self::Contains)
            && !matches!(field, FieldType::Text { .. })
        {
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
                // Compare only escaped text, never the presence byte or terminator.
                // In particular, optional empty text must not match suffix U+0001.
                fn text(key: &[u8], optional: bool) -> Option<&[u8]> {
                    key.get(usize::from(optional)..)?.strip_suffix(&[0, 0])
                }
                actual
                    .and_then(|key| text(key, optional))
                    .zip(operand.and_then(|key| text(key, optional)))
                    .is_some_and(|(actual, operand)| match self {
                        Self::StartsWith => actual.starts_with(operand),
                        Self::EndsWith => actual.ends_with(operand),
                        // Valid UTF-8 needles cannot start within an escaped NUL
                        // (00 FF) or a multibyte character. Search is linear and
                        // constant-space even for repetitive bounded operands.
                        _ => memchr::memmem::find(actual, operand).is_some(),
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
}

struct Predicate<'a> {
    field: &'a str,
    comparison: Comparison,
    value: IndexValue<'a>,
    // Consecutive alternatives form one clause; clauses are ANDed.
    or_next: bool,
}
struct Request<'a> {
    groups: aggregate::GroupRequest<'a>,
    predicates: Vec<Predicate<'a>>,
    index: Option<(&'a str, Vec<IndexValue<'a>>)>,
    order: Option<Vec<&'a str>>,
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
    let plan = Plan::admit(entity, &request, binding, incarnation, None, false)?;
    execute(map, entity, &request, &plan, None)
}

/// Fluent exact predicates over one entity. No callback executor is generated.
/// Ordering uses primary order, an explicit index or a proven automatic path.
/// Arbitrary sorting is not supported.
pub struct Query<'a, E, M: Memory = ic_storage::DefaultMemoryImpl> {
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
    /// Remove duplicate output records. Identity projections and proven leading
    /// index tuples use live pages. Other ordered-field projections scan to
    /// completion in one call, retaining at most 32 tuples within 128 KiB.
    /// That fallback preserves first-match traversal order, refuses cursors and
    /// returns `Limit` if work, result count or combined bytes cannot fit; it
    /// never returns a partial result. An individually oversized row returns
    /// `RowTooLarge`. Comparison keys are bounded to 4096 bytes.
    #[must_use]
    pub fn distinct(mut self) -> Self {
        self.request.distinct = true;
        self
    }
    /// Add an AND term over an exact top-level ordered scalar/unit-enum field.
    /// Eq/Ne with `IndexValue::Absent` also accept any optional stored field,
    /// including blobs and containers. Empty values are present, not absent.
    /// All operands are admitted before storage reads, including contradictory terms.
    /// On a primary traversal, mandatory Eq/Lt/Le/Gt/Ge primary-key comparisons
    /// narrow the scanned range. On an index path, those comparisons can narrow
    /// its first field after the admitted fixed prefix. Only examined rows
    /// undergo integrity checks; oversized derived bounds remain residual filters.
    /// Indexed StartsWith can seek that field's exact encoded text prefix too.
    #[must_use]
    pub fn filter(
        mut self,
        field: &'a str,
        comparison: Comparison,
        value: impl Into<IndexValue<'a>>,
    ) -> Self {
        if self.request.predicates.len() == MAX_TERMS {
            self.request.over_limit = true;
        } else {
            self.request.predicates.push(Predicate {
                field,
                comparison,
                value: value.into(),
                or_next: false,
            });
        }
        self
    }
    /// Add an OR clause of exact top-level comparisons, ANDed with other filters.
    /// Each clause must contain at least one comparison; all filters together
    /// may contain at most eight. Empty/oversized clauses refuse with `Error::Limit`
    /// at execution. Every operand is admitted, even if another alternative matches.
    /// Uses one ordered traversal, without union scans or duplicate rows. Alternatives
    /// do not establish fixed index prefixes; a one-term clause behaves like `filter`.
    #[must_use]
    pub fn filter_any(self, alternatives: &[(&'a str, Comparison, IndexValue<'a>)]) -> Self {
        self.or_clause(alternatives.iter().copied())
    }
    /// Match a field against any supplied exact value, ANDed with other filters.
    /// Equivalent to `filter_any` with equality comparisons in the supplied order.
    /// Absence matches only an explicit `IndexValue::Absent` member on an optional field.
    /// Repeated members never repeat rows, but each counts toward the shared
    /// eight-comparison limit. Empty or oversized lists refuse with `Error::Limit`.
    /// Every member is admitted before reading; there is no coercion, sorting or
    /// deduplication of operands. Reordering members changes cursor/job identity.
    #[must_use]
    pub fn filter_in(self, field: &'a str, values: &[IndexValue<'a>]) -> Self {
        self.or_clause(
            values
                .iter()
                .copied()
                .map(|value| (field, Comparison::Eq, value)),
        )
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
        if values.is_empty() || values.len() > MAX_TERMS - self.request.predicates.len() {
            self.request.over_limit = true;
            return self;
        }
        values.iter().fold(self, |query, &value| {
            query.filter(field, Comparison::Ne, value)
        })
    }

    fn or_clause(
        mut self,
        alternatives: impl ExactSizeIterator<Item = (&'a str, Comparison, IndexValue<'a>)>,
    ) -> Self {
        let count = alternatives.len();
        if count == 0 || count > MAX_TERMS - self.request.predicates.len() {
            self.request.over_limit = true;
        } else {
            self.request.predicates.extend(alternatives.enumerate().map(
                |(i, (field, comparison, value))| Predicate {
                    field,
                    comparison,
                    value,
                    or_next: i + 1 < count,
                },
            ));
        }
        self
    }
    /// Select index order and exact leading components (at most four).
    /// All rows matching this prefix are considered. Sparse indexes require
    /// every optional indexed component to be proven present by the prefix or
    /// filters. Every alternative in a proving OR clause must reject absence.
    #[must_use]
    pub fn using_index(mut self, name: &'a str, values: &[IndexValue<'a>]) -> Self {
        if values.len() > 4 {
            self.request.over_limit = true;
        } else {
            self.request.index = Some((name, values.to_vec()));
        }
        self
    }
    /// Order by 1..=4 distinct top-level ordered fields, then the primary key.
    /// An explicitly listed primary key must be last. All fields share one
    /// direction, reversed by `descending`. Select a complete ordered path or
    /// refuse; never sort a bounded input prefix. Equality terms may fix leading
    /// index components. With `using_index`, verify that requested path instead.
    #[must_use]
    pub fn order_by(mut self, fields: &[&'a str]) -> Self {
        if fields.len() > 4 {
            self.request.over_limit = true;
        } else {
            self.request.order = Some(fields.to_vec());
        }
        self
    }
    /// Reverse the complete access-path order, including its unique tie-breaker.
    #[must_use]
    pub fn descending(mut self) -> Self {
        self.request.descending = true;
        self
    }
    /// Return at most 1..=32 matching rows in this page.
    /// Complete-collection DISTINCT refuses if the full result exceeds this cap.
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
    /// Allow 1..=131072 encoded result-row bytes. Transport framing is host-owned.
    #[must_use]
    pub fn byte_limit(mut self, bytes: usize) -> Self {
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
    /// A matching row larger than the whole byte allowance returns `RowTooLarge`.
    /// # Panics
    /// Supplier corruption traps without repair, as for direct record reads.
    pub fn fetch(self) -> Result<QueryPage<E>, Error> {
        self.db.schema.admits(E::SCHEMA)?;
        let plan = Plan::admit(
            E::SCHEMA,
            &self.request,
            self.db.schema.fingerprint(),
            self.db.incarnation,
            None,
            false,
        )?;
        let raw = execute(&self.db.map, E::SCHEMA, &self.request, &plan, None)?;
        raw.decode_entities()
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
        self.db.schema.admits(E::SCHEMA)?;
        let plan = Plan::admit(
            E::SCHEMA,
            &self.request,
            self.db.schema.fingerprint(),
            self.db.incarnation,
            Some(R::definition()),
            false,
        )?;
        let raw = execute(&self.db.map, E::SCHEMA, &self.request, &plan, None)?;
        Ok(QueryPage {
            rows: raw
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
            next: raw.next,
            examined: raw.examined,
            fetched: raw.fetched,
            output_bytes: raw.output_bytes,
        })
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
    operand: Vec<u8>,
    absent: bool,
    or_next: bool,
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
struct Plan<'a> {
    index: Option<&'a IndexSchema>,
    prefix: Vec<u8>,
    fixed_components: usize,
    terms: Vec<Term>,
    selected: u16,
    // Non-orderable fields need only their validated optional tags, never key slots.
    presence: u16,
    binding: [u8; 32],
    projection: Option<u16>,
    distinct: distinct::Strategy,
}
impl<'a> Plan<'a> {
    fn admit(
        entity: &'a EntitySchema,
        request: &Request<'_>,
        schema: [u8; 32],
        incarnation: [u8; 16],
        projection: Option<&TypeSchema>,
        group_output: bool,
    ) -> Result<Self, Error> {
        if request.over_limit
            || !(1..=32).contains(&request.rows)
            || !(1..=MAX_WORK).contains(&request.work)
            || !(1..=MAX_OUTPUT).contains(&request.bytes)
        {
            return Err(Error::Limit);
        }
        if !group_output && !request.groups.is_empty() {
            return Err(Error::InvalidQuery);
        }
        // Bound literal encoding even when a declaration's maximum exceeds one row.
        for value in request
            .predicates
            .iter()
            .map(|p| &p.value)
            .chain(request.index.iter().flat_map(|(_, values)| values.iter()))
        {
            if matches!(value, IndexValue::Text(text) if text.len() > crate::codec::MAX_ROW_BYTES) {
                return Err(Error::Limit);
            }
        }
        let primary = entity
            .fields
            .iter()
            .position(|f| f.name == entity.primary_key)
            .ok_or(Error::InvalidQuery)?;
        let mut plan = Self {
            index: None,
            prefix: prefix(2, entity.id),
            fixed_components: 0,
            terms: Vec::new(),
            selected: 1 << primary,
            presence: 0,
            binding: [0; 32],
            projection: projection
                .map(|output| admit_projection(entity, output))
                .transpose()?,
            distinct: distinct::Strategy::Identity,
        };
        for predicate in &request.predicates {
            let position = entity
                .fields
                .iter()
                .position(|f| f.name == predicate.field)
                .ok_or(Error::InvalidQuery)?;
            let field = &entity.fields[position];
            let ordered = key::supports(field.value);
            if !ordered
                && !(predicate.value == IndexValue::Absent
                    && matches!(predicate.comparison, Comparison::Eq | Comparison::Ne))
            {
                return Err(Error::InvalidQuery);
            }
            predicate
                .comparison
                .admit(field.value, field.optional, predicate.value)?;
            let absent = predicate.value == IndexValue::Absent;
            let mut operand = Vec::new();
            predicate
                .value
                .append_component(field.optional, &mut operand);
            plan.terms.push(Term {
                position,
                comparison: predicate.comparison,
                operand,
                absent,
                or_next: predicate.or_next,
            });
            if ordered {
                plan.selected |= 1 << position;
            } else {
                plan.presence |= 1 << position;
            }
        }
        // Admit every operand before selecting a path, even for empty or
        // contradictory queries. The scanner consumes this one proven choice.
        if let Some(path) = order::select(entity, request)? {
            plan.index = Some(path.index);
            plan.prefix = path.prefix;
            plan.selected |= entity.key_fields();
            plan.fixed_components = path.fixed;
        }
        if request.distinct {
            plan.distinct = distinct::admit(
                entity,
                request,
                plan.projection,
                plan.index,
                plan.fixed_components,
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
            hash.update((term.operand.len() as u32).to_le_bytes());
            hash.update(&term.operand);
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
    let mut entries = map.range(bounds);
    let mut collected = if plan.distinct == distinct::Strategy::Complete {
        Some(distinct::Collection::new(
            plan.projection.ok_or(Error::InvalidQuery)?,
        ))
    } else {
        None
    };
    let mut component = Vec::new();
    // Each processed equivalence class is either wholly rejected or already
    // emitted. Inclusive byte retries point at an unreturned class instead.
    let mut previous = if let Some(cursor) = request.cursor.filter(|c| !c.inclusive) {
        distinct::boundary(entity, plan.index, plan.distinct, &cursor.position)?.map(<[u8]>::to_vec)
    } else {
        None
    };
    while let Some(entry) = if request.descending {
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
            let owner = RowKey::decode(&owner_bytes, entity.key_type()?)?;
            let bytes = map.get(&row_key(entity.id, owner)).ok_or(Error::Corrupt)?;
            (owner, bytes)
        } else {
            (
                RowKey::decode(&stored_key[5..], entity.key_type()?)?,
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
            (plan.presence != 0
                || plan.projection.is_some()
                || aggregate.as_ref().is_some_and(|a| a.needs_spans()))
            .then_some(&mut spans),
        )?;
        let primary = entity
            .fields
            .iter()
            .position(|f| f.name == entity.primary_key)
            .ok_or(Error::Corrupt)?;
        if RowKey::from_index(values[primary])? != owner {
            return Err(Error::Corrupt);
        }
        if plan.index.is_some() {
            validate_index_entry(entity, &entity.row_info(&values)?, owner, stored_key)?;
        }
        let matches = plan
            .terms
            .split_inclusive(|term| !term.or_next)
            .all(|clause| {
                clause.iter().any(|term| {
                    if plan.presence & (1 << term.position) != 0 {
                        // Admission permits only Eq/Ne(Absent) here. Full decoding
                        // validated this optional tag and the entire present value.
                        let absent = bytes[usize::from(spans[term.position].0)] == 0;
                        return term.comparison.accepts_absent(absent);
                    }
                    let value = values[term.position];
                    component.clear();
                    value.append_component(entity.fields[term.position].optional, &mut component);
                    term.comparison.accepts(
                        (value != IndexValue::Absent).then_some(component.as_slice()),
                        (!term.absent).then_some(term.operand.as_slice()),
                        entity.fields[term.position].optional,
                    )
                })
            });
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
            if (aggregate.is_some() && input_bytes.is_none()) || collected.is_some() {
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
    Ok(page)
}
