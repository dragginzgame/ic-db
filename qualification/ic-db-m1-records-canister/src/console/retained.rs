//! Thin local CLI adapter: product jobs own accumulation, progress and pages.

use super::{
    AmountOnly, AverageAmount, CategoryOnly, EntryIdOnly, NoteOnly, OptionalDecimal, PriorityOnly,
    TotalAmount, TotalDecimal, TotalPriority, TotalScore, ZoneOnly,
    blob::{self, Optional, Payload},
    decimal_query, document_query, entry_query, optional, parse_group_order, select_groups,
};
use crate::{
    aggregate_store::open_jobs,
    handles::{AggregateJobs, Query},
    open, runtime,
    schema::{BinaryObject, DecimalRow, Document, Entry},
};
use ic_db::{
    Entity, Error, GroupCount, GroupCursor, GroupPage, GroupValue, JobHandle, JobOutcome, JobStep,
    Timestamp,
};
use ic_db_qualification_transport::{
    RetainedOutput as Output, RetainedPhase, RetainedProgress as Response, RetainedResume,
    RetainedSelection as Selection,
};

// aggregate_start
// Start a retained console aggregate and return its handle.
// guard  : validates the request frame for method 120.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "aggregate_start",
    guard = crate::transport::admit::<120>,
    decode = crate::transport::decode_timed::<_, 120>,
    encode = crate::transport::encode::<_, 120>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn aggregate_start((selection, time_ns): (Selection, u64)) -> Result<Vec<u8>, String> {
    start_at(&selection, message_time(time_ns))
}

// aggregate_resume
// Advance the retained selection or read a completed result page.
// guard  : validates the request frame for method 121.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "aggregate_resume",
    guard = crate::transport::admit::<121>,
    decode = crate::transport::decode_timed::<_, 121>,
    encode = crate::transport::encode::<_, 121>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn aggregate_resume((request, time_ns): (RetainedResume, u64)) -> Result<Response, String> {
    resume_at(request, message_time(time_ns))
}

// aggregate_cancel
// Cancel the retained aggregate identified by the supplied handle.
// guard  : validates the request frame for method 122.
// limits : 64 KiB request, 64 KiB reply.
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

macro_rules! selectors {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum $name { $($variant),+ }

        impl $name {
            fn parse(text: &str) -> Result<Self, Error> {
                match text {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(Error::InvalidQuery),
                }
            }
        }
    };
}

selectors! { EntrySelector {
    Count => "count",
    CountAmount => "count:amount",
    DistinctAmount => "distinct:amount",
    DistinctCountAmount => "distinct-count:amount",
    DistinctCountCategory => "distinct-count:category",
    SumAmount => "sum:amount",
    AvgAmount => "avg:amount",
    MinAmount => "min:amount",
    MaxAmount => "max:amount",
    CategoryCount => "category,count",
    CategoryCountAmount => "category,count:amount",
    CategoryDistinctCountAmount => "category,distinct-count:amount",
    CategoryDistinctCountCategory => "category,distinct-count:category",
    CategorySummaryAmount => "category,summary:amount",
    CategorySumAmount => "category,sum:amount",
    CategoryAvgAmount => "category,avg:amount",
    CategoryMinAmount => "category,min:amount",
    CategoryMaxAmount => "category,max:amount",
}}

selectors! { DecimalRowSelector {
    CountValue => "count:value",
    DistinctCountValue => "distinct-count:value",
    ZoneCountValue => "zone,count:value",
    ZoneDistinctCountValue => "zone,distinct-count:value",
    CountMaybe => "count:maybe",
    DistinctCountMaybe => "distinct-count:maybe",
    ZoneCountMaybe => "zone,count:maybe",
    ZoneDistinctCountMaybe => "zone,distinct-count:maybe",
    Count => "count",
    ZoneCount => "zone,count",
    MinValue => "min:value",
    ZoneMinValue => "zone,min:value",
    MaxValue => "max:value",
    ZoneMaxValue => "zone,max:value",
    MinMaybe => "min:maybe",
    ZoneMinMaybe => "zone,min:maybe",
    MaxMaybe => "max:maybe",
    ZoneMaxMaybe => "zone,max:maybe",
    SumValue => "sum:value",
    ZoneSummaryValue => "zone,summary:value",
    ZoneSummaryMaybe => "zone,summary:maybe",
    ZoneSumValue => "zone,sum:value",
    AvgValue => "avg:value",
    ZoneAvgValue => "zone,avg:value",
    SumMaybe => "sum:maybe",
    ZoneSumMaybe => "zone,sum:maybe",
    AvgMaybe => "avg:maybe",
    ZoneAvgMaybe => "zone,avg:maybe",
}}

selectors! { DocumentSelector {
    Count => "count",
    CountNote => "count:note",
    CountPriority => "count:priority",
    DistinctCountNote => "distinct-count:note",
    MinNote => "min:note",
    MaxNote => "max:note",
    MinPriority => "min:priority",
    MaxPriority => "max:priority",
    SumPriority => "sum:priority",
    SumScore => "sum:score",
    EntryIdCount => "entry_id,count",
    EntryIdCountNote => "entry_id,count:note",
    EntryIdCountPriority => "entry_id,count:priority",
    EntryIdDistinctCountNote => "entry_id,distinct-count:note",
    EntryIdMinNote => "entry_id,min:note",
    EntryIdMaxNote => "entry_id,max:note",
    EntryIdMinPriority => "entry_id,min:priority",
    EntryIdMaxPriority => "entry_id,max:priority",
    EntryIdSumPriority => "entry_id,sum:priority",
    EntryIdSumScore => "entry_id,sum:score",
}}

selectors! { BinaryObjectSelector {
    Count => "count",
    CountPayload => "count:payload",
    CountOptional => "count:optional",
    DistinctCountPayload => "distinct-count:payload",
    DistinctCountOptional => "distinct-count:optional",
    MinPayload => "min:payload",
    MaxPayload => "max:payload",
    MinOptional => "min:optional",
    MaxOptional => "max:optional",
    PayloadCount => "payload,count",
    PayloadCountOptional => "payload,count:optional",
    OptionalCount => "optional,count",
    OptionalCountOptional => "optional,count:optional",
}}

enum RetainedQuery<'a> {
    Entry(EntrySelector, Query<'a, Entry>),
    Document(DocumentSelector, Query<'a, Document>),
    DecimalRow(DecimalRowSelector, Query<'a, DecimalRow>),
    BinaryObject(BinaryObjectSelector, Query<'a, BinaryObject>),
}

impl<'a> RetainedQuery<'a> {
    fn build(db: &'a crate::handles::Database, selection: &'a Selection) -> Result<Self, String> {
        let (minimum, category, entry_id) = selection.entity_filters;
        Ok(match selection.entity.as_str() {
            "Entry" => Self::Entry(
                EntrySelector::parse(&selection.select).map_err(|e| e.to_string())?,
                configured(entry_query(db, minimum, category, entry_id)?, selection)?,
            ),
            "Document" => Self::Document(
                DocumentSelector::parse(&selection.select).map_err(|e| e.to_string())?,
                configured(document_query(db, minimum, category, entry_id)?, selection)?,
            ),
            "DecimalRow" => Self::DecimalRow(
                DecimalRowSelector::parse(&selection.select).map_err(|e| e.to_string())?,
                configured(decimal_query(db, minimum, category, entry_id)?, selection)?,
            ),
            "BinaryObject" => Self::BinaryObject(
                BinaryObjectSelector::parse(&selection.select).map_err(|e| e.to_string())?,
                configured(blob::query(db, minimum, category, entry_id)?, selection)?,
            ),
            _ => return Err(Error::InvalidQuery.to_string()),
        })
    }
}

// Reconstructed on every call; product admission binds these semantics to the job.
fn configured<'a, E: Entity>(
    query: Query<'a, E>,
    selection: &'a Selection,
) -> Result<Query<'a, E>, String> {
    let Selection {
        select,
        limit,
        group_bounds: (group_min, group_max),
        group_order,
        filters,
        order: (order, descending),
        ..
    } = selection;
    if select == "distinct:amount" && *limit != 32 {
        return Err("use --page-size for retained DISTINCT".into());
    }
    let query = super::filter_rows(query, filters)?;
    let query = super::apply_order(query, &super::parse_order(order)?, *descending);

    select_groups(
        query.limit(*limit as usize),
        select,
        (group_min.as_deref(), group_max.as_deref()),
        parse_group_order(group_order.as_deref())?,
    )
}

fn message_time(time_ns: u64) -> Timestamp {
    Timestamp::from_millis((time_ns / 1_000_000) as i64)
}

pub(super) fn start_at(selection: &Selection, now: Timestamp) -> Result<Vec<u8>, String> {
    let host = runtime();
    let db = open(&host);
    let mut jobs = open_jobs(&host);
    let expires = Timestamp::from_millis(
        now.as_millis()
            .checked_add(60_000)
            .ok_or("clock overflow")?,
    );
    let result = match RetainedQuery::build(&db, selection)? {
        RetainedQuery::Entry(selector, query) => selector.start(query, &mut jobs, now, expires),
        RetainedQuery::Document(selector, query) => selector.start(query, &mut jobs, now, expires),
        RetainedQuery::DecimalRow(selector, query) => {
            selector.start(query, &mut jobs, now, expires)
        }
        RetainedQuery::BinaryObject(selector, query) => {
            selector.start(query, &mut jobs, now, expires)
        }
    };

    result
        .map(|handle| handle.to_bytes())
        .map_err(|e| e.to_string())
}

fn display<T>(step: JobStep<T>, convert: impl FnOnce(T) -> Output) -> Response {
    Response {
        sequence: step.sequence,
        examined: step.examined,
        input_bytes: step.input_bytes,
        output_bytes: step.output_bytes as u64,
        phase: match step.outcome {
            JobOutcome::Running => RetainedPhase::Running,
            JobOutcome::Complete(value) => RetainedPhase::Complete(convert(value)),
            JobOutcome::Expired => RetainedPhase::Expired,
            JobOutcome::Invalidated => RetainedPhase::Invalidated,
        },
    }
}

fn display_summary<T>(
    step: JobStep<GroupPage<T>>,
    key: &str,
    render: fn(T) -> Vec<String>,
) -> Response {
    display(step, |p| {
        let (columns, rows) = super::summary_table(p.rows, key, render);
        (columns, rows, p.next.map(|c| c.to_bytes()))
    })
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

pub(super) fn resume_at(
    RetainedResume {
        selection,
        handle,
        sequence,
        work,
        bytes,
        page_size,
        after,
    }: RetainedResume,
    now: Timestamp,
) -> Result<Response, String> {
    let host = runtime();
    let db = open(&host);
    let mut jobs = open_jobs(&host);
    let handle = JobHandle::from_bytes(&handle).map_err(|e| e.to_string())?;
    let cursor =
        crate::queries::decode_group_cursor(after.as_deref()).map_err(|e| e.to_string())?;
    let budgets = (work, bytes, page_size, cursor.as_ref());
    let result = match RetainedQuery::build(&db, &selection)? {
        RetainedQuery::Entry(selector, query) => selector.resume(
            resume_query(query, &selection.select, budgets)?,
            &mut jobs,
            &handle,
            sequence,
            now,
            cursor.as_ref(),
        ),
        RetainedQuery::Document(selector, query) => selector.resume(
            resume_query(query, &selection.select, budgets)?,
            &mut jobs,
            &handle,
            sequence,
            now,
        ),
        RetainedQuery::DecimalRow(selector, query) => selector.resume(
            resume_query(query, &selection.select, budgets)?,
            &mut jobs,
            &handle,
            sequence,
            now,
        ),
        RetainedQuery::BinaryObject(selector, query) => selector.resume(
            resume_query(query, &selection.select, budgets)?,
            &mut jobs,
            &handle,
            sequence,
            now,
        ),
    };

    result.map_err(|e| e.to_string())
}

fn resume_query<'a, E: Entity>(
    mut query: Query<'a, E>,
    select: &str,
    (work, bytes, page_size, cursor): (u32, u32, u32, Option<&'a GroupCursor>),
) -> Result<Query<'a, E>, String> {
    query = query
        .work_limit(work as usize)
        .test_output_limit(bytes as usize);
    if select.contains(',') {
        query = query.group_page(page_size as usize, cursor);
    } else if select == "distinct:amount" {
        query = query.limit(page_size as usize);
    } else if cursor.is_some() {
        return Err("scalar aggregates have no page cursor".into());
    }

    Ok(query)
}

impl EntrySelector {
    fn start(
        self,
        query: Query<'_, Entry>,
        jobs: &mut AggregateJobs,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        match self {
            Self::Count => query.start_count(jobs, now, expires),
            Self::CountAmount => query.start_count_field("amount", jobs, now, expires),
            Self::DistinctAmount => query.start_distinct::<AmountOnly>(jobs, now, expires),
            Self::DistinctCountAmount => {
                query.start_count_distinct::<AmountOnly>(jobs, now, expires)
            }
            Self::DistinctCountCategory => {
                query.start_count_distinct::<CategoryOnly>(jobs, now, expires)
            }
            Self::SumAmount => query.start_sum::<TotalAmount>(jobs, now, expires),
            Self::AvgAmount => query.start_avg::<AverageAmount>(jobs, now, expires),
            Self::MinAmount => query.start_min::<AmountOnly>(jobs, now, expires),
            Self::MaxAmount => query.start_max::<AmountOnly>(jobs, now, expires),
            Self::CategoryCount => query.start_group_count::<CategoryOnly>(jobs, now, expires),
            Self::CategoryCountAmount => {
                query.start_group_count_field::<CategoryOnly>("amount", jobs, now, expires)
            }
            Self::CategoryDistinctCountAmount => {
                query.start_group_count_distinct::<CategoryOnly, AmountOnly>(jobs, now, expires)
            }
            Self::CategoryDistinctCountCategory => {
                query.start_group_count_distinct::<CategoryOnly, CategoryOnly>(jobs, now, expires)
            }
            Self::CategorySummaryAmount => query
                .start_group_summary::<CategoryOnly, TotalAmount, AverageAmount>(
                    jobs, now, expires,
                ),
            Self::CategorySumAmount => {
                query.start_group_sum::<CategoryOnly, TotalAmount>(jobs, now, expires)
            }
            Self::CategoryAvgAmount => {
                query.start_group_avg::<CategoryOnly, AverageAmount>(jobs, now, expires)
            }
            Self::CategoryMinAmount => {
                query.start_group_min::<CategoryOnly, AmountOnly>(jobs, now, expires)
            }
            Self::CategoryMaxAmount => {
                query.start_group_max::<CategoryOnly, AmountOnly>(jobs, now, expires)
            }
        }
    }

    fn resume<'a>(
        self,
        query: Query<'a, Entry>,
        jobs: &mut AggregateJobs,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
        after: Option<&'a GroupCursor>,
    ) -> Result<Response, Error> {
        match self {
            Self::Count => query
                .resume_count(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::CountAmount => query
                .resume_count_field("amount", jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::DistinctAmount => query
                .resume_distinct::<AmountOnly>(jobs, handle, sequence, now, after)
                .map(|s| {
                    display(s, |p| {
                        (
                            vec!["amount".into()],
                            p.rows
                                .into_iter()
                                .map(|r| vec![r.amount.to_string()])
                                .collect(),
                            p.next.map(|c| c.to_bytes()),
                        )
                    })
                }),
            Self::DistinctCountAmount => query
                .resume_count_distinct::<AmountOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::DistinctCountCategory => query
                .resume_count_distinct::<CategoryOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::SumAmount => query
                .resume_sum::<TotalAmount>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("amount", v.map(|r| r.amount.to_string())))),
            Self::AvgAmount => query
                .resume_avg::<AverageAmount>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("amount", v.map(|r| r.amount.to_string())))),
            Self::MinAmount => query
                .resume_min::<AmountOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("amount", v.map(|r| r.amount.to_string())))),
            Self::MaxAmount => query
                .resume_max::<AmountOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("amount", v.map(|r| r.amount.to_string())))),
            Self::CategoryCount => query
                .resume_group_count::<CategoryOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |p| counts(p, "category", |k| k.category.to_string()))),
            Self::CategoryCountAmount => query
                .resume_group_count_field::<CategoryOnly>("amount", jobs, handle, sequence, now)
                .map(|s| display(s, |p| counts(p, "category", |k| k.category.to_string()))),
            Self::CategoryDistinctCountAmount => query
                .resume_group_count_distinct::<CategoryOnly, AmountOnly>(
                    jobs, handle, sequence, now,
                )
                .map(|s| display(s, |p| counts(p, "category", |k| k.category.to_string()))),
            Self::CategoryDistinctCountCategory => query
                .resume_group_count_distinct::<CategoryOnly, CategoryOnly>(
                    jobs, handle, sequence, now,
                )
                .map(|s| display(s, |p| counts(p, "category", |k| k.category.to_string()))),
            Self::CategorySummaryAmount => query
                .resume_group_summary::<CategoryOnly, TotalAmount, AverageAmount>(
                    jobs, handle, sequence, now,
                )
                .map(|s| display_summary(s, "category", super::entry_summary_row)),
            Self::CategorySumAmount => query
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
            Self::CategoryAvgAmount => query
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
            Self::CategoryMinAmount => query
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
            Self::CategoryMaxAmount => query
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
        }
    }
}

impl DocumentSelector {
    fn start(
        self,
        query: Query<'_, Document>,
        jobs: &mut AggregateJobs,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        match self {
            Self::Count => query.start_count(jobs, now, expires),
            Self::CountNote => query.start_count_field("note", jobs, now, expires),
            Self::CountPriority => query.start_count_field("priority", jobs, now, expires),
            Self::DistinctCountNote => query.start_count_distinct::<NoteOnly>(jobs, now, expires),
            Self::MinNote => query.start_min::<NoteOnly>(jobs, now, expires),
            Self::MaxNote => query.start_max::<NoteOnly>(jobs, now, expires),
            Self::MinPriority => query.start_min::<PriorityOnly>(jobs, now, expires),
            Self::MaxPriority => query.start_max::<PriorityOnly>(jobs, now, expires),
            Self::SumPriority => query.start_sum::<TotalPriority>(jobs, now, expires),
            Self::SumScore => query.start_sum::<TotalScore>(jobs, now, expires),
            Self::EntryIdCount => query.start_group_count::<EntryIdOnly>(jobs, now, expires),
            Self::EntryIdCountNote => {
                query.start_group_count_field::<EntryIdOnly>("note", jobs, now, expires)
            }
            Self::EntryIdCountPriority => {
                query.start_group_count_field::<EntryIdOnly>("priority", jobs, now, expires)
            }
            Self::EntryIdDistinctCountNote => {
                query.start_group_count_distinct::<EntryIdOnly, NoteOnly>(jobs, now, expires)
            }
            Self::EntryIdMinNote => {
                query.start_group_min::<EntryIdOnly, NoteOnly>(jobs, now, expires)
            }
            Self::EntryIdMaxNote => {
                query.start_group_max::<EntryIdOnly, NoteOnly>(jobs, now, expires)
            }
            Self::EntryIdMinPriority => {
                query.start_group_min::<EntryIdOnly, PriorityOnly>(jobs, now, expires)
            }
            Self::EntryIdMaxPriority => {
                query.start_group_max::<EntryIdOnly, PriorityOnly>(jobs, now, expires)
            }
            Self::EntryIdSumPriority => {
                query.start_group_sum::<EntryIdOnly, TotalPriority>(jobs, now, expires)
            }
            Self::EntryIdSumScore => {
                query.start_group_sum::<EntryIdOnly, TotalScore>(jobs, now, expires)
            }
        }
    }

    fn resume(
        self,
        query: Query<'_, Document>,
        jobs: &mut AggregateJobs,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<Response, Error> {
        match self {
            Self::Count => query
                .resume_count(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::CountNote => query
                .resume_count_field("note", jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::CountPriority => query
                .resume_count_field("priority", jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::DistinctCountNote => query
                .resume_count_distinct::<NoteOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::MinNote => query
                .resume_min::<NoteOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("note", v.map(|r| optional(r.note))))),
            Self::MaxNote => query
                .resume_max::<NoteOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("note", v.map(|r| optional(r.note))))),
            Self::MinPriority => query
                .resume_min::<PriorityOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("priority", v.map(|r| optional(r.priority))))),
            Self::MaxPriority => query
                .resume_max::<PriorityOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("priority", v.map(|r| optional(r.priority))))),
            Self::SumPriority => query
                .resume_sum::<TotalPriority>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("priority", v.map(|r| optional(r.priority))))),
            Self::SumScore => query
                .resume_sum::<TotalScore>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("score", v.map(|r| r.score.to_string())))),
            Self::EntryIdCount => query
                .resume_group_count::<EntryIdOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |p| counts(p, "entry_id", |k| k.entry_id.to_string()))),
            Self::EntryIdCountNote => query
                .resume_group_count_field::<EntryIdOnly>("note", jobs, handle, sequence, now)
                .map(|s| display(s, |p| counts(p, "entry_id", |k| k.entry_id.to_string()))),
            Self::EntryIdCountPriority => query
                .resume_group_count_field::<EntryIdOnly>("priority", jobs, handle, sequence, now)
                .map(|s| display(s, |p| counts(p, "entry_id", |k| k.entry_id.to_string()))),
            Self::EntryIdDistinctCountNote => query
                .resume_group_count_distinct::<EntryIdOnly, NoteOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |p| counts(p, "entry_id", |k| k.entry_id.to_string()))),
            Self::EntryIdMinNote => query
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
            Self::EntryIdMaxNote => query
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
            Self::EntryIdMinPriority => query
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
            Self::EntryIdMaxPriority => query
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
            Self::EntryIdSumPriority => query
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
            Self::EntryIdSumScore => query
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
        }
    }
}

impl DecimalRowSelector {
    fn start(
        self,
        query: Query<'_, DecimalRow>,
        jobs: &mut AggregateJobs,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        match self {
            Self::CountValue => query.start_count_field("value", jobs, now, expires),
            Self::DistinctCountValue => {
                query.start_count_distinct::<TotalDecimal>(jobs, now, expires)
            }
            Self::ZoneCountValue => {
                query.start_group_count_field::<ZoneOnly>("value", jobs, now, expires)
            }
            Self::ZoneDistinctCountValue => {
                query.start_group_count_distinct::<ZoneOnly, TotalDecimal>(jobs, now, expires)
            }
            Self::CountMaybe => query.start_count_field("maybe", jobs, now, expires),
            Self::DistinctCountMaybe => {
                query.start_count_distinct::<OptionalDecimal>(jobs, now, expires)
            }
            Self::ZoneCountMaybe => {
                query.start_group_count_field::<ZoneOnly>("maybe", jobs, now, expires)
            }
            Self::ZoneDistinctCountMaybe => {
                query.start_group_count_distinct::<ZoneOnly, OptionalDecimal>(jobs, now, expires)
            }
            Self::Count => query.start_count(jobs, now, expires),
            Self::ZoneCount => query.start_group_count::<ZoneOnly>(jobs, now, expires),
            Self::MinValue => query.start_min::<TotalDecimal>(jobs, now, expires),
            Self::ZoneMinValue => {
                query.start_group_min::<ZoneOnly, TotalDecimal>(jobs, now, expires)
            }
            Self::MaxValue => query.start_max::<TotalDecimal>(jobs, now, expires),
            Self::ZoneMaxValue => {
                query.start_group_max::<ZoneOnly, TotalDecimal>(jobs, now, expires)
            }
            Self::MinMaybe => query.start_min::<OptionalDecimal>(jobs, now, expires),
            Self::ZoneMinMaybe => {
                query.start_group_min::<ZoneOnly, OptionalDecimal>(jobs, now, expires)
            }
            Self::MaxMaybe => query.start_max::<OptionalDecimal>(jobs, now, expires),
            Self::ZoneMaxMaybe => {
                query.start_group_max::<ZoneOnly, OptionalDecimal>(jobs, now, expires)
            }
            Self::SumValue => query.start_sum::<TotalDecimal>(jobs, now, expires),
            Self::ZoneSummaryValue => query
                .start_group_summary::<ZoneOnly, TotalDecimal, TotalDecimal>(jobs, now, expires),
            Self::ZoneSummaryMaybe => query
                .start_group_summary::<ZoneOnly, OptionalDecimal, OptionalDecimal>(
                    jobs, now, expires,
                ),
            Self::ZoneSumValue => {
                query.start_group_sum::<ZoneOnly, TotalDecimal>(jobs, now, expires)
            }
            Self::AvgValue => query.start_avg::<TotalDecimal>(jobs, now, expires),
            Self::ZoneAvgValue => {
                query.start_group_avg::<ZoneOnly, TotalDecimal>(jobs, now, expires)
            }
            Self::SumMaybe => query.start_sum::<OptionalDecimal>(jobs, now, expires),
            Self::ZoneSumMaybe => {
                query.start_group_sum::<ZoneOnly, OptionalDecimal>(jobs, now, expires)
            }
            Self::AvgMaybe => query.start_avg::<OptionalDecimal>(jobs, now, expires),
            Self::ZoneAvgMaybe => {
                query.start_group_avg::<ZoneOnly, OptionalDecimal>(jobs, now, expires)
            }
        }
    }

    fn resume(
        self,
        query: Query<'_, DecimalRow>,
        jobs: &mut AggregateJobs,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<Response, Error> {
        match self {
            Self::ZoneSummaryValue => query
                .resume_group_summary::<ZoneOnly, TotalDecimal, TotalDecimal>(
                    jobs, handle, sequence, now,
                )
                .map(|s| display_summary(s, "zone", super::decimal_summary_row)),
            Self::ZoneSummaryMaybe => query
                .resume_group_summary::<ZoneOnly, OptionalDecimal, OptionalDecimal>(
                    jobs, handle, sequence, now,
                )
                .map(|s| display_summary(s, "zone", super::optional_summary_row)),
            Self::CountValue => query
                .resume_count_field("value", jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::DistinctCountValue => query
                .resume_count_distinct::<TotalDecimal>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::ZoneCountValue => query
                .resume_group_count_field::<ZoneOnly>("value", jobs, handle, sequence, now)
                .map(|s| display(s, |p| counts(p, "zone", |k| k.zone.to_string()))),
            Self::ZoneDistinctCountValue => query
                .resume_group_count_distinct::<ZoneOnly, TotalDecimal>(jobs, handle, sequence, now)
                .map(|s| display(s, |p| counts(p, "zone", |k| k.zone.to_string()))),
            Self::CountMaybe => query
                .resume_count_field("maybe", jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::DistinctCountMaybe => query
                .resume_count_distinct::<OptionalDecimal>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::ZoneCountMaybe => query
                .resume_group_count_field::<ZoneOnly>("maybe", jobs, handle, sequence, now)
                .map(|s| display(s, |p| counts(p, "zone", |k| k.zone.to_string()))),
            Self::ZoneDistinctCountMaybe => query
                .resume_group_count_distinct::<ZoneOnly, OptionalDecimal>(
                    jobs, handle, sequence, now,
                )
                .map(|s| display(s, |p| counts(p, "zone", |k| k.zone.to_string()))),
            Self::Count => query
                .resume_count(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::ZoneCount => query
                .resume_group_count::<ZoneOnly>(jobs, handle, sequence, now)
                .map(|s| display(s, |p| counts(p, "zone", |k| k.zone.to_string()))),
            Self::SumValue | Self::AvgValue | Self::MinValue | Self::MaxValue => {
                let result = match self {
                    Self::AvgValue => query.resume_avg::<TotalDecimal>(jobs, handle, sequence, now),
                    Self::MinValue => query.resume_min::<TotalDecimal>(jobs, handle, sequence, now),
                    Self::MaxValue => query.resume_max::<TotalDecimal>(jobs, handle, sequence, now),
                    _ => query.resume_sum::<TotalDecimal>(jobs, handle, sequence, now),
                };

                result.map(|s| display(s, |v| scalar("value", v.map(|r| r.value.to_string()))))
            }
            Self::ZoneSumValue | Self::ZoneAvgValue | Self::ZoneMinValue | Self::ZoneMaxValue => {
                let result = match self {
                    Self::ZoneAvgValue => query
                        .resume_group_avg::<ZoneOnly, TotalDecimal>(jobs, handle, sequence, now),
                    Self::ZoneMinValue => query
                        .resume_group_min::<ZoneOnly, TotalDecimal>(jobs, handle, sequence, now),
                    Self::ZoneMaxValue => query
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
            Self::SumMaybe | Self::AvgMaybe | Self::MinMaybe | Self::MaxMaybe => {
                let result = match self {
                    Self::AvgMaybe => {
                        query.resume_avg::<OptionalDecimal>(jobs, handle, sequence, now)
                    }
                    Self::MinMaybe => {
                        query.resume_min::<OptionalDecimal>(jobs, handle, sequence, now)
                    }
                    Self::MaxMaybe => {
                        query.resume_max::<OptionalDecimal>(jobs, handle, sequence, now)
                    }
                    _ => query.resume_sum::<OptionalDecimal>(jobs, handle, sequence, now),
                };

                result.map(|s| display(s, |v| scalar("maybe", v.map(|r| optional(r.maybe)))))
            }
            Self::ZoneSumMaybe | Self::ZoneAvgMaybe | Self::ZoneMinMaybe | Self::ZoneMaxMaybe => {
                let result = match self {
                    Self::ZoneAvgMaybe => query
                        .resume_group_avg::<ZoneOnly, OptionalDecimal>(jobs, handle, sequence, now),
                    Self::ZoneMinMaybe => query
                        .resume_group_min::<ZoneOnly, OptionalDecimal>(jobs, handle, sequence, now),
                    Self::ZoneMaxMaybe => query
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
        }
    }
}

impl BinaryObjectSelector {
    fn start(
        self,
        query: Query<'_, BinaryObject>,
        jobs: &mut AggregateJobs,
        now: Timestamp,
        expires: Timestamp,
    ) -> Result<JobHandle, Error> {
        match self {
            Self::Count => query.start_count(jobs, now, expires),
            Self::CountPayload | Self::CountOptional => query.start_count_field(
                if self == Self::CountPayload {
                    "payload"
                } else {
                    "optional"
                },
                jobs,
                now,
                expires,
            ),
            Self::DistinctCountPayload => query.start_count_distinct::<Payload>(jobs, now, expires),
            Self::DistinctCountOptional => {
                query.start_count_distinct::<Optional>(jobs, now, expires)
            }
            Self::MinPayload => query.start_min::<Payload>(jobs, now, expires),
            Self::MaxPayload => query.start_max::<Payload>(jobs, now, expires),
            Self::MinOptional => query.start_min::<Optional>(jobs, now, expires),
            Self::MaxOptional => query.start_max::<Optional>(jobs, now, expires),
            Self::PayloadCount => query.start_group_count::<Payload>(jobs, now, expires),
            Self::PayloadCountOptional => {
                query.start_group_count_field::<Payload>("optional", jobs, now, expires)
            }
            Self::OptionalCount => query.start_group_count::<Optional>(jobs, now, expires),
            Self::OptionalCountOptional => {
                query.start_group_count_field::<Optional>("optional", jobs, now, expires)
            }
        }
    }

    fn resume(
        self,
        query: Query<'_, BinaryObject>,
        jobs: &mut AggregateJobs,
        handle: &JobHandle,
        sequence: u64,
        now: Timestamp,
    ) -> Result<Response, Error> {
        match self {
            Self::Count => query
                .resume_count(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::CountPayload | Self::CountOptional => query
                .resume_count_field(
                    if self == Self::CountPayload {
                        "payload"
                    } else {
                        "optional"
                    },
                    jobs,
                    handle,
                    sequence,
                    now,
                )
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::DistinctCountPayload => query
                .resume_count_distinct::<Payload>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::DistinctCountOptional => query
                .resume_count_distinct::<Optional>(jobs, handle, sequence, now)
                .map(|s| display(s, |v| scalar("count", Some(v.to_string())))),
            Self::MinPayload | Self::MaxPayload => {
                let step = if self == Self::MinPayload {
                    query.resume_min::<Payload>(jobs, handle, sequence, now)
                } else {
                    query.resume_max::<Payload>(jobs, handle, sequence, now)
                };

                step.map(|s| display(s, |v| scalar("payload", v.map(|r| super::hex(&r.payload)))))
            }
            Self::MinOptional | Self::MaxOptional => {
                let step = if self == Self::MinOptional {
                    query.resume_min::<Optional>(jobs, handle, sequence, now)
                } else {
                    query.resume_max::<Optional>(jobs, handle, sequence, now)
                };

                step.map(|s| {
                    display(s, |v| {
                        scalar(
                            "optional",
                            v.and_then(|r| r.optional).map(|v| super::hex(&v)),
                        )
                    })
                })
            }
            Self::PayloadCount => query
                .resume_group_count::<Payload>(jobs, handle, sequence, now)
                .map(|s| display(s, |p| counts(p, "payload", |k| super::hex(&k.payload)))),
            Self::PayloadCountOptional => query
                .resume_group_count_field::<Payload>("optional", jobs, handle, sequence, now)
                .map(|s| display(s, |p| counts(p, "payload", |k| super::hex(&k.payload)))),
            Self::OptionalCount => query
                .resume_group_count::<Optional>(jobs, handle, sequence, now)
                .map(|s| {
                    display(s, |p| {
                        counts(p, "optional", |k| blob::optional_hex(k.optional))
                    })
                }),
            Self::OptionalCountOptional => query
                .resume_group_count_field::<Optional>("optional", jobs, handle, sequence, now)
                .map(|s| {
                    display(s, |p| {
                        counts(p, "optional", |k| blob::optional_hex(k.optional))
                    })
                }),
        }
    }
}
