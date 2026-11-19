//! Select a proven ordered path or a bounded complete-sort fallback.
use super::{Comparison, Predicate, QueryOperand, Request};
use crate::{EntitySchema, Error, IndexSchema, IndexValue, key, store::admit_index_prefix};

// Carry the admitted prefix and its component count together; downstream proofs
// must not reconstruct equality-prefix selection from the request independently.
pub(super) struct IndexPath<'a> {
    pub index: &'a IndexSchema,
    pub prefix: Vec<u8>,
    pub fixed: usize,
}

// Failure to encode a derived prefix disqualifies this candidate only. The
// original predicates have already been admitted and remain on the plan.
fn automatic_prefix<'a>(
    entity: &EntitySchema,
    index: &'a IndexSchema,
    values: &[IndexValue<'_>],
) -> Result<Option<IndexPath<'a>>, Error> {
    if index.present_only && values.contains(&IndexValue::Absent) {
        return Ok(None);
    }
    match admit_index_prefix(entity, index, values) {
        Ok(prefix) => Ok(Some(IndexPath {
            index,
            prefix,
            fixed: values.len(),
        })),
        Err(Error::Limit) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) enum Path<'a> {
    Primary,
    Index(IndexPath<'a>),
    Sort(Vec<(usize, bool)>),
}

pub(super) fn select<'a>(
    entity: &'a EntitySchema,
    request: &Request<'_>,
    output: super::PlanOutput,
) -> Result<Path<'a>, Error> {
    // Plan admission bounds the order to four fields. Indexed paths allocate no
    // additional sort metadata; materialize it only for the complete fallback.
    let mut positions = [(0, false); 4];
    if let Some(order) = &request.order {
        if order.is_empty() {
            return Err(Error::InvalidQuery);
        }
        for (i, name) in order.iter().enumerate() {
            let position = entity.field_position(name).ok_or(Error::InvalidQuery)?;
            let field = &entity.fields[position];
            if !key::supports(field.value)
                || order[..i].contains(name)
                || (*name == entity.primary_key && i + 1 != order.len())
            {
                return Err(Error::InvalidQuery);
            }
            positions[i] = (position, request.order_descending & (1 << i) != 0);
        }
    }
    let single_row = equality(request, entity.primary_key).is_some();
    let fits_order = |index, fixed| single_row || satisfies(entity, request, index, fixed);
    if let Some((name, values)) = &request.index {
        let index = entity
            .indexes
            .iter()
            .find(|i| i.name == *name)
            .ok_or(Error::InvalidQuery)?;
        if values.len() > index.fields.len() {
            return Err(Error::InvalidQuery);
        }
        let prefix = admit_index_prefix(entity, index, values)?;
        let exact_owner = matches!(output, super::PlanOutput::Rows)
            && !request.distinct
            && index.is_unique_lookup(values.len());
        if !complete_membership(entity, index, values, request)
            || !(request.order.is_none() || exact_owner || fits_order(Some(index), values.len()))
        {
            return Err(Error::InvalidQuery);
        }
        return Ok(Path::Index(IndexPath {
            index,
            prefix,
            fixed: values.len(),
        }));
    }
    // Any valid order holds for zero or one owner. A resumed query keeps the
    // physical path bound to its cursor; DISTINCT and aggregates own their paths.
    if matches!(output, super::PlanOutput::Rows)
        && !request.distinct
        && request.cursor.is_none()
        && let Some(path) = unique_lookup(entity, request)?
    {
        return Ok(Path::Index(path));
    }
    // A mandatory primary equality admits at most one row under any valid order.
    // Projected DISTINCT retains its path-dependent continuation strategy.
    // Before 0.23.13 a default-order page could only resume on the primary
    // path. The cursor still has to pass the complete binding and key checks.
    let default_primary = request.order.is_none()
        && (!matches!(output, super::PlanOutput::Rows)
            || request.distinct
            || request
                .cursor
                .is_some_and(|c| c.position.first() == Some(&2))
            || literal_equalities(request).next().is_none());
    if default_primary || (single_row && !request.distinct) {
        return Ok(Path::Primary);
    }
    let primary_fits = fits_order(None, 0);
    let mut best: Option<IndexPath<'_>> = None;
    for index in entity.indexes {
        let values = leading_equalities(request, index);
        if (primary_fits && values.is_empty())
            || !complete_membership(entity, index, &values, request)
            || !fits_order(Some(index), values.len())
        {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|old| rank(index, values.len()) < rank(old.index, old.fixed))
            && let Some(path) = automatic_prefix(entity, index, &values)?
        {
            best = Some(path);
        }
    }
    // A restrictive prefix avoids unrelated candidates. Otherwise prefer direct
    // primary traversal to fetching rows through an equally broad index scan.
    if let Some(path) = best {
        return Ok(Path::Index(path));
    }
    if primary_fits {
        return Ok(Path::Primary);
    }

    // Complete sorting never accepts a live cursor. DISTINCT separately
    // proves projected equivalence after this access path is admitted.
    if request.cursor.is_some() {
        return Err(Error::InvalidQuery);
    }
    let mut positions =
        positions[..request.order.as_ref().ok_or(Error::InvalidQuery)?.len()].to_vec();
    let primary = entity
        .field_position(entity.primary_key)
        .ok_or(Error::InvalidQuery)?;
    if positions.last().map(|(position, _)| *position) != Some(primary) {
        positions.push((primary, false));
    }
    Ok(Path::Sort(positions))
}

/// A full unique key has at most one owner, so using it preserves primary order.
fn unique_lookup<'a>(
    entity: &'a EntitySchema,
    request: &Request<'_>,
) -> Result<Option<IndexPath<'a>>, Error> {
    if request.predicates.is_empty() || equality(request, entity.primary_key).is_some() {
        return Ok(None);
    }
    for index in entity
        .indexes
        .iter()
        .filter(|i| i.unique && i.fields.len() <= request.predicates.len())
    {
        let values = leading_equalities(request, index);
        if index.is_unique_lookup(values.len())
            && let Some(path) = automatic_prefix(entity, index, &values)?
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

// Only supplied prefix components have a fixed value.
pub(super) fn prefix_value<'a>(
    index: &IndexSchema,
    values: &[IndexValue<'a>],
    field: &str,
) -> Option<IndexValue<'a>> {
    index
        .fields
        .iter()
        .zip(values)
        .find_map(|(name, value)| (*name == field).then_some(*value))
}

/// Select the aggregate input path and report whether its keys prove every predicate.
pub(super) fn select_aggregate_prefix<'a>(
    entity: &'a EntitySchema,
    request: &Request<'_>,
    plan: &mut super::Plan<'a>,
) -> Result<bool, Error> {
    if let Some((_, values)) = &request.index {
        return Ok(plan.index.is_some_and(|index| {
            covers_index(entity, request, plan, index, values, plan.prefix.len())
        }));
    }
    let exact_id = equality(request, entity.primary_key).is_some();
    let mut best = None;
    for index in entity.indexes {
        let values = leading_equalities(request, index);
        if !complete_membership(entity, index, &values, request) {
            continue;
        }
        let Some(path) = automatic_prefix(entity, index, &values)? else {
            continue;
        };
        let covered = covers_index(entity, request, plan, index, &values, path.prefix.len());
        let single_owner = index.is_unique_lookup(path.fixed);
        // The exact key proves exhaustion in one visit. If it also covers
        // every predicate, no other candidate can improve the work bound.
        if single_owner && covered {
            plan.use_index(entity, path);
            return Ok(true);
        }
        if !covered
            && (exact_id
                || (!single_owner && !super::range::preserves_primary_bounds(entity, plan, &path)))
        {
            continue;
        }
        let (fixed, width, name) = rank(index, path.fixed);
        let bounded = super::range::has_secondary_bound(entity, plan, &path);
        if values.is_empty() && !bounded {
            continue;
        }
        let score = (!single_owner, !covered, fixed, !bounded, width, name);
        if best.as_ref().is_none_or(|(old, _)| score < *old) {
            best = Some((score, (path, covered)));
        }
    }
    if let Some((_, (path, covered))) = best {
        plan.use_index(entity, path);
        return Ok(covered);
    }
    Ok(false)
}

// Plan admission emits one encoded term per predicate, in request order.
fn covers_index(
    entity: &EntitySchema,
    request: &Request<'_>,
    plan: &super::Plan<'_>,
    index: &IndexSchema,
    values: &[IndexValue<'_>],
    prefix_len: usize,
) -> bool {
    request.predicates.iter().zip(&plan.terms).all(|(p, term)| {
        let fixed_equality = p.comparison == Comparison::Eq
            && matches!(p.operand, QueryOperand::Literal(v) if prefix_value(index, values, p.field) == Some(v));
        !p.or_next
            && (fixed_equality
                || super::range::bounds_index_term(entity, index, values.len(), term, prefix_len))
    })
}

fn leading_equalities<'a>(request: &Request<'a>, index: &IndexSchema) -> Vec<IndexValue<'a>> {
    index
        .fields
        .iter()
        .map_while(|field| equality(request, field))
        .collect()
}

// Prefer a longer fixed prefix, then fewer fields, then the logical index name.
fn rank(index: &IndexSchema, fixed: usize) -> (std::cmp::Reverse<usize>, usize, &str) {
    (std::cmp::Reverse(fixed), index.fields.len(), index.name)
}

// Both operands have already passed admission. For a field operand, a row
// omitted by this sparse index can have a present or an absent counterpart.
fn requires_presence(predicate: &Predicate<'_>, name: &str, entity: &EntitySchema) -> bool {
    match predicate.operand {
        QueryOperand::Length(_) => {
            predicate.field == name && !predicate.comparison.accepts_absent(false)
        }
        QueryOperand::Literal(value) => {
            predicate.field == name
                && !predicate
                    .comparison
                    .accepts_absent(value == IndexValue::Absent)
        }
        QueryOperand::Field(right) => {
            let other = if predicate.field == name {
                right
            } else if right == name {
                predicate.field
            } else {
                return false;
            };
            !predicate.comparison.accepts_absent(false)
                && (!predicate.comparison.accepts_absent(true)
                    || entity
                        .fields
                        .iter()
                        .any(|field| field.name == other && !field.optional))
        }
    }
}

// One mandatory clause must reject absence in every alternative.
pub(super) fn requires_present_field(
    entity: &EntitySchema,
    request: &Request<'_>,
    name: &str,
) -> bool {
    request.clauses().any(|clause| {
        clause
            .iter()
            .all(|predicate| requires_presence(predicate, name, entity))
    })
}

// Sparse traversal must cover every possible match. Each optional component needs
// a present prefix operand or a mandatory clause whose alternatives all reject
// absence on that same field. Presence alone never makes an ordering field fixed.
fn complete_membership(
    entity: &EntitySchema,
    index: &IndexSchema,
    values: &[IndexValue<'_>],
    request: &Request<'_>,
) -> bool {
    !index.present_only
        || index.fields.iter().enumerate().all(|(i, name)| {
            entity
                .fields
                .iter()
                .find(|f| f.name == *name)
                .is_some_and(|field| {
                    !field.optional
                        || values.get(i).is_some_and(|v| *v != IndexValue::Absent)
                        || requires_present_field(entity, request, name)
                })
        })
}

fn satisfies(
    entity: &EntitySchema,
    request: &Request<'_>,
    index: Option<&IndexSchema>,
    prefix_len: usize,
) -> bool {
    // An empty requested field list leaves just the primary-key tie-breaker.
    let order = request.order.as_deref().unwrap_or_default();
    let fields = index.map_or(&[][..], |i| i.fields);
    let fixed =
        |name: &&str| fields[..prefix_len].contains(name) || equality(request, name).is_some();
    // A fixed primary key admits at most one record, so every admitted order fits.
    if fields[..prefix_len].contains(&entity.primary_key) {
        return true;
    }
    // Current physical keys share one direction. Constant fields do not affect
    // ordering, but every varying field must agree with the primary tie-breaker.
    // The ordinary all-ascending order already has uniform directions. For
    // explicit directions, only a disagreement needs a fixed-field proof.
    if request.order_descending != 0 {
        let descending = request.primary_descending(entity);
        if order.iter().enumerate().any(|(i, name)| {
            (request.order_descending & (1 << i) != 0) != descending && !fixed(name)
        }) {
            return false;
        }
    }
    // The primary key uniquely determines all later components. Compare both
    // sequences only up to it, stripping fields constant across matching rows.
    let wanted = order
        .iter()
        .copied()
        .take_while(|name| *name != entity.primary_key)
        .filter(|name| !fixed(name))
        .chain(std::iter::once(entity.primary_key));
    let supplied = fields
        .iter()
        .copied()
        .take_while(|name| *name != entity.primary_key)
        .filter(|name| !fixed(name))
        .chain(std::iter::once(entity.primary_key));
    wanted.eq(supplied)
}

// Only singleton equality clauses are mandatory. An equality inside an OR
// must not restrict an index prefix or prove that an ordering field is constant.
pub(super) fn literal_equalities<'r, 'a>(
    request: &'r Request<'a>,
) -> impl Iterator<Item = (&'a str, IndexValue<'a>)> + Clone + 'r {
    request.clauses().filter_map(|clause| {
        if let [predicate] = clause
            && predicate.comparison == Comparison::Eq
            && let QueryOperand::Literal(value) = predicate.operand
        {
            Some((predicate.field, value))
        } else {
            None
        }
    })
}

fn equality<'a>(request: &Request<'a>, field: &str) -> Option<IndexValue<'a>> {
    literal_equalities(request)
        .find(|(name, _)| *name == field)
        .map(|(_, value)| value)
}
