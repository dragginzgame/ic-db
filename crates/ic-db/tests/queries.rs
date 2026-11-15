//! Fluent reads: independent result expectations, cursor bindings and bounded progress.
use ic_db::{
    Batch, Comparison as C, Database, Entity, Error, IndexValue, QueryCursor, Schema, entity,
};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

#[entity(pk(field="id"),index(fields=["category","amount","id"]),index(fields=["amount","id"]),
index(field="category"),
index(field="maybe",unique,present_only),fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="category",value(item(prim="Nat8"))),
    field(name="amount",value(item(prim="Nat64"))),
    field(name="maybe",value(opt,item(prim="Nat64"))),
    field(name="payload",value(item(prim="Blob",max_bytes=2048)))
))]
struct Entry {}
#[entity(
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
struct Other {}
#[ic_db::record(fields(
    field(name = "id", value(item(prim = "Nat64"))),
    field(name = "amount", value(item(prim = "Nat64")))
))]
struct Summary {}
#[ic_db::record(fields(field(name = "maybe", value(opt, item(prim = "Nat64")))))]
struct OptionalOnly {}
#[ic_db::record(fields(field(name = "payload", value(item(prim = "Blob", max_bytes = 2048)))))]
struct PayloadOnly {}
#[ic_db::record(fields(field(name = "id", value(item(prim = "Nat32")))))]
struct WrongWidth {}
#[ic_db::record(fields(field(name = "maybe", value(item(prim = "Nat64")))))]
struct WrongPresence {}
#[ic_db::record(fields(field(name = "payload", value(item(prim = "Blob", max_bytes = 1024)))))]
struct WrongBound {}
#[ic_db::record(fields(field(name = "missing", value(item(prim = "Nat64")))))]
struct Missing {}
#[ic_db::record(fields())]
struct Empty {}
fn host(backing: VectorMemory) -> MemoryRuntime<VectorMemory> {
    let mut host = MemoryRuntime::new(backing).unwrap();
    let req =
        SealedDeclarationSnapshot::new(vec![MemoryRequest::new("test", "test.db.v1").unwrap()])
            .unwrap();
    let grants =
        AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap();
    host.bootstrap(&req, &grants, &GenericAllocationPolicy)
        .unwrap();
    host
}
fn row(id: u64) -> Entry {
    Entry {
        id,
        category: (id % 2) as u8,
        amount: id * 10,
        maybe: if id.is_multiple_of(3) { None } else { Some(id) },
        payload: vec![7; 100],
    }
}
fn create(runtime: &MemoryRuntime<VectorMemory>, incarnation: u8) -> Database<VectorMemory> {
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Entry::SCHEMA, Other::SCHEMA]).unwrap(),
        [incarnation; 16],
    )
    .unwrap();
    let mut b = Batch::new();
    for id in 1..=8 {
        b.insert(&row(id)).unwrap();
    }
    db.commit(b).unwrap();
    db
}
fn ids(rows: &[Entry]) -> Vec<u64> {
    rows.iter().map(|r| r.id).collect()
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
                .using_index("category,amount,id", &[1_u8.into()])
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
        .byte_limit(size * 2 - 1)
        .fetch()
        .unwrap();
    assert_eq!(ids(&first.rows), [1]);
    assert_eq!(first.examined, 2);
    assert_eq!(first.output_bytes, size);
    let cursor = first.next.unwrap();
    let next = db
        .query::<Entry>()
        .after(&cursor)
        .byte_limit(4096)
        .fetch()
        .unwrap();
    assert_eq!(ids(&next.rows), [2, 3, 4, 5, 6, 7, 8]);
    assert!(next.next.is_none());
    assert!(matches!(
        db.query::<Entry>().byte_limit(size - 1).fetch(),
        Err(Error::RowTooLarge)
    ));
    let none = db
        .query::<Entry>()
        .filter("id", C::Gt, 20_u64)
        .byte_limit(1)
        .fetch()
        .unwrap();
    assert!(none.rows.is_empty());
    assert!(none.next.is_none());
    let first = db
        .query::<Entry>()
        .descending()
        .byte_limit(size * 2 - 1)
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
        Err(Error::InvalidQuery)
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
        db.query::<Entry>().byte_limit(0).fetch(),
        Err(Error::Limit)
    ));
    let mut query = db.query::<Entry>();
    for _ in 0..9 {
        query = query.filter("id", C::Eq, 1_u64);
    }
    assert!(matches!(query.fetch(), Err(Error::Limit)));
}

#[test]
fn live_index_updates_have_explicit_duplicate_and_omission_behavior() {
    let runtime = host(VectorMemory::default());
    let mut db = create(&runtime, 5);
    let cursor = db
        .query::<Entry>()
        .using_index("amount,id", &[])
        .limit(2)
        .fetch()
        .unwrap()
        .next
        .unwrap();
    let mut first = row(1);
    first.amount = 85;
    let mut eighth = row(8);
    eighth.amount = 5;
    let mut ninth = row(9);
    ninth.amount = 25;
    let mut b = Batch::new();
    b.replace(&first).unwrap();
    b.replace(&eighth).unwrap();
    b.insert(&ninth).unwrap();
    b.delete::<Entry>(3).unwrap();
    // Deleting the cursor row is allowed; continuation is a logical position.
    b.delete::<Entry>(2).unwrap();
    db.commit(b).unwrap();
    let page = db
        .query::<Entry>()
        .using_index("amount,id", &[])
        .after(&cursor)
        .fetch()
        .unwrap();
    assert_eq!(ids(&page.rows), [9, 4, 5, 6, 7, 1]);
    assert!(page.next.is_none());
}

#[test]
fn unselected_corruption_is_not_hidden_by_a_false_predicate() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let db = create(&runtime, 6);
    drop(db);
    let slot = StableKey::parse("test.db.v1").unwrap();
    let mut raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap()).unwrap();
    let mut key = vec![2];
    key.extend(Entry::ID.to_be_bytes());
    key.extend(1_u64.to_be_bytes());
    let mut bytes = row(1).encode().unwrap();
    bytes.push(0);
    raw.insert(key, bytes);
    drop(raw);
    let before = backing.borrow().clone();
    let db = Database::open(
        runtime.open_memory(&slot).unwrap(),
        Schema::new(&[Entry::SCHEMA, Other::SCHEMA]).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        db.query::<Entry>().filter("id", C::Eq, 0_u64).fetch(),
        Err(Error::Corrupt)
    ));
    assert!(matches!(
        db.query::<Entry>().fetch_as::<Summary>(),
        Err(Error::Corrupt)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn projection_uses_result_bytes_and_preserves_bounded_continuation() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let db = create(&runtime, 7);
    let before = backing.borrow().clone();
    for descending in [false, true] {
        let mut cursor = None;
        let mut rows = Vec::new();
        loop {
            let mut query = db
                .query::<Entry>()
                .using_index("category,amount,id", &[1_u8.into()])
                .filter("amount", C::Ge, 50_u64)
                .byte_limit(31)
                .work_limit(2);
            if descending {
                query = query.descending();
            }
            if let Some(cursor) = &cursor {
                query = query.after(cursor);
            }
            let page = query.fetch_as::<Summary>().unwrap();
            assert!(page.examined <= 2);
            assert_eq!(page.output_bytes, 16 * page.rows.len());
            rows.extend(page.rows.into_iter().map(|row| (row.id, row.amount)));
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(
            rows,
            if descending {
                vec![(7, 70), (5, 50)]
            } else {
                vec![(5, 50), (7, 70)]
            }
        );
    }
    assert!(matches!(
        db.query::<Entry>().byte_limit(15).fetch_as::<Summary>(),
        Err(Error::RowTooLarge)
    ));
    let page = db
        .query::<Entry>()
        .byte_limit(31)
        .fetch_as::<Summary>()
        .unwrap();
    assert_eq!(
        (page.rows[0].id, page.examined, page.output_bytes),
        (1, 2, 16)
    );
    let cursor = page.next.unwrap();
    let page = db
        .query::<Entry>()
        .after(&cursor)
        .fetch_as::<Summary>()
        .unwrap();
    assert_eq!(
        page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        (2..=8).collect::<Vec<_>>()
    );
    assert!(matches!(
        db.query::<Entry>().after(&cursor).fetch(),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .after(&cursor)
            .fetch_as::<OptionalOnly>(),
        Err(Error::InvalidCursor)
    ));
    let absent = db
        .query::<Entry>()
        .filter("maybe", C::Eq, IndexValue::Absent)
        .byte_limit(1)
        .fetch_as::<OptionalOnly>()
        .unwrap();
    assert_eq!(absent.rows, [OptionalOnly { maybe: None }]);
    assert_eq!(absent.output_bytes, 1);
    let present = db
        .query::<Entry>()
        .limit(1)
        .fetch_as::<OptionalOnly>()
        .unwrap();
    assert_eq!(present.rows, [OptionalOnly { maybe: Some(1) }]);
    let blob = db
        .query::<Entry>()
        .limit(1)
        .fetch_as::<PayloadOnly>()
        .unwrap();
    assert_eq!(blob.rows[0].payload, vec![7; 100]);
    assert_eq!(blob.output_bytes, 104);
    // Admission happens even if no row could match; declarations do not coerce.
    assert!(matches!(
        db.query::<Entry>()
            .filter("id", C::Eq, 0_u64)
            .fetch_as::<WrongWidth>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().fetch_as::<WrongPresence>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().fetch_as::<WrongBound>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().fetch_as::<Missing>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().fetch_as::<Empty>(),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[ic_db::record(
    name = "Details",
    fields(field(
        name = "labels",
        value(many, max_items = 3, item(prim = "Text", max_bytes = 8))
    ))
)]
struct Details {}
#[ic_db::record(
    name = "Details",
    fields(field(
        name = "labels",
        value(many, max_items = 3, item(prim = "Text", max_bytes = 9))
    ))
)]
struct OtherDetails {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "details", value(opt, item(is = "Details"))),
        field(name = "unused", value(item(prim = "Blob", max_bytes = 2048)))
    )
)]
struct Rich {}
#[ic_db::record(fields(field(name = "details", value(opt, item(is = "Details")))))]
struct RichOutput {}
#[ic_db::record(fields(field(name = "details", value(opt, item(is = "OtherDetails")))))]
struct IncompatibleOutput {}

#[test]
fn projection_checks_transitive_types_and_decodes_nested_values() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Rich::SCHEMA]).unwrap(),
        [8; 16],
    )
    .unwrap();
    let details = Details {
        labels: vec!["héllo".into(), "".into()],
    };
    let mut batch = Batch::new();
    batch
        .insert(&Rich {
            id: 1,
            details: Some(details.clone()),
            unused: vec![255; 2048],
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<Rich>().fetch_as::<IncompatibleOutput>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Rich>().fetch_as::<Details>(),
        Err(Error::InvalidQuery)
    ));
    let page = db
        .query::<Rich>()
        .byte_limit(19)
        .fetch_as::<RichOutput>()
        .unwrap();
    assert_eq!(
        page.rows,
        [RichOutput {
            details: Some(details)
        }]
    );
    assert_eq!(page.output_bytes, 19); // presence, count, two lengths, UTF-8 bytes
}

#[test]
fn automatic_order_matches_a_full_sort_with_ties_and_sparse_pages() {
    let runtime = host(VectorMemory::default());
    let mut db = create(&runtime, 9);
    let mut input: Vec<_> = (1..=8).map(row).collect();
    let mut batch = Batch::new();
    for row in &mut input {
        row.amount = (row.id * 7 % 5) * 10;
        batch.replace(row).unwrap();
    }
    db.commit(batch).unwrap();
    for descending in [false, true] {
        for category in [None, Some(1_u8)] {
            let mut expected: Vec<_> = input
                .iter()
                .filter(|r| category.is_none_or(|c| r.category == c) && r.amount >= 10)
                .map(|r| (r.amount, r.id))
                .collect();
            expected.sort();
            if descending {
                expected.reverse();
            }
            let mut cursor = None;
            let mut actual = Vec::new();
            let mut examined = 0;
            loop {
                let mut query = db
                    .query::<Entry>()
                    .order_by(&["amount"])
                    .filter("amount", C::Ge, 10_u64)
                    .limit(2)
                    .work_limit(1)
                    .byte_limit(16);
                if let Some(category) = category {
                    query = query.filter("category", C::Eq, category);
                }
                if descending {
                    query = query.descending();
                }
                if let Some(cursor) = &cursor {
                    query = query.after(cursor);
                }
                let page = query.fetch_as::<Summary>().unwrap();
                assert!(page.examined <= 1);
                examined += page.examined;
                actual.extend(page.rows.into_iter().map(|r| (r.amount, r.id)));
                cursor = page.next;
                if cursor.is_none() {
                    break;
                }
            }
            assert_eq!(actual, expected);
            // Equality-prefix selection visits only that category, not eight rows.
            assert_eq!(examined, if category.is_some() { 4 } else { 8 });
        }
    }
    for order in [&["category"][..], &["category", "amount", "id"][..]] {
        let page = db.query::<Entry>().order_by(order).fetch().unwrap();
        let mut expected: Vec<_> = input
            .iter()
            .map(|r| {
                (
                    r.category,
                    if order.len() == 1 { 0 } else { r.amount },
                    r.id,
                )
            })
            .collect();
        expected.sort();
        assert_eq!(
            ids(&page.rows),
            expected.iter().map(|r| r.2).collect::<Vec<_>>()
        );
    }
    // A fixed middle component does not disturb the requested order.
    let page = db
        .query::<Entry>()
        .using_index("category,amount,id", &[])
        .filter("amount", C::Eq, 20_u64)
        .order_by(&["category"])
        .fetch()
        .unwrap();
    assert_eq!(ids(&page.rows), [6, 1]);
    // Method order has no mathematical meaning; filter and output order compose.
    let a = db
        .query::<Entry>()
        .filter("category", C::Eq, 1_u8)
        .order_by(&["amount"])
        .fetch()
        .unwrap();
    let b = db
        .query::<Entry>()
        .order_by(&["amount"])
        .filter("category", C::Eq, 1_u8)
        .fetch()
        .unwrap();
    assert_eq!(ids(&a.rows), ids(&b.rows));
}

#[test]
fn ordering_refuses_incomplete_paths_and_binds_continuations() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 10);
    for order in [
        &[][..],
        &["missing"],
        &["payload"],
        &["amount", "amount"],
        &["id", "amount"],
        &["amount", "category"],
        &["maybe"],
    ] {
        assert!(matches!(
            db.query::<Entry>()
                .order_by(order)
                .limit(1)
                .work_limit(1)
                .fetch(),
            Err(Error::InvalidQuery)
        ));
    }
    assert!(matches!(
        db.query::<Entry>().order_by(&["id"; 5]).fetch(),
        Err(Error::Limit)
    ));
    // Taking a category-index prefix and sorting it by amount is not global order.
    assert!(matches!(
        db.query::<Entry>()
            .using_index("category", &[])
            .order_by(&["amount"])
            .limit(1)
            .work_limit(1)
            .fetch(),
        Err(Error::InvalidQuery)
    ));
    // Every operand is validated even when a contradiction or fixed PK permits any order.
    assert!(matches!(
        db.query::<Entry>()
            .filter("id", C::Eq, 1_u64)
            .filter("amount", C::Eq, 1_u8)
            .order_by(&["amount"])
            .fetch(),
        Err(Error::InvalidOperand)
    ));
    let first = db
        .query::<Entry>()
        .order_by(&["amount"])
        .limit(1)
        .fetch()
        .unwrap();
    let cursor = first.next.unwrap();
    assert!(matches!(
        db.query::<Entry>()
            .using_index("amount,id", &[])
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["amount", "id"])
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["amount"])
            .descending()
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .order_by(&["amount"])
            .after(&cursor)
            .fetch()
            .unwrap()
            .rows),
        [2, 3, 4, 5, 6, 7, 8]
    );
    // An explicit equivalent physical path is accepted after the same order proof.
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .using_index("amount,id", &[])
            .order_by(&["amount"])
            .after(&cursor)
            .fetch()
            .unwrap()
            .rows),
        [2, 3, 4, 5, 6, 7, 8]
    );
    // Presence-only indexes cannot silently exclude absent rows. A fixed optional
    // value is safe (and primary traversal is sufficient for this requested order).
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .filter("maybe", C::Eq, 2_u64)
            .order_by(&["maybe"])
            .fetch()
            .unwrap()
            .rows),
        [2]
    );
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .filter("maybe", C::Eq, IndexValue::Absent)
            .order_by(&["maybe"])
            .fetch()
            .unwrap()
            .rows),
        [3, 6]
    );
}

#[entity(
    pk(field = "id"),
    index(field = "value"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(opt, item(prim = "Nat64")))
    )
)]
struct OptionalOrdered {}

#[test]
fn automatic_optional_order_retains_absence_and_primary_ties() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[OptionalOrdered::SCHEMA]).unwrap(),
        [11; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (id, value) in [
        (1, Some(2)),
        (2, None),
        (3, Some(0)),
        (4, Some(2)),
        (5, None),
    ] {
        batch.insert(&OptionalOrdered { id, value }).unwrap();
    }
    db.commit(batch).unwrap();
    for descending in [false, true] {
        let mut cursor = None;
        let mut ids = Vec::new();
        loop {
            let mut query = db.query::<OptionalOrdered>().order_by(&["value"]).limit(1);
            if descending {
                query = query.descending();
            }
            if let Some(cursor) = &cursor {
                query = query.after(cursor);
            }
            let page = query.fetch().unwrap();
            ids.extend(page.rows.into_iter().map(|r| r.id));
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(
            ids,
            if descending {
                vec![4, 1, 3, 5, 2]
            } else {
                vec![2, 5, 3, 1, 4]
            }
        );
    }
}
