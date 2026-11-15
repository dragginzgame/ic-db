//! Query-bound checkpoint payloads; the store owns only the enclosing bounded frame.
use super::super::{Accumulator, Groups, MAX_GROUPS, MAX_STATE_BYTES, Reducer, comparison_key};
use super::storage::Record;
use crate::{
    EntitySchema, Error, IndexValue,
    codec::{Decoder, Encoder, MAX_ROW_BYTES},
};

impl Accumulator<'_> {
    pub(super) fn restore(&mut self, entity: &EntitySchema, record: &Record) -> Result<(), Error> {
        match self {
            Self::Groups(groups) => groups.restore(entity, record),
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

    pub(super) fn checkpoint(&self, record: &mut Record) -> Result<(), Error> {
        (record.count, record.state) = match self {
            Self::Groups(groups) => groups.checkpoint()?,
            Self::Scalar(reducer) => {
                let (count, total) = reducer.job_state()?;
                (count, total.to_vec())
            }
        };
        Ok(())
    }
}
impl Reducer {
    fn job_state(&self) -> Result<(u64, [u8; 32]), Error> {
        match self {
            Self::Count {
                presence: None,
                total,
            } => Ok((*total, [0; 32])),
            Self::Numeric(state) => Ok(state.checkpoint()),
            _ => Err(Error::InvalidQuery),
        }
    }

    fn restore_job(&mut self, count: u64, total: [u8; 32]) -> Result<(), Error> {
        if count == 0 && total != [0; 32] {
            return Err(Error::Corrupt);
        }
        match self {
            Self::Count {
                presence: None,
                total: value,
            } => {
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
        let mut writer = Encoder::new();
        writer.nat8(self.rows.len() as u8)?;
        let mut count = 0_u64;
        // Store only original key-record bytes; reconstruct comparison keys through
        // the existing codec on resume, rather than persisting two key representations.
        for (bytes, reducer) in self.rows.values() {
            let (present, total) = reducer.job_state()?;
            writer.bytes(bytes)?;
            writer.nat64(present)?;
            if matches!(reducer, Reducer::Numeric(_)) {
                writer.bytes(&total)?;
            }
            count = count.checked_add(present).ok_or(Error::Corrupt)?;
        }
        Ok((count, writer.finish()))
    }

    fn restore(&mut self, entity: &EntitySchema, record: &Record) -> Result<(), Error> {
        let mut reader = Decoder::new(&record.state)?;
        let length = usize::from(reader.nat8()?);
        if length > MAX_GROUPS || length as u64 > record.examined {
            return Err(Error::Corrupt);
        }
        let mut count = 0_u64;
        for _ in 0..length {
            let bytes = reader.blob(MAX_ROW_BYTES)?;
            let present = reader.nat64()?;
            let mut reducer = self.initial.clone();
            let total = if matches!(reducer, Reducer::Numeric(_)) {
                reader.blob(32)?.try_into().map_err(|_| Error::Corrupt)?
            } else {
                if present == 0 {
                    return Err(Error::Corrupt);
                }
                [0; 32]
            };
            reducer.restore_job(present, total)?;
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
