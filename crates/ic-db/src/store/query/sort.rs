//! Complete bounded sorting; no prefix answers, cursor state or generic sorter.
use super::{
    MAX_COMPARISON_KEY, MAX_STATE_BYTES, Plan, QueryPage, Request, admit_output, copy_projection,
    projection_size,
};
use crate::{EntitySchema, Error, IndexValue};

pub(super) struct Collection {
    rows: Vec<Row>,
    state_bytes: usize,
    limit: usize,
    descending: bool,
    prefix_len: Option<usize>,
}

struct Row {
    key: Vec<u8>,
    bytes: Vec<u8>,
}

impl Collection {
    pub(super) fn new(request: &Request<'_>, strategy: super::distinct::Strategy) -> Self {
        Self {
            rows: Vec::new(),
            state_bytes: 0,
            limit: request.rows,
            descending: request.descending,
            prefix_len: match strategy {
                super::distinct::Strategy::Sorted { prefix_len } => Some(prefix_len),
                _ => None,
            },
        }
    }

    pub(super) fn push(
        &mut self,
        entity: &EntitySchema,
        plan: &Plan<'_>,
        values: &[IndexValue<'_>],
        bytes: &[u8],
        spans: &[(u16, u16)],
    ) -> Result<(), Error> {
        let mut key = Vec::new();
        let order = plan.sort.as_ref().ok_or(Error::InvalidQuery)?;
        // Trailing fields only order representatives of the same projected
        // value. Its leading components suffice for both ranking and equality.
        for &(position, descending) in order.iter().take(self.prefix_len.unwrap_or(order.len())) {
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
        // The projected tuple decides the order between distinct results. Once
        // evicted, a tuple ranks below every retained tuple, even if seen again
        // with a different primary tie-breaker.
        if self.prefix_len.is_some() && self.rows.iter().any(|row| row.key == key) {
            return Ok(());
        }
        if self.rows.len() == self.limit {
            // At most 32 retained rows: find the worst cached key directly.
            let (worst, row) = self
                .rows
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| {
                    if self.descending {
                        b.key.cmp(&a.key)
                    } else {
                        a.key.cmp(&b.key)
                    }
                })
                .ok_or(Error::InvalidQuery)?;
            let better = if self.descending {
                key > row.key
            } else {
                key < row.key
            };
            if !better {
                return Ok(());
            }
            // Release the losing output before copying its replacement. A later
            // refusal drops the whole collection, never exposing a partial answer.
            let removed = self.rows.swap_remove(worst);
            self.state_bytes -= removed.key.capacity() + removed.bytes.capacity();
            drop(removed);
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
        self.rows.push(Row { key, bytes: row });
        Ok(())
    }

    // Only called after the scanner observes exhaustion. Retained keys are unique.
    // Compare admitted bytes without decoding inside the sort.
    pub(super) fn finish(
        mut self,
        byte_limit: usize,
        page: &mut QueryPage<Vec<u8>>,
    ) -> Result<(), Error> {
        self.rows.sort_unstable_by(|left, right| {
            if self.descending {
                right.key.cmp(&left.key)
            } else {
                left.key.cmp(&right.key)
            }
        });
        for row in self.rows {
            admit_output(row.bytes.len(), byte_limit, &mut page.output_bytes)?;
            page.rows.push(row.bytes);
        }
        Ok(())
    }
}
