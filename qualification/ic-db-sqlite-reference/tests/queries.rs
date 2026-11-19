//! Differential checks against SQLite for shared query semantics.

use ic_db::{
    AggregateJobs, Batch, Comparison, Database, Decimal, Entity, JobOutcome, QueryCursor, Schema,
    Timestamp, entity, record,
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
