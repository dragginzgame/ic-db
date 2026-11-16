//! Complete bounded sorting; no prefix answers, cursor state or generic sorter.
use super::{
    MAX_COMPARISON_KEY, MAX_STATE_BYTES, Plan, QueryPage, Request, admit_output, copy_projection,
    projection_size,
};
use crate::{EntitySchema, Error, IndexValue};

const MAX_ROWS: usize = 32;

#[derive(Default)]
pub(super) struct Collection {
    rows: Vec<Row>,
    state_bytes: usize,
}

struct Row {
    key: Vec<u8>,
    bytes: Vec<u8>,
    // Key prefix before the unique primary tie-breaker. Sorted DISTINCT admits
    // exactly the projected fields here, so equality needs no second key buffer.
    projected_key_end: usize,
}

impl Collection {
    pub(super) fn push(
        &mut self,
        entity: &EntitySchema,
        plan: &Plan<'_>,
        values: &[IndexValue<'_>],
        bytes: &[u8],
        spans: &[(u16, u16)],
    ) -> Result<(), Error> {
        if self.rows.len() == MAX_ROWS {
            return Err(Error::Limit);
        }
        let mut key = Vec::new();
        let mut projected_key_end = 0;
        for &(position, descending) in plan.sort.as_ref().ok_or(Error::InvalidQuery)? {
            if entity.fields[position].name == entity.primary_key {
                projected_key_end = key.len();
            }
            let start = key.len();
            values[position].append_component(entity.fields[position].optional, &mut key);
            if key.len() > MAX_COMPARISON_KEY {
                return Err(Error::Limit);
            }
            // Components are prefix-free (including text terminators and absence).
            // Complementing one component reverses it without changing later ties.
            if descending {
                for byte in &mut key[start..] {
                    *byte = !*byte;
                }
            }
        }
        let size = plan
            .projection
            .map_or(bytes.len(), |mask| projection_size(spans, mask));
        if self.state_bytes + key.capacity() + size > MAX_STATE_BYTES {
            return Err(Error::Limit);
        }
        let row = plan.projection.map_or_else(
            || bytes.to_vec(),
            |mask| copy_projection(bytes, spans, mask, size),
        );
        let charge = key.capacity() + row.capacity();
        if self.state_bytes + charge > MAX_STATE_BYTES {
            return Err(Error::Limit);
        }
        self.state_bytes += charge;
        self.rows.push(Row {
            key,
            bytes: row,
            projected_key_end,
        });
        Ok(())
    }

    // Only called after the scanner observes exhaustion. Keys include the unique
    // primary tie-breaker; compare admitted bytes without decoding inside the sort.
    pub(super) fn finish(
        mut self,
        request: &Request<'_>,
        distinct: bool,
        page: &mut QueryPage<Vec<u8>>,
    ) -> Result<(), Error> {
        self.rows.sort_unstable_by(|left, right| {
            if request.descending {
                right.key.cmp(&left.key)
            } else {
                left.key.cmp(&right.key)
            }
        });
        if distinct {
            self.rows.dedup_by(|left, right| {
                left.key[..left.projected_key_end] == right.key[..right.projected_key_end]
            });
        }
        for row in self.rows.into_iter().take(request.rows) {
            admit_output(row.bytes.len(), request.bytes, &mut page.output_bytes)?;
            page.rows.push(row.bytes);
        }
        Ok(())
    }
}
