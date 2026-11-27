//! Cursor boundary, progress and output-budget regressions.

use super::{
    C, Database, Entry, Error, IndexValue, Other, QueryCursor, StableKey, TextRow, VectorMemory,
    create, host, ids, row, text_rows,
};
use ic_db::design::prelude::*;

#[test]
fn secondary_ranges_intersect_valid_cursors_outside_the_selected_value_classes() {
    let runtime = host(VectorMemory::default());
    let (db, input) = text_rows(&runtime);
    let query = || {
        db.query::<TextRow>()
            .using_index("text,id", &[])
            .filter("text", C::Ge, "a")
            .filter("text", C::Le, "ab")
    };
    for descending in [false, true] {
        let mut q = query().limit(1);
        if descending {
            q = q.descending();
        }
        let target = q.fetch().unwrap().next.unwrap().to_bytes();
        for after_range in [false, true] {
            let mut donor = db.query::<TextRow>().using_index("text,id", &[]).limit(1);
            if after_range {
                donor = donor.descending();
            }
            let donor = donor.fetch().unwrap().next.unwrap().to_bytes();
            for inclusive in [false, true] {
                // Keep the admitted query identity but choose a valid logical position
                // from the same index, as an older full-prefix scan could have done.
                let mut bytes = target[..34].to_vec();
                bytes[1] = u8::from(inclusive);
                bytes.extend_from_slice(&donor[34..]);
                let cursor = QueryCursor::from_bytes(&bytes).unwrap();
                let mut q = query().after(&cursor);
                if descending {
                    q = q.descending();
                }
                let page = q.fetch().unwrap();
                let mut expected: Vec<_> = input
                    .iter()
                    .filter(|r| {
                        r.text.as_deref().is_some_and(|text| {
                            after_range == descending && ("a"..="ab").contains(&text)
                        })
                    })
                    .collect();
                expected.sort_by_key(|r| (&r.text, r.id));
                if descending {
                    expected.reverse();
                }
                assert_eq!(
                    page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
                    expected.iter().map(|r| r.id).collect::<Vec<_>>()
                );
                assert_eq!(page.examined, expected.len());
                assert!(page.next.is_none());
            }
        }
    }
}

#[test]
fn primary_range_pages_clamp_cursors_and_retry_byte_limited_rows() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    let query = || {
        db.query::<Entry>()
            .filter("id", C::Ge, 3_u64)
            .filter("id", C::Le, 6_u64)
    };
    for descending in [false, true] {
        let mut cursor = None;
        let mut actual = Vec::new();
        for _ in 0..5 {
            let mut q = query().work_limit(1);
            if descending {
                q = q.descending();
            }
            if let Some(c) = &cursor {
                q = q.after(c);
            }
            let page = q.fetch().unwrap();
            if page
                .rows
                .last()
                .is_some_and(|r| r.id == if descending { 3 } else { 6 })
            {
                assert!(
                    page.next.is_none(),
                    "processed the inclusive end of the range"
                );
            }
            actual.extend(ids(&page.rows));
            assert_eq!(page.examined, page.rows.len());
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        assert!(cursor.is_none());
        assert_eq!(
            actual,
            if descending {
                vec![6, 5, 4, 3]
            } else {
                vec![3, 4, 5, 6]
            }
        );
        let mut q = query().test_output_limit(row(1).encode().unwrap().len());
        if descending {
            q = q.descending();
        }
        let first = q.fetch().unwrap();
        assert_eq!(first.rows.len(), 1);
        let cursor = first.next.unwrap();
        assert_eq!(cursor.to_bytes()[1], 1, "retry the unreturned candidate");
        let mut q = query().after(&cursor);
        if descending {
            q = q.descending();
        }
        let rest = q.fetch().unwrap();
        let mut actual = ids(&first.rows);
        actual.extend(ids(&rest.rows));
        assert_eq!(
            actual,
            if descending {
                vec![6, 5, 4, 3]
            } else {
                vec![3, 4, 5, 6]
            }
        );

        // Valid logical positions from an older full scan, or chosen by a caller,
        // may be outside the newly narrowed interval. They cannot widen it.
        for inclusive in [false, true] {
            for position in [0_u64, 9_u64] {
                let mut bytes = cursor.to_bytes();
                bytes[1] = u8::from(inclusive);
                let offset = bytes.len() - 8;
                bytes[offset..].copy_from_slice(&position.to_be_bytes());
                let cursor = QueryCursor::from_bytes(&bytes).unwrap();
                let mut q = query().after(&cursor);
                if descending {
                    q = q.descending();
                }
                let page = q.fetch().unwrap();
                let count = if (position == 0) != descending { 4 } else { 0 };
                assert_eq!((page.rows.len(), page.examined), (count, count));
                assert!(page.next.is_none());
            }
        }
    }
}

#[test]
fn sparse_index_pages_progress_in_both_directions() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for descending in [false, true] {
        let mut cursor = None;
        let mut all = Vec::new();
        let mut empty = 0;
        let mut examined = 0;
        loop {
            let mut query = db
                .query::<Entry>()
                .using_index("category", &[1_u8.into()])
                .filter("amount", C::Ge, 50_u64)
                .limit(2)
                .work_limit(1);
            if descending {
                query = query.descending();
            }
            if let Some(c) = &cursor {
                query = query.after(c);
            }
            let page = query.fetch().unwrap();
            assert!(page.examined <= 1);
            assert_eq!(page.examined, page.fetched);
            examined += page.examined;
            if page.rows.is_empty() && page.next.is_some() {
                empty += 1;
            }
            all.extend(ids(&page.rows));
            cursor = page
                .next
                .map(|c| QueryCursor::from_bytes(&c.to_bytes()).unwrap());
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(all, if descending { vec![7, 5] } else { vec![5, 7] });
        assert_eq!(empty, 2);
        assert_eq!(examined, 4);
    }
    let page = db
        .query::<Entry>()
        .filter("maybe", C::Eq, IndexValue::Absent)
        .fetch()
        .unwrap();
    assert_eq!(ids(&page.rows), [3, 6]);
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .filter("maybe", C::Ne, IndexValue::Absent)
            .fetch()
            .unwrap()
            .rows),
        [1, 2, 4, 5, 7, 8]
    );
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .filter("maybe", C::Le, 4_u64)
            .fetch()
            .unwrap()
            .rows),
        [1, 2, 4]
    );
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .filter("maybe", C::Lt, 4_u64)
            .fetch()
            .unwrap()
            .rows),
        [1, 2]
    );
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .using_index("maybe", &[4_u64.into()])
            .fetch()
            .unwrap()
            .rows),
        [4]
    );
    assert!(matches!(
        db.query::<Entry>().using_index("maybe", &[]).fetch(),
        Err(Error::InvalidQuery)
    ));
    // Duplicate secondary values use the primary owner suffix as a tie-breaker.
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .using_index("category", &[1_u8.into()])
            .descending()
            .fetch()
            .unwrap()
            .rows),
        [7, 5, 3, 1]
    );
}

#[test]
fn output_budget_retries_the_matching_candidate_without_skipping_it() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 2);
    let size = row(1).encode().unwrap().len();
    let first = db
        .query::<Entry>()
        .test_output_limit(size * 2 - 1)
        .fetch()
        .unwrap();
    assert_eq!(ids(&first.rows), [1]);
    assert_eq!(first.examined, 2);
    assert_eq!(first.output_bytes, size);
    let cursor = first.next.unwrap();
    let next = db
        .query::<Entry>()
        .after(&cursor)
        .test_output_limit(4096)
        .fetch()
        .unwrap();
    assert_eq!(ids(&next.rows), [2, 3, 4, 5, 6, 7, 8]);
    assert!(next.next.is_none());
    assert!(matches!(
        db.query::<Entry>().test_output_limit(size - 1).fetch(),
        Err(Error::RowTooLarge)
    ));
    let none = db
        .query::<Entry>()
        .filter("id", C::Gt, 20_u64)
        .test_output_limit(1)
        .fetch()
        .unwrap();
    assert!(none.rows.is_empty());
    assert!(none.next.is_none());
    let first = db
        .query::<Entry>()
        .descending()
        .test_output_limit(size * 2 - 1)
        .fetch()
        .unwrap();
    assert_eq!(ids(&first.rows), [8]);
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .descending()
            .after(first.next.as_ref().unwrap())
            .fetch()
            .unwrap()
            .rows),
        [7, 6, 5, 4, 3, 2, 1]
    );
}

#[test]
fn admission_and_cursors_refuse_before_data_access() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 3);
    let cursor = db.query::<Entry>().limit(1).fetch().unwrap().next.unwrap();
    assert!(matches!(
        db.query::<Entry>().descending().after(&cursor).fetch(),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter("amount", C::Ge, 0_u64)
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .using_index("amount,id", &[])
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        db.query::<Other>().after(&cursor).fetch(),
        Err(Error::InvalidCursor)
    ));
    let other_runtime = host(VectorMemory::default());
    let other = create(&other_runtime, 4);
    assert!(matches!(
        other.query::<Entry>().after(&cursor).fetch(),
        Err(Error::InvalidCursor)
    ));
    let changed_runtime = host(VectorMemory::default());
    let changed = Database::create(
        changed_runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Entry::SCHEMA]).unwrap(),
        [3; 16],
    )
    .unwrap();
    assert!(matches!(
        changed.query::<Entry>().after(&cursor).fetch(),
        Err(Error::InvalidCursor)
    ));
    for offset in [0, 1, 2, 34] {
        let mut bytes = cursor.to_bytes();
        bytes[offset] = 255;
        if let Ok(bad) = QueryCursor::from_bytes(&bytes) {
            assert!(matches!(
                db.query::<Entry>().after(&bad).fetch(),
                Err(Error::InvalidCursor)
            ));
        }
    }
    for size in [0, 1, 34, 547, 1000] {
        assert!(QueryCursor::from_bytes(&vec![0; size]).is_err());
    }
    let mut trailing = cursor.to_bytes();
    trailing.push(0);
    assert!(matches!(
        db.query::<Entry>()
            .after(&QueryCursor::from_bytes(&trailing).unwrap())
            .fetch(),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter("id", C::Eq, 0_u64)
            .filter("amount", C::Eq, 1_u8)
            .fetch(),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.query::<Entry>().filter("payload", C::Eq, "x").fetch(),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.query::<Entry>().filter("missing", C::Eq, 1_u64).fetch(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter("maybe", C::Lt, IndexValue::Absent)
            .fetch(),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.query::<Entry>().limit(0).fetch(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>().work_limit(513).fetch(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>().test_output_limit(0).fetch(),
        Err(Error::Limit)
    ));
    let mut query = db.query::<Entry>();
    for _ in 0..9 {
        query = query.filter("id", C::Eq, 1_u64);
    }
    assert!(matches!(query.fetch(), Err(Error::Limit)));
}
