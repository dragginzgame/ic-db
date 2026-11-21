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
    match &term.operand {
        TermOperand::Nested { member, .. }
        | TermOperand::Length {
            member: Some(member),
            ..
        } => path.split_once('.') == Some((parent, member.field().name)),
        _ => path == parent,
    }
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
    if secondary && term.tests_presence && entity.fields[term.position].optional {
        return Ok((prefix_len < crate::MAX_INDEX_KEY_BYTES).then_some((Comparison::Ge, &[1])));
    }
    let Some((bytes, _)) = literal(term) else {
        return Ok(None);
    };
    let operand = match term.comparison {
        Comparison::Eq | Comparison::Lt | Comparison::Le | Comparison::Gt | Comparison::Ge => bytes,
        // Drop the terminator but retain escaping and the optional-present tag.
        Comparison::StartsWith if secondary => {
            bytes.strip_suffix(&[0, 0]).ok_or(Error::InvalidQuery)?
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
            // A nonunique index can have many owners per component. A complete run
            // of fixed-width values proves membership for every one of those entries.
            secondary
                && index.is_some_and(|i| !i.unique)
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
) -> bool {
    plan.clauses().any(|clause| {
        if let [term] = clause
            && let Some((bytes, _)) = literal(term)
        {
            let field = match &term.operand {
                TermOperand::Nested { member, .. } => member.field(),
                _ => &entity.fields[term.position],
            };
            // The lowest/highest possible fixed-width key excludes no candidates
            // at an inclusive bound. An optional field has a separate absence tag.
            if !field.optional
                && crate::key::fixed_width(crate::key::operand_type(field.value))
                    == Some(bytes.len())
                && match term.comparison {
                    Comparison::Ge => bytes.iter().all(|&b| b == 0),
                    Comparison::Le => bytes.iter().all(|&b| b == u8::MAX),
                    _ => false,
                }
            {
                return false;
            }
        }
        bounds_index_clause(entity, path, clause)
    })
}

// A disjunction of literal equalities on one key component fits one inclusive scan between
// its extrema. The original predicate still rejects every gap in that scan.
fn membership_interval<'a>(
    entity: &EntitySchema,
    field: &str,
    clause: &'a [Term],
    prefix_len: usize,
) -> Option<(&'a [u8], &'a [u8])> {
    let mut operands = clause.iter().map(|term| {
        if matches_field(entity, term, field) && term.comparison == Comparison::Eq {
            bound_operand(entity, term, false, prefix_len)
                .ok()
                .flatten()
                .map(|(_, bytes)| bytes)
        } else {
            None
        }
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

// Membership on the last component of a physical key supplies all possible keys. Other
// key-only clauses can narrow that set before we spend any storage probes.
pub(super) fn point_members<'a>(
    entity: &EntitySchema,
    plan: &'a Plan<'_>,
    request: &Request<'_>,
    allow_row_checks: bool,
) -> Option<impl ExactSizeIterator<Item = (&'a [u8], bool)>> {
    if plan.terms.len() < 2 {
        return None;
    }
    let position = match plan.index {
        None => plan.primary,
        Some(index) if index.is_unique_lookup(plan.fixed_components + 1) => {
            entity.field_position(index.fields[plan.fixed_components])?
        }
        // Non-unique indexes append the owner ID after all declared components.
        Some(index) if !index.unique && plan.fixed_components == index.fields.len() => plan.primary,
        Some(_) => return None,
    };
    let field = entity.fields[position].name;
    let (seed_index, (seed, literals)) =
        plan.clauses()
            .zip(request.clauses())
            .enumerate()
            .find(|(_, (clause, _))| {
                clause.len() > 1
                    && clause.iter().all(|term| {
                        term.position == position
                            && term.comparison == Comparison::Eq
                            && matches!(term.operand, TermOperand::Literal { .. })
                    })
            })?;
    let mut members = Vec::new();
    for (term, predicate) in seed.iter().zip(literals) {
        let (key, absent) = literal(term)?;
        if absent {
            // An admitted present-only index has no physical key for absence.
            plan.index.filter(|index| index.present_only)?;
            continue;
        }
        // This is an exact, complete key. One too large to store cannot match.
        if plan.prefix.len() + key.len() > crate::MAX_INDEX_KEY_BYTES {
            continue;
        }
        let super::QueryOperand::Literal(value) = &predicate.operand else {
            return None;
        };
        // Keep physical key order so existence stops at the same first candidate as a scan.
        if let Err(at) = members.binary_search_by_key(&key, |&(existing, _, _)| existing) {
            members.insert(at, (key, value, false));
        }
    }
    for (i, (clause, terms)) in request.clauses().zip(plan.clauses()).enumerate() {
        if i == seed_index {
            continue;
        }
        members.retain_mut(|(key, value, unresolved)| {
            let lookup = |name: &str| {
                if name == field {
                    Some(**value)
                } else {
                    super::order::fixed_prefix_value(request, plan, name)
                }
            };
            let mut needs_row = false;
            for (predicate, term) in clause.iter().zip(terms) {
                // Same-field literal bounds already have the candidate's key framing.
                let result = if term.position == position
                    && term.comparison.is_relational()
                    && let TermOperand::Literal { bytes, absent } = &term.operand
                {
                    Some(term.comparison.accepts(
                        Some(key),
                        (!absent).then_some(bytes.as_slice()),
                        entity.fields[position].optional,
                    ))
                } else {
                    super::order::fixed_comparison(entity, predicate, &lookup)
                };
                match result {
                    Some(true) => return true,
                    Some(false) => {}
                    None => needs_row = true,
                }
            }
            // A later mandatory clause may still exclude a candidate that needs its row.
            *unresolved |= needs_row;
            needs_row
        });
    }
    if members.len() > request.work
        || (!allow_row_checks && members.iter().any(|(_, _, unresolved)| *unresolved))
    {
        return None;
    }
    Some(
        members
            .into_iter()
            .map(|(key, _, unresolved)| (key, unresolved)),
    )
}

// A secondary candidate must retain every bound the primary scan could seek.
pub(super) fn preserves_primary_bounds(
    entity: &EntitySchema,
    plan: &Plan<'_>,
    path: &super::order::IndexPath<'_>,
) -> bool {
    plan.clauses().all(|clause| {
        let primary_bound = match clause {
            [term] => {
                term.position == plan.primary
                    && matches!(
                        bound_operand(entity, term, false, plan.prefix.len()),
                        Ok(Some(_))
                    )
            }
            _ => {
                membership_interval(entity, entity.primary_key, clause, plan.prefix.len()).is_some()
            }
        };

        !primary_bound || bounds_index_clause(entity, path, clause)
    })
}

pub(super) fn bounds(
    entity: &EntitySchema,
    request: &Request<'_>,
    plan: &Plan<'_>,
) -> Result<Option<Bounds>, Error> {
    if super::order::has_conflicting_predicates(entity, request, plan.index) {
        return Ok(None);
    }
    let mut lower = Endpoint {
        key: StoreKey::new(plan.prefix.clone())?,
        inclusive: true,
    };
    let full_unique = plan
        .index
        .is_some_and(|index| index.is_unique_lookup(plan.fixed_components));
    let mut upper = if full_unique {
        Endpoint {
            key: lower.key.clone(),
            inclusive: true,
        }
    } else {
        Endpoint {
            key: StoreKey::new(successor(plan.prefix.clone())?)?,
            inclusive: false,
        }
    };
    if let Some((field, secondary)) = next_field(entity, plan.index, plan.fixed_components) {
        for clause in plan.clauses() {
            let [term] = clause else {
                if let Some((low, high)) =
                    membership_interval(entity, field, clause, plan.prefix.len())
                {
                    lower.tighten(
                        StoreKey::new([plan.prefix.as_slice(), low].concat())?,
                        true,
                        Ordering::Greater,
                    );
                    // Secondary components can have trailing fields and owner IDs.
                    // Stop after all keys sharing the greatest member's prefix.
                    let end = [plan.prefix.as_slice(), high].concat();
                    let end = if secondary { successor(end)? } else { end };
                    upper.tighten(StoreKey::new(end)?, !secondary, Ordering::Less);
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
            let mut bytes = plan.prefix.clone();
            bytes.extend_from_slice(operand);
            let end = if secondary
                && matches!(
                    comparison,
                    Comparison::Eq | Comparison::Gt | Comparison::Le | Comparison::StartsWith
                ) {
                successor(bytes.clone())?
            } else {
                bytes.clone()
            };
            let key = StoreKey::new(bytes)?;
            let end = StoreKey::new(end)?;
            // Less-than predicates require presence too. Their upper bound alone
            // would include the optional absent component (00) before present (01).
            if secondary
                && entity.fields[term.position].optional
                && matches!(comparison, Comparison::Lt | Comparison::Le)
            {
                let mut present = plan.prefix.clone();
                present.push(1);
                lower.tighten(StoreKey::new(present)?, true, Ordering::Greater);
            }

            match comparison {
                Comparison::Eq | Comparison::StartsWith => {
                    lower.tighten(key, true, Ordering::Greater);
                    upper.tighten(end, !secondary, Ordering::Less);
                }
                Comparison::Gt => lower.tighten(end, secondary, Ordering::Greater),
                Comparison::Ge => lower.tighten(key, true, Ordering::Greater),
                Comparison::Lt => upper.tighten(key, false, Ordering::Less),
                Comparison::Le => upper.tighten(end, !secondary, Ordering::Less),
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
