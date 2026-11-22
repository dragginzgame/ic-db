//! Projected equality: adjacent live pages or one bounded complete collection.

use super::{
    INDEX_PREFIX_BYTES, MAX_DISTINCT_VALUES, MAX_STATE_BYTES, Plan, Request, admit_ordered_fields,
    admit_output, comparison_key, consume_index_component, copy_projection, projection_size,
};
use crate::{EntitySchema, Error, IndexSchema, IndexValue};

// Sorted lookup keys. Inserting a new tuple moves at most 31 handles.
pub(super) type DistinctKeys = Vec<Box<[u8]>>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Strategy {
    Identity,
    FirstMatch,
    Adjacent(usize),
    Complete { key_fields: u32 },
    Sorted { prefix_len: usize },
}

// A fixed member does not fix its parent. Only complete top-level fields
// can be omitted from a projected tuple's comparison key.
pub(super) fn fixed_fields(entity: &EntitySchema, request: &Request<'_>, plan: &Plan<'_>) -> u32 {
    let prefix = plan
        .index
        .map_or(&[][..], |index| &index.fields[..plan.fixed_components]);
    let mut fields = 0;
    for position in request
        .fixed_equalities(entity)
        .map(|(name, _)| name)
        .chain(prefix.iter().copied())
        .filter_map(|name| entity.field_position(name))
    {
        fields |= 1 << position;
    }
    fields
}

pub(super) fn admit(
    entity: &EntitySchema,
    request: &Request<'_>,
    plan: &Plan<'_>,
) -> Result<Strategy, Error> {
    let Some(projected) = plan.projection else {
        return Ok(Strategy::Identity);
    };
    let position = |name: &str| entity.field_position(name).ok_or(Error::InvalidQuery);
    let primary = 1 << position(entity.primary_key)?;
    if projected & primary != 0 {
        return Ok(Strategy::Identity);
    }
    admit_ordered_fields(entity, projected)?;
    // Fixed fields and projected fields together can identify one matching owner.
    let index = plan.index;
    let fixed = plan.fixed_components;
    let fields = index.map_or(&[][..], |selected| selected.fields);
    let (prefix_fields, suffix_fields) = fields.split_at(fixed);
    let constants = fixed_fields(entity, request, plan);
    // On an ordered traversal, one validated match proves the entire projected
    // result for a fresh single-result probe when no projected field can vary.
    let varying = projected & !constants;
    if request.single_result_probe
        && request.cursor.is_none()
        && plan.sort.is_none()
        && varying == 0
    {
        return Ok(Strategy::FirstMatch);
    }
    if constants & primary != 0 {
        return Ok(Strategy::Identity);
    }
    let prefix_values = request
        .index
        .as_ref()
        .map_or(&[][..], |(_, values)| values.as_slice());
    let known = projected | constants;
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
    if let Some(order) = &plan.sort {
        // Constant components cannot distinguish two matching rows. Stop the
        // key once it distinguishes every varying component of the projection.
        let mut remaining = varying;
        let mut prefix_len = 0;
        while remaining != 0 {
            let field = order.get(prefix_len).ok_or(Error::InvalidQuery)?;
            let bit = 1 << field.position;
            if field.member.is_some() || remaining & bit == 0 {
                return Err(Error::InvalidQuery);
            }
            remaining &= !bit;
            prefix_len += 1;
        }

        return Ok(Strategy::Sorted { prefix_len });
    }
    let mut remaining = projected;
    for field in prefix_fields
        .iter()
        .filter_map(|name| entity.field_position(name))
    {
        remaining &= !(1 << field);
    }
    let mut components = fixed;
    for name in suffix_fields {
        let Some(field) = entity.field_position(name) else {
            break;
        };
        let bit = 1 << field;
        // Cross a nonprojected constant only to reach a varying projected field.
        // Keep the physical prefix in the cursor comparison, including that constant.
        let crosses_constant = constants & bit != 0 && remaining & !constants != 0;
        if remaining & bit == 0 && !crosses_constant {
            break;
        }
        remaining &= !bit;
        components += 1;
    }
    // A mandatory equality fixes its projected value for every matching owner,
    // even when the selected index ends before that field.
    if remaining & !constants == 0 {
        return Ok(Strategy::Adjacent(components));
    }

    if request.cursor.is_some() {
        return Err(Error::InvalidQuery);
    }

    // The adjacency proof may have consumed a varying index component before
    // failing. The seen set still needs every varying projected component.
    Ok(Strategy::Complete {
        key_fields: varying,
    })
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
    if components == 0 {
        // All matches share one class, including on a primary scan.
        return Ok(Some(&[]));
    }
    let index = index.ok_or(Error::InvalidQuery)?;
    let mut remaining = position
        .get(INDEX_PREFIX_BYTES..)
        .ok_or(Error::InvalidCursor)?;
    for name in index.fields.iter().take(components) {
        consume_index_component(entity, index, name, &mut remaining)?;
    }

    Ok(Some(&position[..position.len() - remaining.len()]))
}

// Only canonical comparison keys and the first matching projected bytes are kept.
// No full entities, replay positions or retained job payloads are introduced.
pub(super) struct Collection {
    projected: u32,
    key_fields: u32,
    seen: DistinctKeys,
    pub(super) rows: Vec<Vec<u8>>,
    state_bytes: usize,
    pub(super) output_bytes: usize,
}
impl Collection {
    pub(super) fn new(projected: u32, key_fields: u32) -> Self {
        Self {
            projected,
            key_fields,
            seen: DistinctKeys::new(),
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
        scratch: &mut Vec<u8>,
    ) -> Result<(), Error> {
        comparison_key(entity, self.key_fields, values, scratch)?;
        let Err(position) = self
            .seen
            .binary_search_by(|key| key.as_ref().cmp(scratch.as_slice()))
        else {
            return Ok(());
        };
        if self.rows.len() == MAX_DISTINCT_VALUES || self.rows.len() == request.rows {
            return Err(Error::Limit);
        }
        let size = projection_size(spans, self.projected);
        admit_output(size, request.bytes, &mut self.output_bytes)?;
        if self.state_bytes + scratch.len() + size > MAX_STATE_BYTES {
            return Err(Error::Limit);
        }
        let row = copy_projection(bytes, spans, self.projected, size);
        let charge = scratch.len() + row.capacity();
        if self.state_bytes + charge > MAX_STATE_BYTES {
            return Err(Error::Limit);
        }
        self.state_bytes += charge;
        self.seen.insert(position, Box::from(scratch.as_slice()));
        self.rows.push(row);

        Ok(())
    }
}
