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
) -> Result<Path<'a>, Error> {
    // Plan admission bounds the order to four fields. Indexed paths allocate no
    // additional sort metadata; materialize it only for the complete fallback.
    let mut positions = [(0, false); 4];
    if let Some(order) = &request.order {
        if order.is_empty() {
            return Err(Error::InvalidQuery);
        }
        for (i, name) in order.iter().enumerate() {
            let position = entity
                .fields
                .iter()
                .position(|f| f.name == *name)
                .ok_or(Error::InvalidQuery)?;
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
    let single_row = request.order.is_some() && equality(request, entity.primary_key).is_some();
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
        if !complete_membership(entity, index, values, request)
            || !fits_order(Some(index), values.len())
        {
            return Err(Error::InvalidQuery);
        }
        return Ok(Path::Index(IndexPath {
            index,
            prefix,
            fixed: values.len(),
        }));
    }
    // Keep the existing no-order contract and its cursors unchanged.
    // A mandatory primary equality admits at most one row under any valid order.
    // Projected DISTINCT retains its path-dependent continuation strategy.
    if request.order.is_none() || (single_row && !request.distinct) {
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
        .fields
        .iter()
        .position(|f| f.name == entity.primary_key)
        .ok_or(Error::InvalidQuery)?;
    if positions.last().map(|(position, _)| *position) != Some(primary) {
        positions.push((primary, false));
    }
    Ok(Path::Sort(positions))
}

// A fixed component implies an equality only when its selected value agrees.
fn fixed_equality(
    predicate: &Predicate<'_>,
    index: &IndexSchema,
    values: &[IndexValue<'_>],
) -> bool {
    predicate.comparison == Comparison::Eq
        && index.fields.iter().zip(values).any(|(name, value)| {
            *name == predicate.field
                && matches!(predicate.operand, QueryOperand::Literal(v) if v == *value)
        })
}

/// Select the aggregate input path and report whether its keys prove every predicate.
pub(super) fn select_aggregate_prefix<'a>(
    entity: &'a EntitySchema,
    request: &Request<'_>,
    plan: &mut super::Plan<'a>,
) -> Result<bool, Error> {
    if let Some((_, values)) = &request.index {
        return Ok(plan
            .index
            .is_some_and(|index| covers_index(request, plan, index, values)));
    }
    // Preserve primary seeks when the equality prefix would leave an ID predicate
    // to be checked only after fetching owners.
    let allow_residual = !request
        .predicates
        .iter()
        .any(|p| p.field == entity.primary_key);
    let mut best = None;
    for index in entity.indexes {
        let values = leading_equalities(request, index);
        let covered = request
            .predicates
            .iter()
            .all(|p| !p.or_next && fixed_equality(p, index, &values));
        if (!covered && !allow_residual) || !complete_membership(entity, index, &values, request) {
            continue;
        }
        let Some(path) = automatic_prefix(entity, index, &values)? else {
            continue;
        };
        if covered {
            plan.use_index(entity, path);
            return Ok(true);
        }
        let (fixed, width, name) = rank(index, path.fixed);
        let bounded = super::range::has_secondary_bound(entity, plan, &path);
        if values.is_empty() && !bounded {
            continue;
        }
        let score = (fixed, !bounded, width, name);
        if best.as_ref().is_none_or(|(old, _)| score < *old) {
            best = Some((score, (path, values)));
        }
    }
    if let Some((_, (path, values))) = best {
        let index = path.index;
        plan.use_index(entity, path);
        return Ok(covers_index(request, plan, index, &values));
    }
    Ok(false)
}

// Plan admission emits one encoded term per predicate, in request order.
fn covers_index(
    request: &Request<'_>,
    plan: &super::Plan<'_>,
    index: &IndexSchema,
    values: &[IndexValue<'_>],
) -> bool {
    let next = index.fields.get(values.len()).copied();
    request.predicates.iter().zip(&plan.terms).all(|(p, term)| {
        !p.or_next
            && (fixed_equality(p, index, values)
                || super::range::bounds_secondary_field(p.field, next, term, plan.prefix.len()))
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
                        || request
                            .predicates
                            .split_inclusive(|p| !p.or_next)
                            .any(|clause| {
                                clause
                                    .iter()
                                    .all(|predicate| requires_presence(predicate, name, entity))
                            })
                })
        })
}

fn satisfies(
    entity: &EntitySchema,
    request: &Request<'_>,
    index: Option<&IndexSchema>,
    prefix_len: usize,
) -> bool {
    let Some(order) = &request.order else {
        return true;
    };
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
fn equality<'a>(request: &Request<'a>, field: &str) -> Option<IndexValue<'a>> {
    request
        .predicates
        .split_inclusive(|p| !p.or_next)
        .find_map(|clause| {
            if let [predicate] = clause
                && predicate.field == field
                && predicate.comparison == Comparison::Eq
                && let QueryOperand::Literal(value) = predicate.operand
            {
                Some(value)
            } else {
                None
            }
        })
}
