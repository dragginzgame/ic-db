//! Raw-byte ordering shares one contract across index, filter and aggregate paths.
use super::{C, host};
use ic_db::{Batch, Database, Entity, Error, GroupOrder, IndexValue, Schema, entity};
use ic_storage::{StableKey, VectorMemory, adapter::StableBTreeMap};

#[entity(pk(field="id"), index(fields=["payload","id"]),
    index(fields=["optional","id"]), index(field="unique", unique, present_only), fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="payload",value(item(prim="Blob",max_bytes=1024))),
    field(name="optional",value(opt,item(prim="Blob",max_bytes=1024))),
    field(name="unique",value(opt,item(prim="Blob",max_bytes=1024)))
))]
struct Binary {}
#[ic_db::record(fields(field(name = "payload", value(item(prim = "Blob", max_bytes = 1024)))))]
struct Payload {}
#[ic_db::record(fields(field(
    name = "optional",
    value(opt, item(prim = "Blob", max_bytes = 1024))
)))]
struct Optional {}

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
    let mut expected = rows.clone();
    expected.sort_by(|a, b| (&a.payload, a.id).cmp(&(&b.payload, b.id)));
    let mut actual = Vec::new();
    let mut after = None;
    loop {
        let mut q = db
            .query::<Binary>()
            .order_by(&["payload", "id"])
            .limit(2)
            .work_limit(3);
        if let Some(cursor) = &after {
            q = q.after(cursor);
        }
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
                let mut query =
                    db.query::<Binary>()
                        .filter("payload", comparison, operand.as_slice());
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
    assert_eq!(
        db.query::<Binary>()
            .min::<Payload>()
            .unwrap()
            .value
            .unwrap()
            .payload,
        vec![]
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
        .filter_groups(C::Ge, [0_u8].as_slice())
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
        db.query::<Binary>()
            .filter("payload", C::Contains, [0_u8].as_slice())
            .fetch(),
        Err(Error::InvalidQuery)
    ));
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
fn blob_suffixes_match_raw_bytes_without_matching_inside_zero_escapes() {
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
    let suffixes = (0..=255)
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
        for suffix in &suffixes {
            for indexed in [false, true] {
                for descending in [false, true] {
                    let mut expected = rows
                        .iter()
                        .filter(|r| value(r).is_some_and(|v| v.ends_with(suffix)))
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
                            .filter(field, C::EndsWith, suffix.as_slice())
                            .work_limit(3)
                            .limit(2);
                        if indexed {
                            q = q.using_index(index, &[]);
                        }
                        if descending {
                            q = q.descending();
                        }
                        if let Some(cursor) = &cursor {
                            q = q.after(cursor);
                        }
                        let page = q.fetch().unwrap();
                        assert!(page.examined <= 3);
                        examined += page.examined;
                        actual.extend(page.rows);
                        cursor = page.next;
                        if cursor.is_none() {
                            break;
                        }
                        assert!(page_number < 15, "suffix pages must progress");
                    }
                    assert_eq!(actual, expected);
                    assert_eq!(examined, rows.len(), "suffixes remain residual filters");
                }
            }
        }
    }
    let groups = db
        .query::<Binary>()
        .filter_groups(C::EndsWith, [255].as_slice())
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
                        let mut q = query().limit(2).work_limit(3);
                        if let Some(after) = &cursor {
                            q = q.after(after);
                        }
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
    prefix.extend(Binary::ID.to_be_bytes());
    prefix.extend(index.id.to_be_bytes());
    // Literal bytes, independent of the product encoder: payload, id component, owner.
    let mut valid = prefix.clone();
    valid.extend([255, 0, 0]);
    valid.extend(1_u64.to_be_bytes());
    valid.extend(1_u64.to_be_bytes());
    for component in [vec![255, 0, 1], vec![255], vec![0, 255, 0]] {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
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
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
        raw.remove(&bad);
        raw.insert(valid.clone(), 1_u64.to_be_bytes().to_vec());
    }
}
