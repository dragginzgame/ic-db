//! Private fluent-query witness; production query endpoints belong to Canic.
use crate::{
    console::NoteOnly,
    open, runtime,
    schema::{Document, Entry},
};
use ic_db::{Comparison, QueryCursor};

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

#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<95>",
    decode_with = "crate::transport::decode::<_, 95>",
    encode_with = "crate::transport::encode::<_, 95>"
)]
fn fluent_query(request: Request) -> Result<Page, u8> {
    read(request, Shape::Entity)
}
#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<96>",
    decode_with = "crate::transport::decode::<_, 96>",
    encode_with = "crate::transport::encode::<_, 96>"
)]
fn fluent_projection(request: Request) -> Result<Page, u8> {
    read(request, Shape::Projection)
}
#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<98>",
    decode_with = "crate::transport::decode::<_, 98>",
    encode_with = "crate::transport::encode::<_, 98>"
)]
fn fluent_ordered(request: Request) -> Result<Page, u8> {
    read(request, Shape::Ordered)
}
#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<123>",
    decode_with = "crate::transport::decode::<_, 123>",
    encode_with = "crate::transport::encode::<_, 123>"
)]
fn fluent_any(request: Request) -> Result<Page, u8> {
    read(request, Shape::Any)
}
#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<124>",
    decode_with = "crate::transport::decode::<_, 124>",
    encode_with = "crate::transport::encode::<_, 124>"
)]
fn fluent_membership(request: Request) -> Result<Page, u8> {
    read(request, Shape::Membership)
}

type TextPage = (Vec<Option<String>>, Option<Vec<u8>>, u32, u32);
#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<125>",
    decode_with = "crate::transport::decode::<_, 125>",
    encode_with = "crate::transport::encode::<_, 125>"
)]
fn fluent_text_prefix(
    (prefix, after, work, bytes): (String, Option<Vec<u8>>, u32, u32),
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
        .filter("note", Comparison::StartsWith, prefix.as_str())
        .limit(1)
        .work_limit(work as usize)
        .byte_limit(bytes as usize);
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
    let start = ic_platform_runtime::cdk::api::instruction_counter();
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
            ("amount", Comparison::Ge, minimum.into()),
            ("amount", Comparison::Le, 20_u64.into()),
        ]),
        Shape::Membership => {
            query.filter_in("amount", &[minimum.into(), 20_u64.into(), minimum.into()])
        }
        _ => query.filter("amount", Comparison::Ge, minimum),
    }
    .limit(limit as usize)
    .work_limit(work as usize)
    .byte_limit(bytes as usize);
    if let Some(category) = category {
        query = if matches!(shape, Shape::Ordered) {
            query.filter("category", Comparison::Eq, category)
        } else {
            query.using_index("category,id", &[category.into()])
        };
    }
    if matches!(shape, Shape::Ordered) {
        query = query.order_by(&["category"]);
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
        ic_platform_runtime::cdk::api::instruction_counter() - start,
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
#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<97>",
    decode_with = "crate::transport::decode::<_, 97>",
    encode_with = "crate::transport::encode::<_, 97>"
)]
fn fluent_binary_projection(projected: bool) -> (Vec<u64>, u32, u64) {
    let host = runtime();
    let db = open(&host);
    let start = ic_platform_runtime::cdk::api::instruction_counter();
    let query = db
        .query::<crate::schema::BinaryObject>()
        .filter("id", Comparison::Eq, 1_u64);
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
        ic_platform_runtime::cdk::api::instruction_counter() - start,
    )
}
