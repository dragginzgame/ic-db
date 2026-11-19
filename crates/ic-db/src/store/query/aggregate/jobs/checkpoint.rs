//! Query-bound checkpoint payloads; the store owns only the enclosing bounded frame.

use super::{
    super::{
        Accumulator, Groups, MAX_COMPARISON_KEY, MAX_DISTINCT_VALUES, MAX_GROUPS, MAX_STATE_BYTES,
        Reducer, comparison_key,
    },
    storage::{MAX_JOB_BYTES, Record},
};
use crate::{
    EntitySchema, Error, IndexValue,
    codec::{Decoder, Encoder, MAX_ROW_BYTES},
};
use std::collections::BTreeSet;

impl Accumulator<'_> {
    pub(super) fn restore(&mut self, entity: &EntitySchema, record: &Record) -> Result<(), Error> {
        match self {
            // Existence is synchronous-only and has no admitted durable payload.
            Self::Exists(_) => Err(Error::InvalidQuery),
            Self::Groups(groups) => groups.restore(entity, record),
            Self::Scalar(reducer @ Reducer::Extremum { .. }) => {
                reducer.restore_extremum(entity, record.count, &record.state)
            }
            Self::Scalar(Reducer::Distinct { mask, seen }) => {
                let mut reader = Decoder::with_byte_limit(&record.state, MAX_JOB_BYTES)?;
                *seen = read_distinct(&mut reader, entity, *mask)?;
                if seen.len() as u64 != record.count {
                    return Err(Error::Corrupt);
                }

                reader.finish()
            }
            Self::Scalar(reducer) => {
                let total: [u8; 32] = record
                    .state
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::Corrupt)?;

                reducer.restore_job(record.count, total)
            }
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
                let (count, total) = reducer.job_state()?;

                (count, total.to_vec())
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

    fn job_state(&self) -> Result<(u64, [u8; 32]), Error> {
        match self {
            Self::Count { total, .. } => Ok((*total, [0; 32])),
            Self::Numeric(state) => Ok(state.checkpoint()),
            _ => Err(Error::InvalidQuery),
        }
    }

    fn restore_job(&mut self, count: u64, total: [u8; 32]) -> Result<(), Error> {
        if count == 0 && total != [0; 32] {
            return Err(Error::Corrupt);
        }
        match self {
            Self::Count { total: value, .. } => {
                if total != [0; 32] {
                    return Err(Error::Corrupt);
                }
                *value = count;
            }
            Self::Numeric(state) => state.restore(count, total),
            _ => return Err(Error::InvalidQuery),
        }

        Ok(())
    }
}
impl Groups {
    fn checkpoint(&self) -> Result<(u64, Vec<u8>), Error> {
        let mut writer = Encoder::with_byte_limit(MAX_JOB_BYTES);
        writer.nat8(self.rows.len() as u8)?;
        let mut count = 0_u64;
        // Store only original key-record bytes; reconstruct comparison keys through
        // the existing codec on resume, rather than persisting two key representations.
        for (bytes, reducer) in self.rows.values() {
            writer.bytes(bytes)?;
            let present = if let Reducer::Distinct { seen, .. } = reducer {
                write_distinct(&mut writer, seen)?;

                seen.len() as u64
            } else if let Reducer::Extremum { selected, .. } = reducer {
                writer.boolean(selected.is_some())?;
                if let Some((_, bytes)) = selected {
                    writer.bytes(bytes)?;
                }

                u64::from(selected.is_some())
            } else {
                let (present, total) = reducer.job_state()?;
                writer.nat64(present)?;
                if matches!(reducer, Reducer::Numeric(_)) {
                    writer.bytes(&total)?;
                }

                present
            };
            count = count.checked_add(present).ok_or(Error::Corrupt)?;
        }

        Ok((count, writer.finish()))
    }

    fn restore(&mut self, entity: &EntitySchema, record: &Record) -> Result<(), Error> {
        let mut reader = Decoder::with_byte_limit(&record.state, MAX_JOB_BYTES)?;
        let length = usize::from(reader.nat8()?);
        if length > MAX_GROUPS || length as u64 > record.examined {
            return Err(Error::Corrupt);
        }
        let mut count = 0_u64;
        for _ in 0..length {
            let bytes = reader.blob(MAX_ROW_BYTES)?;
            let mut reducer = self.initial.clone();
            let present = if let Reducer::Distinct { mask, seen } = &mut reducer {
                *seen = read_distinct(&mut reader, entity, *mask)?;
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
                let total = if matches!(reducer, Reducer::Numeric(_)) {
                    reader.blob(32)?.try_into().map_err(|_| Error::Corrupt)?
                } else {
                    if present == 0 && matches!(reducer, Reducer::Count { presence: None, .. }) {
                        return Err(Error::Corrupt);
                    }

                    [0; 32]
                };
                reducer.restore_job(present, total)?;

                present
            };
            count = count.checked_add(present).ok_or(Error::Corrupt)?;
            let mut fields = Decoder::new(&bytes)?;
            let mut values = [IndexValue::Absent; crate::schema::MAX_FIELDS];
            for (i, field) in entity.fields.iter().enumerate() {
                if self.mask & (1 << i) != 0 && (!field.optional || fields.boolean()?) {
                    values[i] = IndexValue::read(&mut fields, field.value)?;
                }
            }
            fields.finish()?;
            let key = comparison_key(entity, self.mask, &values)?;
            if self
                .rows
                .last_key_value()
                .is_some_and(|(previous, _)| *previous >= key)
            {
                return Err(Error::Corrupt);
            }
            self.state_bytes += key.capacity() + bytes.capacity() + reducer.retained_bytes();
            if self.state_bytes > MAX_STATE_BYTES {
                return Err(Error::Corrupt);
            }
            self.rows.insert(key, (bytes, reducer));
        }
        reader.finish()?;
        if count != record.count {
            return Err(Error::Corrupt);
        }

        Ok(())
    }
}

fn write_distinct(writer: &mut Encoder, seen: &BTreeSet<Vec<u8>>) -> Result<(), Error> {
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
) -> Result<BTreeSet<Vec<u8>>, Error> {
    let count = usize::from(reader.nat8()?);
    if count > MAX_DISTINCT_VALUES {
        return Err(Error::Corrupt);
    }
    let mut seen = BTreeSet::new();
    for _ in 0..count {
        let bytes = reader.blob(MAX_COMPARISON_KEY)?;
        let mut remaining = bytes.as_slice();
        for (i, field) in entity.fields.iter().enumerate() {
            if mask & (1 << i) == 0 {
                continue;
            }
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
        if !remaining.is_empty() || seen.last().is_some_and(|previous| previous >= &bytes) {
            return Err(Error::Corrupt);
        }
        seen.insert(bytes);
    }

    Ok(seen)
}
