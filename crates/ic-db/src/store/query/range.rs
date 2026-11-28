//! Tighten the selected traversal by mandatory leading-key predicates and continuation.
//! Uses the admitted fixed prefix; this does not select new paths.

use super::{Comparison, Plan, Request, Term, TermOperand};
use crate::{EntitySchema, Error, IndexSchema, store_key::StoreKey};
use std::{cmp::Ordering, ops::Bound};

pub(super) type Bounds = (Bound<StoreKey>, Bound<StoreKey>);

// Check the final physical bounds, including any cursor and predicate tightening.
pub(super) fn exact_key(bounds: &Bounds) -> Option<&StoreKey> {
    match bounds {
        (Bound::Included(low), Bound::Included(high)) if low == high => Some(low),
        _ => None,
    }
}

// A closed range containing one physical key needs no iterator seek or lookahead.
pub(super) fn entries<'a, M: ic_storage::Memory>(
    map: &'a super::Map<M>,
    bounds: &'a Bounds,
    descending: bool,
) -> impl FnMut() -> Option<(StoreKey, Vec<u8>)> + 'a {
    enum Scan<P, R> {
        Point(P),
        Range(R),
    }
    let mut scan = match exact_key(bounds) {
        Some(key) => Scan::Point(Some(key)),
        None => Scan::Range(map.range((bounds.0.as_ref(), bounds.1.as_ref()))),
    };
    move || match &mut scan {
        Scan::Point(key) => {
            let key = key.take()?;
            map.get(key).map(|bytes| (key.clone(), bytes))
        }
        Scan::Range(entries) => {
            let entry = if descending {
                entries.next_back()
            } else {
                entries.next()
            }?;
            Some(entry.into_pair())
        }
    }
}

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
    field: &str,
    term: &'a Term,
    secondary: bool,
    prefix_len: usize,
) -> Result<Option<(Comparison, &'a [u8])>, Error> {
    if !matches_field(entity, term, field) {
        return Ok(None);
    }
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

///
/// ClauseBound
/// Encodable restriction on the next physical key component.
///
enum ClauseBound<'a> {
    Single {
        term: &'a Term,
        comparison: Comparison,
        key: &'a [u8],
    },
    Membership {
        low: &'a [u8],
        high: &'a [u8],
    },
}

impl<'a> ClauseBound<'a> {
    fn for_field(
        entity: &EntitySchema,
        field: &str,
        secondary: bool,
        clause: &'a [Term],
        prefix_len: usize,
    ) -> Result<Option<Self>, Error> {
        let Some((term, alternatives)) = clause.split_first() else {
            return Ok(None);
        };
        // A disjunction needs exact values. The optional-present tag alone
        // can bound a singleton but cannot identify a member of a set.
        let singleton = alternatives.is_empty();
        let Some((comparison, key)) =
            bound_operand(entity, field, term, secondary && singleton, prefix_len)?
        else {
            return Ok(None);
        };
        if singleton {
            return Ok(Some(Self::Single {
                term,
                comparison,
                key,
            }));
        }
        if comparison != Comparison::Eq {
            return Ok(None);
        }
        let (mut low, mut high) = (key, key);
        for term in alternatives {
            let Some((Comparison::Eq, key)) =
                bound_operand(entity, field, term, false, prefix_len)?
            else {
                return Ok(None);
            };
            low = low.min(key);
            high = high.max(key);
        }

        Ok(Some(Self::Membership { low, high }))
    }

    // Inclusive endpoints spanning the whole domain save no scan work. A
    // complete-key probe can still skip the unlisted members inside that interval.
    fn restricts_scan(
        self,
        includes_absence: bool,
        can_probe: bool,
        extreme: impl Fn(&[u8], bool) -> bool,
    ) -> bool {
        match self {
            Self::Single { term, .. } if term.tests_presence && !includes_absence => false,
            Self::Single {
                comparison, key, ..
            } => match comparison {
                Comparison::Ge => !extreme(key, false),
                Comparison::Le => includes_absence || !extreme(key, true),
                _ => true,
            },
            Self::Membership { low, high } => {
                can_probe || !(extreme(low, false) && extreme(high, true))
            }
        }
    }
}

// A membership range can narrow a scan even when gaps still require row checks.
pub(super) fn key_range_proves_clause(
    entity: &EntitySchema,
    index: Option<&IndexSchema>,
    fixed: usize,
    clause: &[Term],
    prefix_len: usize,
) -> bool {
    let Some((name, secondary)) = next_field(entity, index, fixed) else {
        return false;
    };
    match ClauseBound::for_field(entity, name, secondary, clause, prefix_len) {
        Ok(Some(ClauseBound::Single { .. })) => true,
        Ok(Some(ClauseBound::Membership { low, high })) => {
            low == high
                || (secondary
                    && !index.is_some_and(|i| i.is_unique_lookup(fixed + 1))
                    && low.len() == high.len()
                    && entity
                        .index_field(name)
                        .and_then(|f| crate::key::fixed_width(crate::key::operand_type(f.value)))
                        .is_some()
                    && complete_key_run(low, high, clause))
        }
        _ => false,
    }
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
    let can_probe = can_probe && (!secondary || path.index.is_unique_lookup(path.fixed + 1));
    plan.clauses().any(|clause| {
        ClauseBound::for_field(entity, name, secondary, clause, path.prefix.len())
            .ok()
            .flatten()
            .is_some_and(|bound| bound.restricts_scan(includes_absence, can_probe, extreme))
    })
}

// A secondary candidate must retain every restriction the primary scan could seek.
pub(super) fn preserves_primary_bounds(
    entity: &EntitySchema,
    plan: &Plan<'_>,
    path: &super::order::IndexPath<'_>,
    can_probe: bool,
) -> bool {
    let field = entity.fields[plan.primary].value;
    let next = next_field(entity, Some(path.index), path.fixed);
    plan.clauses().all(|clause| {
        let primary_bound =
            ClauseBound::for_field(entity, entity.primary_key, false, clause, plan.prefix.len())
                .ok()
                .flatten()
                .is_some_and(|bound| {
                    bound.restricts_scan(false, can_probe, |key, upper| {
                        crate::key::is_fixed_extreme(field, key, upper)
                    })
                });
        !primary_bound
            || next.is_some_and(|(name, secondary)| {
                matches!(
                    ClauseBound::for_field(entity, name, secondary, clause, path.prefix.len()),
                    Ok(Some(_))
                )
            })
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
    // The owner is a complete key after the fixed secondary components.
    let mut exact = next.is_none();
    if let Some((field, false)) = next
        && !request.predicates.is_empty()
        && let Some(owner) = super::order::equality(entity, request, field)
    {
        owner.append_component(false, &mut first_key);
        exact = first_key.len() <= crate::MAX_INDEX_KEY_BYTES;
        if !exact {
            first_key.truncate(plan.prefix.len());
        }
    }
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
            let (term, comparison, operand) = match ClauseBound::for_field(
                entity,
                field,
                secondary,
                clause,
                plan.prefix.len(),
            )? {
                Some(ClauseBound::Single {
                    term,
                    comparison,
                    key,
                }) => (term, comparison, key),
                Some(ClauseBound::Membership { low, high }) => {
                    lower.tighten(component_key(low, false)?, true, Ordering::Greater);
                    upper.tighten(component_key(high, true)?, !secondary, Ordering::Less);
                    continue;
                }
                None => continue,
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
