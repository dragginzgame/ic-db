//! Thin local CLI adapter: product jobs own accumulation, progress and pages.
use super::{
    AmountOnly, AverageAmount, CategoryOnly, EntryIdOnly, NoteOnly, OptionalDecimal, PriorityOnly,
    TotalAmount, TotalDecimal, TotalPriority, TotalScore, ZoneOnly, decimal_query, document_query,
    entry_query, optional, parse_group_order, select_groups,
};
use crate::{
    Backing,
    aggregate_store::open_jobs,
    open, runtime,
    schema::{DecimalRow, Document, Entry},
};
use ic_db::{
    AggregateJobs, Entity, Error, GroupCount, GroupCursor, GroupPage, GroupValue, JobHandle,
    JobOutcome, JobStep, Query, Timestamp,
};

fn now() -> Timestamp {
    Timestamp::from_millis((ic_platform_runtime::system::time() / 1_000_000) as i64)
}

// Reconstructed on every call; product admission binds these semantics to the job.
type Selection = (
    String,
    String,
    (Option<u64>, Option<u64>, Option<u64>),
    u32,
    (Option<String>, Option<String>),
    Option<String>,
);
fn configured<'a, E: Entity>(
    query: Query<'a, E, Backing>,
    selection: &'a Selection,
) -> Result<Query<'a, E, Backing>, String> {
    let (_, select, _, limit, (group_min, group_max), group_order) = selection;
    select_groups(
        query.limit(*limit as usize),
        select,
        (group_min.as_deref(), group_max.as_deref()),
        parse_group_order(group_order.as_deref())?,
    )
}

#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "aggregate_start",
    guard = crate::transport::admit::<120>,
    decode = crate::transport::decode::<_, 120>,
    encode = crate::transport::encode::<_, 120>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn aggregate_start(selection: Selection) -> Result<Vec<u8>, String> {
    let host = runtime();
    let db = open(&host);
    let mut jobs = open_jobs(&host);
    let now = now();
    let expires = Timestamp::from_millis(
        now.as_millis()
            .checked_add(60_000)
            .ok_or("clock overflow")?,
    );
    let (entity, select, (minimum, category, entry_id), ..) = &selection;
    let result = match entity.as_str() {
        "Entry" => start_entry(
            configured(
                entry_query(&db, *minimum, *category, *entry_id)?,
                &selection,
            )?,
            &mut jobs,
            now,
            expires,
            select,
        ),
        "Document" => start_document(
            configured(
                document_query(&db, *minimum, *category, *entry_id)?,
                &selection,
            )?,
            &mut jobs,
            now,
            expires,
            select,
        ),
        "DecimalRow" => start_decimal(
            configured(
                decimal_query(&db, *minimum, *category, *entry_id)?,
                &selection,
            )?,
            &mut jobs,
            now,
            expires,
            select,
        ),
        _ => Err(Error::InvalidQuery),
    };
    result
        .map(|handle| handle.to_bytes())
        .map_err(|e| e.to_string())
}

fn start_entry(
    query: Query<'_, Entry, Backing>,
    jobs: &mut AggregateJobs<Backing>,
    now: Timestamp,
    expires: Timestamp,
    select: &str,
) -> Result<JobHandle, Error> {
    match select {
        "count" => query.start_count(jobs, now, expires),
        "count:amount" => query.start_count_field("amount", jobs, now, expires),
        "distinct-count:amount" => query.start_count_distinct::<AmountOnly>(jobs, now, expires),
        "sum:amount" => query.start_sum::<TotalAmount>(jobs, now, expires),
        "avg:amount" => query.start_avg::<AverageAmount>(jobs, now, expires),
        "min:amount" => query.start_min::<AmountOnly>(jobs, now, expires),
        "max:amount" => query.start_max::<AmountOnly>(jobs, now, expires),
        "category,count" => query.start_group_count::<CategoryOnly>(jobs, now, expires),
        "category,count:amount" => {
            query.start_group_count_field::<CategoryOnly>("amount", jobs, now, expires)
        }
        "category,distinct-count:amount" => {
            query.start_group_count_distinct::<CategoryOnly, AmountOnly>(jobs, now, expires)
        }
        "category,sum:amount" => {
            query.start_group_sum::<CategoryOnly, TotalAmount>(jobs, now, expires)
        }
        "category,avg:amount" => {
            query.start_group_avg::<CategoryOnly, AverageAmount>(jobs, now, expires)
        }
        "category,min:amount" => {
            query.start_group_min::<CategoryOnly, AmountOnly>(jobs, now, expires)
        }
        "category,max:amount" => {
            query.start_group_max::<CategoryOnly, AmountOnly>(jobs, now, expires)
        }
        _ => Err(Error::InvalidQuery),
    }
}

type Request = (Selection, Vec<u8>, u64, u32, u32, u32, Option<Vec<u8>>);
type Output = (Vec<String>, Vec<Vec<String>>, Option<Vec<u8>>);
type Response = (u64, String, u64, u64, u64, Output);
fn display<T>(step: JobStep<T>, convert: impl FnOnce(T) -> Output) -> Response {
    let (phase, output) = match step.outcome {
        JobOutcome::Running => ("Running", (vec![], vec![], None)),
        JobOutcome::Complete(value) => ("Complete", convert(value)),
        JobOutcome::Invalidated => ("Invalidated", (vec![], vec![], None)),
        JobOutcome::Expired => ("Expired", (vec![], vec![], None)),
    };
    (
        step.sequence,
        phase.into(),
        step.examined,
        step.input_bytes,
        step.output_bytes as u64,
        output,
    )
}
fn scalar(label: &str, value: Option<String>) -> Output {
    (
        vec![label.into()],
        value.into_iter().map(|v| vec![v]).collect(),
        None,
    )
}
fn counts<K>(page: GroupPage<GroupCount<K>>, column: &str, key: fn(K) -> String) -> Output {
    (
        vec![column.into(), "count".into()],
        page.rows
            .into_iter()
            .map(|r| vec![key(r.key), r.count.to_string()])
            .collect(),
        page.next.map(|c| c.to_bytes()),
    )
}
fn values<K, R>(
    page: GroupPage<GroupValue<K, R>>,
    columns: [&str; 2],
    key: fn(K) -> String,
    render: fn(R) -> String,
) -> Output {
    (
        columns.into_iter().map(String::from).collect(),
        page.rows
            .into_iter()
            .map(|r| vec![key(r.key), optional(r.value.map(render))])
            .collect(),
        page.next.map(|c| c.to_bytes()),
    )
}
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "aggregate_resume",
    guard = crate::transport::admit::<121>,
    decode = crate::transport::decode::<_, 121>,
    encode = crate::transport::encode::<_, 121>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn aggregate_resume(
    (selection, handle, sequence, work, bytes, page_size, after): Request,
) -> Result<Response, String> {
    let host = runtime();
    let db = open(&host);
    let mut jobs = open_jobs(&host);
    let handle = JobHandle::from_bytes(&handle).map_err(|e| e.to_string())?;
    let cursor = after
        .as_deref()
        .map(GroupCursor::from_bytes)
        .transpose()
        .map_err(|e| e.to_string())?;
    let now = now();
    let (entity, select, (minimum, category, entry_id), ..) = &selection;
    let budgets = (work, bytes, page_size, cursor.as_ref());
    let result = match entity.as_str() {
        "Entry" => resume_entry(
            resume_query(
                configured(
                    entry_query(&db, *minimum, *category, *entry_id)?,
                    &selection,
                )?,
                select,
                budgets,
            )?,
            &mut jobs,
            &handle,
            sequence,
            now,
            select,
        ),
        "Document" => resume_document(
            resume_query(
                configured(
                    document_query(&db, *minimum, *category, *entry_id)?,
                    &selection,
                )?,
                select,
                budgets,
            )?,
            &mut jobs,
            &handle,
            sequence,
            now,
            select,
        ),
        "DecimalRow" => resume_decimal(
            resume_query(
                configured(
                    decimal_query(&db, *minimum, *category, *entry_id)?,
                    &selection,
                )?,
                select,
                budgets,
            )?,
            &mut jobs,
            &handle,
            sequence,
            now,
            select,
        ),
        _ => Err(Error::InvalidQuery),
    };
    result.map_err(|e| e.to_string())
}

fn resume_query<'a, E: Entity>(
    mut query: Query<'a, E, Backing>,
    select: &str,
    (work, bytes, page_size, cursor): (u32, u32, u32, Option<&'a GroupCursor>),
) -> Result<Query<'a, E, Backing>, String> {
    query = query.work_limit(work as usize).byte_limit(bytes as usize);
    if select.contains(',') {
        query = query.group_page(page_size as usize, cursor);
    } else if cursor.is_some() {
        return Err("scalar aggregates have no page cursor".into());
    }
    Ok(query)
}

fn resume_entry(
    query: Query<'_, Entry, Backing>,
    jobs: &mut AggregateJobs<Backing>,
    handle: &JobHandle,
    sequence: u64,
    now: Timestamp,
    select: &str,
) -> Result<Response, Error> {
    match select {
        "count" => query
            .resume_count(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
        "count:amount" => query
            .resume_count_field("amount", jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
        "distinct-count:amount" => query
            .resume_count_distinct::<AmountOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
        "sum:amount" => query
            .resume_sum::<TotalAmount>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("amount", v.map(|r| r.amount.to_string())))),
        "avg:amount" => query
            .resume_avg::<AverageAmount>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("amount", v.map(|r| r.amount.to_string())))),
        "min:amount" => query
            .resume_min::<AmountOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("amount", v.map(|r| r.amount.to_string())))),
        "max:amount" => query
            .resume_max::<AmountOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("amount", v.map(|r| r.amount.to_string())))),
        "category,count" => query
            .resume_group_count::<CategoryOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |p| counts(p, "category", |k| k.category.to_string()))),
        "category,count:amount" => query
            .resume_group_count_field::<CategoryOnly>("amount", jobs, handle, sequence, now)
            .map(|s| display(s, |p| counts(p, "category", |k| k.category.to_string()))),
        "category,distinct-count:amount" => query
            .resume_group_count_distinct::<CategoryOnly, AmountOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |p| counts(p, "category", |k| k.category.to_string()))),
        "category,sum:amount" => query
            .resume_group_sum::<CategoryOnly, TotalAmount>(jobs, handle, sequence, now)
            .map(|s| {
                display(s, |p| {
                    values(
                        p,
                        ["category", "amount"],
                        |k| k.category.to_string(),
                        |r| r.amount.to_string(),
                    )
                })
            }),
        "category,avg:amount" => query
            .resume_group_avg::<CategoryOnly, AverageAmount>(jobs, handle, sequence, now)
            .map(|s| {
                display(s, |p| {
                    values(
                        p,
                        ["category", "amount"],
                        |k| k.category.to_string(),
                        |r| r.amount.to_string(),
                    )
                })
            }),
        "category,min:amount" => query
            .resume_group_min::<CategoryOnly, AmountOnly>(jobs, handle, sequence, now)
            .map(|s| {
                display(s, |p| {
                    values(
                        p,
                        ["category", "amount"],
                        |k| k.category.to_string(),
                        |r| r.amount.to_string(),
                    )
                })
            }),
        "category,max:amount" => query
            .resume_group_max::<CategoryOnly, AmountOnly>(jobs, handle, sequence, now)
            .map(|s| {
                display(s, |p| {
                    values(
                        p,
                        ["category", "amount"],
                        |k| k.category.to_string(),
                        |r| r.amount.to_string(),
                    )
                })
            }),
        _ => Err(Error::InvalidQuery),
    }
}

#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "aggregate_cancel",
    guard = crate::transport::admit::<122>,
    decode = crate::transport::decode::<_, 122>,
    encode = crate::transport::encode::<_, 122>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Platform entrypoints receive an owned decoded request"
)]
fn aggregate_cancel(handle: Vec<u8>) -> Result<(), String> {
    let host = runtime();
    let handle = JobHandle::from_bytes(&handle).map_err(|e| e.to_string())?;
    open_jobs(&host).cancel(&handle).map_err(|e| e.to_string())
}

fn start_document(
    query: Query<'_, Document, Backing>,
    jobs: &mut AggregateJobs<Backing>,
    now: Timestamp,
    expires: Timestamp,
    select: &str,
) -> Result<JobHandle, Error> {
    match select {
        "count" => query.start_count(jobs, now, expires),
        "count:note" => query.start_count_field("note", jobs, now, expires),
        "count:priority" => query.start_count_field("priority", jobs, now, expires),
        "distinct-count:note" => query.start_count_distinct::<NoteOnly>(jobs, now, expires),
        "min:note" => query.start_min::<NoteOnly>(jobs, now, expires),
        "max:note" => query.start_max::<NoteOnly>(jobs, now, expires),
        "min:priority" => query.start_min::<PriorityOnly>(jobs, now, expires),
        "max:priority" => query.start_max::<PriorityOnly>(jobs, now, expires),
        "sum:priority" => query.start_sum::<TotalPriority>(jobs, now, expires),
        "sum:score" => query.start_sum::<TotalScore>(jobs, now, expires),
        "entry_id,count" => query.start_group_count::<EntryIdOnly>(jobs, now, expires),
        "entry_id,count:note" => {
            query.start_group_count_field::<EntryIdOnly>("note", jobs, now, expires)
        }
        "entry_id,count:priority" => {
            query.start_group_count_field::<EntryIdOnly>("priority", jobs, now, expires)
        }
        "entry_id,distinct-count:note" => {
            query.start_group_count_distinct::<EntryIdOnly, NoteOnly>(jobs, now, expires)
        }
        "entry_id,min:note" => query.start_group_min::<EntryIdOnly, NoteOnly>(jobs, now, expires),
        "entry_id,max:note" => query.start_group_max::<EntryIdOnly, NoteOnly>(jobs, now, expires),
        "entry_id,min:priority" => {
            query.start_group_min::<EntryIdOnly, PriorityOnly>(jobs, now, expires)
        }
        "entry_id,max:priority" => {
            query.start_group_max::<EntryIdOnly, PriorityOnly>(jobs, now, expires)
        }
        "entry_id,sum:priority" => {
            query.start_group_sum::<EntryIdOnly, TotalPriority>(jobs, now, expires)
        }
        "entry_id,sum:score" => {
            query.start_group_sum::<EntryIdOnly, TotalScore>(jobs, now, expires)
        }
        _ => Err(Error::InvalidQuery),
    }
}

fn resume_document(
    query: Query<'_, Document, Backing>,
    jobs: &mut AggregateJobs<Backing>,
    handle: &JobHandle,
    sequence: u64,
    now: Timestamp,
    select: &str,
) -> Result<Response, Error> {
    match select {
        "count" => query
            .resume_count(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
        "count:note" => query
            .resume_count_field("note", jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
        "count:priority" => query
            .resume_count_field("priority", jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
        "distinct-count:note" => query
            .resume_count_distinct::<NoteOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
        "min:note" => query
            .resume_min::<NoteOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("note", v.map(|r| optional(r.note))))),
        "max:note" => query
            .resume_max::<NoteOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("note", v.map(|r| optional(r.note))))),
        "min:priority" => query
            .resume_min::<PriorityOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("priority", v.map(|r| optional(r.priority))))),
        "max:priority" => query
            .resume_max::<PriorityOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("priority", v.map(|r| optional(r.priority))))),
        "sum:priority" => query
            .resume_sum::<TotalPriority>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("priority", v.map(|r| optional(r.priority))))),
        "sum:score" => query
            .resume_sum::<TotalScore>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("score", v.map(|r| r.score.to_string())))),
        "entry_id,count" => query
            .resume_group_count::<EntryIdOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |p| counts(p, "entry_id", |k| k.entry_id.to_string()))),
        "entry_id,count:note" => query
            .resume_group_count_field::<EntryIdOnly>("note", jobs, handle, sequence, now)
            .map(|s| display(s, |p| counts(p, "entry_id", |k| k.entry_id.to_string()))),
        "entry_id,count:priority" => query
            .resume_group_count_field::<EntryIdOnly>("priority", jobs, handle, sequence, now)
            .map(|s| display(s, |p| counts(p, "entry_id", |k| k.entry_id.to_string()))),
        "entry_id,distinct-count:note" => query
            .resume_group_count_distinct::<EntryIdOnly, NoteOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |p| counts(p, "entry_id", |k| k.entry_id.to_string()))),
        "entry_id,min:note" => query
            .resume_group_min::<EntryIdOnly, NoteOnly>(jobs, handle, sequence, now)
            .map(|s| {
                display(s, |p| {
                    values(
                        p,
                        ["entry_id", "note"],
                        |k| k.entry_id.to_string(),
                        |r| optional(r.note),
                    )
                })
            }),
        "entry_id,max:note" => query
            .resume_group_max::<EntryIdOnly, NoteOnly>(jobs, handle, sequence, now)
            .map(|s| {
                display(s, |p| {
                    values(
                        p,
                        ["entry_id", "note"],
                        |k| k.entry_id.to_string(),
                        |r| optional(r.note),
                    )
                })
            }),
        "entry_id,min:priority" => query
            .resume_group_min::<EntryIdOnly, PriorityOnly>(jobs, handle, sequence, now)
            .map(|s| {
                display(s, |p| {
                    values(
                        p,
                        ["entry_id", "priority"],
                        |k| k.entry_id.to_string(),
                        |r| optional(r.priority),
                    )
                })
            }),
        "entry_id,max:priority" => query
            .resume_group_max::<EntryIdOnly, PriorityOnly>(jobs, handle, sequence, now)
            .map(|s| {
                display(s, |p| {
                    values(
                        p,
                        ["entry_id", "priority"],
                        |k| k.entry_id.to_string(),
                        |r| optional(r.priority),
                    )
                })
            }),
        "entry_id,sum:priority" => query
            .resume_group_sum::<EntryIdOnly, TotalPriority>(jobs, handle, sequence, now)
            .map(|s| {
                display(s, |p| {
                    values(
                        p,
                        ["entry_id", "priority"],
                        |k| k.entry_id.to_string(),
                        |r| optional(r.priority),
                    )
                })
            }),
        "entry_id,sum:score" => query
            .resume_group_sum::<EntryIdOnly, TotalScore>(jobs, handle, sequence, now)
            .map(|s| {
                display(s, |p| {
                    values(
                        p,
                        ["entry_id", "score"],
                        |k| k.entry_id.to_string(),
                        |r| r.score.to_string(),
                    )
                })
            }),
        _ => Err(Error::InvalidQuery),
    }
}

fn start_decimal(
    query: Query<'_, DecimalRow, Backing>,
    jobs: &mut AggregateJobs<Backing>,
    now: Timestamp,
    expires: Timestamp,
    select: &str,
) -> Result<JobHandle, Error> {
    match select {
        "count:value" => query.start_count_field("value", jobs, now, expires),
        "distinct-count:value" => query.start_count_distinct::<TotalDecimal>(jobs, now, expires),
        "zone,count:value" => {
            query.start_group_count_field::<ZoneOnly>("value", jobs, now, expires)
        }
        "zone,distinct-count:value" => {
            query.start_group_count_distinct::<ZoneOnly, TotalDecimal>(jobs, now, expires)
        }
        "count:maybe" => query.start_count_field("maybe", jobs, now, expires),
        "distinct-count:maybe" => query.start_count_distinct::<OptionalDecimal>(jobs, now, expires),
        "zone,count:maybe" => {
            query.start_group_count_field::<ZoneOnly>("maybe", jobs, now, expires)
        }
        "zone,distinct-count:maybe" => {
            query.start_group_count_distinct::<ZoneOnly, OptionalDecimal>(jobs, now, expires)
        }
        "count" => query.start_count(jobs, now, expires),
        "zone,count" => query.start_group_count::<ZoneOnly>(jobs, now, expires),
        "min:value" => query.start_min::<TotalDecimal>(jobs, now, expires),
        "zone,min:value" => query.start_group_min::<ZoneOnly, TotalDecimal>(jobs, now, expires),
        "max:value" => query.start_max::<TotalDecimal>(jobs, now, expires),
        "zone,max:value" => query.start_group_max::<ZoneOnly, TotalDecimal>(jobs, now, expires),
        "min:maybe" => query.start_min::<OptionalDecimal>(jobs, now, expires),
        "zone,min:maybe" => query.start_group_min::<ZoneOnly, OptionalDecimal>(jobs, now, expires),
        "max:maybe" => query.start_max::<OptionalDecimal>(jobs, now, expires),
        "zone,max:maybe" => query.start_group_max::<ZoneOnly, OptionalDecimal>(jobs, now, expires),
        "sum:value" => query.start_sum::<TotalDecimal>(jobs, now, expires),
        "zone,sum:value" => query.start_group_sum::<ZoneOnly, TotalDecimal>(jobs, now, expires),
        "avg:value" => query.start_avg::<TotalDecimal>(jobs, now, expires),
        "zone,avg:value" => query.start_group_avg::<ZoneOnly, TotalDecimal>(jobs, now, expires),
        "sum:maybe" => query.start_sum::<OptionalDecimal>(jobs, now, expires),
        "zone,sum:maybe" => query.start_group_sum::<ZoneOnly, OptionalDecimal>(jobs, now, expires),
        "avg:maybe" => query.start_avg::<OptionalDecimal>(jobs, now, expires),
        "zone,avg:maybe" => query.start_group_avg::<ZoneOnly, OptionalDecimal>(jobs, now, expires),
        _ => Err(Error::InvalidQuery),
    }
}

fn resume_decimal(
    query: Query<'_, DecimalRow, Backing>,
    jobs: &mut AggregateJobs<Backing>,
    handle: &JobHandle,
    sequence: u64,
    now: Timestamp,
    select: &str,
) -> Result<Response, Error> {
    match select {
        "count:value" => query
            .resume_count_field("value", jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
        "distinct-count:value" => query
            .resume_count_distinct::<TotalDecimal>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
        "zone,count:value" => query
            .resume_group_count_field::<ZoneOnly>("value", jobs, handle, sequence, now)
            .map(|s| display(s, |p| counts(p, "zone", |k| k.zone.to_string()))),
        "zone,distinct-count:value" => query
            .resume_group_count_distinct::<ZoneOnly, TotalDecimal>(jobs, handle, sequence, now)
            .map(|s| display(s, |p| counts(p, "zone", |k| k.zone.to_string()))),
        "count:maybe" => query
            .resume_count_field("maybe", jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
        "distinct-count:maybe" => query
            .resume_count_distinct::<OptionalDecimal>(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
        "zone,count:maybe" => query
            .resume_group_count_field::<ZoneOnly>("maybe", jobs, handle, sequence, now)
            .map(|s| display(s, |p| counts(p, "zone", |k| k.zone.to_string()))),
        "zone,distinct-count:maybe" => query
            .resume_group_count_distinct::<ZoneOnly, OptionalDecimal>(jobs, handle, sequence, now)
            .map(|s| display(s, |p| counts(p, "zone", |k| k.zone.to_string()))),
        "count" => query
            .resume_count(jobs, handle, sequence, now)
            .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
        "zone,count" => query
            .resume_group_count::<ZoneOnly>(jobs, handle, sequence, now)
            .map(|s| display(s, |p| counts(p, "zone", |k| k.zone.to_string()))),
        "sum:value" | "avg:value" | "min:value" | "max:value" => {
            let result = match select {
                "avg:value" => query.resume_avg::<TotalDecimal>(jobs, handle, sequence, now),
                "min:value" => query.resume_min::<TotalDecimal>(jobs, handle, sequence, now),
                "max:value" => query.resume_max::<TotalDecimal>(jobs, handle, sequence, now),
                _ => query.resume_sum::<TotalDecimal>(jobs, handle, sequence, now),
            };
            result.map(|s| display(s, |v| scalar("value", v.map(|r| r.value.to_string()))))
        }
        "zone,sum:value" | "zone,avg:value" | "zone,min:value" | "zone,max:value" => {
            let result =
                match select {
                    "zone,avg:value" => query
                        .resume_group_avg::<ZoneOnly, TotalDecimal>(jobs, handle, sequence, now),
                    "zone,min:value" => query
                        .resume_group_min::<ZoneOnly, TotalDecimal>(jobs, handle, sequence, now),
                    "zone,max:value" => query
                        .resume_group_max::<ZoneOnly, TotalDecimal>(jobs, handle, sequence, now),
                    _ => query
                        .resume_group_sum::<ZoneOnly, TotalDecimal>(jobs, handle, sequence, now),
                };
            result.map(|s| {
                display(s, |p| {
                    values(
                        p,
                        ["zone", "value"],
                        |k| k.zone.to_string(),
                        |r| r.value.to_string(),
                    )
                })
            })
        }
        "sum:maybe" | "avg:maybe" | "min:maybe" | "max:maybe" => {
            let result = match select {
                "avg:maybe" => query.resume_avg::<OptionalDecimal>(jobs, handle, sequence, now),
                "min:maybe" => query.resume_min::<OptionalDecimal>(jobs, handle, sequence, now),
                "max:maybe" => query.resume_max::<OptionalDecimal>(jobs, handle, sequence, now),
                _ => query.resume_sum::<OptionalDecimal>(jobs, handle, sequence, now),
            };
            result.map(|s| display(s, |v| scalar("maybe", v.map(|r| optional(r.maybe)))))
        }
        "zone,sum:maybe" | "zone,avg:maybe" | "zone,min:maybe" | "zone,max:maybe" => {
            let result =
                match select {
                    "zone,avg:maybe" => query
                        .resume_group_avg::<ZoneOnly, OptionalDecimal>(jobs, handle, sequence, now),
                    "zone,min:maybe" => query
                        .resume_group_min::<ZoneOnly, OptionalDecimal>(jobs, handle, sequence, now),
                    "zone,max:maybe" => query
                        .resume_group_max::<ZoneOnly, OptionalDecimal>(jobs, handle, sequence, now),
                    _ => query
                        .resume_group_sum::<ZoneOnly, OptionalDecimal>(jobs, handle, sequence, now),
                };
            result.map(|s| {
                display(s, |p| {
                    values(
                        p,
                        ["zone", "maybe"],
                        |k| k.zone.to_string(),
                        |r| optional(r.maybe),
                    )
                })
            })
        }
        _ => Err(Error::InvalidQuery),
    }
}
