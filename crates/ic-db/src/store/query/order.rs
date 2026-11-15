//! Prove complete order from bounded schema metadata, without sorting or statistics.
use super::{Comparison, Request};
use crate::{EntitySchema, Error, IndexSchema, IndexValue, key, store::admit_index_prefix};

pub(super) fn select<'a>(
    entity: &'a EntitySchema,
    request: &Request<'_>,
) -> Result<Option<(&'a IndexSchema, Vec<u8>)>, Error> {
    if let Some(order) = &request.order {
        if order.is_empty() {
            return Err(Error::InvalidQuery);
        }
        for (i, name) in order.iter().enumerate() {
            let field = entity
                .fields
                .iter()
                .find(|f| f.name == *name)
                .ok_or(Error::InvalidQuery)?;
            if !key::supports(field.value)
                || order[..i].contains(name)
                || (*name == entity.primary_key && i + 1 != order.len())
            {
                return Err(Error::InvalidQuery);
            }
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
        if !complete_membership(entity, index, values)
            || !satisfies(entity, request, Some(index), values.len())
        {
            return Err(Error::InvalidQuery);
        }
        return Ok(Some((index, prefix)));
    }
    // Keep the existing no-order contract and its cursors unchanged.
    if request.order.is_none() {
        return Ok(None);
    }
    let primary_fits = satisfies(entity, request, None, 0);
    let mut best: Option<(&IndexSchema, Vec<IndexValue<'_>>)> = None;
    for index in entity.indexes {
        let values: Vec<_> = index
            .fields
            .iter()
            .map_while(|name| {
                request
                    .predicates
                    .iter()
                    .find(|p| p.field == *name && p.comparison == Comparison::Eq)
                    .map(|p| p.value)
            })
            .collect();
        if !complete_membership(entity, index, &values)
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
        return Ok(None);
    }
    let (index, values) = best.ok_or(Error::InvalidQuery)?;
    Ok(Some((index, admit_index_prefix(entity, index, &values)?)))
}

// Sparse traversal must cover every possible match. Initially require a present
// equality-prefix operand for every optional indexed field, as explicit reads do.
fn complete_membership(
    entity: &EntitySchema,
    index: &IndexSchema,
    values: &[IndexValue<'_>],
) -> bool {
    !index.present_only
        || index.fields.iter().enumerate().all(|(i, name)| {
            entity
                .fields
                .iter()
                .find(|f| f.name == *name)
                .is_some_and(|field| {
                    !field.optional || values.get(i).is_some_and(|v| *v != IndexValue::Absent)
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
    let fixed = |name: &&str| {
        fields[..prefix_len].contains(name)
            || request
                .predicates
                .iter()
                .any(|p| p.field == *name && p.comparison == Comparison::Eq)
    };
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
