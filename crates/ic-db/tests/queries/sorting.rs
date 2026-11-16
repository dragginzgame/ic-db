//! Complete sorting compared with independent Rust tuple/text order.
use super::{C, Entry, Summary, create, host, ids, row};
use ic_db::{Batch, Database, Entity, Error, Schema, entity};
use ic_storage::{StableKey, VectorMemory};

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
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["amount", "category"])
            .distinct()
            .fetch_as::<Summary>(),
        Err(Error::InvalidQuery)
    ));
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
fn complete_sort_matching_row_bound_is_independent_of_output_limit() {
    let runtime = host(VectorMemory::default());
    let mut db = create(&runtime, 1);
    let mut batch = Batch::new();
    for id in 9..=33 {
        batch.insert(&row(id)).unwrap();
    }
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["amount", "category"])
            .limit(1)
            .fetch(),
        Err(Error::Limit)
    ));
    let page = db
        .query::<Entry>()
        .filter("id", C::Le, 32_u64)
        .order_by(&["amount", "category"])
        .descending()
        .limit(1)
        .fetch()
        .unwrap();
    assert_eq!(ids(&page.rows), [32]);
    assert_eq!(page.examined, 32);
    assert!(page.next.is_none());
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
                label: Some("\0".repeat(2000)),
                payload: vec![0; 2000],
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<SortRow>().order_by(&["label"]).limit(1).fetch(),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
}
