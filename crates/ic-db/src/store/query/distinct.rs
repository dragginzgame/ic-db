//! Projected equality: adjacent live pages or one bounded complete collection.
use super::{
    MAX_DISTINCT_VALUES, MAX_STATE_BYTES, Request, admit_ordered_fields, admit_output,
    comparison_key, consume_index_component, copy_projection, projection_size,
};
use crate::{EntitySchema, Error, IndexSchema, IndexValue};
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Strategy {
    Identity,
    Adjacent(usize),
    Complete,
    Sorted { prefix_len: usize },
}

fn complete(request: &Request<'_>) -> Result<Strategy, Error> {
    if request.cursor.is_some() {
        return Err(Error::InvalidQuery);
    }
    Ok(Strategy::Complete)
}

pub(super) fn admit(
    entity: &EntitySchema,
    request: &Request<'_>,
    projection: Option<u16>,
    index: Option<&IndexSchema>,
    fixed: usize,
    sorted: bool,
) -> Result<Strategy, Error> {
    let Some(mut remaining) = projection else {
        return Ok(Strategy::Identity);
    };
    let position = |name: &str| entity.field_position(name).ok_or(Error::InvalidQuery);
    let primary = 1 << position(entity.primary_key)?;
    if remaining & primary != 0 {
        return Ok(Strategy::Identity);
    }
    admit_ordered_fields(entity, remaining)?;
    // Fixed fields and projected fields together can identify one matching owner.
    let mut constants = 0_u16;
    for (name, _) in super::order::literal_equalities(request) {
        constants |= 1 << position(name)?;
    }
    let prefix_fields = index.map_or(&[][..], |selected| &selected.fields[..fixed]);
    for name in prefix_fields {
        constants |= 1 << position(name)?;
    }
    if constants & primary != 0 {
        return Ok(Strategy::Identity);
    }
    let prefix_values = request
        .index
        .as_ref()
        .map_or(&[][..], |(_, values)| values.as_slice());
    let known = remaining | constants;
    if entity.indexes.iter().any(|unique| {
        unique.unique
            && unique.fields.iter().all(|name| {
                entity
                    .field_position(name)
                    .is_some_and(|i| known & (1 << i) != 0)
            })
            && (index.is_some_and(|selected| selected.id == unique.id)
                || super::order::complete_membership(
                    entity,
                    unique,
                    prefix_fields,
                    prefix_values,
                    request,
                ))
    }) {
        return Ok(Strategy::Identity);
    }
    if sorted {
        // Constant components cannot distinguish two matching rows. Stop the
        // key once it distinguishes every varying component of the projection.
        remaining &= !constants;
        let order = request.order.as_ref().ok_or(Error::InvalidQuery)?;
        let mut prefix_len = 0;
        while remaining != 0 {
            let name = order.get(prefix_len).ok_or(Error::InvalidQuery)?;
            let bit = 1 << position(name)?;
            if (constants | remaining) & bit == 0 {
                return Err(Error::InvalidQuery);
            }
            remaining &= !bit;
            prefix_len += 1;
        }
        return Ok(Strategy::Sorted { prefix_len });
    }
    let Some(index) = index else {
        return complete(request);
    };
    // Every predicate must have the same answer throughout a projected class.
    // A class is then either wholly rejected or emitted before an exclusive
    // continuation can advance into it. Other filters use the complete collection.
    for predicate in &request.predicates {
        let right = match predicate.operand {
            super::QueryOperand::Field(name) => Some(name),
            super::QueryOperand::Literal(_) | super::QueryOperand::Length(_) => None,
        };
        for name in std::iter::once(predicate.field).chain(right) {
            if remaining & (1 << position(name)?) == 0 && !index.fields[..fixed].contains(&name) {
                return complete(request);
            }
        }
    }
    for name in &index.fields[..fixed] {
        remaining &= !(1 << position(name)?);
    }
    if remaining == 0 {
        return Ok(Strategy::Adjacent(fixed));
    }
    for (i, name) in index.fields.iter().enumerate().skip(fixed) {
        let bit = 1 << position(name)?;
        if remaining & bit == 0 {
            return complete(request);
        }
        remaining &= !bit;
        if remaining == 0 {
            return Ok(Strategy::Adjacent(i + 1));
        }
    }
    complete(request)
}

pub(super) fn boundary<'a>(
    entity: &EntitySchema,
    index: Option<&IndexSchema>,
    strategy: Strategy,
    position: &'a [u8],
) -> Result<Option<&'a [u8]>, Error> {
    let Strategy::Adjacent(components) = strategy else {
        return Ok(None);
    };
    let index = index.ok_or(Error::InvalidQuery)?;
    let mut remaining = position.get(9..).ok_or(Error::InvalidCursor)?;
    for name in index.fields.iter().take(components) {
        consume_index_component(entity, index, name, &mut remaining)?;
    }
    Ok(Some(&position[..position.len() - remaining.len()]))
}

// Only canonical comparison keys and the first matching projected bytes are kept.
// No full entities, replay positions or retained job payloads are introduced.
pub(super) struct Collection {
    mask: u16,
    seen: BTreeSet<Vec<u8>>,
    pub(super) rows: Vec<Vec<u8>>,
    state_bytes: usize,
    pub(super) output_bytes: usize,
}
impl Collection {
    pub(super) fn new(mask: u16) -> Self {
        Self {
            mask,
            seen: BTreeSet::new(),
            rows: Vec::new(),
            state_bytes: 0,
            output_bytes: 0,
        }
    }

    pub(super) fn push(
        &mut self,
        entity: &EntitySchema,
        request: &Request<'_>,
        values: &[IndexValue<'_>],
        bytes: &[u8],
        spans: &[(u16, u16)],
    ) -> Result<(), Error> {
        let key = comparison_key(entity, self.mask, values)?;
        if self.seen.contains(&key) {
            return Ok(());
        }
        if self.rows.len() == MAX_DISTINCT_VALUES || self.rows.len() == request.rows {
            return Err(Error::Limit);
        }
        let size = projection_size(spans, self.mask);
        admit_output(size, request.bytes, &mut self.output_bytes)?;
        if self.state_bytes + key.capacity() + size > MAX_STATE_BYTES {
            return Err(Error::Limit);
        }
        let row = copy_projection(bytes, spans, self.mask, size);
        let charge = key.capacity() + row.capacity();
        if self.state_bytes + charge > MAX_STATE_BYTES {
            return Err(Error::Limit);
        }
        self.state_bytes += charge;
        self.seen.insert(key);
        self.rows.push(row);
        Ok(())
    }
}
