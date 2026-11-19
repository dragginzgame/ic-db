//! Private fluent-query witness; production query endpoints belong to Canic.
use crate::{
    console::NoteOnly,
    open, runtime,
    schema::{BinaryObject, DecimalRow, Document, Entry, ListRow},
};
use ic_db::{Comparison, IndexValue, QueryCursor};

#[ic_db::record(fields(
    field(name = "id", value(item(prim = "Nat64"))),
    field(name = "amount", value(item(prim = "Nat64")))
))]
pub(super) struct Summary {}
#[ic_db::record(fields(field(name = "id", value(item(prim = "Nat64")))))]
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

/// Return a bounded page of matching fixture entities.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_query",
    guard = crate::transport::admit::<95>,
    decode = crate::transport::decode::<_, 95>,
    encode = crate::transport::encode::<_, 95>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_query(request: Request) -> Result<Page, u8> {
    read(request, Shape::Entity)
}

/// Project matching entries into the fixture result record.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_projection",
    guard = crate::transport::admit::<96>,
    decode = crate::transport::decode::<_, 96>,
    encode = crate::transport::encode::<_, 96>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_projection(request: Request) -> Result<Page, u8> {
    read(request, Shape::Projection)
}

/// Page matching entries in the requested order.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_ordered",
    guard = crate::transport::admit::<98>,
    decode = crate::transport::decode::<_, 98>,
    encode = crate::transport::encode::<_, 98>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_ordered(request: Request) -> Result<Page, u8> {
    read(request, Shape::Ordered)
}

/// Page entries matching the fixture’s alternative predicates.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_any",
    guard = crate::transport::admit::<123>,
    decode = crate::transport::decode::<_, 123>,
    encode = crate::transport::encode::<_, 123>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_any(request: Request) -> Result<Page, u8> {
    read(request, Shape::Any)
}

/// Page entries matching the fixture’s membership filter.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_membership",
    guard = crate::transport::admit::<124>,
    decode = crate::transport::decode::<_, 124>,
    encode = crate::transport::encode::<_, 124>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_membership(request: Request) -> Result<Page, u8> {
    read(request, Shape::Membership)
}

type TextPage = (Vec<Option<String>>, Option<Vec<u8>>, u32, u32);
type TextRequest = (String, Option<Vec<u8>>, u32, u32);

/// Page matching text prefixes.
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

/// Page matching text suffixes.
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

/// Page rows containing the supplied text.
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

type SparseRequest = (bool, bool, Option<Vec<u8>>, u32);
type IdPage = (Vec<u64>, Option<Vec<u8>>, u32);
type TextSeekRequest = (String, bool, Option<Vec<u8>>, u32, u32);

/// Seek document titles by prefix and continue in the requested direction.
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
    let cursor = after
        .as_deref()
        .map(QueryCursor::from_bytes)
        .transpose()
        .map_err(|error| query_error(&error))?;
    let mut query = db
        .query::<Document>()
        .using_index("published,title", &[false.into()])
        .filter(Document::TITLE, Comparison::StartsWith, needle.as_str())
        .work_limit(work as usize)
        .test_output_limit(bytes as usize);
    if descending {
        query = query.descending();
    }
    if let Some(cursor) = &cursor {
        query = query.after(cursor);
    }
    let page = query
        .fetch_as::<IdOnly>()
        .map_err(|error| query_error(&error))?;
    Ok((
        page.rows.into_iter().map(|row| row.id).collect(),
        page.next.map(|cursor| cursor.to_bytes()),
        page.examined as u32,
    ))
}

type PrimaryRangeRequest = (Option<Vec<u8>>, bool, u64, u64, u32, u32);

/// Page IDs in the requested primary-key range.
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

type IndexRangeRequest = (Option<Vec<u8>>, bool, Option<u64>, u64, u64, u32, u32);

/// Page IDs in the selected index range, optionally within one category.
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

fn range_read(
    (after, descending, lower, upper, work, bytes): PrimaryRangeRequest,
    index_prefix: Option<&[IndexValue<'_>]>,
    field: &str,
) -> Result<IdPage, u8> {
    let host = runtime();
    let db = open(&host);
    let cursor = after
        .as_deref()
        .map(QueryCursor::from_bytes)
        .transpose()
        .map_err(|error| query_error(&error))?;
    let mut query = db
        .query::<Entry>()
        .gte(field, lower)
        .lte(field, upper)
        .work_limit(work as usize)
        .test_output_limit(bytes as usize);
    if let Some(prefix) = index_prefix {
        query = query.using_index("category,id", prefix);
    }
    if descending {
        query = query.descending();
    }
    if let Some(cursor) = &cursor {
        query = query.after(cursor);
    }
    let page = query
        .fetch_as::<IdOnly>()
        .map_err(|error| query_error(&error))?;
    Ok((
        page.rows.into_iter().map(|row| row.id).collect(),
        page.next.map(|cursor| cursor.to_bytes()),
        page.examined as u32,
    ))
}

/// Page the sparse-index fixture with the requested absence policy.
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
    let cursor = after
        .as_deref()
        .map(QueryCursor::from_bytes)
        .transpose()
        .map_err(|error| query_error(&error))?;
    let mut query = db
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
    if descending {
        query = query.descending();
    }
    if let Some(cursor) = &cursor {
        query = query.after(cursor);
    }
    let page = query
        .fetch_as::<IdOnly>()
        .map_err(|error| query_error(&error))?;
    Ok((
        page.rows.into_iter().map(|row| row.id).collect(),
        page.next.map(|cursor| cursor.to_bytes()),
        page.examined as u32,
    ))
}

/// Page rows under the selected list-presence predicate.
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
    let cursor = after
        .as_deref()
        .map(QueryCursor::from_bytes)
        .transpose()
        .map_err(|error| query_error(&error))?;
    let query = db.query::<ListRow>().work_limit(1);
    let mut query = match mode {
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
    if let Some(cursor) = &cursor {
        query = query.after(cursor);
    }
    let page = query
        .fetch_as::<IdOnly>()
        .map_err(|error| query_error(&error))?;
    Ok((
        page.rows.into_iter().map(|row| row.id).collect(),
        page.next.map(|cursor| cursor.to_bytes()),
        page.examined as u32,
    ))
}

fn text_match(
    (needle, after, work, bytes): TextRequest,
    comparison: Comparison,
) -> Result<TextPage, u8> {
    let host = runtime();
    let db = open(&host);
    let cursor = after
        .as_deref()
        .map(QueryCursor::from_bytes)
        .transpose()
        .map_err(|error| query_error(&error))?;
    let mut query = db
        .query::<Document>()
        .filter(Document::NOTE, comparison, needle.as_str())
        .limit(1)
        .work_limit(work as usize)
        .test_output_limit(bytes as usize);
    if let Some(cursor) = &cursor {
        query = query.after(cursor);
    }
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
) -> Result<Page, u8> {
    let start = crate::system::instruction_counter();
    let host = runtime();
    let db = open(&host);
    let cursor = cursor
        .as_deref()
        .map(QueryCursor::from_bytes)
        .transpose()
        .map_err(|_| 1_u8)?;
    let query = db.query::<Entry>();
    let mut query = match shape {
        Shape::Any => query.filter_any(&[
            (Entry::AMOUNT, Comparison::Ge, minimum.into()),
            (Entry::AMOUNT, Comparison::Le, 20_u64.into()),
        ]),
        Shape::Membership => query.filter_in(
            Entry::AMOUNT,
            &[minimum.into(), 20_u64.into(), minimum.into()],
        ),
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
    if let Some(cursor) = &cursor {
        query = query.after(cursor);
    }
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
        crate::system::instruction_counter() - start,
    ))
}

fn query_error(error: &ic_db::Error) -> u8 {
    match error {
        ic_db::Error::InvalidCursor => 1,
        ic_db::Error::RowTooLarge => 2,
        _ => 3,
    }
}

// Same source, predicate, result and warm open; only materialization differs.

/// Measure entity and ID-only reads of the binary fixture.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "fluent_binary_projection",
    guard = crate::transport::admit::<97>,
    decode = crate::transport::decode::<_, 97>,
    encode = crate::transport::encode::<_, 97>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn fluent_binary_projection(projected: bool) -> (Vec<u64>, u32, u64) {
    let host = runtime();
    let db = open(&host);
    let start = crate::system::instruction_counter();
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
    (
        rows,
        bytes as u32,
        crate::system::instruction_counter() - start,
    )
}

#[cfg(feature = "query-measurements")]
type ListingSample = (Vec<u64>, Option<Vec<u8>>, u32, u32, u64);

// Both measurements use the same admitted query. Only the optional hint varies.

/// Compare bounded listings with and without the explicit index hint.
#[cfg(feature = "query-measurements")]
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime, name = "measured_listing",
    guard = crate::transport::admit::<189>, decode = crate::transport::decode::<_, 189>,
    encode = crate::transport::encode::<_, 189>, max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn measured_listing(
    (hinted, after, work, bytes): (bool, Option<Vec<u8>>, u32, u32),
) -> ListingSample {
    let start = crate::system::instruction_counter();
    let host = runtime();
    let db = open(&host);
    let cursor = after
        .as_deref()
        .map(QueryCursor::from_bytes)
        .transpose()
        .unwrap();
    let mut q = db
        .query::<Document>()
        .eq(Document::ENTRY_ID, 1_u64)
        .eq(Document::PUBLISHED, true)
        .order_by(&[Document::SCORE, Document::ID])
        .descending()
        .limit(20)
        .work_limit(work as usize)
        .test_output_limit(bytes as usize);
    if hinted {
        q = q.using_index("entry_id,score", &[1_u64.into()]);
    }
    if let Some(cursor) = &cursor {
        q = q.after(cursor);
    }
    let page = q.fetch_as::<IdOnly>().unwrap();
    (
        page.rows.into_iter().map(|r| r.id).collect(),
        page.next.map(|c| c.to_bytes()),
        page.examined as u32,
        page.fetched as u32,
        crate::system::instruction_counter() - start,
    )
}
