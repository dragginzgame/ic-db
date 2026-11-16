//! Complete bounded sorting; no prefix answers, cursor state or generic sorter.
use super::{
    MAX_COMPARISON_KEY, MAX_STATE_BYTES, Plan, QueryPage, Request, admit_output, copy_projection,
    projection_size,
};
use crate::{EntitySchema, Error, IndexValue};

const MAX_ROWS: usize = 32;

#[derive(Default)]
pub(super) struct Collection {
    rows: Vec<(Vec<u8>, Vec<u8>)>,
    state_bytes: usize,
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
        for &position in plan.sort.as_ref().ok_or(Error::InvalidQuery)? {
            values[position].append_component(entity.fields[position].optional, &mut key);
            if key.len() > MAX_COMPARISON_KEY {
                return Err(Error::Limit);
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
        self.rows.push((key, row));
        Ok(())
    }

    // Only called after the scanner observes exhaustion. Keys include the unique
    // primary tie-breaker; compare admitted bytes without decoding inside the sort.
    pub(super) fn finish(
        mut self,
        request: &Request<'_>,
        page: &mut QueryPage<Vec<u8>>,
    ) -> Result<(), Error> {
        self.rows.sort_unstable_by(|(left, _), (right, _)| {
            if request.descending {
                right.cmp(left)
            } else {
                left.cmp(right)
            }
        });
        for (_, row) in self.rows.into_iter().take(request.rows) {
            admit_output(row.len(), request.bytes, &mut page.output_bytes)?;
            page.rows.push(row);
        }
        Ok(())
    }
}
