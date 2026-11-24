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

// All offsets come from positions() decoding this same fixed-width timestamp.
fn write_time(bytes: &mut [u8], offset: usize, time: Timestamp) {
    bytes[offset..offset + 8].copy_from_slice(&time.as_millis().to_le_bytes());
}

// Report whether the encoded row changed so the caller can reuse its typed index metadata.
pub(crate) fn save(
    schema: &EntitySchema,
    bytes: &mut [u8],
    old: Option<&[u8]>,
    upsert: bool,
    now: Timestamp,
) -> Result<bool, Error> {
    let stored = old.map(|row| values(schema, row)).transpose()?;
    if let Some(times) = stored {
        validate_order(times)?;
        if Some(&*bytes) == old {
            return Ok(false);
        }
    }

    let supplied = positions(schema, bytes)?;
    let mut rewritten = false;
    for (slot, field) in supplied.into_iter().enumerate() {
        let Some((offset, proposed)) = field else {
            continue;
        };
        let expected = stored.and_then(|times| times[slot]).unwrap_or(now);
        if proposed != expected {
            if !upsert {
                return Err(Error::ManagedField);
            }
            write_time(bytes, offset, expected);
            rewritten = true;
        }
    }

    let Some(stored) = stored else {
        return Ok(rewritten);
    };
    // A retry can carry the original request's timestamps.
    if rewritten && Some(&*bytes) == old {
        return Ok(true);
    }
    if stored.into_iter().flatten().any(|value| now < value) {
        return Err(Error::TimestampRegression);
    }
    let updated = ManagedTimestamp::UpdatedAt.slot();
    if let Some((offset, _)) = supplied[updated]
        && stored[updated] != Some(now)
    {
        write_time(bytes, offset, now);
        rewritten = true;
    }
    Ok(rewritten)
}
