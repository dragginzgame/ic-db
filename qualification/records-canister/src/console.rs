//! Private console adapter: fixed typed entities, fluent execution, display-only mapping.
use crate::{
    Backing, open,
    queries::{IdOnly, Summary},
    runtime,
    schema::{Document, Entry},
};
use ic_db::{Comparison, Entity, Error, Query, QueryCursor, QueryPage};

type Request = (
    String,
    Option<Vec<u8>>,
    u32,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Vec<String>,
    bool,
    u32,
    u32,
    String,
);
type Page = (
    Vec<String>,
    Vec<Vec<String>>,
    Option<Vec<u8>>,
    u32,
    u32,
    u32,
);

#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<3>",
    decode_with = "crate::transport::decode::<_, 3>",
    encode_with = "crate::transport::encode::<_, 3>"
)]
fn query(
    (entity, cursor, limit, minimum, category, entry_id, order, descending, work, bytes, select): Request,
) -> Result<Page, String> {
    let cursor = cursor
        .as_deref()
        .map(QueryCursor::from_bytes)
        .transpose()
        .map_err(message)?;
    let order: Vec<_> = order.iter().map(String::as_str).collect();
    let host = runtime();
    let db = open(&host);
    match entity.as_str() {
        "Entry" => {
            if entry_id.is_some() {
                return Err("--entry-id applies to Document".into());
            }
            let mut query = db.query::<Entry>();
            if let Some(value) = minimum {
                query = query.filter("amount", Comparison::Ge, value);
            }
            if let Some(value) = category {
                query = query.filter("category", Comparison::Eq, value);
            }
            execute(
                configure(
                    query,
                    cursor.as_ref(),
                    &order,
                    descending,
                    limit,
                    work,
                    bytes,
                ),
                &select,
                &["id", "category", "amount"],
                |r| {
                    vec![
                        r.id.to_string(),
                        r.category.to_string(),
                        r.amount.to_string(),
                    ]
                },
            )
        }
        "Document" => {
            if minimum.is_some() || category.is_some() {
                return Err("--min and --category apply to Entry".into());
            }
            let mut query = db.query::<Document>();
            if let Some(value) = entry_id {
                query = query.filter("entry_id", Comparison::Eq, value);
            }
            execute(
                configure(
                    query,
                    cursor.as_ref(),
                    &order,
                    descending,
                    limit,
                    work,
                    bytes,
                ),
                &select,
                &[
                    "id",
                    "entry_id",
                    "title",
                    "note",
                    "published",
                    "score",
                    "previous_score",
                    "priority",
                ],
                |r| {
                    vec![
                        r.id.to_string(),
                        r.entry_id.to_string(),
                        r.title,
                        r.note.unwrap_or_else(|| "—".into()),
                        r.published.to_string(),
                        r.score.to_string(),
                        optional(r.previous_score),
                        optional(r.priority),
                    ]
                },
            )
        }
        _ => Err("query supports Entry and Document in this fixture".into()),
    }
}

fn configure<'a, E: Entity>(
    mut query: Query<'a, E, Backing>,
    cursor: Option<&'a QueryCursor>,
    order: &[&'a str],
    descending: bool,
    limit: u32,
    work: u32,
    bytes: u32,
) -> Query<'a, E, Backing> {
    query = query
        .limit(limit as usize)
        .work_limit(work as usize)
        .byte_limit(bytes as usize);
    if !order.is_empty() {
        query = query.order_by(order);
    }
    if descending {
        query = query.descending();
    }
    if let Some(cursor) = cursor {
        query = query.after(cursor);
    }
    query
}

fn execute<E: Entity>(
    query: Query<'_, E, Backing>,
    select: &str,
    columns: &[&str],
    render: fn(E) -> Vec<String>,
) -> Result<Page, String> {
    match select {
        "full" => query.fetch().map(|p| display(p, columns, render)),
        "id" => query
            .fetch_as::<IdOnly>()
            .map(|p| display(p, &["id"], |r| vec![r.id.to_string()])),
        "id,amount" => query.fetch_as::<Summary>().map(|p| {
            display(p, &["id", "amount"], |r| {
                vec![r.id.to_string(), r.amount.to_string()]
            })
        }),
        _ => return Err("unsupported projection; use full, id or id,amount".into()),
    }
    .map_err(message)
}
fn display<T>(page: QueryPage<T>, columns: &[&str], render: fn(T) -> Vec<String>) -> Page {
    (
        columns.iter().map(|c| (*c).to_owned()).collect(),
        page.rows.into_iter().map(render).collect(),
        page.next.map(|c| c.to_bytes()),
        page.examined as u32,
        page.fetched as u32,
        page.output_bytes as u32,
    )
}
fn optional(value: Option<impl ToString>) -> String {
    value.map_or_else(|| "—".into(), |v| v.to_string())
}
fn message(error: Error) -> String {
    match error {
        Error::InvalidCursor => {
            "invalid cursor or different entity, filters, order or projection".into()
        }
        Error::InvalidQuery => "unsupported order or projection for this entity".into(),
        Error::RowTooLarge => "one matching result exceeds --bytes".into(),
        Error::Limit => "query exceeds a database limit".into(),
        other => format!("database refused query: {other:?}"),
    }
}
