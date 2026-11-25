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

#[derive(Clone, Copy)]
pub(super) struct SortField {
    pub position: usize,
    pub member: Option<crate::member::RecordMember>,
    pub descending: bool,
}

pub(super) enum Path<'a> {
    Primary,
    Index(IndexPath<'a>, bool),
    Filtered { sort: Option<Vec<SortField>> },
}

pub(super) fn select<'a>(
    entity: &'a EntitySchema,
    request: &Request<'_>,
    output: super::PlanOutput,
) -> Result<Path<'a>, Error> {
    // Unordered aggregates choose their filtered access path after predicate
    // admission. Only an explicit index needs order admission at this stage.
    if request.order.is_none()
        && request.index.is_none()
        && !matches!(output, super::PlanOutput::Rows)
    {
        return Ok(Path::Primary);
    }
    // Plan admission bounds the order to four fields. Indexed paths allocate no
    // additional sort metadata; materialize it only for the complete fallback.
    let mut positions = [SortField {
        position: 0,
        member: None,
        descending: false,
    }; 4];
    if let Some(order) = &request.order {
        if order.is_empty() {
            return Err(Error::InvalidQuery);
        }
        for (i, name) in order.iter().enumerate() {
            let (position, member) = crate::member::resolve_field(entity, name)?;
            if (member.is_none() && !key::supports(entity.fields[position].value))
                || order[..i].contains(name)
                || (*name == entity.primary_key && i + 1 != order.len())
            {
                return Err(Error::InvalidQuery);
            }
            positions[i] = SortField {
                position,
                member,
                descending: request.order_descending & (1 << i) != 0,
            };
        }
    }
    let single_row = equality(entity, request, entity.primary_key).is_some();
    let primary_direction = request.primary_descending(entity);
    let direction = |index, fixed| {
        if single_row {
            Some(primary_direction)
        } else {
            scan_direction(entity, request, index, fixed, primary_direction)
        }
    };
    if let Some((name, values)) = &request.index {
        let index = entity.index_by_name(name).ok_or(Error::InvalidQuery)?;
        if values.len() > index.fields.len() {
            return Err(Error::InvalidQuery);
        }
        let prefix = admit_index_prefix(entity, index, values)?;
        let exact_owner =
            matches!(output, super::PlanOutput::Rows) && index.is_unique_lookup(values.len());
        if !complete_membership(entity, index, index.fields, values, request) {
            return Err(Error::InvalidQuery);
        }
        let descending = if request.order.is_none() || exact_owner {
            primary_direction
        } else {
            direction(Some(index), values.len()).ok_or(Error::InvalidQuery)?
        };
        return Ok(Path::Index(
            IndexPath {
                index,
                prefix,
                fixed: values.len(),
            },
            descending,
        ));
    }
    // No secondary prefix can narrow a direct primary-key seek. Resumed DISTINCT
    // pages may already be bound to a secondary traversal.
    if single_row && (!request.distinct || request.cursor.is_none()) {
        return Ok(Path::Primary);
    }
    // Any valid order holds for zero or one owner. A resumed query keeps the
    // physical path bound to its cursor. Aggregates select their own paths.
    if matches!(output, super::PlanOutput::Rows)
        && request.cursor.is_none()
        && !request.predicates.is_empty()
        && let Some(path) = unique_lookup(entity, request)?
    {
        return Ok(Path::Index(path, primary_direction));
    }
    let default_primary = request.order.is_none()
        && (request.distinct || request.fixed_equalities(entity).next().is_none());
    if default_primary {
        return Ok(Path::Primary);
    }
    let primary_fits = direction(None, 0).is_some();
    let mut best: Option<(IndexPath<'_>, bool)> = None;
    for index in entity.indexes {
        let values = leading_equalities(entity, request, index);
        if (primary_fits && values.is_empty())
            || !complete_membership(entity, index, index.fields, &values, request)
        {
            continue;
        }
        let Some(descending) = direction(Some(index), values.len()) else {
            continue;
        };
        if best
            .as_ref()
            .is_none_or(|(old, _)| rank(index, values.len()) < rank(old.index, old.fixed))
            && let Some(path) = automatic_prefix(entity, index, &values)?
        {
            best = Some((path, descending));
        }
    }
    // A restrictive prefix avoids unrelated candidates. Otherwise prefer direct
    // primary traversal to fetching rows through an equally broad index scan.
    if let Some((path, descending)) = best {
        return Ok(Path::Index(path, descending));
    }
    if primary_fits {
        return Ok(Path::Primary);
    }

    // A live cursor or a retained input scan must advance along one physical order.
    // Only a fresh row query can fall back to a complete sort.
    if request.cursor.is_some() || !matches!(output, super::PlanOutput::Rows) {
        return Err(Error::InvalidQuery);
    }
    // For try_one, any second owner proves multiplicity and the sole owner
    // is independent of order. DISTINCT may still need to compare owners.
    if request.single_result_probe == Some(super::SingleResult::Only) && !request.distinct {
        return Ok(Path::Filtered { sort: None });
    }
    // Every requested field has been admitted. Equal components contribute
    // neither ordering nor tie-breaking bytes to the complete sort.
    let order = request.order.as_ref().ok_or(Error::InvalidQuery)?;
    let mut varying = Vec::with_capacity(order.len() + 1);
    for (i, name) in order.iter().enumerate() {
        if equality(entity, request, name).is_none() {
            varying.push(positions[i]);
        }
    }
    let primary = entity
        .field_position(entity.primary_key)
        .ok_or(Error::InvalidQuery)?;
    if varying.last().map(|field| field.position) != Some(primary) {
        varying.push(SortField {
            position: primary,
            member: None,
            descending: false,
        });
    }

    Ok(Path::Filtered {
        sort: Some(varying),
    })
}

/// A full unique key has at most one owner, so using it preserves primary order.
fn unique_lookup<'a>(
    entity: &'a EntitySchema,
    request: &Request<'_>,
) -> Result<Option<IndexPath<'a>>, Error> {
    for index in entity
        .indexes
        .iter()
        .filter(|i| i.unique && i.fields.len() <= request.predicates.len())
    {
        let values = leading_equalities(entity, request, index);
        if index.is_unique_lookup(values.len())
            && let Some(path) = automatic_prefix(entity, index, &values)?
        {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

// Synchronous value aggregates may seek one proven owner without relying on
// selectivity estimates. Retained jobs keep the traversal bound to their cursor.
pub(super) fn select_unique_aggregate_path<'a>(
    entity: &'a EntitySchema,
    request: &Request<'_>,
    plan: &mut super::Plan<'a>,
) -> Result<(), Error> {
    if plan.index.is_none()
        && !request.predicates.is_empty()
        && equality(entity, request, entity.primary_key).is_none()
        && let Some(path) = unique_lookup(entity, request)?
    {
        plan.use_index(entity, path);
    }
    Ok(())
}

// Only supplied prefix components have a fixed value.
fn prefix_value<'a>(
    fields: &[&str],
    values: &[IndexValue<'a>],
    field: &str,
) -> Option<IndexValue<'a>> {
    fields
        .iter()
        .zip(values)
        .find_map(|(name, value)| (*name == field).then_some(*value))
}

/// Select a complete input range before reduction or sorting. Predicate coverage
/// saves owner reads only when the terminal can consume keys alone.
pub(super) fn select_filtered_path<'a>(
    entity: &'a EntitySchema,
    request: &Request<'_>,
    plan: &mut super::Plan<'a>,
    probe: Option<super::probe::MemberProbe>,
) -> Result<bool, Error> {
    if let Some((_, values)) = &request.index {
        return Ok(plan.index.is_some_and(|index| {
            covers_keys(
                entity,
                request,
                plan,
                Some(index),
                values,
                plan.prefix.len(),
            )
        }));
    }
    let primary = equality(entity, request, entity.primary_key);
    if covers_keys(
        entity,
        request,
        plan,
        None,
        primary.as_slice(),
        plan.prefix.len(),
    ) {
        return Ok(true);
    }
    let exact_id = primary.is_some();
    let can_probe = probe.is_some();
    let mut best = None;
    for index in entity.indexes {
        let values = leading_equalities(entity, request, index);
        if !complete_membership(entity, index, index.fields, &values, request) {
            continue;
        }
        let Some(path) = automatic_prefix(entity, index, &values)? else {
            continue;
        };
        let covered = probe == Some(super::probe::MemberProbe::CountWithRows)
            && covers_keys(
                entity,
                request,
                plan,
                Some(index),
                &values,
                path.prefix.len(),
            );
        let single_owner = index.is_unique_lookup(path.fixed);
        // The exact key proves exhaustion in one visit. If it also covers
        // every predicate, no other candidate can improve the work bound.
        if single_owner && covered {
            plan.use_index(entity, path);

            return Ok(true);
        }
        if !covered
            && (exact_id
                || (!single_owner
                    && !super::range::preserves_primary_bounds(entity, plan, &path, can_probe)))
        {
            continue;
        }
        let (fixed, width, name) = rank(index, path.fixed);
        let bounded = super::range::has_secondary_bound(entity, plan, &path, can_probe);
        // A complete sparse traversal already skips rows with absent components.
        if values.is_empty() && !bounded && !covered && !index.present_only {
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
fn covers_keys(
    entity: &EntitySchema,
    request: &Request<'_>,
    plan: &super::Plan<'_>,
    index: Option<&IndexSchema>,
    values: &[IndexValue<'_>],
    prefix_len: usize,
) -> bool {
    let fields = index.map_or(std::slice::from_ref(&entity.primary_key), |index| {
        index.fields
    });

    request
        .clauses()
        .zip(plan.clauses())
        .all(|(clause, terms)| {
            let key_proves_clause = clause.iter().zip(terms).any(|(p, term)| {
                // Every entry in an admitted sparse path has all its components.
                (term.tests_presence
                    && (entity.field_is_required(p.field)
                        || index.is_some_and(|i| i.present_only && i.fields.contains(&p.field))))
                    || fixed_comparison(entity, p, &|field| prefix_value(fields, values, field))
                        == Some(true)
            });

            key_proves_clause
                || super::range::key_range_proves_clause(
                    entity,
                    index,
                    values.len(),
                    terms,
                    prefix_len,
                )
        })
}

fn leading_equalities<'a>(
    entity: &EntitySchema,
    request: &Request<'a>,
    index: &IndexSchema,
) -> Vec<IndexValue<'a>> {
    index
        .fields
        .iter()
        .map_while(|field| equality(entity, request, field))
        .collect()
}

// Prefer a longer fixed prefix, then fewer fields, then the logical index name.
fn rank(index: &IndexSchema, fixed: usize) -> (std::cmp::Reverse<usize>, usize, &str) {
    (std::cmp::Reverse(fixed), index.fields.len(), index.name)
}

// Test the predicate directly, without following field-to-field comparisons.
fn directly_requires_presence(predicate: &Predicate<'_>, name: &str) -> bool {
    if predicate.field != name {
        return false;
    }
    let other_absent = match predicate.operand {
        QueryOperand::Length(_) => false,
        QueryOperand::Literal(value) => value == IndexValue::Absent,
        QueryOperand::Field(_) => return false,
    };

    !predicate.comparison.accepts_absent(other_absent)
}

fn requires_presence<'a>(
    predicate: &Predicate<'_>,
    name: &str,
    entity: &EntitySchema,
    request: &Request<'_>,
    lookup: &dyn Fn(&str) -> Option<IndexValue<'a>>,
) -> bool {
    let Some(other) = predicate.compared_field(name) else {
        return directly_requires_presence(predicate, name);
    };

    if let Some(value) = lookup(other) {
        return !predicate
            .comparison
            .accepts_absent(value == IndexValue::Absent);
    }
    match predicate.comparison {
        Comparison::Eq => {
            entity.field_is_required(other)
                || presence_clause(entity, request, other, lookup, &|p| {
                    directly_requires_presence(p, other)
                })
        }
        Comparison::Ne => false,
        comparison => !comparison.accepts_absent(false) && !comparison.accepts_absent(true),
    }
}

// One mandatory clause must reject absence in every alternative.
pub(super) fn requires_present_field(
    entity: &EntitySchema,
    request: &Request<'_>,
    name: &str,
) -> bool {
    let lookup = |field: &str| equality(entity, request, field);
    presence_clause(entity, request, name, &lookup, &|p| {
        requires_presence(p, name, entity, request, &lookup)
    })
}

// Look through one field equality only. The peer must have its own direct
// presence constraint; equalities cannot supply evidence for each other.
fn presence_clause<'a>(
    entity: &EntitySchema,
    request: &Request<'_>,
    name: &str,
    lookup: &dyn Fn(&str) -> Option<IndexValue<'a>>,
    requires: &dyn Fn(&Predicate<'_>) -> bool,
) -> bool {
    request.clauses().any(|clause| {
        let mut needs_presence = false;
        for predicate in clause {
            if requires(predicate) {
                needs_presence = true;
                continue;
            }
            // An alternative on another field can still match an absent row.
            // Discard it only when the request itself fixes a contradictory value.
            if clause.len() == 1
                || predicate.field == name
                || fixed_comparison(entity, predicate, lookup) != Some(false)
            {
                return false;
            }
        }
        needs_presence
    })
}

// Sparse traversal must cover every possible match. Each optional component needs
// a present prefix operand or a mandatory clause whose alternatives all reject
// absence on that same field. Presence alone never makes an ordering field fixed.
pub(super) fn complete_membership(
    entity: &EntitySchema,
    index: &IndexSchema,
    prefix_fields: &[&str],
    values: &[IndexValue<'_>],
    request: &Request<'_>,
) -> bool {
    !index.present_only
        || index.fields.iter().all(|name| {
            entity.field_by_name(name).is_some_and(|field| {
                !field.optional
                    || prefix_value(prefix_fields, values, name)
                        .is_some_and(|v| v != IndexValue::Absent)
                    || requires_present_field(entity, request, name)
            })
        })
}

fn scan_direction(
    entity: &EntitySchema,
    request: &Request<'_>,
    index: Option<&IndexSchema>,
    prefix_len: usize,
    primary_direction: bool,
) -> Option<bool> {
    // An empty requested field list leaves just the primary-key tie-breaker.
    let order = request.order.as_deref().unwrap_or_default();
    let fields = index.map_or(&[][..], |i| i.fields);
    let fixed = |name: &&str| {
        fields[..prefix_len].contains(name) || equality(entity, request, name).is_some()
    };
    // A fixed primary key admits at most one record, so every admitted order fits.
    if fields[..prefix_len].contains(&entity.primary_key) {
        return Some(primary_direction);
    }
    // Once the requested order distinguishes a complete unique tuple, no
    // later term can break a tie. Only varying key components set the direction.
    if index.is_some_and(|i| i.unique) && !order.is_empty() {
        let mut wanted = order.iter().enumerate().filter(|(_, name)| !fixed(name));
        let mut direction = None;
        let matches = fields.iter().filter(|name| !fixed(name)).all(|name| {
            let Some((slot, requested)) = wanted.next() else {
                return false;
            };
            let descending = request.order_descending & (1 << slot) != 0;
            requested == name && *direction.get_or_insert(descending) == descending
        });
        if matches {
            return Some(direction.unwrap_or(primary_direction));
        }
    }
    // Current physical keys share one direction. Constant fields do not affect
    // ordering, but every varying field must agree with the primary tie-breaker.
    // The ordinary all-ascending order already has uniform directions. For
    // explicit directions, only a disagreement needs a fixed-field proof.
    if request.order_descending != 0 {
        let descending = primary_direction;
        if order.iter().enumerate().any(|(i, name)| {
            (request.order_descending & (1 << i) != 0) != descending && !fixed(name)
        }) {
            return None;
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

    wanted.eq(supplied).then_some(primary_direction)
}

// The caller supplies either known query values or just the selected key prefix.
pub(super) fn fixed_comparison<'a>(
    entity: &EntitySchema,
    predicate: &Predicate<'_>,
    lookup: &dyn Fn(&str) -> Option<IndexValue<'a>>,
) -> Option<bool> {
    if let Some(result) = predicate.constant_comparison(entity) {
        return Some(result);
    }
    let left = lookup(predicate.field)?;
    let right = match predicate.operand {
        QueryOperand::Length(threshold) => {
            let length = if left == IndexValue::Absent {
                None
            } else {
                Some(u32::try_from(super::scalar_bytes(left)?.len()).ok()?)
            };

            return Some(predicate.comparison.accepts_length(length, threshold));
        }
        QueryOperand::Literal(operand) => operand,
        QueryOperand::Field(field) => lookup(field)?,
    };

    predicate.comparison.accepts_fixed_values(left, right)
}

pub(super) fn fixed_prefix_value<'a>(
    entity: &EntitySchema,
    request: &Request<'a>,
    plan: &super::Plan<'_>,
    field: &str,
) -> Option<IndexValue<'a>> {
    let index = plan.index?;
    if index.fields[..plan.fixed_components].contains(&field) {
        equality(entity, request, field)
    } else {
        None
    }
}

// Only singleton clauses constrain every match. Required prefixes or suffixes
// must agree on their shared bytes. Ne alone allows absence and cannot narrow length.
fn excluded_by_bounds(
    entity: &EntitySchema,
    request: &Request<'_>,
    predicate: &Predicate<'_>,
) -> bool {
    if request.predicates.len() < 2 {
        return false;
    }
    let operand_bytes = |operand| {
        let value = match operand {
            QueryOperand::Literal(value) => value,
            QueryOperand::Field(name) => equality(entity, request, name)?,
            QueryOperand::Length(_) => return None,
        };

        super::scalar_bytes(value)
    };
    if let op @ (Comparison::StartsWith | Comparison::EndsWith) = predicate.comparison
        && let Some(bytes) = operand_bytes(predicate.operand)
    {
        return request.clauses().any(|clause| {
            let [other] = clause else { return false };
            if other.field != predicate.field
                || other.comparison != op
                || std::ptr::eq(other, predicate)
            {
                return false;
            }
            operand_bytes(other.operand).is_some_and(|other| {
                let (long, short) = if bytes.len() >= other.len() {
                    (bytes, other)
                } else {
                    (other, bytes)
                };
                !op.accepts_bytes(long, short)
            })
        });
    }
    let QueryOperand::Length(threshold) = predicate.operand else {
        return false;
    };
    let Some(field) = entity.index_field(predicate.field) else {
        return false;
    };
    let Some(mut max) = super::length_limit(field.value) else {
        return false;
    };
    let mut min = 0;
    for clause in request.clauses() {
        let [bound] = clause else { continue };
        if bound.field != predicate.field {
            continue;
        }
        let (comparison, n) = match (bound.comparison, bound.operand) {
            (comparison, QueryOperand::Length(n)) => (comparison, n),
            // Collection lengths count elements, even for a Text or Blob needle.
            (Comparison::Contains, _) if super::member_type(field.value).is_some() => {
                (Comparison::Ge, 1)
            }
            (Comparison::StartsWith | Comparison::EndsWith | Comparison::Contains, operand) => {
                let Some(bytes) = operand_bytes(operand) else {
                    continue;
                };
                (
                    Comparison::Ge,
                    u32::try_from(bytes.len()).unwrap_or(u32::MAX),
                )
            }
            _ => continue,
        };
        match comparison {
            Comparison::Eq => {
                min = min.max(n);
                max = max.min(n);
            }
            Comparison::Ge => min = min.max(n),
            Comparison::Le => max = max.min(n),
            Comparison::Gt => {
                let Some(lower) = n.checked_add(1) else {
                    return true;
                };
                min = min.max(lower);
            }
            Comparison::Lt => {
                let Some(upper) = n.checked_sub(1) else {
                    return true;
                };
                max = max.min(upper);
            }
            _ => continue,
        }
        if min > max
            || predicate
                .comparison
                .uniform_length_result(min, max, threshold)
                == Some(false)
        {
            return true;
        }
    }
    false
}

// A mandatory clause is impossible only if every alternative is disproved.
pub(super) fn has_conflicting_predicates(
    entity: &EntitySchema,
    request: &Request<'_>,
    index: Option<&IndexSchema>,
) -> bool {
    if request.index.is_none()
        && (request.predicates.len() < 2 || request.fixed_equalities(entity).next().is_none())
    {
        return request.clauses().any(|clause| {
            clause.iter().all(|p| {
                p.constant_comparison(entity) == Some(false)
                    || excluded_by_bounds(entity, request, p)
            })
        });
    }
    let prefix = request.index.as_ref().zip(index);
    let components = prefix
        .into_iter()
        .flat_map(|((_, values), index)| index.fields.iter().copied().zip(values.iter().copied()));
    if components
        .chain(request.fixed_equalities(entity))
        .any(|(field, value)| {
            value == IndexValue::Absent && requires_present_field(entity, request, field)
        })
    {
        return true;
    }
    let lookup = |field: &str| equality(entity, request, field);

    request.clauses().any(|clause| {
        clause.iter().all(|p| {
            fixed_comparison(entity, p, &lookup) == Some(false)
                || excluded_by_bounds(entity, request, p)
        })
    })
}

pub(super) fn equality<'a>(
    entity: &EntitySchema,
    request: &Request<'a>,
    field: &str,
) -> Option<IndexValue<'a>> {
    direct_equality(entity, request, field).or_else(|| transferred_equality(entity, request, field))
}

fn direct_equality<'a>(
    entity: &EntitySchema,
    request: &Request<'a>,
    field: &str,
) -> Option<IndexValue<'a>> {
    if let Some((index, values)) = &request.index
        && let Some(value) = prefix_value(entity.index_by_name(index)?.fields, values, field)
    {
        return Some(value);
    }
    // Member reads return absence before decoding children of an absent record.
    request.fixed_equalities(entity).find_map(|(name, value)| {
        let fixes_field = name == field
            || (value == IndexValue::Absent
                && entity
                    .record_member(field)
                    .is_some_and(|(slot, _)| entity.fields[slot].name == name));
        fixes_field.then_some(value)
    })
}

// Transfer a fixed value through one mandatory field equality. The physical
// primary range and automatic secondary prefixes consume this same proof.
pub(super) fn transferred_equality<'a>(
    entity: &EntitySchema,
    request: &Request<'a>,
    field: &str,
) -> Option<IndexValue<'a>> {
    if request.predicates.len() < 2
        && request
            .index
            .as_ref()
            .is_none_or(|(_, values)| values.is_empty())
    {
        return None;
    }
    equality_transfers(request)
        .filter(|&(_, target)| target == field)
        .find_map(|(source, target)| transferred_value(entity, request, source, target))
}

// Each mandatory equality can carry a directly fixed operand in either direction.
pub(super) fn equality_transfers<'a>(
    request: &Request<'a>,
) -> impl Iterator<Item = (&'a str, &'a str)> {
    request
        .clauses()
        .filter_map(|clause| {
            let [predicate] = clause else { return None };
            let QueryOperand::Field(right) = predicate.operand else {
                return None;
            };
            (predicate.comparison == Comparison::Eq)
                .then_some([(predicate.field, right), (right, predicate.field)])
        })
        .flatten()
}

pub(super) fn transferred_value<'a>(
    entity: &EntitySchema,
    request: &Request<'a>,
    source: &str,
    target: &str,
) -> Option<IndexValue<'a>> {
    let value = direct_equality(entity, request, source)?;
    // Comparable fields can have different byte limits or decimal scales.
    let target = entity.index_field(target)?;
    value.admit(target.value, target.optional).ok()?;
    Some(value)
}
