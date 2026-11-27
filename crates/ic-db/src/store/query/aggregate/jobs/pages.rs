//! Bounded pages over immutable completed groups; no additional durable state.

use super::{
    super::{Completed, GroupOrder, Groups, Reducer, admit_output},
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

impl<T> GroupPage<T> {
    fn try_map<U>(self, decode: impl FnMut(T) -> Result<U, Error>) -> Result<GroupPage<U>, Error> {
        Ok(GroupPage {
            rows: self
                .rows
                .into_iter()
                .map(decode)
                .collect::<Result<_, _>>()?,
            next: self.next,
        })
    }
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
    const WIRE_VERSION: u8 = 3;

    /// Encode a fixed 28-byte transport cursor.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(JobHandle::ENCODED_LEN + 2);
        bytes.push(Self::WIRE_VERSION);
        self.handle.append_to(&mut bytes);
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
        if *position == 0 || *version != Self::WIRE_VERSION {
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
            || !matches!(
                record.meta.phase,
                JobOutcome::Complete(()) | JobOutcome::Expired
            ))
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
    if groups.selection.order == GroupOrder::FirstOccurrence {
        let keys = groups.first_occurrences()?;
        let (selected, used) = select(keys.into_iter(), |key| key.len(), bytes, page, handle)?;
        return Ok((selected.try_map(|key| decode(key, Completed::Key))?, used));
    }
    let (page, used) = select_rows(groups, limit, bytes, page, handle)?;

    Ok((page.try_map(|(key, value)| decode(&key, value))?, used))
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
    if matches!(groups.initial, Reducer::Keys(_)) {
        let mut keys = groups
            .rows
            .into_values()
            .map(|(key, _)| (key, Completed::Key));
        let mut reversed;
        let ordered: &mut dyn ExactSizeIterator<Item = _> =
            if groups.selection.order == GroupOrder::KeyDescending {
                reversed = keys.rev();
                &mut reversed
            } else {
                &mut keys
            };

        return select(ordered, |(key, _)| key.len(), bytes, page, handle);
    }
    // Finalize every group before selection; even an excluded overflow must refuse.
    let rows = groups.into_output(limit)?;
    select(
        rows.into_iter(),
        |(key, value)| key.len() + value.encoded_len(),
        bytes,
        page,
        handle,
    )
}

fn select<T>(
    rows: impl ExactSizeIterator<Item = T>,
    size: impl Fn(&T) -> usize,
    bytes: usize,
    page: Option<PageRequest<'_>>,
    handle: &JobHandle,
) -> Result<(GroupPage<T>, usize), Error> {
    let start = page
        .and_then(|page| page.after)
        .map_or(0, |cursor| usize::from(cursor.position));
    let total = rows.len();
    if start > total {
        return Err(Error::InvalidCursor);
    }
    let capacity = page.map_or(total, |page| page.size);
    let mut selected = Vec::with_capacity(capacity.min(total - start));
    let mut used = 0;
    for row in rows.skip(start).take(capacity) {
        match admit_output(size(&row), bytes, &mut used) {
            Ok(()) => selected.push(row),
            // The cursor stays before the uncharged row so the next page retries it.
            Err(Error::Limit | Error::RowTooLarge) if page.is_some() && !selected.is_empty() => {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    let end = start + selected.len();
    let next = (end < total).then(|| GroupCursor {
        handle: handle.clone(),
        position: end as u8,
    });

    Ok((
        GroupPage {
            rows: selected,
            next,
        },
        used,
    ))
}
