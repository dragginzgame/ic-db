//! Private fluent-query witness; production query endpoints belong to Canic.

use crate::{
    console::NoteOnly,
    open, runtime,
    schema::{BinaryObject, DecimalRow, Document, Entry, ListRow},
};
use ic_db::design::prelude::*;
use ic_db::{Comparison, IndexValue, QueryCursor};

// fluent_query
// Return a bounded page of matching fixture entities.
// guard  : validates the request frame for method 95.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_query",
    guard = crate::transport::admit::<95>,
    decode = crate::transport::decode_measured::<_, 95>,
    encode = crate::transport::encode::<_, 95>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_query((request, counter): crate::transport::Measured<Request>) -> Result<Page, u8> {
    read(request, Shape::Entity, counter)
}

// fluent_projection
// Project matching entries into the fixture result record.
// guard  : validates the request frame for method 96.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_projection",
    guard = crate::transport::admit::<96>,
    decode = crate::transport::decode_measured::<_, 96>,
    encode = crate::transport::encode::<_, 96>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_projection((request, counter): crate::transport::Measured<Request>) -> Result<Page, u8> {
    read(request, Shape::Projection, counter)
}

// fluent_ordered
// Page matching entries in the requested order.
// guard  : validates the request frame for method 98.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_ordered",
    guard = crate::transport::admit::<98>,
    decode = crate::transport::decode_measured::<_, 98>,
    encode = crate::transport::encode::<_, 98>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_ordered((request, counter): crate::transport::Measured<Request>) -> Result<Page, u8> {
    read(request, Shape::Ordered, counter)
}

// fluent_any
// Page entries matching the fixture’s alternative predicates.
// guard  : validates the request frame for method 123.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_any",
    guard = crate::transport::admit::<123>,
    decode = crate::transport::decode_measured::<_, 123>,
    encode = crate::transport::encode::<_, 123>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_any((request, counter): crate::transport::Measured<Request>) -> Result<Page, u8> {
    read(request, Shape::Any, counter)
}

// fluent_membership
// Page entries matching the fixture’s membership filter.
// guard  : validates the request frame for method 124.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_membership",
    guard = crate::transport::admit::<124>,
    decode = crate::transport::decode_measured::<_, 124>,
    encode = crate::transport::encode::<_, 124>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_membership((request, counter): crate::transport::Measured<Request>) -> Result<Page, u8> {
    read(request, Shape::Membership, counter)
}

// fluent_text_prefix
// Page matching text prefixes.
// guard  : validates the request frame for method 125.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_text_prefix",
    guard = crate::transport::admit::<125>,
    decode = crate::transport::decode::<_, 125>,
    encode = crate::transport::encode::<_, 125>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_text_prefix(request: TextRequest) -> Result<TextPage, u8> {
    text_match(request, Comparison::StartsWith)
}

// fluent_text_suffix
// Page matching text suffixes.
// guard  : validates the request frame for method 126.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_text_suffix",
    guard = crate::transport::admit::<126>,
    decode = crate::transport::decode::<_, 126>,
    encode = crate::transport::encode::<_, 126>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_text_suffix(request: TextRequest) -> Result<TextPage, u8> {
    text_match(request, Comparison::EndsWith)
}

// fluent_text_contains
// Page rows containing the supplied text.
// guard  : validates the request frame for method 127.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_text_contains",
    guard = crate::transport::admit::<127>,
    decode = crate::transport::decode::<_, 127>,
    encode = crate::transport::encode::<_, 127>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_text_contains(request: TextRequest) -> Result<TextPage, u8> {
    text_match(request, Comparison::Contains)
}

// fluent_text_seek
// Seek document titles by prefix and continue in the requested direction.
// guard  : validates the request frame for method 132.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_text_seek",
    guard = crate::transport::admit::<132>,
    decode = crate::transport::decode::<_, 132>,
    encode = crate::transport::encode::<_, 132>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_text_seek(
    (needle, descending, after, work, bytes): TextSeekRequest,
) -> Result<IdPage, u8> {
    let host = runtime();
    let db = open(&host);
    let cursor = decode_cursor(after.as_deref()).map_err(|error| query_error(&error))?;
    let query = db
        .query::<Document>()
        .using_index("published,title", &[false.into()])
        .filter(Document::TITLE, Comparison::StartsWith, &needle)
        .work_limit(work as usize)
        .test_output_limit(bytes as usize);
    fetch_ids(query, descending, cursor.as_ref())
}

// fluent_primary_range
// Page IDs in the requested primary-key range.
// guard  : validates the request frame for method 130.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_primary_range",
    guard = crate::transport::admit::<130>,
    decode = crate::transport::decode::<_, 130>,
    encode = crate::transport::encode::<_, 130>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_primary_range(request: PrimaryRangeRequest) -> Result<IdPage, u8> {
    range_read(request, None, Entry::ID)
}

// fluent_index_range
// Page IDs in the selected index range, optionally within one category.
// guard  : validates the request frame for method 131.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_index_range",
    guard = crate::transport::admit::<131>,
    decode = crate::transport::decode::<_, 131>,
    encode = crate::transport::encode::<_, 131>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_index_range(
    (after, descending, category, lower, upper, work, bytes): IndexRangeRequest,
) -> Result<IdPage, u8> {
    let request = (after, descending, lower, upper, work, bytes);

    match category {
        Some(category) => range_read(request, Some(&[category.into()]), Entry::ID),
        None => range_read(request, Some(&[]), Entry::CATEGORY),
    }
}

// fluent_sparse_presence
// Page the sparse-index fixture with the requested absence policy.
// guard  : validates the request frame for method 128.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_sparse_presence",
    guard = crate::transport::admit::<128>,
    decode = crate::transport::decode::<_, 128>,
    encode = crate::transport::encode::<_, 128>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_sparse_presence(
    (include_absent, descending, after, bytes): SparseRequest,
) -> Result<IdPage, u8> {
    let host = runtime();
    let db = open(&host);
    let cursor = decode_cursor(after.as_deref()).map_err(|error| query_error(&error))?;
    let query = db
        .query::<DecimalRow>()
        .filter_any(&[
            (DecimalRow::MAYBE, Comparison::Ne, ic_db::IndexValue::Absent),
            (
                DecimalRow::MAYBE,
                if include_absent {
                    Comparison::Eq
                } else {
                    Comparison::Ne
                },
                ic_db::IndexValue::Absent,
            ),
        ])
        .order_asc(DecimalRow::MAYBE)
        .work_limit(1)
        .test_output_limit(bytes as usize);
    fetch_ids(query, descending, cursor.as_ref())
}

// fluent_list_presence
// Page rows under the selected list-presence predicate.
// guard  : validates the request frame for method 129.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_list_presence",
    guard = crate::transport::admit::<129>,
    decode = crate::transport::decode::<_, 129>,
    encode = crate::transport::encode::<_, 129>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_list_presence((mode, after): (u8, Option<Vec<u8>>)) -> Result<IdPage, u8> {
    let host = runtime();
    let db = open(&host);
    let cursor = decode_cursor(after.as_deref()).map_err(|error| query_error(&error))?;
    let query = db.query::<ListRow>().work_limit(1);
    let query = match mode {
        0 => query.filter_absent(ListRow::LABELS),
        1 => query.filter_present(ListRow::LABELS),
        2 => query.filter_length(ListRow::LABELS, Comparison::Eq, 1),
        3 => query.filter_length(ListRow::NUMBERS, Comparison::Eq, 0),
        4 => query.filter(ListRow::LABELS, Comparison::Contains, ""),
        5 => query.filter(ListRow::LABELS, Comparison::Contains, "x"),
        6 => query.filter(ListRow::NUMBERS, Comparison::Contains, 258_u16),
        7 => query.filter(ListRow::NUMBERS, Comparison::Contains, 1_u16),
        _ => return Err(1),
    };
    fetch_ids(query, false, cursor.as_ref())
}

// Same source, predicate, result and warm open; only materialization differs.

// fluent_binary_projection
// Measure entity and ID-only reads of the binary fixture.
// guard  : validates the request frame for method 97.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_binary_projection",
    guard = crate::transport::admit::<97>,
    decode = crate::transport::decode_measured::<_, 97>,
    encode = crate::transport::encode::<_, 97>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_binary_projection(
    (projected, counter): crate::transport::Measured<bool>,
) -> (Vec<u64>, u32, u64) {
    let host = runtime();
    let db = open(&host);
    let start = counter();
    let query = db.query::<BinaryObject>().eq(BinaryObject::ID, 1_u64);
    let (rows, bytes) = if projected {
        let page = query.fetch_as::<IdOnly>().unwrap();

        (
            page.rows.into_iter().map(|row| row.id).collect(),
            page.output_bytes,
        )
    } else {
        let page = query.fetch().unwrap();

        (
            page.rows.into_iter().map(|row| row.id).collect(),
            page.output_bytes,
        )
    };

    (rows, bytes as u32, counter() - start)
}

///
/// Summary
/// Groups `id` and `amount` into one record.
///

#[record(fields(
    field(name = "id", value(item(prim = "Nat64"))),
    field(name = "amount", value(item(prim = "Nat64")))
))]
pub(super) struct Summary {}

///
/// IdOnly
/// Carries the `Nat64` value of `id`.
///

#[record(fields(field(name = "id", value(item(prim = "Nat64")))))]
pub(super) struct IdOnly {}

type Request = (Option<Vec<u8>>, bool, Option<u64>, u64, u32, u32, u32);
type Page = (Vec<(u64, u64)>, Option<Vec<u8>>, u32, u32, u32, u64);

#[derive(Clone, Copy)]
enum Shape {
    Entity,
    Projection,
    Ordered,
    Any,
    Membership,
}

type TextPage = (Vec<Option<String>>, Option<Vec<u8>>, u32, u32);
type TextRequest = (String, Option<Vec<u8>>, u32, u32);

type SparseRequest = (bool, bool, Option<Vec<u8>>, u32);
type IdPage = (Vec<u64>, Option<Vec<u8>>, u32);
type TextSeekRequest = (String, bool, Option<Vec<u8>>, u32, u32);

type PrimaryRangeRequest = (Option<Vec<u8>>, bool, u64, u64, u32, u32);

type IndexRangeRequest = (Option<Vec<u8>>, bool, Option<u64>, u64, u64, u32, u32);

fn range_read(
    (after, descending, lower, upper, work, bytes): PrimaryRangeRequest,
    index_prefix: Option<&[IndexValue<'_>]>,
    field: &str,
) -> Result<IdPage, u8> {
    let host = runtime();
    let db = open(&host);
    let cursor = decode_cursor(after.as_deref()).map_err(|error| query_error(&error))?;
    let mut query = db
        .query::<Entry>()
        .gte(field, lower)
        .lte(field, upper)
        .work_limit(work as usize)
        .test_output_limit(bytes as usize);
    if let Some(prefix) = index_prefix {
        query = query.using_index("category,id", prefix);
    }
    fetch_ids(query, descending, cursor.as_ref())
}

fn text_match(
    (needle, after, work, bytes): TextRequest,
    comparison: Comparison,
) -> Result<TextPage, u8> {
    let host = runtime();
    let db = open(&host);
    let cursor = decode_cursor(after.as_deref()).map_err(|error| query_error(&error))?;
    let query = db
        .query::<Document>()
        .filter(Document::NOTE, comparison, &needle)
        .limit(1)
        .work_limit(work as usize)
        .test_output_limit(bytes as usize)
        .after(cursor.as_ref());
    let page = query
        .fetch_as::<NoteOnly>()
        .map_err(|error| query_error(&error))?;

    Ok((
        page.rows.into_iter().map(|r| r.note).collect(),
        page.next.map(|c| c.to_bytes()),
        page.examined as u32,
        page.output_bytes as u32,
    ))
}

fn read(
    (cursor, descending, category, minimum, limit, work, bytes): Request,
    shape: Shape,
    counter: fn() -> u64,
) -> Result<Page, u8> {
    let start = counter();
    let host = runtime();
    let db = open(&host);
    let cursor = decode_cursor(cursor.as_deref()).map_err(|_| 1_u8)?;
    let query = db.query::<Entry>();
    let mut query = match shape {
        Shape::Any => query.filter_any(&[
            (Entry::AMOUNT, Comparison::Ge, minimum.into()),
            (Entry::AMOUNT, Comparison::Le, 20_u64.into()),
        ]),
        Shape::Membership => query.filter_in(Entry::AMOUNT, &[minimum, 20, minimum]),
        _ => query.gte(Entry::AMOUNT, minimum),
    }
    .limit(limit as usize)
    .work_limit(work as usize)
    .test_output_limit(bytes as usize);
    if let Some(category) = category {
        query = if matches!(shape, Shape::Ordered | Shape::Projection) {
            query.eq(Entry::CATEGORY, category)
        } else {
            query.using_index("category,id", &[category.into()])
        };
    }
    if matches!(shape, Shape::Ordered) {
        query = query.order_asc(Entry::CATEGORY);
    }
    if descending {
        query = query.descending();
    }
    let query = query.after(cursor.as_ref());
    let page = if !matches!(shape, Shape::Entity) {
        query.fetch_as::<Summary>()
    } else {
        query.fetch().map(|page| ic_db::QueryPage {
            rows: page
                .rows
                .into_iter()
                .map(|row| Summary {
                    id: row.id,
                    amount: row.amount,
                })
                .collect(),
            next: page.next,
            examined: page.examined,
            fetched: page.fetched,
            output_bytes: page.output_bytes,
        })
    }
    .map_err(|error| query_error(&error))?;
    let rows = page
        .rows
        .into_iter()
        .map(|row| (row.id, row.amount))
        .collect();

    Ok((
        rows,
        page.next.map(|c| c.to_bytes()),
        page.examined as u32,
        page.fetched as u32,
        page.output_bytes as u32,
        counter() - start,
    ))
}

fn fetch_ids<'a, E: Entity>(
    mut query: crate::handles::Query<'a, E>,
    descending: bool,
    cursor: Option<&'a QueryCursor>,
) -> Result<IdPage, u8> {
    if descending {
        query = query.descending();
    }

    query
        .after(cursor)
        .fetch_as::<IdOnly>()
        .map(id_page)
        .map_err(|error| query_error(&error))
}

fn id_page(page: ic_db::QueryPage<IdOnly>) -> IdPage {
    (
        page.rows.into_iter().map(|row| row.id).collect(),
        page.next.map(|cursor| cursor.to_bytes()),
        page.examined as u32,
    )
}

pub(super) fn decode_cursor(bytes: Option<&[u8]>) -> Result<Option<QueryCursor>, ic_db::Error> {
    bytes.map(QueryCursor::from_bytes).transpose()
}

fn query_error(error: &ic_db::Error) -> u8 {
    match error {
        ic_db::Error::InvalidCursor => 1,
        ic_db::Error::RowTooLarge => 2,
        _ => 3,
    }
}
