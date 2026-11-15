//! Private console adapter: fixed typed entities, fluent execution, display-only mapping.
use crate::{
    Backing, open,
    queries::{IdOnly, Summary},
    runtime,
    schema::{Document, Entry},
};
use ic_db::{
    Aggregate, Comparison, Entity, Error, GroupCount, GroupOrder, Query, QueryCursor, QueryPage,
    Value,
};

#[ic_db::record(fields(field(name = "category", value(item(prim = "Nat64")))))]
struct CategoryOnly {}
#[ic_db::record(fields(field(name = "amount", value(item(prim = "Nat64")))))]
struct AmountOnly {}
#[ic_db::record(fields(field(name = "note", value(opt, item(prim = "Text", max_bytes = 64)))))]
struct NoteOnly {}
#[ic_db::record(fields(field(name = "priority", value(opt, item(prim = "Nat64")))))]
struct PriorityOnly {}
#[ic_db::record(fields(field(name = "entry_id", value(item(prim = "Nat64")))))]
struct EntryIdOnly {}

#[ic_db::record(fields(field(name = "amount", value(item(prim = "Nat128")))))]
struct TotalAmount {}
#[ic_db::record(fields(field(name = "priority", value(opt, item(prim = "Nat128")))))]
struct TotalPriority {}
#[ic_db::record(fields(field(name = "score", value(item(prim = "Int128")))))]
struct TotalScore {}

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
    (bool, Option<u64>, Option<String>),
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
    (
        entity,
        cursor,
        limit,
        minimum,
        category,
        entry_id,
        order,
        descending,
        work,
        bytes,
        select,
        (distinct, group_min, group_order),
    ): Request,
) -> Result<Page, String> {
    let group_order = match group_order.as_deref() {
        None => None,
        Some("key") => Some(GroupOrder::Key),
        Some("asc") => Some(GroupOrder::AggregateAscending),
        Some("desc") => Some(GroupOrder::AggregateDescending),
        _ => return Err("unsupported group order".into()),
    };
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
                (distinct, group_min, group_order),
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
                (distinct, group_min, group_order),
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
    mut query: Query<'_, E, Backing>,
    select: &str,
    (distinct, group_min, group_order): (bool, Option<u64>, Option<GroupOrder>),
    columns: &[&str],
    render: fn(E) -> Vec<String>,
) -> Result<Page, String> {
    if distinct {
        query = query.distinct();
    }
    if let Some(value) = group_min {
        query = query.filter_groups(Comparison::Ge, value);
    }
    if let Some(order) = group_order {
        query = query.order_groups(order);
    }
    match select {
        "sum:amount" => query
            .sum::<TotalAmount>()
            .map(|r| display_value(r, "amount", |r| r.amount.to_string())),
        "sum:priority" => query
            .sum::<TotalPriority>()
            .map(|r| display_value(r, "priority", |r| optional(r.priority))),
        "sum:score" => query
            .sum::<TotalScore>()
            .map(|r| display_value(r, "score", |r| r.score.to_string())),
        "category,min:amount" | "category,max:amount" => grouped_extremum(
            query,
            select == "category,max:amount",
            &["category", "amount"],
            |k: CategoryOnly| k.category.to_string(),
            |r: AmountOnly| r.amount.to_string(),
        ),
        "entry_id,min:note" | "entry_id,max:note" => grouped_extremum(
            query,
            select == "entry_id,max:note",
            &["entry_id", "note"],
            |k: EntryIdOnly| k.entry_id.to_string(),
            |r: NoteOnly| optional(r.note),
        ),
        "entry_id,min:priority" | "entry_id,max:priority" => grouped_extremum(
            query,
            select == "entry_id,max:priority",
            &["entry_id", "priority"],
            |k: EntryIdOnly| k.entry_id.to_string(),
            |r: PriorityOnly| optional(r.priority),
        ),
        "min:amount" | "max:amount" => {
            extremum(query, select == "max:amount", "amount", |r: AmountOnly| {
                r.amount.to_string()
            })
        }
        "min:note" | "max:note" => extremum(query, select == "max:note", "note", |r: NoteOnly| {
            optional(r.note)
        }),
        "min:priority" | "max:priority" => extremum(
            query,
            select == "max:priority",
            "priority",
            |r: PriorityOnly| optional(r.priority),
        ),
        "count" => query.count().map(|r| display_count(&r)),
        "distinct-count:category" => query
            .count_distinct::<CategoryOnly>()
            .map(|r| display_count(&r)),
        "category,count" => query.group_count::<CategoryOnly>().map(display_groups),
        s if s.starts_with("count:") => query.count_field(&s[6..]).map(|r| display_count(&r)),
        s if s.starts_with("category,count:") => query
            .group_count_field::<CategoryOnly>(&s[15..])
            .map(display_groups),
        "full" => query.fetch().map(|p| display(p, columns, render)),
        "id" => query
            .fetch_as::<IdOnly>()
            .map(|p| display(p, &["id"], |r| vec![r.id.to_string()])),
        "id,amount" => query.fetch_as::<Summary>().map(|p| {
            display(p, &["id", "amount"], |r| {
                vec![r.id.to_string(), r.amount.to_string()]
            })
        }),
        "category" => query
            .fetch_as::<CategoryOnly>()
            .map(|p| display(p, &["category"], |r| vec![r.category.to_string()])),
        _ => return Err("unsupported projection or aggregate".into()),
    }
    .map_err(message)
}
fn grouped_extremum<E: Entity, K: Value, R: Value>(
    query: Query<'_, E, Backing>,
    maximum: bool,
    columns: &[&str; 2],
    key: fn(K) -> String,
    render: fn(R) -> String,
) -> Result<Page, Error> {
    let r = if maximum {
        query.group_max::<K, R>()?
    } else {
        query.group_min::<K, R>()?
    };
    Ok((
        columns.iter().map(|c| (*c).into()).collect(),
        r.value
            .into_iter()
            .map(|g| vec![key(g.key), g.value.map_or_else(|| "—".into(), render)])
            .collect(),
        None,
        r.examined as u32,
        r.fetched as u32,
        r.output_bytes as u32,
    ))
}
fn extremum<E: Entity, R: Value>(
    query: Query<'_, E, Backing>,
    maximum: bool,
    column: &str,
    render: fn(R) -> String,
) -> Result<Page, Error> {
    let r = if maximum {
        query.max::<R>()?
    } else {
        query.min::<R>()?
    };
    Ok(display_value(r, column, render))
}
fn display_value<R>(r: Aggregate<Option<R>>, column: &str, render: fn(R) -> String) -> Page {
    (
        vec![column.into()],
        r.value
            .into_iter()
            .map(|value| vec![render(value)])
            .collect(),
        None,
        r.examined as u32,
        r.fetched as u32,
        r.output_bytes as u32,
    )
}
fn display_count(r: &Aggregate<u64>) -> Page {
    (
        vec!["count".into()],
        vec![vec![r.value.to_string()]],
        None,
        r.examined as u32,
        r.fetched as u32,
        r.output_bytes as u32,
    )
}
fn display_groups(r: Aggregate<Vec<GroupCount<CategoryOnly>>>) -> Page {
    (
        vec!["category".into(), "count".into()],
        r.value
            .into_iter()
            .map(|g| vec![g.key.category.to_string(), g.count.to_string()])
            .collect(),
        None,
        r.examined as u32,
        r.fetched as u32,
        r.output_bytes as u32,
    )
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
            "invalid cursor or different entity, filters, order, projection or DISTINCT setting"
                .into()
        }
        Error::InvalidQuery => {
            "unsupported order, projection, DISTINCT or aggregate shape for this entity".into()
        }
        Error::RowTooLarge => "one matching result exceeds --bytes".into(),
        Error::ArithmeticOverflow => "sum exceeds its selected integer range".into(),
        Error::Limit => "query exceeds a database limit".into(),
        other => format!("database refused query: {other:?}"),
    }
}
