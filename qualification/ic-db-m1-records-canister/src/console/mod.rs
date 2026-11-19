//! Private console adapter: fixed typed entities, fluent execution, display-only mapping.

mod blob;
#[cfg(feature = "retained-console")]
pub(crate) mod migration;
#[cfg(feature = "retained-console")]
mod retained;
#[cfg(all(test, feature = "retained-console"))]
mod tests;

use crate::{
    handles::Query,
    open,
    queries::{IdOnly, Summary},
    runtime,
    schema::{DecimalRow, Document, Entry},
    transport::RowFilters,
};
use ic_db::{
    Aggregate, Comparison, Entity, Error, GroupCount, GroupOrder, GroupValue, QueryCursor,
    QueryPage, SortDirection, Value,
};
use std::fmt::Write;

// cardinality
// Read the maintained row count for the named entity.
// guard  : validates the request frame for method 148.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "cardinality",
    guard = crate::transport::admit::<148>,
    decode = crate::transport::decode::<_, 148>,
    encode = crate::transport::encode::<_, 148>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn cardinality(entity: String) -> Result<u64, String> {
    let host = runtime();
    let db = open(&host);

    db.cardinality_by_name(&entity)
        .map_err(|error| match error {
            Error::UnknownEntity => "Unknown entity. Use db entities to list entities.".into(),
            other => message(other),
        })
}

// Measure the exact console path, including open/admission/result formatting.
// An update permits complete-call cycle accounting; the driver verifies that
// both successful and refused reads preserve the backing bytes.
// measured_query
// Execute a console query and report its instruction count.
// guard  : validates the request frame for method 147.
// limits : 64 KiB request, 64 KiB reply.
#[cfg(feature = "query-measurements")]
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "measured_query",
    guard = crate::transport::admit::<147>,
    decode = crate::transport::decode_measured::<_, 147>,
    encode = crate::transport::encode::<_, 147>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn measured_query(
    (request, counter): crate::transport::Measured<Request>,
) -> (Result<Page, String>, u64) {
    let start = counter();
    let result = query(request);

    (result, counter() - start)
}

// query
// Read the requested bounded result from the local fixture.
// guard  : validates the request frame for method 3.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "query",
    guard = crate::transport::admit::<3>,
    decode = crate::transport::decode::<_, 3>,
    encode = crate::transport::encode::<_, 3>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
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
        (distinct, (group_min, group_max), group_order, filters),
    ): Request,
) -> Result<Page, String> {
    let group_order = parse_group_order(group_order.as_deref())?;
    let cursor = cursor
        .as_deref()
        .map(QueryCursor::from_bytes)
        .transpose()
        .map_err(message)?;
    let order: Vec<_> = order
        .iter()
        .map(|field| {
            let (name, direction) = field.split_once(':').unwrap_or((field, "asc"));
            let direction = match direction {
                "asc" => SortDirection::Ascending,
                "desc" => SortDirection::Descending,
                _ => return Err("order direction must be asc or desc".to_owned()),
            };

            Ok((name, direction))
        })
        .collect::<Result<_, String>>()?;
    let host = runtime();
    let db = open(&host);

    match entity.as_str() {
        "Entry" => {
            let query = filter_rows(entry_query(&db, minimum, category, entry_id)?, &filters)?;

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
            let query = filter_rows(document_query(&db, minimum, category, entry_id)?, &filters)?;

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
                filter_rows(decimal_query(&db, minimum, category, entry_id)?, &filters)?,
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
        "BinaryObject" => {
            let query = filter_rows(blob::query(&db, minimum, category, entry_id)?, &filters)?;

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
                &["id", "payload", "optional"],
                |r| {
                    vec![
                        r.id.to_string(),
                        hex(&r.payload),
                        blob::optional_hex(r.optional),
                    ]
                },
            )
        }
        _ => Err(
            "query supports Entry, Document, DecimalRow and BinaryObject in this fixture".into(),
        ),
    }
}

pub(super) fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(2 + bytes.len() * 2);
    text.push_str("0x");
    for byte in bytes {
        write!(text, "{byte:02x}").expect("writing to String cannot fail");
    }

    text
}

///
/// CategoryOnly
/// Carries the `Nat64` value of `category`.
///

#[ic_db::record(fields(field(name = "category", value(item(prim = "Nat64")))))]
struct CategoryOnly {}

///
/// AmountOnly
/// Carries the `Nat64` value of `amount`.
///

#[ic_db::record(fields(field(name = "amount", value(item(prim = "Nat64")))))]
struct AmountOnly {}

///
/// TitleOnly
/// Carries the `Text` value of `title`.
///

#[ic_db::record(fields(field(name = "title", value(item(prim = "Text", max_bytes = 32)))))]
struct TitleOnly {}

///
/// NoteOnly
/// Carries an optional `Text` value of `note`.
///

#[ic_db::record(fields(field(name = "note", value(opt, item(prim = "Text", max_bytes = 64)))))]
pub(super) struct NoteOnly {}

///
/// PriorityOnly
/// Carries an optional `Nat64` value of `priority`.
///

#[ic_db::record(fields(field(name = "priority", value(opt, item(prim = "Nat64")))))]
struct PriorityOnly {}

///
/// EntryIdOnly
/// Carries the `Nat64` value of `entry_id`.
///

#[ic_db::record(fields(field(name = "entry_id", value(item(prim = "Nat64")))))]
struct EntryIdOnly {}

///
/// TotalAmount
/// Carries the `Nat128` value of `amount`.
///

#[ic_db::record(fields(field(name = "amount", value(item(prim = "Nat128")))))]
struct TotalAmount {}

///
/// AverageAmount
/// Carries the `Decimal` value of `amount`.
///

#[ic_db::record(fields(field(name = "amount", value(item(prim = "Decimal", scale = 2)))))]
struct AverageAmount {}

///
/// TotalPriority
/// Carries an optional `Nat128` value of `priority`.
///

#[ic_db::record(fields(field(name = "priority", value(opt, item(prim = "Nat128")))))]
struct TotalPriority {}

///
/// TotalScore
/// Carries the `Int128` value of `score`.
///

#[ic_db::record(fields(field(name = "score", value(item(prim = "Int128")))))]
struct TotalScore {}

///
/// TotalDecimal
/// Carries the `Decimal` value of `value`.
///

#[ic_db::record(fields(field(name = "value", value(item(prim = "Decimal", scale = 2)))))]
struct TotalDecimal {}

///
/// OptionalDecimal
/// Carries an optional `Decimal` value of `maybe`.
///

#[ic_db::record(fields(field(name = "maybe", value(opt, item(prim = "Decimal", scale = 2)))))]
struct OptionalDecimal {}

///
/// ZoneOnly
/// Carries the `Bool` value of `zone`.
///

#[ic_db::record(fields(field(name = "zone", value(item(prim = "Bool")))))]
struct ZoneOnly {}

fn filter_rows<'a, E: Entity>(
    query: Query<'a, E>,
    filters: &'a RowFilters,
) -> Result<Query<'a, E>, String> {
    let RowFilters {
        payload,
        compare,
        between_fields,
        not_between_fields,
        text,
        boolean,
        present,
        absent,
        length,
    } = filters;
    blob::validate(E::SCHEMA.name, payload)?;
    let mut query = blob::filter(query, payload);
    if let Some(field) = present {
        query = query.filter_present(field);
    }
    if let Some(field) = absent {
        query = query.filter_absent(field);
    }
    if let Some([left, op, right]) = compare {
        query = query.filter_fields(left, comparison(op)?, right);
    }
    if let Some([field, lower, upper]) = between_fields {
        query = query.filter_between_fields(field, lower, upper);
    }
    if let Some([field, lower, upper]) = not_between_fields {
        query = query.filter_not_between_fields(field, lower, upper);
    }
    if let Some((field, value)) = boolean {
        query = query.filter(field, Comparison::Eq, *value);
    }
    if let Some([field, op, value]) = text {
        query = query.filter(field, comparison(op)?, value.as_str());
    }
    if let Some((field, op, n)) = length {
        query = query.filter_length(field, comparison(op)?, *n);
    }

    Ok(query)
}

fn comparison(op: &str) -> Result<Comparison, String> {
    Ok(match op {
        "eq" => Comparison::Eq,
        "ne" => Comparison::Ne,
        "lt" => Comparison::Lt,
        "le" => Comparison::Le,
        "gt" => Comparison::Gt,
        "ge" => Comparison::Ge,
        "starts-with" => Comparison::StartsWith,
        "ends-with" => Comparison::EndsWith,
        "contains" => Comparison::Contains,
        _ => return Err(format!("unknown comparison '{op}'")),
    })
}

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
    (
        bool,
        (Option<String>, Option<String>),
        Option<String>,
        RowFilters,
    ),
);
type Page = (
    Vec<String>,
    Vec<Vec<String>>,
    Option<Vec<u8>>,
    u32,
    u32,
    u32,
);

fn configure<'a, E: Entity>(
    mut query: Query<'a, E>,
    cursor: Option<&'a QueryCursor>,
    order: &[(&'a str, SortDirection)],
    descending: bool,
    limit: u32,
    work: u32,
    bytes: u32,
) -> Query<'a, E> {
    query = query
        .limit(limit as usize)
        .work_limit(work as usize)
        .test_output_limit(bytes as usize);
    if !order.is_empty() {
        query = query.order_by_each(order);
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
        Some("key-desc") => Ok(Some(GroupOrder::KeyDescending)),
        Some("asc") => Ok(Some(GroupOrder::AggregateAscending)),
        Some("desc") => Ok(Some(GroupOrder::AggregateDescending)),
        _ => Err("unsupported group order".into()),
    }
}

fn entry_query(
    db: &crate::handles::Database,
    minimum: Option<u64>,
    category: Option<u64>,
    entry_id: Option<u64>,
) -> Result<Query<'_, Entry>, String> {
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
    db: &crate::handles::Database,
    minimum: Option<u64>,
    category: Option<u64>,
    entry_id: Option<u64>,
) -> Result<Query<'_, Document>, String> {
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
    db: &crate::handles::Database,
    minimum: Option<u64>,
    category: Option<u64>,
    entry_id: Option<u64>,
) -> Result<Query<'_, DecimalRow>, String> {
    if minimum.is_some() || category.is_some() || entry_id.is_some() {
        return Err("--min, --category and --entry-id do not apply to DecimalRow".into());
    }

    Ok(db.query::<DecimalRow>())
}

fn select_groups<'a, E: Entity>(
    mut query: Query<'a, E>,
    select: &str,
    (group_min, group_max): (Option<&'a str>, Option<&'a str>),
    group_order: Option<GroupOrder>,
) -> Result<Query<'a, E>, String> {
    for (comparison, value) in [(Comparison::Ge, group_min), (Comparison::Le, group_max)] {
        let Some(value) = value else {
            continue;
        };
        if value.len() > 64 {
            return Err(message(Error::InvalidOperand));
        }
        query = match select {
            "entry_id,min:note" | "entry_id,max:note" => query.filter_groups(comparison, value),
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

fn execute<'a, E: Entity>(
    mut query: Query<'a, E>,
    select: &str,
    (distinct, bounds, group_order): (bool, (Option<&'a str>, Option<&'a str>), Option<GroupOrder>),
    columns: &[&str],
    render: fn(E) -> Vec<String>,
) -> Result<Page, String> {
    if distinct {
        query = query.distinct();
    }
    let query = select_groups(query, select, bounds, group_order)?;

    match select {
        "payload"
        | "optional"
        | "min:payload"
        | "max:payload"
        | "min:optional"
        | "max:optional"
        | "distinct-count:payload"
        | "distinct-count:optional"
        | "payload,count"
        | "payload,count:optional"
        | "optional,count"
        | "optional,count:optional" => blob::execute(query, select),
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
            .map(|r| display_scalar(&r, "count")),
        "zone,distinct-count:value" => query
            .group_count_distinct::<ZoneOnly, TotalDecimal>()
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "zone,count:value" => query
            .group_count_field::<ZoneOnly>("value")
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "distinct-count:maybe" => query
            .count_distinct::<OptionalDecimal>()
            .map(|r| display_scalar(&r, "count")),
        "zone,distinct-count:maybe" => query
            .group_count_distinct::<ZoneOnly, OptionalDecimal>()
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "zone,count:maybe" => query
            .group_count_field::<ZoneOnly>("maybe")
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "exists" => query.exists().map(|r| display_scalar(&r, "exists")),
        "count" => query.count().map(|r| display_scalar(&r, "count")),
        "distinct-count:category" => query
            .count_distinct::<CategoryOnly>()
            .map(|r| display_scalar(&r, "count")),
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
        s if s.starts_with("count:") => query
            .count_field(&s[6..])
            .map(|r| display_scalar(&r, "count")),
        s if s.starts_with("category,count:") => query
            .group_count_field::<CategoryOnly>(&s[15..])
            .map(|r| display_groups(r, "category", |k| k.category.to_string())),
        "full" => query.fetch().map(|p| display(p, columns, render)),
        "id" => query
            .fetch_as::<IdOnly>()
            .map(|p| display(p, &["id"], |r| vec![r.id.to_string()])),
        "title" => query
            .fetch_as::<TitleOnly>()
            .map(|p| display(p, &["title"], |r| vec![r.title])),
        "note" => query
            .fetch_as::<NoteOnly>()
            .map(|p| display(p, &["note"], |r| vec![r.note.unwrap_or_else(|| "—".into())])),
        "id,amount" => query.fetch_as::<Summary>().map(|p| {
            display(p, &["id", "amount"], |r| {
                vec![r.id.to_string(), r.amount.to_string()]
            })
        }),
        "category" => query
            .fetch_as::<CategoryOnly>()
            .map(|p| display(p, &["category"], |r| vec![r.category.to_string()])),
        "value" => query
            .fetch_as::<TotalDecimal>()
            .map(|p| display(p, &["value"], |r| vec![r.value.to_string()])),
        "maybe" => query
            .fetch_as::<OptionalDecimal>()
            .map(|p| display(p, &["maybe"], |r| vec![optional(r.maybe)])),
        _ => return Err("unsupported projection or aggregate".into()),
    }
    .map_err(message)
}

fn grouped_extremum<E: Entity, K: Value, R: Value>(
    query: Query<'_, E>,
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
    query: Query<'_, E>,
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

fn display_scalar<T: ToString>(r: &Aggregate<T>, column: &str) -> Page {
    (
        vec![column.into()],
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
        Error::RowTooLarge => "one matching result exceeds the output budget".into(),
        Error::ArithmeticOverflow => "aggregate exceeds its declared result range".into(),
        Error::Limit => "query exceeds a database limit".into(),
        other => format!("database refused query: {other:?}"),
    }
}
