//! Raw-byte ordering shares one contract across index, filter and aggregate paths.

use super::{C, host};
use crate::raw_map::RawMap;
use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error, GroupOrder, IndexValue};
use ic_storage::{StableKey, VectorMemory};

///
/// Binary
/// Exercises indexed access by `payload`, `id`, `optional` and `unique` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["payload", "id"]),
    index(fields = ["optional", "id"]),
    index(field = "unique", unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 1024))),
        field(name = "optional", value(opt, item(prim = "Blob", max_bytes = 1024))),
        field(name = "unique", value(opt, item(prim = "Blob", max_bytes = 1024)))
    )
)]
struct Binary {}

///
/// Payload
/// Carries the `Blob` value of `payload`.
///

#[record(fields(field(name = "payload", value(item(prim = "Blob", max_bytes = 1024)))))]
struct Payload {}

///
/// Optional
/// Carries an optional `Blob` value of `optional`.
///

#[record(fields(field(name = "optional", value(opt, item(prim = "Blob", max_bytes = 1024)))))]
struct Optional {}

#[test]
fn field_patterns_use_each_rows_present_raw_needle() {
    let runtime = host(VectorMemory::default());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Binary::SCHEMA]).unwrap(),
        [48; 16],
    )
    .unwrap();
    let pairs: &[(&[u8], Option<&[u8]>)] = &[
        (&[0], Some(&[255])),
        (&[0, 255], Some(&[255])),
        (&[255, 0], Some(&[255])),
        (&[0, 255, 0], Some(&[255])),
        (&[1], Some(&[])),
        (&[], Some(&[])),
        (&[], None),
    ];
    let mut batch = Batch::new();
    for (id, &(payload, needle)) in pairs.iter().enumerate() {
        batch
            .insert(&Binary {
                id: id as u64,
                payload: payload.to_vec(),
                optional: needle.map(<[u8]>::to_vec),
                unique: needle.map(|_| vec![id as u8]),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    for (field, op, threshold, expected) in [
        (Binary::PAYLOAD, C::Eq, 0, (2, 2)),
        (Binary::OPTIONAL, C::Lt, 1, (2, 2)),
        (Binary::PAYLOAD, C::Le, 1024, (7, 0)),
        (Binary::UNIQUE, C::Le, 1024, (6, 6)),
        (Binary::OPTIONAL, C::Le, 1024, (6, 6)),
        (Binary::UNIQUE, C::Ge, 0, (6, 6)),
        (Binary::UNIQUE, C::Gt, 1024, (0, 0)),
        (Binary::UNIQUE, C::Ne, 1025, (7, 0)),
    ] {
        let count = db
            .query::<Binary>()
            .filter_length(field, op, threshold)
            .count()
            .unwrap();
        assert_eq!((count.value, count.examined), expected);
        assert_eq!(count.fetched, 0);
    }
    for (length, expected) in [(1, 0), (2, 1)] {
        let count = db
            .query::<Binary>()
            .filter(Binary::PAYLOAD, C::Contains, &[0, 255])
            .filter_length(Binary::PAYLOAD, C::Eq, length)
            .count()
            .unwrap();
        assert_eq!(count.value, expected);
        if length == 1 {
            assert_eq!(count.examined, 0);
        }
    }
    for needle in [&[255][..], &[][..]] {
        let prefix = [IndexValue::Blob(needle)];
        for query in [
            db.query::<Binary>().eq(Binary::OPTIONAL, needle),
            db.query::<Binary>().using_index("optional,id", &prefix),
        ] {
            let count = query
                .filter_fields(Binary::PAYLOAD, C::Contains, Binary::OPTIONAL)
                .filter_length(Binary::PAYLOAD, C::Eq, 0)
                .count()
                .unwrap();
            if needle.is_empty() {
                assert_eq!(count.value, 1);
            } else {
                assert_eq!((count.value, count.examined), (0, 0));
            }
        }
    }
    // A contradicted suffix on fixed operands can be rejected before reading a row.
    for (actual, needle) in [
        (b"\0".as_slice(), b"\xff".as_slice()),
        (b"\0\xff", b"\xff"),
        (b"", b""),
    ] {
        let query = || {
            db.query::<Binary>()
                .eq(Binary::PAYLOAD, actual)
                .eq(Binary::OPTIONAL, needle)
                .filter_fields(Binary::PAYLOAD, C::EndsWith, Binary::OPTIONAL)
        };
        let expected = pairs
            .iter()
            .filter(|(p, n)| *p == actual && *n == Some(needle) && p.ends_with(needle))
            .count() as u64;
        let count = query().count().unwrap();
        assert_eq!(count.value, expected);
        if !actual.ends_with(needle) {
            assert_eq!((count.examined, count.fetched), (0, 0));
        }
        assert_eq!(query().count_validated().unwrap().value, expected);
    }
    for (op, expected) in [
        (C::StartsWith, vec![2, 4, 5]),
        (C::EndsWith, vec![1, 4, 5]),
        (C::Contains, vec![1, 2, 3, 4, 5]),
    ] {
        for field in [Binary::PAYLOAD, Binary::UNIQUE, Binary::OPTIONAL] {
            for query in [
                db.query::<Binary>().filter(field, op, b"".as_slice()),
                db.query::<Binary>().filter_fields(field, op, field),
            ] {
                let count = query.count().unwrap();
                let expected = if field == Binary::PAYLOAD {
                    (7, 0)
                } else {
                    (6, 6)
                };
                assert_eq!((count.value, count.examined), expected);
                assert_eq!(count.fetched, 0);
            }
        }
        let rows = db
            .query::<Binary>()
            .filter_fields("payload", op, "optional")
            .fetch()
            .unwrap()
            .rows;
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), expected);
        // The right-hand payload is required. An absent left-hand field still cannot match.
        assert_eq!(
            db.query::<Binary>()
                .filter_fields("optional", op, "payload")
                .count()
                .unwrap()
                .value,
            1
        );
    }
}

#[test]
fn bytes_agree_across_indexes_pages_filters_and_aggregates() {
    let runtime = host(VectorMemory::default());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[Binary::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [48; 16]).unwrap();
    // Intentionally unsorted, with duplicate, prefix, escape and non-UTF-8 bytes.
    let values = [
        vec![255],
        vec![0, 255],
        vec![],
        vec![128],
        vec![0],
        vec![0, 1],
        vec![0],
        vec![1, 0],
    ];
    let mut batch = Batch::new();
    let rows: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(n, payload)| Binary {
            id: n as u64,
            payload: payload.clone(),
            optional: if n % 3 == 0 {
                None
            } else {
                Some(payload.clone())
            },
            unique: None,
        })
        .collect();
    for row in &rows {
        batch.insert(row).unwrap();
    }
    db.commit(batch).unwrap();
    drop(db);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    // The embedded zero expands in an index key. Its raw Blob length is still two.
    let prefix = [IndexValue::Blob(&[0, 255])];
    let by_length = |n| {
        db.query::<Binary>()
            .using_index("payload,id", &prefix)
            .filter_length(Binary::PAYLOAD, C::Eq, n)
    };
    let count = by_length(2).count().unwrap();
    assert_eq!((count.value, count.examined, count.fetched), (1, 1, 0));
    assert_eq!(by_length(2).count_validated().unwrap().value, count.value);
    let impossible = by_length(3).count().unwrap();
    assert_eq!((impossible.value, impossible.examined), (0, 0));

    // The two zero-only payloads have no literal FF, although both index keys do.
    let zero = [IndexValue::Blob(&[0])];
    for op in [C::StartsWith, C::EndsWith, C::Contains] {
        for (needle, matches) in [(&[][..], 2), (&[0][..], 2), (&[255][..], 0)] {
            let query = || {
                db.query::<Binary>()
                    .using_index("payload,id", &zero)
                    .filter(Binary::PAYLOAD, op, needle)
            };
            let count = query().count().unwrap();
            assert_eq!(
                (count.value, count.examined, count.fetched),
                (matches, matches as usize, 0)
            );
            assert_eq!(query().count_validated().unwrap().value, matches);
        }
    }

    let mut expected = rows.clone();
    expected.sort_by(|a, b| (&a.payload, a.id).cmp(&(&b.payload, b.id)));
    let mut actual = Vec::new();
    let mut after = None;
    loop {
        let q = db
            .query::<Binary>()
            .order_by(&["payload", "id"])
            .limit(2)
            .work_limit(3)
            .after(after.as_ref());
        let page = q.fetch().unwrap();
        actual.extend(page.rows);
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    assert_eq!(actual, expected);
    for operand in &values {
        for comparison in [C::Eq, C::Ne, C::Lt, C::Le, C::Gt, C::Ge] {
            let accepts = |row: &&Binary| match comparison {
                C::Eq => row.payload == *operand,
                C::Ne => row.payload != *operand,
                C::Lt => row.payload < *operand,
                C::Le => row.payload <= *operand,
                C::Gt => row.payload > *operand,
                C::Ge => row.payload >= *operand,
                _ => unreachable!(),
            };
            for indexed in [false, true] {
                let mut query = db.query::<Binary>().filter("payload", comparison, operand);
                let source = if indexed {
                    query = query.using_index("payload,id", &[]);

                    &expected
                } else {
                    &rows
                };
                assert_eq!(
                    query.fetch().unwrap().rows,
                    source.iter().filter(accepts).cloned().collect::<Vec<_>>()
                );
            }
        }
        assert_eq!(
            db.index_prefix::<Binary>("payload,id", &[operand.as_slice().into()], 32)
                .unwrap(),
            expected
                .iter()
                .filter(|r| r.payload == *operand)
                .cloned()
                .collect::<Vec<_>>()
        );
    }
    let distinct: std::collections::BTreeSet<_> = values.into_iter().collect();
    for indexed in [false, true] {
        let mut q = db.query::<Binary>().distinct();
        if indexed {
            q = q.using_index("payload,id", &[]);
        }
        let actual: std::collections::BTreeSet<_> = q
            .fetch_as::<Payload>()
            .unwrap()
            .rows
            .into_iter()
            .map(|r| r.payload)
            .collect();
        assert_eq!(actual, distinct);
    }
    assert!(
        db.query::<Binary>()
            .min::<Payload>()
            .unwrap()
            .value
            .unwrap()
            .payload
            .is_empty()
    );
    assert_eq!(
        db.query::<Binary>()
            .max::<Payload>()
            .unwrap()
            .value
            .unwrap()
            .payload,
        vec![255]
    );
    assert_eq!(
        db.query::<Binary>()
            .count_distinct::<Payload>()
            .unwrap()
            .value,
        distinct.len() as u64
    );
    let groups = db.query::<Binary>().group_count::<Payload>().unwrap().value;
    assert_eq!(
        groups
            .iter()
            .map(|g| (&g.key.payload, g.count))
            .collect::<Vec<_>>(),
        distinct
            .iter()
            .map(|v| (v, rows.iter().filter(|r| r.payload == *v).count() as u64))
            .collect::<Vec<_>>()
    );
    let mut optional_order = rows.clone();
    let mut optional_groups = std::collections::BTreeMap::new();
    for row in &rows {
        *optional_groups.entry(row.optional.clone()).or_insert(0_u64) += 1;
    }
    assert_eq!(
        db.query::<Binary>()
            .count_distinct::<Optional>()
            .unwrap()
            .value,
        optional_groups.len() as u64
    );
    for present_only in [false, true] {
        let query = db.query::<Binary>();
        let groups = if present_only {
            query.group_count_field::<Optional>("optional")
        } else {
            query.group_count::<Optional>()
        }
        .unwrap()
        .value;
        assert_eq!(
            groups
                .into_iter()
                .map(|g| (g.key.optional, g.count))
                .collect::<Vec<_>>(),
            optional_groups
                .iter()
                .map(|(key, count)| (
                    key.clone(),
                    if present_only && key.is_none() {
                        0
                    } else {
                        *count
                    }
                ))
                .collect::<Vec<_>>()
        );
    }
    optional_order.sort_by(|a, b| (&a.optional, a.id).cmp(&(&b.optional, b.id)));
    assert_eq!(
        db.query::<Binary>()
            .order_by(&["optional", "id"])
            .fetch()
            .unwrap()
            .rows,
        optional_order
    );
    let groups = db
        .query::<Binary>()
        .order_groups(GroupOrder::AggregateDescending)
        .filter_groups(C::Ge, &[0_u8])
        .group_max::<Payload, Optional>()
        .unwrap()
        .value;
    let mut oracle = std::collections::BTreeMap::<Vec<u8>, Option<Vec<u8>>>::new();
    for row in &rows {
        let value = oracle.entry(row.payload.clone()).or_default();
        *value = value.clone().max(row.optional.clone());
    }
    let mut oracle: Vec<_> = oracle
        .into_iter()
        .filter(|(_, v)| v.as_deref().is_some_and(|v| v >= [0_u8].as_slice()))
        .collect();
    oracle.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    assert_eq!(
        groups
            .into_iter()
            .map(|g| (g.key.payload, g.value.and_then(|v| v.optional)))
            .collect::<Vec<_>>(),
        oracle
    );
    assert!(matches!(
        db.query::<Binary>().filter("payload", C::Eq, "").fetch(),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.query::<Binary>()
            .filter("payload", C::Eq, IndexValue::Absent)
            .fetch(),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.query::<Binary>()
            .filter("payload", C::Eq, vec![1; 1025].as_slice())
            .fetch(),
        Err(Error::Limit)
    ));
}

#[test]
fn blob_suffixes_and_substrings_match_raw_bytes_without_matching_inside_zero_escapes() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Binary::SCHEMA]).unwrap(),
        [53; 16],
    )
    .unwrap();
    let values = [
        vec![],
        vec![0],
        vec![255],
        vec![0, 255],
        vec![255, 0],
        vec![0, 0],
        vec![255, 255],
        vec![1, 255],
        vec![1],
        vec![128],
        vec![0, 255, 0],
        vec![0, 255, 255, 128],
    ];
    let rows = values
        .into_iter()
        .enumerate()
        .map(|(id, payload)| Binary {
            id: id as u64,
            optional: if id % 4 == 1 {
                None
            } else {
                Some(payload.clone())
            },
            payload,
            unique: None,
        })
        .collect::<Vec<_>>();
    let mut batch = Batch::new();
    for row in &rows {
        batch.insert(row).unwrap();
    }
    db.commit(batch).unwrap();
    let saved = backing.borrow().clone();
    let patterns = (0..=255)
        .map(|b| vec![b])
        .chain([
            vec![],
            vec![0, 0],
            vec![0, 255],
            vec![255, 0],
            vec![255, 255],
            vec![0; 600],
        ])
        .collect::<Vec<_>>();
    for (op, bound, expected) in [
        (C::Gt, &[0][..], 1),
        (C::Lt, &[0, 255][..], 0),
        (C::Le, &[0, 255][..], 1),
        (C::Ge, &[255][..], 0),
    ] {
        let count = db
            .query::<Binary>()
            .eq(Binary::PAYLOAD, &[0_u8, 255])
            .filter(Binary::PAYLOAD, op, bound)
            .count()
            .unwrap();
        assert_eq!(
            (count.value, count.examined, count.fetched),
            (expected, expected as usize, 0)
        );
    }
    for optional in [false, true] {
        let field = if optional { "optional" } else { "payload" };
        let index = if optional {
            "optional,id"
        } else {
            "payload,id"
        };
        let value = |r: &Binary| {
            if optional {
                r.optional.clone()
            } else {
                Some(r.payload.clone())
            }
        };
        for comparison in [C::EndsWith, C::Contains] {
            for needle in &patterns {
                for indexed in [false, true] {
                    for descending in [false, true] {
                        let mut expected = rows
                            .iter()
                            .filter(|r| {
                                value(r).is_some_and(|v| match comparison {
                                    C::EndsWith => v.ends_with(needle),
                                    _ => {
                                        needle.is_empty()
                                            || v.windows(needle.len()).any(|w| w == needle)
                                    }
                                })
                            })
                            .cloned()
                            .collect::<Vec<_>>();
                        if indexed {
                            expected.sort_by_key(|r| (value(r), r.id));
                        }
                        if descending {
                            expected.reverse();
                        }
                        let mut actual = Vec::new();
                        let mut cursor = None;
                        let mut examined = 0;
                        for page_number in 0..16 {
                            let mut q = db
                                .query::<Binary>()
                                .filter(field, comparison, needle.as_slice())
                                .work_limit(3)
                                .limit(2);
                            if indexed {
                                q = q.using_index(index, &[]);
                            }
                            if descending {
                                q = q.descending();
                            }
                            q = q.after(cursor.as_ref());
                            let page = q.fetch().unwrap();
                            assert!(page.examined <= 3);
                            examined += page.examined;
                            actual.extend(page.rows);
                            cursor = page.next;
                            if cursor.is_none() {
                                break;
                            }
                            assert!(page_number < 15, "pattern pages must progress");
                        }
                        assert_eq!(actual, expected);
                        assert_eq!(
                            examined,
                            if indexed && optional && needle.is_empty() {
                                expected.len()
                            } else {
                                rows.len()
                            }
                        );
                    }
                }
            }
        }
    }
    let groups = db
        .query::<Binary>()
        .filter_groups(C::EndsWith, b"\xff")
        .group_max::<Payload, Optional>()
        .unwrap()
        .value;
    assert_eq!(
        groups
            .into_iter()
            .map(|g| g.key.payload)
            .collect::<Vec<_>>(),
        vec![vec![0, 255], vec![1, 255], vec![255], vec![255, 255]]
    );
    let groups = db
        .query::<Binary>()
        .filter_groups(C::Contains, [255, 255].as_slice())
        .group_max::<Payload, Optional>()
        .unwrap()
        .value;
    assert_eq!(
        groups
            .into_iter()
            .map(|g| g.key.payload)
            .collect::<Vec<_>>(),
        [vec![0, 255, 255, 128], vec![255, 255]]
    );
    assert_eq!(*backing.borrow(), saved);
}

#[test]
fn blob_prefixes_match_raw_bytes_on_scans_indexes_and_sparse_pages() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let mut db = Database::create(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        Schema::new(&[Binary::SCHEMA]).unwrap(),
        [52; 16],
    )
    .unwrap();
    let values = [
        vec![],
        vec![0],
        vec![0, 0],
        vec![0, 255],
        vec![0, 255, 128],
        vec![255],
        vec![255, 0],
        vec![255, 255],
        vec![128],
        vec![1, 0],
        vec![0],
    ];
    let rows = values
        .into_iter()
        .enumerate()
        .map(|(id, payload)| Binary {
            id: id as u64,
            optional: if id % 4 == 1 {
                None
            } else {
                Some(payload.clone())
            },
            payload,
            unique: None,
        })
        .collect::<Vec<_>>();
    let mut batch = Batch::new();
    for row in &rows {
        batch.insert(row).unwrap();
    }
    db.commit(batch).unwrap();
    let saved = backing.borrow().clone();
    let prefixes = (0..=255)
        .map(|b| vec![b])
        .chain([
            vec![],
            vec![0, 0],
            vec![0, 255],
            vec![0, 255, 128],
            vec![255, 255],
            vec![0; 600], // Admitted comparison, too large for an index seek.
        ])
        .collect::<Vec<_>>();
    for optional in [false, true] {
        let field = if optional { "optional" } else { "payload" };
        let index = if optional {
            "optional,id"
        } else {
            "payload,id"
        };
        let value = |r: &Binary| {
            if optional {
                r.optional.clone()
            } else {
                Some(r.payload.clone())
            }
        };
        for prefix in &prefixes {
            for indexed in [false, true] {
                for descending in [false, true] {
                    let mut expected = rows
                        .iter()
                        .filter(|r| value(r).is_some_and(|v| v.starts_with(prefix)))
                        .cloned()
                        .collect::<Vec<_>>();
                    if indexed {
                        expected.sort_by_key(|r| (value(r), r.id));
                    }
                    if descending {
                        expected.reverse();
                    }
                    let query = || {
                        let mut q =
                            db.query::<Binary>()
                                .filter(field, C::StartsWith, prefix.as_slice());
                        if indexed {
                            q = q.using_index(index, &[]);
                        }
                        if descending {
                            q = q.descending();
                        }

                        q
                    };
                    if !descending {
                        assert_eq!(query().count().unwrap().value, expected.len() as u64);
                    }
                    let mut actual = Vec::new();
                    let mut cursor = None;
                    let mut examined = 0;
                    let mut complete = false;
                    for _ in 0..16 {
                        let q = query().limit(2).work_limit(3).after(cursor.as_ref());
                        let page = q.fetch().unwrap();
                        assert!(page.examined <= 3);
                        examined += page.examined;
                        actual.extend(page.rows);
                        cursor = page.next;
                        if cursor.is_none() {
                            complete = true;
                            break;
                        }
                    }
                    assert!(complete);
                    assert_eq!(actual, expected);
                    if indexed && prefix.len() < 600 {
                        assert_eq!(examined, expected.len());
                    }
                }
            }
        }
    }
    // DISTINCT must apply the prefix before collapsing a projected value class.
    for prefix in [&[][..], &[0][..], &[0, 255][..], &[255][..]] {
        let expected = rows
            .iter()
            .filter_map(|r| r.optional.as_ref())
            .filter(|v| v.starts_with(prefix))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        for indexed in [false, true] {
            let query = || {
                let q = db
                    .query::<Binary>()
                    .filter("optional", C::StartsWith, prefix);

                if indexed {
                    q.using_index("optional,id", &[])
                } else {
                    q
                }
            };
            let actual = query()
                .distinct()
                .fetch_as::<Optional>()
                .unwrap()
                .rows
                .into_iter()
                .map(|r| r.optional.unwrap())
                .collect::<Vec<_>>();
            assert_eq!(actual.len(), expected.len());
            assert_eq!(
                actual
                    .into_iter()
                    .collect::<std::collections::BTreeSet<_>>(),
                expected
            );
            assert_eq!(
                query().count_distinct::<Optional>().unwrap().value,
                expected.len() as u64
            );
        }
    }
    // Group selection uses the same admitted prefix semantics on a completed winner.
    let groups = db
        .query::<Binary>()
        .filter_groups(C::StartsWith, [0, 255].as_slice())
        .group_max::<Payload, Optional>()
        .unwrap()
        .value;
    assert_eq!(
        groups
            .iter()
            .map(|g| g.key.payload.clone())
            .collect::<Vec<_>>(),
        vec![vec![0, 255], vec![0, 255, 128]]
    );
    for operand in [
        IndexValue::Absent,
        IndexValue::Text(""),
        IndexValue::Nat64(0),
    ] {
        assert!(matches!(
            db.query::<Binary>()
                .filter("optional", C::StartsWith, operand)
                .fetch(),
            Err(Error::InvalidOperand)
        ));
    }
    assert_eq!(*backing.borrow(), saved);
}

#[test]
fn escaped_blob_index_bounds_and_unique_changes_preserve_backing() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(
        runtime.open_memory(&slot).unwrap(),
        Schema::new(&[Binary::SCHEMA]).unwrap(),
        [49; 16],
    )
    .unwrap();
    let mut row = Binary {
        id: 1,
        payload: vec![],
        optional: None,
        unique: Some(vec![0, 255]),
    };
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    for other in [
        Binary {
            id: 2,
            ..row.clone()
        },
        Binary {
            id: 2,
            payload: vec![0; 247],
            unique: None,
            ..row.clone()
        },
    ] {
        let before = backing.borrow().clone();
        let mut batch = Batch::new();
        batch.insert(&other).unwrap();
        let result = db.commit(batch);
        if other.unique.is_some() {
            assert!(matches!(result, Err(Error::UniqueViolation)));
        } else {
            assert!(matches!(result, Err(Error::Limit)));
        }
        assert_eq!(*backing.borrow(), before);
    }
    // The unique key has nine prefix bytes, a presence tag and a two-byte
    // terminator. Each embedded zero contributes two encoded bytes.
    let boundary = vec![0; 250];
    let mut oversized = boundary.clone();
    oversized.push(1);
    let mut batch = Batch::new();
    batch
        .insert(&Binary {
            id: 2,
            unique: Some(boundary.clone()),
            ..row.clone()
        })
        .unwrap();
    db.commit(batch).unwrap();
    let mut batch = Batch::new();
    batch
        .insert(&Binary {
            id: 3,
            unique: Some(oversized.clone()),
            ..row.clone()
        })
        .unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Limit)));
    let wide = [IndexValue::Blob(&oversized), IndexValue::Blob(&boundary)];
    let small = [IndexValue::Blob(&boundary), IndexValue::Blob(&[128])];
    for (first, second) in [(&wide, &small), (&small, &wide)] {
        let query = || {
            db.query::<Binary>()
                .using_index("unique", &[])
                .filter_in(Binary::UNIQUE, first)
                .filter_in(Binary::UNIQUE, second)
        };
        let count = query().work_limit(1).count().unwrap();
        assert_eq!((count.value, count.examined, count.fetched), (1, 1, 0));
        assert_eq!(query().count_validated().unwrap().value, count.value);
    }
    let impossible = db
        .query::<Binary>()
        .using_index("unique", &[])
        .filter_in(Binary::UNIQUE, &[IndexValue::Blob(&oversized); 2])
        .work_limit(1)
        .count()
        .unwrap();
    assert_eq!(
        (impossible.value, impossible.examined, impossible.fetched),
        (0, 0, 0)
    );
    row.unique = Some(vec![128]);
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    assert!(
        db.index_prefix::<Binary>("unique", &[IndexValue::Blob(&[0, 255])], 1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.index_prefix::<Binary>("unique", &[IndexValue::Blob(&[128])], 1)
            .unwrap(),
        [row.clone()]
    );
    let mut batch = Batch::new();
    batch.delete::<Binary>(1).unwrap();
    db.commit(batch).unwrap();
    assert!(
        db.index_prefix::<Binary>("unique", &[IndexValue::Blob(&[128])], 1)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn malformed_blob_index_components_refuse_without_mutation() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[Binary::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [50; 16]).unwrap();
    let mut batch = Batch::new();
    batch
        .insert(&Binary {
            id: 1,
            payload: vec![255],
            optional: None,
            unique: None,
        })
        .unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let index = Binary::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "payload,id")
        .unwrap();
    let mut prefix = vec![3];
    prefix.extend(Binary::SCHEMA.id.to_be_bytes());
    prefix.extend(index.id.to_be_bytes());
    // Literal bytes, independent of the product encoder: payload, id component, owner.
    let mut valid = prefix.clone();
    valid.extend([255, 0, 0]);
    valid.extend(1_u64.to_be_bytes());
    valid.extend(1_u64.to_be_bytes());
    for component in [vec![255, 0, 1], vec![255], vec![0, 255, 0]] {
        let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
        assert!(raw.remove(&valid).is_some());
        let mut bad = prefix.clone();
        bad.extend(component);
        raw.insert(bad.clone(), 1_u64.to_be_bytes().to_vec());
        drop(raw);
        let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
        let before = backing.borrow().clone();
        assert!(matches!(
            db.query::<Binary>().using_index("payload,id", &[]).fetch(),
            Err(Error::Corrupt)
        ));
        assert_eq!(*backing.borrow(), before);
        drop(db);
        let mut raw = RawMap::load(runtime.open_memory(&slot).unwrap());
        raw.remove(&bad);
        raw.insert(valid.clone(), 1_u64.to_be_bytes().to_vec());
    }
}
