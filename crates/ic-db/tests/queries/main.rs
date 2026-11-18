//! Fluent reads: independent result expectations, cursor bindings and bounded progress.
mod blobs;
mod sorting;
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
fn field_comparisons_use_both_row_values_and_bind_the_right_field() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for (left, comparison, right, expected) in [
        ("maybe", C::Eq, "id", vec![1, 2, 4, 5, 7, 8]),
        ("id", C::Ne, "maybe", vec![3, 6]),
        ("maybe", C::Eq, "maybe", (1..=8).collect()),
        ("maybe", C::Lt, "amount", vec![1, 2, 4, 5, 7, 8]),
        ("amount", C::Gt, "maybe", vec![1, 2, 4, 5, 7, 8]),
        ("id", C::Le, "maybe", vec![1, 2, 4, 5, 7, 8]),
        ("maybe", C::Ge, "id", vec![1, 2, 4, 5, 7, 8]),
    ] {
        let query = || db.query::<Entry>().filter_fields(left, comparison, right);
        assert_eq!(ids(&query().fetch().unwrap().rows), expected);
        assert_eq!(query().count().unwrap().value, expected.len() as u64);
    }
    let query = |right| {
        db.query::<Entry>()
            .filter_fields("id", C::Eq, right)
            .work_limit(1)
    };
    let cursor = query("maybe").fetch().unwrap().next.unwrap();
    assert_eq!(
        ids(&query("maybe").after(&cursor).fetch().unwrap().rows),
        [2]
    );
    assert!(matches!(
        query("amount").after(&cursor).fetch(),
        Err(Error::InvalidCursor)
    ));
    for (comparison, right) in [(C::Eq, "category"), (C::Eq, "missing"), (C::Contains, "id")] {
        assert!(matches!(
            db.query::<Entry>()
                .filter_fields("id", comparison, right)
                .fetch(),
            Err(Error::InvalidQuery)
        ));
    }
}

#[test]
fn field_ranges_use_row_bounds_and_resume_as_their_explicit_comparisons() {
    use ic_db::QueryOperand::Field;

    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    let inside = || {
        db.query::<Entry>()
            .filter_between_fields("id", "maybe", "amount")
    };
    let expected = [1, 2, 4, 5, 7, 8];
    assert_eq!(ids(&inside().fetch().unwrap().rows), expected);
    let first = inside().limit(2).fetch().unwrap();
    assert_eq!(ids(&first.rows), expected[..2]);
    let rest = db
        .query::<Entry>()
        .filter_fields("id", C::Ge, "maybe")
        .filter_fields("id", C::Le, "amount")
        .after(first.next.as_ref().unwrap())
        .fetch()
        .unwrap();
    assert_eq!(ids(&rest.rows), expected[2..]);

    // id < amount holds even on rows 3 and 6, where the other bound is absent.
    let outside = || {
        db.query::<Entry>()
            .filter_not_between_fields("id", "amount", "maybe")
    };
    assert_eq!(outside().count().unwrap().value, 8);
    let first = outside().limit(2).fetch().unwrap();
    let rest = db
        .query::<Entry>()
        .filter_any_operands(&[
            ("id", C::Lt, Field("amount")),
            ("id", C::Gt, Field("maybe")),
        ])
        .after(first.next.as_ref().unwrap())
        .fetch()
        .unwrap();
    assert_eq!(ids(&rest.rows), [3, 4, 5, 6, 7, 8]);
    assert_eq!(
        db.query::<Entry>()
            .filter_not_between_fields("maybe", "amount", "id")
            .count()
            .unwrap()
            .value,
        6
    );
    assert_eq!(
        db.query::<Entry>()
            .filter_between_fields("id", "amount", "maybe")
            .count()
            .unwrap()
            .value,
        0
    );
    assert_eq!(
        db.query::<Entry>()
            .filter_not_between_fields("id", "id", "id")
            .count()
            .unwrap()
            .value,
        0
    );
    // Only the upper comparison can match here. Absent upper bounds exclude rows 3 and 6.
    let above_ceiling = db
        .query::<Entry>()
        .filter_not_between_fields("amount", "id", "maybe")
        .fetch()
        .unwrap();
    assert_eq!(ids(&above_ceiling.rows), expected);
    for bound in ["category", "missing"] {
        assert!(matches!(
            db.query::<Entry>()
                .filter_between_fields("id", bound, "id")
                .fetch(),
            Err(Error::InvalidQuery)
        ));
        assert!(matches!(
            db.query::<Entry>()
                .filter_not_between_fields("id", "amount", bound)
                .fetch(),
            Err(Error::InvalidQuery)
        ));
    }
}

#[test]
fn mixed_or_clauses_share_one_scan_and_preserve_operand_identity() {
    use ic_db::QueryOperand::{Field, Literal};
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    // Both alternatives match ids 1 and 2; only the literal matches id 3.
    let alternatives = [
        ("id", C::Eq, Field("maybe")),
        ("id", C::Le, Literal(3_u64.into())),
    ];
    let query = || db.query::<Entry>().filter_any_operands(&alternatives);
    assert_eq!(ids(&query().fetch().unwrap().rows), [1, 2, 3, 4, 5, 7, 8]);
    assert_eq!(
        query()
            .filter("category", C::Eq, 1_u8)
            .count()
            .unwrap()
            .value,
        4
    );
    assert!(matches!(
        query().using_index("maybe", &[]).fetch(),
        Err(Error::InvalidQuery)
    ));
    let proven = [alternatives[0], ("maybe", C::Le, Literal(3_u64.into()))];
    let page = db
        .query::<Entry>()
        .filter_any_operands(&proven)
        .using_index("maybe", &[])
        .fetch()
        .unwrap();
    assert_eq!(ids(&page.rows), [1, 2, 4, 5, 7, 8]);
    assert_eq!(page.examined, 6);
    let cursor = query().work_limit(2).fetch().unwrap().next.unwrap();
    let changed = [("id", C::Eq, Field("amount")), alternatives[1]];
    assert!(matches!(
        db.query::<Entry>()
            .filter_any_operands(&changed)
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
    let literals = [("id", C::Ge, 1_u64.into()), ("id", C::Eq, 2_u64.into())];
    let cursor = db
        .query::<Entry>()
        .filter_any(&literals)
        .work_limit(1)
        .fetch()
        .unwrap()
        .next
        .unwrap();
    let explicit = literals.map(|(field, op, value)| (field, op, Literal(value)));
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .filter_any_operands(&explicit)
            .after(&cursor)
            .fetch()
            .unwrap()
            .rows),
        [2, 3, 4, 5, 6, 7, 8]
    );
    let invalid = [("id", C::Eq, Field("id")), ("id", C::Eq, Field("missing"))];
    assert!(matches!(
        db.query::<Entry>().filter_any_operands(&invalid).count(),
        Err(Error::InvalidQuery)
    ));
    for terms in [&[][..], &[alternatives[0]; 9][..]] {
        assert!(matches!(
            db.query::<Entry>().filter_any_operands(terms).fetch(),
            Err(Error::Limit)
        ));
    }
}

#[test]
fn field_comparisons_prove_sparse_membership_on_either_side() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    let expected = [1, 2, 4, 5, 7, 8];
    for (left, comparison, right) in [
        ("maybe", C::Eq, "id"),
        ("id", C::Eq, "maybe"),
        ("amount", C::Gt, "maybe"),
        ("maybe", C::Lt, "amount"),
        ("maybe", C::Ge, "maybe"),
    ] {
        let query = || db.query::<Entry>().filter_fields(left, comparison, right);
        let page = query().using_index("maybe", &[]).fetch().unwrap();
        assert_eq!(ids(&page.rows), expected);
        assert_eq!(page.examined, expected.len());

        // Work smaller than the data set forces a real ordered continuation.
        let first = query().order_by(&["maybe"]).work_limit(2).fetch().unwrap();
        assert_eq!(ids(&first.rows), expected[..2]);
        let rest = query()
            .order_by(&["maybe"])
            .after(first.next.as_ref().unwrap())
            .fetch()
            .unwrap();
        assert_eq!(ids(&rest.rows), expected[2..]);
    }
    for (left, comparison, right) in [
        ("maybe", C::Eq, "maybe"),
        ("id", C::Ne, "maybe"),
        ("maybe", C::Ne, "amount"),
        ("amount", C::Gt, "id"),
    ] {
        let query = || db.query::<Entry>().filter_fields(left, comparison, right);
        assert!(matches!(
            query().using_index("maybe", &[]).fetch(),
            Err(Error::InvalidQuery)
        ));
        let rows = query().order_by(&["maybe"]).fetch().unwrap().rows;
        assert!(rows.iter().any(|row| row.maybe.is_none()));
    }
}

#[test]
fn distinct_checks_a_field_operand_that_varies_within_an_index_class() {
    let runtime = host(VectorMemory::default());
    let mut db = create(&runtime, 1);
    let mut batch = Batch::new();
    for id in 1..=8 {
        let mut entry = row(id);
        entry.amount = 4;
        batch.replace(&entry).unwrap();
    }
    db.commit(batch).unwrap();
    let query = || {
        db.query::<Entry>()
            .using_index("amount,id", &[])
            .filter_fields("amount", C::Lt, "id")
            .distinct()
    };
    assert!(matches!(
        query().work_limit(4).fetch_as::<AmountOnly>(),
        Err(Error::Limit)
    ));
    let page = query().fetch_as::<AmountOnly>().unwrap();
    assert_eq!(page.examined, 8);
    assert!(page.next.is_none());
    assert_eq!(page.rows.iter().map(|r| r.amount).collect::<Vec<_>>(), [4]);
}

#[test]
fn compound_index_ranges_include_whole_duplicate_classes_and_respect_fixed_prefixes() {
    let runtime = host(VectorMemory::default());
    let mut db = create(&runtime, 1);
    let mut input: Vec<_> = (1..=8).map(row).collect();
    let mut batch = Batch::new();
    for row in &mut input {
        row.amount = if row.id == 8 {
            u64::MAX
        } else {
            row.id / 3 * 10
        };
        batch.replace(row).unwrap();
    }
    db.commit(batch).unwrap();
    let accepts = |actual: u64, comparison: C, operand: u64| match comparison {
        C::Eq => actual == operand,
        C::Lt => actual < operand,
        C::Le => actual <= operand,
        C::Gt => actual > operand,
        C::Ge => actual >= operand,
        _ => unreachable!(),
    };
    for prefix in [None, Some(0_u8), Some(1_u8)] {
        for comparison in [C::Eq, C::Lt, C::Le, C::Gt, C::Ge] {
            for operand in [0_u64, 10, 20, u64::MAX] {
                for descending in [false, true] {
                    let mut expected: Vec<_> = input
                        .iter()
                        .filter(|r| {
                            prefix.is_none_or(|p| r.category == p)
                                && accepts(r.amount, comparison, operand)
                        })
                        .map(|r| (r.amount, r.id))
                        .collect();
                    expected.sort();
                    if descending {
                        expected.reverse();
                    }
                    let values: Vec<_> = prefix.into_iter().map(IndexValue::from).collect();
                    let query = || {
                        db.query::<Entry>()
                            .using_index(
                                if prefix.is_some() {
                                    "category,amount,id"
                                } else {
                                    "amount,id"
                                },
                                &values,
                            )
                            .filter("amount", comparison, operand)
                    };
                    if !descending {
                        let count = query().count().unwrap();
                        assert_eq!(
                            (count.value, count.examined, count.fetched),
                            (expected.len() as u64, expected.len(), 0)
                        );
                    }
                    let mut query = query();
                    if descending {
                        query = query.descending();
                    }
                    let page = query.fetch().unwrap();
                    assert_eq!(
                        ids(&page.rows),
                        expected.iter().map(|r| r.1).collect::<Vec<_>>()
                    );
                    assert_eq!(
                        page.examined,
                        expected.len(),
                        "{prefix:?} {comparison:?} {operand}"
                    );
                }
            }
        }
    }
    // A later field cannot restrict a range before all earlier components are fixed.
    let residual = db
        .query::<Entry>()
        .using_index("category,amount,id", &[])
        .filter("amount", C::Eq, 10_u64)
        .fetch()
        .unwrap();
    assert_eq!(residual.examined, 8);
    let empty = db
        .query::<Entry>()
        .using_index("amount,id", &[])
        .filter("amount", C::Gt, 10_u64)
        .filter("amount", C::Le, 10_u64)
        .count()
        .unwrap();
    assert_eq!((empty.value, empty.examined), (0, 0));
    let alternatives = [
        ("amount", C::Eq, 0_u64.into()),
        ("amount", C::Eq, 20_u64.into()),
    ];
    let page = db
        .query::<Entry>()
        .using_index("amount,id", &[])
        .filter_any(&alternatives)
        .fetch()
        .unwrap();
    assert_eq!(ids(&page.rows), [1, 2, 6, 7]);
    assert_eq!(page.examined, 8);
}

#[test]
fn optional_text_index_ranges_preserve_absence_escaped_bytes_and_duplicate_values() {
    let runtime = host(VectorMemory::default());
    let (db, input) = text_rows(&runtime);
    for comparison in [C::Eq, C::Lt, C::Le, C::Gt, C::Ge] {
        for operand in ["", "a", "a\0", "ab", "éclair", "\u{10ffff}"] {
            let mut expected: Vec<_> = input
                .iter()
                .filter(|r| {
                    r.text.as_deref().is_some_and(|text| match comparison {
                        C::Eq => text == operand,
                        C::Lt => text < operand,
                        C::Le => text <= operand,
                        C::Gt => text > operand,
                        C::Ge => text >= operand,
                        _ => unreachable!(),
                    })
                })
                .collect();
            expected.sort_by_key(|r| (&r.text, r.id));
            let indexed = || {
                db.query::<TextRow>()
                    .using_index("text,id", &[])
                    .filter("text", comparison, operand)
            };
            let count = indexed().count().unwrap();
            assert_eq!((count.value, count.fetched), (expected.len() as u64, 0));
            let page = indexed().fetch().unwrap();
            assert_eq!(
                page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
                expected.iter().map(|r| r.id).collect::<Vec<_>>()
            );
            assert_eq!(page.examined, expected.len(), "{comparison:?} {operand:?}");
        }
    }
    let absent = db
        .query::<TextRow>()
        .using_index("text,id", &[])
        .filter("text", C::Eq, IndexValue::Absent)
        .fetch()
        .unwrap();
    assert_eq!(
        absent.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        input
            .iter()
            .filter(|r| r.text.is_none())
            .map(|r| r.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(absent.examined, absent.rows.len());

    for descending in [false, true] {
        let mut cursor = None;
        let mut rows = Vec::new();
        let mut retries = 0;
        for _ in 0..16 {
            let mut query = db
                .query::<TextRow>()
                .using_index("text,id", &[])
                .filter("text", C::Ge, "a")
                .filter("text", C::Le, "ab")
                .byte_limit(8)
                .work_limit(32);
            if descending {
                query = query.descending();
            }
            if let Some(cursor) = &cursor {
                query = query.after(cursor);
            }
            let page = query.fetch_as::<PresenceId>().unwrap();
            rows.extend(page.rows.into_iter().map(|r| r.id));
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
            assert_eq!(cursor.as_ref().unwrap().to_bytes()[1], 1);
            retries += 1;
        }
        let mut expected: Vec<_> = input
            .iter()
            .filter(|r| {
                r.text
                    .as_deref()
                    .is_some_and(|text| ("a"..="ab").contains(&text))
            })
            .collect();
        expected.sort_by_key(|r| (&r.text, r.id));
        if descending {
            expected.reverse();
        }
        assert_eq!(rows, expected.iter().map(|r| r.id).collect::<Vec<_>>());
        assert!(cursor.is_none());
        assert_eq!(retries, rows.len() - 1);
    }
}

#[entity(
    pk(field = "id"),
    index(field = "text"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "text", value(item(prim = "Text", max_bytes = 4096)))
    )
)]
struct WideRange {}

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
fn oversized_index_range_operands_fall_back_to_admitted_residual_comparison() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[WideRange::SCHEMA]).unwrap(),
        [1; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (id, text) in [(1, "a"), (2, "z")] {
        batch
            .insert(&WideRange {
                id,
                text: text.into(),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    for operand in ["m".repeat(501), "m".repeat(600), "\0".repeat(300)] {
        // A legal field operand can exceed the complete secondary-key limit.
        // Automatic selection can still enumerate this query in primary order.
        let ordered = || {
            db.query::<WideRange>()
                .filter("text", C::Eq, operand.as_str())
                .order_by(&["id"])
                .work_limit(1)
        };
        let mut page = ordered().fetch().unwrap();
        for _ in 0..2 {
            assert!(page.rows.is_empty());
            assert_eq!(page.examined, 1);
            page = ordered().after(&page.next.unwrap()).fetch().unwrap();
        }
        assert!(page.rows.is_empty() && page.next.is_none());
        assert!(matches!(
            ordered()
                .using_index("text", &[operand.as_str().into()])
                .fetch(),
            Err(Error::Limit)
        ));
        for comparison in [C::Eq, C::Lt, C::Le, C::Gt, C::Ge, C::StartsWith] {
            let expected = db
                .query::<WideRange>()
                .filter("text", comparison, operand.as_str())
                .fetch()
                .unwrap();
            let indexed = || {
                db.query::<WideRange>().using_index("text", &[]).filter(
                    "text",
                    comparison,
                    operand.as_str(),
                )
            };
            let actual = indexed().fetch().unwrap();
            let count = indexed().count().unwrap();
            assert_eq!(count.value, expected.rows.len() as u64);
            if operand.len() > 501 {
                assert_eq!(count.fetched, 2);
            }
            assert_eq!(actual.rows, expected.rows);
            assert_eq!(
                actual.examined,
                if operand.len() == 501 {
                    expected.rows.len()
                } else {
                    2
                }
            );
        }
    }
    // The terminator is not part of a prefix seek: this derived bound is exactly
    // 512 bytes even though the complete comparison component would exceed it.
    let prefix = "m".repeat(503);
    let page = db
        .query::<WideRange>()
        .using_index("text", &[])
        .filter("text", C::StartsWith, prefix.as_str())
        .fetch()
        .unwrap();
    assert!(page.rows.is_empty());
    assert_eq!(page.examined, 0);
}

#[test]
fn text_prefix_seeks_cover_unicode_boundaries_and_empty_required_text() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[WideRange::SCHEMA]).unwrap(),
        [1; 16],
    )
    .unwrap();
    let input = [
        "",
        "\0",
        "\0\u{1}",
        "a\0b",
        "\u{7f}",
        "\u{80}",
        "\u{7ff}",
        "\u{800}",
        "\u{d7ff}",
        "\u{e000}",
        "\u{ffff}",
        "\u{10000}",
        "\u{10fffe}",
        "\u{10ffff}",
        "\u{10ffff}x",
        "\u{10ffff}\u{10ffff}",
        "é\u{10ffff}",
    ];
    let mut batch = Batch::new();
    for (id, text) in input.iter().enumerate() {
        batch
            .insert(&WideRange {
                id: id as u64,
                text: (*text).into(),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    for needle in input.into_iter().chain(["a", "é", "missing"]) {
        let mut expected: Vec<_> = input
            .iter()
            .enumerate()
            .filter(|(_, text)| text.starts_with(needle))
            .collect();
        expected.sort_by_key(|(id, text)| (*text, *id));
        let query = || {
            db.query::<WideRange>()
                .using_index("text", &[])
                .filter("text", C::StartsWith, needle)
        };
        let page = query().fetch().unwrap();
        assert_eq!(
            page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            expected
                .iter()
                .map(|(id, _)| *id as u64)
                .collect::<Vec<_>>()
        );
        assert_eq!(page.examined, expected.len());
        let exists = query().work_limit(1).exists().unwrap();
        assert_eq!(
            (exists.value, exists.examined),
            (!expected.is_empty(), usize::from(!expected.is_empty()))
        );
        let count = query().work_limit(expected.len() + 1).count().unwrap();
        assert_eq!(
            (count.value, count.examined),
            (expected.len() as u64, expected.len())
        );
    }
    // An OR alternative cannot silently become a mandatory text range.
    let page = db
        .query::<WideRange>()
        .using_index("text", &[])
        .filter_any(&[
            ("text", C::StartsWith, "a".into()),
            ("text", C::StartsWith, "é".into()),
        ])
        .fetch()
        .unwrap();
    assert_eq!(page.rows.iter().map(|r| r.id).collect::<Vec<_>>(), [3, 16]);
    assert_eq!(page.examined, input.len());
    let empty = db
        .query::<WideRange>()
        .using_index("text", &[])
        .filter("text", C::StartsWith, "a")
        .filter("text", C::StartsWith, "é")
        .fetch()
        .unwrap();
    assert!(empty.rows.is_empty());
    assert_eq!(empty.examined, 0);
}

#[test]
fn primary_ranges_match_integer_oracle_and_only_examine_the_intersection() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    let accepts = |id: u64, op: C, value: u64| match op {
        C::Eq => id == value,
        C::Lt => id < value,
        C::Le => id <= value,
        C::Gt => id > value,
        C::Ge => id >= value,
        _ => unreachable!(),
    };
    for left in [C::Eq, C::Lt, C::Le, C::Gt, C::Ge] {
        for right in [C::Eq, C::Lt, C::Le, C::Gt, C::Ge] {
            for (a, b) in [(0, 9), (3, 6), (6, 3), (4, 4), (u64::MAX, u64::MAX)] {
                let expected: Vec<_> = (1..=8)
                    .filter(|&id| accepts(id, left, a) && accepts(id, right, b))
                    .collect();
                for descending in [false, true] {
                    let mut query = db
                        .query::<Entry>()
                        .filter("id", left, a)
                        .filter("id", right, b);
                    if descending {
                        query = query.descending();
                    }
                    let page = query.fetch().unwrap();
                    let mut expected = expected.clone();
                    if descending {
                        expected.reverse();
                    }
                    assert_eq!(ids(&page.rows), expected, "{left:?} {a}; {right:?} {b}");
                    assert_eq!(
                        (page.examined, page.fetched),
                        (expected.len(), expected.len())
                    );
                    assert!(page.next.is_none());
                }
            }
        }
    }
    let point = db
        .query::<Entry>()
        .filter("id", C::Eq, 8_u64)
        .work_limit(1)
        .exists()
        .unwrap();
    assert_eq!((point.value, point.examined), (true, 1));
    let empty = db
        .query::<Entry>()
        .filter("id", C::Gt, 8_u64)
        .work_limit(1)
        .count()
        .unwrap();
    assert_eq!((empty.value, empty.examined), (0, 0));
    // An empty interval never bypasses complete operand admission.
    assert!(matches!(
        db.query::<Entry>()
            .filter("id", C::Gt, 8_u64)
            .filter("id", C::Lt, 1_u64)
            .filter("amount", C::Eq, 0_u8)
            .fetch(),
        Err(Error::InvalidOperand)
    ));
    let alternatives = [("id", C::Le, 2_u64.into()), ("id", C::Ge, 7_u64.into())];
    let page = db
        .query::<Entry>()
        .filter_any(&alternatives)
        .fetch()
        .unwrap();
    assert_eq!(ids(&page.rows), [1, 2, 7, 8]);
    assert_eq!(page.examined, 8);
    let page = db
        .query::<Entry>()
        .filter_any(&alternatives)
        .filter("id", C::Ge, 2_u64)
        .fetch()
        .unwrap();
    assert_eq!(ids(&page.rows), [2, 7, 8]);
    assert_eq!(page.examined, 7);
    let indexed = db
        .query::<Entry>()
        .using_index("category", &[])
        .filter("id", C::Ge, 7_u64)
        .fetch()
        .unwrap();
    assert_eq!(ids(&indexed.rows), [8, 7]);
    assert_eq!(indexed.examined, 8);
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
        let mut q = query().byte_limit(row(1).encode().unwrap().len());
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

#[entity(
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Int64"))))
)]
struct SignedRange {}
#[entity(
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Principal"))))
)]
struct PrincipalRange {}

#[test]
fn primary_range_encoding_handles_signed_extremes_and_variable_width_keys() {
    use ic_db::Principal;
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[SignedRange::SCHEMA, PrincipalRange::SCHEMA]).unwrap(),
        [9; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for id in [i64::MIN, -1, 0, 1, i64::MAX] {
        batch.insert(&SignedRange { id }).unwrap();
    }
    let principals: Vec<_> = [&[][..], &[0][..], &[255][..], &[0, 0][..]]
        .into_iter()
        .map(|bytes| Principal::try_from_slice(bytes).unwrap())
        .collect();
    for &id in &principals {
        batch.insert(&PrincipalRange { id }).unwrap();
    }
    db.commit(batch).unwrap();
    let page = db
        .query::<SignedRange>()
        .filter("id", C::Ge, -1_i64)
        .filter("id", C::Lt, i64::MAX)
        .fetch()
        .unwrap();
    assert_eq!(
        page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        [-1, 0, 1]
    );
    assert_eq!(page.examined, 3);
    for (i, &id) in principals.iter().enumerate() {
        let page = db
            .query::<PrincipalRange>()
            .filter("id", C::Ge, id)
            .fetch()
            .unwrap();
        assert_eq!(
            page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            principals[i..]
        );
        assert_eq!(page.examined, principals.len() - i);
    }
}

#[entity(pk(field="id"), index(fields=["text", "id"]), fields(
    field(name="id", value(item(prim="Nat64"))),
    field(name="bucket", value(item(prim="Nat8"))),
    field(name="required", value(item(prim="Text", max_bytes=4096))),
    field(name="text", value(opt, item(prim="Text", max_bytes=64)))
))]
struct TextRow {}
#[ic_db::record(fields(field(name = "text", value(opt, item(prim = "Text", max_bytes = 64)))))]
struct TextOnly {}
#[ic_db::record(fields(field(name = "required", value(item(prim = "Text", max_bytes = 4096)))))]
struct RequiredText {}
#[ic_db::record(fields(field(name = "bucket", value(item(prim = "Nat8")))))]
struct TextBucket {}

fn text_rows(runtime: &MemoryRuntime<VectorMemory>) -> (Database<VectorMemory>, Vec<TextRow>) {
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[TextRow::SCHEMA]).unwrap(),
        [1; 16],
    )
    .unwrap();
    let rows: Vec<_> = [
        None,
        Some(""),
        Some("a"),
        Some("ab"),
        Some("a\0b"),
        Some("a\0"),
        Some("A"),
        Some("éclair"),
        Some("e\u{301}clair"),
        Some("βeta"),
        Some("β"),
        Some("zero"),
        Some("ab"),
        None,
        None,
        Some("\u{1}"),
        Some("\0"),
        Some("\0\u{1}"),
        Some("\u{1}\0"),
        Some("\u{1}\u{1}"),
        None,
        None,
        None,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, text)| TextRow {
        id: i as u64,
        bucket: (i / 2) as u8,
        required: text.unwrap_or_default().to_owned(),
        text: text.map(str::to_owned),
    })
    .collect();
    let mut batch = Batch::new();
    for row in &rows {
        batch.insert(row).unwrap();
    }
    db.commit(batch).unwrap();
    (db, rows)
}

fn text_matches(comparison: C, text: &str, needle: &str) -> bool {
    match comparison {
        C::StartsWith => text.starts_with(needle),
        C::EndsWith => text.ends_with(needle),
        C::Contains => text.contains(needle),
        _ => panic!("not a text comparison"),
    }
}

#[test]
fn text_contains_matches_long_repetitive_and_escaped_inputs() {
    let runtime = host(VectorMemory::default());
    let (mut db, mut rows) = text_rows(&runtime);
    for required in [
        format!("{}b", "a".repeat(3999)),
        format!("{}é", "\0".repeat(1999)),
    ] {
        let row = TextRow {
            id: rows.len() as u64,
            bucket: 0,
            required,
            text: None,
        };
        let mut batch = Batch::new();
        batch.insert(&row).unwrap();
        db.commit(batch).unwrap();
        rows.push(row);
    }
    for needle in [
        format!("{}b", "a".repeat(3998)),
        format!("{}c", "a".repeat(3998)),
        format!("{}é", "\0".repeat(1998)),
        "\0\0".to_owned(),
    ] {
        let expected: Vec<_> = rows
            .iter()
            .filter(|row| row.required.contains(&needle))
            .map(|row| row.id)
            .collect();
        let actual = db
            .query::<TextRow>()
            .filter("required", C::Contains, needle.as_str())
            .fetch()
            .unwrap();
        assert_eq!(
            actual.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            expected
        );
    }
    // All alternatives are evaluated by the same scanner; overlap never repeats rows.
    let actual = db
        .query::<TextRow>()
        .filter_any(&[
            ("required", C::Contains, "lai".into()),
            ("required", C::Contains, "clair".into()),
        ])
        .fetch()
        .unwrap();
    assert_eq!(
        actual.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        [7, 8]
    );
}

#[test]
fn required_text_and_groups_match_rust_oracle() {
    let runtime = host(VectorMemory::default());
    let (db, rows) = text_rows(&runtime);
    for comparison in [C::StartsWith, C::EndsWith, C::Contains] {
        for needle in ["", "a", "clair", "\0", "\u{1}", "\0\u{1}", "\u{1}\0"] {
            let expected: Vec<_> = rows
                .iter()
                .filter(|row| text_matches(comparison, &row.required, needle))
                .map(|row| row.id)
                .collect();
            let actual = db
                .query::<TextRow>()
                .filter("required", comparison, needle)
                .fetch()
                .unwrap();
            assert_eq!(
                actual.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
                expected
            );
            let mut maxima = std::collections::BTreeMap::<u8, &str>::new();
            for row in &rows {
                maxima
                    .entry(row.bucket)
                    .and_modify(|value| *value = (*value).max(&row.required))
                    .or_insert(&row.required);
            }
            let expected: Vec<_> = maxima
                .into_iter()
                .filter(|(_, value)| text_matches(comparison, value, needle))
                .map(|(key, value)| (key, value.to_owned()))
                .collect();
            let actual = db
                .query::<TextRow>()
                .filter_groups(comparison, needle)
                .group_max::<TextBucket, RequiredText>()
                .unwrap()
                .value;
            assert_eq!(
                actual
                    .into_iter()
                    .map(|group| (group.key.bucket, group.value.unwrap().required))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}

#[test]
fn length_compares_text_in_utf8_bytes_and_binds_pages() {
    let runtime = host(VectorMemory::default());
    let (db, rows) = text_rows(&runtime);
    for n in [0, 1, 2, 3, u32::MAX] {
        let expected: Vec<_> = rows
            .iter()
            .filter(|row| row.text.as_ref().is_some_and(|v| v.len() == n as usize))
            .map(|row| row.id)
            .collect();
        let query = || db.query::<TextRow>().filter_length("text", C::Eq, n);
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
    }
    let cursor = db
        .query::<TextRow>()
        .filter_length("text", C::Ge, 2)
        .work_limit(1)
        .fetch()
        .unwrap()
        .next
        .unwrap();
    assert!(matches!(
        db.query::<TextRow>()
            .filter_length("text", C::Ge, 3)
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
}

#[test]
fn text_matches_rust_oracle_across_pages_and_distinct_classes() {
    let runtime = host(VectorMemory::default());
    let (db, rows) = text_rows(&runtime);
    for comparison in [C::StartsWith, C::EndsWith, C::Contains] {
        for needle in [
            "", "a", "a\0", "a\0b", "A", "é", "e\u{301}", "β", "clair", "\0", "\u{1}", "\0\u{1}",
            "\u{1}\0", "lai", "\u{301}", "missing",
        ] {
            let matching: Vec<_> = rows
                .iter()
                .filter(|r| {
                    r.text
                        .as_deref()
                        .is_some_and(|text| text_matches(comparison, text, needle))
                })
                .collect();
            for descending in [false, true] {
                let mut actual = Vec::new();
                let mut cursor = None;
                let mut examined = 0;
                for step in 0..32 {
                    let mut query = db
                        .query::<TextRow>()
                        .filter("text", comparison, needle)
                        .order_by(&["text"])
                        .work_limit(1)
                        .byte_limit(80);
                    if descending {
                        query = query.descending();
                    }
                    if let Some(cursor) = &cursor {
                        query = query.after(cursor);
                    }
                    let page = query.fetch().unwrap();
                    assert!(page.examined <= 1);
                    examined += page.examined;
                    actual.extend(page.rows.into_iter().map(|r| r.id));
                    cursor = page.next;
                    if cursor.is_none() {
                        break;
                    }
                    assert!(step < 31);
                }
                let mut sorted = matching.clone();
                sorted.sort_by_key(|r| (&r.text, r.id));
                let mut expected: Vec<_> = sorted.into_iter().map(|r| r.id).collect();
                if descending {
                    expected.reverse();
                }
                assert_eq!(actual, expected, "{needle:?}");
                assert_eq!(
                    examined,
                    if comparison == C::StartsWith {
                        matching.len()
                    } else {
                        rows.len()
                    }
                );
            }
            let expected: std::collections::BTreeSet<_> =
                matching.iter().map(|r| r.text.clone()).collect();
            let mut actual = Vec::new();
            let mut cursor = None;
            let mut examined = 0;
            for step in 0..32 {
                let mut query = db
                    .query::<TextRow>()
                    .filter("text", comparison, needle)
                    .order_by(&["text"])
                    .distinct()
                    .work_limit(1);
                if let Some(cursor) = &cursor {
                    query = query.after(cursor);
                }
                let page = query.fetch_as::<TextOnly>().unwrap();
                examined += page.examined;
                actual.extend(page.rows.into_iter().map(|r| r.text));
                cursor = page.next;
                if cursor.is_none() {
                    break;
                }
                assert!(step < 31);
            }
            assert_eq!(actual, expected.into_iter().collect::<Vec<_>>());
            assert_eq!(
                examined,
                if comparison == C::StartsWith {
                    matching.len()
                } else {
                    rows.len()
                }
            );
            assert_eq!(
                db.query::<TextRow>()
                    .filter("text", comparison, needle)
                    .count()
                    .unwrap()
                    .value,
                matching.len() as u64
            );
        }
    }
    let rows = db
        .query::<TextRow>()
        .filter_any(&[
            ("text", C::StartsWith, "é".into()),
            ("text", C::StartsWith, "a\0".into()),
        ])
        .fetch()
        .unwrap()
        .rows;
    assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), [4, 5, 7]);
}

#[test]
fn text_groups_and_refusals_share_exact_admission() {
    let runtime = host(VectorMemory::default());
    let (db, rows) = text_rows(&runtime);
    for comparison in [C::StartsWith, C::EndsWith, C::Contains] {
        for needle in ["", "a\0", "é", "e\u{301}", "clair", "\0", "\u{1}"] {
            let mut expected = std::collections::BTreeMap::<u8, Vec<&str>>::new();
            for row in &rows {
                if let Some(text) = row.text.as_deref() {
                    expected.entry(row.bucket).or_default().push(text);
                }
            }
            let expected: Vec<_> = expected
                .into_iter()
                .filter_map(|(key, values)| {
                    let value = values.into_iter().max().unwrap();
                    text_matches(comparison, value, needle).then_some((key, value.to_owned()))
                })
                .collect();
            let actual = db
                .query::<TextRow>()
                .filter_groups(comparison, needle)
                .group_max::<TextBucket, TextOnly>()
                .unwrap()
                .value;
            assert_eq!(
                actual
                    .into_iter()
                    .map(|g| (g.key.bucket, g.value.unwrap().text.unwrap()))
                    .collect::<Vec<_>>(),
                expected
            );
        }
        for invalid in [IndexValue::Absent, 1_u64.into()] {
            assert!(matches!(
                db.query::<TextRow>()
                    .filter("text", comparison, invalid)
                    .fetch(),
                Err(Error::InvalidOperand)
            ));
            assert!(matches!(
                db.query::<TextRow>()
                    .filter_groups(comparison, invalid)
                    .group_min::<TextBucket, TextOnly>(),
                Err(Error::InvalidOperand)
            ));
        }
        let oversized = "x".repeat(65);
        assert!(matches!(
            db.query::<TextRow>()
                .filter("text", comparison, oversized.as_str())
                .fetch(),
            Err(Error::Limit)
        ));
        assert!(matches!(
            db.query::<TextRow>()
                .filter_groups(comparison, oversized.as_str())
                .group_min::<TextBucket, TextOnly>(),
            Err(Error::Limit)
        ));
        assert!(matches!(
            db.query::<TextRow>()
                .filter_any(&[
                    ("text", comparison, "".into()),
                    ("id", comparison, 0_u64.into())
                ])
                .fetch(),
            Err(Error::InvalidQuery)
        ));
        assert!(matches!(
            db.query::<TextRow>()
                .filter("id", comparison, 0_u64)
                .fetch(),
            Err(Error::InvalidQuery)
        ));
        assert!(matches!(
            db.query::<TextRow>()
                .filter_groups(comparison, 0_u64)
                .group_count::<TextBucket>(),
            Err(Error::InvalidQuery)
        ));
        let cursor = db
            .query::<TextRow>()
            .filter("text", comparison, "a")
            .work_limit(1)
            .fetch()
            .unwrap()
            .next
            .unwrap();
        assert!(matches!(
            db.query::<TextRow>()
                .filter("text", C::Ge, "a")
                .after(&cursor)
                .fetch(),
            Err(Error::InvalidCursor)
        ));
        assert!(matches!(
            db.query::<TextRow>()
                .filter("text", comparison, "A")
                .after(&cursor)
                .fetch(),
            Err(Error::InvalidCursor)
        ));
    }
}

#[test]
fn exclusion_matches_optional_value_oracle_and_inequality_cursors() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for excluded in [vec![Some(1), Some(1)], vec![None], vec![None, Some(1)]] {
        let values: Vec<_> = excluded
            .iter()
            .map(|value| value.map_or(IndexValue::Absent, IndexValue::Nat64))
            .collect();
        let expected: Vec<_> = (1..=8)
            .map(row)
            .filter(|r| !excluded.contains(&r.maybe))
            .map(|r| r.id)
            .collect();
        for descending in [false, true] {
            let mut actual = Vec::new();
            let mut cursor = None;
            for step in 0..16 {
                let mut query = db
                    .query::<Entry>()
                    .filter_not_in("maybe", &values)
                    .work_limit(1);
                if descending {
                    query = query.descending();
                }
                if let Some(cursor) = &cursor {
                    query = query.after(cursor);
                }
                let page = query.fetch().unwrap();
                assert!(page.examined <= 1);
                actual.extend(ids(&page.rows));
                cursor = page.next;
                if cursor.is_none() {
                    break;
                }
                assert!(
                    step < 15,
                    "exclusion must progress past rejected candidates"
                );
            }
            let mut ordered = expected.clone();
            if descending {
                ordered.reverse();
            }
            assert_eq!(actual, ordered);
        }
        assert_eq!(
            db.query::<Entry>()
                .filter_not_in("maybe", &values)
                .count()
                .unwrap()
                .value,
            expected.len() as u64
        );
    }
    let values = [1_u64.into(), 3_u64.into(), 3_u64.into()];
    let first = db
        .query::<Entry>()
        .filter_not_in("id", &values)
        .limit(1)
        .fetch()
        .unwrap();
    assert_eq!(ids(&first.rows), [2]);
    let cursor = first.next.unwrap();
    let rest = db
        .query::<Entry>()
        .filter("id", C::Ne, 1_u64)
        .filter("id", C::Ne, 3_u64)
        .filter("id", C::Ne, 3_u64)
        .after(&cursor)
        .fetch()
        .unwrap();
    assert_eq!(ids(&rest.rows), [4, 5, 6, 7, 8]);
    assert!(matches!(
        db.query::<Entry>()
            .filter_not_in("id", &[values[1], values[0], values[2]])
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
}

#[test]
fn exclusion_admits_every_member_and_shares_the_filter_budget() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for values in [vec![], vec![1_u64.into(); 9]] {
        assert!(matches!(
            db.query::<Entry>().filter_not_in("id", &values).fetch(),
            Err(Error::Limit)
        ));
    }
    assert_eq!(
        db.query::<Entry>()
            .filter_not_in("id", &[1_u64.into(); 8])
            .count()
            .unwrap()
            .value,
        7
    );
    assert!(matches!(
        db.query::<Entry>()
            .filter("amount", C::Ge, 0_u64)
            .filter_not_in("id", &[1_u64.into(); 8])
            .fetch(),
        Err(Error::Limit)
    ));
    for invalid in [IndexValue::Absent, 1_u8.into(), "wrong".into()] {
        // No row survives the first filter; invalid later operands still refuse.
        assert!(matches!(
            db.query::<Entry>()
                .filter("id", C::Lt, 0_u64)
                .filter_not_in("id", &[1_u64.into(), invalid])
                .fetch(),
            Err(Error::InvalidOperand)
        ));
    }
    assert!(matches!(
        db.query::<Entry>()
            .filter_not_in("missing", &[1_u64.into()])
            .fetch(),
        Err(Error::InvalidQuery)
    ));
}

#[test]
fn inclusive_ranges_preserve_endpoints_absence_index_order_and_continuation() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for indexed in [false, true] {
        for descending in [false, true] {
            let mut query = db.query::<Entry>().filter_between("maybe", 2_u64, 7_u64);
            if indexed {
                query = query.using_index("maybe", &[]);
            }
            if descending {
                query = query.descending();
            }
            let expected = if descending {
                vec![7, 5, 4, 2]
            } else {
                vec![2, 4, 5, 7]
            };
            assert_eq!(ids(&query.fetch().unwrap().rows), expected);
        }
    }
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .filter_between("id", 4_u64, 4_u64)
            .fetch()
            .unwrap()
            .rows),
        [4]
    );
    assert_eq!(
        db.query::<Entry>()
            .filter_between("id", 7_u64, 2_u64)
            .count()
            .unwrap()
            .value,
        0
    );
    let page = db
        .query::<Entry>()
        .filter_between("amount", 20_u64, 40_u64)
        .using_index("amount,id", &[])
        .limit(1)
        .fetch()
        .unwrap();
    assert_eq!(ids(&page.rows), [2]);
    let cursor = page.next.unwrap();
    let rest = db
        .query::<Entry>()
        .filter("amount", C::Ge, 20_u64)
        .filter("amount", C::Le, 40_u64)
        .using_index("amount,id", &[])
        .after(&cursor)
        .fetch()
        .unwrap();
    assert_eq!(ids(&rest.rows), [3, 4]);
    assert!(matches!(
        db.query::<Entry>()
            .filter_between("amount", 20_u64, 50_u64)
            .using_index("amount,id", &[])
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
}

#[test]
fn inclusive_ranges_admit_both_operands_and_share_the_filter_budget() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for invalid in [IndexValue::Absent, 1_u8.into(), "wrong".into()] {
        for (lower, upper) in [(invalid, 8_u64.into()), (1_u64.into(), invalid)] {
            assert!(matches!(
                db.query::<Entry>()
                    .filter("id", C::Lt, 0_u64)
                    .filter_between("id", lower, upper)
                    .fetch(),
                Err(Error::InvalidOperand)
            ));
        }
    }
    assert!(matches!(
        db.query::<Entry>()
            .filter_between("missing", 1_u64, 8_u64)
            .fetch(),
        Err(Error::InvalidQuery)
    ));
    let mut query = db.query::<Entry>();
    for _ in 0..4 {
        query = query.filter_between("id", 2_u64, 4_u64);
    }
    assert_eq!(query.count().unwrap().value, 3);
    let mut query = db.query::<Entry>().filter("id", C::Ge, 1_u64);
    for _ in 0..4 {
        query = query.filter_between("id", 2_u64, 4_u64);
    }
    assert!(matches!(query.fetch(), Err(Error::Limit)));
}

#[test]
fn outside_ranges_preserve_sparse_progress_absence_and_or_cursor_identity() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for indexed in [false, true] {
        for descending in [false, true] {
            for (lower, upper, expected) in [
                (2_u64, 7_u64, vec![1, 8]),
                (4, 4, vec![1, 2, 5, 7, 8]),
                (7, 2, vec![1, 2, 4, 5, 7, 8]),
            ] {
                let mut expected = expected;
                if descending {
                    expected.reverse();
                }
                let alternatives = [
                    ("maybe", C::Lt, lower.into()),
                    ("maybe", C::Gt, upper.into()),
                ];
                let mut cursor = None;
                let mut actual = Vec::new();
                for page_number in 0..16 {
                    // Continue a helper-created cursor using the explicit OR clause.
                    let mut query = if cursor.is_none() {
                        db.query::<Entry>()
                            .filter_not_between("maybe", lower, upper)
                    } else {
                        db.query::<Entry>().filter_any(&alternatives)
                    }
                    .work_limit(1);
                    if indexed {
                        query = query.using_index("maybe", &[]);
                    }
                    if descending {
                        query = query.descending();
                    }
                    if let Some(cursor) = &cursor {
                        query = query.after(cursor);
                    }
                    let page = query.fetch().unwrap();
                    assert!(page.examined <= 1);
                    actual.extend(ids(&page.rows));
                    cursor = page.next;
                    if cursor.is_none() {
                        break;
                    }
                    assert!(page_number < 15, "outside range must progress");
                }
                assert_eq!(actual, expected);
            }
        }
    }
}

#[test]
fn outside_ranges_admit_both_operands_and_share_the_filter_budget() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for invalid in [IndexValue::Absent, 1_u8.into(), "wrong".into()] {
        for (lower, upper) in [(invalid, 0_u64.into()), (9_u64.into(), invalid)] {
            // The valid alternative matches every row, but cannot hide a bad operand.
            assert!(matches!(
                db.query::<Entry>()
                    .filter_not_between("id", lower, upper)
                    .fetch(),
                Err(Error::InvalidOperand)
            ));
        }
    }
    let mut query = db.query::<Entry>();
    for _ in 0..4 {
        query = query.filter_not_between("id", 2_u64, 7_u64);
    }
    assert_eq!(query.count().unwrap().value, 2);
    let mut query = db.query::<Entry>().filter("id", C::Ge, 0_u64);
    for _ in 0..4 {
        query = query.filter_not_between("id", 2_u64, 7_u64);
    }
    assert!(matches!(query.fetch(), Err(Error::Limit)));
}

#[test]
fn membership_preserves_absence_duplicates_and_equality_cursor_identity() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    let values = [IndexValue::Absent, 1_u64.into(), 1_u64.into()];
    let expected: Vec<_> = (1..=8)
        .map(row)
        .filter(|r| r.maybe.is_none() || r.maybe == Some(1))
        .map(|r| r.id)
        .collect();
    let mut cursor = None;
    let mut actual = Vec::new();
    for page_number in 0..16 {
        let mut query = db
            .query::<Entry>()
            .filter_in("maybe", &values)
            .work_limit(1);
        if let Some(cursor) = &cursor {
            query = query.after(cursor);
        }
        let page = query.fetch().unwrap();
        assert!(page.examined <= 1);
        actual.extend(ids(&page.rows));
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
        assert!(page_number < 15, "membership scan must progress");
    }
    assert_eq!(actual, expected);
    assert_eq!(
        db.query::<Entry>()
            .filter_in("maybe", &values)
            .count()
            .unwrap()
            .value,
        3
    );
    assert_eq!(
        db.query::<Entry>()
            .filter_in("id", &[1_u64.into(); 8])
            .count()
            .unwrap()
            .value,
        1
    );

    let values = [1_u64.into(), 3_u64.into()];
    let page = db
        .query::<Entry>()
        .filter_in("id", &values)
        .limit(1)
        .fetch()
        .unwrap();
    assert_eq!(ids(&page.rows), [1]);
    let cursor = page.next.unwrap();
    let page = db
        .query::<Entry>()
        .filter_any(&[("id", C::Eq, values[0]), ("id", C::Eq, values[1])])
        .after(&cursor)
        .fetch()
        .unwrap();
    assert_eq!(ids(&page.rows), [3]);
    assert!(matches!(
        db.query::<Entry>()
            .filter_in("id", &[values[1], values[0]])
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
    let a = db
        .query::<Entry>()
        .filter_in("id", &values[..1])
        .limit(1)
        .fetch()
        .unwrap()
        .next;
    let b = db
        .query::<Entry>()
        .filter("id", C::Eq, 1_u64)
        .limit(1)
        .fetch()
        .unwrap()
        .next;
    assert_eq!(a, b);
}

#[test]
fn membership_admits_every_member_and_charges_duplicates_before_execution() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    assert!(matches!(
        db.query::<Entry>().filter_in("id", &[]).fetch(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter_in("id", &[1_u64.into(); 9])
            .fetch(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter("id", C::Ge, 0_u64)
            .filter_in("id", &[1_u64.into(); 8])
            .fetch(),
        Err(Error::Limit)
    ));
    for value in [1_u8.into(), IndexValue::Absent] {
        assert!(matches!(
            db.query::<Entry>()
                .filter_in("id", &[1_u64.into(), value])
                .fetch(),
            Err(Error::InvalidOperand)
        ));
        assert!(matches!(
            db.query::<Other>()
                .filter_in("id", &[1_u64.into(), value])
                .fetch(),
            Err(Error::InvalidOperand)
        ));
    }
    assert!(matches!(
        db.query::<Entry>()
            .filter_in("missing", &[1_u64.into()])
            .fetch(),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(
        db.query::<Entry>()
            .filter_in("category", &[0_u8.into(), 1_u8.into()])
            .order_by(&["id"])
            .fetch()
            .unwrap()
            .rows
            .len(),
        8
    );
}

#[test]
fn any_clauses_compose_with_and_and_progress_without_duplicate_rows() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    let first = [
        ("amount", C::Lt, 30_u64.into()),
        ("maybe", C::Eq, IndexValue::Absent),
    ];
    let second = [
        ("category", C::Eq, 0_u8.into()),
        ("id", C::Eq, 1_u64.into()),
    ];
    let expected: Vec<_> = (1..=8)
        .map(row)
        .filter(|r| {
            (r.amount < 30 || r.maybe.is_none()) && (r.category == 0 || r.id == 1) && r.id >= 2
        })
        .map(|r| r.id)
        .collect();
    for descending in [false, true] {
        for work in 1..=3 {
            let mut cursor = None;
            let mut actual = Vec::new();
            for call in 0..32 {
                let mut query = db
                    .query::<Entry>()
                    .filter_any(&first)
                    .filter("id", C::Ge, 2_u64)
                    .filter_any(&second)
                    .order_by(&["id"])
                    .limit(2)
                    .work_limit(work)
                    .byte_limit(16);
                if descending {
                    query = query.descending();
                }
                if let Some(cursor) = &cursor {
                    query = query.after(cursor);
                }
                let page = query.fetch_as::<Summary>().unwrap();
                assert!(page.examined <= work);
                assert!(page.output_bytes <= 16);
                actual.extend(page.rows.into_iter().map(|r| r.id));
                cursor = page
                    .next
                    .map(|c| QueryCursor::from_bytes(&c.to_bytes()).unwrap());
                if cursor.is_none() {
                    break;
                }
                assert!(call < 31, "bounded pages must progress");
            }
            let mut expected = expected.clone();
            if descending {
                expected.reverse();
            }
            assert_eq!(actual, expected);
        }
    }
    assert_eq!(
        db.query::<Entry>()
            .filter_any(&first)
            .filter_any(&second)
            .filter("id", C::Ge, 2_u64)
            .count()
            .unwrap()
            .value,
        expected.len() as u64
    );
}

#[test]
fn ordered_primary_equality_seeks_one_row_before_checking_secondary_predicates() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for descending in [false, true] {
        for (id, category, expected, examined) in
            [(8_u64, 0_u8, Some(8), 1), (8, 1, None, 1), (9, 1, None, 0)]
        {
            let mut query = db
                .query::<Entry>()
                .filter("category", C::Eq, category)
                .filter("id", C::Eq, id)
                .order_by(&["amount"])
                .work_limit(1);
            if descending {
                query = query.descending();
            }
            let page = query.fetch().unwrap();
            assert_eq!(ids(&page.rows), expected.into_iter().collect::<Vec<_>>());
            assert_eq!((page.examined, page.fetched), (examined, examined));
        }
    }
}

#[test]
fn any_equalities_cannot_claim_fixed_order_or_index_membership() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    let alternatives = [
        ("category", C::Eq, 0_u8.into()),
        ("category", C::Eq, 1_u8.into()),
    ];
    let rows = db
        .query::<Entry>()
        .filter_any(&alternatives)
        .order_by(&["id"])
        .fetch()
        .unwrap();
    assert_eq!(ids(&rows.rows), (1..=8).collect::<Vec<_>>());
    assert!(matches!(
        db.query::<Entry>()
            .filter_any(&alternatives)
            .using_index("category", &[])
            .order_by(&["id"])
            .fetch(),
        Err(Error::InvalidQuery)
    ));
    let groups = db
        .query::<Entry>()
        .filter_any(&alternatives)
        .order_by(&["category"])
        .distinct()
        .fetch_as::<CategoryOnly>()
        .unwrap();
    assert_eq!(
        groups.rows.iter().map(|r| r.category).collect::<Vec<_>>(),
        [0, 1]
    );
    let groups = db
        .query::<Entry>()
        .filter_any(&alternatives)
        .group_count::<CategoryOnly>()
        .unwrap();
    assert_eq!(
        groups
            .value
            .iter()
            .map(|g| (g.key.category, g.count))
            .collect::<Vec<_>>(),
        [(0, 4), (1, 4)]
    );
    let mixed = db
        .query::<Entry>()
        .filter_any(&[
            ("category", C::Eq, 0_u8.into()),
            ("id", C::Eq, 1_u64.into()),
        ])
        .order_by(&["category"])
        .distinct()
        .fetch_as::<CategoryOnly>()
        .unwrap();
    assert_eq!(
        mixed.rows.iter().map(|r| r.category).collect::<Vec<_>>(),
        [0, 1]
    );
    assert!(mixed.next.is_none());
    assert_eq!(
        ids(&db
            .query::<Entry>()
            .filter_any(&[
                ("maybe", C::Eq, 1_u64.into()),
                ("maybe", C::Eq, 2_u64.into())
            ])
            .using_index("maybe", &[])
            .fetch()
            .unwrap()
            .rows),
        [1, 2]
    );
}

#[test]
fn any_admits_every_operand_and_binds_boolean_structure() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for bad in [
        ("category", C::Eq, 0_u64.into()),
        ("maybe", C::Lt, IndexValue::Absent),
    ] {
        assert!(matches!(
            db.query::<Entry>()
                .filter_any(&[("id", C::Ge, 0_u64.into()), bad])
                .fetch(),
            Err(Error::InvalidOperand)
        ));
    }
    assert!(matches!(
        db.query::<Other>()
            .filter_any(&[("id", C::Eq, 0_u64.into()), ("id", C::Eq, 1_u8.into())])
            .fetch(),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.query::<Entry>().filter_any(&[]).fetch(),
        Err(Error::Limit)
    ));
    let repeated = [("id", C::Ge, 0_u64.into()); 8];
    assert_eq!(
        db.query::<Entry>()
            .filter_any(&repeated)
            .count()
            .unwrap()
            .value,
        8
    );
    assert!(matches!(
        db.query::<Entry>()
            .filter_any(&repeated)
            .filter("id", C::Ge, 0_u64)
            .fetch(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter_any(&[("id", C::Ge, 0_u64.into()); 9])
            .fetch(),
        Err(Error::Limit)
    ));

    let alternatives = [("id", C::Ge, 2_u64.into()), ("id", C::Le, 5_u64.into())];
    let cursor = db
        .query::<Entry>()
        .filter_any(&alternatives)
        .limit(1)
        .fetch()
        .unwrap()
        .next
        .unwrap();
    assert!(matches!(
        db.query::<Entry>()
            .filter("id", C::Ge, 2_u64)
            .filter("id", C::Le, 5_u64)
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
    let a = db
        .query::<Entry>()
        .filter_any(&alternatives[..1])
        .limit(1)
        .fetch()
        .unwrap()
        .next;
    let b = db
        .query::<Entry>()
        .filter("id", C::Ge, 2_u64)
        .limit(1)
        .fetch()
        .unwrap()
        .next;
    assert_eq!(
        a, b,
        "singleton clauses preserve conjunction cursor identity"
    );
    let three = [
        alternatives[0],
        alternatives[1],
        ("id", C::Gt, 3_u64.into()),
    ];
    let cursor = db
        .query::<Entry>()
        .filter_any(&three)
        .limit(1)
        .fetch()
        .unwrap()
        .next
        .unwrap();
    assert!(
        matches!(
            db.query::<Entry>()
                .filter_any(&alternatives)
                .filter("id", C::Gt, 3_u64)
                .after(&cursor)
                .fetch(),
            Err(Error::InvalidCursor)
        ),
        "identical leaves with different clause boundaries must not resume"
    );
}

#[test]
fn existence_stops_on_a_valid_match_and_never_reports_budget_exhaustion_as_false() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let db = create(&runtime, 1);
    let before = backing.borrow().clone();
    let first = db
        .query::<Entry>()
        .work_limit(1)
        .byte_limit(1)
        .exists()
        .unwrap();
    assert_eq!(
        (
            first.value,
            first.examined,
            first.fetched,
            first.output_bytes
        ),
        (true, 1, 1, 1)
    );
    assert!(matches!(
        db.query::<Entry>().byte_limit(1).fetch(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter("amount", C::Ge, 50_u64)
            .using_index("category", &[])
            .work_limit(2)
            .exists(),
        Err(Error::Limit)
    ));
    let late = db
        .query::<Entry>()
        .filter("amount", C::Ge, 50_u64)
        .work_limit(1)
        .limit(1)
        .exists()
        .unwrap();
    assert_eq!((late.value, late.examined, late.fetched), (true, 1, 1));
    assert!(matches!(
        db.query::<Entry>()
            .filter("amount", C::Gt, 80_u64)
            .using_index("category", &[])
            .work_limit(8)
            .exists(),
        Err(Error::Limit)
    ));
    let range = db
        .query::<Entry>()
        .filter("amount", C::Gt, 40_u64)
        .filter("amount", C::Le, 60_u64)
        .work_limit(3)
        .count()
        .unwrap();
    assert_eq!((range.value, range.examined, range.fetched), (2, 2, 0));
    // A range in one alternative cannot exclude matches from the other.
    let alternatives = db
        .query::<Entry>()
        .filter_any(&[
            ("amount", C::Ge, 80_u64.into()),
            ("category", C::Eq, 1_u8.into()),
        ])
        .count()
        .unwrap();
    assert_eq!(
        (
            alternatives.value,
            alternatives.examined,
            alternatives.fetched
        ),
        (5, 8, 8)
    );
    let none = db
        .query::<Entry>()
        .filter("amount", C::Gt, 80_u64)
        .work_limit(1)
        .exists()
        .unwrap();
    assert_eq!(
        (none.value, none.examined, none.output_bytes),
        (false, 0, 1)
    );
    for (amount, expected, reads) in [(80_u64, true, 1), (81_u64, false, 0)] {
        let result = db
            .query::<Entry>()
            .filter("amount", C::Eq, amount)
            .work_limit(1)
            .exists()
            .unwrap();
        assert_eq!(
            (result.value, result.examined, result.fetched),
            (expected, reads, reads)
        );
    }
    let lookup = |category, minimum, budget| {
        db.query::<Entry>()
            .filter("category", C::Eq, category)
            .filter("amount", C::Ge, minimum)
            .work_limit(budget)
            .exists()
    };
    let hit = lookup(0_u8, 40_u64, 1).unwrap();
    assert_eq!((hit.value, hit.examined, hit.fetched), (true, 1, 1));
    let beyond = lookup(0, 81, 1).unwrap();
    assert_eq!((beyond.value, beyond.examined), (false, 0));
    // The next index component enforces the minimum amount without fetching owners.
    let count = db
        .query::<Entry>()
        .filter("category", C::Eq, 0_u8)
        .filter("amount", C::Ge, 40_u64)
        .work_limit(5)
        .count()
        .unwrap();
    assert_eq!((count.value, count.examined, count.fetched), (3, 3, 0));
    let miss = lookup(7, 40, 1).unwrap();
    assert_eq!((miss.value, miss.examined, miss.fetched), (false, 0, 0));
    let empty = db.query::<Other>().work_limit(1).exists().unwrap();
    assert_eq!((empty.value, empty.examined), (false, 0));
    let indexed = db
        .query::<Entry>()
        .using_index("category,amount,id", &[1_u8.into()])
        .filter("amount", C::Ge, 50_u64)
        .work_limit(3)
        .exists()
        .unwrap();
    assert_eq!((indexed.value, indexed.examined), (true, 1));
    let sparse = db
        .query::<Entry>()
        .using_index("maybe", &[])
        .filter("maybe", C::Ne, IndexValue::Absent)
        .work_limit(1)
        .exists()
        .unwrap();
    assert_eq!((sparse.value, sparse.examined), (true, 1));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn exact_primary_existence_and_count_complete_with_one_candidate() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for (id, amount, expected, fetched) in [
        (8_u64, 80_u64, true, 1),
        (8, 90, false, 1),
        (9, 90, false, 0),
    ] {
        let result = db
            .query::<Entry>()
            .filter("amount", C::Ge, amount)
            .filter("id", C::Eq, id)
            .work_limit(1)
            .exists()
            .unwrap();
        assert_eq!(
            (result.value, result.examined, result.fetched),
            (expected, fetched, fetched)
        );
    }
    let count = db
        .query::<Entry>()
        .filter("id", C::Eq, 8_u64)
        .work_limit(1)
        .count()
        .unwrap();
    assert_eq!((count.value, count.examined, count.fetched), (1, 1, 0));
    let page = db
        .query::<Entry>()
        .filter("id", C::Eq, 8_u64)
        .limit(1)
        .fetch()
        .unwrap();
    assert_eq!(ids(&page.rows), [8]);
    assert!(page.next.is_none());
}

#[test]
fn existence_admits_every_operand_and_refuses_incompatible_modifiers() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    assert!(matches!(
        db.query::<Entry>()
            .filter_any(&[("id", C::Ge, 0_u64.into()), ("amount", C::Eq, 0_u8.into())])
            .exists(),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.query::<Entry>().filter("missing", C::Eq, 0_u64).exists(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().distinct().exists(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().order_by(&["id"]).exists(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().descending().exists(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().filter_groups(C::Ge, 0_u64).exists(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().byte_limit(0).exists(),
        Err(Error::Limit)
    ));
    let cursor = db.query::<Entry>().limit(1).fetch().unwrap().next.unwrap();
    assert!(matches!(
        db.query::<Entry>().after(&cursor).exists(),
        Err(Error::InvalidQuery)
    ));
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
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
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
        db.query::<Entry>().filter("amount", C::Eq, 0_u64).fetch(),
        Err(Error::Corrupt)
    ));
    assert!(matches!(
        db.query::<Entry>().fetch_as::<Summary>(),
        Err(Error::Corrupt)
    ));
    // A late corrupt row cannot be hidden by the sorted output limit.
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["amount", "category"])
            .descending()
            .limit(1)
            .fetch_as::<Summary>(),
        Err(Error::Corrupt)
    ));
    // Descending category order emits category 1 before reaching corrupt row 1.
    // Suppressing its duplicate projection must not suppress the integrity error.
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["category"])
            .descending()
            .distinct()
            .fetch_as::<CategoryOnly>(),
        Err(Error::Corrupt)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .descending()
            .distinct()
            .fetch_as::<CategoryOnly>(),
        Err(Error::Corrupt)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter("amount", C::Eq, 0_u64)
            .sum::<WideAmount>(),
        Err(Error::Corrupt)
    ));
    assert!(matches!(
        db.query::<Entry>().group_sum::<CategoryOnly, WideAmount>(),
        Err(Error::Corrupt)
    ));
    assert!(matches!(
        db.query::<Entry>().count_validated(),
        Err(Error::Corrupt)
    ));
    assert!(matches!(db.query::<Entry>().exists(), Err(Error::Corrupt)));
    assert!(matches!(
        db.query::<Entry>().filter("amount", C::Eq, 10_u64).exists(),
        Err(Error::Corrupt)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter("category", C::Eq, 1_u8)
            .filter("amount", C::Ne, 10_u64)
            .work_limit(1)
            .exists(),
        Err(Error::Corrupt)
    ));
    // This path reaches a valid category-0 row before the corrupt category-1 row.
    // Existence validates the matched row, but deliberately does not audit later rows.
    let exists = db
        .query::<Entry>()
        .using_index("category", &[])
        .exists()
        .unwrap();
    assert_eq!((exists.value, exists.examined), (true, 1));
    assert!(matches!(
        db.query::<Entry>().count_field("maybe"),
        Err(Error::Corrupt)
    ));
    assert!(matches!(
        db.query::<Entry>().count_distinct::<CategoryOnly>(),
        Err(Error::Corrupt)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .group_count_distinct::<CategoryOnly, AmountOnly>(),
        Err(Error::Corrupt)
    ));
    assert!(matches!(
        db.query::<Entry>().group_min::<CategoryOnly, AmountOnly>(),
        Err(Error::Corrupt)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter("amount", C::Eq, 0_u64)
            .min::<AmountOnly>(),
        Err(Error::Corrupt)
    ));
    let missing = db
        .query::<Entry>()
        .filter("amount", C::Eq, 0_u64)
        .group_count::<CategoryOnly>()
        .unwrap();
    assert!(missing.value.is_empty());
    assert_eq!((missing.examined, missing.fetched), (0, 0));
    assert!(matches!(
        db.query::<Entry>()
            .filter("category", C::Eq, 1_u8)
            .filter("amount", C::Ne, 10_u64)
            .group_count::<CategoryOnly>(),
        Err(Error::Corrupt)
    ));
    // A seek does not audit out-of-range rows, but validates a selected corrupt row.
    let sought = db
        .query::<Entry>()
        .filter("id", C::Eq, 2_u64)
        .fetch()
        .unwrap();
    assert_eq!(ids(&sought.rows), [2]);
    assert_eq!(sought.examined, 1);
    assert!(matches!(
        db.query::<Entry>().filter("id", C::Eq, 1_u64).fetch(),
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

#[ic_db::set(item(prim = "Nat8"), max_items = 3)]
struct PresenceSet {}
#[entity(pk(field="id"), index(fields=["needle","id"],unique,present_only), fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="needle",value(opt,item(prim="Nat8"))),
    field(name="other",value(opt,item(prim="Nat8"))),
    field(name="items",value(opt,many,max_items=3,item(prim="Nat8"))),
    field(name="members",value(opt,item(is="PresenceSet")))
))]
struct MemberRow {}

#[test]
fn field_membership_checks_each_rows_needle_and_binds_the_continuation() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[MemberRow::SCHEMA]).unwrap(),
        [9; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (id, (needle, items)) in [
        (Some(2), Some(vec![1, 2, 2])),
        (None, Some(vec![0])),
        (Some(1), None),
        (Some(3), Some(vec![])),
        (Some(4), Some(vec![1, 3])),
        (Some(1), Some(vec![1])),
    ]
    .into_iter()
    .enumerate()
    {
        let members = items
            .as_ref()
            .map(|xs| PresenceSet(xs.iter().copied().collect()));
        batch
            .insert(&MemberRow {
                id: id as u64,
                needle,
                other: needle,
                items,
                members,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    for field in ["items", "members"] {
        let query = |needle| {
            db.query::<MemberRow>()
                .filter_fields(field, C::Contains, needle)
        };
        assert_eq!(query("needle").count().unwrap().value, 2);
        let indexed = query("needle")
            .using_index("needle,id", &[])
            .fetch()
            .unwrap();
        assert_eq!(
            indexed.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            [5, 0]
        );
        let first = query("needle").limit(1).fetch().unwrap();
        assert_eq!(first.rows[0].id, 0);
        let cursor = first.next.unwrap();
        assert!(matches!(
            query("other").after(&cursor).fetch(),
            Err(Error::InvalidCursor)
        ));
        assert!(matches!(
            db.query::<MemberRow>()
                .filter(field, C::Contains, 2_u8)
                .after(&cursor)
                .fetch(),
            Err(Error::InvalidCursor)
        ));
        assert_eq!(
            query("needle").after(&cursor).fetch().unwrap().rows[0].id,
            5
        );
        assert!(matches!(query("id").count(), Err(Error::InvalidQuery)));
        assert!(matches!(query("items").count(), Err(Error::InvalidQuery)));
    }
}
#[ic_db::map(key(prim = "Nat8"), value(item(prim = "Nat8")), max_items = 3)]
struct PresenceMap {}
#[ic_db::enum_(
    variant(name = "Idle"),
    variant(name = "Data", value(item(prim = "Nat8")))
)]
struct PresenceEnum {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "blob", value(opt, item(prim = "Blob", max_bytes = 8))),
        field(name = "items", value(opt, many, max_items = 3, item(prim = "Nat8"))),
        field(name = "details", value(opt, item(is = "Details"))),
        field(name = "map", value(opt, item(is = "PresenceMap"))),
        field(name = "set", value(opt, item(is = "PresenceSet"))),
        field(name = "variant", value(opt, item(is = "PresenceEnum")))
    )
)]
struct PresenceData {}
#[ic_db::record(fields(field(name = "id", value(item(prim = "Nat64")))))]
struct PresenceId {}

#[test]
fn presence_filters_cover_optional_stored_types_and_keep_empty_values_present() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[PresenceData::SCHEMA]).unwrap(),
        [1; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for id in 0..4 {
        let full = id == 2;
        batch
            .insert(&PresenceData {
                id,
                blob: (id == 1 || full).then(|| if full { vec![0] } else { vec![] }),
                items: (id != 0).then(|| if full { vec![7, 0, 0] } else { vec![] }),
                details: (id == 1 || full).then(|| Details {
                    labels: if full { vec!["x".into()] } else { vec![] },
                }),
                map: (id != 0).then(|| {
                    PresenceMap(if full {
                        [(0, 1)].into()
                    } else {
                        Default::default()
                    })
                }),
                set: (id == 1 || full).then(|| {
                    PresenceSet(if full {
                        [0, 7].into()
                    } else {
                        Default::default()
                    })
                }),
                variant: (id != 0).then_some(if full {
                    PresenceEnum::Data(0)
                } else {
                    PresenceEnum::Idle
                }),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    for field in ["items", "set"] {
        let page = db
            .query::<PresenceData>()
            .filter(field, C::Contains, if field == "set" { 7_u8 } else { 0 })
            .fetch_as::<PresenceId>()
            .unwrap();
        assert_eq!(page.rows.iter().map(|row| row.id).collect::<Vec<_>>(), [2]);
        assert_eq!(
            db.query::<PresenceData>()
                .filter(field, C::Contains, 1_u8)
                .count()
                .unwrap()
                .value,
            0
        );
        for invalid in [IndexValue::Absent, 0_u64.into()] {
            assert!(matches!(
                db.query::<PresenceData>()
                    .filter(field, C::Contains, invalid)
                    .fetch(),
                Err(Error::InvalidOperand)
            ));
        }
    }
    assert!(matches!(
        db.query::<PresenceData>()
            .filter("map", C::Contains, 0_u8)
            .fetch(),
        Err(Error::InvalidQuery)
    ));
    for field in ["blob", "items", "set", "map"] {
        let empty = if matches!(field, "items" | "map") {
            vec![1, 3]
        } else {
            vec![1]
        };
        for (comparison, len, expected) in [
            (C::Eq, 0, empty),
            (C::Gt, 0, vec![2]),
            (
                C::Ne,
                match field {
                    "items" => 3,
                    "set" => 2,
                    _ => 1,
                },
                vec![0, 1, 3],
            ),
        ] {
            let page = db
                .query::<PresenceData>()
                .filter_length(field, comparison, len)
                .fetch_as::<PresenceId>()
                .unwrap();
            assert_eq!(
                page.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
                expected,
                "{field}"
            );
        }
        assert_eq!(
            db.query::<PresenceData>()
                .filter_present(field)
                .filter_length(field, C::Gt, 0)
                .count()
                .unwrap()
                .value,
            1
        );
    }
    for (field, op) in [
        ("id", C::Eq),
        ("details", C::Gt),
        ("variant", C::Eq),
        ("items", C::Contains),
    ] {
        assert!(matches!(
            db.query::<PresenceData>()
                .filter_length(field, op, 0)
                .count(),
            Err(Error::InvalidQuery)
        ));
    }
    for field in ["blob", "items", "details", "map", "set", "variant"] {
        let present = if ["items", "map", "variant"].contains(&field) {
            vec![1, 2, 3]
        } else {
            vec![1, 2]
        };
        for comparison in [C::Eq, C::Ne] {
            let expected: Vec<_> = (0..4)
                .filter(|id| present.contains(id) == (comparison == C::Ne))
                .collect();
            for descending in [false, true] {
                let mut actual = Vec::new();
                let mut cursor = None;
                for step in 0..5 {
                    let mut query = db
                        .query::<PresenceData>()
                        .filter(field, comparison, IndexValue::Absent)
                        .work_limit(1)
                        .byte_limit(8)
                        .distinct();
                    if descending {
                        query = query.descending();
                    }
                    if let Some(cursor) = &cursor {
                        query = query.after(cursor);
                    }
                    let page = query.fetch_as::<PresenceId>().unwrap();
                    actual.extend(page.rows.into_iter().map(|r| r.id));
                    cursor = page.next;
                    if cursor.is_none() {
                        break;
                    }
                    assert!(step < 4);
                }
                let mut ordered = expected.clone();
                if descending {
                    ordered.reverse();
                }
                assert_eq!(actual, ordered, "{field} {comparison:?}");
            }
            assert_eq!(
                db.query::<PresenceData>()
                    .filter(field, comparison, IndexValue::Absent)
                    .byte_limit(1)
                    .exists()
                    .unwrap()
                    .value,
                !expected.is_empty()
            );
            assert_eq!(
                db.query::<PresenceData>()
                    .filter(field, comparison, IndexValue::Absent)
                    .count()
                    .unwrap()
                    .value,
                expected.len() as u64
            );
            assert_eq!(
                db.query::<PresenceData>()
                    .filter(field, comparison, IndexValue::Absent)
                    .fetch()
                    .unwrap()
                    .rows
                    .len(),
                expected.len()
            );
        }
        let result = db
            .query::<PresenceData>()
            .filter(field, C::Eq, 0_u8)
            .fetch();
        if field == "blob" {
            assert!(matches!(result, Err(Error::InvalidOperand)));
        } else {
            assert!(matches!(result, Err(Error::InvalidQuery)));
        }
        let result = db
            .query::<PresenceData>()
            .filter(field, C::Lt, IndexValue::Absent)
            .fetch();
        if field == "blob" {
            assert!(matches!(result, Err(Error::InvalidOperand)));
        } else {
            assert!(matches!(result, Err(Error::InvalidQuery)));
        }
    }
    let page = db
        .query::<PresenceData>()
        .filter_any(&[
            ("blob", C::Eq, IndexValue::Absent),
            ("items", C::Ne, IndexValue::Absent),
        ])
        .fetch_as::<PresenceId>()
        .unwrap();
    assert_eq!(
        page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    let cursor = db
        .query::<PresenceData>()
        .filter_absent("blob")
        .work_limit(1)
        .fetch()
        .unwrap()
        .next
        .unwrap();
    let resumed = db
        .query::<PresenceData>()
        .filter("blob", C::Eq, IndexValue::Absent)
        .after(&cursor)
        .fetch()
        .unwrap();
    assert_eq!(
        resumed.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        [3]
    );
    assert!(matches!(
        db.query::<PresenceData>()
            .filter_present("blob")
            .after(&cursor)
            .fetch(),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        db.query::<PresenceData>().filter_absent("missing").fetch(),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn presence_filters_validate_skipped_values_and_optional_tags() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = Schema::new(&[PresenceData::SCHEMA]).unwrap();
    let mut db =
        Database::create(runtime.open_memory(&slot).unwrap(), schema.clone(), [1; 16]).unwrap();
    let row = PresenceData {
        id: 0,
        blob: None,
        items: Some(vec![0]),
        details: None,
        map: None,
        set: None,
        variant: None,
    };
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let valid = row.encode().unwrap();
    // Canonical field-name order puts the optional blob tag first.
    assert_eq!(valid[0], 0);
    let mut bad_tag = valid.clone();
    bad_tag[0] = 2;
    let mut trailing = valid;
    trailing.push(0);
    for bytes in [bad_tag, trailing] {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
        let mut key = vec![2];
        key.extend(PresenceData::ID.to_be_bytes());
        key.extend(0_u64.to_be_bytes());
        raw.insert(key, bytes);
        drop(raw);
        let before = backing.borrow().clone();
        let db = Database::open(runtime.open_memory(&slot).unwrap(), schema.clone()).unwrap();
        for comparison in [C::Eq, C::Ne] {
            assert!(matches!(
                db.query::<PresenceData>()
                    .filter("blob", comparison, IndexValue::Absent)
                    .fetch_as::<PresenceId>(),
                Err(Error::Corrupt)
            ));
        }
        assert!(matches!(
            db.query::<PresenceData>()
                .filter_length("blob", C::Eq, 0)
                .count(),
            Err(Error::Corrupt)
        ));
        assert!(matches!(
            db.query::<PresenceData>()
                .filter("items", C::Contains, 0_u8)
                .count(),
            Err(Error::Corrupt)
        ));
        assert_eq!(*backing.borrow(), before);
    }
}

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
    assert!(matches!(
        db.query::<Rich>()
            .filter("unused", C::Eq, IndexValue::Absent)
            .fetch(),
        Err(Error::InvalidOperand)
    ));
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
            // Prefix and range selection visit only matching candidates.
            assert_eq!(examined, expected.len());
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
fn sparse_membership_uses_filter_presence_without_changing_order_or_distinct() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let db = create(&runtime, 10);
    let before = backing.borrow().clone();
    // The presence clause proves sparse membership, but the absent equality
    // cannot form a sparse key. Other indexes and both predicates still apply.
    let contradiction = db
        .query::<Entry>()
        .filter_absent("maybe")
        .filter_present("maybe")
        .filter("amount", C::Eq, 20_u64)
        .order_by(&["id"])
        .fetch()
        .unwrap();
    assert!(contradiction.rows.is_empty());
    assert_eq!(contradiction.examined, 1);
    let absent_prefix = db
        .query::<Entry>()
        .filter_absent("maybe")
        .filter_present("maybe")
        .order_by(&["id"])
        .fetch()
        .unwrap();
    assert!(absent_prefix.rows.is_empty());
    for (comparison, operand) in [
        (C::Ne, IndexValue::Absent),
        (C::Ge, 4_u64.into()),
        (C::Gt, 4_u64.into()),
        (C::Lt, 4_u64.into()),
        (C::Le, 4_u64.into()),
    ] {
        let expected = db
            .query::<Entry>()
            .filter("maybe", comparison, operand)
            .fetch()
            .unwrap()
            .rows;
        for explicit in [false, true] {
            for descending in [false, true] {
                let mut cursor = None;
                let mut actual = Vec::new();
                let mut examined = 0;
                for step in 0..8 {
                    let mut query = db
                        .query::<Entry>()
                        .filter("maybe", comparison, operand)
                        .order_by(&["maybe"])
                        .work_limit(1);
                    if explicit {
                        query = query.using_index("maybe", &[]);
                    }
                    if descending {
                        query = query.descending();
                    }
                    if let Some(cursor) = &cursor {
                        query = query.after(cursor);
                    }
                    let page = query.fetch().unwrap();
                    examined += page.examined;
                    actual.extend(ids(&page.rows));
                    cursor = page.next;
                    if cursor.is_none() {
                        break;
                    }
                    assert!(step < 7);
                }
                let mut ordered: Vec<_> = expected.iter().collect();
                ordered.sort_by_key(|row| (row.maybe, row.id));
                if descending {
                    ordered.reverse();
                }
                assert_eq!(
                    actual,
                    ordered.into_iter().map(|row| row.id).collect::<Vec<_>>()
                );
                assert_eq!(
                    examined,
                    expected.len(),
                    "absence and out-of-range rows are skipped"
                );
            }
        }
    }
    let values = [1_u64.into(), 4_u64.into(), 4_u64.into()];
    let distinct = db
        .query::<Entry>()
        .filter_in("maybe", &values)
        .order_by(&["maybe"])
        .distinct()
        .fetch_as::<OptionalOnly>()
        .unwrap();
    assert_eq!(
        distinct.rows,
        [
            OptionalOnly { maybe: Some(1) },
            OptionalOnly { maybe: Some(4) }
        ]
    );
    assert_eq!(
        db.query::<Entry>()
            .filter_in("maybe", &values)
            .using_index("maybe", &[])
            .count()
            .unwrap()
            .value,
        2
    );
    for alternatives in [
        vec![("maybe", C::Ne, 4_u64.into())],
        vec![("maybe", C::Eq, IndexValue::Absent)],
        vec![
            ("maybe", C::Eq, 1_u64.into()),
            ("maybe", C::Eq, IndexValue::Absent),
        ],
        vec![
            ("maybe", C::Ge, 1_u64.into()),
            ("category", C::Eq, 0_u8.into()),
        ],
    ] {
        assert!(matches!(
            db.query::<Entry>()
                .filter_any(&alternatives)
                .using_index("maybe", &[])
                .fetch(),
            Err(Error::InvalidQuery)
        ));
    }
    // Presence establishes membership, not a constant ordering field.
    assert!(matches!(
        db.query::<Entry>()
            .filter("maybe", C::Ne, IndexValue::Absent)
            .using_index("maybe", &[])
            .order_by(&["id"])
            .fetch(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter("maybe", C::Ne, IndexValue::Absent)
            .filter("maybe", C::Eq, 1_u8)
            .order_by(&["maybe"])
            .fetch(),
        Err(Error::InvalidOperand)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[entity(pk(field="id"), index(fields=["left","right","id"],unique,present_only),fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="left",value(opt,item(prim="Nat64"))),
    field(name="right",value(opt,item(prim="Text",max_bytes=8)))
))]
struct SparsePair {}

#[test]
fn sparse_compound_membership_requires_every_optional_component() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[SparsePair::SCHEMA]).unwrap(),
        [1; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (i, (left, right)) in [
        (None, None),
        (Some(1), None),
        (None, Some("a")),
        (Some(1), Some("a")),
        (Some(2), Some("")),
        (Some(1), Some("ba")),
    ]
    .into_iter()
    .enumerate()
    {
        batch
            .insert(&SparsePair {
                id: i as u64,
                left,
                right: right.map(str::to_owned),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    // The matching row with an absent right field is missing from this index.
    let left = || db.query::<SparsePair>().filter("left", C::Eq, 1_u64);
    let incomplete_index = left().count().unwrap();
    assert_eq!((incomplete_index.value, incomplete_index.fetched), (3, 6));
    let covered = left().filter("right", C::Eq, "a").count().unwrap();
    assert_eq!(
        (covered.value, covered.examined, covered.fetched),
        (1, 1, 0)
    );
    for comparison in [C::StartsWith, C::EndsWith, C::Contains] {
        let page = db
            .query::<SparsePair>()
            .filter_present("left")
            .filter("right", comparison, "")
            .order_by(&["left", "right"])
            .fetch()
            .unwrap();
        assert_eq!(
            page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            [3, 5, 4]
        );
        assert_eq!(page.examined, 3);
        let page = db
            .query::<SparsePair>()
            .using_index("left,right,id", &[1_u64.into()])
            .filter("right", comparison, "")
            .order_by(&["right"])
            .fetch()
            .unwrap();
        assert_eq!(page.rows.iter().map(|r| r.id).collect::<Vec<_>>(), [3, 5]);
    }
    for missing in ["left", "right"] {
        assert!(matches!(
            db.query::<SparsePair>()
                .filter(missing, C::Ne, IndexValue::Absent)
                .using_index("left,right,id", &[])
                .fetch(),
            Err(Error::InvalidQuery)
        ));
    }
    // An OR between fields does not prove either field present on every match.
    assert!(matches!(
        db.query::<SparsePair>()
            .filter_any(&[
                ("left", C::Ne, IndexValue::Absent),
                ("right", C::Ne, IndexValue::Absent),
            ])
            .using_index("left,right,id", &[])
            .fetch(),
        Err(Error::InvalidQuery)
    ));
}

#[test]
fn ordering_refuses_incomplete_paths_and_binds_continuations() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 10);
    for order in [
        &[][..],
        &["missing"],
        &["amount", "amount"],
        &["id", "amount"],
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
    for order in [&["amount", "category"][..], &["maybe"], &["payload"]] {
        assert!(matches!(
            db.query::<Entry>()
                .order_by(order)
                .limit(1)
                .work_limit(1)
                .fetch(),
            Err(Error::Limit)
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
#[ic_db::record(fields(field(name = "category", value(item(prim = "Nat8")))))]
struct CategoryOnly {}
#[ic_db::record(fields(field(name = "amount", value(item(prim = "Nat64")))))]
struct AmountOnly {}
#[ic_db::record(fields(
    field(name = "category", value(item(prim = "Nat8"))),
    field(name = "amount", value(item(prim = "Nat64")))
))]
struct CategoryAmount {}
#[ic_db::record(fields(field(name = "value", value(opt, item(prim = "Nat64")))))]
struct OptionalValue {}

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
        let mut query = db
            .query::<OptionalOrdered>()
            .order_by(&["value"])
            .distinct();
        if descending {
            query = query.descending();
        }
        let page = query.fetch_as::<OptionalValue>().unwrap();
        assert_eq!(
            page.rows.into_iter().map(|r| r.value).collect::<Vec<_>>(),
            if descending {
                vec![Some(2), Some(0), None]
            } else {
                vec![None, Some(0), Some(2)]
            }
        );
        for (comparison, expected) in [(C::Eq, vec![None]), (C::Ne, vec![Some(0), Some(2)])] {
            let mut cursor = None;
            let mut actual = Vec::new();
            let mut examined = 0;
            loop {
                let mut query = db
                    .query::<OptionalOrdered>()
                    .filter("value", comparison, IndexValue::Absent)
                    .order_by(&["value"])
                    .distinct()
                    .work_limit(1);
                if descending {
                    query = query.descending();
                }
                if let Some(cursor) = &cursor {
                    query = query.after(cursor);
                }
                let page = query.fetch_as::<OptionalValue>().unwrap();
                actual.extend(page.rows.into_iter().map(|r| r.value));
                examined += page.examined;
                assert!(examined <= 5);
                cursor = page.next;
                if cursor.is_none() {
                    break;
                }
            }
            let mut expected = expected;
            if descending {
                expected.reverse();
            }
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn distinct_prefixes_span_work_pages_and_retry_output_bytes() {
    let runtime = host(VectorMemory::default());
    let mut db = create(&runtime, 12);
    for descending in [false, true] {
        let mut cursor = None;
        let mut values = Vec::new();
        let mut examined = 0;
        let mut empty = 0;
        loop {
            let mut query = db
                .query::<Entry>()
                .order_by(&["category"])
                .distinct()
                .limit(1)
                .work_limit(1);
            if descending {
                query = query.descending();
            }
            if let Some(c) = &cursor {
                query = query.after(c);
            }
            let page = query.fetch_as::<CategoryOnly>().unwrap();
            examined += page.examined;
            assert_eq!(page.output_bytes, page.rows.len());
            if page.rows.is_empty() && page.next.is_some() {
                empty += 1;
            }
            values.extend(page.rows.into_iter().map(|r| r.category));
            cursor = page
                .next
                .map(|c| QueryCursor::from_bytes(&c.to_bytes()).unwrap());
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(values, if descending { vec![1, 0] } else { vec![0, 1] });
        assert_eq!(examined, 8);
        assert_eq!(empty, 6);
        let mut q = db
            .query::<Entry>()
            .order_by(&["category"])
            .distinct()
            .byte_limit(1);
        if descending {
            q = q.descending();
        }
        let first = q.fetch_as::<CategoryOnly>().unwrap();
        assert_eq!(first.rows[0].category, u8::from(descending));
        let cursor = first.next.unwrap();
        let mut q = db
            .query::<Entry>()
            .order_by(&["category"])
            .distinct()
            .after(&cursor);
        if descending {
            q = q.descending();
        }
        let next = q.fetch_as::<CategoryOnly>().unwrap();
        assert_eq!(next.rows[0].category, u8::from(!descending));
        assert_eq!(next.rows.len(), 1);
    }
    let constant = db
        .query::<Entry>()
        .using_index("category,amount,id", &[1_u8.into()])
        .distinct()
        .fetch_as::<CategoryOnly>()
        .unwrap();
    assert_eq!(constant.rows, [CategoryOnly { category: 1 }]);
    let first = db
        .query::<Entry>()
        .order_by(&["category"])
        .distinct()
        .limit(1)
        .fetch_as::<CategoryOnly>()
        .unwrap();
    let cursor = first.next.unwrap();
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["category"])
            .after(&cursor)
            .fetch_as::<CategoryOnly>(),
        Err(Error::InvalidCursor)
    ));
    let mut batch = Batch::new();
    batch.delete::<Entry>(2).unwrap();
    db.commit(batch).unwrap();
    let page = db
        .query::<Entry>()
        .order_by(&["category"])
        .distinct()
        .after(&cursor)
        .fetch_as::<CategoryOnly>()
        .unwrap();
    assert_eq!(page.rows, [CategoryOnly { category: 1 }]); // no reread of deleted cursor row
}

#[test]
fn distinct_uses_the_projected_tuple_and_collects_nonadjacent_classes() {
    let runtime = host(VectorMemory::default());
    let mut db = create(&runtime, 13);
    let mut batch = Batch::new();
    let mut expected = std::collections::BTreeSet::new();
    for id in 1..=8 {
        let mut r = row(id);
        r.amount = id % 3;
        expected.insert((r.category, r.amount));
        batch.replace(&r).unwrap();
    }
    db.commit(batch).unwrap();
    let page = db
        .query::<Entry>()
        .using_index("category,amount,id", &[])
        .distinct()
        .fetch_as::<CategoryAmount>()
        .unwrap();
    assert_eq!(
        page.rows
            .into_iter()
            .map(|r| (r.category, r.amount))
            .collect::<Vec<_>>(),
        expected.into_iter().collect::<Vec<_>>()
    );
    for (order, expected) in [
        (None, [1, 0]),
        (Some("amount"), [1, 0]),
        (Some("category"), [0, 1]),
    ] {
        let mut query = db
            .query::<Entry>()
            .filter("amount", C::Ge, 0_u64)
            .distinct();
        if let Some(order) = order {
            query = query.order_by(std::slice::from_ref(&order));
        }
        let page = query.fetch_as::<CategoryOnly>().unwrap();
        assert_eq!(
            page.rows.iter().map(|r| r.category).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(page.examined, 8);
        assert!(page.next.is_none());
    }
    let page = db
        .query::<Entry>()
        .filter("amount", C::Ge, 1_u64)
        .distinct()
        .fetch_as::<Summary>()
        .unwrap();
    assert_eq!(
        page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        [1, 2, 4, 5, 7, 8]
    );
    assert_eq!(
        db.query::<Entry>().distinct().fetch().unwrap().rows.len(),
        8
    );
}

#[test]
fn distinct_filters_reject_or_emit_whole_classes_across_pages() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 14);
    // Independent expected results, including rejected classes before/after matches.
    for (comparison, operand, expected) in [
        (C::Eq, 0_u8, vec![0]),
        (C::Ne, 0, vec![1]),
        (C::Lt, 1, vec![0]),
        (C::Le, 0, vec![0]),
        (C::Gt, 0, vec![1]),
        (C::Ge, 1, vec![1]),
        (C::Ge, 0, vec![0, 1]),
        (C::Gt, 1, vec![]),
    ] {
        for descending in [false, true] {
            for work in [1, 3] {
                let mut cursor = None;
                let mut actual = Vec::new();
                let mut examined = 0;
                loop {
                    let mut query = db
                        .query::<Entry>()
                        .filter("category", comparison, operand)
                        .order_by(&["category"])
                        .distinct()
                        .work_limit(work)
                        .byte_limit(1);
                    if descending {
                        query = query.descending();
                    }
                    if let Some(cursor) = &cursor {
                        query = query.after(cursor);
                    }
                    let page = query.fetch_as::<CategoryOnly>().unwrap();
                    assert!(page.examined <= work);
                    actual.extend(page.rows.into_iter().map(|r| r.category));
                    examined += page.examined;
                    // A byte retry can examine the unreturned candidate twice.
                    assert!(examined <= 9);
                    cursor = page.next;
                    if cursor.is_none() {
                        break;
                    }
                }
                let mut expected = expected.clone();
                if descending {
                    expected.reverse();
                }
                assert_eq!(actual, expected);
            }
        }
    }
}

#[test]
fn complete_distinct_never_returns_a_prefix_or_changes_storage() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = create(&runtime, 91);
    let before = backing.borrow().clone();
    for descending in [false, true] {
        let mut query = db
            .query::<Entry>()
            .distinct()
            .limit(2)
            .byte_limit(2)
            .work_limit(9);
        if descending {
            query = query.descending();
        }
        let page = query.fetch_as::<CategoryOnly>().unwrap();
        assert_eq!(
            page.rows.iter().map(|r| r.category).collect::<Vec<_>>(),
            if descending { [0, 1] } else { [1, 0] }
        );
        assert_eq!((page.examined, page.fetched, page.output_bytes), (8, 8, 2));
        assert!(page.next.is_none());
    }
    for work in [1, 2, 8] {
        assert!(matches!(
            db.query::<Entry>()
                .distinct()
                .work_limit(work)
                .fetch_as::<CategoryOnly>(),
            Err(Error::Limit)
        ));
    }
    assert!(matches!(
        db.query::<Entry>()
            .distinct()
            .limit(1)
            .fetch_as::<CategoryOnly>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .distinct()
            .byte_limit(1)
            .fetch_as::<CategoryOnly>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .distinct()
            .byte_limit(1)
            .fetch_as::<AmountOnly>(),
        Err(Error::RowTooLarge)
    ));
    assert!(
        db.query::<Entry>()
            .distinct()
            .fetch_as::<PayloadOnly>()
            .is_ok()
    );
    let cursor = db
        .query::<Entry>()
        .order_by(&["category"])
        .distinct()
        .limit(1)
        .fetch_as::<CategoryOnly>()
        .unwrap()
        .next
        .unwrap();
    assert!(matches!(
        db.query::<Entry>()
            .distinct()
            .after(&cursor)
            .fetch_as::<CategoryOnly>(),
        Err(Error::InvalidQuery)
    ));
    // Early rows fail within both classes; the first later match still emits each class.
    let filtered = db
        .query::<Entry>()
        .filter("id", C::Ge, 4_u64)
        .order_by(&["category"])
        .distinct()
        .fetch_as::<CategoryOnly>()
        .unwrap();
    assert_eq!(
        filtered.rows.iter().map(|r| r.category).collect::<Vec<_>>(),
        [0, 1]
    );
    let empty = db
        .query::<Entry>()
        .filter("amount", C::Eq, 0_u64)
        .distinct()
        .fetch_as::<CategoryOnly>()
        .unwrap();
    assert!(empty.rows.is_empty() && empty.next.is_none());
    assert_eq!(empty.examined, 8);
    assert_eq!(*backing.borrow(), before);
    for id in 9..=33 {
        let mut batch = Batch::new();
        batch.insert(&row(id)).unwrap();
        db.commit(batch).unwrap();
    }
    let at_cap = db
        .query::<Entry>()
        .filter("id", C::Le, 32_u64)
        .distinct()
        .fetch_as::<AmountOnly>()
        .unwrap();
    assert_eq!(at_cap.rows.len(), 32);
    assert!(at_cap.next.is_none());
    assert!(matches!(
        db.query::<Entry>().distinct().fetch_as::<AmountOnly>(),
        Err(Error::Limit)
    ));
}

#[test]
fn complete_distinct_optional_text_matches_first_occurrence_oracle() {
    let runtime = host(VectorMemory::default());
    let (db, input) = text_rows(&runtime);
    for descending in [false, true] {
        let mut input = input.clone();
        let mut query = db.query::<TextRow>().distinct();
        if descending {
            input.reverse();
            query = query.descending();
        }
        let mut expected = Vec::new();
        for row in input {
            if !expected.contains(&row.text) {
                expected.push(row.text);
            }
        }
        let page = query.fetch_as::<TextOnly>().unwrap();
        assert_eq!(
            page.rows.into_iter().map(|r| r.text).collect::<Vec<_>>(),
            expected
        );
        assert!(page.next.is_none());
    }
}

#[test]
fn distinct_filters_share_automatic_and_explicit_prefix_proofs() {
    let runtime = host(VectorMemory::default());
    let mut db = create(&runtime, 15);
    let mut batch = Batch::new();
    let mut expected = std::collections::BTreeSet::new();
    for id in 1..=8 {
        let mut row = row(id);
        row.amount = id % 3;
        if row.category == 1 && row.amount >= 1 {
            expected.insert(row.amount);
        }
        batch.replace(&row).unwrap();
    }
    db.commit(batch).unwrap();
    for explicit in [false, true] {
        let mut cursor = None;
        let mut actual = Vec::new();
        let mut examined = 0;
        loop {
            let mut query = db
                .query::<Entry>()
                .distinct()
                .filter("category", C::Eq, 1_u8)
                .filter("amount", C::Ge, 1_u64)
                .order_by(&["amount"])
                .work_limit(1);
            if explicit {
                query = query.using_index("category,amount,id", &[1_u8.into()]);
            }
            if let Some(cursor) = &cursor {
                query = query.after(cursor);
            }
            let page = query.fetch_as::<AmountOnly>().unwrap();
            actual.extend(page.rows.into_iter().map(|r| r.amount));
            examined += page.examined;
            assert!(examined <= 4);
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(actual, expected.iter().copied().collect::<Vec<_>>());
        assert_eq!(examined, 3);
    }
    // Fixed-prefix predicates remain checked, including contradictions.
    let empty = db
        .query::<Entry>()
        .using_index("category,amount,id", &[1_u8.into()])
        .filter("category", C::Eq, 0_u8)
        .distinct()
        .fetch_as::<AmountOnly>()
        .unwrap();
    assert!(empty.rows.is_empty());
    assert_eq!(empty.examined, 4);
    // With a varying predicate, the first row of a class may fail and a later
    // duplicate may match. A one-candidate cursor cannot carry that distinction.
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["category"])
            .filter("id", C::Ge, 4_u64)
            .distinct()
            .work_limit(1)
            .fetch_as::<CategoryOnly>(),
        Err(Error::Limit)
    ));
    // Exact operand admission still precedes the DISTINCT proof.
    assert!(matches!(
        db.query::<Entry>()
            .order_by(&["category"])
            .filter("category", C::Eq, 1_u64)
            .distinct()
            .fetch_as::<CategoryOnly>(),
        Err(Error::InvalidOperand)
    ));
}

#[test]
fn exact_counts_and_groups_complete_before_output_limits() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = create(&runtime, 16);
    let mut batch = Batch::new();
    let mut oracle = std::collections::BTreeMap::new();
    for id in 1..=8 {
        let mut r = row(id);
        r.amount = if id % 3 == 0 { 256 } else { 1 };
        if id >= 3 {
            *oracle.entry((r.amount, r.category)).or_insert(0_u64) += 1;
        }
        batch.replace(&r).unwrap();
    }
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    let count = db
        .query::<Entry>()
        .limit(1)
        .byte_limit(8)
        .work_limit(9)
        .count()
        .unwrap();
    assert_eq!(
        (
            count.value,
            count.examined,
            count.fetched,
            count.output_bytes
        ),
        (8, 0, 0, 8)
    );
    let groups = db
        .query::<Entry>()
        .filter("id", C::Ge, 3_u64)
        .group_count::<CategoryAmount>()
        .unwrap();
    let actual: Vec<_> = groups
        .value
        .into_iter()
        .map(|g| ((g.key.amount, g.key.category), g.count))
        .collect();
    assert_eq!(actual, oracle.into_iter().collect::<Vec<_>>());
    assert_eq!(groups.examined, 6);
    // Category duplicates are interleaved in primary order. A result limit must
    // not terminate counting when its first matching row or first group appears.
    let first = db
        .query::<Entry>()
        .limit(1)
        .byte_limit(9)
        .group_count::<CategoryOnly>()
        .unwrap();
    assert_eq!(first.value.len(), 1);
    assert_eq!((first.value[0].key.category, first.value[0].count), (0, 4));
    assert_eq!((first.examined, first.output_bytes), (8, 9));
    let prefix = db
        .query::<Entry>()
        .using_index("category", &[1_u8.into()])
        .count()
        .unwrap();
    assert_eq!((prefix.value, prefix.examined), (4, 4));
    let empty = db
        .query::<Entry>()
        .filter("id", C::Gt, 8_u64)
        .count()
        .unwrap();
    assert_eq!(empty.value, 0);
    let empty = db
        .query::<Entry>()
        .filter("id", C::Gt, 8_u64)
        .group_count::<CategoryOnly>()
        .unwrap();
    assert!(empty.value.is_empty());
    assert_eq!(empty.output_bytes, 0);
    // Row counts include absent grouping operands. These are not present-field counts.
    let groups = db.query::<Entry>().group_count::<OptionalOnly>().unwrap();
    assert_eq!(
        (groups.value[0].key.maybe, groups.value[0].count),
        (None, 2)
    );
    assert_eq!(groups.value.iter().map(|g| g.count).sum::<u64>(), 8);
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn present_counts_and_distinct_tuple_counts_have_different_absence_semantics() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let db = create(&runtime, 21);
    let before = backing.borrow().clone();
    assert_eq!(db.query::<Entry>().count().unwrap().value, 8);
    assert_eq!(db.query::<Entry>().count_field("maybe").unwrap().value, 6);
    assert_eq!(db.query::<Entry>().count_field("payload").unwrap().value, 8);
    let filtered = || {
        db.query::<Entry>()
            .filter("category", C::Eq, 0_u8)
            .filter("amount", C::Ge, 40_u64)
    };
    // IDs 4, 6 and 8 match. Only ID 6 has an absent operand.
    let present = filtered().work_limit(5).count_field("maybe").unwrap();
    assert_eq!(
        (present.value, present.examined, present.fetched),
        (2, 3, 3)
    );
    let validated = filtered().work_limit(5).count_validated().unwrap();
    assert_eq!(
        (validated.value, validated.examined, validated.fetched),
        (3, 3, 3)
    );
    assert!(matches!(
        filtered().work_limit(2).count_field("maybe"),
        Err(Error::Limit)
    ));
    let distinct = filtered()
        .work_limit(5)
        .count_distinct::<OptionalOnly>()
        .unwrap();
    assert_eq!(
        (distinct.value, distinct.examined, distinct.fetched),
        (3, 3, 3)
    );
    assert!(matches!(
        filtered().work_limit(2).count_distinct::<CategoryOnly>(),
        Err(Error::Limit)
    ));
    let grouped = filtered()
        .work_limit(5)
        .group_count_field::<OptionalOnly>("maybe")
        .unwrap();
    assert_eq!((grouped.examined, grouped.fetched), (3, 3));
    assert_eq!(
        grouped
            .value
            .iter()
            .map(|g| (g.key.maybe, g.count))
            .collect::<Vec<_>>(),
        [(None, 0), (Some(4), 1), (Some(8), 1)]
    );
    let distinct = db
        .query::<Entry>()
        .limit(1)
        .byte_limit(8)
        .count_distinct::<OptionalOnly>()
        .unwrap();
    assert_eq!(
        (distinct.value, distinct.examined, distinct.output_bytes),
        (7, 8, 8)
    );
    assert_eq!(
        db.query::<Entry>()
            .filter("maybe", C::Ne, IndexValue::Absent)
            .count_distinct::<OptionalOnly>()
            .unwrap()
            .value,
        6
    );
    let groups = db
        .query::<Entry>()
        .group_count_field::<OptionalOnly>("maybe")
        .unwrap();
    assert_eq!(
        (groups.value[0].key.maybe, groups.value[0].count),
        (None, 0)
    );
    let groups = db
        .query::<Entry>()
        .group_count_field::<CategoryOnly>("maybe")
        .unwrap();
    assert_eq!(
        groups
            .value
            .iter()
            .map(|g| (g.key.category, g.count))
            .collect::<Vec<_>>(),
        [(0, 3), (1, 3)]
    );
    for field in ["maybe", "payload"] {
        assert_eq!(
            db.query::<Entry>()
                .filter("id", C::Gt, 8_u64)
                .count_field(field)
                .unwrap()
                .value,
            0
        );
    }
    assert_eq!(
        db.query::<Entry>()
            .filter("id", C::Gt, 8_u64)
            .count_distinct::<OptionalOnly>()
            .unwrap()
            .value,
        0
    );
    assert!(matches!(
        db.query::<Entry>()
            .filter("id", C::Gt, 8_u64)
            .count_field("missing"),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .group_count_field::<CategoryOnly>("missing"),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .byte_limit(7)
            .count_distinct::<OptionalOnly>(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Entry>().work_limit(8).count_field("maybe"),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .work_limit(8)
            .count_distinct::<OptionalOnly>(),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat8"))),
        field(name = "maybe", value(opt, item(prim = "Nat64"))),
        field(name = "payload", value(opt, item(prim = "Blob", max_bytes = 16))),
        field(name = "items", value(opt, many, max_items = 4, item(prim = "Nat64")))
    )
)]
struct CountInput {}

#[test]
fn field_counts_use_presence_for_empty_and_non_scalar_values() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[CountInput::SCHEMA]).unwrap(),
        [22; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (i, maybe) in [Some(10), None, Some(30), Some(10), None, None]
        .into_iter()
        .enumerate()
    {
        batch
            .insert(&CountInput {
                id: i as u64,
                category: (i % 2) as u8,
                maybe,
                payload: match i {
                    0 => Some(vec![]),
                    2 => Some(vec![0]),
                    _ => None,
                },
                items: match i {
                    0 => Some(vec![]),
                    2 => Some(vec![0, 0]),
                    _ => None,
                },
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    assert_eq!(db.query::<CountInput>().count().unwrap().value, 6);
    let min = db
        .query::<CountInput>()
        .limit(1)
        .min::<OptionalOnly>()
        .unwrap();
    let max = db
        .query::<CountInput>()
        .limit(1)
        .max::<OptionalOnly>()
        .unwrap();
    assert_eq!(min.value.unwrap().maybe, Some(10));
    assert_eq!(max.value.unwrap().maybe, Some(30));
    assert_eq!((min.examined, min.fetched, min.output_bytes), (6, 6, 9));
    let absent = db
        .query::<CountInput>()
        .filter("maybe", C::Eq, IndexValue::Absent)
        .min::<OptionalOnly>()
        .unwrap();
    assert!(absent.value.is_none());
    assert_eq!(absent.output_bytes, 0);
    assert_eq!(
        db.query::<CountInput>().count_field("maybe").unwrap().value,
        3
    );
    assert_eq!(
        db.query::<CountInput>()
            .count_distinct::<OptionalOnly>()
            .unwrap()
            .value,
        3
    );
    assert_eq!(
        db.query::<CountInput>()
            .filter("maybe", C::Ne, IndexValue::Absent)
            .count_distinct::<OptionalOnly>()
            .unwrap()
            .value,
        2
    );
    for field in ["payload", "items"] {
        assert_eq!(
            db.query::<CountInput>().count_field(field).unwrap().value,
            2
        );
        let groups = db
            .query::<CountInput>()
            .group_count_field::<CategoryOnly>(field)
            .unwrap();
        assert_eq!(
            groups
                .value
                .iter()
                .map(|g| (g.key.category, g.count))
                .collect::<Vec<_>>(),
            [(0, 2), (1, 0)]
        );
    }
    let mut batch = Batch::new();
    batch
        .insert(&CountInput {
            id: 6,
            category: 0,
            maybe: Some(0),
            payload: None,
            items: None,
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert_eq!(
        db.query::<CountInput>()
            .min::<OptionalOnly>()
            .unwrap()
            .value
            .unwrap()
            .maybe,
        Some(0)
    );
}

#[test]
fn counts_refuse_incomplete_or_ambiguous_results() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 17);
    for work in [1, 7, 8] {
        assert!(matches!(
            db.query::<Entry>().work_limit(work).count_validated(),
            Err(Error::Limit)
        ));
        assert!(matches!(
            db.query::<Entry>()
                .work_limit(work)
                .limit(1)
                .group_count::<CategoryOnly>(),
            Err(Error::Limit)
        ));
    }
    assert!(matches!(
        db.query::<Entry>().byte_limit(7).count(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .byte_limit(8)
            .group_count::<CategoryOnly>(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .byte_limit(17)
            .group_count::<CategoryOnly>(),
        Err(Error::Limit)
    ));
    assert!(db.query::<Entry>().group_count::<PayloadOnly>().is_ok());
    assert!(matches!(
        db.query::<Entry>().group_count::<WrongWidth>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().group_count::<Empty>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().distinct().count(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().order_by(&["id"]).count(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .descending()
            .group_count::<CategoryOnly>(),
        Err(Error::InvalidQuery)
    ));
    let cursor = db.query::<Entry>().limit(1).fetch().unwrap().next.unwrap();
    assert!(matches!(
        db.query::<Entry>().after(&cursor).count(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().filter("category", C::Eq, 1_u64).count(),
        Err(Error::InvalidOperand)
    ));
}

#[test]
fn grouping_cardinality_is_bounded_independently_of_output_limit() {
    let runtime = host(VectorMemory::default());
    let mut db = create(&runtime, 18);
    let mut batch = Batch::new();
    for id in 9..=32 {
        batch.insert(&row(id)).unwrap();
    }
    db.commit(batch).unwrap();
    assert_eq!(
        db.query::<Entry>()
            .group_count::<Summary>()
            .unwrap()
            .value
            .len(),
        32
    );
    let mut batch = Batch::new();
    batch.insert(&row(33)).unwrap();
    assert_eq!(
        db.query::<Entry>()
            .group_max::<Summary, AmountOnly>()
            .unwrap()
            .value
            .len(),
        32
    );
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<Entry>().limit(1).group_count::<Summary>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>().limit(1).count_distinct::<Summary>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .limit(1)
            .group_min::<Summary, AmountOnly>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter_groups(C::Gt, 100_u64)
            .limit(1)
            .group_count::<Summary>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .limit(1)
            .filter_groups(C::Gt, 999_u128)
            .group_sum::<Summary, WideAmount>(),
        Err(Error::Limit)
    ));
    assert_eq!(db.query::<Entry>().limit(1).count().unwrap().value, 33);
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "label", value(item(prim = "Text", max_bytes = 3000)))
    )
)]
struct GroupText {}
#[ic_db::record(fields(field(name = "label", value(item(prim = "Text", max_bytes = 3000)))))]
struct TextKey {}

#[test]
fn grouping_bytes_are_bounded_separately_from_index_keys_and_output() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[GroupText::SCHEMA]).unwrap(),
        [19; 16],
    )
    .unwrap();
    let text = |id: u64| GroupText {
        id,
        label: format!("{}{}", "\0".repeat(1999), char::from(b'A' + id as u8)),
    };
    let mut batch = Batch::new();
    for id in 0..21 {
        batch.insert(&text(id)).unwrap();
    }
    db.commit(batch).unwrap();
    // These grouping keys exceed the physical index-key cap, but are not indexes.
    let groups = db.query::<GroupText>().group_count::<TextKey>().unwrap();
    assert_eq!(groups.value.len(), 21);
    assert!(groups.value.iter().all(|g| g.count == 1));
    let distinct = db
        .query::<GroupText>()
        .distinct()
        .fetch_as::<TextKey>()
        .unwrap();
    assert_eq!(
        distinct
            .rows
            .iter()
            .map(|r| r.label.clone())
            .collect::<Vec<_>>(),
        (0..21).map(|id| text(id).label).collect::<Vec<_>>()
    );
    assert!(distinct.next.is_none());
    let mut batch = Batch::new();
    batch.insert(&text(21)).unwrap();
    db.commit(batch).unwrap();
    // The 22nd distinct key exceeds retained bytes even with one output group.
    assert!(matches!(
        db.query::<GroupText>().distinct().fetch_as::<TextKey>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<GroupText>().limit(1).group_count::<TextKey>(),
        Err(Error::Limit)
    ));
    let mut batch = Batch::new();
    batch
        .replace(&GroupText {
            id: 0,
            label: "\0".repeat(2100),
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<GroupText>()
            .filter("id", C::Eq, 0_u64)
            .distinct()
            .fetch_as::<TextKey>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<GroupText>()
            .filter("id", C::Eq, 0_u64)
            .group_count::<TextKey>(),
        Err(Error::Limit)
    ));
}

#[entity(
    pk(field = "id"),
    index(field = "price"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "price", value(item(prim = "Decimal", scale = 2)))
    )
)]
struct Priced {}
#[ic_db::record(fields(field(name = "price", value(item(prim = "Decimal", scale = 2)))))]
struct PriceKey {}

#[test]
fn grouping_decimal_equality_agrees_with_filters_and_index_order() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Priced::SCHEMA]).unwrap(),
        [20; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (id, price) in ["1.0", "-2.00", "1.00", "0.00"].into_iter().enumerate() {
        batch
            .insert(&Priced {
                id: id as u64,
                price: price.parse().unwrap(),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let projected = db
        .query::<Priced>()
        .distinct()
        .fetch_as::<PriceKey>()
        .unwrap();
    assert_eq!(
        projected
            .rows
            .iter()
            .map(|r| r.price.coefficient_at_scale(2).unwrap())
            .collect::<Vec<_>>(),
        [100, -200, 0]
    );
    assert!(projected.next.is_none());
    assert_eq!(
        db.query::<Priced>()
            .min::<PriceKey>()
            .unwrap()
            .value
            .unwrap()
            .price
            .coefficient_at_scale(2)
            .unwrap(),
        -200
    );
    assert_eq!(
        db.query::<Priced>()
            .max::<PriceKey>()
            .unwrap()
            .value
            .unwrap()
            .price
            .coefficient_at_scale(2)
            .unwrap(),
        100
    );
    assert_eq!(
        db.query::<Priced>()
            .count_distinct::<PriceKey>()
            .unwrap()
            .value,
        3
    );
    let distinct = db
        .query::<Priced>()
        .group_count_distinct::<PriceKey, PriceKey>()
        .unwrap();
    assert_eq!(
        distinct.value.iter().map(|g| g.count).collect::<Vec<_>>(),
        [1, 1, 1]
    );
    let groups = db.query::<Priced>().group_count::<PriceKey>().unwrap();
    assert_eq!(
        groups
            .value
            .iter()
            .map(|g| (g.key.price.coefficient_at_scale(2).unwrap(), g.count))
            .collect::<Vec<_>>(),
        [(-200, 1), (0, 1), (100, 2)]
    );
    let price: ic_db::Decimal = "1.00".parse().unwrap();
    assert_eq!(
        db.query::<Priced>()
            .filter("price", C::Eq, price)
            .count()
            .unwrap()
            .value,
        2
    );
    assert_eq!(
        db.query::<Priced>()
            .using_index("price", &[price.into()])
            .count()
            .unwrap()
            .value,
        2
    );
}

#[test]
fn extrema_scan_complete_input_and_refuse_ambiguous_shapes() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let db = create(&runtime, 23);
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Entry>().group_min::<CategoryOnly, Summary>(),
        Err(Error::InvalidQuery)
    ));
    assert!(
        db.query::<Entry>()
            .group_max::<CategoryOnly, PayloadOnly>()
            .is_ok()
    );
    assert!(
        db.query::<Entry>()
            .group_max::<PayloadOnly, AmountOnly>()
            .is_ok()
    );
    let min = db
        .query::<Entry>()
        .limit(1)
        .byte_limit(8)
        .min::<AmountOnly>()
        .unwrap();
    let max = db
        .query::<Entry>()
        .limit(1)
        .byte_limit(8)
        .max::<AmountOnly>()
        .unwrap();
    assert_eq!(min.value.unwrap().amount, 10);
    assert_eq!(max.value.unwrap().amount, 80);
    assert_eq!((max.examined, max.fetched, max.output_bytes), (8, 8, 8));
    assert_eq!(
        db.query::<Entry>()
            .using_index("category", &[1_u8.into()])
            .max::<AmountOnly>()
            .unwrap()
            .value
            .unwrap()
            .amount,
        70
    );
    assert!(
        db.query::<Entry>()
            .filter("id", C::Gt, 8_u64)
            .max::<AmountOnly>()
            .unwrap()
            .value
            .is_none()
    );
    assert!(matches!(
        db.query::<Entry>().work_limit(8).max::<AmountOnly>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>().byte_limit(7).min::<AmountOnly>(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Entry>().min::<Summary>(),
        Err(Error::InvalidQuery)
    ));
    assert!(db.query::<Entry>().min::<PayloadOnly>().is_ok());
    assert!(matches!(
        db.query::<Entry>().min::<WrongWidth>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().min::<Missing>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().min::<Empty>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().distinct().min::<AmountOnly>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().order_by(&["id"]).max::<AmountOnly>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().descending().max::<AmountOnly>(),
        Err(Error::InvalidQuery)
    ));
    let cursor = db.query::<Entry>().limit(1).fetch().unwrap().next.unwrap();
    assert!(matches!(
        db.query::<Entry>().after(&cursor).min::<AmountOnly>(),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn extrema_own_text_winners_and_bound_comparison_and_final_output_separately() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[GroupText::SCHEMA]).unwrap(),
        [24; 16],
    )
    .unwrap();
    let labels = [
        "z".repeat(1000),
        "middle".into(),
        "a".into(),
        "Ω".into(),
        "\0x".into(),
    ];
    let mut batch = Batch::new();
    for (id, label) in labels.iter().enumerate() {
        batch
            .insert(&GroupText {
                id: id as u64,
                label: label.clone(),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    // An oversized provisional winner must not refuse a smaller final answer.
    let min = db
        .query::<GroupText>()
        .byte_limit(6)
        .min::<TextKey>()
        .unwrap();
    let max = db
        .query::<GroupText>()
        .byte_limit(6)
        .max::<TextKey>()
        .unwrap();
    assert_eq!(&min.value.unwrap().label, labels.iter().min().unwrap());
    assert_eq!(&max.value.unwrap().label, labels.iter().max().unwrap());
    assert_eq!((min.examined, min.output_bytes), (5, 6));
    assert!(matches!(
        db.query::<GroupText>().byte_limit(5).min::<TextKey>(),
        Err(Error::RowTooLarge)
    ));
    let mut batch = Batch::new();
    batch
        .insert(&GroupText {
            id: 5,
            label: "\0".repeat(2100),
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<GroupText>().max::<TextKey>(),
        Err(Error::Limit)
    ));
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(opt, item(prim = "Nat8"))),
        field(name = "maybe", value(opt, item(prim = "Nat64")))
    )
)]
struct GroupedOptional {}
#[ic_db::record(fields(field(name = "category", value(opt, item(prim = "Nat8")))))]
struct OptionalCategory {}
#[ic_db::record(fields(
    field(name = "category", value(opt, item(prim = "Nat8"))),
    field(name = "maybe", value(opt, item(prim = "Nat64")))
))]
struct OptionalGroupTuple {}

#[test]
fn grouped_extrema_keep_absent_keys_and_all_absent_groups() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[GroupedOptional::SCHEMA]).unwrap(),
        [25; 16],
    )
    .unwrap();
    let inputs = [
        (None, Some(10)),
        (Some(1), None),
        (None, Some(30)),
        (Some(2), Some(0)),
        (Some(1), None),
        (Some(2), Some(5)),
    ];
    let mut oracle = std::collections::BTreeMap::<_, Vec<u64>>::new();
    let mut batch = Batch::new();
    for (id, (category, maybe)) in inputs.into_iter().enumerate() {
        oracle.entry(category).or_default().extend(maybe);
        batch
            .insert(&GroupedOptional {
                id: id as u64,
                category,
                maybe,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    let min = db
        .query::<GroupedOptional>()
        .group_min::<OptionalCategory, OptionalOnly>()
        .unwrap();
    let max = db
        .query::<GroupedOptional>()
        .group_max::<OptionalCategory, OptionalOnly>()
        .unwrap();
    let minimum: Vec<_> = min
        .value
        .into_iter()
        .map(|g| (g.key.category, g.value.and_then(|r| r.maybe)))
        .collect();
    let maximum: Vec<_> = max
        .value
        .into_iter()
        .map(|g| (g.key.category, g.value.and_then(|r| r.maybe)))
        .collect();
    assert_eq!(
        minimum,
        oracle
            .iter()
            .map(|(k, v)| (*k, v.iter().min().copied()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        maximum,
        oracle
            .iter()
            .map(|(k, v)| (*k, v.iter().max().copied()))
            .collect::<Vec<_>>()
    );
    assert_eq!((min.examined, min.fetched, min.output_bytes), (6, 6, 23));
    let limited = db
        .query::<GroupedOptional>()
        .limit(1)
        .byte_limit(10)
        .group_max::<OptionalCategory, OptionalOnly>()
        .unwrap();
    assert_eq!(limited.value.len(), 1);
    assert_eq!(limited.value[0].key.category, None);
    assert_eq!(limited.value[0].value.as_ref().unwrap().maybe, Some(30));
    assert_eq!((limited.examined, limited.output_bytes), (6, 10));
    let absent = db
        .query::<GroupedOptional>()
        .filter("category", C::Eq, 1_u8)
        .byte_limit(2)
        .group_min::<OptionalCategory, OptionalOnly>()
        .unwrap();
    assert_eq!(absent.value.len(), 1);
    assert!(absent.value[0].value.is_none());
    for (order, expected) in [
        (
            ic_db::GroupOrder::AggregateAscending,
            vec![Some(1), Some(2), None],
        ),
        (
            ic_db::GroupOrder::AggregateDescending,
            vec![None, Some(2), Some(1)],
        ),
    ] {
        let sorted = db
            .query::<GroupedOptional>()
            .order_groups(order)
            .group_min::<OptionalCategory, OptionalOnly>()
            .unwrap();
        assert_eq!(
            sorted
                .value
                .into_iter()
                .map(|g| g.key.category)
                .collect::<Vec<_>>(),
            expected
        );
    }
    for (comparison, operand, expected) in [
        (C::Eq, IndexValue::Absent, vec![Some(1)]),
        (C::Ne, IndexValue::Absent, vec![None, Some(2)]),
        (C::Ge, IndexValue::Nat64(0), vec![None, Some(2)]),
        (C::Lt, IndexValue::Nat64(10), vec![Some(2)]),
    ] {
        let filtered = db
            .query::<GroupedOptional>()
            .filter_groups(comparison, operand)
            .group_min::<OptionalCategory, OptionalOnly>()
            .unwrap();
        assert_eq!(
            filtered
                .value
                .into_iter()
                .map(|g| g.key.category)
                .collect::<Vec<_>>(),
            expected
        );
    }
    let absent_output = db
        .query::<GroupedOptional>()
        .filter_groups(C::Eq, IndexValue::Absent)
        .byte_limit(2)
        .group_min::<OptionalCategory, OptionalOnly>()
        .unwrap();
    assert_eq!(absent_output.output_bytes, 2);
    assert_eq!(absent_output.value.len(), 1);
    let zero = db
        .query::<GroupedOptional>()
        .filter_groups(C::Eq, 0_u64)
        .group_count_field::<OptionalCategory>("maybe")
        .unwrap();
    assert_eq!(zero.value.len(), 1);
    assert_eq!(
        (zero.value[0].key.category, zero.value[0].count),
        (Some(1), 0)
    );
    let overlap = db
        .query::<GroupedOptional>()
        .group_min::<OptionalOnly, OptionalOnly>()
        .unwrap();
    assert_eq!(overlap.value[0].key.maybe, None);
    assert!(overlap.value[0].value.is_none());
    assert!(
        overlap
            .value
            .iter()
            .skip(1)
            .all(|g| g.key.maybe == g.value.as_ref().unwrap().maybe)
    );
    assert!(
        db.query::<GroupedOptional>()
            .filter("id", C::Gt, 6_u64)
            .group_min::<OptionalCategory, OptionalOnly>()
            .unwrap()
            .value
            .is_empty()
    );
    assert!(matches!(
        db.query::<GroupedOptional>()
            .work_limit(6)
            .group_min::<OptionalCategory, OptionalOnly>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<GroupedOptional>()
            .limit(1)
            .byte_limit(9)
            .group_max::<OptionalCategory, OptionalOnly>(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<GroupedOptional>()
            .byte_limit(22)
            .group_max::<OptionalCategory, OptionalOnly>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<GroupedOptional>()
            .group_min::<OptionalCategory, WrongPresence>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<GroupedOptional>()
            .group_min::<OptionalCategory, Missing>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<GroupedOptional>()
            .group_max::<OptionalCategory, Empty>(),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat8"))),
        field(name = "label", value(item(prim = "Text", max_bytes = 3000)))
    )
)]
struct GroupedText {}

#[test]
fn grouped_winner_replacement_reclaims_state_and_growth_can_refuse() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[GroupedText::SCHEMA]).unwrap(),
        [26; 16],
    )
    .unwrap();
    // Fifteen large winners fit. Shrinking each winner must release its charge
    // before the next wave of large winners, rather than accumulating history.
    for (phase, label) in [
        format!("b{}", "x".repeat(2800)),
        "c".into(),
        format!("d{}", "x".repeat(2800)),
        "z".into(),
    ]
    .into_iter()
    .enumerate()
    {
        let mut batch = Batch::new();
        for category in 0..15 {
            batch
                .insert(&GroupedText {
                    id: (phase * 32 + category) as u64,
                    category: category as u8,
                    label: label.clone(),
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
    }
    // Output admission uses the small final winners, not provisional values.
    let result = db
        .query::<GroupedText>()
        .byte_limit(90)
        .group_max::<CategoryOnly, TextKey>()
        .unwrap();
    assert_eq!(
        (result.value.len(), result.examined, result.output_bytes),
        (15, 60, 90)
    );
    assert!(
        result
            .value
            .iter()
            .all(|g| g.value.as_ref().unwrap().label == "z")
    );
    // Only existing groups change here: their MIN winners now grow beyond the
    // shared state envelope. MAX still fits, so the source itself is valid.
    for phase in 0..2 {
        let mut batch = Batch::new();
        for category in 0..32 {
            batch
                .insert(&GroupedText {
                    id: 128 + (phase * 32 + category) as u64,
                    category: category as u8,
                    label: if phase == 0 {
                        "b".into()
                    } else {
                        format!("a{}", "x".repeat(2800))
                    },
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
    }
    assert!(matches!(
        db.query::<GroupedText>()
            .filter("id", C::Ge, 128_u64)
            .work_limit(200)
            .limit(1)
            .filter_groups(C::Ge, "zz")
            .group_min::<CategoryOnly, TextKey>(),
        Err(Error::Limit)
    ));
    let max = db
        .query::<GroupedText>()
        .filter("id", C::Ge, 128_u64)
        .work_limit(200)
        .group_max::<CategoryOnly, TextKey>()
        .unwrap();
    assert_eq!(max.value.len(), 32);
    assert!(
        max.value
            .iter()
            .all(|g| g.value.as_ref().unwrap().label == "b")
    );
}

#[test]
fn completed_group_selection_precedes_output_limits_not_input_bounds() {
    use ic_db::GroupOrder as O;
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let db = create(&runtime, 28);
    let before = backing.borrow().clone();
    // Matching input is (category,amount): (0,60),(1,70),(0,80).
    // Both row filtering and full accumulation must precede the group predicate.
    for reordered in [false, true] {
        let query = db.query::<Entry>();
        let query = if reordered {
            query
                .limit(1)
                .order_groups(O::AggregateDescending)
                .filter_groups(C::Ge, 2_u64)
                .filter("amount", C::Ge, 60_u64)
        } else {
            query
                .filter("amount", C::Ge, 60_u64)
                .filter_groups(C::Ge, 2_u64)
                .order_groups(O::AggregateDescending)
                .limit(1)
        };
        let result = query.byte_limit(9).group_count::<CategoryOnly>().unwrap();
        assert_eq!(result.value.len(), 1);
        assert_eq!(
            (result.value[0].key.category, result.value[0].count),
            (0, 2)
        );
        assert_eq!((result.examined, result.output_bytes), (3, 9));
    }
    for (order, expected) in [
        (O::AggregateAscending, vec![(1, 1), (0, 2)]),
        (O::AggregateDescending, vec![(0, 2), (1, 1)]),
    ] {
        let result = db
            .query::<Entry>()
            .filter("amount", C::Ge, 60_u64)
            .order_groups(order)
            .group_count::<CategoryOnly>()
            .unwrap();
        assert_eq!(
            result
                .value
                .into_iter()
                .map(|g| (g.key.category, g.count))
                .collect::<Vec<_>>(),
            expected
        );
        // Equal counts keep key order even when aggregate ordering is reversed.
        let tied = db
            .query::<Entry>()
            .order_groups(order)
            .group_count::<CategoryOnly>()
            .unwrap();
        assert_eq!(
            tied.value
                .into_iter()
                .map(|g| g.key.category)
                .collect::<Vec<_>>(),
            [0, 1]
        );
    }
    let minimum = db
        .query::<Entry>()
        .filter_groups(C::Ge, 20_u64)
        .order_groups(O::AggregateDescending)
        .byte_limit(9)
        .group_min::<CategoryOnly, AmountOnly>()
        .unwrap();
    assert_eq!(minimum.value.len(), 1);
    assert_eq!(minimum.value[0].key.category, 0);
    assert_eq!(minimum.value[0].value.as_ref().unwrap().amount, 20);
    let none = db
        .query::<Entry>()
        .filter_groups(C::Gt, 8_u64)
        .byte_limit(1)
        .group_count::<CategoryOnly>()
        .unwrap();
    assert!(none.value.is_empty());
    assert_eq!(none.output_bytes, 0);
    assert!(matches!(
        db.query::<Entry>()
            .filter_groups(C::Gt, 8_u64)
            .work_limit(8)
            .group_count::<CategoryOnly>(),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn group_or_clauses_select_completed_values_before_order_and_limits() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 29);
    for (lower, upper) in [(0, 3), (1, 2), (2, 1), (3, 0)] {
        let alternatives = [
            (C::Le, IndexValue::Nat64(lower)),
            (C::Ge, IndexValue::Nat64(upper)),
        ];
        let result = db
            .query::<Entry>()
            .filter("amount", C::Ge, 60_u64)
            .filter_groups_any(&alternatives)
            .filter_groups(C::Ne, 0_u64)
            .order_groups(ic_db::GroupOrder::AggregateDescending)
            .limit(1)
            .group_count::<CategoryOnly>()
            .unwrap();
        let expected: Vec<_> = [(0, 2), (1, 1)]
            .into_iter()
            .filter(|(_, count)| *count <= lower || *count >= upper)
            .take(1)
            .collect();
        assert_eq!(
            result
                .value
                .into_iter()
                .map(|g| (g.key.category, g.count))
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(result.examined, 3);
    }
    let result = db
        .query::<Entry>()
        .filter_groups_any(&[(C::Lt, 160_u128.into()), (C::Ge, 200_u128.into())])
        .group_sum::<CategoryOnly, WideAmount>()
        .unwrap();
    assert_eq!(
        result
            .value
            .into_iter()
            .map(|g| (g.key.category, g.value.unwrap().amount))
            .collect::<Vec<_>>(),
        [(0, 200)]
    );

    let (text, _) = text_rows(&host(VectorMemory::default()));
    let all = text
        .query::<TextRow>()
        .group_max::<TextBucket, TextOnly>()
        .unwrap();
    let expected: Vec<_> = all
        .value
        .into_iter()
        .filter(|g| {
            g.value
                .as_ref()
                .is_none_or(|v| v.text.as_deref().is_some_and(|s| s.contains('a')))
        })
        .collect();
    let selected = text
        .query::<TextRow>()
        .filter_groups_any(&[(C::Eq, IndexValue::Absent), (C::Contains, "a".into())])
        .group_max::<TextBucket, TextOnly>()
        .unwrap();
    assert_eq!(
        selected
            .value
            .into_iter()
            .map(|g| (g.key.bucket, g.value.map(|v| v.text)))
            .collect::<Vec<_>>(),
        expected
            .into_iter()
            .map(|g| (g.key.bucket, g.value.map(|v| v.text)))
            .collect::<Vec<_>>()
    );
}

#[test]
fn group_or_admits_all_operands_and_shares_the_group_term_budget() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 29);
    for alternatives in [&[][..], &[(C::Ge, IndexValue::Nat64(0)); 9][..]] {
        assert!(matches!(
            db.query::<Entry>()
                .filter_groups_any(alternatives)
                .group_count::<CategoryOnly>(),
            Err(Error::Limit)
        ));
    }
    let eight = [(C::Ge, IndexValue::Nat64(0)); 8];
    assert_eq!(
        db.query::<Entry>()
            .filter_groups_any(&eight)
            .group_count::<CategoryOnly>()
            .unwrap()
            .value
            .len(),
        2
    );
    assert!(matches!(
        db.query::<Entry>()
            .filter_groups(C::Ge, 0_u64)
            .filter_groups_any(&eight)
            .group_count::<CategoryOnly>(),
        Err(Error::Limit)
    ));
    for bad in [IndexValue::Absent, IndexValue::Nat8(1)] {
        assert!(matches!(
            db.query::<Entry>()
                .filter_groups_any(&[(C::Ge, 0_u64.into()), (C::Eq, bad)])
                .group_count::<CategoryOnly>(),
            Err(Error::InvalidOperand)
        ));
    }
    let one = [(C::Ge, IndexValue::Nat64(0))];
    assert!(matches!(
        db.query::<Entry>().filter_groups_any(&one).fetch(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().filter_groups_any(&one).count(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter_groups_any(&one)
            .min::<AmountOnly>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter_groups_any(&one)
            .work_limit(2)
            .group_count::<CategoryOnly>(),
        Err(Error::Limit)
    ));
}

#[test]
fn group_modifiers_refuse_incompatible_terminals_and_bad_operands() {
    use ic_db::GroupOrder as O;
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 29);
    for filter in [true, false] {
        let query = || {
            if filter {
                db.query::<Entry>().filter_groups(C::Ge, 0_u64)
            } else {
                db.query::<Entry>().order_groups(O::Key)
            }
        };
        assert!(matches!(query().fetch(), Err(Error::InvalidQuery)));
        assert!(matches!(
            query().fetch_as::<CategoryOnly>(),
            Err(Error::InvalidQuery)
        ));
        assert!(matches!(query().count(), Err(Error::InvalidQuery)));
        assert!(matches!(
            query().count_field("amount"),
            Err(Error::InvalidQuery)
        ));
        assert!(matches!(
            query().count_distinct::<CategoryOnly>(),
            Err(Error::InvalidQuery)
        ));
        assert!(matches!(
            query().min::<AmountOnly>(),
            Err(Error::InvalidQuery)
        ));
        assert!(matches!(
            query().max::<AmountOnly>(),
            Err(Error::InvalidQuery)
        ));
    }
    for value in [
        IndexValue::Nat8(1),
        IndexValue::Text("1"),
        IndexValue::Absent,
    ] {
        assert!(matches!(
            db.query::<Entry>()
                .filter("id", C::Gt, 100_u64)
                .filter_groups(C::Eq, value)
                .group_count::<CategoryOnly>(),
            Err(Error::InvalidOperand)
        ));
    }
    assert!(matches!(
        db.query::<Entry>()
            .filter("id", C::Gt, 100_u64)
            .filter_groups(C::Eq, 1_u8)
            .group_min::<CategoryOnly, AmountOnly>(),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter_groups(C::Ge, IndexValue::Absent)
            .group_max::<CategoryOnly, AmountOnly>(),
        Err(Error::InvalidOperand)
    ));
    let mut query = db.query::<Entry>();
    for _ in 0..8 {
        query = query.filter_groups(C::Ge, 0_u64);
    }
    assert_eq!(query.group_count::<CategoryOnly>().unwrap().value.len(), 2);
    let mut query = db.query::<Entry>();
    for _ in 0..9 {
        query = query.filter_groups(C::Ge, 0_u64);
    }
    assert!(matches!(
        query.group_count::<CategoryOnly>(),
        Err(Error::Limit)
    ));
}

#[test]
fn integer_sums_cover_each_width_without_float_conversion() {
    macro_rules! check {
        ($prim:literal) => {{
            #[entity(pk(field="id"), fields(
                                                        field(name="id",value(item(prim="Nat64"))),
                                                        field(name="number",value(item(prim=$prim)))
                                                    ))]
            struct Input {}
            #[ic_db::record(fields(field(name="number",value(item(prim=$prim)))))]
            struct Output {}
            let runtime = host(VectorMemory::default());
            let mut db = Database::create(
                runtime
                    .open_memory(&StableKey::parse("test.db.v1").unwrap())
                    .unwrap(),
                Schema::new(&[Input::SCHEMA]).unwrap(),
                [31; 16],
            )
            .unwrap();
            assert!(db.query::<Input>().sum::<Output>().unwrap().value.is_none());
            let mut batch = Batch::new();
            batch
                .insert(&Input {
                    id: 1,
                    number: 2_u8.try_into().unwrap(),
                })
                .unwrap();
            batch
                .insert(&Input {
                    id: 2,
                    number: 3_u8.try_into().unwrap(),
                })
                .unwrap();
            db.commit(batch).unwrap();
            assert_eq!(
                db.query::<Input>()
                    .sum::<Output>()
                    .unwrap()
                    .value
                    .unwrap()
                    .number,
                Output {
                    number: 5_u8.try_into().unwrap()
                }
                .number
            );
        }};
    }
    check!("Nat8");
    check!("Nat16");
    check!("Nat32");
    check!("Nat64");
    check!("Nat128");
    check!("U256");
    check!("Int8");
    check!("Int16");
    check!("Int32");
    check!("Int64");
    check!("Int128");
}

#[ic_db::record(fields(field(name = "amount", value(item(prim = "Nat128")))))]
struct WideAmount {}
#[ic_db::record(fields(field(name = "amount", value(item(prim = "U256")))))]
struct HugeAmount {}
#[ic_db::record(fields(field(name = "amount", value(item(prim = "Nat32")))))]
struct NarrowAmount {}
#[ic_db::record(fields(field(name = "amount", value(item(prim = "Int128")))))]
struct SignedAmount {}
#[test]
fn sums_widen_only_explicitly_and_keep_work_output_and_shape_bounds() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = create(&runtime, 32);
    let before = backing.borrow().clone();
    let sum = db
        .query::<Entry>()
        .limit(1)
        .byte_limit(16)
        .sum::<WideAmount>()
        .unwrap();
    assert_eq!(sum.value.unwrap().amount, 360);
    assert_eq!((sum.examined, sum.fetched, sum.output_bytes), (8, 8, 16));
    assert_eq!(
        db.query::<Entry>()
            .filter("amount", C::Ge, 60_u64)
            .sum::<WideAmount>()
            .unwrap()
            .value
            .unwrap()
            .amount,
        210
    );
    assert!(
        db.query::<Entry>()
            .filter("id", C::Gt, 100_u64)
            .byte_limit(1)
            .sum::<WideAmount>()
            .unwrap()
            .value
            .is_none()
    );
    assert_eq!(
        db.query::<Entry>()
            .sum::<OptionalOnly>()
            .unwrap()
            .value
            .unwrap()
            .maybe,
        Some(27)
    );
    assert!(
        db.query::<Entry>()
            .filter("maybe", C::Eq, IndexValue::Absent)
            .sum::<OptionalOnly>()
            .unwrap()
            .value
            .is_none()
    );
    assert!(matches!(
        db.query::<Entry>().work_limit(8).sum::<WideAmount>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>().byte_limit(15).sum::<WideAmount>(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Entry>().sum::<NarrowAmount>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().sum::<SignedAmount>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().sum::<Summary>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().sum::<PayloadOnly>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().sum::<WrongPresence>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter("id", C::Gt, 100_u64)
            .sum::<Missing>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().distinct().sum::<WideAmount>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().order_by(&["id"]).sum::<WideAmount>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().descending().sum::<WideAmount>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .filter_groups(C::Ge, 0_u64)
            .sum::<WideAmount>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .order_groups(ic_db::GroupOrder::Key)
            .sum::<WideAmount>(),
        Err(Error::InvalidQuery)
    ));
    let cursor = db.query::<Entry>().limit(1).fetch().unwrap().next.unwrap();
    assert!(matches!(
        db.query::<Entry>().after(&cursor).sum::<WideAmount>(),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    for id in 1..=2 {
        let mut r = row(id);
        r.amount = u64::MAX;
        batch.replace(&r).unwrap();
    }
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<Entry>().sum::<AmountOnly>(),
        Err(Error::ArithmeticOverflow)
    ));
    let expected = u128::from(u64::MAX) * 2 + 330;
    assert_eq!(
        db.query::<Entry>()
            .sum::<WideAmount>()
            .unwrap()
            .value
            .unwrap()
            .amount,
        expected
    );
    assert_eq!(
        db.query::<Entry>()
            .sum::<HugeAmount>()
            .unwrap()
            .value
            .unwrap()
            .amount,
        ic_db::U256::from(expected)
    );
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "number", value(item(prim = "Int128")))
    )
)]
struct SignedSum {}
#[ic_db::record(fields(field(name = "number", value(item(prim = "Int128")))))]
struct SignedTotal {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "number", value(item(prim = "U256")))
    )
)]
struct HugeSum {}
#[ic_db::record(fields(field(name = "number", value(item(prim = "U256")))))]
struct HugeTotal {}
#[test]
fn sums_distinguish_final_range_overflow_from_cancelling_signed_prefixes() {
    for inputs in [
        [i128::MAX, 1, -1],
        [1, i128::MAX, -1],
        [-1, 1, i128::MAX],
        [i128::MIN, -1, 1],
        [-1, i128::MIN, 1],
        [1, -1, i128::MIN],
    ] {
        let runtime = host(VectorMemory::default());
        let mut db = Database::create(
            runtime
                .open_memory(&StableKey::parse("test.db.v1").unwrap())
                .unwrap(),
            Schema::new(&[SignedSum::SCHEMA]).unwrap(),
            [33; 16],
        )
        .unwrap();
        let mut batch = Batch::new();
        for (id, number) in inputs.into_iter().enumerate() {
            batch
                .insert(&SignedSum {
                    id: id as u64,
                    number,
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
        let expected = if inputs.contains(&i128::MAX) {
            i128::MAX
        } else {
            i128::MIN
        };
        assert_eq!(
            db.query::<SignedSum>()
                .sum::<SignedTotal>()
                .unwrap()
                .value
                .unwrap()
                .number,
            expected
        );
        let prefix = db
            .query::<SignedSum>()
            .filter("id", C::Le, 1_u64)
            .sum::<SignedTotal>();
        match inputs[0].checked_add(inputs[1]) {
            Some(expected) => assert_eq!(prefix.unwrap().value.unwrap().number, expected),
            None => assert!(matches!(prefix, Err(Error::ArithmeticOverflow))),
        }
        let mut batch = Batch::new();
        for id in 0..3 {
            batch.replace(&SignedSum { id, number: 0 }).unwrap();
        }
        db.commit(batch).unwrap();
        assert_eq!(
            db.query::<SignedSum>()
                .sum::<SignedTotal>()
                .unwrap()
                .value
                .unwrap()
                .number,
            0
        );
    }
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[HugeSum::SCHEMA]).unwrap(),
        [34; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    batch
        .insert(&HugeSum {
            id: 1,
            number: ic_db::U256::MAX,
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert_eq!(
        db.query::<HugeSum>()
            .sum::<HugeTotal>()
            .unwrap()
            .value
            .unwrap()
            .number,
        ic_db::U256::MAX
    );
    let mut batch = Batch::new();
    batch
        .insert(&HugeSum {
            id: 2,
            number: ic_db::U256::ONE,
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<HugeSum>().sum::<HugeTotal>(),
        Err(Error::ArithmeticOverflow)
    ));
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "number", value(item(prim = "Nat128")))
    )
)]
struct Unsigned128Sum {}
#[ic_db::record(fields(field(name = "number", value(item(prim = "Nat128")))))]
struct Unsigned128Total {}
#[test]
fn unsigned_128_totals_can_widen_to_u256() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Unsigned128Sum::SCHEMA]).unwrap(),
        [35; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for id in 0..2 {
        batch
            .insert(&Unsigned128Sum {
                id,
                number: u128::MAX,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<Unsigned128Sum>().sum::<Unsigned128Total>(),
        Err(Error::ArithmeticOverflow)
    ));
    let expected = ic_db::U256::from(u128::MAX) * 2;
    assert_eq!(
        db.query::<Unsigned128Sum>()
            .sum::<HugeTotal>()
            .unwrap()
            .value
            .unwrap()
            .number,
        expected
    );
}

#[test]
fn grouped_sums_finish_before_filter_sort_and_limit() {
    use ic_db::GroupOrder as O;
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let db = create(&runtime, 36);
    let before = backing.borrow().clone();
    let all = db
        .query::<Entry>()
        .order_groups(O::AggregateAscending)
        .group_sum::<CategoryOnly, WideAmount>()
        .unwrap();
    assert_eq!(
        all.value
            .into_iter()
            .map(|g| (g.key.category, g.value.unwrap().amount))
            .collect::<Vec<_>>(),
        [(1, 160), (0, 200)]
    );
    assert_eq!((all.examined, all.output_bytes), (8, 34));
    let filtered = db
        .query::<Entry>()
        .limit(1)
        .filter_groups(C::Ge, 180_u128)
        .order_groups(O::AggregateDescending)
        .byte_limit(17)
        .group_sum::<CategoryOnly, WideAmount>()
        .unwrap();
    assert_eq!(filtered.value[0].key.category, 0);
    assert_eq!(filtered.value[0].value.as_ref().unwrap().amount, 200);
    let inputs = db
        .query::<Entry>()
        .filter("amount", C::Ge, 60_u64)
        .filter_groups(C::Ge, 100_u128)
        .group_sum::<CategoryOnly, WideAmount>()
        .unwrap();
    assert_eq!(inputs.value.len(), 1);
    assert_eq!(inputs.value[0].value.as_ref().unwrap().amount, 140);
    let overlap = db
        .query::<Entry>()
        .group_sum::<AmountOnly, WideAmount>()
        .unwrap();
    assert!(
        overlap
            .value
            .iter()
            .all(|g| u128::from(g.key.amount) == g.value.as_ref().unwrap().amount)
    );
    assert!(
        db.query::<Entry>()
            .filter("id", C::Gt, 100_u64)
            .group_sum::<CategoryOnly, WideAmount>()
            .unwrap()
            .value
            .is_empty()
    );
    assert!(matches!(
        db.query::<Entry>()
            .filter_groups(C::Ge, 180_u64)
            .group_sum::<CategoryOnly, WideAmount>(),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .group_sum::<CategoryOnly, NarrowAmount>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .work_limit(8)
            .filter_groups(C::Gt, 999_u128)
            .group_sum::<CategoryOnly, WideAmount>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .byte_limit(16)
            .group_sum::<CategoryOnly, WideAmount>(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Entry>()
            .byte_limit(33)
            .group_sum::<CategoryOnly, WideAmount>(),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[ic_db::record(fields(field(name = "maybe", value(opt, item(prim = "Nat128")))))]
struct WideOptional {}
#[test]
fn grouped_sums_keep_absence_and_zero_distinct() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[GroupedOptional::SCHEMA]).unwrap(),
        [37; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (id, (category, maybe)) in [
        (None, Some(10)),
        (Some(1), None),
        (None, Some(30)),
        (Some(2), Some(0)),
        (Some(1), None),
    ]
    .into_iter()
    .enumerate()
    {
        batch
            .insert(&GroupedOptional {
                id: id as u64,
                category,
                maybe,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    for (order, expected) in [
        (
            ic_db::GroupOrder::Key,
            vec![(None, Some(40)), (Some(1), None), (Some(2), Some(0))],
        ),
        (
            ic_db::GroupOrder::AggregateAscending,
            vec![(Some(1), None), (Some(2), Some(0)), (None, Some(40))],
        ),
        (
            ic_db::GroupOrder::AggregateDescending,
            vec![(None, Some(40)), (Some(2), Some(0)), (Some(1), None)],
        ),
    ] {
        let result = db
            .query::<GroupedOptional>()
            .order_groups(order)
            .group_sum::<OptionalCategory, WideOptional>()
            .unwrap();
        assert_eq!(
            result
                .value
                .into_iter()
                .map(|g| (g.key.category, g.value.and_then(|r| r.maybe)))
                .collect::<Vec<_>>(),
            expected
        );
    }
    let absent = db
        .query::<GroupedOptional>()
        .filter_groups(C::Eq, IndexValue::Absent)
        .byte_limit(2)
        .group_sum::<OptionalCategory, WideOptional>()
        .unwrap();
    assert_eq!(absent.value.len(), 1);
    assert_eq!(absent.value[0].key.category, Some(1));
    assert!(absent.value[0].value.is_none());
    assert_eq!(absent.output_bytes, 2);
    let zero = db
        .query::<GroupedOptional>()
        .filter_groups(C::Eq, 0_u128)
        .group_sum::<OptionalCategory, WideOptional>()
        .unwrap();
    assert_eq!(zero.value.len(), 1);
    assert_eq!(zero.value[0].key.category, Some(2));
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat8"))),
        field(name = "amount", value(item(prim = "Int128")))
    )
)]
struct SignedGroups {}
#[test]
fn grouped_overflow_cannot_hide_behind_filter_or_limit() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[SignedGroups::SCHEMA]).unwrap(),
        [38; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (id, category, amount) in [(0, 0, 3), (1, 1, i128::MAX), (2, 1, 1)] {
        batch
            .insert(&SignedGroups {
                id,
                category,
                amount,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    for filtered in [false, true] {
        let query = db.query::<SignedGroups>().limit(1);
        let query = if filtered {
            query.filter_groups(C::Eq, 3_i128)
        } else {
            query
        };
        assert!(matches!(
            query.group_sum::<CategoryOnly, SignedAmount>(),
            Err(Error::ArithmeticOverflow)
        ));
    }
    assert!(matches!(
        db.query::<SignedGroups>()
            .filter_groups_any(&[(C::Eq, 3_i128.into()), (C::Lt, 0_i128.into())])
            .limit(1)
            .group_sum::<CategoryOnly, SignedAmount>(),
        Err(Error::ArithmeticOverflow)
    ));
    let mut batch = Batch::new();
    batch
        .insert(&SignedGroups {
            id: 3,
            category: 1,
            amount: -1,
        })
        .unwrap();
    db.commit(batch).unwrap();
    let result = db
        .query::<SignedGroups>()
        .order_groups(ic_db::GroupOrder::AggregateDescending)
        .group_sum::<CategoryOnly, SignedAmount>()
        .unwrap();
    assert_eq!(result.value[0].key.category, 1);
    assert_eq!(result.value[0].value.as_ref().unwrap().amount, i128::MAX);
    let mut batch = Batch::new();
    batch
        .replace(&SignedGroups {
            id: 1,
            category: 1,
            amount: 3,
        })
        .unwrap();
    db.commit(batch).unwrap();
    for order in [
        ic_db::GroupOrder::AggregateAscending,
        ic_db::GroupOrder::AggregateDescending,
    ] {
        let result = db
            .query::<SignedGroups>()
            .order_groups(order)
            .group_sum::<CategoryOnly, SignedAmount>()
            .unwrap();
        assert_eq!(
            result
                .value
                .into_iter()
                .map(|g| g.key.category)
                .collect::<Vec<_>>(),
            [0, 1]
        );
    }
}

#[ic_db::record(fields(field(name = "id", value(item(prim = "U256")))))]
struct HugeId {}
#[test]
fn sum_finalization_obeys_group_state_budget_before_selection() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[GroupText::SCHEMA]).unwrap(),
        [39; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for id in 0..19 {
        batch
            .insert(&GroupText {
                id,
                label: format!("{id:02}{}", "x".repeat(2742)),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    assert_eq!(
        db.query::<GroupText>()
            .group_count::<TextKey>()
            .unwrap()
            .value
            .len(),
        19
    );
    assert!(matches!(
        db.query::<GroupText>()
            .limit(1)
            .filter_groups(C::Gt, ic_db::U256::MAX)
            .group_sum::<TextKey, HugeId>(),
        Err(Error::Limit)
    ));
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat8"))),
        field(name = "price", value(opt, item(prim = "Decimal", scale = 2)))
    )
)]
struct DecimalSale {}
#[ic_db::record(fields(field(name = "price", value(opt, item(prim = "Decimal", scale = 2)))))]
struct OptionalPrice {}
#[ic_db::record(fields(field(name = "price", value(opt, item(prim = "Decimal", scale = 3)))))]
struct RescaledPrice {}
#[ic_db::record(fields(field(name = "price", value(opt, item(prim = "Int128")))))]
struct IntegerPrice {}

fn decimal_sales(coefficients: &[(u8, Option<i128>)]) -> Database<VectorMemory> {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[DecimalSale::SCHEMA]).unwrap(),
        [47; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (id, &(category, coefficient)) in coefficients.iter().enumerate() {
        batch
            .insert(&DecimalSale {
                id: id as u64,
                category,
                price: coefficient.map(|n| ic_db::Decimal::try_new(n, 2).unwrap()),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    db
}

#[test]
fn decimal_sums_are_exact_and_group_selection_uses_completed_decimal_values() {
    // Literal cents are an independent oracle; equivalent canonical scales mix.
    let db = decimal_sales(&[
        (0, Some(100)),
        (1, None),
        (0, Some(125)),
        (2, Some(-20)),
        (2, Some(20)),
        (3, Some(-99)),
    ]);
    let sum = db.query::<DecimalSale>().sum::<OptionalPrice>().unwrap();
    assert_eq!(sum.value.unwrap().price.unwrap().to_string(), "1.26");
    assert_eq!(sum.output_bytes, 18);
    let groups = db
        .query::<DecimalSale>()
        .order_groups(ic_db::GroupOrder::AggregateDescending)
        .group_sum::<CategoryOnly, OptionalPrice>()
        .unwrap();
    assert_eq!(
        groups
            .value
            .iter()
            .map(|g| (
                g.key.category,
                g.value
                    .as_ref()
                    .and_then(|v| v.price)
                    .map(|v| v.to_string())
            ))
            .collect::<Vec<_>>(),
        [
            (0, Some("2.25".into())),
            (2, Some("0".into())),
            (3, Some("-0.99".into())),
            (1, None)
        ]
    );
    let filtered = db
        .query::<DecimalSale>()
        .filter_groups(C::Ge, "0.00".parse::<ic_db::Decimal>().unwrap())
        .order_groups(ic_db::GroupOrder::AggregateAscending)
        .limit(1)
        .group_sum::<CategoryOnly, OptionalPrice>()
        .unwrap();
    assert_eq!(filtered.value[0].key.category, 2);
    assert_eq!(
        filtered.value[0].value.as_ref().unwrap().price,
        Some(ic_db::Decimal::ZERO)
    );
    assert!(
        db.query::<DecimalSale>()
            .filter("category", C::Eq, 1_u8)
            .sum::<OptionalPrice>()
            .unwrap()
            .value
            .is_none()
    );
    assert!(
        db.query::<DecimalSale>()
            .filter("id", C::Eq, 99_u64)
            .sum::<OptionalPrice>()
            .unwrap()
            .value
            .is_none()
    );
    assert!(matches!(
        db.query::<DecimalSale>()
            .byte_limit(17)
            .sum::<OptionalPrice>(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<DecimalSale>()
            .work_limit(2)
            .sum::<OptionalPrice>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<DecimalSale>()
            .filter_groups(C::Ge, 0_i128)
            .group_sum::<CategoryOnly, OptionalPrice>(),
        Err(Error::InvalidOperand)
    ));
}

#[test]
fn decimal_sums_check_final_fixed_scale_range_even_for_excluded_groups() {
    for (bound, step) in [(i128::MAX, 1_i128), (i128::MIN, -1)] {
        let db = decimal_sales(&[(0, Some(bound)), (0, Some(step)), (0, Some(-step))]);
        assert_eq!(
            db.query::<DecimalSale>()
                .sum::<OptionalPrice>()
                .unwrap()
                .value
                .unwrap()
                .price
                .unwrap()
                .coefficient_at_scale(2),
            Some(bound)
        );
        let db = decimal_sales(&[(0, Some(bound)), (0, Some(step)), (1, Some(0))]);
        assert!(matches!(
            db.query::<DecimalSale>().sum::<OptionalPrice>(),
            Err(Error::ArithmeticOverflow)
        ));
        assert!(matches!(
            db.query::<DecimalSale>()
                .filter_groups(C::Eq, ic_db::Decimal::ZERO)
                .limit(1)
                .group_sum::<CategoryOnly, OptionalPrice>(),
            Err(Error::ArithmeticOverflow)
        ));
    }
}

#[test]
fn decimal_sum_admission_refuses_rescaling_conversion_and_optionality_changes() {
    let db = decimal_sales(&[]);
    assert!(matches!(
        db.query::<DecimalSale>().sum::<RescaledPrice>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<DecimalSale>().sum::<IntegerPrice>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<DecimalSale>().sum::<PriceKey>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<DecimalSale>()
            .group_sum::<CategoryOnly, RescaledPrice>(),
        Err(Error::InvalidQuery)
    ));
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "price", value(item(prim = "Decimal", scale = 28)))
    )
)]
struct FinePriceRow {}
#[ic_db::record(fields(field(name = "price", value(item(prim = "Decimal", scale = 28)))))]
struct FinePrice {}

#[test]
fn decimal_sums_preserve_all_twenty_eight_fractional_places() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[FinePriceRow::SCHEMA]).unwrap(),
        [48; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (id, price) in ["1", "0.0000000000000000000000000001"]
        .into_iter()
        .enumerate()
    {
        batch
            .insert(&FinePriceRow {
                id: id as u64,
                price: price.parse().unwrap(),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let total = db.query::<FinePriceRow>().sum::<FinePrice>().unwrap();
    assert_eq!(
        total.value.unwrap().price.to_string(),
        "1.0000000000000000000000000001"
    );
    assert_eq!(total.output_bytes, 17);
}

#[ic_db::record(fields(field(name = "price", value(opt, item(prim = "Decimal", scale = 0)))))]
struct WholePrice {}
#[ic_db::record(fields(field(name = "price", value(opt, item(prim = "Decimal", scale = 28)))))]
struct PrecisePrice {}
#[ic_db::record(fields(field(name = "number", value(item(prim = "Decimal", scale = 0)))))]
struct WholeNumberAverage {}

#[test]
fn averages_round_once_to_declared_scale_with_signed_even_ties() {
    for (inputs, expected) in [
        (vec![124, 125], "1.24"),
        (vec![125, 126], "1.26"),
        (vec![-124, -125], "-1.24"),
        (vec![-125, -126], "-1.26"),
        (vec![1, 1, 0], "0.01"),
        (vec![-1, -1, 0], "-0.01"),
        (vec![1, 0, 0], "0"),
    ] {
        let db = decimal_sales(&inputs.into_iter().map(|n| (0, Some(n))).collect::<Vec<_>>());
        let average = db.query::<DecimalSale>().avg::<OptionalPrice>().unwrap();
        assert_eq!(average.value.unwrap().price.unwrap().to_string(), expected);
        assert_eq!(average.output_bytes, 18);
    }
    for (inputs, expected) in [
        ([100, 200], "2"),
        ([200, 300], "2"),
        ([-100, -200], "-2"),
        ([-200, -300], "-2"),
    ] {
        let db = decimal_sales(&inputs.map(|n| (0, Some(n))));
        assert_eq!(
            db.query::<DecimalSale>()
                .avg::<WholePrice>()
                .unwrap()
                .value
                .unwrap()
                .price
                .unwrap()
                .to_string(),
            expected
        );
    }
    let db = decimal_sales(&[(0, Some(124)), (0, Some(125)), (0, None)]);
    assert_eq!(
        db.query::<DecimalSale>()
            .avg::<RescaledPrice>()
            .unwrap()
            .value
            .unwrap()
            .price
            .unwrap()
            .to_string(),
        "1.245"
    );
    let db = decimal_sales(&[(0, Some(100)), (0, Some(0)), (0, Some(0))]);
    assert_eq!(
        db.query::<DecimalSale>()
            .avg::<PrecisePrice>()
            .unwrap()
            .value
            .unwrap()
            .price
            .unwrap()
            .to_string(),
        "0.3333333333333333333333333333"
    );
}

#[test]
fn grouped_averages_filter_rounded_results_and_retain_absence() {
    let db = decimal_sales(&[
        (0, Some(124)),
        (0, Some(125)),
        (1, Some(124)),
        (2, None),
        (3, Some(0)),
    ]);
    let groups = db
        .query::<DecimalSale>()
        .order_groups(ic_db::GroupOrder::AggregateDescending)
        .group_avg::<CategoryOnly, OptionalPrice>()
        .unwrap();
    assert_eq!(
        groups
            .value
            .iter()
            .map(|g| (
                g.key.category,
                g.value
                    .as_ref()
                    .and_then(|v| v.price)
                    .map(|v| v.to_string())
            ))
            .collect::<Vec<_>>(),
        [
            (0, Some("1.24".into())),
            (1, Some("1.24".into())),
            (3, Some("0".into())),
            (2, None)
        ]
    );
    let threshold: ic_db::Decimal = "1.24".parse().unwrap();
    assert!(
        db.query::<DecimalSale>()
            .filter_groups(C::Gt, threshold)
            .group_avg::<CategoryOnly, OptionalPrice>()
            .unwrap()
            .value
            .is_empty()
    );
    let tied = db
        .query::<DecimalSale>()
        .filter_groups(C::Eq, threshold)
        .order_groups(ic_db::GroupOrder::AggregateDescending)
        .limit(1)
        .group_avg::<CategoryOnly, OptionalPrice>()
        .unwrap();
    assert_eq!(tied.value[0].key.category, 0);
    assert!(
        db.query::<DecimalSale>()
            .filter("category", C::Eq, 2_u8)
            .avg::<OptionalPrice>()
            .unwrap()
            .value
            .is_none()
    );
    assert!(
        decimal_sales(&[])
            .query::<DecimalSale>()
            .avg::<OptionalPrice>()
            .unwrap()
            .value
            .is_none()
    );
    assert!(matches!(
        db.query::<DecimalSale>()
            .work_limit(2)
            .avg::<OptionalPrice>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<DecimalSale>()
            .byte_limit(17)
            .avg::<OptionalPrice>(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<DecimalSale>()
            .filter_groups(C::Ge, 0_u64)
            .group_avg::<CategoryOnly, OptionalPrice>(),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.query::<DecimalSale>()
            .filter_groups(C::Eq, threshold)
            .avg::<OptionalPrice>(),
        Err(Error::InvalidQuery)
    ));
}

#[test]
fn average_overflow_is_checked_after_division_without_reducing_result_scale() {
    // The unrepresentable sum still has a representable average.
    for bound in [i128::MIN, i128::MAX] {
        let db = decimal_sales(&[(0, Some(bound)), (0, Some(bound))]);
        assert_eq!(
            db.query::<DecimalSale>()
                .avg::<OptionalPrice>()
                .unwrap()
                .value
                .unwrap()
                .price
                .unwrap()
                .coefficient_at_scale(2),
            Some(bound)
        );
    }
    // Even a filtered-out group must fit the selected 28-place result coefficient.
    let db = decimal_sales(&[(0, Some(i128::MAX)), (1, Some(0))]);
    assert!(matches!(
        db.query::<DecimalSale>().avg::<PrecisePrice>(),
        Err(Error::ArithmeticOverflow)
    ));
    assert!(matches!(
        db.query::<DecimalSale>()
            .filter_groups(C::Eq, ic_db::Decimal::ZERO)
            .limit(1)
            .group_avg::<CategoryOnly, PrecisePrice>(),
        Err(Error::ArithmeticOverflow)
    ));
    assert!(matches!(
        db.query::<DecimalSale>().avg::<IntegerPrice>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<DecimalSale>().avg::<PriceKey>(),
        Err(Error::InvalidQuery)
    ));
}

#[test]
fn every_integer_width_has_an_explicit_decimal_average() {
    macro_rules! check {
        ($prim:literal, $left:expr, $right:expr, $expected:literal) => {{
            #[entity(pk(field="id"),fields(field(name="id",value(item(prim="Nat64"))),field(name="value",value(opt,item(prim=$prim)))))]
            struct Row {}
            #[ic_db::record(fields(field(
                name = "value",
                value(opt, item(prim = "Decimal", scale = 2))
            )))]
            struct Average {}
            let runtime = host(VectorMemory::default());
            let mut db = Database::create(
                runtime
                    .open_memory(&StableKey::parse("test.db.v1").unwrap())
                    .unwrap(),
                Schema::new(&[Row::SCHEMA]).unwrap(),
                [49; 16],
            )
            .unwrap();
            let mut batch = Batch::new();
            for (id, value) in [Some($left), Some($right), None].into_iter().enumerate() {
                batch
                    .insert(&Row {
                        id: id as u64,
                        value,
                    })
                    .unwrap();
            }
            db.commit(batch).unwrap();
            assert_eq!(
                db.query::<Row>()
                    .avg::<Average>()
                    .unwrap()
                    .value
                    .unwrap()
                    .value
                    .unwrap()
                    .to_string(),
                $expected
            );
        }};
    }
    check!("Nat8", 1_u8, 2_u8, "1.5");
    check!("Nat16", 1_u16, 2_u16, "1.5");
    check!("Nat32", 1_u32, 2_u32, "1.5");
    check!("Nat64", 1_u64, 2_u64, "1.5");
    check!("Nat128", 1_u128, 2_u128, "1.5");
    check!("U256", ic_db::U256::ONE, ic_db::U256::from(2_u8), "1.5");
    check!("Int8", -1_i8, -2_i8, "-1.5");
    check!("Int16", -1_i16, -2_i16, "-1.5");
    check!("Int32", -1_i32, -2_i32, "-1.5");
    check!("Int64", -1_i64, -2_i64, "-1.5");
    check!("Int128", -1_i128, -2_i128, "-1.5");
}

#[test]
fn u256_average_divides_before_checked_decimal_conversion() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[HugeSum::SCHEMA]).unwrap(),
        [50; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (id, number) in [
        ic_db::U256::from(i128::MAX as u128) + 1,
        ic_db::U256::ZERO,
        ic_db::U256::ZERO,
    ]
    .into_iter()
    .enumerate()
    {
        batch
            .insert(&HugeSum {
                id: id as u64,
                number,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    assert_eq!(
        db.query::<HugeSum>()
            .avg::<WholeNumberAverage>()
            .unwrap()
            .value
            .unwrap()
            .number
            .to_string(),
        "56713727820156410577229101238628035243"
    );
    let mut batch = Batch::new();
    batch
        .replace(&HugeSum {
            id: 0,
            number: ic_db::U256::MAX,
        })
        .unwrap();
    batch
        .replace(&HugeSum {
            id: 1,
            number: ic_db::U256::MAX,
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<HugeSum>().avg::<WholeNumberAverage>(),
        Err(Error::ArithmeticOverflow)
    ));
}

#[test]
fn grouped_distinct_counts_include_absent_tuples_and_filter_completed_counts() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[GroupedOptional::SCHEMA]).unwrap(),
        [51; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for (id, (category, maybe)) in [
        (None, None),
        (Some(1), None),
        (None, Some(0)),
        (Some(2), Some(9)),
        (None, None),
        (Some(1), None),
        (Some(2), Some(9)),
    ]
    .into_iter()
    .enumerate()
    {
        batch
            .insert(&GroupedOptional {
                id: id as u64,
                category,
                maybe,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let groups = db
        .query::<GroupedOptional>()
        .group_count_distinct::<OptionalCategory, OptionalOnly>()
        .unwrap();
    assert_eq!(
        groups
            .value
            .iter()
            .map(|g| (g.key.category, g.count))
            .collect::<Vec<_>>(),
        [(None, 2), (Some(1), 1), (Some(2), 1)]
    );
    let overlap = db
        .query::<GroupedOptional>()
        .group_count_distinct::<OptionalCategory, OptionalCategory>()
        .unwrap();
    assert!(overlap.value.iter().all(|g| g.count == 1));
    let tuples = db
        .query::<GroupedOptional>()
        .group_count_distinct::<OptionalCategory, OptionalGroupTuple>()
        .unwrap();
    assert_eq!(
        tuples.value.iter().map(|g| g.count).collect::<Vec<_>>(),
        [2, 1, 1]
    );
    assert_eq!(
        db.query::<GroupedOptional>()
            .count_distinct::<OptionalGroupTuple>()
            .unwrap()
            .value,
        4
    );
    let selected = db
        .query::<GroupedOptional>()
        .filter_groups(C::Ge, 2_u64)
        .order_groups(ic_db::GroupOrder::AggregateDescending)
        .limit(1)
        .group_count_distinct::<OptionalCategory, OptionalOnly>()
        .unwrap();
    assert_eq!(selected.value.len(), 1);
    assert_eq!(
        (selected.value[0].key.category, selected.value[0].count),
        (None, 2)
    );
    assert_eq!(selected.output_bytes, 9);
    assert!(
        db.query::<GroupedOptional>()
            .filter("id", C::Eq, 99_u64)
            .group_count_distinct::<OptionalCategory, OptionalOnly>()
            .unwrap()
            .value
            .is_empty()
    );
    assert!(matches!(
        db.query::<GroupedOptional>()
            .work_limit(2)
            .group_count_distinct::<OptionalCategory, OptionalOnly>(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.query::<GroupedOptional>()
            .byte_limit(8)
            .group_count_distinct::<OptionalCategory, OptionalOnly>(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<GroupedOptional>()
            .filter_groups(C::Ge, 1_u128)
            .group_count_distinct::<OptionalCategory, OptionalOnly>(),
        Err(Error::InvalidOperand)
    ));
}

#[test]
fn distinct_count_caps_apply_per_group_before_output_selection() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Entry::SCHEMA]).unwrap(),
        [52; 16],
    )
    .unwrap();
    for id in 0..64 {
        let mut batch = Batch::new();
        batch
            .insert(&Entry {
                id,
                category: (id / 32) as u8,
                amount: id,
                maybe: None,
                payload: vec![],
            })
            .unwrap();
        db.commit(batch).unwrap();
    }
    // Repeating a value is legal even after a group reaches its distinct cap.
    let mut batch = Batch::new();
    batch
        .insert(&Entry {
            id: 64,
            category: 0,
            amount: 0,
            maybe: None,
            payload: vec![],
        })
        .unwrap();
    db.commit(batch).unwrap();
    let groups = db
        .query::<Entry>()
        .group_count_distinct::<CategoryOnly, AmountOnly>()
        .unwrap();
    assert_eq!(
        groups
            .value
            .iter()
            .map(|g| (g.key.category, g.count))
            .collect::<Vec<_>>(),
        [(0, 32), (1, 32)]
    );
    assert!(matches!(
        db.query::<Entry>().count_distinct::<AmountOnly>(),
        Err(Error::Limit)
    ));
    assert!(
        db.query::<Entry>()
            .group_count_distinct::<CategoryOnly, PayloadOnly>()
            .is_ok()
    );
    let mut batch = Batch::new();
    batch
        .insert(&Entry {
            id: 65,
            category: 0,
            amount: 99,
            maybe: None,
            payload: vec![],
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<Entry>()
            .filter_groups(C::Eq, 0_u64)
            .limit(1)
            .group_count_distinct::<CategoryOnly, AmountOnly>(),
        Err(Error::Limit)
    ));
}

#[test]
fn distinct_count_keys_share_the_query_byte_budget_and_key_bound() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[GroupedText::SCHEMA]).unwrap(),
        [53; 16],
    )
    .unwrap();
    for id in 0..48 {
        let mut batch = Batch::new();
        batch
            .insert(&GroupedText {
                id,
                category: (id / 24) as u8,
                label: format!("{id:03}{}", "x".repeat(2997)),
            })
            .unwrap();
        db.commit(batch).unwrap();
    }
    let one = db
        .query::<GroupedText>()
        .filter("category", C::Eq, 0_u8)
        .group_count_distinct::<CategoryOnly, TextKey>()
        .unwrap();
    assert_eq!(one.value[0].count, 24);
    assert_eq!(
        db.query::<GroupedText>()
            .filter("category", C::Eq, 0_u8)
            .count_distinct::<TextKey>()
            .unwrap()
            .value,
        24
    );
    // Neither group exceeds 32 values, but their combined comparison keys exceed 128 KiB.
    assert!(matches!(
        db.query::<GroupedText>()
            .filter_groups(C::Eq, 0_u64)
            .limit(1)
            .group_count_distinct::<CategoryOnly, TextKey>(),
        Err(Error::Limit)
    ));
    let mut batch = Batch::new();
    batch
        .replace(&GroupedText {
            id: 0,
            category: 0,
            label: "\0".repeat(2100),
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<GroupedText>()
            .filter("id", C::Eq, 0_u64)
            .count_distinct::<TextKey>(),
        Err(Error::Limit)
    ));
}

#[test]
fn primary_range_counts_skip_rows_only_for_a_complete_interval() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for (lower, upper, expected) in [
        (C::Ge, C::Le, 4),
        (C::Gt, C::Le, 3),
        (C::Ge, C::Lt, 3),
        (C::Gt, C::Lt, 2),
    ] {
        let range = || {
            db.query::<Entry>()
                .filter("id", lower, 3_u64)
                .filter("id", upper, 6_u64)
        };
        let count = range().work_limit(expected + 1).count().unwrap();
        assert_eq!(
            (count.value, count.examined, count.fetched),
            (expected as u64, expected, 0)
        );
        let exact = range().work_limit(expected).count();
        if upper == C::Le {
            assert_eq!(exact.unwrap().value, expected as u64);
        } else {
            assert!(matches!(exact, Err(Error::Limit)));
        }
        assert_eq!(range().count_validated().unwrap().fetched, expected);
    }
    let empty = db
        .query::<Entry>()
        .filter("id", C::Ge, 5_u64)
        .filter("id", C::Lt, 5_u64)
        .work_limit(1)
        .count()
        .unwrap();
    assert_eq!((empty.value, empty.examined), (0, 0));
    // The final physical key proves exhaustion even when its row fails the filter.
    let none = db
        .query::<Entry>()
        .filter("id", C::Ge, 6_u64)
        .filter("id", C::Le, 8_u64)
        .filter("amount", C::Gt, 100_u64)
        .work_limit(3)
        .exists()
        .unwrap();
    assert_eq!((none.value, none.examined), (false, 3));
    // An inclusive bound beyond the highest stored key has not been visited.
    assert!(matches!(
        db.query::<Entry>()
            .filter("id", C::Ge, 6_u64)
            .filter("id", C::Le, 9_u64)
            .work_limit(3)
            .count(),
        Err(Error::Limit)
    ));
    let point = db
        .query::<Entry>()
        .filter("id", C::Eq, 1_u64)
        .count()
        .unwrap();
    assert_eq!((point.value, point.fetched), (1, 0));
    let residual = db
        .query::<Entry>()
        .filter("id", C::Gt, 4_u64)
        .filter("amount", C::Lt, 70_u64)
        .count()
        .unwrap();
    assert_eq!((residual.value, residual.fetched), (2, 4));
    let alternatives = db
        .query::<Entry>()
        .filter_any(&[("id", C::Eq, 1_u64.into()), ("id", C::Eq, 8_u64.into())])
        .count()
        .unwrap();
    assert_eq!((alternatives.value, alternatives.fetched), (2, 8));
}

#[test]
fn index_entry_counts_are_bounded_and_independent_of_owner_rows() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    let query = || {
        db.query::<Entry>()
            .using_index("category,amount,id", &[0_u8.into()])
    };
    let count = query()
        .work_limit(5)
        .limit(1)
        .count_index_entries()
        .unwrap();
    assert_eq!(
        (
            count.value,
            count.examined,
            count.fetched,
            count.output_bytes
        ),
        (4, 4, 0, 8)
    );
    assert!(matches!(
        query().work_limit(4).count_index_entries(),
        Err(Error::Limit)
    ));
    assert!(matches!(
        query().byte_limit(7).count_index_entries(),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        query().filter("id", C::Eq, 7_u64).count_index_entries(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Entry>().count_index_entries(),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(
        db.query::<Entry>()
            .using_index("category", &[3_u8.into()])
            .count_index_entries()
            .unwrap()
            .value,
        0
    );
    let conflicting = db
        .query::<Entry>()
        .filter("category", C::Eq, 0_u8)
        .filter("category", C::Eq, 1_u8)
        .count()
        .unwrap();
    assert_eq!(conflicting.value, 0);
    drop(db);
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    let row_key = [&[2][..], &Entry::ID.to_be_bytes(), &2_u64.to_be_bytes()].concat();
    assert!(raw.remove(&row_key).is_some());
    drop(raw);
    let db = Database::open(
        runtime.open_memory(&key).unwrap(),
        Schema::new(&[Entry::SCHEMA, Other::SCHEMA]).unwrap(),
    )
    .unwrap();
    let query = || db.query::<Entry>().using_index("category", &[0_u8.into()]);
    assert_eq!(query().count_index_entries().unwrap().value, 4);
    assert_eq!(query().count().unwrap().value, 4);
    assert!(matches!(query().count_validated(), Err(Error::Corrupt)));
    let implicit = db
        .query::<Entry>()
        .filter("category", C::Eq, 0_u8)
        .count()
        .unwrap();
    assert_eq!((implicit.value, implicit.fetched), (4, 0));
    assert!(matches!(
        db.query::<Entry>()
            .filter("category", C::Eq, 0_u8)
            .filter("category", C::Eq, 1_u8)
            .count(),
        Err(Error::Corrupt)
    ));
}
