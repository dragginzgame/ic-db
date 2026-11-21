//! Differential checks against SQLite for shared query semantics.

use ic_db::design::prelude::*;
use ic_db::{
    AggregateJobs, Batch, Comparison, Database, DatabaseSession, Error, GroupOrder, JobOutcome,
    QueryCursor,
};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory,
};
use rusqlite::{Connection, params};

///
/// Item
/// Exercises indexed access by `lane`, `rank`, `price` and `id` on rows keyed by `id`.
///

#[entity(
    store = crate::TestStore,
    pk(field = "id"),
    index(fields = ["lane", "rank"]),
    index(fields = ["lane", "price", "id"], unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "lane", value(item(prim = "Nat8"))),
        field(name = "rank", value(item(prim = "Nat8"))),
        field(name = "visible", value(item(prim = "Bool"))),
        field(name = "price", value(opt, item(prim = "Decimal", scale = 2)))
    )
)]
struct Item {}

///
/// Id
/// Carries the `Nat64` value of `id`.
///

#[record(fields(field(name = "id", value(item(prim = "Nat64")))))]
struct Id {}

///
/// Lane
/// Carries the `Nat8` value of `lane`.
///

#[record(fields(field(name = "lane", value(item(prim = "Nat8")))))]
struct Lane {}

///
/// Price
/// Carries an optional `Decimal` value of `price`.
///

#[record(fields(field(name = "price", value(opt, item(prim = "Decimal", scale = 2)))))]
struct Price {}

/// Projects the fixed lane together with the varying rank and price.
#[record(fields(
    field(name = "lane", value(item(prim = "Nat8"))),
    field(name = "rank", value(item(prim = "Nat8"))),
    field(name = "price", value(opt, item(prim = "Decimal", scale = 2)))
))]
struct RankedPrice {}

fn fixture() -> (Connection, MemoryRuntime<VectorMemory>) {
    let sql = Connection::open_in_memory().unwrap();
    sql.execute_batch("CREATE TABLE items(id INTEGER PRIMARY KEY, lane INTEGER, rank INTEGER, visible INTEGER, cents INTEGER)").unwrap();
    let mut host = MemoryRuntime::new(VectorMemory::default()).unwrap();
    host.bootstrap(
        &SealedDeclarationSnapshot::new(vec![
            MemoryRequest::new("test", "test.db.v1").unwrap(),
            MemoryRequest::new("test", "test.jobs.v1").unwrap(),
        ])
        .unwrap(),
        &AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap(),
        &GenericAllocationPolicy,
    )
    .unwrap();
    let mut db = Database::create(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Item::SCHEMA]).unwrap(),
        [1; 16],
    )
    .unwrap();
    AggregateJobs::create(
        host.open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
        [2; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (id, (lane, rank, visible, cents)) in [
        (0, 2, true, Some(124_i64)),
        (0, 1, false, None),
        (0, 2, true, Some(125)),
        (0, 1, false, None),
        (0, 3, false, None),
        (0, 3, false, None),
        (0, 4, true, None),
        (0, 2, true, None),
        (1, 2, true, Some(125)),
        (1, 1, false, Some(126)),
        (2, 1, true, None),
        (0, 3, false, None),
        (0, 3, false, None),
    ]
    .into_iter()
    .enumerate()
    {
        let id = id as u64 + 1;
        batch
            .insert(&Item {
                id,
                lane,
                rank,
                visible,
                price: cents.map(|n| Decimal::try_new(i128::from(n), 2).unwrap()),
            })
            .unwrap();
        sql.execute(
            "INSERT INTO items VALUES (?1,?2,?3,?4,?5)",
            params![i64::try_from(id).unwrap(), lane, rank, visible, cents],
        )
        .unwrap();
    }
    db.commit(batch).unwrap();

    (sql, host)
}

fn open(host: &MemoryRuntime<VectorMemory>) -> Database {
    Database::open(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Item::SCHEMA]).unwrap(),
    )
    .unwrap()
}

#[test]
fn every_compound_cursor_returns_the_remaining_sqlite_order() {
    let (sql, host) = fixture();
    let db = open(&host);
    for descending in [false, true] {
        let direction = if descending { "DESC" } else { "ASC" };
        let statement = format!(
            "SELECT id FROM items WHERE lane=0 AND visible=1 ORDER BY rank {direction}, id {direction}"
        );
        let expected = sql
            .prepare(&statement)
            .unwrap()
            .query_map([], |row| row.get::<_, u32>(0).map(u64::from))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for hinted in [false, true] {
            let page = |after: Option<&QueryCursor>, bytes| {
                let prefix = [0_u8.into()];
                let mut q = db
                    .query::<Item>()
                    .filter("lane", Comparison::Eq, 0_u8)
                    .order_by(&["rank", "id"])
                    .filter("visible", Comparison::Eq, true)
                    .test_output_limit(bytes)
                    .work_limit(2);
                if hinted {
                    q = q.using_index("lane,rank", &prefix);
                }
                if descending {
                    q = q.descending();
                }
                if let Some(cursor) = after {
                    q = q.after(cursor);
                }

                q.fetch_as::<Id>().unwrap()
            };
            let mut boundaries = vec![(None, 0)];
            let mut saw_empty = false;
            let mut saw_retry = false;
            for _ in 0..=20 {
                let (cursor, offset) = boundaries.last().unwrap();
                let p = page(cursor.as_ref(), 8);
                let ids: Vec<_> = p.rows.iter().map(|v| v.id).collect();
                assert!(expected[*offset..].starts_with(&ids));
                let end = offset + ids.len();
                let Some(next) = p.next else {
                    assert_eq!(end, expected.len());
                    break;
                };
                saw_empty |= ids.is_empty();
                // Same start and work budget, but room for two IDs. If the second
                // candidate qualifies, the narrow page must retry it on continuation.
                saw_retry |= ids.len() == 1 && page(cursor.as_ref(), 16).rows.len() == 2;
                boundaries.push((
                    Some(QueryCursor::from_bytes(&next.to_bytes()).unwrap()),
                    end,
                ));
            }
            assert!(saw_empty && saw_retry);
            for (mut cursor, offset) in boundaries {
                let mut suffix = Vec::new();
                for _ in 0..=20 {
                    let p = page(cursor.as_ref(), 8);
                    suffix.extend(p.rows.into_iter().map(|r| r.id));
                    cursor = p.next;
                    if cursor.is_none() {
                        break;
                    }
                }
                assert!(cursor.is_none());
                assert_eq!(suffix, expected[offset..]);
            }
        }
    }
}

#[test]
fn filtered_sort_finishes_only_after_its_complete_index_range() {
    let (sql, host) = fixture();
    let db = open(&host);
    let query = || {
        db.query::<Item>()
            .eq(Item::LANE, 1_u8)
            .order_by(&[Item::VISIBLE, Item::ID])
            .limit(1)
    };
    // ID 9 is visible; ID 10 sorts first. Both lie outside a two-row primary scan.
    let winner: u32 = sql
        .query_row(
            "SELECT id FROM items WHERE lane=1 ORDER BY visible,id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let page = query().work_limit(3).fetch_as::<Id>().unwrap();
    assert_eq!(
        page.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        [u64::from(winner)]
    );
    assert_eq!((page.examined, page.fetched), (2, 2));
    assert!(page.next.is_none());
    assert!(matches!(
        query().work_limit(1).fetch_as::<Id>(),
        Err(Error::Limit)
    ));
}

#[test]
fn live_page_and_filtered_total_share_one_read_boundary() {
    let (sql, host) = fixture();
    let session = DatabaseSession::new(open(&host));
    let expected = |after: Option<(u8, u32)>| {
        sql.prepare(
            "WITH visible AS (
                SELECT rank, id, COUNT(*) OVER () AS total FROM items WHERE lane=0 AND visible=1
             ) SELECT id, total FROM visible WHERE ?1 IS NULL OR (rank,id)>(?1,?2)
               ORDER BY rank,id LIMIT 2",
        )
        .unwrap()
        .query_map(params![after.map(|p| p.0), after.map(|p| p.1)], |r| {
            Ok((
                u64::from(r.get::<_, u32>(0)?),
                u64::from(r.get::<_, u32>(1)?),
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    let read_page = |db: &Database, after: Option<&QueryCursor>| {
        let matching = || {
            db.query::<Item>()
                .eq(Item::LANE, 0_u8)
                .eq(Item::VISIBLE, true)
        };
        let mut listing = matching()
            .order_by(&[Item::RANK, Item::ID])
            .limit(2)
            .work_limit(4);
        if let Some(cursor) = after {
            listing = listing.after(cursor);
        }
        let page = listing.fetch_as::<Id>().unwrap();
        assert!(matches!(
            matching().work_limit(4).count(),
            Err(Error::Limit)
        ));
        let total = matching().count().unwrap().value;
        (
            page.rows
                .into_iter()
                .map(|r| (r.id, total))
                .collect::<Vec<_>>(),
            page.next,
        )
    };

    let read = session.read().unwrap();
    let (first, cursor) = read_page(&read, None);
    assert_eq!(first, expected(None));
    assert_eq!(first, [(1, 4), (3, 4)]);
    let cursor = cursor.unwrap();
    let mut moved = read.get::<Item>(1).unwrap().unwrap();
    moved.rank = 3;
    let mut entered = read.get::<Item>(2).unwrap().unwrap();
    entered.visible = true;
    let writer = session.clone();
    assert!(matches!(writer.replace(&moved), Err(Error::BorrowConflict)));
    drop(read);

    let mut batch = Batch::new();
    batch.replace(&moved).unwrap();
    batch.replace(&entered).unwrap();
    writer.commit(batch).unwrap();
    sql.execute_batch(
        "BEGIN; UPDATE items SET rank=3 WHERE id=1;
        UPDATE items SET visible=1 WHERE id=2; COMMIT;",
    )
    .unwrap();
    let read = session.read().unwrap();
    let (continued, _) = read_page(&read, Some(&cursor));
    assert_eq!(continued, expected(Some((2, 3))));
    assert_eq!(continued, [(8, 5), (1, 5)]);
    let (fresh, _) = read_page(&read, None);
    assert_eq!(fresh, expected(None));
    assert_eq!(fresh, [(2, 5), (3, 5)]);
}

#[test]
fn complete_distinct_keeps_varying_components_of_a_partially_matching_index() {
    let (sql, host) = fixture();
    let db = open(&host);
    // The index fixes lane and orders rank. Prices can recur within a rank,
    // and NULL at two different ranks represents two different projected rows.
    let prefix = [0_u8.into()];
    let query = || db.query::<Item>().using_index("lane,rank", &prefix);
    let page = query()
        .order_asc(Item::RANK)
        .distinct()
        .fetch_as::<RankedPrice>()
        .unwrap();
    let expected = sql
        .prepare(
            "SELECT lane, rank, cents FROM items WHERE lane = 0
             GROUP BY lane, rank, cents ORDER BY rank, MIN(id)",
        )
        .unwrap()
        .query_map([], |row| {
            let cents = row.get::<_, Option<i64>>(2)?;
            Ok((
                row.get::<_, u8>(0)?,
                row.get::<_, u8>(1)?,
                cents.map(i128::from),
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        query().count_distinct::<RankedPrice>().unwrap().value,
        expected.len() as u64
    );
    // Lane is fixed by the index prefix and price by the current group.
    // Count the remaining ranks even in the group whose price is NULL.
    let mut counts = std::collections::BTreeMap::new();
    for &(_, _, price) in &expected {
        *counts.entry(price).or_insert(0_u64) += 1;
    }
    let groups = query()
        .group_count_distinct::<Price, RankedPrice>()
        .unwrap();
    assert_eq!(
        groups
            .value
            .into_iter()
            .map(|g| (
                g.key.price.map(|p| p.coefficient_at_scale(2).unwrap()),
                g.count
            ))
            .collect::<Vec<_>>(),
        counts.into_iter().collect::<Vec<_>>()
    );
    assert_eq!(
        page.rows
            .into_iter()
            .map(|r| (
                r.lane,
                r.rank,
                r.price.map(|p| p.coefficient_at_scale(2).unwrap())
            ))
            .collect::<Vec<_>>(),
        expected
    );
    assert!(page.next.is_none());
}

#[test]
fn ordered_distinct_matches_first_qualifying_occurrences_in_sqlite() {
    let (sql, host) = fixture();
    let db = open(&host);
    let mut jobs = AggregateJobs::open(
        host.open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let now = Timestamp::from_millis(0);
    for descending in [false, true] {
        let direction = if descending { "DESC" } else { "ASC" };
        // Rank only qualifying rows, then select the earliest rank for each price,
        // including NULL. This is independent of both DB traversal and key encoding.
        let statement = format!(
            "WITH matches AS (
                SELECT cents, ROW_NUMBER() OVER (ORDER BY rank {direction}, id {direction}) AS pos
                FROM items WHERE lane = 0 AND visible = 1
             ) SELECT cents FROM matches GROUP BY cents ORDER BY MIN(pos)"
        );
        let expected = sql
            .prepare(&statement)
            .unwrap()
            .query_map([], |row| Ok(row.get::<_, Option<i64>>(0)?.map(i128::from)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let query = || {
            let q = db
                .query::<Item>()
                .eq(Item::LANE, 0_u8)
                .eq(Item::VISIBLE, true)
                .order_by(&[Item::RANK, Item::ID]);
            if descending { q.descending() } else { q }
        };
        let cents = |row: Price| row.price.map(|p| p.coefficient_at_scale(2).unwrap());
        let page = query().distinct().fetch_as::<Price>().unwrap();
        assert!(page.next.is_none());
        assert_eq!(
            page.rows.into_iter().map(cents).collect::<Vec<_>>(),
            expected
        );

        let retained = || query().order_groups(GroupOrder::FirstOccurrence);
        let handle = retained()
            .start_distinct::<Price>(&mut jobs, now, Timestamp::from_millis(100))
            .unwrap();
        let mut cursor = None;
        let mut sequence = 0;
        let mut actual = Vec::new();
        for _ in 0..32 {
            let step = retained()
                .work_limit(1)
                .limit(1)
                .resume_distinct::<Price>(&mut jobs, &handle, sequence, now, cursor.as_ref())
                .unwrap();
            sequence = step.sequence;
            match step.outcome {
                JobOutcome::Running => assert_eq!(step.output_bytes, 0),
                JobOutcome::Complete(page) => {
                    assert_eq!(page.rows.len(), 1);
                    actual.extend(page.rows.into_iter().map(cents));
                    cursor = page.next;
                    if cursor.is_none() {
                        break;
                    }
                }
                other => panic!("unexpected DISTINCT outcome: {other:?}"),
            }
        }
        assert!(cursor.is_none());
        assert_eq!(actual, expected);
        jobs.cancel(&handle).unwrap();
    }
}

#[test]
fn grouped_decimal_averages_agree_across_reopen_and_work_limits() {
    let (sql, host) = fixture();
    // Integer cents keep SQLite's SUM and COUNT exact. These groups are
    // 1.245, 1.255 and absent; final scale two rounds the ties in opposite directions.
    let totals = sql
        .prepare("SELECT lane, SUM(cents), COUNT(cents) FROM items GROUP BY lane ORDER BY lane")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, u8>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(totals, [(0, Some(249), 2), (1, Some(251), 2), (2, None, 0)]);
    let expected = [(0, Some(124)), (1, Some(126)), (2, None)];
    let db = open(&host);
    let rows = db.query::<Item>().group_avg::<Lane, Price>().unwrap().value;
    let cents = |row: &ic_db::GroupValue<Lane, Price>| {
        (
            row.key.lane,
            row.value
                .as_ref()
                .and_then(|v| v.price)
                .map(|p| p.coefficient_at_scale(2).unwrap()),
        )
    };
    let sums = db.query::<Item>().group_sum::<Lane, Price>().unwrap().value;
    let counts = db
        .query::<Item>()
        .group_count_field::<Lane>("price")
        .unwrap()
        .value;
    assert_eq!(
        sums.iter().map(cents).collect::<Vec<_>>(),
        totals
            .iter()
            .map(|&(lane, sum, _)| (lane, sum.map(i128::from)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        counts
            .iter()
            .map(|group| (group.key.lane, group.count))
            .collect::<Vec<_>>(),
        totals
            .iter()
            .map(|&(lane, _, count)| (lane, u64::try_from(count).unwrap()))
            .collect::<Vec<_>>()
    );
    assert_eq!(rows.iter().map(cents).collect::<Vec<_>>(), expected);
    drop(db);
    let jobs_key = StableKey::parse("test.jobs.v1").unwrap();
    let now = Timestamp::from_millis(0);
    for work in [1, 3, 5] {
        let mut jobs = AggregateJobs::open(host.open_memory(&jobs_key).unwrap()).unwrap();
        let handle = open(&host)
            .query::<Item>()
            .start_group_avg::<Lane, Price>(&mut jobs, now, Timestamp::from_millis(100))
            .unwrap();
        drop(jobs);
        let mut sequence = 0;
        let mut completed = false;
        for _ in 0..=16 {
            let mut jobs = AggregateJobs::open(host.open_memory(&jobs_key).unwrap()).unwrap();
            let step = open(&host)
                .query::<Item>()
                .work_limit(work)
                .resume_group_avg::<Lane, Price>(&mut jobs, &handle, sequence, now)
                .unwrap();
            sequence = step.sequence;

            match step.outcome {
                JobOutcome::Running => assert_eq!(step.output_bytes, 0),
                JobOutcome::Complete(rows) => {
                    assert_eq!(rows.rows.iter().map(cents).collect::<Vec<_>>(), expected);
                    jobs.cancel(&handle).unwrap();
                    completed = true;
                    break;
                }
                _ => panic!("unexpected aggregate outcome"),
            }
        }
        assert!(completed);
    }
}

#[test]
fn ranked_group_summaries_match_sqlite_in_one_call_and_across_pages() {
    use ic_db::{SortDirection, SummaryMeasure};
    let (sql, host) = fixture();
    let mut db = open(&host);
    // Lane 3 has a larger total than lane 1 but ties lane 0 after rounding.
    // Lane 4 exercises negative half-even rounding.
    let mut batch = Batch::new();
    for (offset, (lane, cents)) in [
        (3u8, 125i64),
        (3, 124),
        (3, 124),
        (3, 124),
        (4, -125),
        (4, -126),
    ]
    .into_iter()
    .enumerate()
    {
        let id = 20 + offset as u64;
        batch
            .insert(&Item {
                id,
                lane,
                rank: 0,
                visible: true,
                price: Decimal::try_new(i128::from(cents), 2),
            })
            .unwrap();
        sql.execute(
            "INSERT INTO items VALUES (?1,?2,0,1,?3)",
            params![i64::try_from(id).unwrap(), lane, cents],
        )
        .unwrap();
    }
    db.commit(batch).unwrap();
    let mut jobs = AggregateJobs::open(
        host.open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let now = Timestamp::from_millis(0);
    let values = |g: ic_db::GroupSummary<Lane, Price, Price>| {
        let cents = |r: Option<Price>| {
            r.and_then(|r| r.price)
                .map(|p| p.coefficient_at_scale(2).unwrap())
        };
        (g.key.lane, g.count, cents(g.sum), cents(g.average))
    };
    for (measure, column) in [
        (SummaryMeasure::Count, "n"),
        (SummaryMeasure::Sum, "total"),
        (SummaryMeasure::Average, "mean"),
    ] {
        for (direction, keyword) in [
            (SortDirection::Ascending, "ASC"),
            (SortDirection::Descending, "DESC"),
        ] {
            for (filtered, limit) in [false, true]
                .into_iter()
                .flat_map(|filtered| (1..=5).map(move |limit| (filtered, limit)))
            {
                let condition = if filtered {
                    "WHERE n >= 2 AND total >= 249 AND mean <= 124"
                } else {
                    ""
                };
                // SQLite's integer quotient truncates toward zero. Apply the declared
                // ties-to-even rule in integer cents before SQLite orders the groups.
                let stmt = format!(
                "WITH sums AS (
                SELECT lane, COUNT(cents) AS n, SUM(cents) AS total FROM items GROUP BY lane
            ), parts AS (
                SELECT *, total/n AS q, 2*abs(total%n) AS twice_remainder FROM sums
            ), summaries AS (SELECT lane,n,total,q + CASE
                WHEN twice_remainder > n OR (twice_remainder = n AND q%2 != 0)
                THEN CASE WHEN total < 0 THEN -1 ELSE 1 END ELSE 0 END AS mean
              FROM parts) SELECT lane,n,total,mean FROM summaries {condition} ORDER BY {column} {keyword}, lane ASC LIMIT {limit}"
            );
                let expected = sql
                    .prepare(&stmt)
                    .unwrap()
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, u8>(0)?,
                            u64::try_from(row.get::<_, i64>(1)?).unwrap(),
                            row.get::<_, Option<i64>>(2)?.map(i128::from),
                            row.get::<_, Option<i64>>(3)?.map(i128::from),
                        ))
                    })
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                let build = |last_measure| {
                    let q = db
                        .query::<Item>()
                        .order_groups(GroupOrder::Summary(measure, direction))
                        .limit(limit);
                    if filtered {
                        q.filter_summary(SummaryMeasure::Count, Comparison::Ge, 2_u64)
                            .filter_summary(
                                SummaryMeasure::Sum,
                                Comparison::Ge,
                                Decimal::try_new(249, 2).unwrap(),
                            )
                            .filter_summary(
                                last_measure,
                                Comparison::Le,
                                Decimal::try_new(124, 2).unwrap(),
                            )
                    } else {
                        q
                    }
                };
                let query = || build(SummaryMeasure::Average);
                let sync = query().group_summary::<Lane, Price, Price>().unwrap();
                assert_eq!(
                    sync.value.into_iter().map(values).collect::<Vec<_>>(),
                    expected
                );
                let handle = query()
                    .start_group_summary::<Lane, Price, Price>(
                        &mut jobs,
                        now,
                        Timestamp::from_millis(100),
                    )
                    .unwrap();
                if filtered {
                    assert!(matches!(
                        build(SummaryMeasure::Sum)
                            .resume_group_summary::<Lane, Price, Price>(&mut jobs, &handle, 0, now),
                        Err(ic_db::Error::InvalidJob)
                    ));
                }
                let mut sequence = 0;
                let mut cursor = None;
                let mut actual = Vec::new();
                for _ in 0..16 {
                    let step = query()
                        .work_limit(3)
                        .group_page(1, cursor.as_ref())
                        .resume_group_summary::<Lane, Price, Price>(
                            &mut jobs, &handle, sequence, now,
                        )
                        .unwrap();
                    sequence = step.sequence;
                    match step.outcome {
                        JobOutcome::Running => continue,
                        JobOutcome::Complete(page) => {
                            assert_eq!(page.rows.len(), 1);
                            actual.extend(page.rows.into_iter().map(values));
                            cursor = page.next;
                            if cursor.is_none() {
                                break;
                            }
                        }
                        _ => panic!("unexpected summary outcome"),
                    }
                }
                assert!(cursor.is_none());
                assert_eq!(actual, expected);
                jobs.cancel(&handle).unwrap();
            }
        }
    }
}

#[test]
fn summary_alternatives_match_sqlite_and_bind_retained_clause_boundaries() {
    use ic_db::{
        IndexValue,
        SummaryMeasure::{Count, Sum},
    };
    let (sql, host) = fixture();
    let expected = sql
        .prepare(
            "SELECT lane FROM items GROUP BY lane HAVING (COUNT(cents)=0 OR SUM(cents)>=250)
         AND COUNT(cents)<=2 ORDER BY lane",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, u8>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(expected, [1, 2]);
    let db = open(&host);
    let alternatives = [
        (Count, Comparison::Eq, 0_u64.into()),
        (
            Sum,
            Comparison::Ge,
            Decimal::try_new(250, 2).unwrap().into(),
        ),
    ];
    let query = || {
        db.query::<Item>()
            .filter_summary_any(&alternatives)
            .filter_summary(Count, Comparison::Le, 2_u64)
    };
    let rows = query().group_summary::<Lane, Price, Price>().unwrap().value;
    assert_eq!(
        rows.iter().map(|g| g.key.lane).collect::<Vec<_>>(),
        expected
    );

    let mut jobs = AggregateJobs::open(
        host.open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let now = Timestamp::from_millis(0);
    let handle = query()
        .start_group_summary::<Lane, Price, Price>(&mut jobs, now, Timestamp::from_millis(100))
        .unwrap();
    // Same measures, comparisons and operands, but three AND clauses.
    let mut changed = db.query::<Item>();
    for (measure, comparison, value) in alternatives {
        changed = changed.filter_summary(measure, comparison, value);
    }
    assert!(matches!(
        changed
            .filter_summary(Count, Comparison::Le, 2_u64)
            .resume_group_summary::<Lane, Price, Price>(&mut jobs, &handle, 0, now),
        Err(ic_db::Error::InvalidJob)
    ));
    let step = query()
        .resume_group_summary::<Lane, Price, Price>(&mut jobs, &handle, 0, now)
        .unwrap();
    let JobOutcome::Complete(page) = step.outcome else {
        panic!("expected complete groups")
    };
    assert_eq!(
        page.rows.iter().map(|g| g.key.lane).collect::<Vec<_>>(),
        expected
    );
    for invalid in [
        db.query::<Item>().filter_summary_any(&[]),
        db.query::<Item>()
            .filter_summary(Count, Comparison::Ge, 0_u64)
            .filter_summary_any(&[(Count, Comparison::Eq, IndexValue::Nat64(0)); 8]),
    ] {
        assert!(matches!(
            invalid.group_summary::<Lane, Price, Price>(),
            Err(ic_db::Error::Limit)
        ));
    }
}

#[test]
fn moving_a_row_between_groups_preserves_completed_summary_pages_only() {
    use ic_db::{GroupSummary, SortDirection, SummaryMeasure};

    let (sql, host) = fixture();
    let expected = || {
        sql.prepare(
            "SELECT lane, COUNT(cents), SUM(cents) FROM items
             GROUP BY lane ORDER BY SUM(cents) DESC, lane ASC LIMIT 2",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, u8>(0)?,
                u64::from(row.get::<_, u32>(1)?),
                row.get::<_, Option<i64>>(2)?.map(i128::from),
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    let values = |row: GroupSummary<Lane, Price, Price>| {
        (
            row.key.lane,
            row.count,
            row.sum
                .and_then(|p| p.price)
                .and_then(|p| p.coefficient_at_scale(2)),
        )
    };
    let order = GroupOrder::Summary(SummaryMeasure::Sum, SortDirection::Descending);
    let mut db = open(&host);
    let mut jobs = AggregateJobs::open(
        host.open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let now = Timestamp::from_millis(0);
    let start = |jobs: &mut AggregateJobs| {
        db.query::<Item>()
            .order_groups(order)
            .limit(2)
            .start_group_summary::<Lane, Price, Price>(jobs, now, Timestamp::from_millis(100))
            .unwrap()
    };
    let completed = start(&mut jobs);
    let running = start(&mut jobs);
    let progress = db
        .query::<Item>()
        .order_groups(order)
        .limit(2)
        .work_limit(1)
        .resume_group_summary::<Lane, Price, Price>(&mut jobs, &running, 0, now)
        .unwrap();
    assert!(matches!(progress.outcome, JobOutcome::Running));

    let before = expected();
    let first = db
        .query::<Item>()
        .order_groups(order)
        .limit(2)
        .group_page(1, None)
        .resume_group_summary::<Lane, Price, Price>(&mut jobs, &completed, 0, now)
        .unwrap();
    let JobOutcome::Complete(page) = first.outcome else {
        panic!("expected completed summary")
    };
    assert_eq!(
        page.rows.into_iter().map(values).collect::<Vec<_>>(),
        before[..1]
    );
    let cursor = page.next.unwrap();

    let mut moved = db.get::<Item>(10).unwrap().unwrap();
    moved.lane = 2;
    let mut batch = Batch::new();
    batch.replace(&moved).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(
        sql.execute("UPDATE items SET lane=2 WHERE id=10", [])
            .unwrap(),
        1
    );

    let invalidated = db
        .query::<Item>()
        .order_groups(order)
        .limit(2)
        .resume_group_summary::<Lane, Price, Price>(&mut jobs, &running, progress.sequence, now)
        .unwrap();
    assert!(matches!(invalidated.outcome, JobOutcome::Invalidated));
    let rest = db
        .query::<Item>()
        .order_groups(order)
        .limit(2)
        .group_page(2, Some(&cursor))
        .resume_group_summary::<Lane, Price, Price>(&mut jobs, &completed, first.sequence, now)
        .unwrap();
    let JobOutcome::Complete(page) = rest.outcome else {
        panic!("expected remaining groups")
    };
    assert!(page.next.is_none());
    assert_eq!(
        page.rows.into_iter().map(values).collect::<Vec<_>>(),
        before[1..]
    );

    let after = expected();
    assert_eq!([before[0].0, before[1].0], [1, 0]);
    assert_eq!([after[0].0, after[1].0], [0, 2]);
    let fresh = db
        .query::<Item>()
        .order_groups(order)
        .limit(2)
        .group_summary::<Lane, Price, Price>()
        .unwrap();
    assert_eq!(
        fresh.value.into_iter().map(values).collect::<Vec<_>>(),
        after
    );
}

#[test]
fn automatic_counts_follow_committed_index_membership_across_reopen() {
    let (sql, host) = fixture();
    let mut db = open(&host);
    let price = Decimal::try_new(125, 2).unwrap();
    let check =
        |db: &Database| {
            let total: u32 = sql
                .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
                .unwrap();
            assert_eq!(
                db.query::<Item>().work_limit(1).count().unwrap().value,
                u64::from(total)
            );
            for lane in [0_u8, 1, 2, 3] {
                let (all, priced, absent): (u32, u32, u32) = sql
                    .query_row(
                        "SELECT COUNT(*), COUNT(*) FILTER (WHERE cents=125),
                 COUNT(*) FILTER (WHERE cents IS NULL) FROM items WHERE lane=?1",
                        [lane],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .unwrap();
                let query = || db.query::<Item>().filter("lane", Comparison::Eq, lane);
                let prefix = query().count().unwrap();
                assert_eq!(
                    (prefix.value, prefix.fetched),
                    (u64::from(all), 0),
                    "lane {lane}"
                );
                let sparse = query()
                    .filter("price", Comparison::Eq, price)
                    .count()
                    .unwrap();
                assert_eq!(
                    (sparse.value, sparse.fetched),
                    (u64::from(priced), 0),
                    "priced lane {lane}"
                );
                // A lost entry and a stale entry in the same prefix can cancel in the total.
                for id in [2_u32, 3, 9, 14, 15] {
                    let present: bool = sql.query_row(
                    "SELECT EXISTS(SELECT 1 FROM items WHERE lane=?1 AND cents=125 AND id=?2)",
                    params![lane, id], |r| r.get(0),
                ).unwrap();
                    let member = query()
                        .filter("price", Comparison::Eq, price)
                        .filter("id", Comparison::Eq, u64::from(id))
                        .count()
                        .unwrap();
                    assert_eq!(
                        (member.value, member.fetched),
                        (u64::from(present), 0),
                        "lane {lane}, id {id}"
                    );
                }
                assert_eq!(
                    query().filter_absent("price").count().unwrap().value,
                    u64::from(absent),
                    "absent lane {lane}"
                );
            }
        };
    check(&db);

    let mut batch = Batch::new();
    let mut moved = db.get::<Item>(3).unwrap().unwrap();
    moved.lane = 1;
    moved.price = None;
    batch.replace(&moved).unwrap();
    let mut priced = db.get::<Item>(2).unwrap().unwrap();
    priced.price = Some(price);
    batch.replace(&priced).unwrap();
    batch.delete::<Item>(9).unwrap();
    for id in [14, 15] {
        batch
            .insert(&Item {
                id,
                lane: 2,
                rank: 1,
                visible: true,
                price: Some(price),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    sql.execute_batch(
        "BEGIN;
        UPDATE items SET lane=1, cents=NULL WHERE id=3;
        UPDATE items SET cents=125 WHERE id=2;
        DELETE FROM items WHERE id=9;
        INSERT INTO items VALUES (14,2,1,1,125),(15,2,1,1,125);
        COMMIT;",
    )
    .unwrap();
    check(&db);

    // The missing target refuses the whole batch. SQLite still holds the last commit.
    let mut refused = Batch::new();
    refused.delete::<Item>(14).unwrap();
    refused.delete::<Item>(99).unwrap();
    assert!(matches!(db.commit(refused), Err(ic_db::Error::Missing)));
    check(&db);
    drop(db);
    check(&open(&host));
}

/// Logical owner of the TestStore allocation.
#[canister]
pub struct TestCanister {}

/// Store using the existing fixture.records.v1 allocation key.
#[store(canister = TestCanister, key = "fixture.records.v1")]
pub struct TestStore {}
