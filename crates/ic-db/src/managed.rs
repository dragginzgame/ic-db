//! Insert/update time policy over canonical row bytes, without a mutation overlay.

use crate::{EntitySchema, Error, Timestamp, codec::Decoder};

/// Database-owned timestamp assignment during bounded write preparation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum ManagedTimestamp {
    /// Set on insertion and preserve on replacement.
    CreatedAt,
    /// Set on insertion and on a semantic change, preserving no-op timestamps.
    UpdatedAt,
}

impl ManagedTimestamp {
    pub(crate) const fn slot(self) -> usize {
        match self {
            Self::CreatedAt => 0,
            Self::UpdatedAt => 1,
        }
    }
}

#[doc(hidden)]
pub fn validate_order(values: [Option<Timestamp>; 2]) -> Result<(), Error> {
    if matches!(values, [Some(created), Some(updated)] if created > updated) {
        return Err(Error::Corrupt);
    }

    Ok(())
}

// Only these two fixed-width fields need byte positions. All other fields use
// the existing bounded decoder; no owned values or general slot table is built.
fn positions(
    schema: &EntitySchema,
    bytes: &[u8],
) -> Result<[Option<(usize, Timestamp)>; 2], Error> {
    let mut reader = Decoder::new(bytes)?;
    let mut values = [None; 2];
    for field in schema.fields {
        if field.optional && !reader.boolean()? {
            continue;
        }

        if let Some(managed) = field.managed {
            let offset = reader.position();
            values[managed.slot()] = Some((offset, reader.timestamp()?));
        } else {
            reader.skip(field.value)?;
        }
    }
    reader.finish()?;

    Ok(values)
}

// Migration preserves role values even when their declared field names change.
pub(crate) fn values(schema: &EntitySchema, bytes: &[u8]) -> Result<[Option<Timestamp>; 2], Error> {
    Ok(positions(schema, bytes)?.map(|entry| entry.map(|(_, value)| value)))
}

pub(crate) fn validate_insert(
    schema: &EntitySchema,
    bytes: &[u8],
    now: Timestamp,
) -> Result<(), Error> {
    if positions(schema, bytes)?
        .into_iter()
        .flatten()
        .any(|(_, value)| value != now)
    {
        return Err(Error::ManagedField);
    }

    Ok(())
}

pub(crate) fn replace(
    schema: &EntitySchema,
    bytes: &mut [u8],
    old: &[u8],
    now: Timestamp,
) -> Result<bool, Error> {
    let before = positions(schema, old)?;
    validate_order(before.map(|entry| entry.map(|(_, value)| value)))?;
    if bytes == old {
        return Ok(false);
    }
    let after = positions(schema, bytes)?;
    for (previous, supplied) in before.iter().zip(&after) {
        if previous.map(|(_, value)| value) != supplied.map(|(_, value)| value) {
            return Err(Error::ManagedField);
        }
    }
    if before.into_iter().flatten().any(|(_, value)| now < value) {
        return Err(Error::TimestampRegression);
    }
    if let Some((offset, _)) = after[1] {
        bytes
            .get_mut(offset..offset + 8)
            .ok_or(Error::Corrupt)?
            .copy_from_slice(&now.as_millis().to_le_bytes());
    }

    Ok(true)
}
