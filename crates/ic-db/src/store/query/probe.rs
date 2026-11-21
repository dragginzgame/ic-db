//! Exact membership probes for count and existence on complete physical keys.

use super::{
    Comparison, Plan, Request, TermOperand,
    aggregate::{Accumulator, Aggregate},
};
use crate::{EntitySchema, Error};
use ic_storage::Memory;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MemberProbe {
    CountKeys,
    CountWithRows,
    CountValidated,
    Exists,
}

pub(super) fn probe_members<M: Memory>(
    source: &super::QuerySource<'_, M>,
    entity: &EntitySchema,
    request: &Request<'_>,
    plan: &Plan<'_>,
    mode: MemberProbe,
) -> Result<Option<Aggregate<u64>>, Error> {
    let Some(members) = point_members(entity, plan, request, mode != MemberProbe::CountKeys) else {
        return Ok(None);
    };
    let db = source.read()?;
    let mut count = 0;
    let mut examined = 0;
    let mut fetched = 0;
    for (components, needs_row) in members {
        examined += 1;
        let key = crate::store_key::StoreKey::new(components.concat())?;
        if needs_row || matches!(mode, MemberProbe::CountValidated | MemberProbe::Exists) {
            let mut found = false;
            let bounds = (
                std::ops::Bound::Included(key.clone()),
                std::ops::Bound::Included(key),
            );
            let page = super::execute_inner(
                &db.map,
                entity,
                request,
                plan,
                Some(bounds),
                Some(&mut Accumulator::Exists(&mut found)),
                None,
            )?;
            count += u64::from(found);
            fetched += page.fetched;
            if found && mode == MemberProbe::Exists {
                break;
            }
        } else {
            count += u64::from(db.map.contains_key(&key));
        }
    }

    Ok(Some(Aggregate {
        value: count,
        examined,
        fetched,
        output_bytes: 8,
    }))
}

// Membership on the last component of a physical key supplies all possible keys. Other
// key-only clauses can narrow that set before we spend any storage probes.
fn point_members<'a>(
    entity: &EntitySchema,
    plan: &'a Plan<'_>,
    request: &Request<'_>,
    allow_row_checks: bool,
) -> Option<impl ExactSizeIterator<Item = ([&'a [u8]; 3], bool)>> {
    if plan.terms.len() < 2 {
        return None;
    }
    let (position, repeat_owner) = match plan.index {
        None => (plan.primary, false),
        Some(index) if index.is_unique_lookup(plan.fixed_components + 1) => (
            entity.field_position(index.fields[plan.fixed_components])?,
            false,
        ),
        // Non-unique keys end with the owner ID, even when it is also a declared field.
        Some(index) if !index.unique => match index.fields.get(plan.fixed_components..)? {
            [] => (plan.primary, false),
            [last] if *last == entity.primary_key => (plan.primary, true),
            _ => return None,
        },
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
        let TermOperand::Literal { bytes, absent } = &term.operand else {
            return None;
        };
        let key = bytes.as_slice();
        if *absent {
            // An admitted present-only index has no physical key for absence.
            plan.index.filter(|index| index.present_only)?;
            continue;
        }
        // This is an exact, complete key. One too large to store cannot match.
        if plan.prefix.len() + key.len() * (1 + usize::from(repeat_owner))
            > crate::MAX_INDEX_KEY_BYTES
        {
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
                    super::order::fixed_prefix_value(entity, request, plan, name)
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
    if members.len() > request.work {
        return None;
    }
    if members.iter().any(|(_, _, unresolved)| *unresolved) {
        if !allow_row_checks {
            return None;
        }
        // Probes bypass range construction. Apply its contradiction check before
        // reading owners for predicates the individual keys could not settle.
        if super::order::has_conflicting_predicates(entity, request, plan.index) {
            members.clear();
        }
    }
    Some(members.into_iter().map(move |(key, _, unresolved)| {
        let suffix = if repeat_owner { key } else { &[] };
        ([plan.prefix.as_slice(), key, suffix], unresolved)
    }))
}
