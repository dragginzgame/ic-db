//! Compare grouped Duration arithmetic and result selection against integer milliseconds in SQLite.

use ic_db::design::prelude::*;
use ic_db::{Batch, Comparison, GroupOrder, SortDirection, SummaryMeasure, testing::Fixture};
use rusqlite::Connection;

/// One observation, including measurements that did not report a duration.
#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "lane", value(item(prim = "Nat8"))),
        field(name = "elapsed", value(opt, item(prim = "Duration")))
    )
)]
struct Sample {}

/// Partition for summary results.
#[record(fields(field(name = "lane", value(item(prim = "Nat8")))))]
struct Lane {}

/// Duration operand and result in milliseconds.
#[record(fields(field(name = "elapsed", value(opt, item(prim = "Duration")))))]
struct Elapsed {}

#[test]
fn duration_summaries_filter_and_order_by_completed_milliseconds() {
    let fixture = Fixture::new("test.durations.v1");
    let mut db = fixture
        .create(Schema::new(&[Sample::SCHEMA]).unwrap(), [1; 16])
        .unwrap();
    let sql = Connection::open_in_memory().unwrap();
    sql.execute("CREATE TABLE samples(lane INTEGER, ms INTEGER)", [])
        .unwrap();
    let mut batch = Batch::new();
    for (id, (lane, ms)) in [
        (0_u8, None),
        (1, Some(0_i64)),
        (2, Some(1)),
        (2, Some(2)),
        (2, None),
        (3, Some(2)),
        (3, Some(3)),
        (4, Some((1 << 53) + 1)),
        (4, Some((1 << 53) + 2)),
    ]
    .into_iter()
    .enumerate()
    {
        sql.execute("INSERT INTO samples VALUES (?1, ?2)", (lane, ms))
            .unwrap();
        batch
            .insert(&Sample {
                id: id as u64,
                lane,
                elapsed: ms.map(|n| Duration::from_millis(n.try_into().unwrap())),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();

    // Integer quotient and remainder preserve the half-millisecond above 2^53.
    // A NULL threshold includes the all-absent group.
    let mut reference = sql
        .prepare(
            "WITH totals AS (
            SELECT lane, COUNT(ms) AS n, SUM(ms) AS total FROM samples GROUP BY lane
         ), parts AS (
            SELECT *, total/n AS q, 2*(total%n) AS r FROM totals
         ), rounded AS (
            SELECT *, q + (r > n OR (r = n AND q%2 = 1)) AS mean FROM parts
         )
         SELECT lane, n, total, mean FROM rounded WHERE ?1 IS NULL OR mean >= ?1
         ORDER BY mean DESC, lane ASC",
        )
        .unwrap();
    for threshold in [None, Some(2_i64), Some((1 << 53) + 2)] {
        let expected = reference
            .query_map([threshold], |row| {
                Ok((
                    row.get::<_, u8>(0)?,
                    u64::try_from(row.get::<_, i64>(1)?).unwrap(),
                    row.get::<_, Option<i64>>(2)?
                        .map(|ms| u64::try_from(ms).unwrap()),
                    row.get::<_, Option<i64>>(3)?
                        .map(|ms| u64::try_from(ms).unwrap()),
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut query = db.query::<Sample>().order_groups(GroupOrder::Summary(
            SummaryMeasure::Average,
            SortDirection::Descending,
        ));
        if let Some(ms) = threshold {
            query = query.filter_summary(
                SummaryMeasure::Average,
                Comparison::Ge,
                Duration::from_millis(ms.try_into().unwrap()),
            );
        }
        let results = query.group_summary::<Lane, Elapsed, Elapsed>().unwrap();
        let millis =
            |value: Option<Elapsed>| value.and_then(|r| r.elapsed).map(Duration::as_millis);
        let actual: Vec<_> = results
            .value
            .into_iter()
            .map(|r| (r.key.lane, r.count, millis(r.sum), millis(r.average)))
            .collect();
        assert_eq!(actual, expected);
    }
}
