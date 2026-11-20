//! Bounded pages over immutable completed groups; no additional durable state.

use super::{
    super::{Completed, Groups, MAX_GROUPS, Reducer, admit_output},
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
    fn version(position: u8) -> u8 {
        if usize::from(position) > MAX_GROUPS {
            2
        } else {
            1
        }
    }

    /// Encode a fixed 28-byte transport cursor.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![Self::version(self.position)];
        bytes.extend(self.handle.to_bytes());
        bytes.push(self.position);

        bytes
    }

    /// Decode bounded framing; resume also checks the live job and result bounds.
    /// # Errors
    /// Refuses unsupported versions, malformed handles or zero positions. Resume checks the position against the completed result.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let [version, handle @ .., position] = bytes else {
            return Err(Error::InvalidCursor);
        };
        if *position == 0 || *version != Self::version(*position) {
            return Err(Error::InvalidCursor);
        }

        Ok(Self {
            handle: JobHandle::from_bytes(handle).map_err(|_| Error::InvalidCursor)?,
            position: *position,
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
    let (page, used) = select_rows(groups, limit, bytes, page, handle)?;
    let rows = page
        .rows
        .into_iter()
        .map(|(key, value)| decode(&key, value))
        .collect::<Result<_, _>>()?;

    Ok((
        GroupPage {
            rows,
            next: page.next,
        },
        used,
    ))
}

type EncodedPage = GroupPage<(Vec<u8>, Completed)>;

// Keep traversal and ownership of the result buffers outside the typed conversion.
#[inline(never)]
fn select_rows(
    groups: Groups,
    limit: usize,
    bytes: usize,
    page: Option<PageRequest<'_>>,
    handle: &JobHandle,
) -> Result<(EncodedPage, usize), Error> {
    if matches!(groups.initial, Reducer::Keys) {
        // Restored keys are already validated, unique and in their final order.
        let sizes = groups.rows.values().map(|(key, _)| key.len());
        let (range, next, used) = select(sizes, bytes, page, handle)?;
        let rows = groups
            .rows
            .into_values()
            .skip(range.start)
            .take(range.len())
            .map(|(key, _)| (key, Completed::Key))
            .collect();

        return Ok((GroupPage { rows, next }, used));
    }
    // Finalize every group before selection; even an excluded overflow must refuse.
    let mut rows = groups.into_output()?;
    rows.truncate(limit);
    let sizes = rows
        .iter()
        .map(|(key, value)| key.len() + value.encoded_len());
    let (range, next, used) = select(sizes, bytes, page, handle)?;
    rows.truncate(range.end);
    rows.drain(..range.start);

    Ok((GroupPage { rows, next }, used))
}

fn select(
    sizes: impl ExactSizeIterator<Item = usize>,
    bytes: usize,
    page: Option<PageRequest<'_>>,
    handle: &JobHandle,
) -> Result<(std::ops::Range<usize>, Option<GroupCursor>, usize), Error> {
    let start = page
        .and_then(|page| page.after)
        .map_or(0, |cursor| usize::from(cursor.position));
    let total = sizes.len();
    if start > total {
        return Err(Error::InvalidCursor);
    }
    let capacity = page.map_or(total, |page| page.size);
    let mut end = start;
    let mut used = 0;
    for size in sizes.skip(start).take(capacity) {
        if page.is_some() && size > bytes - used {
            if end == start {
                return Err(Error::RowTooLarge);
            }
            break; // Retry the unreturned group on the next page.
        }
        admit_output(size, bytes, &mut used)?;
        end += 1;
    }
    let next = (end < total).then(|| GroupCursor {
        handle: handle.clone(),
        position: end as u8,
    });

    Ok((start..end, next, used))
}
