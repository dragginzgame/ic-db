//! Complete sorting compared with independent Rust tuple/text order.
use super::{C, DecimalSale, Entry, OptionalPrice, Summary, create, decimal_sales, host, ids, row};
use ic_db::{
    Batch, Database, Entity, Error, Schema,
    SortDirection::{Ascending as Asc, Descending as Desc},
    entity,
};
use ic_storage::{StableKey, VectorMemory};

#[test]
fn decimal_sort_and_distinct_agree_with_integer_cents_and_grouping() {
    // Fixed-scale integer cents are independent of Decimal comparison/key encoding.
    let cents = [
        Some(100),
        None,
        Some(-100),
        Some(i128::MAX),
        Some(0),
        Some(101),
        Some(i128::MIN),
        Some(100),
        Some(-101),
        None,
        Some(10),
        Some(-10),
        Some(0),
        Some(1),
        Some(-1),
    ];
    let db = decimal_sales(&cents.map(|value| (0, value)));
    let coefficient =
        |value: Option<ic_db::Decimal>| value.map(|value| value.coefficient_at_scale(2).unwrap());
    for price_desc in [false, true] {
        for id_desc in [false, true] {
            let mut expected: Vec<_> = cents.iter().enumerate().collect();
            expected.sort_by(|(ai, a), (bi, b)| {
                let order = if price_desc { b.cmp(a) } else { a.cmp(b) };
                order.then_with(|| if id_desc { bi.cmp(ai) } else { ai.cmp(bi) })
            });
            let page = db
                .query::<DecimalSale>()
                .order_by_each(&[
                    ("price", if price_desc { Desc } else { Asc }),
                    ("id", if id_desc { Desc } else { Asc }),
                ])
                .fetch()
                .unwrap();
            assert_eq!(
                page.rows
                    .iter()
                    .map(|r| (r.id as usize, coefficient(r.price)))
                    .collect::<Vec<_>>(),
                expected
                    .iter()
                    .map(|(id, value)| (*id, **value))
                    .collect::<Vec<_>>()
            );
            assert_eq!((page.examined, page.fetched), (cents.len(), cents.len()));
            assert!(page.next.is_none());
        }
        let mut expected = cents.to_vec();
        expected.sort();
        expected.dedup();
        if price_desc {
            expected.reverse();
        }
        for limit in [1, 4, 32] {
            let page = db
                .query::<DecimalSale>()
                .order_by_each(&[("price", if price_desc { Desc } else { Asc })])
                .distinct()
                .limit(limit)
                .fetch_as::<OptionalPrice>()
                .unwrap();
            assert_eq!(
                page.rows
                    .iter()
                    .map(|r| coefficient(r.price))
                    .collect::<Vec<_>>(),
                expected.iter().take(limit).copied().collect::<Vec<_>>()
            );
            assert_eq!(page.examined, cents.len());
            assert!(page.next.is_none());
        }
    }
    let mut counts = std::collections::BTreeMap::new();
    for value in cents {
        *counts.entry(value).or_insert(0_u64) += 1;
    }
    let groups = db
        .query::<DecimalSale>()
        .group_count::<OptionalPrice>()
        .unwrap();
    assert_eq!(
        groups
            .value
            .iter()
            .map(|g| (coefficient(g.key.price), g.count))
            .collect::<Vec<_>>(),
        counts.into_iter().collect::<Vec<_>>()
    );
    for alias in ["1", "1.0", "1.00"] {
        let page = db
            .query::<DecimalSale>()
            .filter("price", C::Eq, alias.parse::<ic_db::Decimal>().unwrap())
            .order_by_each(&[("price", Desc), ("id", Desc)])
            .fetch()
            .unwrap();
        assert_eq!(page.rows.iter().map(|r| r.id).collect::<Vec<_>>(), [7, 0]);
    }
}

#[test]
fn complete_sort_matches_full_oracle_before_limit_and_projection() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = create(&runtime, 1);
    let mut input: Vec<_> = (1..=8).map(row).collect();
    let mut batch = Batch::new();
    for (row, amount) in input.iter_mut().zip([40, 0, 30, 10, 40, 20, 0, 99]) {
        row.amount = amount;
        batch.replace(row).unwrap();
    }
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    for descending in [false, true] {
        for limit in [1, 3, 32] {
            let mut expected: Vec<_> = input.iter().filter(|r| r.amount >= 10).collect();
            expected.sort_by_key(|r| (r.amount, r.category, r.id));
            if descending {
                expected.reverse();
            }
            expected.truncate(limit);
            let query = || {
                let query = db
                    .query::<Entry>()
                    .filter("amount", C::Ge, 10_u64)
                    .order_by(&["amount", "category"])
                    .limit(limit);
                if descending {
                    query.descending()
                } else {
                    query
                }
            };
            let page = query().fetch().unwrap();
            assert_eq!(
                ids(&page.rows),
                expected.iter().map(|r| r.id).collect::<Vec<_>>()
            );
            assert!(page.next.is_none());
            assert_eq!((page.examined, page.fetched), (8, 8));
            let projected = query().fetch_as::<Summary>().unwrap();
            assert_eq!(
                projected
                    .rows
                    .iter()
                    .map(|r| (r.id, r.amount))
                    .collect::<Vec<_>>(),
                expected
                    .iter()
                    .map(|r| (r.id, r.amount))
                    .collect::<Vec<_>>()
            );
            assert_eq!(projected.output_bytes, expected.len() * 16);
            assert!(projected.next.is_none());
        }
    }
    // Sparse optional indexes cannot omit absent matches; sort the complete primary scan.
    let mut expected: Vec<_> = input.iter().collect();
    expected.sort_by_key(|r| (r.maybe, r.id));
    let page = db
        .query::<Entry>()
        .order_by(&["maybe"])
        .fetch_as::<Summary>()
        .unwrap();
    assert_eq!(
        page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        expected.iter().map(|r| r.id).collect::<Vec<_>>()
    );
    assert_eq!(page.examined, 8);
    // Output bytes apply after sorting and selection, not to discarded candidates.
    assert_eq!(
        db.query::<Entry>()
            .order_by(&["amount", "category"])
            .descending()
            .limit(1)
            .byte_limit(16)
            .fetch_as::<Summary>()
            .unwrap()
            .rows[0]
            .id,
        8
    );
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["amount", "category"])
            .limit(1)
            .byte_limit(15)
            .fetch_as::<Summary>(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["amount", "category"])
            .limit(2)
            .byte_limit(16)
            .fetch_as::<Summary>(),
        Err(Error::Limit)
    ));
    // Even when the best result is already seen, work exhaustion cannot return a prefix.
    for work in [1, 8] {
        assert!(matches!(
            db.query::<Entry>()
                .order_by(&["amount", "category"])
                .limit(1)
                .work_limit(work)
                .fetch(),
            Err(Error::Limit)
        ));
    }
    let cursor = db.query::<Entry>().limit(1).fetch().unwrap().next.unwrap();
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["amount", "category"])
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(
        db.query::<Entry>()
            .order_by(&["amount", "category"])
            .distinct()
            .fetch_as::<Summary>()
            .unwrap()
            .rows
            .len(),
        8
    );
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["amount", "category"])
            .count(),
        Err(Error::InvalidQuery)
    ));
    assert!(
        db.query::<Entry>()
            .filter("id", C::Eq, 99_u64)
            .order_by(&["maybe"])
            .fetch()
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn bounded_selection_matches_full_sort_at_the_work_envelope() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = create(&runtime, 1);
    let input: Vec<_> = (1..=511)
        .map(|id| Entry {
            amount: (id * 37) % 101,
            ..row(id)
        })
        .collect();
    for chunk in input.chunks(32) {
        let mut batch = Batch::new();
        for row in chunk {
            if row.id <= 8 {
                batch.replace(row).unwrap();
            } else {
                batch.insert(row).unwrap();
            }
        }
        db.commit(batch).unwrap();
    }
    let before = backing.borrow().clone();
    for reverse in [false, true] {
        let mut expected: Vec<_> = input.iter().collect();
        expected.sort_by_key(|r| (r.category, std::cmp::Reverse(r.amount), r.id));
        if reverse {
            expected.reverse();
        }
        for limit in [1, 7, 32] {
            for distinct in [false, true] {
                let mut query = db
                    .query::<Entry>()
                    .order_by_each(&[("category", Asc), ("amount", Desc)])
                    .limit(limit)
                    .work_limit(512);
                if reverse {
                    query = query.descending();
                }
                if distinct {
                    query = query.distinct();
                }
                let page = query.fetch_as::<Summary>().unwrap();
                assert_eq!(
                    page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
                    expected
                        .iter()
                        .take(limit)
                        .map(|r| r.id)
                        .collect::<Vec<_>>()
                );
                assert_eq!((page.examined, page.fetched), (511, 511));
                assert!(page.next.is_none());
            }
        }
    }
    assert!(matches!(
        db.query::<Entry>()
            .order_by_each(&[("category", Asc), ("amount", Desc)])
            .limit(1)
            .work_limit(511)
            .fetch(),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    batch.insert(&row(512)).unwrap();
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<Entry>()
            .order_by_each(&[("category", Asc), ("amount", Desc)])
            .limit(1)
            .work_limit(512)
            .fetch(),
        Err(Error::Limit)
    ));
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "label", value(opt, item(prim = "Text", max_bytes = 4096))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 4096)))
    )
)]
struct SortRow {}

#[ic_db::record(fields(field(name = "label", value(opt, item(prim = "Text", max_bytes = 4096)))))]
struct LabelOnly {}

#[test]
fn complete_sort_uses_exact_text_keys_and_bounds_retained_buffers() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[SortRow::SCHEMA]).unwrap(),
        [1; 16],
    )
    .unwrap();
    let input = [
        Some("z"),
        None,
        Some("é"),
        Some(""),
        Some("a\0"),
        Some("a"),
        Some("a"),
    ];
    let mut batch = Batch::new();
    for (i, label) in input.iter().enumerate() {
        batch
            .insert(&SortRow {
                id: i as u64,
                label: label.map(str::to_owned),
                payload: vec![],
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let mut expected: Vec<_> = input.iter().enumerate().collect();
    expected.sort_by_key(|(id, value)| (*value, *id));
    for descending in [false, true] {
        let query = db.query::<SortRow>().order_by(&["label"]);
        let query = if descending {
            query.descending()
        } else {
            query
        };
        let page = query.fetch().unwrap();
        let mut ids: Vec<_> = expected.iter().map(|(id, _)| *id as u64).collect();
        if descending {
            ids.reverse();
        }
        assert_eq!(page.rows.iter().map(|r| r.id).collect::<Vec<_>>(), ids);
    }
    // Variable-length keys, embedded NUL and absence must reverse per component,
    // while equal labels keep the independently selected primary-key direction.
    for label_desc in [false, true] {
        for id_desc in [false, true] {
            let mut expected: Vec<_> = input.iter().enumerate().collect();
            expected.sort_by(|(ai, a), (bi, b)| {
                let label = if label_desc { b.cmp(a) } else { a.cmp(b) };
                label.then_with(|| if id_desc { bi.cmp(ai) } else { ai.cmp(bi) })
            });
            let page = db
                .query::<SortRow>()
                .order_by_each(&[
                    ("label", if label_desc { Desc } else { Asc }),
                    ("id", if id_desc { Desc } else { Asc }),
                ])
                .fetch()
                .unwrap();
            assert_eq!(
                page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
                expected
                    .iter()
                    .map(|(id, _)| *id as u64)
                    .collect::<Vec<_>>()
            );
        }
    }
    let mut expected = input.to_vec();
    expected.sort();
    expected.dedup();
    expected.reverse();
    let page = db
        .query::<SortRow>()
        .order_by_each(&[("label", Desc)])
        .distinct()
        .fetch_as::<LabelOnly>()
        .unwrap();
    assert_eq!(
        page.rows
            .iter()
            .map(|row| row.label.as_deref())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(page.examined, input.len());
    assert!(page.next.is_none());
    // A legal row can still exceed the independently bounded sort key.
    let mut batch = Batch::new();
    batch
        .insert(&SortRow {
            id: 20,
            label: Some("\0".repeat(2048)),
            payload: vec![],
        })
        .unwrap();
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<SortRow>().order_by(&["label"]).limit(1).fetch(),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    batch.delete::<SortRow>(20).unwrap();
    for id in 21..=40 {
        batch
            .insert(&SortRow {
                id,
                label: Some(format!("{}{id:03}", "\0".repeat(1990))),
                payload: vec![0; 2000],
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    // Each later long key wins. Replaced buffers must stop consuming the retained
    // budget, and comparisons must reach beyond their shared escaped-text prefix.
    assert_eq!(
        db.query::<SortRow>()
            .filter("id", C::Ge, 21_u64)
            .order_by(&["label"])
            .descending()
            .limit(1)
            .fetch()
            .unwrap()
            .rows[0]
            .id,
        40
    );
    assert_eq!(
        db.query::<SortRow>()
            .order_by(&["label"])
            .limit(1)
            .fetch()
            .unwrap()
            .rows[0]
            .id,
        1
    );
    assert!(matches!(
        db.query::<SortRow>().order_by(&["label"]).limit(32).fetch(),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn mixed_directions_sort_complete_results_and_retain_proven_live_paths() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = create(&runtime, 1);
    let mut input: Vec<_> = (1..=8).map(row).collect();
    let mut batch = Batch::new();
    for (row, amount) in input.iter_mut().zip([40, 0, 30, 10, 40, 20, 0, 99]) {
        row.amount = amount;
        batch.replace(row).unwrap();
    }
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    for reverse in [false, true] {
        let mut expected: Vec<_> = input.iter().collect();
        expected.sort_by_key(|r| (r.category, std::cmp::Reverse(r.amount), r.id));
        if reverse {
            expected.reverse();
        }
        let query = || {
            let query = db
                .query::<Entry>()
                .order_by_each(&[("category", Asc), ("amount", Desc)])
                .limit(3);
            if reverse { query.descending() } else { query }
        };
        let page = query().fetch().unwrap();
        assert_eq!(
            ids(&page.rows),
            expected.iter().take(3).map(|r| r.id).collect::<Vec<_>>()
        );
        assert_eq!(page.examined, 8);
        assert!(page.next.is_none());
        let page = query().fetch_as::<Summary>().unwrap();
        assert_eq!(
            page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            expected.iter().take(3).map(|r| r.id).collect::<Vec<_>>()
        );
        assert!(matches!(query().work_limit(8).fetch(), Err(Error::Limit)));
        assert!(matches!(
            query().using_index("category,amount,id", &[]).fetch(),
            Err(Error::InvalidQuery)
        ));
    }
    for order in [
        vec![],
        vec![("category", Asc), ("category", Desc)],
        vec![("id", Desc), ("amount", Asc)],
        vec![("missing", Desc)],
    ] {
        assert!(matches!(
            db.query::<Entry>().order_by_each(&order).fetch(),
            Err(Error::InvalidQuery)
        ));
    }
    assert!(matches!(
        db.query::<Entry>().order_by_each(&[("id", Asc); 5]).fetch(),
        Err(Error::Limit)
    ));
    // A fixed leading field contributes no direction; the varying amount/id
    // suffix can traverse its ordinary index backwards, including byte retries.
    let order = [("category", Asc), ("amount", Desc), ("id", Desc)];
    let mut expected: Vec<_> = input.iter().filter(|r| r.category == 1).collect();
    expected.sort_by_key(|r| std::cmp::Reverse((r.amount, r.id)));
    let mut cursor = None;
    let mut seen = vec![];
    for _ in 0..8 {
        let query = db
            .query::<Entry>()
            .filter("category", C::Eq, 1_u8)
            .order_by_each(&order)
            .limit(2)
            .byte_limit(16);
        let page = if let Some(cursor) = &cursor {
            query.after(cursor)
        } else {
            query
        }
        .fetch_as::<Summary>()
        .unwrap();
        seen.extend(page.rows.iter().map(|r| r.id));
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(seen, expected.iter().map(|r| r.id).collect::<Vec<_>>());
    assert!(cursor.is_none());
    let ascending = db
        .query::<Entry>()
        .order_by(&["id"])
        .limit(1)
        .fetch()
        .unwrap()
        .next
        .unwrap();
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .order_by_each(&[("id", Asc)])
            .after(&ascending)
            .limit(1)
            .fetch()
            .unwrap()
            .rows),
        [2]
    );
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .order_by_each(&[("id", Desc)])
            .descending()
            .limit(1)
            .fetch()
            .unwrap()
            .rows),
        [1]
    );
    let page = db
        .query::<Entry>()
        .order_by_each(&[("amount", Desc), ("id", Desc)])
        .limit(1)
        .fetch()
        .unwrap();
    let cursor = page.next.unwrap();
    assert!(matches!(
        db.query::<Entry>()
            .order_by_each(&[("amount", Asc), ("id", Asc)])
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
    // Replacing the order clears earlier per-field directions.
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .order_by_each(&order)
            .order_by(&["id"])
            .fetch()
            .unwrap()
            .rows),
        (1..=8).collect::<Vec<_>>()
    );
    assert_eq!(*backing.borrow(), before);
}

#[ic_db::record(fields(
    field(name = "category", value(item(prim = "Nat8"))),
    field(name = "amount", value(item(prim = "Nat64")))
))]
struct CategoryAmount {}

#[test]
fn sorted_distinct_selects_complete_tuples_before_limit_within_sort_bounds() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = create(&runtime, 1);
    let mut batch = Batch::new();
    let input: Vec<_> = (1..=8)
        .map(|id| {
            let mut row = row(id);
            row.category = (id % 2) as u8;
            row.amount = if id <= 6 { 10 } else { 20 };
            row
        })
        .collect();
    for row in &input {
        batch.replace(row).unwrap();
    }
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    for reverse in [false, true] {
        let mut expected: Vec<_> = input.iter().map(|r| (r.category, r.amount)).collect();
        expected.sort_by_key(|(category, amount)| (std::cmp::Reverse(*amount), *category));
        if reverse {
            expected.reverse();
        }
        expected.dedup();
        for limit in [1, 2, 32] {
            let query = db
                .query::<Entry>()
                .order_by_each(&[("amount", Desc), ("category", Asc)])
                .distinct()
                .limit(limit);
            let page = if reverse { query.descending() } else { query }
                .fetch_as::<CategoryAmount>()
                .unwrap();
            assert_eq!(
                page.rows
                    .iter()
                    .map(|r| (r.category, r.amount))
                    .collect::<Vec<_>>(),
                expected.iter().take(limit).copied().collect::<Vec<_>>()
            );
            assert!(page.next.is_none());
            assert_eq!(
                (page.examined, page.fetched, page.output_bytes),
                (8, 8, expected.len().min(limit) * 9)
            );
        }
    }
    let query = || {
        db.query::<Entry>()
            .order_by_each(&[("amount", Desc), ("category", Asc)])
            .distinct()
    };
    assert!(matches!(
        query().work_limit(8).fetch_as::<CategoryAmount>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        query().byte_limit(8).fetch_as::<CategoryAmount>(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        query().limit(2).byte_limit(9).fetch_as::<CategoryAmount>(),
        Err(Error::Limit)
    ));
    assert_eq!(
        query()
            .limit(1)
            .byte_limit(9)
            .fetch_as::<CategoryAmount>()
            .unwrap()
            .rows
            .len(),
        1
    );
    // Hidden ordering values and incomplete tuple order remain ambiguous/refused.
    assert!(matches!(
        db.query::<Entry>()
            .order_by_each(&[("amount", Desc)])
            .distinct()
            .fetch_as::<CategoryAmount>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["amount", "category", "id"])
            .distinct()
            .fetch_as::<CategoryAmount>(),
        Err(Error::InvalidQuery)
    ));
    let cursor = db.query::<Entry>().limit(1).fetch().unwrap().next.unwrap();
    assert!(matches!(
        query().after(&cursor).fetch_as::<CategoryAmount>(),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    for id in 9..=33 {
        let mut row = row(id);
        row.category = 0;
        row.amount = 10;
        batch.insert(&row).unwrap();
    }
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    // Duplicate input no longer consumes a retained-row slot.
    let page = db
        .query::<Entry>()
        .order_by_each(&[("amount", Desc), ("category", Asc)])
        .distinct()
        .limit(1)
        .fetch_as::<CategoryAmount>()
        .unwrap();
    assert_eq!(page.examined, 33);
    assert_eq!((page.rows[0].category, page.rows[0].amount), (0, 20));
    assert!(page.next.is_none());
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn sorted_distinct_bounds_unique_tuples_not_matching_rows_or_output_limit() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = create(&runtime, 1);
    let rows: Vec<_> = (1..=128)
        .map(|id| Entry {
            category: 0,
            amount: (id - 1) % 32,
            ..row(id)
        })
        .collect();
    for chunk in rows.chunks(32) {
        let mut batch = Batch::new();
        for row in chunk {
            if row.id <= 8 {
                batch.replace(row).unwrap();
            } else {
                batch.insert(row).unwrap();
            }
        }
        db.commit(batch).unwrap();
    }
    let before = backing.borrow().clone();
    for reverse in [false, true] {
        for limit in [1, 7, 32] {
            let query = db
                .query::<Entry>()
                .order_by_each(&[("amount", Desc), ("category", Asc)])
                .distinct()
                .limit(limit)
                .byte_limit(limit * 9)
                .work_limit(129);
            let page = if reverse { query.descending() } else { query }
                .fetch_as::<CategoryAmount>()
                .unwrap();
            let expected: Vec<_> = (0..32)
                .map(|n| if reverse { n } else { 31 - n })
                .take(limit)
                .map(|amount| (0, amount))
                .collect();
            assert_eq!(
                page.rows
                    .iter()
                    .map(|r| (r.category, r.amount))
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(
                (page.examined, page.fetched, page.output_bytes),
                (128, 128, limit * 9)
            );
            assert!(page.next.is_none());
        }
    }
    assert!(matches!(
        db.query::<Entry>()
            .order_by_each(&[("amount", Desc), ("category", Asc)])
            .distinct()
            .limit(1)
            .work_limit(128)
            .fetch_as::<CategoryAmount>(),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut extra = row(128);
    extra.category = 0;
    extra.amount = 32;
    let mut batch = Batch::new();
    batch.replace(&extra).unwrap();
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Entry>()
            .order_by_each(&[("amount", Desc), ("category", Asc)])
            .distinct()
            .limit(1)
            .work_limit(129)
            .fetch_as::<CategoryAmount>(),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
}
