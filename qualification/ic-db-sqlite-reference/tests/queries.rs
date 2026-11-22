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
        // Each predicate shape fixes the Nat8 lane to zero.
        use Comparison::{Eq, Ge, Le};
        for (hinted, bounds) in [
            (false, &[Eq][..]),
            (true, &[Eq][..]),
            (false, &[Ge, Le][..]),
            (false, &[Le][..]),
        ] {
            let page = |after: Option<&QueryCursor>, bytes| {
                let prefix = [0_u8.into()];
                let mut q = db
                    .query::<Item>()
                    .order_by(&["rank", "id"])
                    .filter("visible", Comparison::Eq, true)
                    .test_output_limit(bytes)
                    .work_limit(2);
                for &comparison in bounds {
                    q = q.filter(Item::LANE, comparison, 0_u8);
                }
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

// These indexed fixtures allow one candidate per call. Bound the retries so
// an empty page that repeats its cursor cannot hang the comparison.
fn assert_id_pages(expected: &[u64], fetch: impl Fn(Option<&QueryCursor>) -> ic_db::QueryPage<Id>) {
    let mut cursor = None;
    let mut seen = Vec::new();
    for _ in 0..=expected.len() {
        let page = fetch(cursor.as_ref());
        assert!(page.examined <= 1 && page.rows.len() <= 1);
        seen.extend(page.rows.into_iter().map(|row| row.id));
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert!(cursor.is_none());
    assert_eq!(seen, expected);
}

#[test]
fn a_fixed_rank_seeks_the_matching_lane_and_rank() {
    let (sql, host) = fixture();
    let db = open(&host);
    let expected = sql
        .prepare("SELECT id FROM items WHERE rank=1 AND lane=rank ORDER BY id DESC")
        .unwrap()
        .query_map([], |r| r.get::<_, u32>(0).map(u64::from))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let lane = [1_u8.into()];
    for (left, right) in [(Item::RANK, Item::LANE), (Item::LANE, Item::RANK)] {
        for hinted in [false, true] {
            assert_id_pages(&expected, |after| {
                let q = db.query::<Item>();
                let q = if hinted {
                    q.using_index("lane,rank", &lane)
                } else {
                    q.eq(Item::RANK, 1_u8)
                };
                let mut q = q
                    .filter_fields(left, Comparison::Eq, right)
                    .order_desc(Item::ID)
                    .limit(1)
                    .work_limit(1);
                if let Some(after) = after {
                    q = q.after(after);
                }
                let page = q.fetch_as::<Id>().unwrap();
                assert_eq!(page.examined, page.rows.len());
                page
            });
        }
    }
    // IDs 9 and 10 share the lane. Only ID 10 has the requested rank.
    for id in [9_u32, 10, 99] {
        let expected: u32 = sql
            .query_row(
                "SELECT COUNT(*) FROM items WHERE lane=1 AND lane=rank AND id=?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        let query = || {
            db.query::<Item>()
                .using_index("lane,rank", &lane)
                .filter_fields(Item::LANE, Comparison::Eq, Item::RANK)
                .eq(Item::ID, u64::from(id))
                .work_limit(1)
        };
        let page = query().fetch_as::<Id>().unwrap();
        assert_eq!(page.examined, expected as usize);
        assert_eq!(page.rows.len(), expected as usize);
        assert!(page.next.is_none());
        let count = query().count_validated().unwrap();
        assert_eq!(
            (count.value, count.examined),
            (u64::from(expected), expected as usize)
        );
    }
    // Inside an OR the equality cannot restrict the index prefix.
    let mixed = db
        .query::<Item>()
        .eq(Item::RANK, 1_u8)
        .filter_any_operands(&[
            (
                Item::LANE,
                Comparison::Eq,
                ic_db::QueryOperand::Field(Item::RANK),
            ),
            (
                Item::VISIBLE,
                Comparison::Eq,
                ic_db::QueryOperand::Literal(false.into()),
            ),
        ])
        .order_desc(Item::ID)
        .fetch_as::<Id>()
        .unwrap();
    let expected = sql
        .prepare(
            "SELECT id FROM items WHERE rank=1 AND (lane=rank OR NOT visible) ORDER BY id DESC",
        )
        .unwrap()
        .query_map([], |r| r.get::<_, u32>(0).map(u64::from))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        mixed.rows.into_iter().map(|row| row.id).collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn integer_bounds_fix_one_lane_for_ordered_pages() {
    use Comparison::{Ge, Gt, Le, Lt};
    let (sql, host) = fixture();
    let db = open(&host);
    for (lower, lo, upper, hi) in [(Ge, 1_u8, Lt, 2_u8), (Gt, 0, Le, 1), (Gt, 0, Lt, 2)] {
        let statement = format!(
            "SELECT id FROM items WHERE lane {} ?1 AND lane {} ?2 ORDER BY rank DESC, id DESC",
            if lower == Ge { ">=" } else { ">" },
            if upper == Le { "<=" } else { "<" }
        );
        let expected = sql
            .prepare(&statement)
            .unwrap()
            .query_map(params![lo, hi], |r| r.get::<_, u32>(0).map(u64::from))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_id_pages(&expected, |continuation| {
            let mut q = db
                .query::<Item>()
                .filter(Item::LANE, upper, hi)
                .filter(Item::LANE, lower, lo)
                .order_desc(Item::RANK)
                .order_desc(Item::ID)
                .work_limit(1)
                .limit(1);
            if let Some(after) = continuation {
                q = q.after(after);
            }
            q.fetch_as::<Id>().unwrap()
        });
    }
    // An interval containing lanes 1 and 2 cannot deliver global rank order.
    assert!(matches!(
        db.query::<Item>()
            .using_index("lane,rank", &[])
            .gt(Item::LANE, 0_u8)
            .lt(Item::LANE, 3_u8)
            .order_asc(Item::RANK)
            .fetch(),
        Err(Error::InvalidQuery)
    ));
}

#[test]
fn membership_intersections_and_exclusions_leave_one_lane_for_index_order() {
    let (sql, host) = fixture();
    let db = open(&host);
    let members = [0_u8, 1, 1, 2];
    for (include, others, clause) in [
        (true, [1_u8, 3], "lane IN (1,3)"),
        (false, [0_u8, 2], "lane NOT IN (0,2)"),
    ] {
        let expected = sql.prepare(&format!("SELECT id FROM items WHERE lane IN (0,1,1,2) AND {clause} ORDER BY rank DESC, id DESC"))
            .unwrap().query_map([], |row| row.get::<_, u32>(0).map(u64::from)).unwrap()
            .collect::<Result<Vec<_>, _>>().unwrap();
        for reverse in [false, true] {
            assert_id_pages(&expected, |continuation| {
                let mut q = db.query::<Item>();
                if !reverse {
                    q = q.filter_in(Item::LANE, &members);
                }
                q = if include {
                    q.filter_in(Item::LANE, &others)
                } else {
                    q.filter_not_in(Item::LANE, &others)
                };
                if reverse {
                    q = q.filter_in(Item::LANE, &members);
                }
                q = q
                    .order_desc(Item::RANK)
                    .order_desc(Item::ID)
                    .limit(1)
                    .work_limit(1);
                if let Some(after) = continuation {
                    q = q.after(after);
                }
                q.fetch_as::<Id>().unwrap()
            });
        }
    }
    let empty = db
        .query::<Item>()
        .filter_in(Item::LANE, &[0_u8, 1])
        .filter_not_in(Item::LANE, &[0_u8, 1])
        .work_limit(1)
        .fetch()
        .unwrap();
    assert!(empty.rows.is_empty() && empty.next.is_none());
    assert_eq!(empty.examined, 0);

    let disjoint = db
        .query::<Item>()
        .filter_in(Item::LANE, &[0_u8, 1])
        .filter_in(Item::LANE, &[2_u8, 3])
        .work_limit(1)
        .fetch()
        .unwrap();
    assert!(disjoint.rows.is_empty() && disjoint.next.is_none());
    assert_eq!(disjoint.examined, 0);

    let hinted = || {
        db.query::<Item>()
            .using_index("lane,rank", &[])
            .filter_in(Item::LANE, &[0_u8, 1])
            .order_asc(Item::RANK)
    };
    for q in [
        hinted().filter_in(Item::LANE, &[1_u8, 2, 0]),
        hinted().filter_any(&[
            (Item::LANE, Comparison::Eq, 1_u8.into()),
            (Item::VISIBLE, Comparison::Eq, true.into()),
        ]),
        hinted().ne(Item::RANK, 0_u8),
        hinted().filter_any(&[
            (Item::LANE, Comparison::Ne, 0_u8.into()),
            (Item::VISIBLE, Comparison::Eq, true.into()),
        ]),
    ] {
        assert!(matches!(q.fetch(), Err(Error::InvalidQuery)));
    }
}

#[test]
fn ranges_narrow_membership_for_ranked_pages() {
    use Comparison::{Ge, Gt, Le, Lt};
    let (sql, host) = fixture();
    let db = open(&host);
    for (cmp, op, bound) in [
        (Lt, "<", 1_u8),
        (Le, "<=", 0),
        (Gt, ">", 1),
        (Ge, ">=", 2),
        (Gt, ">", 2),
    ] {
        let expected = sql
            .prepare(&format!(
                "SELECT id FROM items WHERE lane IN (0,1,2) AND lane {op} ?1 ORDER BY rank, id"
            ))
            .unwrap()
            .query_map([bound], |r| r.get::<_, u32>(0).map(u64::from))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut seen = Vec::new();
        let mut after = None;
        for _ in 0..=expected.len() {
            let mut q = db
                .query::<Item>()
                .filter_in(Item::LANE, &[0_u8, 1, 2])
                .filter(Item::LANE, cmp, bound)
                .order_asc(Item::RANK)
                .limit(1)
                .work_limit(1);
            if let Some(cursor) = &after {
                q = q.after(cursor);
            }
            let page = q.fetch_as::<Id>().unwrap();
            if expected.is_empty() {
                assert_eq!(page.examined, 0);
            }
            seen.extend(page.rows.into_iter().map(|row| row.id));
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        assert!(after.is_none());
        assert_eq!(seen, expected);
    }
    // Adjacent strict endpoints exclude every integer. A sibling OR alternative can still match.
    for alternative in [false, true] {
        let expected: u32 = sql
            .query_row(
                "SELECT COUNT(*) FROM items WHERE lane > 0 AND (lane < 1 OR (?1 AND lane = 2))",
                [alternative],
                |row| row.get(0),
            )
            .unwrap();
        let q = db.query::<Item>().gt(Item::LANE, 0_u8);
        let result = if alternative {
            q.filter_any(&[
                (Item::LANE, Lt, 1_u8.into()),
                (Item::LANE, Comparison::Eq, 2_u8.into()),
            ])
        } else {
            q.lt(Item::LANE, 1_u8).work_limit(1)
        }
        .count()
        .unwrap();
        assert_eq!(result.value, u64::from(expected));
        if !alternative {
            assert_eq!((result.examined, result.fetched), (0, 0));
        }
    }
    // The range removes absence as well as the upper Decimal value.
    let low = Decimal::try_new(124, 2).unwrap();
    let high = Decimal::try_new(125, 2).unwrap();
    let expected: u32 = sql.query_row(
        "SELECT id FROM items WHERE lane=0 AND (cents IS NULL OR cents IN (124,125)) AND cents<125",
        [], |row| row.get(0),
    ).unwrap();
    let page = db
        .query::<Item>()
        .eq(Item::LANE, 0_u8)
        .filter_in(Item::PRICE, &[None, Some(low), Some(high)])
        .lt(Item::PRICE, high)
        .order_desc(Item::ID)
        .work_limit(1)
        .fetch_as::<Id>()
        .unwrap();
    assert_eq!(
        page.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        [u64::from(expected)]
    );
    assert_eq!(page.examined, 1);
    assert!(matches!(
        db.query::<Item>()
            .using_index("lane,rank", &[])
            .filter_in(Item::LANE, &[0_u8, 1, 2])
            .lt(Item::LANE, 2_u8)
            .order_asc(Item::RANK)
            .fetch(),
        Err(Error::InvalidQuery)
    ));
}

#[test]
fn sparse_price_order_ignores_an_impossible_or_alternative() {
    let (sql, host) = fixture();
    let db = open(&host);
    let expected = sql.prepare("SELECT id FROM items WHERE lane=0 AND (cents IS NOT NULL OR lane=2) ORDER BY cents, id")
        .unwrap().query_map([], |r| r.get::<_, u32>(0).map(u64::from)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    let scope = [0_u8.into()];
    let clause = [
        (Item::PRICE, Comparison::Ne, ic_db::IndexValue::Absent),
        (Item::LANE, Comparison::Eq, 2_u8.into()),
    ];
    for hinted in [false, true] {
        assert_id_pages(&expected, |continuation| {
            let query = db.query::<Item>().filter_any(&clause);
            let mut query = if hinted {
                query.using_index("lane,price,id", &scope)
            } else {
                query.eq(Item::LANE, 0_u8)
            }
            .order_asc(Item::PRICE)
            .order_asc(Item::ID)
            .limit(1)
            .work_limit(1);
            if let Some(cursor) = continuation {
                query = query.after(cursor);
            }
            query.fetch_as::<Id>().unwrap()
        });
    }
    // Lane 2 contains an unpriced item. Its OR branch keeps the sparse path unsafe.
    let unpriced: u32 = sql
        .query_row(
            "SELECT id FROM items WHERE lane=2 AND cents IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let page = db
        .query::<Item>()
        .eq(Item::LANE, 2_u8)
        .filter_any(&clause)
        .order_asc(Item::PRICE)
        .fetch_as::<Id>()
        .unwrap();
    assert_eq!(
        page.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        [u64::from(unpriced)]
    );
    assert!(matches!(
        db.query::<Item>()
            .using_index("lane,price,id", &[2_u8.into()])
            .filter_any(&clause)
            .order_asc(Item::PRICE)
            .fetch(),
        Err(Error::InvalidQuery)
    ));
}

#[test]
fn sparse_price_order_checks_both_operands_of_a_field_comparison() {
    use ic_db::QueryOperand::{Field, Literal};
    let (sql, host) = fixture();
    let db = open(&host);
    let clause = [
        (
            Item::PRICE,
            Comparison::Ne,
            Literal(ic_db::IndexValue::Absent),
        ),
        (Item::LANE, Comparison::Gt, Field(Item::RANK)),
    ];
    let query = || {
        db.query::<Item>()
            .filter_any_operands(&clause)
            .order_asc(Item::PRICE)
            .order_asc(Item::ID)
    };
    let expected = sql.prepare("SELECT id FROM items WHERE lane=0 AND rank=2 AND (cents IS NOT NULL OR lane>rank) ORDER BY cents,id")
        .unwrap().query_map([], |r| r.get::<_, u32>(0).map(u64::from)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert_id_pages(&expected, |continuation| {
        let mut q = query()
            .eq(Item::LANE, 0_u8)
            .eq(Item::RANK, 2_u8)
            .limit(1)
            .work_limit(1);
        if let Some(cursor) = continuation {
            q = q.after(cursor);
        }
        q.fetch_as::<Id>().unwrap()
    });
    let lane = [2_u8.into()];
    let hinted = || query().using_index("lane,price,id", &lane);
    assert!(matches!(hinted().fetch(), Err(Error::InvalidQuery)));
    assert!(matches!(
        hinted().eq(Item::RANK, 1_u8).fetch(),
        Err(Error::InvalidQuery)
    ));
    let unpriced: u32 = sql
        .query_row(
            "SELECT id FROM items WHERE lane=2 AND rank=1 AND cents IS NULL AND lane>rank",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let page = query()
        .eq(Item::LANE, 2_u8)
        .eq(Item::RANK, 1_u8)
        .fetch_as::<Id>()
        .unwrap();
    assert_eq!(
        page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        [u64::from(unpriced)]
    );
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
        let page = listing.select(|item| item.id).fetch().unwrap();
        assert!(matches!(
            matching().work_limit(4).count(),
            Err(Error::Limit)
        ));
        let total = matching().count().unwrap().value;
        (
            page.rows
                .into_iter()
                .map(|id| (id, total))
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
fn top_lanes_by_distinct_visible_offers_match_sqlite_across_checkpoints() {
    let (sql, host) = fixture();
    let mut db = open(&host);
    // Extend the 13-item fixture to 128. Visible offers repeat across chunks;
    // hidden offers have a separate price that would inflate the counts.
    for start in (14_u32..=128).step_by(16) {
        let mut batch = Batch::new();
        for id in start..=(start + 15).min(128) {
            let lane = (id % 6) as u8;
            let rank = ((id / 6) % u32::from(1 + lane / 2)) as u8;
            let visible = id % 7 != 0;
            let cents = if !visible {
                Some(999_i64)
            } else if lane == 0 {
                None
            } else {
                [None, Some(125), Some(250)][(id / 18 % 3) as usize]
            };
            sql.execute(
                "INSERT INTO items VALUES (?,?,?,?,?)",
                params![id, lane, rank, visible, cents],
            )
            .unwrap();
            batch
                .insert(&Item {
                    id: u64::from(id),
                    lane,
                    rank,
                    visible,
                    price: cents.map(|n| Decimal::try_new(i128::from(n), 2).unwrap()),
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
    }

    let expected = sql
        .prepare(
            "WITH offers AS (SELECT DISTINCT lane, rank, cents FROM items WHERE visible)
         SELECT lane, COUNT(*) AS n FROM offers GROUP BY lane HAVING COUNT(*) >= 6
         ORDER BY n DESC, lane ASC LIMIT 3",
        )
        .unwrap()
        .query_map([], |r| {
            Ok((r.get::<_, u8>(0)?, u64::from(r.get::<_, u32>(1)?)))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(expected, [(4, 9), (5, 9), (2, 6)]);
    let query = || {
        db.query::<Item>()
            .eq(Item::VISIBLE, true)
            .filter_groups(Comparison::Ge, 6_u64)
            .order_groups(GroupOrder::AggregateDescending)
            .limit(3)
    };
    let values = |g: ic_db::GroupCount<Lane>| (g.key.lane, g.count);
    let sync = query()
        .work_limit(129)
        .group_count_distinct::<Lane, RankedPrice>()
        .unwrap();
    assert_eq!(
        sync.value.into_iter().map(values).collect::<Vec<_>>(),
        expected
    );
    assert!(matches!(
        query()
            .work_limit(7)
            .group_count_distinct::<Lane, RankedPrice>(),
        Err(Error::Limit)
    ));

    let mut jobs = AggregateJobs::open(
        host.open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let now = Timestamp::from_millis(0);
    let handle = query()
        .start_group_count_distinct::<Lane, RankedPrice>(
            &mut jobs,
            now,
            Timestamp::from_millis(100),
        )
        .unwrap();
    let mut cursor = None;
    let mut sequence = 0;
    let mut examined = 0;
    let mut output = Vec::new();
    loop {
        let step = query()
            .work_limit(7)
            .group_page(1, cursor.as_ref())
            .resume_group_count_distinct::<Lane, RankedPrice>(&mut jobs, &handle, sequence, now)
            .unwrap();
        assert!((examined..=examined + 7).contains(&step.examined));
        sequence = step.sequence;
        match step.outcome {
            JobOutcome::Running => {
                assert!(cursor.is_none() && step.examined > examined && step.examined <= 128);
                assert_eq!(step.output_bytes, 0);
            }
            JobOutcome::Complete(page) => {
                assert_eq!(step.examined, 128);
                assert_eq!(page.rows.len(), 1);
                output.extend(page.rows.into_iter().map(values));
                assert_eq!(output, expected[..output.len()]);
                cursor = page.next;
                if cursor.is_none() {
                    break;
                }
            }
            _ => panic!("unchanged listing fixture must complete"),
        }
        examined = step.examined;
    }
    assert_eq!(output, expected);
    jobs.cancel(&handle).unwrap();
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
    let check = |db: &Database| {
        // The batch below deletes 9 and inserts 14. Missing 30 and repeated 9
        // exercise both the complete key probes and the smaller-budget scan.
        let candidates = [30_u64, 14, 9, 9];
        let first_present: u32 = sql
            .query_row("SELECT MIN(id) FROM items WHERE id IN (9,14,30)", [], |r| {
                r.get(0)
            })
            .unwrap();
        for visible in [true, false] {
            let expected: u32 = sql
                .query_row(
                    "SELECT COUNT(*) FROM items WHERE id IN (9,14,30) AND visible=?1",
                    [visible],
                    |r| r.get(0),
                )
                .unwrap();
            let query = || {
                db.query::<Item>()
                    .filter_in(Item::ID, &candidates)
                    .eq(Item::VISIBLE, visible)
            };
            let found = query().work_limit(3).exists().unwrap();
            let probes = if expected == 0 {
                3
            } else if first_present == 9 {
                1
            } else {
                2
            };
            assert_eq!(
                (found.value, found.examined, found.fetched),
                (expected > 0, probes, 1)
            );
            let count = query().work_limit(3).count().unwrap();
            assert_eq!(
                (count.value, count.examined, count.fetched),
                (u64::from(expected), 3, 1)
            );
            assert!(matches!(query().work_limit(1).count(), Err(Error::Limit)));
            let bounded = query().work_limit(1).exists();
            if visible && first_present == 9 {
                let hit = bounded.unwrap();
                assert_eq!((hit.value, hit.examined, hit.fetched), (true, 1, 1));
            } else {
                assert!(matches!(bounded, Err(Error::Limit)));
            }
        }
        let ids = [2_u64, 8, 30, 2];
        for reverse in [false, true] {
            let mut terms = [
                (Item::VISIBLE, Comparison::Eq, true.into()),
                (Item::ID, Comparison::Ge, 2_u64.into()),
            ];
            if reverse {
                terms.reverse();
            }
            let query = || {
                db.query::<Item>()
                    .filter_in(Item::ID, &ids)
                    .filter_any(&terms)
                    .work_limit(3)
            };
            let count = query().count().unwrap();
            let expected: u32 = sql
                .query_row(
                    "SELECT COUNT(*) FROM items WHERE id IN (2,8,30,2) AND (visible OR id>=2)",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                (count.value, count.examined, count.fetched),
                (u64::from(expected), 3, 0)
            );
            let checked = query().count_validated().unwrap();
            assert_eq!(
                (checked.value, checked.examined, checked.fetched),
                (u64::from(expected), 3, expected as usize)
            );
        }
        let alternatives = [
            (Item::VISIBLE, Comparison::Eq, true.into()),
            (Item::ID, Comparison::Eq, 8_u64.into()),
        ];
        let expected: u32 = sql.query_row(
            "SELECT COUNT(*) FROM items WHERE id IN (2,8,30,2) AND (visible OR id=8) AND id BETWEEN 7 AND 9",
            [], |row| row.get(0)).unwrap();
        for range_first in [false, true] {
            let query = db.query::<Item>().filter_in(Item::ID, &ids);
            let query = if range_first {
                query
                    .filter_between(Item::ID, 7_u64, 9_u64)
                    .filter_any(&alternatives)
            } else {
                query
                    .filter_any(&alternatives)
                    .filter_between(Item::ID, 7_u64, 9_u64)
            };
            let count = query.work_limit(1).count().unwrap();
            assert_eq!(
                (count.value, count.examined, count.fetched),
                (u64::from(expected), 1, 0)
            );
        }
        let residual = db
            .query::<Item>()
            .filter_in(Item::ID, &ids)
            .filter_any(&alternatives)
            .filter_between(Item::ID, 2_u64, 9_u64)
            .count()
            .unwrap();
        let expected: u32 = sql.query_row(
            "SELECT COUNT(*) FROM items WHERE id IN (2,8,30,2) AND (visible OR id=8) AND id BETWEEN 2 AND 9",
            [], |row| row.get(0)).unwrap();
        assert_eq!(residual.value, u64::from(expected));
        assert!(residual.fetched > 0);
        let fixed = [0_u8.into(), 2_u8.into()];
        let query = || {
            db.query::<Item>()
                .using_index("lane,rank", &fixed)
                .filter_in(Item::ID, &[1_u64, 3, 8, 30])
        };
        for lane in [0_u8, 1] {
            let expected: u32 = sql.query_row(
                    "SELECT COUNT(*) FROM items WHERE lane=0 AND rank=2 AND id IN (1,3,8,30) AND (lane=?1 OR id=3)",
                    [lane], |row| row.get(0)).unwrap();
            let probes = if lane == 0 { 4 } else { 1 };
            let count = query()
                .filter_any(&[
                    (Item::LANE, Comparison::Eq, lane.into()),
                    (Item::ID, Comparison::Eq, 3_u64.into()),
                ])
                .work_limit(probes)
                .count()
                .unwrap();
            assert_eq!(
                (count.value, count.examined, count.fetched),
                (u64::from(expected), probes, 0)
            );
        }
        let residual = query()
            .filter_any(&[
                (Item::VISIBLE, Comparison::Eq, true.into()),
                (Item::ID, Comparison::Eq, 3_u64.into()),
            ])
            .count()
            .unwrap();
        let expected: u32 = sql.query_row(
                "SELECT COUNT(*) FROM items WHERE lane=0 AND rank=2 AND id IN (1,3,8,30) AND (visible OR id=3)",
                [], |row| row.get(0)).unwrap();
        assert_eq!(residual.value, u64::from(expected));
        assert!(residual.fetched > 0);
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
                let present: bool = sql
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM items WHERE lane=?1 AND cents=125 AND id=?2)",
                        params![lane, id],
                        |r| r.get(0),
                    )
                    .unwrap();
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

    // Deleting absent IDs is a no-op in both engines.
    let mut deletes = Batch::new();
    deletes.delete::<Item>(15).unwrap();
    deletes.delete::<Item>(99).unwrap();
    db.commit(deletes).unwrap();
    sql.execute("DELETE FROM items WHERE id IN (15,99)", [])
        .unwrap();
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
