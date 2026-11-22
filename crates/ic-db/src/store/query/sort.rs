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
    key: Box<[u8]>,
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
        key: &mut Vec<u8>,
    ) -> Result<(), Error> {
        key.clear();
        let order = plan.sort.as_ref().ok_or(Error::InvalidQuery)?;
        // Trailing fields only order representatives of the same projected
        // value. Its leading components suffice for both ranking and equality.
        for field in order.iter().take(self.prefix_len.unwrap_or(order.len())) {
            let start = key.len();
            let position = field.position;
            let optional = entity.fields[position].optional;
            if let Some(member) = &field.member {
                let (start, end) = spans[position];
                let value = member.read(&bytes[usize::from(start)..usize::from(end)], optional)?;
                value.append_component(optional || member.field().optional, key);
            } else {
                values[position].append_component(optional, key);
            }
            if key.len() > MAX_COMPARISON_KEY {
                return Err(Error::Limit);
            }
            // Components are prefix-free (including text terminators and absence).
            // Complementing one component reverses it without changing later ties.
            if field.descending {
                for byte in &mut key[start..] {
                    *byte = !*byte;
                }
            }
        }
        // The projected tuple decides the order between distinct results. Once
        // evicted, a tuple ranks below every retained tuple, even if seen again
        // with a different primary tie-breaker.
        let compare = |row: &Row| {
            if self.descending {
                key.as_slice().cmp(&row.key)
            } else {
                row.key.as_ref().cmp(key.as_slice())
            }
        };
        let candidates = if self.rows.len() == self.limit {
            let (worst, better) = self.rows.split_last().ok_or(Error::InvalidQuery)?;
            if !compare(worst).is_gt() {
                return Ok(());
            }
            better
        } else {
            self.rows.as_slice()
        };
        let position = match candidates.binary_search_by(compare) {
            // Full row keys include the primary tie-breaker. Equal projected
            // keys in sorted DISTINCT already have a representative.
            Ok(_) => return Ok(()),
            Err(position) => position,
        };
        if self.rows.len() == self.limit {
            // Release the worst retained output before charging its replacement.
            let removed = self.rows.pop().ok_or(Error::InvalidQuery)?;
            self.state_bytes -= removed.key.len() + removed.bytes.capacity();
            drop(removed);
        }
        let size = plan
            .projection
            .map_or(bytes.len(), |mask| projection_size(spans, mask));
        if self.state_bytes + key.len() + size > MAX_STATE_BYTES {
            return Err(Error::Limit);
        }
        let row = plan.projection.map_or_else(
            || bytes.to_vec(),
            |mask| copy_projection(bytes, spans, mask, size),
        );
        let charge = key.len() + row.capacity();
        if self.state_bytes + charge > MAX_STATE_BYTES {
            return Err(Error::Limit);
        }
        self.state_bytes += charge;
        self.rows.insert(
            position,
            Row {
                key: Box::from(key.as_slice()),
                bytes: row,
            },
        );

        Ok(())
    }

    pub(super) fn result_count(&self) -> usize {
        self.rows.len()
    }

    // Only called after the scanner observes exhaustion. Retained keys are unique.
    pub(super) fn finish(
        self,
        byte_limit: usize,
        page: &mut QueryPage<Vec<u8>>,
    ) -> Result<(), Error> {
        for row in self.rows {
            admit_output(row.bytes.len(), byte_limit, &mut page.output_bytes)?;
            page.rows.push(row.bytes);
        }

        Ok(())
    }
}
