//! Query-bound checkpoint payloads; the store owns only the enclosing bounded frame.

use super::{
    super::{
        Accumulator, DistinctKeys, GroupOrder, Groups, MAX_COMPARISON_KEY, MAX_DISTINCT_VALUES,
        MAX_STATE_BYTES, Reducer,
    },
    storage::{MAX_JOB_BYTES, Record},
};
use crate::store::query::{build_comparison_key, projection_positions};
use crate::{
    EntitySchema, Error, IndexValue,
    codec::{Decoder, Encoder, MAX_ROW_BYTES},
};

impl Accumulator<'_> {
    pub(super) fn restore(&mut self, entity: &EntitySchema, record: &Record) -> Result<(), Error> {
        match self {
            // Existence is synchronous-only and has no admitted durable payload.
            Self::Exists(_) => Err(Error::InvalidQuery),
            Self::Groups(groups) => groups.restore(entity, record),
            Self::Scalar(reducer @ Reducer::Extremum { .. }) => {
                reducer.restore_extremum(entity, record.count, &record.state)
            }
            Self::Scalar(Reducer::Distinct {
                mask,
                seen,
                key_bytes,
            }) => {
                let mut reader = Decoder::with_byte_limit(&record.state, MAX_JOB_BYTES)?;
                (*seen, *key_bytes) = read_distinct(&mut reader, entity, *mask)?;
                if seen.len() as u64 != record.count {
                    return Err(Error::Corrupt);
                }

                reader.finish()
            }
            Self::Scalar(reducer) => reducer.restore_job(record.count, &record.state),
        }
    }

    pub(super) fn checkpoint(&self) -> Result<(u64, Vec<u8>), Error> {
        Ok(match self {
            Self::Exists(_) => return Err(Error::InvalidQuery),
            Self::Groups(groups) => groups.checkpoint()?,
            Self::Scalar(Reducer::Extremum { selected, .. }) => selected
                .as_ref()
                .map_or_else(|| (0, Vec::new()), |(_, bytes)| (1, bytes.clone())),
            Self::Scalar(Reducer::Distinct { seen, .. }) => {
                let mut writer = Encoder::with_byte_limit(MAX_JOB_BYTES);
                write_distinct(&mut writer, seen)?;

                (seen.len() as u64, writer.finish())
            }
            Self::Scalar(reducer) => {
                reducer.with_job_state(|count, total| (count, total.to_vec()))?
            }
        })
    }
}
impl Reducer {
    fn restore_extremum(
        &mut self,
        entity: &EntitySchema,
        count: u64,
        bytes: &[u8],
    ) -> Result<(), Error> {
        let Self::Extremum {
            position, selected, ..
        } = self
        else {
            return Err(Error::InvalidQuery);
        };
        *selected = match count {
            0 if bytes.is_empty() => None,
            1 => {
                let field = &entity.fields[*position];
                let mut reader = Decoder::new(bytes)?;
                if field.optional && !reader.boolean()? {
                    return Err(Error::Corrupt);
                }
                let value = IndexValue::read(&mut reader, field.value)?;
                reader.finish()?;
                let mut key = Vec::new();
                value.append_component(field.optional, &mut key);
                if key.len() > MAX_COMPARISON_KEY {
                    return Err(Error::Corrupt);
                }

                Some((key, bytes.to_vec()))
            }
            _ => return Err(Error::Corrupt),
        };

        Ok(())
    }

    fn with_job_state<T>(&self, write: impl FnOnce(u64, &[u8]) -> T) -> Result<T, Error> {
        match self {
            Self::Count { total, .. } => Ok(write(*total, &[0; 32])),
            Self::Numeric(state) => Ok(state.with_checkpoint(write)),
            _ => Err(Error::InvalidQuery),
        }
    }

    fn restore_job(&mut self, count: u64, bytes: &[u8]) -> Result<(), Error> {
        if count == 0 && bytes.iter().any(|&b| b != 0) {
            return Err(Error::Corrupt);
        }
        match self {
            Self::Count { total, .. } if bytes == [0; 32] => *total = count,
            Self::Count { .. } => return Err(Error::Corrupt),
            Self::Numeric(state) => state.restore(count, bytes)?,
            _ => return Err(Error::InvalidQuery),
        }
        Ok(())
    }
}
impl Groups {
    fn checkpoint(&self) -> Result<(u64, Vec<u8>), Error> {
        let mut writer = Encoder::with_byte_limit(MAX_JOB_BYTES);
        writer.nat8(self.rows.len() as u8)?;
        if self.selection.order == GroupOrder::FirstOccurrence {
            for bytes in self.first_occurrences()? {
                writer.bytes(bytes)?;
            }
            return Ok((self.rows.len() as u64, writer.finish()));
        }
        let mut count = 0_u64;
        // Store only original key-record bytes; reconstruct comparison keys through
        // the existing codec on resume, rather than persisting two key representations.
        for (bytes, reducer) in self.rows.values() {
            writer.bytes(bytes)?;
            let present = if matches!(reducer, Reducer::Keys(_)) {
                1
            } else if let Reducer::Distinct { seen, .. } = reducer {
                write_distinct(&mut writer, seen)?;

                seen.len() as u64
            } else if let Reducer::Extremum { selected, .. } = reducer {
                writer.boolean(selected.is_some())?;
                if let Some((_, bytes)) = selected {
                    writer.bytes(bytes)?;
                }

                u64::from(selected.is_some())
            } else {
                reducer.with_job_state(|present, total| {
                    writer.nat64(present)?;
                    if matches!(reducer, Reducer::Numeric(_)) {
                        writer.bytes(total)?;
                    }
                    Ok::<_, Error>(present)
                })??
            };
            count = count.checked_add(present).ok_or(Error::Corrupt)?;
        }

        Ok((count, writer.finish()))
    }

    fn restore(&mut self, entity: &EntitySchema, record: &Record) -> Result<(), Error> {
        let mut reader = Decoder::with_byte_limit(&record.state, MAX_JOB_BYTES)?;
        let length = usize::from(reader.nat8()?);
        if length > self.max_keys() || length as u64 > record.examined {
            return Err(Error::Corrupt);
        }
        let mut count = 0_u64;
        for position in 0..length {
            let bytes = reader.blob(MAX_ROW_BYTES)?;
            let mut reducer = self.initial.clone();
            let present = if let Reducer::Keys(ordinal) = &mut reducer {
                *ordinal = position as u8;
                1
            } else if let Reducer::Distinct {
                mask,
                seen,
                key_bytes,
            } = &mut reducer
            {
                (*seen, *key_bytes) = read_distinct(&mut reader, entity, *mask)?;
                if seen.is_empty() {
                    return Err(Error::Corrupt);
                }

                seen.len() as u64
            } else if let Reducer::Extremum { position, .. } = &reducer {
                let present = reader.boolean()?;
                if !present && !entity.fields[*position].optional {
                    return Err(Error::Corrupt);
                }
                let winner = if present {
                    reader.blob(MAX_ROW_BYTES)?
                } else {
                    Vec::new()
                };
                reducer.restore_extremum(entity, u64::from(present), &winner)?;

                u64::from(present)
            } else {
                let present = reader.nat64()?;
                let total = if let Reducer::Numeric(state) = &reducer {
                    reader.blob_ref(state.total_bytes())?
                } else {
                    if present == 0 && matches!(reducer, Reducer::Count { presence: None, .. }) {
                        return Err(Error::Corrupt);
                    }

                    &[0; 32]
                };
                reducer.restore_job(present, total)?;

                present
            };
            count = count.checked_add(present).ok_or(Error::Corrupt)?;
            let mut fields = Decoder::new(&bytes)?;
            let mut key = Vec::new();
            build_comparison_key(entity, self.mask, &mut key, |_, field| {
                if field.optional && !fields.boolean()? {
                    Ok(IndexValue::Absent)
                } else {
                    IndexValue::read(&mut fields, field.value)
                }
            })?;
            fields.finish()?;
            let key = key.into_boxed_slice();
            if self.selection.order != GroupOrder::FirstOccurrence
                && self
                    .rows
                    .last_key_value()
                    .is_some_and(|(previous, _)| *previous >= key)
            {
                return Err(Error::Corrupt);
            }
            self.state_bytes += key.len() + bytes.capacity() + reducer.retained_bytes();
            if self.state_bytes > MAX_STATE_BYTES {
                return Err(Error::Corrupt);
            }
            if self.rows.insert(key, (bytes, reducer)).is_some() {
                return Err(Error::Corrupt);
            }
        }
        reader.finish()?;
        if count != record.count {
            return Err(Error::Corrupt);
        }

        Ok(())
    }
}

fn write_distinct(writer: &mut Encoder, seen: &DistinctKeys) -> Result<(), Error> {
    writer.nat8(seen.len() as u8)?;
    for bytes in seen {
        writer.bytes(bytes)?;
    }

    Ok(())
}

fn read_distinct(
    reader: &mut Decoder<'_>,
    entity: &EntitySchema,
    mask: u16,
) -> Result<(DistinctKeys, usize), Error> {
    let count = usize::from(reader.nat8()?);
    if count > MAX_DISTINCT_VALUES {
        return Err(Error::Corrupt);
    }
    let mut seen = DistinctKeys::with_capacity(count);
    let mut key_bytes = 0;
    for _ in 0..count {
        let bytes = reader.blob_ref(MAX_COMPARISON_KEY)?;
        let mut remaining = bytes;
        for i in projection_positions(mask) {
            let field = &entity.fields[i];
            if field.optional {
                let (&tag, rest) = remaining.split_first().ok_or(Error::Corrupt)?;
                remaining = rest;

                match tag {
                    0 => continue,
                    1 => {}
                    _ => return Err(Error::Corrupt),
                }
            }
            crate::key::consume(&mut remaining, field.value)?;
        }
        if !remaining.is_empty()
            || seen
                .last()
                .is_some_and(|previous| previous.as_ref() >= bytes)
        {
            return Err(Error::Corrupt);
        }
        key_bytes += bytes.len();
        seen.push(Box::from(bytes));
    }

    Ok((seen, key_bytes))
}
