//! Bounded fluent traversal. Typed result construction stays outside shared execution.
mod aggregate;
mod distinct;
mod order;
pub use aggregate::jobs::{AggregateJobs, GroupCursor, GroupPage, JobHandle, JobOutcome, JobStep};
pub use aggregate::{Aggregate, GroupCount, GroupOrder, GroupValue};

use super::{Database, Map, prefix, row_key, validate_index_entry};
use crate::{
    Entity, EntitySchema, Error, IndexSchema, IndexValue, TypeBody, TypeSchema, Value,
    codec::Decoder, key, primary::RowKey, store_key::StoreKey, types::TypeGraph,
};
use ic_storage::Memory;
use sha2::{Digest, Sha256};
use std::{cmp::Ordering, marker::PhantomData, ops::Bound};

const MAX_TERMS: usize = 8;
const MAX_WORK: usize = 512;
const MAX_OUTPUT: usize = 128 * 1024;

/// Exact comparison. Equality distinguishes absent values; ranges require presence.
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
}

impl Comparison {
    fn accepts(self, order: Ordering, absent: bool) -> bool {
        match self {
            Self::Eq => order == Ordering::Equal,
            Self::Ne => order != Ordering::Equal,
            _ if absent => false,
            Self::Lt => order == Ordering::Less,
            Self::Le => order != Ordering::Greater,
            Self::Gt => order == Ordering::Greater,
            Self::Ge => order != Ordering::Less,
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
    const MAX_ENCODED_BYTES: usize = 34 + crate::MAX_INDEX_KEY_BYTES;

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

/// A live bounded page. Empty rows do not mean completion when `next` is present.
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

struct Predicate<'a> {
    field: &'a str,
    comparison: Comparison,
    value: IndexValue<'a>,
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
            request: Request {
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
            },
        }
    }
}
impl<'a, E: Entity, M: Memory> Query<'a, E, M> {
    /// Remove duplicate output records across pages. Identity projections are
    /// already unique. Other projections require a leading index tuple after
    /// fixed prefix components. Filters may reference only projected fields or
    /// fixed prefix fields; unsupported shapes refuse. This is live enumeration,
    /// not an exact aggregate snapshot.
    #[must_use]
    pub fn distinct(mut self) -> Self {
        self.request.distinct = true;
        self
    }
    /// Add an AND term over an exact top-level ordered scalar/unit-enum field.
    /// All operands are admitted before storage reads, including contradictory terms.
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
            });
        }
        self
    }
    /// Select index order and exact leading components (at most four).
    /// All rows matching this prefix are considered. Sparse indexes require
    /// every optional indexed component to be covered by a present prefix operand.
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
        Ok(QueryPage {
            rows: raw
                .rows
                .iter()
                .map(|bytes| E::decode(bytes))
                .collect::<Result<_, _>>()?,
            next: raw.next,
            examined: raw.examined,
            fetched: raw.fetched,
            output_bytes: raw.output_bytes,
        })
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
}

// One owner for copying already validated projection spans into a record codec.
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
    terms: Vec<Term>,
    selected: u16,
    binding: [u8; 32],
    projection: Option<u16>,
    distinct_components: Option<usize>,
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
            terms: Vec::new(),
            selected: 1 << primary,
            binding: [0; 32],
            projection: projection
                .map(|output| admit_projection(entity, output))
                .transpose()?,
            distinct_components: None,
        };
        for predicate in &request.predicates {
            let position = entity
                .fields
                .iter()
                .position(|f| f.name == predicate.field)
                .ok_or(Error::InvalidQuery)?;
            let field = &entity.fields[position];
            if !key::supports(field.value) {
                return Err(Error::InvalidQuery);
            }
            predicate.value.admit(field.value, field.optional)?;
            let absent = predicate.value == IndexValue::Absent;
            if absent && !matches!(predicate.comparison, Comparison::Eq | Comparison::Ne) {
                return Err(Error::InvalidOperand);
            }
            let mut operand = Vec::new();
            predicate
                .value
                .append_component(field.optional, &mut operand);
            plan.terms.push(Term {
                position,
                comparison: predicate.comparison,
                operand,
            });
            plan.selected |= 1 << position;
        }
        // Admit every operand before selecting a path, even for empty or
        // contradictory queries. The scanner consumes this one proven choice.
        let fixed = if let Some(path) = order::select(entity, request)? {
            plan.index = Some(path.index);
            plan.prefix = path.prefix;
            plan.selected |= entity.key_fields();
            path.fixed
        } else {
            0
        };
        if request.distinct {
            plan.distinct_components =
                distinct::admit(entity, request, plan.projection, plan.index, fixed)?;
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
    let mut upper = plan.prefix.clone();
    // Namespace prefixes always have a successor; no unbounded reverse scan.
    while upper.last() == Some(&255) {
        upper.pop();
    }
    let last = upper.last_mut().ok_or(Error::InvalidQuery)?;
    *last += 1;
    let mut lower = Bound::Included(StoreKey::new(plan.prefix.clone())?);
    let mut upper = Bound::Excluded(StoreKey::new(upper)?);
    if let Some(cursor) = request.cursor {
        let key = StoreKey::new(cursor.position.clone())?;
        let boundary = if cursor.inclusive {
            Bound::Included(key)
        } else {
            Bound::Excluded(key)
        };
        if request.descending {
            upper = boundary;
        } else {
            lower = boundary;
        }
    }
    let mut entries = map.range((lower, upper));
    let mut page = QueryPage {
        rows: Vec::new(),
        next: None,
        examined: 0,
        fetched: 0,
        output_bytes: 0,
    };
    let mut component = Vec::new();
    // Each processed equivalence class is either wholly rejected or already
    // emitted. Inclusive byte retries point at an unreturned class instead.
    let mut previous = if let Some(cursor) = request.cursor.filter(|c| !c.inclusive) {
        distinct::boundary(
            entity,
            plan.index,
            plan.distinct_components,
            &cursor.position,
        )?
        .map(<[u8]>::to_vec)
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
            (plan.projection.is_some() || aggregate.as_ref().is_some_and(|a| a.needs_spans()))
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
        let matches = plan.terms.iter().all(|term| {
            let value = values[term.position];
            component.clear();
            value.append_component(entity.fields[term.position].optional, &mut component);
            let order = component.cmp(&term.operand);
            term.comparison.accepts(order, value == IndexValue::Absent)
        });
        let boundary =
            distinct::boundary(entity, plan.index, plan.distinct_components, stored_key)?;
        let duplicate = boundary.is_some_and(|key| previous.as_deref() == Some(key));
        if matches && let Some(accumulator) = aggregate.as_mut() {
            accumulator.push(entity, &values, &bytes, &spans)?;
        } else if matches && !duplicate {
            let size = plan.projection.map_or(bytes.len(), |mask| {
                spans
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| mask & (1 << i) != 0)
                    .map(|(_, (start, end))| usize::from(end - start))
                    .sum()
            });
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
            if aggregate.is_some() && input_bytes.is_none() {
                // Completion has not been observed. Never expose prefix aggregates;
                // an exact-boundary scan may conservatively require more budget.
                return Err(Error::Limit);
            }
            page.next = Some(plan.cursor(stored_key, false));
            return Ok(page);
        }
    }
    Ok(page)
}
