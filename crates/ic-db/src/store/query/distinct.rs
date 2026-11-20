//! Projected equality: adjacent live pages or one bounded complete collection.

use super::{
    INDEX_PREFIX_BYTES, MAX_DISTINCT_VALUES, MAX_STATE_BYTES, Request, admit_ordered_fields,
    admit_output, comparison_key, consume_index_component, copy_projection, projection_size,
};
use crate::{EntitySchema, Error, IndexSchema, IndexValue};
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Strategy {
    Identity,
    FirstMatch,
    Adjacent(usize),
    Complete,
    Sorted { prefix_len: usize },
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
    let fields = index.map_or(&[][..], |selected| selected.fields);
    let (prefix_fields, suffix_fields) = fields.split_at(fixed);
    for name in prefix_fields {
        constants |= 1 << position(name)?;
    }
    // On an ordered traversal, one validated match proves the entire projected
    // result for a fresh single-result probe when no projected field can vary.
    if request.single_result_probe
        && request.cursor.is_none()
        && !sorted
        && remaining & !constants == 0
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
    for name in prefix_fields {
        remaining &= !(1 << position(name)?);
    }
    let mut components = fixed;
    for name in suffix_fields {
        let bit = 1 << position(name)?;
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

    Ok(Strategy::Complete)
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
