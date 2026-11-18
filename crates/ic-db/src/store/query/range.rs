//! Tighten the selected traversal by mandatory leading-key predicates and continuation.
//! Uses the admitted fixed prefix; this does not select new paths.
use super::{Comparison, Plan, Request, Term, TermOperand};
use crate::{EntitySchema, Error, store_key::StoreKey};
use std::{cmp::Ordering, ops::Bound};

type Bounds = (Bound<StoreKey>, Bound<StoreKey>);

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

// Report the actual encoded operand used to tighten this path. A residual
// operand may exceed the physical key bound even when its query is valid.
fn bound_operand(term: &Term, secondary: bool, prefix_len: usize) -> Result<Option<&[u8]>, Error> {
    let TermOperand::Literal { bytes, .. } = &term.operand else {
        return Ok(None);
    };
    let operand = match term.comparison {
        Comparison::Eq | Comparison::Lt | Comparison::Le | Comparison::Gt | Comparison::Ge => {
            bytes.as_slice()
        }
        // Drop the terminator but retain escaping and the optional-present tag.
        Comparison::StartsWith if secondary => {
            bytes.strip_suffix(&[0, 0]).ok_or(Error::InvalidQuery)?
        }
        _ => return Ok(None),
    };
    Ok((prefix_len + operand.len() <= crate::MAX_INDEX_KEY_BYTES).then_some(operand))
}

// Every predicate must independently constrain this same primary-key interval.
pub(super) fn covers_primary(plan: &Plan<'_>) -> bool {
    plan.index.is_none()
        && plan.terms.iter().all(|term| {
            !term.or_next
                && term.position == plan.primary
                && matches!(bound_operand(term, false, plan.prefix.len()), Ok(Some(_)))
        })
}

pub(super) fn bounds(
    entity: &EntitySchema,
    request: &Request<'_>,
    plan: &Plan<'_>,
) -> Result<Option<Bounds>, Error> {
    let mut lower = Endpoint {
        key: StoreKey::new(plan.prefix.clone())?,
        inclusive: true,
    };
    let mut upper = Endpoint {
        key: StoreKey::new(successor(plan.prefix.clone())?)?,
        inclusive: false,
    };
    let secondary = plan.index.is_some();
    let field = match plan.index {
        Some(index) => index.fields.get(plan.fixed_components).copied(),
        None => Some(entity.primary_key),
    };
    if let Some(field) = field {
        for clause in plan.terms.split_inclusive(|term| !term.or_next) {
            let [term] = clause else { continue };
            if entity.fields[term.position].name != field {
                continue;
            }
            let Some(operand) = bound_operand(term, secondary, plan.prefix.len())? else {
                continue;
            };
            let mut bytes = plan.prefix.clone();
            bytes.extend_from_slice(operand);
            let end = if secondary
                && matches!(
                    term.comparison,
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
                && matches!(term.comparison, Comparison::Lt | Comparison::Le)
            {
                let mut present = plan.prefix.clone();
                present.push(1);
                lower.tighten(StoreKey::new(present)?, true, Ordering::Greater);
            }
            match term.comparison {
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
            upper.tighten(key, cursor.inclusive, Ordering::Less);
        } else {
            lower.tighten(key, cursor.inclusive, Ordering::Greater);
        }
    }
    // Do not hand contradictory or equal-exclusive bounds to the supplier.
    if lower.key > upper.key || (lower.key == upper.key && !(lower.inclusive && upper.inclusive)) {
        return Ok(None);
    }
    Ok(Some((lower.into_bound(), upper.into_bound())))
}
