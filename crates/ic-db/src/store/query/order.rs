//! Select a proven ordered path or a bounded complete-sort fallback.
use super::{Comparison, Request};
use crate::{EntitySchema, Error, IndexSchema, IndexValue, key, store::admit_index_prefix};

// Carry the admitted prefix and its component count together; downstream proofs
// must not reconstruct equality-prefix selection from the request independently.
pub(super) struct IndexPath<'a> {
    pub index: &'a IndexSchema,
    pub prefix: Vec<u8>,
    pub fixed: usize,
}

pub(super) enum Path<'a> {
    Primary,
    Index(IndexPath<'a>),
    Sort(Vec<usize>),
}

pub(super) fn select<'a>(
    entity: &'a EntitySchema,
    request: &Request<'_>,
) -> Result<Path<'a>, Error> {
    // Plan admission bounds the order to four fields. Indexed paths allocate no
    // additional sort metadata; materialize it only for the complete fallback.
    let mut positions = [0; 4];
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
            positions[i] = position;
        }
    }
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
            || !satisfies(entity, request, Some(index), values.len())
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
    if request.order.is_none() {
        return Ok(Path::Primary);
    }
    let primary_fits = satisfies(entity, request, None, 0);
    let mut best: Option<(&IndexSchema, Vec<IndexValue<'_>>)> = None;
    for index in entity.indexes {
        let values: Vec<_> = index
            .fields
            .iter()
            .map_while(|name| equality(request, name))
            .collect();
        if !complete_membership(entity, index, &values, request)
            || !satisfies(entity, request, Some(index), values.len())
        {
            continue;
        }
        // Deterministic, schema-only preference: more fixed leading components,
        // then fewer index fields, then logical name. No data scans or statistics.
        let rank = (
            std::cmp::Reverse(values.len()),
            index.fields.len(),
            index.name,
        );
        if best.as_ref().is_none_or(|(old, old_values)| {
            rank < (
                std::cmp::Reverse(old_values.len()),
                old.fields.len(),
                old.name,
            )
        }) {
            best = Some((index, values));
        }
    }
    // A restrictive prefix avoids unrelated candidates. Otherwise prefer direct
    // primary traversal to fetching rows through an equally broad index scan.
    if primary_fits && best.as_ref().is_none_or(|(_, values)| values.is_empty()) {
        return Ok(Path::Primary);
    }
    let Some((index, values)) = best else {
        // No live cursor or projected DISTINCT semantics are implied by a sort.
        // Explicit paths above still have to prove their requested order.
        if request.cursor.is_some() || request.distinct {
            return Err(Error::InvalidQuery);
        }
        let mut positions =
            positions[..request.order.as_ref().ok_or(Error::InvalidQuery)?.len()].to_vec();
        let primary = entity
            .fields
            .iter()
            .position(|f| f.name == entity.primary_key)
            .ok_or(Error::InvalidQuery)?;
        if positions.last() != Some(&primary) {
            positions.push(primary);
        }
        return Ok(Path::Sort(positions));
    };
    Ok(Path::Index(IndexPath {
        index,
        prefix: admit_index_prefix(entity, index, &values)?,
        fixed: values.len(),
    }))
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
                                clause.iter().all(|predicate| {
                                    predicate.field == *name
                                        && !predicate
                                            .comparison
                                            .accepts_absent(predicate.value == IndexValue::Absent)
                                })
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
    if fixed(&entity.primary_key) {
        return true;
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
            {
                Some(predicate.value)
            } else {
                None
            }
        })
}
