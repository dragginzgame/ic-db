//! Private console adapter: fixed typed entities, fluent execution, display-only mapping.
#[cfg(feature = "retained-console")]
mod retained;
use crate::{
    Backing, open,
    queries::{IdOnly, Summary},
    runtime,
    schema::{DecimalRow, Document, Entry},
};
use ic_db::{
    Aggregate, Comparison, Entity, Error, GroupCount, GroupOrder, GroupValue, Query, QueryCursor,
    QueryPage, Value,
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
#[ic_db::record(fields(field(name = "amount", value(item(prim = "Decimal", scale = 2)))))]
struct AverageAmount {}
#[ic_db::record(fields(field(name = "priority", value(opt, item(prim = "Nat128")))))]
struct TotalPriority {}
#[ic_db::record(fields(field(name = "score", value(item(prim = "Int128")))))]
struct TotalScore {}
#[ic_db::record(fields(field(name = "value", value(item(prim = "Decimal", scale = 2)))))]
struct TotalDecimal {}
#[ic_db::record(fields(field(name = "maybe", value(opt, item(prim = "Decimal", scale = 2)))))]
struct OptionalDecimal {}
#[ic_db::record(fields(field(name = "zone", value(item(prim = "Bool")))))]
struct ZoneOnly {}

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
    (bool, (Option<String>, Option<String>), Option<String>),
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
        (distinct, (group_min, group_max), group_order),
    ): Request,
) -> Result<Page, String> {
    let group_order = parse_group_order(group_order.as_deref())?;
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
            let query = entry_query(&db, minimum, category, entry_id)?;
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
                (
                    distinct,
                    (group_min.as_deref(), group_max.as_deref()),
                    group_order,
                ),
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
            let query = document_query(&db, minimum, category, entry_id)?;
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
                (
                    distinct,
                    (group_min.as_deref(), group_max.as_deref()),
                    group_order,
                ),
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
        "DecimalRow" => execute(
            configure(
                decimal_query(&db, minimum, category, entry_id)?,
                cursor.as_ref(),
                &order,
                descending,
                limit,
                work,
                bytes,
            ),
            &select,
            (
                distinct,
                (group_min.as_deref(), group_max.as_deref()),
                group_order,
            ),
            &["id", "zone", "value", "maybe"],
            |r| {
                vec![
                    r.id.to_string(),
                    r.zone.to_string(),
                    r.value.to_string(),
                    optional(r.maybe),
                ]
            },
        ),
        _ => Err("query supports Entry, Document and DecimalRow in this fixture".into()),
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

fn parse_group_order(order: Option<&str>) -> Result<Option<GroupOrder>, String> {
    match order {
        None => Ok(None),
        Some("key") => Ok(Some(GroupOrder::Key)),
        Some("asc") => Ok(Some(GroupOrder::AggregateAscending)),
        Some("desc") => Ok(Some(GroupOrder::AggregateDescending)),
        _ => Err("unsupported group order".into()),
    }
}

fn entry_query(
    db: &ic_db::Database<Backing>,
    minimum: Option<u64>,
    category: Option<u64>,
    entry_id: Option<u64>,
) -> Result<Query<'_, Entry, Backing>, String> {
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
    Ok(query)
}

fn document_query(
    db: &ic_db::Database<Backing>,
    minimum: Option<u64>,
    category: Option<u64>,
    entry_id: Option<u64>,
) -> Result<Query<'_, Document, Backing>, String> {
    if minimum.is_some() || category.is_some() {
        return Err("--min and --category apply to Entry".into());
    }
    let mut query = db.query::<Document>();
    if let Some(value) = entry_id {
        query = query.filter("entry_id", Comparison::Eq, value);
    }
    Ok(query)
}

fn decimal_query(
    db: &ic_db::Database<Backing>,
    minimum: Option<u64>,
    category: Option<u64>,
    entry_id: Option<u64>,
) -> Result<Query<'_, DecimalRow, Backing>, String> {
    if minimum.is_some() || category.is_some() || entry_id.is_some() {
        return Err("--min, --category and --entry-id do not apply to DecimalRow".into());
    }
    Ok(db.query::<DecimalRow>())
}

fn select_groups<'a, E: Entity>(
    mut query: Query<'a, E, Backing>,
    select: &str,
    (group_min, group_max): (Option<&str>, Option<&str>),
    group_order: Option<GroupOrder>,
) -> Result<Query<'a, E, Backing>, String> {
    for (comparison, value) in [(Comparison::Ge, group_min), (Comparison::Le, group_max)] {
        let Some(value) = value else {
            continue;
        };
        if value.len() > 64 {
            return Err(message(Error::InvalidOperand));
        }
        query = match select {
            "category,sum:amount" | "entry_id,sum:priority" => query.filter_groups(
                comparison,
                value
                    .parse::<u128>()
                    .map_err(|_| message(Error::InvalidOperand))?,
            ),
            "entry_id,sum:score" => query.filter_groups(
                comparison,
                value
                    .parse::<i128>()
                    .map_err(|_| message(Error::InvalidOperand))?,
            ),
            "zone,sum:value"
            | "zone,sum:maybe"
            | "zone,avg:value"
            | "zone,avg:maybe"
            | "zone,min:value"
            | "zone,max:value"
            | "zone,min:maybe"
            | "zone,max:maybe"
            | "category,avg:amount" => query.filter_groups(
                comparison,
                value
                    .parse::<ic_db::Decimal>()
                    .map_err(|_| message(Error::InvalidOperand))?,
            ),
            _ => query.filter_groups(
                comparison,
                value
                    .parse::<u64>()
                    .map_err(|_| message(Error::InvalidOperand))?,
            ),
        };
    }
    if let Some(order) = group_order {
        query = query.order_groups(order);
    }
    Ok(query)
}

fn execute<E: Entity>(
    mut query: Query<'_, E, Backing>,
    select: &str,
    (distinct, bounds, group_order): (bool, (Option<&str>, Option<&str>), Option<GroupOrder>),
    columns: &[&str],
    render: fn(E) -> Vec<String>,
) -> Result<Page, String> {
    if distinct {
        query = query.distinct();
    }
    let query = select_groups(query, select, bounds, group_order)?;
    match select {
        "avg:amount" => query
            .avg::<AverageAmount>()
            .map(|r| display_value(r, "amount", |r| r.amount.to_string())),
        "category,avg:amount" => query.group_avg::<CategoryOnly, AverageAmount>().map(|r| {
            display_group_values(
                r,
                &["category", "amount"],
                |k| k.category.to_string(),
                |v| v.amount.to_string(),
            )
        }),
        "sum:value" | "avg:value" | "min:value" | "max:value" => {
            let result = match select {
                "avg:value" => query.avg::<TotalDecimal>(),
                "min:value" => query.min::<TotalDecimal>(),
                "max:value" => query.max::<TotalDecimal>(),
                _ => query.sum::<TotalDecimal>(),
            };
            result.map(|r| display_value(r, "value", |r| r.value.to_string()))
        }
        "sum:maybe" | "avg:maybe" | "min:maybe" | "max:maybe" => {
            let result = match select {
                "avg:maybe" => query.avg::<OptionalDecimal>(),
                "min:maybe" => query.min::<OptionalDecimal>(),
                "max:maybe" => query.max::<OptionalDecimal>(),
                _ => query.sum::<OptionalDecimal>(),
            };
            result.map(|r| display_value(r, "maybe", |r| optional(r.maybe)))
        }
        "zone,sum:value" | "zone,avg:value" | "zone,min:value" | "zone,max:value" => {
            let result = match select {
                "zone,avg:value" => query.group_avg::<ZoneOnly, TotalDecimal>(),
                "zone,min:value" => query.group_min::<ZoneOnly, TotalDecimal>(),
                "zone,max:value" => query.group_max::<ZoneOnly, TotalDecimal>(),
                _ => query.group_sum::<ZoneOnly, TotalDecimal>(),
            };
            result.map(|r| {
                display_group_values(
                    r,
                    &["zone", "value"],
                    |k| k.zone.to_string(),
                    |v| v.value.to_string(),
                )
            })
        }
        "zone,sum:maybe" | "zone,avg:maybe" | "zone,min:maybe" | "zone,max:maybe" => {
            let result = match select {
                "zone,avg:maybe" => query.group_avg::<ZoneOnly, OptionalDecimal>(),
                "zone,min:maybe" => query.group_min::<ZoneOnly, OptionalDecimal>(),
                "zone,max:maybe" => query.group_max::<ZoneOnly, OptionalDecimal>(),
                _ => query.group_sum::<ZoneOnly, OptionalDecimal>(),
            };
            result.map(|r| {
                display_group_values(
                    r,
                    &["zone", "maybe"],
                    |k| k.zone.to_string(),
                    |v| optional(v.maybe),
                )
            })
        }
        "category,sum:amount" => query.group_sum::<CategoryOnly, TotalAmount>().map(|r| {
            display_group_values(
                r,
                &["category", "amount"],
                |k| k.category.to_string(),
                |v| v.amount.to_string(),
            )
        }),
        "entry_id,sum:priority" => query.group_sum::<EntryIdOnly, TotalPriority>().map(|r| {
            display_group_values(
                r,
                &["entry_id", "priority"],
                |k| k.entry_id.to_string(),
                |v| optional(v.priority),
            )
        }),
        "entry_id,sum:score" => query.group_sum::<EntryIdOnly, TotalScore>().map(|r| {
            display_group_values(
                r,
                &["entry_id", "score"],
                |k| k.entry_id.to_string(),
                |v| v.score.to_string(),
            )
        }),
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
        "zone,count" => query
            .group_count::<ZoneOnly>()
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "distinct-count:value" => query
            .count_distinct::<TotalDecimal>()
            .map(|r| display_count(&r)),
        "zone,distinct-count:value" => query
            .group_count_distinct::<ZoneOnly, TotalDecimal>()
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "zone,count:value" => query
            .group_count_field::<ZoneOnly>("value")
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "distinct-count:maybe" => query
            .count_distinct::<OptionalDecimal>()
            .map(|r| display_count(&r)),
        "zone,distinct-count:maybe" => query
            .group_count_distinct::<ZoneOnly, OptionalDecimal>()
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "zone,count:maybe" => query
            .group_count_field::<ZoneOnly>("maybe")
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "count" => query.count().map(|r| display_count(&r)),
        "distinct-count:category" => query
            .count_distinct::<CategoryOnly>()
            .map(|r| display_count(&r)),
        "category,distinct-count:amount" => query
            .group_count_distinct::<CategoryOnly, AmountOnly>()
            .map(|r| display_groups(r, "category", |k| k.category.to_string())),
        "category,distinct-count:category" => query
            .group_count_distinct::<CategoryOnly, CategoryOnly>()
            .map(|r| display_groups(r, "category", |k| k.category.to_string())),
        "entry_id,distinct-count:note" => query
            .group_count_distinct::<EntryIdOnly, NoteOnly>()
            .map(|r| display_groups(r, "entry_id", |k| k.entry_id.to_string())),
        "category,count" => query
            .group_count::<CategoryOnly>()
            .map(|r| display_groups(r, "category", |k| k.category.to_string())),
        s if s.starts_with("count:") => query.count_field(&s[6..]).map(|r| display_count(&r)),
        s if s.starts_with("category,count:") => query
            .group_count_field::<CategoryOnly>(&s[15..])
            .map(|r| display_groups(r, "category", |k| k.category.to_string())),
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
    Ok(display_group_values(r, columns, key, render))
}
fn display_group_values<K, R>(
    r: Aggregate<Vec<GroupValue<K, R>>>,
    columns: &[&str; 2],
    key: fn(K) -> String,
    render: fn(R) -> String,
) -> Page {
    (
        columns.iter().map(|c| (*c).into()).collect(),
        r.value
            .into_iter()
            .map(|g| vec![key(g.key), g.value.map_or_else(|| "—".into(), render)])
            .collect(),
        None,
        r.examined as u32,
        r.fetched as u32,
        r.output_bytes as u32,
    )
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
fn display_groups<K>(r: Aggregate<Vec<GroupCount<K>>>, column: &str, key: fn(K) -> String) -> Page {
    (
        vec![column.into(), "count".into()],
        r.value
            .into_iter()
            .map(|g| vec![key(g.key), g.count.to_string()])
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
        Error::ArithmeticOverflow => "aggregate exceeds its declared result range".into(),
        Error::Limit => "query exceeds a database limit".into(),
        other => format!("database refused query: {other:?}"),
    }
}
