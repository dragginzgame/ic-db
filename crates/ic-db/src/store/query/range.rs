//! Tighten the selected traversal by mandatory leading-key predicates and continuation.
//! Uses the admitted fixed prefix; this does not select new paths.

use super::{Comparison, Plan, Request, Term, TermOperand};
use crate::{EntitySchema, Error, IndexSchema, store_key::StoreKey};
use std::{cmp::Ordering, ops::Bound};

pub(super) type Bounds = (Bound<StoreKey>, Bound<StoreKey>);

// Only an inclusive endpoint can be observed as the final physical map key.
pub(super) fn terminal_key(bounds: &Bounds, descending: bool) -> Option<&StoreKey> {
    match if descending { &bounds.0 } else { &bounds.1 } {
        Bound::Included(key) => Some(key),
        _ => None,
    }
}

// Skip an entire encoded prefix, including every trailing component/owner.
// Namespace framing guarantees a successor, even for all-FF component bytes.
fn successor(mut bytes: Vec<u8>) -> Result<Vec<u8>, Error> {
    while bytes.last() == Some(&255) {
        bytes.pop();
    }
    *bytes.last_mut().ok_or(Error::InvalidQuery)? += 1;

    Ok(bytes)
}

struct Endpoint {
    key: StoreKey,
    inclusive: bool,
}
impl Endpoint {
    // Equal endpoints tighten only toward exclusion, at either end of a range.
    fn tighten(&mut self, key: StoreKey, inclusive: bool, direction: Ordering) {
        let order = key.cmp(&self.key);
        if order == direction || (order == Ordering::Equal && !inclusive) {
            *self = Self { key, inclusive };
        }
    }

    fn into_bound(self) -> Bound<StoreKey> {
        if self.inclusive {
            Bound::Included(self.key)
        } else {
            Bound::Excluded(self.key)
        }
    }
}

// A physical component names the complete scalar. Siblings share the parent slot.
fn matches_field(entity: &EntitySchema, term: &Term, path: &str) -> bool {
    let parent = entity.fields[term.position].name;
    match left_member(term) {
        Some(member) => path.split_once('.') == Some((parent, member.field().name)),
        None => path == parent,
    }
}

fn left_member(term: &Term) -> Option<crate::member::RecordMember> {
    match &term.operand {
        TermOperand::Nested { member, .. } => Some(*member),
        TermOperand::Field { left_member, .. } => *left_member,
        TermOperand::Length { member, .. } => *member,
        _ => None,
    }
}

fn optional_component(entity: &EntitySchema, term: &Term) -> bool {
    entity.fields[term.position].optional || left_member(term).is_some_and(|m| m.field().optional)
}

fn literal(term: &Term) -> Option<(&[u8], bool)> {
    match &term.operand {
        TermOperand::Literal { bytes, absent }
        | TermOperand::Nested {
            literal: bytes,
            absent,
            ..
        } => Some((bytes, *absent)),
        _ => None,
    }
}

// Report the actual encoded operand used to tighten this path. A residual
// operand may exceed the physical key bound even when its query is valid.
fn bound_operand<'a>(
    entity: &EntitySchema,
    term: &'a Term,
    secondary: bool,
    prefix_len: usize,
) -> Result<Option<(Comparison, &'a [u8])>, Error> {
    // All present values share the 01 tag on an optional secondary component.
    if secondary && term.tests_presence && optional_component(entity, term) {
        return Ok((prefix_len < crate::MAX_INDEX_KEY_BYTES).then_some((Comparison::Ge, &[1])));
    }
    if let TermOperand::Length { member, threshold } = &term.operand {
        let field = member.map_or(&entity.fields[term.position], |m| m.field());
        if term
            .comparison
            .empty_length_value(*threshold, field.value)
            .is_some()
        {
            let key = crate::key::empty_bytes_component(
                entity.fields[term.position].optional || field.optional,
            );
            return Ok((prefix_len + key.len() <= crate::MAX_INDEX_KEY_BYTES)
                .then_some((Comparison::Eq, key)));
        }
    }
    let Some((bytes, _)) = literal(term) else {
        return Ok(None);
    };
    let operand = match term.comparison {
        Comparison::Eq | Comparison::Lt | Comparison::Le | Comparison::Gt | Comparison::Ge => bytes,
        // Drop the terminator but retain escaping and the optional-present tag.
        Comparison::StartsWith if secondary => {
            crate::key::bytes_prefix(bytes).ok_or(Error::InvalidQuery)?
        }
        _ => return Ok(None),
    };

    Ok((prefix_len + operand.len() <= crate::MAX_INDEX_KEY_BYTES)
        .then_some((term.comparison, operand)))
}

// A non-unique index stores the owner key immediately after all declared
// components. Once they are fixed, owner comparisons bound complete keys.
fn next_field<'a>(
    entity: &'a EntitySchema,
    index: Option<&'a IndexSchema>,
    fixed: usize,
) -> Option<(&'a str, bool)> {
    if let Some(index) = index {
        if let Some(field) = index.fields.get(fixed) {
            return Some((field, true));
        }
        if index.unique {
            return None;
        }
    }

    Some((entity.primary_key, false))
}

// Both path ranking and key-only counting use the same encodable range proof.
pub(super) fn key_range_proves_clause(
    entity: &EntitySchema,
    index: Option<&IndexSchema>,
    fixed: usize,
    clause: &[Term],
    prefix_len: usize,
) -> bool {
    next_field(entity, index, fixed).is_some_and(|(name, secondary)| match clause {
        [term] => {
            matches_field(entity, term, name)
                && matches!(
                    bound_operand(entity, term, secondary, prefix_len),
                    Ok(Some(_))
                )
        }
        _ => membership_interval(entity, name, clause, prefix_len).is_some_and(|(low, high)| {
            if low == high {
                return true;
            }
            // Trailing components do not affect membership in this interval.
            // Keep complete unique keys on the bounded point-probe path.
            secondary
                && !index.is_some_and(|i| i.is_unique_lookup(fixed + 1))
                && low.len() == high.len()
                && entity
                    .index_field(name)
                    .and_then(|f| crate::key::fixed_width(crate::key::operand_type(f.value)))
                    .is_some()
                && complete_key_run(low, high, clause)
        }),
    })
}

// Follow consecutive admitted literals by reference. Carry skips trailing FF
// bytes in the current key and requires zeros in those positions in its successor.
fn complete_key_run<'a>(mut current: &'a [u8], high: &[u8], clause: &'a [Term]) -> bool {
    while current != high {
        let Some(pivot) = current.iter().rposition(|&byte| byte != u8::MAX) else {
            return false;
        };
        let next = clause
            .iter()
            .filter_map(|term| literal(term).map(|(key, _)| key))
            .find(|key| {
                key.len() == current.len()
                    && key[..pivot] == current[..pivot]
                    && key[pivot] == current[pivot] + 1
                    && key[pivot + 1..].iter().all(|&byte| byte == 0)
            });
        let Some(next) = next else { return false };
        current = next;
    }
    true
}

fn bounds_index_clause(
    entity: &EntitySchema,
    path: &super::order::IndexPath<'_>,
    clause: &[Term],
) -> bool {
    if clause.len() == 1 {
        return key_range_proves_clause(
            entity,
            Some(path.index),
            path.fixed,
            clause,
            path.prefix.len(),
        );
    }

    next_field(entity, Some(path.index), path.fixed).is_some_and(|(field, _)| {
        membership_interval(entity, field, clause, path.prefix.len()).is_some()
    })
}

// Rank only bounds the traversal can actually encode on the next index field.
pub(super) fn has_secondary_bound(
    entity: &EntitySchema,
    plan: &Plan<'_>,
    path: &super::order::IndexPath<'_>,
    can_probe: bool,
) -> bool {
    let Some((name, secondary)) = next_field(entity, Some(path.index), path.fixed) else {
        return false;
    };
    let Some(field) = entity.index_field(name) else {
        return false;
    };
    let includes_absence = field.optional && !path.index.present_only;
    let extreme = |key: &[u8], upper: bool| {
        let present = if field.optional {
            match key {
                [0] => return !upper,
                [1, value @ ..] if upper || !includes_absence => value,
                _ => return false,
            }
        } else {
            key
        };
        crate::key::is_fixed_extreme(field.value, present, upper)
    };
    plan.clauses().any(|clause| {
        if let [term] = clause {
            if !matches_field(entity, term, name)
                || (term.tests_presence
                    && (entity.field_is_required(name) || path.index.present_only))
            {
                return false;
            }
            return bound_operand(entity, term, secondary, path.prefix.len())
                .ok()
                .flatten()
                .is_some_and(|(comparison, key)| match comparison {
                    Comparison::Ge => !extreme(key, false),
                    // Optional upper bounds also exclude absent entries.
                    Comparison::Le => includes_absence || !extreme(key, true),
                    _ => true,
                });
        }
        membership_interval(entity, name, clause, path.prefix.len()).is_some_and(|(low, high)| {
            // Complete keys can use point probes. A component with trailing keys
            // must scan every value between the two endpoints.
            (can_probe && (!secondary || path.index.is_unique_lookup(path.fixed + 1)))
                || !(extreme(low, false) && extreme(high, true))
        })
    })
}

// A disjunction of exact values on one key component fits one inclusive scan between
// its extrema. The original predicate still rejects every gap in that scan.
fn membership_interval<'a>(
    entity: &EntitySchema,
    field: &str,
    clause: &'a [Term],
    prefix_len: usize,
) -> Option<(&'a [u8], &'a [u8])> {
    let mut operands = clause.iter().map(|term| {
        if !matches_field(entity, term, field) {
            return None;
        }
        let (comparison, bytes) = bound_operand(entity, term, false, prefix_len).ok()??;
        (comparison == Comparison::Eq).then_some(bytes)
    });
    let mut low = operands.next()??;
    let mut high = low;
    for operand in operands {
        let key = operand?;
        low = low.min(key);
        high = high.max(key);
    }

    Some((low, high))
}

// A secondary candidate must retain every restriction the primary scan could seek.
pub(super) fn preserves_primary_bounds(
    entity: &EntitySchema,
    plan: &Plan<'_>,
    path: &super::order::IndexPath<'_>,
    can_probe: bool,
) -> bool {
    let field = entity.fields[plan.primary].value;
    plan.clauses().all(|clause| {
        let primary_bound = match clause {
            [term] => {
                term.position == plan.primary
                    && !term.tests_presence
                    && bound_operand(entity, term, false, plan.prefix.len())
                        .ok()
                        .flatten()
                        .is_some_and(|(comparison, key)| match comparison {
                            Comparison::Ge => !crate::key::is_fixed_extreme(field, key, false),
                            Comparison::Le => !crate::key::is_fixed_extreme(field, key, true),
                            _ => true,
                        })
            }
            _ => {
                membership_interval(entity, entity.primary_key, clause, plan.prefix.len())
                    .is_some_and(|(low, high)| {
                        // Probes can skip unlisted IDs even when the interval spans the table.
                        can_probe
                            || !(crate::key::is_fixed_extreme(field, low, false)
                                && crate::key::is_fixed_extreme(field, high, true))
                    })
            }
        };

        !primary_bound || bounds_index_clause(entity, path, clause)
    })
}

// Only mandatory bounds constrain every result, including when a different
// index supplies the order. Compare the same admitted keys used by traversal.
pub(super) fn empty_input(entity: &EntitySchema, request: &Request<'_>, plan: &Plan<'_>) -> bool {
    if plan.terms.len() >= 2 {
        let endpoints = |inclusive, strict| {
            plan.clauses()
                .zip(request.clauses())
                .filter_map(move |(terms, predicates)| {
                    let [term] = terms else { return None };
                    if term.comparison != inclusive && term.comparison != strict {
                        return None;
                    }
                    let (key, false) = literal(term)? else {
                        return None;
                    };
                    Some((predicates.first()?, key))
                })
        };
        for (lower, start) in endpoints(Comparison::Ge, Comparison::Gt) {
            for (upper, end) in endpoints(Comparison::Le, Comparison::Lt) {
                if lower.field != upper.field {
                    continue;
                }
                let strict_lower = lower.comparison == Comparison::Gt;
                let strict_upper = upper.comparison == Comparison::Lt;
                if start > end || (start == end && (strict_lower || strict_upper)) {
                    return true;
                }
                // An open interval between consecutive discrete values is empty.
                if strict_lower
                    && strict_upper
                    && let (super::QueryOperand::Literal(low), super::QueryOperand::Literal(high)) =
                        (lower.operand, upper.operand)
                    && super::consecutive_values(low, high)
                {
                    return true;
                }
            }
        }
    }
    super::order::has_conflicting_predicates(entity, request, plan.index)
}

pub(super) fn bounds(
    entity: &EntitySchema,
    request: &Request<'_>,
    plan: &Plan<'_>,
) -> Result<Option<Bounds>, Error> {
    if empty_input(entity, request, plan) {
        return Ok(None);
    }
    let mut first_key = plan.prefix.clone();
    let next = next_field(entity, plan.index, plan.fixed_components);
    // Extend the seek through consecutive fixed components, including an owner suffix.
    // Keep the admitted plan prefix for residual bounds and cursor validation.
    let mut fixed = plan.fixed_components;
    let mut component = next;
    while !request.predicates.is_empty()
        && let Some((field, secondary)) = component
    {
        let Some(value) = super::order::equality(entity, request, field) else {
            break;
        };
        let schema = entity.index_field(field).ok_or(Error::InvalidSchema)?;
        let prefix_len = first_key.len();
        value.append_component(schema.optional, &mut first_key);
        if first_key.len() > crate::MAX_INDEX_KEY_BYTES {
            // An oversized operand remains a row predicate on the last encodable prefix.
            first_key.truncate(prefix_len);
            break;
        }
        fixed += 1;
        component = if secondary {
            next_field(entity, plan.index, fixed)
        } else {
            None
        };
    }
    let exact = component.is_none();
    let mut lower = Endpoint {
        key: StoreKey::new(first_key)?,
        inclusive: true,
    };
    let mut upper = if exact {
        Endpoint {
            key: lower.key.clone(),
            inclusive: true,
        }
    } else {
        Endpoint {
            key: StoreKey::new(successor(lower.key.to_vec())?)?,
            inclusive: false,
        }
    };
    if let Some((field, secondary)) = next {
        // A secondary component can precede more fields or an owner suffix. Its end
        // is the successor of the whole prefix; a primary key ends at the key itself.
        let component_key = |operand: &[u8], after: bool| {
            let bytes = [plan.prefix.as_slice(), operand].concat();
            StoreKey::new(if after && secondary {
                successor(bytes)?
            } else {
                bytes
            })
        };
        for clause in plan.clauses() {
            let [term] = clause else {
                if let Some((low, high)) =
                    membership_interval(entity, field, clause, plan.prefix.len())
                {
                    lower.tighten(component_key(low, false)?, true, Ordering::Greater);
                    upper.tighten(component_key(high, true)?, !secondary, Ordering::Less);
                }
                continue;
            };
            if !matches_field(entity, term, field) {
                continue;
            }
            let Some((comparison, operand)) =
                bound_operand(entity, term, secondary, plan.prefix.len())?
            else {
                continue;
            };
            // Less-than predicates require presence too. Their upper bound alone
            // would include the optional absent component (00) before present (01).
            if secondary
                && optional_component(entity, term)
                && matches!(comparison, Comparison::Lt | Comparison::Le)
            {
                lower.tighten(component_key(&[1], false)?, true, Ordering::Greater);
            }

            match comparison {
                Comparison::Eq | Comparison::StartsWith => {
                    lower.tighten(component_key(operand, false)?, true, Ordering::Greater);
                    upper.tighten(component_key(operand, true)?, !secondary, Ordering::Less);
                }
                Comparison::Gt => {
                    lower.tighten(component_key(operand, true)?, secondary, Ordering::Greater)
                }
                Comparison::Ge => {
                    lower.tighten(component_key(operand, false)?, true, Ordering::Greater)
                }
                Comparison::Lt => {
                    upper.tighten(component_key(operand, false)?, false, Ordering::Less)
                }
                Comparison::Le => {
                    upper.tighten(component_key(operand, true)?, !secondary, Ordering::Less)
                }
                _ => {}
            }
        }
    }
    if let Some(cursor) = request.cursor {
        let key = StoreKey::new(cursor.position.clone())?;

        // Old or caller-selected positions must never widen the predicate range.
        if plan.descending {
            upper.tighten(key, cursor.resume == super::Resume::Retry, Ordering::Less);
        } else {
            lower.tighten(
                key,
                cursor.resume == super::Resume::Retry,
                Ordering::Greater,
            );
        }
    }
    // Do not hand contradictory or equal-exclusive bounds to the supplier.
    if lower.key > upper.key || (lower.key == upper.key && !(lower.inclusive && upper.inclusive)) {
        return Ok(None);
    }

    Ok(Some((lower.into_bound(), upper.into_bound())))
}
