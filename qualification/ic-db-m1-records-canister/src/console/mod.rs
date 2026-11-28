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
    transport::{
        ConsolePage as Page, DisplayPage, EntityFilters, FieldRange, FilterComparison,
        QueryRequest, RowFilters,
    },
};
use ic_db::design::prelude::*;
use ic_db::{
    Aggregate, Comparison, Error, GroupCount, GroupOrder, GroupValue, QueryCursor, QueryPage,
    SortDirection, SummaryMeasure,
};

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
    (request, counter): crate::transport::Measured<QueryRequest>,
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
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn query(request: QueryRequest) -> Result<Page, String> {
    let parsed = ParsedQuery {
        group_order: parse_group_order(request.options.group_order.as_deref())?,
        cursor: crate::queries::decode_cursor(request.cursor.as_deref()).map_err(message)?,
        order: parse_order(&request.order)?,
        request: &request,
    };
    let host = runtime();
    let db = open(&host);
    let entity_filters = &request.entity_filters;

    match request.entity.as_str() {
        "Entry" => parsed.execute(
            entry_query(&db, entity_filters)?,
            &["id", "category", "amount"],
            |r| {
                vec![
                    r.id.to_string(),
                    r.category.to_string(),
                    r.amount.to_string(),
                ]
            },
        ),
        "Document" => parsed.execute(
            document_query(&db, entity_filters)?,
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
        ),
        "DecimalRow" => parsed.execute(
            decimal_query(&db, entity_filters)?,
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
        "BinaryObject" => parsed.execute(
            blob::query(&db, entity_filters)?,
            &["id", "payload", "optional"],
            |r| {
                vec![
                    r.id.to_string(),
                    hex(&r.payload),
                    blob::optional_hex(r.optional),
                ]
            },
        ),
        _ => Err(
            "query supports Entry, Document, DecimalRow and BinaryObject in this fixture".into(),
        ),
    }
}

pub(super) fn hex(bytes: &[u8]) -> String {
    ic_db_qualification_transport::lowercase_hex(bytes, "0x")
}

///
/// CategoryOnly
/// Carries the `Nat64` value of `category`.
///

#[record(fields(field(name = "category", value(item(prim = "Nat64")))))]
struct CategoryOnly {}

///
/// AmountOnly
/// Carries the `Nat64` value of `amount`.
///

#[record(fields(field(name = "amount", value(item(prim = "Nat64")))))]
struct AmountOnly {}

///
/// TitleOnly
/// Carries the `Text` value of `title`.
///

#[record(fields(field(name = "title", value(item(prim = "Text", max_bytes = 32)))))]
struct TitleOnly {}

///
/// NoteOnly
/// Carries an optional `Text` value of `note`.
///

#[record(fields(field(name = "note", value(opt, item(prim = "Text", max_bytes = 64)))))]
pub(super) struct NoteOnly {}

///
/// PriorityOnly
/// Carries an optional `Nat64` value of `priority`.
///

#[record(fields(field(name = "priority", value(opt, item(prim = "Nat64")))))]
struct PriorityOnly {}

///
/// EntryIdOnly
/// Carries the `Nat64` value of `entry_id`.
///

#[record(fields(field(name = "entry_id", value(item(prim = "Nat64")))))]
struct EntryIdOnly {}

///
/// TotalAmount
/// Carries the `Nat128` value of `amount`.
///

#[record(fields(field(name = "amount", value(item(prim = "Nat128")))))]
struct TotalAmount {}

///
/// AverageAmount
/// Carries the `Decimal` value of `amount`.
///

#[record(fields(field(name = "amount", value(item(prim = "Decimal", scale = 2)))))]
struct AverageAmount {}

///
/// TotalPriority
/// Carries an optional `Nat128` value of `priority`.
///

#[record(fields(field(name = "priority", value(opt, item(prim = "Nat128")))))]
struct TotalPriority {}

///
/// TotalScore
/// Carries the `Int128` value of `score`.
///

#[record(fields(field(name = "score", value(item(prim = "Int128")))))]
struct TotalScore {}

///
/// TotalDecimal
/// Carries the `Decimal` value of `value`.
///

#[record(fields(field(name = "value", value(item(prim = "Decimal", scale = 2)))))]
struct TotalDecimal {}

///
/// OptionalDecimal
/// Carries an optional `Decimal` value of `maybe`.
///

#[record(fields(field(name = "maybe", value(opt, item(prim = "Decimal", scale = 2)))))]
struct OptionalDecimal {}

///
/// ZoneOnly
/// Carries the `Bool` value of `zone`.
///

#[record(fields(field(name = "zone", value(item(prim = "Bool")))))]
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
    let mut query = blob::filter(query, payload)?;
    if let Some(field) = present {
        query = query.filter_present(field);
    }
    if let Some(field) = absent {
        query = query.filter_absent(field);
    }
    if let Some(filter) = compare {
        query = query.filter_fields(&filter.field, comparison(filter.op), &filter.operand);
    }
    if let Some(FieldRange {
        field,
        lower,
        upper,
    }) = between_fields
    {
        query = query
            .filter_fields(field, Comparison::Ge, lower)
            .filter_fields(field, Comparison::Le, upper);
    }
    if let Some(FieldRange {
        field,
        lower,
        upper,
    }) = not_between_fields
    {
        query = query.filter_any(&[
            (field, Comparison::Lt, ic_db::QueryOperand::Field(lower)),
            (field, Comparison::Gt, ic_db::QueryOperand::Field(upper)),
        ]);
    }
    if let Some((field, value)) = boolean {
        query = query.filter(field, Comparison::Eq, *value);
    }
    if let Some(filter) = text {
        query = query.filter(
            &filter.field,
            comparison(filter.op),
            filter.operand.as_str(),
        );
    }
    if let Some(filter) = length {
        query = query.filter_length(&filter.field, comparison(filter.op), filter.operand);
    }

    Ok(query)
}

fn comparison(op: FilterComparison) -> Comparison {
    match op {
        FilterComparison::Eq => Comparison::Eq,
        FilterComparison::Ne => Comparison::Ne,
        FilterComparison::Lt => Comparison::Lt,
        FilterComparison::Le => Comparison::Le,
        FilterComparison::Gt => Comparison::Gt,
        FilterComparison::Ge => Comparison::Ge,
        FilterComparison::StartsWith => Comparison::StartsWith,
        FilterComparison::EndsWith => Comparison::EndsWith,
        FilterComparison::Contains => Comparison::Contains,
    }
}

fn parse_direction(text: &str) -> Option<SortDirection> {
    match text {
        "asc" => Some(SortDirection::Ascending),
        "desc" => Some(SortDirection::Descending),
        _ => None,
    }
}

fn parse_order(order: &[String]) -> Result<Vec<(&str, SortDirection)>, String> {
    order
        .iter()
        .map(|field| {
            let (name, direction) = field.split_once(':').unwrap_or((field, "asc"));
            let direction =
                parse_direction(direction).ok_or("order direction must be asc or desc")?;

            Ok((name, direction))
        })
        .collect()
}

fn apply_order<'a, E: Entity>(
    mut query: Query<'a, E>,
    order: &[(&'a str, SortDirection)],
    descending: bool,
) -> Query<'a, E> {
    if !order.is_empty() {
        query = query.order_by(order);
    }
    if descending {
        query = query.descending();
    }

    query
}

fn summary_measure(name: &str) -> Result<SummaryMeasure, String> {
    match name {
        "count" => Ok(SummaryMeasure::Count),
        "sum" => Ok(SummaryMeasure::Sum),
        "avg" => Ok(SummaryMeasure::Average),
        _ => Err("summary measure must be count, sum or avg".into()),
    }
}

fn parse_group_order(order: Option<&str>) -> Result<Option<GroupOrder>, String> {
    match order {
        None => Ok(None),
        Some("key") => Ok(Some(GroupOrder::Key)),
        Some("first") => Ok(Some(GroupOrder::FirstOccurrence)),
        Some("key-desc") => Ok(Some(GroupOrder::KeyDescending)),
        Some("asc") => Ok(Some(GroupOrder::AggregateAscending)),
        Some("desc") => Ok(Some(GroupOrder::AggregateDescending)),
        Some(value) if let Some((measure, direction)) = value.split_once(':') => {
            let direction =
                parse_direction(direction).ok_or("summary order direction must be asc or desc")?;
            Ok(Some(GroupOrder::Summary(
                summary_measure(measure)?,
                direction,
            )))
        }
        _ => Err("unsupported group order".into()),
    }
}

fn entry_query<'a>(
    db: &'a crate::handles::Database,
    filters: &EntityFilters,
) -> Result<Query<'a, Entry>, String> {
    if filters.entry_id.is_some() {
        return Err("--entry-id applies to Document".into());
    }
    let mut query = db.query::<Entry>();
    if let Some(value) = filters.minimum {
        query = query.filter("amount", Comparison::Ge, value);
    }
    if let Some(value) = filters.category {
        query = query.filter("category", Comparison::Eq, value);
    }

    Ok(query)
}

fn document_query<'a>(
    db: &'a crate::handles::Database,
    filters: &EntityFilters,
) -> Result<Query<'a, Document>, String> {
    if filters.minimum.is_some() || filters.category.is_some() {
        return Err("--min and --category apply to Entry".into());
    }
    let mut query = db.query::<Document>();
    if let Some(value) = filters.entry_id {
        query = query.filter("entry_id", Comparison::Eq, value);
    }

    Ok(query)
}

fn decimal_query<'a>(
    db: &'a crate::handles::Database,
    filters: &EntityFilters,
) -> Result<Query<'a, DecimalRow>, String> {
    if filters.minimum.is_some() || filters.category.is_some() || filters.entry_id.is_some() {
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
            "category,summary:amount" | "zone,summary:value" | "zone,summary:maybe" => {
                let (name, threshold) = value
                    .split_once(':')
                    .ok_or("summary bound requires count:VALUE, sum:VALUE or avg:VALUE")?;
                let measure = summary_measure(name)?;
                let invalid = |_| message(Error::InvalidOperand);
                let operand = match measure {
                    SummaryMeasure::Count => {
                        ic_db::IndexValue::Nat64(threshold.parse().map_err(invalid)?)
                    }
                    SummaryMeasure::Sum if select == "category,summary:amount" => {
                        ic_db::IndexValue::Nat128(threshold.parse().map_err(invalid)?)
                    }
                    SummaryMeasure::Sum | SummaryMeasure::Average => ic_db::IndexValue::Decimal(
                        threshold
                            .parse()
                            .map_err(|_| message(Error::InvalidOperand))?,
                    ),
                };
                query.filter_summary(measure, comparison, operand)
            }
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

///
/// ParsedQuery
/// Request options shared by all typed entity dispatch branches.
///
struct ParsedQuery<'a> {
    request: &'a QueryRequest,
    cursor: Option<QueryCursor>,
    order: Vec<(&'a str, SortDirection)>,
    group_order: Option<GroupOrder>,
}

impl ParsedQuery<'_> {
    fn execute<'a, E: Entity>(
        &'a self,
        query: Query<'a, E>,
        columns: &[&str],
        render: fn(E) -> Vec<String>,
    ) -> Result<Page, String> {
        let request = self.request;
        let options = &request.options;
        let query = filter_rows(query, &options.filters)?
            .limit(request.limit as usize)
            .work_limit(request.work as usize)
            .test_output_limit(request.bytes as usize);
        let mut query =
            apply_order(query, &self.order, request.descending).after(self.cursor.as_ref());
        if options.distinct {
            query = query.distinct();
        }
        let select = request.select.as_str();
        let (min, max) = &options.group_bounds;
        let query = select_groups(
            query,
            select,
            (min.as_deref(), max.as_deref()),
            self.group_order,
        )?;

        render_selection(query, select, columns, render)
    }
}

fn render_selection<E: Entity>(
    query: Query<'_, E>,
    select: &str,
    columns: &[&str],
    render: fn(E) -> Vec<String>,
) -> Result<Page, String> {
    match select {
        "avg:amount" => query
            .avg::<AverageAmount>()
            .map(|r| display_value(r, "amount", |r| r.amount.to_string())),
        "category,summary:amount" => query
            .group_summary::<CategoryOnly, TotalAmount, AverageAmount>()
            .map(|r| display_summary(r, "category", entry_summary_row)),
        "zone,summary:value" => query
            .group_summary::<ZoneOnly, TotalDecimal, TotalDecimal>()
            .map(|r| display_summary(r, "zone", decimal_summary_row)),
        "zone,summary:maybe" => query
            .group_summary::<ZoneOnly, OptionalDecimal, OptionalDecimal>()
            .map(|r| display_summary(r, "zone", optional_summary_row)),
        "category,avg:amount" => query.group_avg::<CategoryOnly, AverageAmount>().map(|r| {
            display_group_values(
                r,
                &["category", "amount"],
                |k| k.category.to_string(),
                |v| v.amount.to_string(),
            )
        }),
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
            .map(|r| display_scalar(r, "count")),
        "zone,distinct-count:value" => query
            .group_count_distinct::<ZoneOnly, TotalDecimal>()
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "zone,count:value" => query
            .group_count_field::<ZoneOnly>("value")
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "distinct-count:maybe" => query
            .count_distinct::<OptionalDecimal>()
            .map(|r| display_scalar(r, "count")),
        "zone,distinct-count:maybe" => query
            .group_count_distinct::<ZoneOnly, OptionalDecimal>()
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "zone,count:maybe" => query
            .group_count_field::<ZoneOnly>("maybe")
            .map(|r| display_groups(r, "zone", |k| k.zone.to_string())),
        "exists" => query.exists().map(|r| display_scalar(r, "exists")),
        "count" => query.count().map(|r| display_scalar(r, "count")),
        "distinct-count:category" => query
            .count_distinct::<CategoryOnly>()
            .map(|r| display_scalar(r, "count")),
        "distinct-count:amount" => query
            .count_distinct::<AmountOnly>()
            .map(|r| display_scalar(r, "count")),
        "distinct-count:note" => query
            .count_distinct::<NoteOnly>()
            .map(|r| display_scalar(r, "count")),
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
            .map(|r| display_scalar(r, "count")),
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
        _ => {
            if let Some((op, grouped, field)) = DecimalOperation::parse(select) {
                match field {
                    "value" => op.execute(query, grouped, "value", |r: TotalDecimal| {
                        r.value.to_string()
                    }),
                    "maybe" => op.execute(query, grouped, "maybe", |r: OptionalDecimal| {
                        optional(r.maybe)
                    }),
                    _ => return blob::execute(query, select),
                }
            } else {
                return blob::execute(query, select);
            }
        }
    }
    .map_err(message)
}

enum DecimalOperation {
    Sum,
    Average,
    Min,
    Max,
}

impl DecimalOperation {
    fn parse(select: &str) -> Option<(Self, bool, &str)> {
        let (op, field) = select.split_once(':')?;
        let grouped = op.strip_prefix("zone,");
        let op = match grouped.unwrap_or(op) {
            "sum" => Self::Sum,
            "avg" => Self::Average,
            "min" => Self::Min,
            "max" => Self::Max,
            _ => return None,
        };
        Some((op, grouped.is_some(), field))
    }

    fn execute<E: Entity, R: Value>(
        self,
        query: Query<'_, E>,
        grouped: bool,
        column: &str,
        render: fn(R) -> String,
    ) -> Result<Page, Error> {
        if grouped {
            let result = match self {
                Self::Sum => query.group_sum::<ZoneOnly, R>(),
                Self::Average => query.group_avg::<ZoneOnly, R>(),
                Self::Min => query.group_min::<ZoneOnly, R>(),
                Self::Max => query.group_max::<ZoneOnly, R>(),
            }?;
            Ok(display_group_values(
                result,
                &["zone", column],
                |k| k.zone.to_string(),
                render,
            ))
        } else {
            let result = match self {
                Self::Sum => query.sum::<R>(),
                Self::Average => query.avg::<R>(),
                Self::Min => query.min::<R>(),
                Self::Max => query.max::<R>(),
            }?;
            Ok(display_value(result, column, render))
        }
    }
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

fn summary_cells(
    key: String,
    count: u64,
    sum: Option<impl ToString>,
    average: Option<impl ToString>,
) -> Vec<String> {
    vec![key, count.to_string(), optional(sum), optional(average)]
}

fn entry_summary_row(
    g: ic_db::GroupSummary<CategoryOnly, TotalAmount, AverageAmount>,
) -> Vec<String> {
    summary_cells(
        g.key.category.to_string(),
        g.count,
        g.sum.map(|r| r.amount),
        g.average.map(|r| r.amount),
    )
}

fn decimal_summary_row(
    g: ic_db::GroupSummary<ZoneOnly, TotalDecimal, TotalDecimal>,
) -> Vec<String> {
    summary_cells(
        g.key.zone.to_string(),
        g.count,
        g.sum.map(|r| r.value),
        g.average.map(|r| r.value),
    )
}

fn optional_summary_row(
    g: ic_db::GroupSummary<ZoneOnly, OptionalDecimal, OptionalDecimal>,
) -> Vec<String> {
    summary_cells(
        g.key.zone.to_string(),
        g.count,
        g.sum.and_then(|r| r.maybe),
        g.average.and_then(|r| r.maybe),
    )
}

fn summary_table<T>(
    groups: Vec<T>,
    key: &str,
    render: fn(T) -> Vec<String>,
) -> (Vec<String>, Vec<Vec<String>>) {
    (
        [key, "count", "sum", "avg"].map(String::from).into(),
        groups.into_iter().map(render).collect(),
    )
}

fn display_summary<T>(r: Aggregate<Vec<T>>, key: &str, render: fn(T) -> Vec<String>) -> Page {
    aggregate_page(r, |groups| summary_table(groups, key, render))
}

fn aggregate_page<T>(
    r: Aggregate<T>,
    table: impl FnOnce(T) -> (Vec<String>, Vec<Vec<String>>),
) -> Page {
    let (columns, rows) = table(r.value);

    Page {
        page: DisplayPage {
            columns,
            rows,
            next: None,
        },
        examined: r.examined as u32,
        fetched: r.fetched as u32,
        output_bytes: r.output_bytes as u32,
    }
}

fn display_group_values<K, R>(
    r: Aggregate<Vec<GroupValue<K, R>>>,
    columns: &[&str; 2],
    key: fn(K) -> String,
    render: fn(R) -> String,
) -> Page {
    aggregate_page(r, |rows| value_table(rows, columns, key, render))
}

fn value_table<K, R>(
    rows: Vec<GroupValue<K, R>>,
    columns: &[&str; 2],
    key: fn(K) -> String,
    render: fn(R) -> String,
) -> (Vec<String>, Vec<Vec<String>>) {
    (
        columns.iter().map(|c| (*c).into()).collect(),
        rows.into_iter()
            .map(|g| vec![key(g.key), g.value.map_or_else(|| "—".into(), render)])
            .collect(),
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
    aggregate_page(r, |value| {
        (
            vec![column.into()],
            value.into_iter().map(|value| vec![render(value)]).collect(),
        )
    })
}

fn display_scalar<T: ToString>(r: Aggregate<T>, column: &str) -> Page {
    aggregate_page(r, |value| {
        (vec![column.into()], vec![vec![value.to_string()]])
    })
}

fn display_groups<K>(r: Aggregate<Vec<GroupCount<K>>>, column: &str, key: fn(K) -> String) -> Page {
    aggregate_page(r, |rows| count_table(rows, column, key))
}

fn count_table<K>(
    rows: Vec<GroupCount<K>>,
    column: &str,
    key: fn(K) -> String,
) -> (Vec<String>, Vec<Vec<String>>) {
    (
        vec![column.into(), "count".into()],
        rows.into_iter()
            .map(|g| vec![key(g.key), g.count.to_string()])
            .collect(),
    )
}

fn display<T>(page: QueryPage<T>, columns: &[&str], render: fn(T) -> Vec<String>) -> Page {
    Page {
        page: DisplayPage {
            columns: columns.iter().map(|c| (*c).to_owned()).collect(),
            rows: page.rows.into_iter().map(render).collect(),
            next: page.next.map(|c| c.to_bytes()),
        },
        examined: page.examined as u32,
        fetched: page.fetched as u32,
        output_bytes: page.output_bytes as u32,
    }
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
