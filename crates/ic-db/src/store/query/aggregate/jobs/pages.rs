//! Bounded pages over immutable completed groups; no additional durable state.
use super::{
    super::{Completed, Groups, MAX_GROUPS, admit_output},
    JobHandle, JobOutcome,
    storage::Record,
};
use crate::Error;

/// One page of completed retained groups. Keep `next` when mapping `rows`.
#[derive(Debug)]
pub struct GroupPage<T> {
    /// Completed groups in the bound query's selected total order.
    pub rows: Vec<T>,
    /// None marks the end of the selected result, including the query's total limit.
    pub next: Option<GroupCursor>,
}

/// Logical position in one completed retained job, not an authorization token.
/// Positions are revalidated against the selected result; caller edits can skip
/// rows intentionally but cannot change the result or advance input progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupCursor {
    handle: JobHandle,
    position: u8,
}
impl GroupCursor {
    /// Encode a fixed 28-byte transport cursor.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![1];
        bytes.extend(self.handle.to_bytes());
        bytes.push(self.position);
        bytes
    }

    /// Decode bounded framing; resume also checks the live job and result bounds.
    /// # Errors
    /// Refuses unsupported versions, malformed handles or positions outside 1..=32.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != 28 || bytes[0] != 1 || !(1..=MAX_GROUPS as u8).contains(&bytes[27]) {
            return Err(Error::InvalidCursor);
        }
        Ok(Self {
            handle: JobHandle::from_bytes(&bytes[1..27]).map_err(|_| Error::InvalidCursor)?,
            position: bytes[27],
        })
    }
}

#[derive(Clone, Copy)]
pub(in crate::store::query::aggregate) struct PageRequest<'a> {
    pub size: usize,
    pub after: Option<&'a GroupCursor>,
}

pub(super) fn admit(
    page: Option<PageRequest<'_>>,
    handle: &JobHandle,
    record: &Record,
) -> Result<(), Error> {
    if let Some(cursor) = page.and_then(|page| page.after)
        && (cursor.handle != *handle
            || !matches!(record.phase, JobOutcome::Complete(()) | JobOutcome::Expired))
    {
        return Err(Error::InvalidCursor);
    }
    Ok(())
}

pub(super) fn group_page<T>(
    groups: Groups,
    limit: usize,
    bytes: usize,
    page: Option<PageRequest<'_>>,
    handle: &JobHandle,
    decode: fn(&[u8], Completed) -> Result<T, Error>,
) -> Result<(GroupPage<T>, usize), Error> {
    // Finalize every group before selection; even an excluded overflow must refuse.
    let mut rows = groups.into_output()?;
    rows.truncate(limit);
    let (range, next, used) = select(&rows, bytes, page, handle)?;
    let rows = rows
        .drain(range)
        .map(|(key, value)| decode(&key, value))
        .collect::<Result<_, _>>()?;
    Ok((GroupPage { rows, next }, used))
}

fn select(
    rows: &[(Vec<u8>, Completed)],
    bytes: usize,
    page: Option<PageRequest<'_>>,
    handle: &JobHandle,
) -> Result<(std::ops::Range<usize>, Option<GroupCursor>, usize), Error> {
    let start = page
        .and_then(|page| page.after)
        .map_or(0, |cursor| usize::from(cursor.position));
    if start > rows.len() {
        return Err(Error::InvalidCursor);
    }
    let capacity = page.map_or(rows.len(), |page| page.size);
    let mut end = start;
    let mut used = 0;
    for (key, value) in rows.iter().skip(start).take(capacity) {
        let size = key.len() + value.encoded_len();
        if page.is_some() && size > bytes - used {
            if end == start {
                return Err(Error::RowTooLarge);
            }
            break; // Retry the unreturned group on the next page.
        }
        admit_output(size, bytes, &mut used)?;
        end += 1;
    }
    let next = (end < rows.len()).then(|| GroupCursor {
        handle: handle.clone(),
        position: end as u8,
    });
    Ok((start..end, next, used))
}
