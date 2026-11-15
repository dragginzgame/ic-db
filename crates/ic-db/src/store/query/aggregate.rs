//! Complete bounded counts over the shared scanner, without durable jobs or row history.
use super::{Plan, Query, QueryPage, copy_projection, execute};
use crate::{Entity, EntitySchema, Error, IndexValue, TypeSchema, Value, codec::Decoder, key};
use ic_storage::Memory;
use std::collections::BTreeMap;

const MAX_GROUP_KEY: usize = 4096;
const MAX_GROUPS: usize = 32;
const MAX_STATE_BYTES: usize = 128 * 1024;

/// An exact completed aggregate and its execution costs. No partial result or cursor.
#[derive(Debug)]
pub struct Aggregate<T> {
    /// Completed value over all matching input rows.
    pub value: T,
    /// Examined primary/index candidates, including residual nonmatches.
    pub examined: usize,
    /// Fetched and validated source rows.
    pub fetched: usize,
    /// Encoded result bytes, excluding transport framing.
    pub output_bytes: usize,
}

/// One complete group. Absent optional key values belong to a group too.
#[derive(Debug)]
pub struct GroupCount<K> {
    /// Projected grouping tuple in an ordinary named record.
    pub key: K,
    /// Matching rows, or present operand values for `group_count_field`.
    pub count: u64,
}

impl<E: Entity, M: Memory> Query<'_, E, M> {
    /// Count all matching rows in one synchronous execution. Empty input yields zero.
    /// `limit` does not limit input; the scalar output occupies eight bytes.
    /// Completion must be observed before `work_limit` is reached; an exact-boundary
    /// scan can conservatively refuse. Resumable aggregation is not implemented yet.
    /// # Errors
    /// Refuses cursors, DISTINCT, explicit ordering and the ordinary query failures.
    /// Work exhaustion returns `Limit`, never a partial count. Too few output bytes
    /// returns `RowTooLarge`.
    /// # Panics
    /// Supplier corruption traps without repair, as for ordinary query execution.
    pub fn count(self) -> Result<Aggregate<u64>, Error> {
        self.scalar_count(None, None)
    }

    /// Count present values of a top-level field, including non-scalar fields.
    /// Empty input and all-absent input yield zero; present empty values count.
    /// Limits, refusals and corruption traps are the same as [`Self::count`].
    /// # Errors
    /// Unknown fields refuse even for empty input. Ordinary count errors apply.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn count_field(self, field: &str) -> Result<Aggregate<u64>, Error> {
        self.scalar_count(None, Some(field))
    }

    /// Count distinct projected tuples, including absent values, without an index.
    /// Uses the key types and state bounds of [`Self::group_count`]. `limit` never
    /// truncates this count. The scalar result occupies eight bytes.
    /// # Errors
    /// Ordinary count/projection errors and group-state exhaustion refuse.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn count_distinct<K: Value>(self) -> Result<Aggregate<u64>, Error> {
        self.scalar_count(Some(K::definition()), None)
    }

    fn scalar_count(
        self,
        output: Option<&TypeSchema>,
        field: Option<&str>,
    ) -> Result<Aggregate<u64>, Error> {
        if self.request.bytes < 8 {
            return Err(Error::RowTooLarge);
        }
        let (counts, page) = self.counts(output, field)?;
        Ok(Aggregate {
            value: if output.is_some() {
                counts.groups.len() as u64
            } else {
                counts.total
            },
            examined: page.examined,
            fetched: page.fetched,
            output_bytes: 8,
        })
    }

    /// Count matching rows grouped by an exact named-record projection.
    /// Keys support ordered scalars/unit enums, including optional absence; no index
    /// is required. Results use ascending tuple order in canonical field-name order.
    /// `limit` selects the first completed groups (1..=32), never input rows.
    /// `byte_limit` bounds key-record bytes plus eight count bytes per group.
    /// Empty input yields no groups. At most 32 groups and 128 KiB of key/output
    /// state may be retained, independently of the requested output limit;
    /// each comparison key is capped at 4096 bytes, independently of index-key limits.
    /// # Errors
    /// Refuses unsupported key types, cursors, DISTINCT and explicit ordering.
    /// Work, group or retained-state exhaustion returns `Limit`, never partial groups.
    /// Completion must be observed before `work_limit`; a boundary scan can refuse.
    /// Ordinary projection/query integrity and byte-limit errors also apply.
    /// # Panics
    /// Supplier corruption traps without repair, as for ordinary query execution.
    pub fn group_count<K: Value>(self) -> Result<Aggregate<Vec<GroupCount<K>>>, Error> {
        self.group_counts(None)
    }

    /// Count present operand values within each projected group. Groups whose
    /// operands are all absent remain in the output with count zero. The operand
    /// may be any top-level stored field and need not belong to the group key.
    /// Bounds and ordering are the same as [`Self::group_count`].
    /// # Errors
    /// Unknown operands and ordinary grouped-count errors refuse.
    /// # Panics
    /// Supplier corruption traps without repair.
    pub fn group_count_field<K: Value>(
        self,
        field: &str,
    ) -> Result<Aggregate<Vec<GroupCount<K>>>, Error> {
        self.group_counts(Some(field))
    }

    fn group_counts<K: Value>(
        self,
        field: Option<&str>,
    ) -> Result<Aggregate<Vec<GroupCount<K>>>, Error> {
        let limit = self.request.rows;
        let byte_limit = self.request.bytes;
        let (counts, page) = self.counts(Some(K::definition()), field)?;
        let mut output_bytes = 0;
        let value = counts
            .groups
            .into_values()
            .take(limit)
            .map(|(bytes, count)| {
                let size = bytes.len() + 8;
                if size > byte_limit {
                    return Err(Error::RowTooLarge);
                }
                if size > byte_limit - output_bytes {
                    return Err(Error::Limit);
                }
                output_bytes += size;
                let mut reader = Decoder::new(&bytes)?;
                let key = K::decode_from(&mut reader)?;
                reader.finish()?;
                Ok(GroupCount { key, count })
            })
            .collect::<Result<_, Error>>()?;
        Ok(Aggregate {
            value,
            examined: page.examined,
            fetched: page.fetched,
            output_bytes,
        })
    }

    fn counts(
        self,
        output: Option<&TypeSchema>,
        field: Option<&str>,
    ) -> Result<(Counts, QueryPage<Vec<u8>>), Error> {
        if self.request.cursor.is_some()
            || self.request.distinct
            || self.request.order.is_some()
            || self.request.descending
        {
            return Err(Error::InvalidQuery);
        }
        self.db.schema.admits(E::SCHEMA)?;
        // Presence needs only the validated tag, not an ordered scalar slot.
        let presence = field
            .map(|name| {
                E::SCHEMA
                    .fields
                    .iter()
                    .enumerate()
                    .find(|(_, f)| f.name == name)
                    .map(|(i, f)| f.optional.then_some(i))
                    .ok_or(Error::InvalidQuery)
            })
            .transpose()?
            .flatten();
        let mut plan = Plan::admit(
            E::SCHEMA,
            &self.request,
            self.db.schema.fingerprint(),
            self.db.incarnation,
            output,
        )?;
        if let Some(mask) = plan.projection {
            for (i, field) in E::SCHEMA.fields.iter().enumerate() {
                if mask & (1 << i) != 0 && !key::supports(field.value) {
                    return Err(Error::InvalidQuery);
                }
            }
            plan.selected |= mask;
        }
        let mut counts = Counts {
            mask: plan.projection,
            presence,
            total: 0,
            groups: BTreeMap::new(),
            state_bytes: 0,
        };
        let page = execute(
            &self.db.map,
            E::SCHEMA,
            &self.request,
            &plan,
            Some(&mut counts),
        )?;
        Ok((counts, page))
    }
}

pub(super) struct Counts {
    mask: Option<u16>,
    presence: Option<usize>,
    total: u64,
    groups: BTreeMap<Vec<u8>, (Vec<u8>, u64)>,
    state_bytes: usize,
}
impl Counts {
    pub(super) fn needs_spans(&self) -> bool {
        self.presence.is_some()
    }

    pub(super) fn push(
        &mut self,
        entity: &EntitySchema,
        values: &[IndexValue<'_>],
        bytes: &[u8],
        spans: &[(u16, u16)],
    ) -> Result<(), Error> {
        let increment = u64::from(
            self.presence
                .is_none_or(|i| bytes[usize::from(spans[i].0)] != 0),
        );
        let Some(mask) = self.mask else {
            self.total += increment; // At most MAX_WORK source rows per execution.
            return Ok(());
        };
        let mut key = Vec::new();
        let mut size = 0;
        for (i, field) in entity.fields.iter().enumerate() {
            if mask & (1 << i) != 0 {
                // The validated 4096-byte source row bounds temporary encoding;
                // reject before retaining a key beyond the separate grouping cap.
                values[i].append_component(field.optional, &mut key);
                if key.len() > MAX_GROUP_KEY {
                    return Err(Error::Limit);
                }
                size += usize::from(spans[i].1 - spans[i].0);
            }
        }
        if let Some((_, count)) = self.groups.get_mut(&key) {
            *count += increment;
            return Ok(());
        }
        let output_size = size + 8;
        // Include spare key capacity, not just its logical bytes. Map nodes and
        // fixed bookkeeping are independently bounded by MAX_GROUPS.
        let retained = key.capacity() + output_size;
        if self.groups.len() == MAX_GROUPS || retained > MAX_STATE_BYTES - self.state_bytes {
            return Err(Error::Limit);
        }
        let output = copy_projection(bytes, spans, mask, size);
        self.groups.insert(key, (output, increment));
        self.state_bytes += retained;
        Ok(())
    }
}
