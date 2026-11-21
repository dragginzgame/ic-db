//! Compare direct record-member predicates with nullable SQLite scalar columns.

use ic_db::design::prelude::*;
use ic_db::{Batch, Error, testing::Fixture};
use rusqlite::Connection;

///
/// Price
/// Stores an optional amount at scale two.
///

#[record(fields(
    field(name = "amount", value(opt, item(prim = "Decimal", scale = 2))),
    field(name = "currency", value(opt, item(prim = "Text", max_bytes = 3)))
))]
struct Price {}

/// Currency codes accepted by a product.
#[set(item(prim = "Text", max_bytes = 3), max_items = 3)]
struct Currencies {}

///
/// Product
/// Exercises a price field that may be absent independently of its amount.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "accepted", value(opt, many, max_items = 3, item(prim = "Text", max_bytes = 3))),
        field(name = "unique_codes", value(opt, item(is = "Currencies"))),
        field(name = "prefix", value(opt, item(prim = "Text", max_bytes = 8))),
        field(name = "price", value(opt, item(is = "Price")))
    )
)]
struct Product {}

#[test]
fn nested_prices_match_sqlite_nullable_cents_after_reopen() {
    const AMOUNT: &str = ic_db::field_path!(Product::PRICE, Price::AMOUNT);
    const CURRENCY: &str = ic_db::field_path!(Product::PRICE, Price::CURRENCY);
    let fixture = Fixture::new("test.products.v1");
    let schema = || Schema::new(&[Product::SCHEMA]).unwrap();
    let mut db = fixture.create(schema(), [1; 16]).unwrap();
    let sql = Connection::open_in_memory().unwrap();
    sql.execute(
        "CREATE TABLE products(id INTEGER PRIMARY KEY, cents INTEGER, currency TEXT, prefix TEXT)",
        [],
    )
    .unwrap();
    sql.execute("CREATE TABLE codes(product INTEGER, code TEXT)", [])
        .unwrap();
    let mut batch = Batch::new();
    for (id, (price, currency, prefix, codes)) in [
        (None, None, Some(""), Some(&["USD"][..])),
        (Some(None), None, None, Some(&[""])),
        (Some(Some(0_i32)), Some(""), Some("US"), Some(&[""])),
        (
            Some(Some(-15)),
            Some("USD"),
            Some("U"),
            Some(&["USD", "USD"]),
        ),
        (Some(Some(125)), Some("EUR"), None, None),
        (Some(Some(-15)), Some("USD"), Some(""), Some(&[])),
    ]
    .into_iter()
    .enumerate()
    {
        sql.execute(
            "INSERT INTO products VALUES (?1,?2,?3,?4)",
            (id as u32, price.flatten(), currency, prefix),
        )
        .unwrap();
        for code in codes.into_iter().flatten() {
            sql.execute("INSERT INTO codes VALUES (?1,?2)", (id as u32, code))
                .unwrap();
        }
        batch
            .insert(&Product {
                id: id as u64,
                accepted: codes.map(|codes| codes.iter().copied().map(String::from).collect()),
                unique_codes: codes
                    .map(|codes| Currencies(codes.iter().copied().map(String::from).collect())),
                prefix: prefix.map(String::from),
                price: price.map(|amount| Price {
                    amount: amount.map(|c| Decimal::try_new(i128::from(c), 2).unwrap()),
                    currency: currency.map(String::from),
                }),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    drop(db);
    let db = fixture.open(schema()).unwrap();
    let zero = Decimal::try_new(0, 2).unwrap();
    for (op, sql_op) in [
        (ic_db::Comparison::Eq, "="),
        (ic_db::Comparison::Ne, "IS NOT"),
        (ic_db::Comparison::Lt, "<"),
        (ic_db::Comparison::Le, "<="),
        (ic_db::Comparison::Gt, ">"),
        (ic_db::Comparison::Ge, ">="),
    ] {
        let mut oracle = sql
            .prepare(&format!(
                "SELECT COUNT(*) FROM products WHERE length(CAST(currency AS BLOB)) {sql_op} ?1"
            ))
            .unwrap();
        for threshold in [0_u32, 1, 3, 4, u32::MAX] {
            let expected: u64 = oracle
                .query_row([threshold], |r| r.get::<_, u32>(0).map(u64::from))
                .unwrap();
            let query = || db.query::<Product>().filter_length(CURRENCY, op, threshold);
            let count = query().count().unwrap();
            assert_eq!(count.value, expected);
            assert_eq!(query().count_validated().unwrap().value, expected);
            // Integer lengths strictly between 2 and 4 can only be 3.
            let narrowed = query()
                .filter_length(CURRENCY, ic_db::Comparison::Gt, 2)
                .filter_length(CURRENCY, ic_db::Comparison::Lt, 4)
                .count()
                .unwrap();
            let (expected, accepts_three): (u32, bool) = sql
                .query_row(
                    &format!("SELECT count(*), 3 {sql_op} ?1 FROM products WHERE length(CAST(currency AS BLOB)) = 3 AND 3 {sql_op} ?1"),
                    [threshold],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(narrowed.value, u64::from(expected));
            if !accepts_three {
                assert_eq!(narrowed.examined, 0);
            }
            if threshold > 3
                && matches!(
                    op,
                    ic_db::Comparison::Eq
                        | ic_db::Comparison::Ne
                        | ic_db::Comparison::Gt
                        | ic_db::Comparison::Ge
                )
            {
                assert_eq!((count.examined, count.fetched), (0, 0));
            }
        }
    }
    for (comparison, condition) in [
        (ic_db::Comparison::Eq, "cents IS cents"),
        (ic_db::Comparison::Ne, "cents IS NOT cents"),
        (ic_db::Comparison::Lt, "cents < cents"),
        (ic_db::Comparison::Gt, "cents > cents"),
    ] {
        let expected: u64 = sql
            .query_row(
                &format!("SELECT COUNT(*) FROM products WHERE {condition}"),
                [],
                |row| row.get::<_, u32>(0).map(u64::from),
            )
            .unwrap();
        let query = || {
            db.query::<Product>()
                .filter_fields(AMOUNT, comparison, AMOUNT)
        };
        let count = query().work_limit(1).count().unwrap();
        assert_eq!(
            (count.value, count.examined, count.fetched),
            (expected, 0, 0)
        );
        assert_eq!(query().count_validated().unwrap().value, expected);
        if expected == 0 {
            assert_eq!(query().work_limit(1).fetch().unwrap().examined, 0);
        }
    }
    let same_amount = || {
        db.query::<Product>()
            .filter_fields(AMOUNT, ic_db::Comparison::Eq, AMOUNT)
    };
    assert!(matches!(
        same_amount().work_limit(1).count_validated(),
        Err(Error::Limit)
    ));
    let either = |comparison| {
        db.query::<Product>().filter_any_operands(&[
            (AMOUNT, comparison, ic_db::QueryOperand::Field(AMOUNT)),
            (
                Product::ID,
                ic_db::Comparison::Eq,
                ic_db::QueryOperand::Literal(0_u64.into()),
            ),
        ])
    };
    assert_eq!(
        either(ic_db::Comparison::Eq)
            .work_limit(1)
            .count()
            .unwrap()
            .value,
        6
    );
    assert_eq!(either(ic_db::Comparison::Lt).count().unwrap().value, 1);
    assert_eq!(
        same_amount().eq(Product::ID, 0_u64).count().unwrap().value,
        1
    );
    assert!(matches!(
        same_amount().eq("missing", 1_u64).count(),
        Err(Error::InvalidQuery)
    ));
    for (query, predicate) in [
        (
            db.query::<Product>().filter_fields(
                Product::ACCEPTED,
                ic_db::Comparison::Contains,
                CURRENCY,
            ),
            "EXISTS (SELECT 1 FROM codes WHERE product = products.id AND code = currency)",
        ),
        (
            db.query::<Product>().filter_fields(
                Product::UNIQUE_CODES,
                ic_db::Comparison::Contains,
                CURRENCY,
            ),
            "EXISTS (SELECT 1 FROM codes WHERE product = products.id AND code = currency)",
        ),
        (
            db.query::<Product>().filter_fields(
                CURRENCY,
                ic_db::Comparison::StartsWith,
                Product::PREFIX,
            ),
            "instr(currency, prefix) = 1",
        ),
        (
            db.query::<Product>().filter_fields(
                Product::PREFIX,
                ic_db::Comparison::StartsWith,
                CURRENCY,
            ),
            "instr(prefix, currency) = 1",
        ),
        (
            db.query::<Product>()
                .filter_fields(AMOUNT, ic_db::Comparison::Eq, AMOUNT),
            "cents IS cents",
        ),
        (
            db.query::<Product>()
                .filter_fields(AMOUNT, ic_db::Comparison::Ge, AMOUNT),
            "cents >= cents",
        ),
        (
            db.query::<Product>()
                .filter_length(CURRENCY, ic_db::Comparison::Eq, 0),
            "length(CAST(currency AS BLOB)) = 0",
        ),
        (
            db.query::<Product>()
                .filter_length(CURRENCY, ic_db::Comparison::Ne, 3),
            "length(CAST(currency AS BLOB)) IS NOT 3",
        ),
        (
            db.query::<Product>()
                .filter(CURRENCY, ic_db::Comparison::StartsWith, ""),
            "instr(currency, '') = 1",
        ),
        (
            db.query::<Product>()
                .filter(CURRENCY, ic_db::Comparison::StartsWith, "US"),
            "instr(currency, 'US') = 1",
        ),
        (db.query::<Product>().filter_absent(AMOUNT), "cents IS NULL"),
        (
            db.query::<Product>().filter_present(AMOUNT),
            "cents IS NOT NULL",
        ),
        (db.query::<Product>().eq(AMOUNT, zero), "cents = 0"),
        (db.query::<Product>().gt(AMOUNT, zero), "cents > 0"),
        (db.query::<Product>().ne(AMOUNT, zero), "cents IS NOT 0"),
    ] {
        let expected = sql
            .prepare(&format!(
                "SELECT id FROM products WHERE {predicate} ORDER BY id"
            ))
            .unwrap()
            .query_map([], |r| r.get::<_, u32>(0).map(u64::from))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let ids: Vec<_> = query
            .fetch()
            .unwrap()
            .rows
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(ids, expected);
    }
    let containing = |field| {
        db.query::<Product>()
            .filter_fields(Product::ACCEPTED, ic_db::Comparison::Contains, field)
    };
    assert_eq!(containing(CURRENCY).count().unwrap().value, 2);
    let first = containing(CURRENCY).work_limit(1).fetch().unwrap();
    assert!(first.rows.is_empty());
    let cursor = first.next.unwrap();
    assert_eq!(
        containing(CURRENCY)
            .after(&cursor)
            .fetch()
            .unwrap()
            .rows
            .len(),
        2
    );
    assert!(matches!(
        containing(Product::PREFIX).after(&cursor).fetch(),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        containing(AMOUNT).fetch(),
        Err(Error::InvalidQuery)
    ));
    for (query, order) in [
        (db.query::<Product>().order_asc(AMOUNT), "cents ASC, id ASC"),
        (
            db.query::<Product>().order_desc(AMOUNT),
            "cents DESC, id ASC",
        ),
        (
            db.query::<Product>()
                .order_desc(AMOUNT)
                .order_desc(Product::ID),
            "cents DESC, id DESC",
        ),
    ] {
        let expected = sql
            .prepare(&format!("SELECT id FROM products ORDER BY {order} LIMIT 5"))
            .unwrap()
            .query_map([], |row| row.get::<_, u32>(0).map(u64::from))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let page = query.limit(5).fetch().unwrap();
        assert_eq!(
            page.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(page.examined, 6);
        assert!(page.next.is_none());
    }
    assert!(matches!(
        db.query::<Product>()
            .order_asc(AMOUNT)
            .limit(1)
            .work_limit(5)
            .fetch(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Product>().order_asc(Product::PRICE).fetch(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Product>()
            .eq(Product::ID, 0_u64)
            .order_asc("price.missing")
            .fetch(),
        Err(Error::InvalidQuery)
    ));
}

#[record(fields(
    field(name = "score", value(item(prim = "Nat16"))),
    field(name = "other", value(item(prim = "Nat16")))
))]
struct Rating {}

#[record(fields(field(name = "bucket", value(item(prim = "Nat16")))))]
struct Bucket {}

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["rating.score"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "bucket", value(item(prim = "Nat16"))),
        field(name = "rating", value(item(is = "Rating")))
    )
)]
struct Ranked {}

#[test]
fn indexed_member_ranges_match_sqlite_with_one_candidate_pages() {
    const SCORE: &str = ic_db::field_path!(Ranked::RATING, Rating::SCORE);
    const OTHER: &str = ic_db::field_path!(Ranked::RATING, Rating::OTHER);
    let fixture = Fixture::new("test.ranked.v1");
    let mut db = fixture
        .create(Schema::new(&[Ranked::SCHEMA]).unwrap(), [2; 16])
        .unwrap();
    let sql = Connection::open_in_memory().unwrap();
    sql.execute(
        "CREATE TABLE ranked(id INTEGER PRIMARY KEY, score INTEGER, other INTEGER, bucket INTEGER)",
        [],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (id, score) in [65535_u16, 255, 256, 0, 256, 257, 254]
        .into_iter()
        .enumerate()
    {
        let other = if id == 2 { 0_u16 } else { 500 };
        let bucket = score % 3;
        sql.execute(
            "INSERT INTO ranked VALUES (?1,?2,?3,?4)",
            (id as u32, score, other, bucket),
        )
        .unwrap();
        batch
            .insert(&Ranked {
                id: id as u64,
                bucket,
                rating: Rating { score, other },
            })
            .unwrap();
    }
    db.commit(batch).unwrap();

    for (left, right, predicate) in [
        (SCORE, OTHER, "score < other"),
        (Ranked::BUCKET, OTHER, "bucket < other"),
        (OTHER, Ranked::BUCKET, "other < bucket"),
    ] {
        let expected = sql
            .prepare(&format!(
                "SELECT id FROM ranked WHERE {predicate} ORDER BY id"
            ))
            .unwrap()
            .query_map([], |r| r.get::<_, u32>(0).map(u64::from))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let query = || {
            db.query::<Ranked>()
                .filter_fields(left, ic_db::Comparison::Lt, right)
        };
        assert_eq!(
            query()
                .fetch()
                .unwrap()
                .rows
                .iter()
                .map(|r| r.id)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(query().count().unwrap().value, expected.len() as u64);
    }
    let compare = |left, right| {
        db.query::<Ranked>()
            .filter_fields(left, ic_db::Comparison::Lt, right)
            .limit(1)
    };
    let cursor = compare(SCORE, OTHER).fetch().unwrap().next.unwrap();
    for changed in [
        compare(OTHER, SCORE),
        compare(SCORE, SCORE),
        compare(OTHER, OTHER),
    ] {
        assert!(matches!(
            changed.after(&cursor).fetch(),
            Err(Error::InvalidCursor)
        ));
    }
    assert!(matches!(
        compare(SCORE, Ranked::ID).fetch(),
        Err(Error::InvalidQuery)
    ));

    let sorted = |q: ic_db::Query<'_, Ranked>| {
        q.order_asc(OTHER)
            .order_desc(Ranked::BUCKET)
            .distinct()
            .limit(2)
            .fetch_as::<Bucket>()
    };
    let expected = sql
        .prepare("SELECT DISTINCT bucket FROM ranked WHERE other = 500 ORDER BY other, bucket DESC LIMIT 2")
        .unwrap()
        .query_map([], |r| r.get::<_, u16>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>().unwrap();
    let page = sorted(db.query::<Ranked>().eq(OTHER, 500_u16)).unwrap();
    assert_eq!(
        page.rows.into_iter().map(|r| r.bucket).collect::<Vec<_>>(),
        expected
    );
    assert!(page.next.is_none());
    assert!(matches!(
        sorted(db.query::<Ranked>().eq(OTHER, 500_u16).work_limit(1)),
        Err(Error::Limit)
    ));
    for query in [
        db.query::<Ranked>().eq(SCORE, 256_u16),
        db.query::<Ranked>().filter_in(OTHER, &[0_u16, 500]),
    ] {
        assert!(matches!(sorted(query), Err(Error::InvalidQuery)));
    }

    use ic_db::Comparison::{Ge, Gt, Le, Lt};
    for (filters, predicate, candidates) in [
        (
            &[(SCORE, Ge, 255_u16), (SCORE, Le, 256)][..],
            "score BETWEEN 255 AND 256",
            3,
        ),
        (
            &[(SCORE, Gt, 255), (SCORE, Lt, 257)],
            "score > 255 AND score < 257",
            2,
        ),
        (&[(SCORE, Gt, u16::MAX)], "score > 65535", 0),
        (&[(SCORE, Lt, 0)], "score < 0", 0),
        (
            &[(SCORE, Ge, 256), (SCORE, Lt, 256)],
            "score >= 256 AND score < 256",
            0,
        ),
        (
            &[(SCORE, Gt, 255), (OTHER, Lt, 256)],
            "score > 255 AND other < 256",
            4,
        ),
    ] {
        let expected = sql
            .prepare(&format!(
                "SELECT id FROM ranked WHERE {predicate} ORDER BY score, id"
            ))
            .unwrap()
            .query_map([], |r| r.get::<_, u32>(0).map(u64::from))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut cursor = None;
        let mut ids = Vec::new();
        let mut examined = 0;
        loop {
            let mut query = db.query::<Ranked>().order_asc(SCORE).work_limit(1);
            for &(field, op, value) in filters {
                query = query.filter(field, op, value);
            }
            if let Some(after) = &cursor {
                query = query.after(after);
            }
            let page = query.fetch().unwrap();
            examined += page.examined;
            assert!(examined <= candidates, "{predicate}");
            ids.extend(page.rows.into_iter().map(|row| row.id));
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(ids, expected, "{predicate}");
        assert_eq!(examined, candidates, "{predicate}");
    }
    // Each row in the SQLite interval corresponds to one candidate-budgeted page.
    // The two tied scores at 256 produce consecutive empty pages between matches.
    let expected = sql
        .prepare("SELECT CASE WHEN score IN (257,255) THEN id END FROM ranked WHERE score BETWEEN 255 AND 257 ORDER BY score DESC, id DESC")
        .unwrap()
        .query_map([], |r| r.get::<_, Option<u32>>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(expected, [Some(5), None, None, Some(1)]);
    let sparse = || {
        db.query::<Ranked>()
            .filter_in(SCORE, &[257_u16, 255, 257])
            .order_desc(SCORE)
            .order_desc(Ranked::ID)
            .work_limit(1)
    };
    let mut page = sparse().fetch().unwrap();
    for (position, id) in expected.into_iter().enumerate() {
        if position > 0 {
            page = sparse().after(page.next.as_ref().unwrap()).fetch().unwrap();
        }
        assert_eq!(page.examined, 1);
        assert!(page.rows.iter().map(|row| row.id).eq(id.map(u64::from)));
    }
    if let Some(after) = page.next {
        let end = sparse().after(&after).fetch().unwrap();
        assert_eq!(end.examined, 0);
        assert!(end.rows.is_empty() && end.next.is_none());
    }

    let count = db
        .query::<Ranked>()
        .gt(SCORE, 255_u16)
        .lt(SCORE, 257_u16)
        .count()
        .unwrap();
    assert_eq!((count.value, count.examined, count.fetched), (2, 2, 0));

    // A sibling can match outside the indexed alternative. At 256 one owner
    // matches both alternatives; it must still appear exactly once.
    for score in [255_u16, 256] {
        let expected = sql
            .prepare("SELECT id FROM ranked WHERE score >= 255 AND (score = ?1 OR other = 0) ORDER BY score, id")
            .unwrap()
            .query_map([score], |r| r.get::<_, u32>(0).map(u64::from))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let query = || {
            db.query::<Ranked>().gte(SCORE, 255_u16).filter_any(&[
                (SCORE, ic_db::Comparison::Eq, score.into()),
                (OTHER, ic_db::Comparison::Eq, 0_u16.into()),
            ])
        };
        let count = query().count().unwrap();
        assert_eq!(count.value, expected.len() as u64);
        assert_eq!((count.examined, count.fetched), (5, 5));
        assert!(matches!(query().work_limit(4).count(), Err(Error::Limit)));
        let page = query().order_asc(SCORE).fetch().unwrap();
        assert_eq!(
            page.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn nested_index_cursor_resumes_after_deleted_key_across_reopen() {
    const SCORE: &str = ic_db::field_path!(Ranked::RATING, Rating::SCORE);
    let fixture = Fixture::new("test.ranked.live.v1");
    let schema = || Schema::new(&[Ranked::SCHEMA]).unwrap();
    let mut db = fixture.create(schema(), [3; 16]).unwrap();
    let sql = Connection::open_in_memory().unwrap();
    sql.execute(
        "CREATE TABLE ranked(id INTEGER PRIMARY KEY, score INTEGER)",
        [],
    )
    .unwrap();
    let row = |id: u32, score| Ranked {
        id: u64::from(id),
        bucket: 0,
        rating: Rating { score, other: 0 },
    };
    let mut batch = Batch::new();
    for (id, score) in [(1, 10), (2, 20), (3, 20), (4, 30)] {
        batch.insert(&row(id, score)).unwrap();
        sql.execute("INSERT INTO ranked VALUES (?1,?2)", (id, score))
            .unwrap();
    }
    db.commit(batch).unwrap();
    let first = db
        .query::<Ranked>()
        .order_asc(SCORE)
        .limit(2)
        .fetch()
        .unwrap();
    assert_eq!(first.rows.iter().map(|r| r.id).collect::<Vec<_>>(), [1, 2]);
    let anchor = first.rows.last().unwrap();
    let cursor = first.next.unwrap();

    // Remove the anchor, move a returned row ahead of it and an unseen row behind it.
    let mut batch = Batch::new();
    batch.delete::<Ranked>(2).unwrap();
    sql.execute("DELETE FROM ranked WHERE id=2", []).unwrap();
    for (id, score) in [(1, 40), (4, 5)] {
        batch.replace(&row(id, score)).unwrap();
        sql.execute("UPDATE ranked SET score=?2 WHERE id=?1", (id, score))
            .unwrap();
    }
    db.commit(batch).unwrap();
    drop(db);
    let db = fixture.open(schema()).unwrap();
    let expected = sql
        .prepare("SELECT id FROM ranked WHERE (score,id) > (?1,?2) ORDER BY score,id")
        .unwrap()
        .query_map(
            (anchor.rating.score, i64::try_from(anchor.id).unwrap()),
            |r| r.get::<_, u32>(0).map(u64::from),
        )
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(expected, [3, 1]);
    let page = db
        .query::<Ranked>()
        .order_asc(SCORE)
        .limit(2)
        .after(&cursor)
        .fetch()
        .unwrap();
    assert_eq!(page.rows.iter().map(|r| r.id).collect::<Vec<_>>(), expected);
}

#[newtype(item(prim = "Text", max_bytes = 32))]
struct Tag {}

#[enum_(
    variant(name = "Ready"),
    variant(name = "Archived"),
    variant(name = "Draft")
)]
struct Phase {}

#[enum_(
    variant(name = "Ready"),
    variant(name = "Archived"),
    variant(name = "Draft")
)]
struct OtherPhase {}

#[record(fields(
    field(name = "tag", value(item(is = "Tag"))),
    field(name = "phase", value(item(indirect, is = "Phase"))),
    field(name = "label", value(item(prim = "Text", max_bytes = 32))),
    field(name = "payload", value(item(prim = "Blob", max_bytes = 32))),
    field(name = "header", value(item(prim = "Blob", max_bytes = 4))),
    field(name = "account_part", value(item(prim = "Subaccount"))),
    field(name = "balance", value(item(prim = "Decimal", scale = 2))),
    field(name = "score", value(item(prim = "Int64")))
))]
struct Measures {}

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["measures.account_part"]),
    index(fields = ["measures.label"]),
    index(fields = ["measures.tag"]),
    index(fields = ["measures.phase"]),
    index(fields = ["measures.payload"]),
    index(fields = ["measures.balance"]),
    index(fields = ["measures.score"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "measures", value(item(is = "Measures")))
    )
)]
struct Observation {}

#[test]
fn scalar_member_keys_agree_with_sqlite_after_reopen_and_replacement() {
    use ic_db::IndexValue;
    const SCORE: &str = ic_db::field_path!(Observation::MEASURES, Measures::SCORE);
    const BALANCE: &str = ic_db::field_path!(Observation::MEASURES, Measures::BALANCE);
    const LABEL: &str = ic_db::field_path!(Observation::MEASURES, Measures::LABEL);
    const PAYLOAD: &str = ic_db::field_path!(Observation::MEASURES, Measures::PAYLOAD);
    const HEADER: &str = ic_db::field_path!(Observation::MEASURES, Measures::HEADER);
    const TAG: &str = ic_db::field_path!(Observation::MEASURES, Measures::TAG);
    const PHASE: &str = ic_db::field_path!(Observation::MEASURES, Measures::PHASE);
    const PART: &str = ic_db::field_path!(Observation::MEASURES, Measures::ACCOUNT_PART);
    let fixture = Fixture::new("test.measures.v1");
    let schema = || Schema::new(&[Observation::SCHEMA]).unwrap();
    let mut db = fixture.create(schema(), [3; 16]).unwrap();
    let sql = Connection::open_in_memory().unwrap();
    sql.execute("CREATE TABLE observations(id INTEGER PRIMARY KEY, score INTEGER, cents INTEGER, part BLOB, label TEXT COLLATE BINARY, payload BLOB, phase TEXT COLLATE BINARY, tag TEXT COLLATE BINARY, header BLOB)", []).unwrap();
    for round in 0..2 {
        let mut batch = Batch::new();
        for (id, score, cents, byte) in [
            (1_u32, i64::MIN, 200, 128_u8),
            (2, 1, -101, 255),
            (3, -1, 0, 0),
            (4, i64::MAX, -100, 127),
        ] {
            if round == 1 && id != 2 {
                continue;
            }
            let (score, cents, byte) = if round == 1 {
                (-2, 201, 1)
            } else {
                (score, cents, byte)
            };
            let part = [byte; 32];
            let (label, payload, header): (&str, &[u8], &[u8]) = match byte {
                0 => ("", &[0xff], &[]),
                127 => ("a", &[], &[0]),
                128 => ("a\0", &[0, 0xff], &[0]),
                255 => ("é", &[0], &[0, 1]),
                _ => ("a\0z", &[0, 1], &[0, 1]),
            };
            let tag = format!("{:03}", 255 - byte);
            let (phase, phase_name) = match byte {
                0 | 127 => (Phase::Archived, "Archived"),
                128 | 1 => (Phase::Draft, "Draft"),
                _ => (Phase::Ready, "Ready"),
            };
            sql.execute(
                "INSERT OR REPLACE INTO observations VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                (
                    id,
                    score,
                    cents,
                    part.as_slice(),
                    label,
                    payload,
                    phase_name,
                    &tag,
                    header,
                ),
            )
            .unwrap();
            let row = Observation {
                id: u64::from(id),
                measures: Measures {
                    tag: Tag(tag),
                    phase: Box::new(phase),
                    label: label.into(),
                    payload: payload.into(),
                    header: header.into(),
                    account_part: part,
                    balance: Decimal::try_new(i128::from(cents), 2).unwrap(),
                    score,
                },
            };
            if round == 0 {
                batch.insert(&row)
            } else {
                batch.replace(&row)
            }
            .unwrap();
        }
        db.commit(batch).unwrap();
        drop(db);
        db = fixture.open(schema()).unwrap();
        assert!(matches!(
            db.query::<Observation>()
                .eq(PHASE, &OtherPhase::Draft)
                .count(),
            Err(Error::InvalidOperand)
        ));
        let tag = Tag("128".into());
        let expected = sql
            .prepare(
                "SELECT id FROM observations WHERE instr(payload,header) = 1 ORDER BY payload,id",
            )
            .unwrap()
            .query_map([], |r| r.get::<_, u32>(0).map(u64::from))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let field_prefix = || {
            db.query::<Observation>()
                .filter_fields(PAYLOAD, ic_db::Comparison::StartsWith, HEADER)
        };
        let page = field_prefix().order_asc(PAYLOAD).fetch().unwrap();
        assert_eq!(
            page.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            expected
        );
        let count = field_prefix().count().unwrap();
        assert_eq!((count.value, count.fetched), (expected.len() as u64, 4));
        assert!(matches!(
            field_prefix().work_limit(3).count(),
            Err(Error::Limit)
        ));
        assert!(matches!(
            db.query::<Observation>()
                .filter_fields(PAYLOAD, ic_db::Comparison::StartsWith, LABEL)
                .count(),
            Err(Error::InvalidQuery)
        ));
        for (path, column) in [(LABEL, "label"), (PAYLOAD, "payload"), (TAG, "tag")] {
            let expected = sql.prepare(&format!("SELECT id FROM observations WHERE length(CAST({column} AS BLOB)) >= 2 ORDER BY id"))
                .unwrap().query_map([], |r| r.get::<_, u32>(0).map(u64::from)).unwrap()
                .collect::<Result<Vec<_>, _>>().unwrap();
            let query = || {
                db.query::<Observation>()
                    .filter_length(path, ic_db::Comparison::Ge, 2)
            };
            let page = query().fetch().unwrap();
            assert_eq!(page.rows.iter().map(|r| r.id).collect::<Vec<_>>(), expected);
            assert!(matches!(query().work_limit(1).count(), Err(Error::Limit)));
        }
        let length = |path, n| {
            db.query::<Observation>()
                .filter_length(path, ic_db::Comparison::Ge, n)
                .limit(1)
        };
        let cursor = length(LABEL, 0).fetch().unwrap().next.unwrap();
        for changed in [length(LABEL, 1), length(PAYLOAD, 0)] {
            assert!(matches!(
                changed.after(&cursor).fetch(),
                Err(Error::InvalidCursor)
            ));
        }
        assert!(matches!(length(SCORE, 0).fetch(), Err(Error::InvalidQuery)));
        for (path, operand, column, prefix) in [
            (LABEL, IndexValue::from("a\0"), "label", &b"a\0"[..]),
            (LABEL, "é".into(), "label", "é".as_bytes()),
            (PAYLOAD, (&[0_u8]).into(), "payload", &[0][..]),
            (PAYLOAD, (&[0xff_u8]).into(), "payload", &[0xff][..]),
            (PAYLOAD, (&[] as &[u8]).into(), "payload", &[][..]),
        ] {
            let expected = sql
                .prepare(&format!("SELECT id FROM observations WHERE instr(CAST({column} AS BLOB), ?1) = 1 ORDER BY {column}, id"))
                .unwrap()
                .query_map([prefix], |r| r.get::<_, u32>(0).map(u64::from))
                .unwrap()
                .collect::<Result<Vec<_>, _>>().unwrap();
            let query = || {
                db.query::<Observation>()
                    .filter(path, ic_db::Comparison::StartsWith, operand)
            };
            let page = query().order_asc(path).fetch().unwrap();
            assert_eq!(page.examined, expected.len());
            assert_eq!(page.rows.iter().map(|r| r.id).collect::<Vec<_>>(), expected);
            let count = query().count().unwrap();
            assert_eq!((count.value, count.fetched), (expected.len() as u64, 0));
        }
        for (path, upper, column, sql_value) in [
            (
                TAG,
                IndexValue::from(&tag),
                "tag",
                String::from("128").into(),
            ),
            (
                PHASE,
                IndexValue::from(&Phase::Draft),
                "phase",
                String::from("Draft").into(),
            ),
            (
                SCORE,
                IndexValue::from(0_i64),
                "score",
                rusqlite::types::Value::Integer(0),
            ),
            (
                BALANCE,
                Decimal::try_new(2, 0).unwrap().into(),
                "cents",
                200_i64.into(),
            ),
            (LABEL, "a\0z".into(), "label", String::from("a\0z").into()),
            (
                PAYLOAD,
                (&[0, 0xff]).into(),
                "payload",
                vec![0_u8, 0xff].into(),
            ),
            (PART, [128_u8; 32].into(), "part", vec![128_u8; 32].into()),
        ] {
            let expected = sql
                .prepare(&format!(
                    "SELECT id FROM observations WHERE {column} <= ?1 ORDER BY {column}, id"
                ))
                .unwrap()
                .query_map([sql_value], |r| r.get::<_, u32>(0).map(u64::from))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let page = db
                .query::<Observation>()
                .lte(path, upper)
                .order_asc(path)
                .fetch()
                .unwrap();
            assert_eq!(
                page.rows.into_iter().map(|r| r.id).collect::<Vec<_>>(),
                expected
            );
            let count = db.query::<Observation>().lte(path, upper).count().unwrap();
            assert_eq!(count.value, expected.len() as u64);
            assert_eq!((count.examined, count.fetched), (expected.len(), 0));
        }
    }
}

#[record(fields(
    field(name = "value", value(item(prim = "Nat16"))),
    field(name = "maybe", value(opt, item(prim = "Nat16")))
))]
struct Reading {}

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["sample", "id"], unique, present_only),
    index(fields = ["calibration", "sample", "id"], unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "sample", value(opt, item(prim = "Nat16"))),
        field(name = "calibration", value(opt, item(prim = "Nat16"))),
        field(name = "reference", value(item(is = "Reading"))),
        field(name = "previous", value(opt, item(is = "Reading")))
    )
)]
struct Measurement {}

#[test]
fn sparse_index_requires_schema_or_predicates_to_prove_presence() {
    use ic_db::Comparison::Eq;
    const REQUIRED: &str = ic_db::field_path!(Measurement::REFERENCE, Reading::VALUE);
    const OPTIONAL_MEMBER: &str = ic_db::field_path!(Measurement::REFERENCE, Reading::MAYBE);
    const OPTIONAL_PARENT: &str = ic_db::field_path!(Measurement::PREVIOUS, Reading::VALUE);
    let fixture = Fixture::new("test.measurements.v1");
    let mut db = fixture
        .create(Schema::new(&[Measurement::SCHEMA]).unwrap(), [1; 16])
        .unwrap();
    let sql = Connection::open_in_memory().unwrap();
    sql.execute("CREATE TABLE readings(id INTEGER, sample INTEGER, reference INTEGER, maybe INTEGER, previous INTEGER, calibration INTEGER)", []).unwrap();
    let mut batch = Batch::new();
    for (id, sample) in [None, Some(7_u16), Some(9)].into_iter().enumerate() {
        let maybe = sample.map(|_| 7_u16);
        sql.execute(
            "INSERT INTO readings VALUES (?1, ?2, 7, ?3, NULL, 7)",
            (id as u32, sample, maybe),
        )
        .unwrap();
        batch
            .insert(&Measurement {
                id: id as u64,
                sample,
                calibration: Some(7),
                reference: Reading { value: 7, maybe },
                previous: None,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let expected_ids = |predicate: &str| {
        sql.prepare(&format!(
            "SELECT id FROM readings WHERE {predicate} ORDER BY sample,id"
        ))
        .unwrap()
        .query_map([], |r| r.get::<_, u32>(0).map(u64::from))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    for (path, column) in [
        (REQUIRED, "reference"),
        (OPTIONAL_MEMBER, "maybe"),
        (OPTIONAL_PARENT, "previous"),
    ] {
        let expected = expected_ids(&format!("sample IS {column}"));
        for (left, right) in [(Measurement::SAMPLE, path), (path, Measurement::SAMPLE)] {
            let query = || {
                db.query::<Measurement>()
                    .filter_fields(left, Eq, right)
                    .order_asc(Measurement::SAMPLE)
            };
            let page = query().fetch().unwrap();
            assert_eq!(
                page.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
                expected
            );
            let indexed = query().using_index("sample,id", &[]).fetch();
            if path == REQUIRED {
                assert_eq!(page.examined, 2);
                assert_eq!(indexed.unwrap().rows[0].id, expected[0]);
            } else {
                assert!(matches!(indexed, Err(Error::InvalidQuery)));
            }
        }
    }

    for (op, path, fixed, predicate) in [
        (
            Eq,
            OPTIONAL_MEMBER,
            Some(7_u16),
            "sample IS maybe AND maybe IS 7",
        ),
        (
            ic_db::Comparison::Ne,
            OPTIONAL_PARENT,
            None,
            "sample IS NOT previous AND previous IS NULL",
        ),
    ] {
        let expected = expected_ids(predicate);
        for (left, right) in [(Measurement::SAMPLE, path), (path, Measurement::SAMPLE)] {
            let query = db
                .query::<Measurement>()
                .filter_fields(left, op, right)
                .order_asc(Measurement::SAMPLE);
            let query = match fixed {
                Some(value) => query.eq(path, value),
                None => query.filter_absent(path),
            };
            let page = query.work_limit(2).fetch().unwrap();
            assert_eq!(page.examined, 2);
            assert_eq!(
                page.rows.into_iter().map(|row| row.id).collect::<Vec<_>>(),
                expected
            );
        }
    }
    for (comparison, operator) in [(ic_db::Comparison::Le, "<="), (ic_db::Comparison::Ge, ">=")] {
        for (path, column) in [
            (Measurement::ID, "id"),
            (REQUIRED, "reference"),
            (OPTIONAL_MEMBER, "maybe"),
            (OPTIONAL_PARENT, "previous"),
        ] {
            let query = || {
                db.query::<Measurement>()
                    .filter_fields(path, comparison, path)
            };
            let expected = expected_ids(&format!("{column} {operator} {column}")).len() as u64;
            if path == REQUIRED || path == Measurement::ID {
                let count = query().work_limit(1).count().unwrap();
                assert_eq!(
                    (count.value, count.examined, count.fetched),
                    (expected, 0, 0)
                );
                assert!(matches!(
                    query().work_limit(1).count_validated(),
                    Err(Error::Limit)
                ));
            } else {
                assert_eq!(query().count().unwrap().value, expected);
            }
        }
        let query = || {
            db.query::<Measurement>().filter_fields(
                Measurement::SAMPLE,
                comparison,
                Measurement::SAMPLE,
            )
        };
        let expected = expected_ids(&format!("sample {operator} sample")).len() as u64;
        let count = query().count().unwrap();
        assert_eq!(
            (count.value, count.examined, count.fetched),
            (expected, 2, 0)
        );
        assert_eq!(query().count_validated().unwrap().value, expected);
        // The sparse index proves sample is present but says nothing about previous.value.
        let covered = query()
            .filter_fields(REQUIRED, comparison, REQUIRED)
            .using_index("sample,id", &[])
            .count()
            .unwrap();
        assert_eq!(
            (covered.value, covered.examined, covered.fetched),
            (expected, 2, 0)
        );
        let other = query()
            .filter_fields(OPTIONAL_PARENT, comparison, OPTIONAL_PARENT)
            .using_index("sample,id", &[])
            .count()
            .unwrap();
        assert_eq!((other.value, other.fetched), (0, 2));
    }
    let calibration_index = "calibration,sample,id";
    let prefix = [7_u16.into()];
    for (left, right) in [
        (Measurement::CALIBRATION, Measurement::SAMPLE),
        (Measurement::SAMPLE, Measurement::CALIBRATION),
    ] {
        let query = |op| db.query::<Measurement>().filter_fields(left, op, right);
        let matching = || query(Eq).using_index(calibration_index, &prefix);
        assert_eq!(
            matching()
                .fetch()
                .unwrap()
                .rows
                .into_iter()
                .map(|r| r.id)
                .collect::<Vec<_>>(),
            expected_ids("calibration = 7 AND sample IS calibration")
        );
        assert_eq!(matching().count().unwrap().value, 1);
        assert!(matches!(
            query(Eq).using_index(calibration_index, &[]).fetch(),
            Err(Error::InvalidQuery)
        ));
        // The prefix also matches the row with absent sample. Inequality must retain it.
        assert!(matches!(
            query(ic_db::Comparison::Ne)
                .using_index(calibration_index, &prefix)
                .fetch(),
            Err(Error::InvalidQuery)
        ));
    }
    let equal_sample = || {
        db.query::<Measurement>()
            .filter_fields(Measurement::SAMPLE, Eq, OPTIONAL_MEMBER)
            .order_asc(Measurement::SAMPLE)
    };
    let present_options = [
        (OPTIONAL_MEMBER, Eq, 7_u16.into()),
        (OPTIONAL_MEMBER, Eq, 8_u16.into()),
    ];
    for (query, guard) in [
        (
            equal_sample().filter_present(OPTIONAL_MEMBER),
            "maybe IS NOT NULL",
        ),
        (
            equal_sample().filter_any(&present_options),
            "maybe IN (7,8)",
        ),
    ] {
        let page = query.work_limit(2).fetch().unwrap();
        assert_eq!(page.examined, 2);
        assert_eq!(
            page.rows.into_iter().map(|row| row.id).collect::<Vec<_>>(),
            expected_ids(&format!("sample IS maybe AND {guard}"))
        );
    }
    // A non-equality against a present literal can still accept an absent member.
    let nullable = || equal_sample().ne(OPTIONAL_MEMBER, 8_u16);
    assert_eq!(
        nullable()
            .fetch()
            .unwrap()
            .rows
            .into_iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        expected_ids("sample IS maybe AND maybe IS NOT 8")
    );
    assert!(matches!(
        nullable().using_index("sample,id", &[]).fetch(),
        Err(Error::InvalidQuery)
    ));
    // An alternative fixing the other operand cannot exclude the both-absent row.
    let alternatives = [
        (OPTIONAL_MEMBER, Eq, 7_u16.into()),
        (Measurement::ID, Eq, 0_u64.into()),
    ];
    let query = || {
        db.query::<Measurement>()
            .filter_fields(Measurement::SAMPLE, Eq, OPTIONAL_MEMBER)
            .filter_any(&alternatives)
    };
    assert_eq!(query().count().unwrap().value, 2);
    assert!(matches!(
        query().using_index("sample,id", &[]).fetch(),
        Err(Error::InvalidQuery)
    ));
}
