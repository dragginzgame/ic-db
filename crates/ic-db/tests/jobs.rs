//! Exact retained aggregates: independent results, retries, invalidation and reclamation.

#[path = "support/allocations.rs"]
mod allocations;
#[path = "support/raw_map.rs"]
pub mod raw_map;
use crate::raw_map::RawMap;
use ic_db::design::prelude::*;
use ic_db::{AggregateJobs, Batch, Comparison, Database, Error, GroupOrder, JobHandle, JobOutcome};
use ic_storage::{MemoryRuntime, StableKey, VectorMemory};

///
/// Row
/// Exercises indexed access by `flag` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(field = "flag"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "flag", value(item(prim = "Nat8"))),
        field(name = "amount", value(opt, item(prim = "Int128"))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 3500)))
    )
)]
struct Row {}

///
/// Other
/// Exercises a row containing only its `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
struct Other {}

///
/// Amount
/// Carries an optional `Int128` value of `amount`.
///

#[record(fields(field(name = "amount", value(opt, item(prim = "Int128")))))]
struct Amount {}

///
/// Id
/// Carries the `Nat64` value of `id`.
///

#[record(fields(field(name = "id", value(item(prim = "Nat64")))))]
struct Id {}

///
/// Payload
/// Carries the `Blob` value of `payload`.
///

#[record(fields(field(name = "payload", value(item(prim = "Blob", max_bytes = 3500)))))]
struct Payload {}
fn time(ms: i64) -> Timestamp {
    Timestamp::from_millis(ms)
}

#[test]
fn retained_blob_groups_and_distinct_merge_across_sparse_reopens_and_pages() {
    use std::collections::BTreeMap;
    let keys = [
        vec![],
        vec![0],
        vec![0, 0],
        vec![0, 255],
        vec![128],
        vec![255],
    ];
    let rows = (0..64)
        .map(|id| Row {
            id,
            flag: u8::from(id >= 24),
            amount: None,
            payload: if id < 24 {
                vec![42]
            } else {
                keys[(id as usize - 24) % keys.len()].clone()
            },
        })
        .collect::<Vec<_>>();
    let mut counts = BTreeMap::<Vec<u8>, u64>::new();
    for row in rows.iter().filter(|r| r.flag == 1) {
        *counts.entry(row.payload.clone()).or_default() += 1;
    }
    let distinct = counts.len() as u64;
    let mut expected = counts
        .into_iter()
        .filter(|(_, n)| *n >= 7)
        .collect::<Vec<_>>();
    expected.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    expected.truncate(3);
    for indexed in [false, true] {
        for work in [1, 7] {
            let backing = VectorMemory::default();
            let runtime = host(backing.clone());
            let (mut db, mut jobs) = create_stores(&runtime, schema());
            for chunk in rows.chunks(32) {
                let mut batch = Batch::new();
                for row in chunk {
                    batch.insert(row).unwrap();
                }
                db.commit(batch).unwrap();
            }
            let query = || {
                let q = db.query::<Row>().filter("flag", Comparison::Eq, 1_u8);

                if indexed {
                    q.using_index("flag", &[])
                } else {
                    q
                }
            };
            let grouped = query()
                .limit(3)
                .filter_groups(Comparison::Ge, 7_u64)
                .order_groups(GroupOrder::AggregateDescending)
                .start_group_count::<Payload>(&mut jobs, time(0), time(100))
                .unwrap();
            let unique = query()
                .start_count_distinct::<Payload>(&mut jobs, time(0), time(100))
                .unwrap();
            drop((db, jobs, runtime));
            let mut sequence = 0;
            let mut examined = 0;
            let mut cursor = None;
            let mut actual = Vec::new();
            let mut complete = false;
            // Reopen after every advance and every completed page. Bounds keep a
            // broken continuation from turning this regression into an endless loop.
            for _ in 0..70 {
                let runtime = host(backing.clone());
                let db = Database::open(
                    runtime
                        .open_memory(&StableKey::parse("test.db.v1").unwrap())
                        .unwrap(),
                    schema(),
                )
                .unwrap();
                let mut jobs = AggregateJobs::open(
                    runtime
                        .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
                        .unwrap(),
                )
                .unwrap();
                let query = || {
                    let q = db
                        .query::<Row>()
                        .filter("flag", Comparison::Eq, 1_u8)
                        .work_limit(work);

                    if indexed {
                        q.using_index("flag", &[])
                    } else {
                        q
                    }
                };
                let groups = || {
                    query()
                        .limit(3)
                        .filter_groups(Comparison::Ge, 7_u64)
                        .order_groups(GroupOrder::AggregateDescending)
                        .group_page(2, cursor.as_ref())
                };
                let saved = backing.borrow().clone();
                if cursor.is_some() {
                    assert!(matches!(
                        groups().test_output_limit(1).resume_group_count::<Payload>(
                            &mut jobs,
                            &grouped,
                            sequence,
                            time(1)
                        ),
                        Err(Error::RowTooLarge)
                    ));
                    assert_eq!(*backing.borrow(), saved);
                }
                let step = groups()
                    .resume_group_count::<Payload>(&mut jobs, &grouped, sequence, time(1))
                    .unwrap();
                let checkpoint = backing.borrow().clone();
                let retry = groups()
                    .resume_group_count::<Payload>(&mut jobs, &grouped, sequence, time(1))
                    .unwrap();
                assert_eq!(
                    (retry.sequence, retry.examined),
                    (step.sequence, step.examined)
                );
                match (&retry.outcome, &step.outcome) {
                    (JobOutcome::Running, JobOutcome::Running) => {}
                    (JobOutcome::Complete(retry), JobOutcome::Complete(page)) => {
                        assert_eq!(
                            retry
                                .rows
                                .iter()
                                .map(|r| (&r.key.payload, r.count))
                                .collect::<Vec<_>>(),
                            page.rows
                                .iter()
                                .map(|r| (&r.key.payload, r.count))
                                .collect::<Vec<_>>()
                        );
                        assert_eq!(
                            retry.next.as_ref().map(|c| c.to_bytes()),
                            page.next.as_ref().map(|c| c.to_bytes())
                        );
                    }
                    other => panic!("retry changed the result: {other:?}"),
                }
                assert_eq!(*backing.borrow(), checkpoint);
                if cursor.is_none() {
                    let count = query()
                        .resume_count_distinct::<Payload>(&mut jobs, &unique, sequence, time(1))
                        .unwrap();
                    assert_eq!(
                        (count.sequence, count.examined),
                        (step.sequence, step.examined)
                    );

                    match count.outcome {
                        JobOutcome::Running => assert!(matches!(step.outcome, JobOutcome::Running)),
                        JobOutcome::Complete(value) => assert_eq!(value, distinct),
                        other => panic!("unexpected distinct outcome: {other:?}"),
                    }
                }
                assert!(step.examined >= examined && step.examined - examined <= work as u64);
                examined = step.examined;
                sequence = step.sequence;
                if !indexed && examined <= 24 {
                    assert!(matches!(step.outcome, JobOutcome::Running));
                }

                match step.outcome {
                    JobOutcome::Running => assert!(cursor.is_none()),
                    JobOutcome::Complete(page) => {
                        assert_eq!(examined, if indexed { 40 } else { 64 });
                        actual.extend(page.rows.into_iter().map(|r| (r.key.payload, r.count)));
                        cursor = page.next;
                        if cursor.is_none() {
                            jobs.cancel(&grouped).unwrap();
                            jobs.cancel(&unique).unwrap();
                            complete = true;
                            break;
                        }
                    }
                    other => panic!("unexpected grouped outcome: {other:?}"),
                }
            }
            assert!(complete);
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn retained_blob_group_prefix_selection_waits_for_final_winners_and_pages() {
    // Later rows move winners into/out of the prefix. Row 8 must be excluded
    // before accumulation, and the result limit must apply after group selection.
    let values = [
        (0, vec![0]),
        (1, vec![255]),
        (2, vec![]),
        (3, vec![0]),
        (0, vec![255]),
        (1, vec![0, 255]),
        (2, vec![0, 255]),
        (3, vec![0, 255, 128]),
        (3, vec![255]),
    ];
    for maximum in [false, true] {
        let expected = if maximum {
            vec![(3, vec![0, 255, 128]), (2, vec![0, 255])]
        } else {
            vec![(1, vec![0, 255]), (0, vec![0])]
        };
        for indexed in [false, true] {
            let backing = VectorMemory::default();
            let runtime = host(backing.clone());
            let (mut db, mut jobs) = create_stores(&runtime, schema());
            let mut batch = Batch::new();
            for (id, (flag, payload)) in values.iter().enumerate() {
                batch
                    .insert(&Row {
                        id: id as u64,
                        flag: *flag,
                        amount: None,
                        payload: payload.clone(),
                    })
                    .unwrap();
            }
            db.commit(batch).unwrap();
            let mut query = db
                .query::<Row>()
                .filter_between("id", 0_u64, 7_u64)
                .filter_groups(Comparison::StartsWith, [0].as_slice())
                .order_groups(GroupOrder::AggregateDescending)
                .limit(2);
            if indexed {
                query = query.using_index("flag", &[]);
            }
            let handle = if maximum {
                query.start_group_max::<Flag, Payload>(&mut jobs, time(0), time(100))
            } else {
                query.start_group_min::<Flag, Payload>(&mut jobs, time(0), time(100))
            }
            .unwrap();
            drop((db, jobs, runtime));
            let mut sequence = 0;
            let mut cursor = None;
            let mut actual = Vec::new();
            let mut finished = false;
            for _ in 0..12 {
                let runtime = host(backing.clone());
                let db = Database::open(
                    runtime
                        .open_memory(&StableKey::parse("test.db.v1").unwrap())
                        .unwrap(),
                    schema(),
                )
                .unwrap();
                let mut jobs = AggregateJobs::open(
                    runtime
                        .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
                        .unwrap(),
                )
                .unwrap();
                let query = |prefix: &'static [u8]| {
                    let q = db
                        .query::<Row>()
                        .filter_between("id", 0_u64, 7_u64)
                        .filter_groups(Comparison::StartsWith, prefix)
                        .order_groups(GroupOrder::AggregateDescending)
                        .limit(2)
                        .work_limit(1)
                        .group_page(1, cursor.as_ref());

                    if indexed {
                        q.using_index("flag", &[])
                    } else {
                        q
                    }
                };
                if sequence == 1 {
                    let saved = backing.borrow().clone();
                    let changed = if maximum {
                        query(&[255]).resume_group_max::<Flag, Payload>(
                            &mut jobs,
                            &handle,
                            sequence,
                            time(1),
                        )
                    } else {
                        query(&[255]).resume_group_min::<Flag, Payload>(
                            &mut jobs,
                            &handle,
                            sequence,
                            time(1),
                        )
                    };
                    assert!(matches!(changed, Err(Error::InvalidJob)));
                    assert_eq!(*backing.borrow(), saved);
                }
                let step = if maximum {
                    query(&[0]).resume_group_max::<Flag, Payload>(
                        &mut jobs,
                        &handle,
                        sequence,
                        time(1),
                    )
                } else {
                    query(&[0]).resume_group_min::<Flag, Payload>(
                        &mut jobs,
                        &handle,
                        sequence,
                        time(1),
                    )
                }
                .unwrap();
                sequence = step.sequence;

                match step.outcome {
                    JobOutcome::Running => assert!(actual.is_empty()),
                    JobOutcome::Complete(page) => {
                        assert_eq!(step.examined, if indexed { 9 } else { 8 });
                        assert_eq!(page.rows.len(), 1);
                        actual.extend(
                            page.rows
                                .into_iter()
                                .map(|r| (r.key.flag, r.value.unwrap().payload)),
                        );
                        cursor = page.next;
                        if cursor.is_none() {
                            jobs.cancel(&handle).unwrap();
                            finished = true;
                            break;
                        }
                    }
                    other => panic!("unexpected group outcome: {other:?}"),
                }
            }
            assert!(finished, "completed group pages must terminate");
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn retained_optional_blob_groups_preserve_absence_across_chunks() {
    use std::collections::BTreeMap;

    ///
    /// Binary
    /// Exercises indexed access by `bytes` on rows keyed by `id`.
    ///

    #[entity(
        store = ic_db::testing::FixtureStore,
        pk(field = "id"),
        index(field = "bytes"),
        fields(
            field(name = "id", value(item(prim = "Nat64"))),
            field(name = "bytes", value(opt, item(prim = "Blob", max_bytes = 16)))
        )
    )]
    struct Binary {}

    ///
    /// Key
    /// Carries an optional `Blob` value of `bytes`.
    ///

    #[record(fields(field(name = "bytes", value(opt, item(prim = "Blob", max_bytes = 16)))))]
    struct Key {}
    let values = [
        None,
        Some(vec![]),
        Some(vec![0]),
        None,
        Some(vec![255]),
        Some(vec![]),
    ];
    let rows = (0..24)
        .map(|id| Binary {
            id,
            bytes: values[id as usize % values.len()].clone(),
        })
        .collect::<Vec<_>>();
    let mut expected = BTreeMap::<Option<Vec<u8>>, u64>::new();
    for row in &rows {
        *expected.entry(row.bytes.clone()).or_default() += u64::from(row.bytes.is_some());
    }
    for indexed in [false, true] {
        let backing = VectorMemory::default();
        let runtime = host(backing.clone());
        let schema = || Schema::new(&[Binary::SCHEMA]).unwrap();
        let (mut db, mut jobs) = create_stores(&runtime, schema());
        let mut batch = Batch::new();
        for row in &rows {
            batch.insert(row).unwrap();
        }
        db.commit(batch).unwrap();
        let query = || {
            let q = db.query::<Binary>();

            if indexed {
                q.using_index("bytes", &[])
            } else {
                q
            }
        };
        let groups = query()
            .start_group_count_field::<Key>("bytes", &mut jobs, time(0), time(100))
            .unwrap();
        let distinct = query()
            .start_count_distinct::<Key>(&mut jobs, time(0), time(100))
            .unwrap();
        drop((db, jobs, runtime));
        let mut sequence = 0;
        let mut complete = false;
        for _ in 0..14 {
            let runtime = host(backing.clone());
            let db = Database::open(
                runtime
                    .open_memory(&StableKey::parse("test.db.v1").unwrap())
                    .unwrap(),
                schema(),
            )
            .unwrap();
            let mut jobs = AggregateJobs::open(
                runtime
                    .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
                    .unwrap(),
            )
            .unwrap();
            let query = || {
                let q = db.query::<Binary>().work_limit(2);

                if indexed {
                    q.using_index("bytes", &[])
                } else {
                    q
                }
            };
            let g = query()
                .resume_group_count_field::<Key>("bytes", &mut jobs, &groups, sequence, time(1))
                .unwrap();
            let d = query()
                .resume_count_distinct::<Key>(&mut jobs, &distinct, sequence, time(1))
                .unwrap();
            assert_eq!((g.sequence, g.examined), (d.sequence, d.examined));
            sequence = g.sequence;

            match (g.outcome, d.outcome) {
                (JobOutcome::Running, JobOutcome::Running) => {}
                (JobOutcome::Complete(page), JobOutcome::Complete(count)) => {
                    assert_eq!(g.examined, rows.len() as u64);
                    assert!(page.next.is_none());
                    assert_eq!(page.rows.len(), expected.len());
                    assert_eq!(
                        page.rows
                            .into_iter()
                            .map(|r| (r.key.bytes, r.count))
                            .collect::<BTreeMap<_, _>>(),
                        expected
                    );
                    assert_eq!(count, expected.len() as u64);
                    jobs.cancel(&groups).unwrap();
                    jobs.cancel(&distinct).unwrap();
                    complete = true;
                    break;
                }
                other => panic!("inconsistent optional Blob completion: {other:?}"),
            }
        }
        assert!(complete);
    }
}

#[test]
fn retained_blob_prefix_binding_survives_reopen() {
    retained_blob_pattern_reopen(Comparison::StartsWith, &[0, 255]);
}

#[test]
fn retained_blob_suffix_binding_survives_reopen() {
    retained_blob_pattern_reopen(Comparison::EndsWith, &[255]);
}

#[test]
fn retained_blob_substring_binding_survives_reopen() {
    retained_blob_pattern_reopen(Comparison::Contains, &[255]);
}

fn retained_blob_pattern_reopen(comparison: Comparison, pattern: &[u8]) {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (mut db, mut jobs) = create_stores(&runtime, schema());
    let values = [
        vec![],
        vec![0],
        vec![0, 255],
        vec![0, 255, 128],
        vec![255],
        vec![0, 0],
    ];
    let mut batch = Batch::new();
    for (id, payload) in values.iter().enumerate() {
        batch
            .insert(&Row {
                id: id as u64,
                flag: 0,
                amount: None,
                payload: payload.clone(),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let expected = values
        .iter()
        .filter(|v| match comparison {
            Comparison::StartsWith => v.starts_with(pattern),
            Comparison::EndsWith => v.ends_with(pattern),
            Comparison::Contains => v.windows(pattern.len()).any(|w| w == pattern),
            _ => unreachable!(),
        })
        .count() as u64;
    let job = db
        .query::<Row>()
        .filter("payload", comparison, pattern)
        .start_count(&mut jobs, time(0), time(100))
        .unwrap();
    let step = db
        .query::<Row>()
        .filter("payload", comparison, pattern)
        .work_limit(1)
        .resume_count(&mut jobs, &job, 0, time(1))
        .unwrap();
    assert!(matches!(step.outcome, JobOutcome::Running));
    drop((db, jobs, runtime));
    let runtime = host(backing.clone());
    let db = Database::open(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let saved = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .filter("payload", comparison, [0].as_slice())
            .work_limit(1)
            .resume_count(&mut jobs, &job, step.sequence, time(1)),
        Err(Error::InvalidJob)
    ));
    assert_eq!(*backing.borrow(), saved);
    let mut sequence = step.sequence;
    let mut complete = false;
    for _ in 0..8 {
        let step = db
            .query::<Row>()
            .filter("payload", comparison, pattern)
            .work_limit(1)
            .resume_count(&mut jobs, &job, sequence, time(1))
            .unwrap();
        sequence = step.sequence;

        match step.outcome {
            JobOutcome::Running => {}
            JobOutcome::Complete(count) => {
                assert_eq!(count, expected);
                assert_eq!(step.examined, values.len() as u64);
                complete = true;
                break;
            }
            other => panic!("unexpected pattern count: {other:?}"),
        }
    }
    assert!(complete);
    jobs.cancel(&job).unwrap();
}

#[test]
fn range_counts_resume_after_reopen_without_visiting_outside_rows() {
    for index in [false, true] {
        let (field, lower, upper) = if index {
            (
                "flag",
                ic_db::IndexValue::from(1_u8),
                ic_db::IndexValue::from(2_u8),
            )
        } else {
            (
                "id",
                ic_db::IndexValue::from(3_u64),
                ic_db::IndexValue::from(7_u64),
            )
        };
        let backing = VectorMemory::default();
        let runtime = host(backing.clone());
        let (db, mut jobs) = create(&runtime, &[Some(1); 8], 0);
        let query = || {
            let query = db
                .query::<Row>()
                .filter(field, Comparison::Ge, lower)
                .filter(field, Comparison::Lt, upper);

            if index {
                query.using_index("flag", &[])
            } else {
                query
            }
        };
        let handle = query().start_count(&mut jobs, time(0), time(100)).unwrap();
        let first = query()
            .work_limit(2)
            .resume_count(&mut jobs, &handle, 0, time(1))
            .unwrap();
        assert_eq!(first.examined, 2);
        assert!(matches!(first.outcome, JobOutcome::Running));
        drop((db, jobs, runtime));
        let runtime = host(backing);
        let db = Database::open(
            runtime
                .open_memory(&StableKey::parse("test.db.v1").unwrap())
                .unwrap(),
            schema(),
        )
        .unwrap();
        let mut jobs = AggregateJobs::open(
            runtime
                .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
                .unwrap(),
        )
        .unwrap();
        let query = || {
            let query = db
                .query::<Row>()
                .filter(field, Comparison::Ge, lower)
                .filter(field, Comparison::Lt, upper);

            if index {
                query.using_index("flag", &[])
            } else {
                query
            }
        };
        let second = query()
            .work_limit(2)
            .resume_count(&mut jobs, &handle, first.sequence, time(2))
            .unwrap();
        assert_eq!(second.examined, 4);
        assert!(matches!(second.outcome, JobOutcome::Complete(4)));
        let retry = query()
            .work_limit(1)
            .resume_count(&mut jobs, &handle, first.sequence, time(3))
            .unwrap();
        assert_eq!(retry.examined, 4);
        assert!(matches!(retry.outcome, JobOutcome::Complete(4)));
    }
}

#[test]
fn group_or_jobs_bind_clauses_and_resume_completed_pages_after_reopen() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(&runtime, &[Some(1); 6], 0);
    let alternatives = [
        (Comparison::Eq, 0_u64.into()),
        (Comparison::Eq, 3_u64.into()),
    ];
    let handle = db
        .query::<Row>()
        .filter_groups_any(&alternatives)
        .start_group_count::<Flag>(&mut jobs, time(0), time(100))
        .unwrap();
    let first = db
        .query::<Row>()
        .filter_groups_any(&alternatives)
        .work_limit(2)
        .resume_group_count::<Flag>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    // Singleton clauses retain ordinary conjunction identities.
    let singleton = db
        .query::<Row>()
        .filter_groups(Comparison::Ge, 0_u64)
        .start_group_count::<Flag>(&mut jobs, time(0), time(100))
        .unwrap();
    assert!(matches!(
        db.query::<Row>()
            .filter_groups_any(&[(Comparison::Ge, 0_u64.into())])
            .resume_group_count::<Flag>(&mut jobs, &singleton, 0, time(1))
            .unwrap()
            .outcome,
        JobOutcome::Complete(_)
    ));
    drop((db, jobs, runtime));
    let runtime = host(backing.clone());
    let db = Database::open(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let saved = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .filter_groups(Comparison::Eq, 0_u64)
            .filter_groups(Comparison::Eq, 3_u64)
            .resume_group_count::<Flag>(&mut jobs, &handle, first.sequence, time(2)),
        Err(Error::InvalidJob)
    ));
    assert_eq!(*backing.borrow(), saved);
    let result = db
        .query::<Row>()
        .filter_groups_any(&alternatives)
        .group_page(1, None)
        .resume_group_count::<Flag>(&mut jobs, &handle, first.sequence, time(2))
        .unwrap();
    let JobOutcome::Complete(page) = result.outcome else {
        panic!("expected completion");
    };
    assert_eq!((page.rows[0].key.flag, page.rows[0].count), (0, 3));
    let cursor = page.next.unwrap();
    let result = db
        .query::<Row>()
        .filter_groups_any(&alternatives)
        .group_page(1, Some(&cursor))
        .resume_group_count::<Flag>(&mut jobs, &handle, result.sequence, time(3))
        .unwrap();
    let JobOutcome::Complete(page) = result.outcome else {
        panic!("expected second page");
    };
    assert_eq!((page.rows[0].key.flag, page.rows[0].count), (1, 3));
    assert!(page.next.is_none());
}

#[test]
fn range_helpers_resume_exact_counts_through_explicit_comparisons_after_reopen() {
    for outside in [false, true] {
        let backing = VectorMemory::default();
        let runtime = host(backing.clone());
        let (db, mut jobs) = create(
            &runtime,
            &[Some(10), None, Some(20), Some(30), Some(40), Some(50)],
            0,
        );
        let query = db.query::<Row>();
        let query = if outside {
            query.filter_not_between("amount", 20_i128, 30_i128)
        } else {
            query.filter_between("amount", 20_i128, 30_i128)
        };
        let handle = query.start_count(&mut jobs, time(0), time(100)).unwrap();
        drop((db, jobs, runtime));
        let mut sequence = 0;
        let mut completed = false;
        for _ in 0..7 {
            let runtime = host(backing.clone());
            let db = Database::open(
                runtime
                    .open_memory(&StableKey::parse("test.db.v1").unwrap())
                    .unwrap(),
                schema(),
            )
            .unwrap();
            let mut jobs = AggregateJobs::open(
                runtime
                    .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
                    .unwrap(),
            )
            .unwrap();
            let alternatives = [
                ("amount", Comparison::Lt, 20_i128.into()),
                ("amount", Comparison::Gt, 30_i128.into()),
            ];
            let query = db.query::<Row>();
            let query = if outside {
                query.filter_any(&alternatives)
            } else {
                query.filter("amount", Comparison::Ge, 20_i128).filter(
                    "amount",
                    Comparison::Le,
                    30_i128,
                )
            };
            let step = query
                .work_limit(1)
                .resume_count(&mut jobs, &handle, sequence, time(1))
                .unwrap();
            sequence = step.sequence;

            match step.outcome {
                JobOutcome::Running => {}
                JobOutcome::Complete(value) => {
                    assert_eq!(value, if outside { 3 } else { 2 });
                    assert_eq!(step.examined, 6);
                    completed = true;
                    break;
                }
                other => panic!("unexpected outcome: {other:?}"),
            }
        }
        assert!(completed, "range aggregate must complete");
    }
}

#[test]
fn exclusion_jobs_share_inequality_identity_and_exact_counts() {
    let runtime = host(VectorMemory::default());
    let (db, mut jobs) = create(&runtime, &[Some(10), None, Some(30), Some(10)], 0);
    let values = [ic_db::IndexValue::Absent, 10_i128.into(), 10_i128.into()];
    let handle = db
        .query::<Row>()
        .filter_not_in("amount", &values)
        .start_count(&mut jobs, time(0), time(100))
        .unwrap();
    let mut sequence = 0;
    for _ in 0..4 {
        let step = db
            .query::<Row>()
            .filter("amount", Comparison::Ne, ic_db::IndexValue::Absent)
            .filter("amount", Comparison::Ne, 10_i128)
            .filter("amount", Comparison::Ne, 10_i128)
            .work_limit(2)
            .resume_count(&mut jobs, &handle, sequence, time(1))
            .unwrap();
        sequence = step.sequence;

        match step.outcome {
            JobOutcome::Running => {}
            JobOutcome::Complete(value) => {
                assert_eq!(value, 1);
                assert_eq!(step.examined, 4);

                return;
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
    panic!("bounded exclusion must complete");
}

#[test]
fn membership_jobs_share_equality_clause_identity_and_exact_counts() {
    let runtime = host(VectorMemory::default());
    let (db, mut jobs) = create(&runtime, &[Some(10), None, Some(30), Some(10)], 0);
    let values = [ic_db::IndexValue::Absent, 10_i128.into(), 10_i128.into()];
    let handle = db
        .query::<Row>()
        .filter_in("amount", &values)
        .start_count(&mut jobs, time(0), time(100))
        .unwrap();
    let alternatives = values.map(|value| ("amount", Comparison::Eq, value));
    let mut sequence = 0;
    for _ in 0..4 {
        let step = db
            .query::<Row>()
            .filter_any(&alternatives)
            .work_limit(2)
            .resume_count(&mut jobs, &handle, sequence, time(1))
            .unwrap();
        sequence = step.sequence;

        match step.outcome {
            JobOutcome::Running => {}
            JobOutcome::Complete(value) => {
                assert_eq!(value, 3);
                assert_eq!(step.examined, 4);

                return;
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
    panic!("bounded membership must complete");
}

#[test]
fn any_clauses_resume_exact_counts_and_refuse_changed_boolean_structure() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(&runtime, &[Some(10), None, Some(30), Some(40)], 0);
    let alternatives = [
        ("id", Comparison::Lt, 2_u64.into()),
        ("amount", Comparison::Ge, 30_i128.into()),
    ];
    let query = || db.query::<Row>().filter_any(&alternatives).work_limit(1);
    let handle = query().start_count(&mut jobs, time(0), time(100)).unwrap();
    let first = query()
        .resume_count(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    let saved = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .filter("id", Comparison::Lt, 2_u64)
            .filter("amount", Comparison::Ge, 30_i128)
            .resume_count(&mut jobs, &handle, first.sequence, time(1)),
        Err(Error::InvalidJob)
    ));
    assert_eq!(*backing.borrow(), saved);
    let retry = query()
        .resume_count(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert_eq!(retry.sequence, first.sequence);
    assert_eq!(retry.examined, first.examined);
    drop(jobs);
    drop(db);
    drop(runtime);

    let runtime = host(backing);
    let db = Database::open(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let mut sequence = first.sequence;
    for _ in 0..5 {
        let step = db
            .query::<Row>()
            .filter_any(&alternatives)
            .work_limit(1)
            .resume_count(&mut jobs, &handle, sequence, time(2))
            .unwrap();
        sequence = step.sequence;

        match step.outcome {
            JobOutcome::Running => {}
            JobOutcome::Complete(value) => {
                assert_eq!(value, 4);
                assert_eq!(step.examined, 4);

                return;
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
    panic!("bounded input must complete");
}

fn host(backing: VectorMemory) -> MemoryRuntime<VectorMemory> {
    allocations::host_with(backing, &["test.db.v1", "test.jobs.v1"])
}

fn schema() -> Schema {
    Schema::new(&[Row::SCHEMA, Other::SCHEMA]).unwrap()
}

fn create_stores(host: &MemoryRuntime<VectorMemory>, schema: Schema) -> (Database, AggregateJobs) {
    let db = Database::create(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema,
        [1; 16],
    )
    .unwrap();
    let jobs = AggregateJobs::create(
        host.open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
        [2; 16],
    )
    .unwrap();

    (db, jobs)
}

fn create(
    host: &MemoryRuntime<VectorMemory>,
    values: &[Option<i128>],
    payload: usize,
) -> (Database, AggregateJobs) {
    let (mut db, jobs) = create_stores(host, schema());
    for (chunk, values) in values.chunks(32).enumerate() {
        let mut batch = Batch::new();
        for (i, amount) in values.iter().enumerate() {
            let id = (chunk * 32 + i) as u64;
            batch
                .insert(&Row {
                    id,
                    amount: *amount,
                    flag: (id % 2) as u8,
                    payload: vec![1; payload],
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
    }

    (db, jobs)
}

#[test]
fn retained_length_filter_binds_threshold_and_resumes_the_same_clause() {
    use ic_db::QueryOperand::{Length, Literal};
    let runtime = host(VectorMemory::default());
    let (db, mut jobs) = create(&runtime, &[None, None, None], 4);
    let query = |length| {
        db.query::<Row>().filter_any_operands(&[
            ("payload", Comparison::Lt, Length(length)),
            ("id", Comparison::Eq, Literal(2_u64.into())),
        ])
    };
    let handle = query(4).start_count(&mut jobs, time(0), time(100)).unwrap();
    let first = query(4)
        .work_limit(1)
        .resume_count(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    assert!(matches!(
        query(5).resume_count(&mut jobs, &handle, first.sequence, time(1)),
        Err(Error::InvalidJob)
    ));
    let finished = query(4)
        .resume_count(&mut jobs, &handle, first.sequence, time(1))
        .unwrap();
    assert!(matches!(finished.outcome, JobOutcome::Complete(1)));
}

#[test]
fn retained_index_count_preserves_checkpoint_on_invalid_owner() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(&runtime, &[None; 5], 0);
    fn query(db: &Database) -> ic_db::Query<'_, Row> {
        db.query::<Row>().using_index("flag", &[0_u8.into()])
    }
    let handle = query(&db)
        .start_count(&mut jobs, time(0), time(100))
        .unwrap();
    let first = query(&db)
        .work_limit(1)
        .resume_count(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    assert_eq!(first.examined, 1);
    let mut changed = db.get::<Row>(4).unwrap().unwrap();
    changed.flag = 1;
    drop(db);

    // Keep the flag-0 entries for IDs 0, 2 and 4. Both faults occur after
    // the advance has processed the healthy owner at ID 2.
    let key = StableKey::parse("test.db.v1").unwrap();
    let row = [
        &[2][..],
        &Row::SCHEMA.id.to_be_bytes(),
        &4_u64.to_be_bytes(),
    ]
    .concat();
    let raw = || RawMap::load(runtime.open_memory(&key).unwrap());
    let saved = raw().remove(&row).unwrap();
    let reopen = || Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
    let mut check_refusal = |expected_flag| {
        let db = reopen();
        assert_eq!(
            db.get::<Row>(4).unwrap().map(|owner| owner.flag),
            expected_flag
        );
        assert_eq!(query(&db).count().unwrap().value, 3);
        let before = backing.borrow().clone();
        assert!(matches!(
            query(&db)
                .work_limit(2)
                .resume_count(&mut jobs, &handle, first.sequence, time(2)),
            Err(Error::Corrupt)
        ));
        assert_eq!(*backing.borrow(), before);
    };
    check_refusal(None);
    raw().insert(row.clone(), changed.encode().unwrap());
    check_refusal(Some(1));

    raw().insert(row, saved);
    let db = reopen();
    let done = query(&db)
        .work_limit(2)
        .resume_count(&mut jobs, &handle, first.sequence, time(2))
        .unwrap();
    assert!(matches!(done.outcome, JobOutcome::Complete(3)));
    assert_eq!((done.sequence, done.examined), (first.sequence + 1, 3));
}

#[test]
fn count_and_sum_resume_beyond_synchronous_budget_without_prefix_results() {
    let host = host(VectorMemory::default());
    let (db, mut jobs) = create(&host, &vec![Some(7); 520], 0);
    assert!(matches!(
        db.query::<Row>().work_limit(512).count_validated(),
        Err(Error::Limit)
    ));
    let count = db
        .query::<Row>()
        .start_count(&mut jobs, time(0), time(100))
        .unwrap();
    let sum = db
        .query::<Row>()
        .start_sum::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let mut sequence = 0;

    loop {
        let c = db
            .query::<Row>()
            .work_limit(37)
            .resume_count(&mut jobs, &count, sequence, time(1))
            .unwrap();
        let s = db
            .query::<Row>()
            .work_limit(37)
            .resume_sum::<Amount>(&mut jobs, &sum, sequence, time(1))
            .unwrap();
        assert_eq!(c.examined, s.examined);
        let retry = db
            .query::<Row>()
            .work_limit(512)
            .resume_count(&mut jobs, &count, sequence, time(1))
            .unwrap();
        assert_eq!(retry.sequence, c.sequence);
        assert_eq!(retry.examined, c.examined);
        assert_eq!(retry.outcome, c.outcome);
        sequence = c.sequence;

        match c.outcome {
            JobOutcome::Running => assert!(matches!(s.outcome, JobOutcome::Running)),
            JobOutcome::Complete(n) => {
                assert_eq!(n, 520);
                let JobOutcome::Complete(Some(value)) = s.outcome else {
                    panic!("missing complete sum")
                };
                assert_eq!(value.amount, Some(3640));
                assert_eq!(c.examined, 520);
                break;
            }
            _ => panic!("unexpected terminal state"),
        }
    }
}

#[test]
fn sparse_index_work_and_source_bytes_bound_progress_independently() {
    let host = host(VectorMemory::default());
    let amounts = std::array::from_fn::<_, 64, _>(|id| Some(id as i128));
    let (db, mut jobs) = create(&host, &amounts, 3500);
    let query = || {
        db.query::<Row>()
            .using_index("flag", &[0_u8.into()])
            .filter("amount", Comparison::Ge, 60_i128)
    };
    let handle = query().start_count(&mut jobs, time(0), time(100)).unwrap();
    let mut sequence = 0;
    let mut previous_bytes = 0;
    let mut previous_examined = 0;

    loop {
        let step = query()
            .work_limit(512)
            .resume_count(&mut jobs, &handle, sequence, time(1))
            .unwrap();
        assert!(step.input_bytes - previous_bytes <= 64 * 1024);
        assert!(step.examined - previous_examined < 32);
        assert!(step.examined > previous_examined);
        sequence = step.sequence;
        previous_bytes = step.input_bytes;
        previous_examined = step.examined;

        match step.outcome {
            JobOutcome::Running => {}
            JobOutcome::Complete(n) => {
                assert_eq!(n, 2);
                assert_eq!(step.examined, 32);
                break;
            }
            _ => panic!("unexpected terminal state"),
        }
    }
}

#[test]
fn same_code_reopen_retains_exact_signed_sum_and_absence() {
    let backing = VectorMemory::default();
    let handle;
    {
        let host = host(backing.clone());
        let (db, mut jobs) = create(
            &host,
            &[Some(i128::MAX), Some(i128::MAX), None, Some(-i128::MAX)],
            0,
        );
        handle = db
            .query::<Row>()
            .start_sum::<Amount>(&mut jobs, time(0), time(100))
            .unwrap()
            .to_bytes();
        let step = db
            .query::<Row>()
            .work_limit(2)
            .resume_sum::<Amount>(
                &mut jobs,
                &JobHandle::from_bytes(&handle).unwrap(),
                0,
                time(1),
            )
            .unwrap();
        assert!(matches!(step.outcome, JobOutcome::Running));
        let replay = db
            .query::<Row>()
            .resume_sum::<Amount>(
                &mut jobs,
                &JobHandle::from_bytes(&handle).unwrap(),
                0,
                time(3),
            )
            .unwrap();
        assert!(matches!(replay.outcome, JobOutcome::Running));
        assert_eq!(
            (replay.sequence, replay.examined, replay.input_bytes),
            (step.sequence, step.examined, step.input_bytes)
        );
    }
    let host = host(backing);
    let db = Database::open(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        host.open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let handle = JobHandle::from_bytes(&handle).unwrap();
    assert!(matches!(
        db.query::<Row>()
            .resume_sum::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::TimestampRegression)
    ));
    let step = db
        .query::<Row>()
        .resume_sum::<Amount>(&mut jobs, &handle, 1, time(3))
        .unwrap();
    assert_eq!((step.sequence, step.examined), (2, 4));
    let JobOutcome::Complete(Some(value)) = step.outcome else {
        panic!("missing complete sum")
    };
    assert_eq!(value.amount, Some(i128::MAX));
    let absent = db
        .query::<Row>()
        .filter("id", Comparison::Eq, 2_u64)
        .start_sum::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    assert!(matches!(
        db.query::<Row>()
            .filter("id", Comparison::Eq, 2_u64)
            .resume_sum::<Amount>(&mut jobs, &absent, 0, time(1))
            .unwrap()
            .outcome,
        JobOutcome::Complete(None)
    ));
}

#[test]
fn source_changes_invalidate_but_noops_refusals_other_entities_and_finished_jobs_do_not() {
    let host = host(VectorMemory::default());
    let (mut db, mut jobs) = create(&host, &[Some(1), Some(2), Some(3)], 0);
    let handle = db
        .query::<Row>()
        .start_count(&mut jobs, time(0), time(100))
        .unwrap();
    let completed = db
        .query::<Row>()
        .start_count(&mut jobs, time(0), time(100))
        .unwrap();
    assert!(matches!(
        db.query::<Row>()
            .resume_count(&mut jobs, &completed, 0, time(1))
            .unwrap()
            .outcome,
        JobOutcome::Complete(3)
    ));
    let row = db.get::<Row>(0).unwrap().unwrap();
    let revision = db.revision::<Row>().unwrap();
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    batch.insert(&Other { id: 1 }).unwrap();
    db.commit(batch).unwrap();
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Exists)));
    assert_eq!(db.revision::<Row>().unwrap(), revision);
    assert!(matches!(
        db.query::<Row>()
            .work_limit(1)
            .resume_count(&mut jobs, &handle, 0, time(1))
            .unwrap()
            .outcome,
        JobOutcome::Running
    ));
    let mut changed = db.get::<Row>(0).unwrap().unwrap();
    changed.payload.push(9);
    let mut batch = Batch::new();
    batch.replace(&changed).unwrap();
    db.commit(batch).unwrap();
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    // Even a replay must notice changes; restoring old bytes does not restore a revision.
    assert!(matches!(
        db.query::<Row>()
            .resume_count(&mut jobs, &handle, 0, time(2))
            .unwrap()
            .outcome,
        JobOutcome::Invalidated
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_count(&mut jobs, &completed, 0, time(2))
            .unwrap()
            .outcome,
        JobOutcome::Complete(3)
    ));
}

#[test]
fn query_result_shape_and_sequence_refusals_preserve_progress() {
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (db, mut jobs) = create(&host, &[Some(1), Some(2)], 0);
    let handle = db
        .query::<Row>()
        .start_sum::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .resume_count(&mut jobs, &handle, 0, time(1)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_sum::<Id>(&mut jobs, &handle, 0, time(1)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .filter("id", Comparison::Eq, 1_u64)
            .resume_sum::<Amount>(&mut jobs, &handle, 0, time(1)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_sum::<Amount>(&mut jobs, &handle, 1, time(1)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .test_output_limit(1)
            .resume_sum::<Amount>(&mut jobs, &handle, 0, time(1)),
        Err(Error::RowTooLarge)
    ));
    assert_eq!(*backing.borrow(), before);
    let step = db
        .query::<Row>()
        .resume_sum::<Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let JobOutcome::Complete(Some(value)) = step.outcome else {
        panic!("missing complete sum")
    };
    assert_eq!(value.amount, Some(3));
    assert!(matches!(
        db.query::<Row>()
            .resume_sum::<Amount>(&mut jobs, &handle, 0, time(0)),
        Err(Error::TimestampRegression)
    ));
}

#[test]
fn overflow_refuses_finalization_without_losing_the_checkpoint() {
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (db, mut jobs) = create(&host, &[Some(i128::MAX), Some(1)], 0);
    let handle = db
        .query::<Row>()
        .start_sum::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .work_limit(1)
        .resume_sum::<Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .resume_sum::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::ArithmeticOverflow)
    ));
    assert_eq!(*backing.borrow(), before);
    let retry = db
        .query::<Row>()
        .resume_sum::<Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert_eq!(retry.examined, 1);
    assert_eq!(retry.sequence, 1);
    jobs.cancel(&handle).unwrap();
}

#[test]
fn capacity_expiry_cleanup_and_slot_generation_are_explicit() {
    let host = host(VectorMemory::default());
    let (db, mut jobs) = create(&host, &[], 0);
    assert!(matches!(
        db.query::<Row>()
            .start_count(&mut jobs, time(0), time(3_600_001)),
        Err(Error::Limit)
    ));
    let handles: Vec<_> = (0..16)
        .map(|_| {
            db.query::<Row>()
                .start_count(&mut jobs, time(0), time(10))
                .unwrap()
        })
        .collect();
    assert!(matches!(
        db.query::<Row>().start_count(&mut jobs, time(0), time(10)),
        Err(Error::JobCapacity)
    ));
    assert_eq!(jobs.cleanup_expired(time(9), 16).unwrap(), 0);
    assert!(matches!(
        db.query::<Row>()
            .resume_count(&mut jobs, &handles[0], 0, time(10))
            .unwrap()
            .outcome,
        JobOutcome::Expired
    ));
    assert_eq!(jobs.cleanup_expired(time(10), 1).unwrap(), 1);
    let replacement = db
        .query::<Row>()
        .start_count(&mut jobs, time(10), time(20))
        .unwrap();
    assert_ne!(replacement, handles[0]);
    assert!(matches!(jobs.cancel(&handles[0]), Err(Error::InvalidJob)));
    assert_eq!(jobs.cleanup_expired(time(10), 16).unwrap(), 15);
    assert!(matches!(
        db.query::<Row>()
            .resume_count(&mut jobs, &replacement, 0, time(11))
            .unwrap()
            .outcome,
        JobOutcome::Complete(0)
    ));
    jobs.cancel(&replacement).unwrap();
    assert_eq!(jobs.cleanup_expired(time(20), 16).unwrap(), 0);
    assert!(JobHandle::from_bytes(&[0; 27]).is_err());
}

#[test]
fn foreign_database_and_job_store_cannot_resume_a_handle() {
    let first_host = host(VectorMemory::default());
    let (db, mut jobs) = create(&first_host, &[Some(1)], 0);
    let handle = db
        .query::<Row>()
        .start_count(&mut jobs, time(0), time(100))
        .unwrap();
    let second_host = host(VectorMemory::default());
    let other_db = Database::create(
        second_host
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
        [9; 16],
    )
    .unwrap();
    let mut other_jobs = AggregateJobs::create(
        second_host
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
        [9; 16],
    )
    .unwrap();
    assert!(matches!(
        other_db
            .query::<Row>()
            .resume_count(&mut jobs, &handle, 0, time(1)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_count(&mut other_jobs, &handle, 0, time(1)),
        Err(Error::InvalidJob)
    ));
}

#[test]
fn previous_job_header_is_refused_without_touching_existing_jobs() {
    use ic_storage::adapter::StableBTreeMap;
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.jobs.v1").unwrap();
    let mut raw = StableBTreeMap::create(runtime.open_memory(&key).unwrap());
    let mut header = b"ICDBAJ02".to_vec();
    header.extend(1_u32.to_le_bytes());
    header.extend([1; 16]);
    header.extend(1_u64.to_le_bytes());
    raw.insert(0_u8, header);
    raw.insert(1, vec![42; 128]);
    drop(raw);
    let before = backing.borrow().clone();
    assert!(matches!(
        AggregateJobs::open(runtime.open_memory(&key).unwrap()),
        Err(Error::InvalidJob)
    ));
    assert_eq!(*backing.borrow(), before);
}

// Independent byte oracle for the fixed-slot layout, including its 36-byte header.
fn raw_frame(memory: &ic_storage::RuntimeMemory<VectorMemory>, slot: u8) -> Vec<u8> {
    if slot == 0 {
        let mut header = vec![0; 36];
        memory.read(0, &mut header);
        return header;
    }
    let index = u64::from(slot - 1);
    let mut len = [0; 2];
    memory.read(36 + 2 * index, &mut len);
    let mut bytes = vec![0; usize::from(u16::from_le_bytes(len))];
    memory.read(68 + 16384 * index, &mut bytes);
    bytes
}

fn set_raw_frame(memory: &ic_storage::RuntimeMemory<VectorMemory>, slot: u8, bytes: &[u8]) {
    if slot == 0 {
        memory.write(0, bytes);
    } else {
        let index = u64::from(slot - 1);
        memory.write(
            36 + 2 * index,
            &u16::try_from(bytes.len()).unwrap().to_le_bytes(),
        );
        memory.write(68 + 16384 * index, bytes);
    }
}

#[test]
fn invalid_slot_length_or_extent_refuses_open_intact() {
    for (slot, len) in [(1_u64, 16_385_u16), (16, 1)] {
        let backing = VectorMemory::default();
        let runtime = host(backing.clone());
        let (_, jobs) = create(&runtime, &[], 0);
        drop(jobs);
        let key = StableKey::parse("test.jobs.v1").unwrap();
        let memory = runtime.open_memory(&key).unwrap();
        memory.write(36 + 2 * (slot - 1), &len.to_le_bytes());
        drop(memory);
        let before = backing.borrow().clone();
        assert!(matches!(
            AggregateJobs::open(runtime.open_memory(&key).unwrap()),
            Err(Error::Corrupt)
        ));
        assert_eq!(*backing.borrow(), before);
    }
}

#[test]
fn new_jobs_preserve_each_initial_checkpoint_encoding() {
    let runtime = host(VectorMemory::default());
    let (db, mut jobs) = create(&runtime, &[], 0);
    let query = || db.query::<Row>();
    query().start_count(&mut jobs, time(0), time(10)).unwrap();
    query()
        .start_sum::<Amount>(&mut jobs, time(0), time(10))
        .unwrap();
    query()
        .start_min::<Amount>(&mut jobs, time(0), time(10))
        .unwrap();
    query()
        .start_count_distinct::<Id>(&mut jobs, time(0), time(10))
        .unwrap();
    query()
        .start_group_count::<Id>(&mut jobs, time(0), time(10))
        .unwrap();
    drop(jobs);

    let key = StableKey::parse("test.jobs.v1").unwrap();
    let raw = runtime.open_memory(&key).unwrap();
    // Eight fixed-width numbers, the framed 32-byte binding, phase and empty cursor
    // precede the state length. The last fixed-width number is the accumulator count.
    let state_length_offset = 8 * 8 + (4 + 32) + 1 + 4;
    for (slot, state_len) in [(1, 32_u32), (2, 32), (3, 0), (4, 1), (5, 1)] {
        let record = raw_frame(&raw, slot);
        assert_eq!(
            &record[state_length_offset - 8..state_length_offset],
            &[0; 8]
        );
        let mut expected = state_len.to_le_bytes().to_vec();
        expected.resize(4 + state_len as usize, 0);
        assert_eq!(&record[state_length_offset..], expected, "slot {slot}");
    }
}

#[test]
fn lifecycle_checks_complete_framing_without_interpreting_accumulator_bytes() {
    for corruption in 0..4 {
        let backing = VectorMemory::default();
        let runtime = host(backing.clone());
        let (db, mut jobs) = create(&runtime, &[], 0);
        db.query::<Row>()
            .start_count(&mut jobs, time(0), time(10))
            .unwrap();
        let handle = db
            .query::<Row>()
            .start_count(&mut jobs, time(0), time(10))
            .unwrap();
        drop(jobs);
        let key = StableKey::parse("test.jobs.v1").unwrap();
        let raw = runtime.open_memory(&key).unwrap();
        let mut record = raw_frame(&raw, 2);
        // Scalar count carries a final 32-byte accumulator with a u32 prefix.
        // Damage the tail while keeping the lifecycle metadata untouched.
        let payload = record.len() - 32;
        match corruption {
            0 => {
                record.pop();
            }
            1 => record.push(0),
            2 => record[payload - 4..payload].copy_from_slice(&33_u32.to_le_bytes()),
            3 => record[payload..].fill(255),
            _ => unreachable!(),
        }
        set_raw_frame(&raw, 2, &record);
        drop(raw);
        let mut jobs = AggregateJobs::open(runtime.open_memory(&key).unwrap()).unwrap();
        let before = backing.borrow().clone();
        assert!(matches!(
            db.query::<Row>()
                .resume_count(&mut jobs, &handle, 0, time(1)),
            Err(Error::Corrupt)
        ));
        assert_eq!(*backing.borrow(), before);

        if corruption == 3 {
            // Framed opaque state remains cancellable even when the reducer
            // rejects its contents. Cleanup never becomes an aggregate executor.
            jobs.cancel(&handle).unwrap();
            assert_eq!(jobs.cleanup_expired(time(10), 16).unwrap(), 1);
        } else {
            assert!(matches!(jobs.cancel(&handle), Err(Error::Corrupt)));
            assert_eq!(*backing.borrow(), before);
            assert!(matches!(
                jobs.cleanup_expired(time(10), 16),
                Err(Error::Corrupt)
            ));
            assert_eq!(
                *backing.borrow(),
                before,
                "no deletion of the earlier valid job"
            );
        }
    }
}

#[test]
fn unsupported_store_and_malformed_job_refuse_without_cleanup_damage() {
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (db, mut jobs) = create(&host, &[], 0);
    db.query::<Row>()
        .start_count(&mut jobs, time(0), time(10))
        .unwrap();
    db.query::<Row>()
        .start_count(&mut jobs, time(0), time(10))
        .unwrap();
    drop(jobs);
    let key = StableKey::parse("test.jobs.v1").unwrap();
    let raw = host.open_memory(&key).unwrap();
    set_raw_frame(&raw, 2, &[255]);
    drop(raw);
    let mut jobs = AggregateJobs::open(host.open_memory(&key).unwrap()).unwrap();
    let before = backing.borrow().clone();
    assert!(jobs.cleanup_expired(time(10), 16).is_err());
    assert_eq!(
        *backing.borrow(),
        before,
        "no early removal before checking remaining records"
    );
    drop(jobs);
    let raw = host.open_memory(&key).unwrap();
    let mut header = raw_frame(&raw, 0);
    header[0] ^= 1;
    set_raw_frame(&raw, 0, &header);
    drop(raw);
    let before = backing.borrow().clone();
    assert!(AggregateJobs::open(host.open_memory(&key).unwrap()).is_err());
    assert_eq!(*backing.borrow(), before);
}

///
/// NumericRow
/// Stores `price` and `wide` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "price", value(item(prim = "Decimal", scale = 2))),
        field(name = "wide", value(item(prim = "U256")))
    )
)]
struct NumericRow {}

///
/// Price
/// Carries the `Decimal` value of `price`.
///

#[record(fields(field(name = "price", value(item(prim = "Decimal", scale = 2)))))]
struct Price {}

///
/// WrongScale
/// Carries the `Decimal` value of `price`.
///

#[record(fields(field(name = "price", value(item(prim = "Decimal", scale = 3)))))]
struct WrongScale {}

///
/// Wide
/// Carries the `U256` value of `wide`.
///

#[record(fields(field(name = "wide", value(item(prim = "U256")))))]
struct Wide {}
#[test]
fn decimal_coefficients_and_full_width_unsigned_totals_survive_checkpoints() {
    use ic_db::{Decimal, U256};
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (mut db, mut jobs) = create_stores(&host, Schema::new(&[NumericRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    for (i, coefficient) in [i128::MAX, i128::MAX, -i128::MAX].into_iter().enumerate() {
        batch
            .insert(&NumericRow {
                id: i as u64,
                price: Decimal::try_new(coefficient, 2).unwrap(),
                wide: if i == 0 { U256::MAX } else { U256::ZERO },
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let decimal = db
        .query::<NumericRow>()
        .start_sum::<Price>(&mut jobs, time(0), time(100))
        .unwrap();
    let unsigned = db
        .query::<NumericRow>()
        .start_sum::<Wide>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<NumericRow>()
        .work_limit(2)
        .resume_sum::<Price>(&mut jobs, &decimal, 0, time(1))
        .unwrap();
    db.query::<NumericRow>()
        .work_limit(2)
        .resume_sum::<Wide>(&mut jobs, &unsigned, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<NumericRow>()
            .resume_sum::<WrongScale>(&mut jobs, &decimal, 1, time(2)),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(*backing.borrow(), before);
    let JobOutcome::Complete(Some(value)) = db
        .query::<NumericRow>()
        .resume_sum::<Price>(&mut jobs, &decimal, 1, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing Decimal")
    };
    assert_eq!(value.price.coefficient_at_scale(2), Some(i128::MAX));
    let JobOutcome::Complete(Some(value)) = db
        .query::<NumericRow>()
        .resume_sum::<Wide>(&mut jobs, &unsigned, 1, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing U256")
    };
    assert_eq!(value.wide, U256::MAX);
}

///
/// Average0
/// Carries an optional `Decimal` value of `amount`.
///

#[record(fields(field(name = "amount", value(opt, item(prim = "Decimal", scale = 0)))))]
struct Average0 {}

///
/// Average2
/// Carries an optional `Decimal` value of `amount`.
///

#[record(fields(field(name = "amount", value(opt, item(prim = "Decimal", scale = 2)))))]
struct Average2 {}

///
/// Average28
/// Carries an optional `Decimal` value of `amount`.
///

#[record(fields(field(name = "amount", value(opt, item(prim = "Decimal", scale = 28)))))]
struct Average28 {}

///
/// WideAverage
/// Carries the `Decimal` value of `wide`.
///

#[record(fields(field(name = "wide", value(item(prim = "Decimal", scale = 0)))))]
struct WideAverage {}

#[test]
fn averages_round_once_across_unequal_chunks_and_ignore_absence() {
    for (values, expected) in [
        (vec![], None),
        (vec![None, None], None),
        (vec![Some(0), None], Some(0)),
        (vec![Some(0), None, Some(1)], Some(0)),
        (vec![Some(1), Some(2), None], Some(2)),
        (vec![Some(-1), Some(-2), None], Some(-2)),
        (vec![Some(0), Some(-1), None], Some(0)),
        (vec![Some(1), None, Some(2), Some(2)], Some(2)),
    ] {
        let host = host(VectorMemory::default());
        let (db, mut jobs) = create(&host, &values, 0);
        for work in [1, 2, 3] {
            let handle = db
                .query::<Row>()
                .start_avg::<Average0>(&mut jobs, time(0), time(100))
                .unwrap();
            let mut sequence = 0;
            loop {
                let step = db
                    .query::<Row>()
                    .work_limit(work)
                    .resume_avg::<Average0>(&mut jobs, &handle, sequence, time(1))
                    .unwrap();

                match step.outcome {
                    JobOutcome::Running => {
                        assert_eq!(step.output_bytes, 0);
                        sequence = step.sequence;
                    }
                    JobOutcome::Complete(value) => {
                        assert_eq!(
                            value
                                .and_then(|r| r.amount)
                                .map(|n| n.coefficient_at_scale(0).unwrap()),
                            expected
                        );
                        break;
                    }
                    _ => panic!("unexpected terminal state"),
                }
            }
            jobs.cancel(&handle).unwrap();
        }
    }
    // 1803 / 1001 = 1.801198..., irrespective of page size or absent rows.
    let host = host(VectorMemory::default());
    let mut values = vec![Some(1); 600];
    values.extend(vec![Some(3); 401]);
    values.extend(vec![None; 27]);
    let (db, mut jobs) = create(&host, &values, 0);
    for work in [7, 128, 511] {
        let handle = db
            .query::<Row>()
            .start_avg::<Average2>(&mut jobs, time(0), time(100))
            .unwrap();
        let mut sequence = 0;
        loop {
            let step = db
                .query::<Row>()
                .work_limit(work)
                .resume_avg::<Average2>(&mut jobs, &handle, sequence, time(1))
                .unwrap();
            let retry = db
                .query::<Row>()
                .work_limit(512)
                .resume_avg::<Average2>(&mut jobs, &handle, sequence, time(1))
                .unwrap();
            assert_eq!(
                (retry.sequence, retry.examined),
                (step.sequence, step.examined)
            );
            sequence = step.sequence;
            if let JobOutcome::Complete(Some(value)) = step.outcome {
                assert_eq!(value.amount.unwrap().coefficient_at_scale(2), Some(180));
                assert_eq!(step.examined, 1028);
                break;
            }
            assert!(matches!(step.outcome, JobOutcome::Running));
        }
        jobs.cancel(&handle).unwrap();
    }
}

#[test]
fn average_reopen_preserves_large_canceling_total_and_denominator() {
    let backing = VectorMemory::default();
    let handle;
    {
        let host = host(backing.clone());
        let mut values = vec![Some(i128::MAX); 512];
        values.extend(vec![Some(-i128::MAX); 512]);
        values.push(Some(1025));
        let (db, mut jobs) = create(&host, &values, 0);
        handle = db
            .query::<Row>()
            .start_avg::<Average28>(&mut jobs, time(0), time(100))
            .unwrap();
        let step = db
            .query::<Row>()
            .work_limit(512)
            .resume_avg::<Average28>(&mut jobs, &handle, 0, time(1))
            .unwrap();
        assert!(matches!(step.outcome, JobOutcome::Running));
        assert_eq!(step.examined, 512);
    }
    let host = host(backing);
    let db = Database::open(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        host.open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    db.query::<Row>()
        .work_limit(512)
        .resume_avg::<Average28>(&mut jobs, &handle, 1, time(2))
        .unwrap();
    let step = db
        .query::<Row>()
        .resume_avg::<Average28>(&mut jobs, &handle, 2, time(3))
        .unwrap();
    let JobOutcome::Complete(Some(value)) = step.outcome else {
        panic!("missing completed average")
    };
    assert_eq!(
        value.amount.unwrap().coefficient_at_scale(28),
        Some(10_i128.pow(28))
    );
}

#[test]
fn average_operation_and_scale_bind_without_changing_decimal_sum_semantics() {
    use ic_db::{Decimal, U256};
    for (left, right, expected) in [
        (124, 125, 124),
        (125, 126, 126),
        (-124, -125, -124),
        (-125, -126, -126),
    ] {
        let backing = VectorMemory::default();
        let host = host(backing.clone());
        let (mut db, mut jobs) = create_stores(&host, Schema::new(&[NumericRow::SCHEMA]).unwrap());
        let mut batch = Batch::new();
        for (id, n) in [left, right].into_iter().enumerate() {
            batch
                .insert(&NumericRow {
                    id: id as u64,
                    price: Decimal::try_new(n, 2).unwrap(),
                    wide: U256::ZERO,
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
        let avg = db
            .query::<NumericRow>()
            .start_avg::<Price>(&mut jobs, time(0), time(100))
            .unwrap();
        let sum = db
            .query::<NumericRow>()
            .start_sum::<Price>(&mut jobs, time(0), time(100))
            .unwrap();
        let before = backing.borrow().clone();
        assert!(matches!(
            db.query::<NumericRow>()
                .resume_sum::<Price>(&mut jobs, &avg, 0, time(1)),
            Err(Error::InvalidJob)
        ));
        assert!(matches!(
            db.query::<NumericRow>()
                .resume_avg::<Price>(&mut jobs, &sum, 0, time(1)),
            Err(Error::InvalidJob)
        ));
        assert!(matches!(
            db.query::<NumericRow>()
                .resume_avg::<WrongScale>(&mut jobs, &avg, 0, time(1)),
            Err(Error::InvalidJob)
        ));
        assert_eq!(*backing.borrow(), before);
        db.query::<NumericRow>()
            .work_limit(1)
            .resume_avg::<Price>(&mut jobs, &avg, 0, time(1))
            .unwrap();
        let JobOutcome::Complete(Some(value)) = db
            .query::<NumericRow>()
            .resume_avg::<Price>(&mut jobs, &avg, 1, time(2))
            .unwrap()
            .outcome
        else {
            panic!("missing average")
        };
        assert_eq!(value.price.coefficient_at_scale(2), Some(expected));
        let JobOutcome::Complete(Some(value)) = db
            .query::<NumericRow>()
            .resume_sum::<Price>(&mut jobs, &sum, 0, time(2))
            .unwrap()
            .outcome
        else {
            panic!("missing sum")
        };
        assert_eq!(value.price.coefficient_at_scale(2), Some(left + right));
    }
}

#[test]
fn average_finalization_errors_preserve_exact_checkpoint_without_scale_fallback() {
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (db, mut jobs) = create(&host, &[Some(i128::MAX), Some(i128::MAX)], 0);
    let avg = db
        .query::<Row>()
        .start_avg::<Average0>(&mut jobs, time(0), time(100))
        .unwrap();
    let overflow = db
        .query::<Row>()
        .start_avg::<Average28>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .work_limit(1)
        .resume_avg::<Average0>(&mut jobs, &avg, 0, time(1))
        .unwrap();
    db.query::<Row>()
        .work_limit(1)
        .resume_avg::<Average28>(&mut jobs, &overflow, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .test_output_limit(1)
            .resume_avg::<Average0>(&mut jobs, &avg, 1, time(2)),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_avg::<Average28>(&mut jobs, &overflow, 1, time(2)),
        Err(Error::ArithmeticOverflow)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_avg::<Average0>(&mut jobs, &overflow, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert_eq!(*backing.borrow(), before);
    let retry = db
        .query::<Row>()
        .resume_avg::<Average28>(&mut jobs, &overflow, 0, time(1))
        .unwrap();
    assert_eq!((retry.sequence, retry.examined), (1, 1));
    let JobOutcome::Complete(Some(value)) = db
        .query::<Row>()
        .resume_avg::<Average0>(&mut jobs, &avg, 1, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing average")
    };
    assert_eq!(
        value.amount.unwrap().coefficient_at_scale(0),
        Some(i128::MAX)
    );
}

#[test]
fn unsigned_average_checks_result_after_division_across_messages() {
    use ic_db::{Decimal, U256};
    for (value, expected) in [
        (U256::from(1_u128 << 127), Some(1_i128 << 126)),
        (U256::ONE << 255, None),
    ] {
        let backing = VectorMemory::default();
        let host = host(backing.clone());
        let (mut db, mut jobs) = create_stores(&host, Schema::new(&[NumericRow::SCHEMA]).unwrap());
        let mut batch = Batch::new();
        for (id, wide) in [value, U256::ZERO].into_iter().enumerate() {
            batch
                .insert(&NumericRow {
                    id: id as u64,
                    price: Decimal::ZERO,
                    wide,
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
        let handle = db
            .query::<NumericRow>()
            .start_avg::<WideAverage>(&mut jobs, time(0), time(100))
            .unwrap();
        db.query::<NumericRow>()
            .work_limit(1)
            .resume_avg::<WideAverage>(&mut jobs, &handle, 0, time(1))
            .unwrap();
        let before = backing.borrow().clone();
        let result =
            db.query::<NumericRow>()
                .resume_avg::<WideAverage>(&mut jobs, &handle, 1, time(2));

        if let Some(expected) = expected {
            let JobOutcome::Complete(Some(value)) = result.unwrap().outcome else {
                panic!("missing average")
            };
            assert_eq!(value.wide.coefficient_at_scale(0), Some(expected));
        } else {
            assert!(matches!(result, Err(Error::ArithmeticOverflow)));
            assert_eq!(*backing.borrow(), before);
        }
    }
}

///
/// Flag
/// Carries the `Nat8` value of `flag`.
///

#[record(fields(field(name = "flag", value(item(prim = "Nat8")))))]
struct Flag {}

///
/// GroupKey
/// Groups `amount` and `flag` into one record.
///

#[record(fields(
    field(name = "amount", value(opt, item(prim = "Int128"))),
    field(name = "flag", value(item(prim = "Nat8")))
))]
struct GroupKey {}

#[test]
fn descending_group_keys_select_and_page_after_complete_accumulation() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(
        &runtime,
        &[Some(1), None, Some(2), Some(1), None, Some(2)],
        0,
    );
    let expected = vec![
        (Some(2), 1),
        (Some(2), 0),
        (Some(1), 1),
        (Some(1), 0),
        (None, 1),
    ];
    let query = || {
        db.query::<Row>()
            .filter_groups(Comparison::Ge, 1_u64)
            .order_groups(GroupOrder::KeyDescending)
            .limit(5)
    };
    let complete = query().group_count::<GroupKey>().unwrap();
    assert_eq!(
        complete
            .value
            .iter()
            .map(|r| (r.key.amount, r.key.flag))
            .collect::<Vec<_>>(),
        expected
    );
    assert!(complete.value.iter().all(|r| r.count == 1));
    // Six qualifying keys exercise both a full wrap and a partially overwritten buffer.
    for limit in 1..5 {
        let rows = query()
            .limit(limit)
            .group_count::<GroupKey>()
            .unwrap()
            .value;
        assert_eq!(
            rows.iter()
                .map(|r| (r.key.amount, r.key.flag))
                .collect::<Vec<_>>(),
            expected[..limit]
        );
    }
    let ascending = query()
        .order_groups(GroupOrder::Key)
        .group_count::<GroupKey>()
        .unwrap();
    assert_eq!(
        (ascending.value[0].key.amount, ascending.value[0].key.flag),
        (None, 0)
    );
    let handle = query()
        .start_group_count::<GroupKey>(&mut jobs, time(0), time(100))
        .unwrap();
    let first = query()
        .work_limit(2)
        .resume_group_count::<GroupKey>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    assert_eq!(first.output_bytes, 0);
    drop((db, jobs, runtime));
    let runtime = host(backing.clone());
    let db = Database::open(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let query = || {
        db.query::<Row>()
            .filter_groups(Comparison::Ge, 1_u64)
            .order_groups(GroupOrder::KeyDescending)
            .limit(5)
    };
    let before = backing.borrow().clone();
    for order in [
        GroupOrder::Key,
        GroupOrder::AggregateAscending,
        GroupOrder::AggregateDescending,
    ] {
        assert!(matches!(
            query().order_groups(order).resume_group_count::<GroupKey>(
                &mut jobs,
                &handle,
                first.sequence,
                time(2)
            ),
            Err(Error::InvalidJob)
        ));
        assert_eq!(*backing.borrow(), before);
    }
    let mut sequence = first.sequence;
    let mut cursor = None;
    let mut actual = Vec::new();
    for _ in 0..8 {
        let step = query()
            .work_limit(2)
            .group_page(2, cursor.as_ref())
            .resume_group_count::<GroupKey>(&mut jobs, &handle, sequence, time(2))
            .unwrap();
        sequence = step.sequence;

        match step.outcome {
            JobOutcome::Running => assert_eq!(step.output_bytes, 0),
            JobOutcome::Complete(page) => {
                assert_eq!(step.examined, 6);
                actual.extend(page.rows.into_iter().map(|r| (r.key.amount, r.key.flag)));
                cursor = page.next;
                if cursor.is_none() {
                    break;
                }
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
    assert_eq!(actual, expected);
    assert!(cursor.is_none());
}

#[test]
fn retained_distinct_scans_once_and_pages_all_keys_after_reopen() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    // A stride coprime to 255 interleaves all classes, including absence.
    let values: Vec<_> = (0..521)
        .map(|i| {
            let key = (i * 19) % 255;
            (key != 0).then_some(key)
        })
        .collect();
    let expected: Vec<_> = values
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(expected.len(), 255);
    let (db, mut jobs) = create(&runtime, &values, 0);
    let handle = db
        .query::<Row>()
        .start_distinct::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let first = db
        .query::<Row>()
        .work_limit(29)
        .resume_distinct::<Amount>(&mut jobs, &handle, 0, time(1), None)
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    assert_eq!(first.examined, 29);
    drop((db, jobs, runtime));
    let runtime = host(backing);
    let db = Database::open(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        db.query::<Row>()
            .resume_group_count::<Amount>(&mut jobs, &handle, first.sequence, time(1)),
        Err(Error::InvalidJob)
    ));
    let mut sequence = first.sequence;
    let mut cursor = None;
    let mut actual = Vec::new();
    for _ in 0..64 {
        let page_size = if cursor.is_some() { 17 } else { 5 };
        let step = db
            .query::<Row>()
            .work_limit(37)
            .limit(page_size)
            .resume_distinct::<Amount>(&mut jobs, &handle, sequence, time(1), cursor.as_ref())
            .unwrap();
        sequence = step.sequence;
        match step.outcome {
            JobOutcome::Running => assert_eq!(step.output_bytes, 0),
            JobOutcome::Complete(page) => {
                assert_eq!(step.examined, 521);
                assert!(page.rows.len() <= page_size);
                let size: usize = page
                    .rows
                    .iter()
                    .map(|r| if r.amount.is_some() { 17 } else { 1 })
                    .sum();
                assert_eq!(step.output_bytes, size);
                actual.extend(page.rows.into_iter().map(|row| row.amount));
                let Some(next) = page.next else { break };
                cursor = Some(ic_db::GroupCursor::from_bytes(&next.to_bytes()).unwrap());
            }
            other => panic!("unexpected job outcome: {other:?}"),
        }
    }
    assert_eq!(actual, expected);
}

#[test]
fn retained_distinct_preserves_first_index_occurrence_and_rejects_duplicate_checkpoint_keys() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(&runtime, &[Some(8), Some(9), None, Some(9)], 0);
    let query = || {
        db.query::<Row>()
            .using_index("flag", &[])
            .order_groups(GroupOrder::FirstOccurrence)
    };
    let handle = query()
        .start_distinct::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let first = query()
        .work_limit(2)
        .resume_distinct::<Amount>(&mut jobs, &handle, 0, time(1), None)
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    assert!(matches!(
        query()
            .order_groups(GroupOrder::Key)
            .resume_distinct::<Amount>(&mut jobs, &handle, first.sequence, time(1), None),
        Err(Error::InvalidJob)
    ));
    let step = query()
        .resume_distinct::<Amount>(&mut jobs, &handle, first.sequence, time(1), None)
        .unwrap();
    let JobOutcome::Complete(page) = step.outcome else {
        panic!("expected completed DISTINCT keys")
    };
    assert_eq!(
        page.rows.iter().map(|r| r.amount).collect::<Vec<_>>(),
        [Some(8), None, Some(9)]
    );
    drop(jobs);

    let key = StableKey::parse("test.jobs.v1").unwrap();
    let raw = runtime.open_memory(&key).unwrap();
    let mut record = raw_frame(&raw, 1);
    // The last projected Int128 is present. Turn it into a duplicate of the first.
    let tail = record.len() - 16;
    assert_eq!(&record[tail..], &9_i128.to_le_bytes());
    record[tail..].copy_from_slice(&8_i128.to_le_bytes());
    set_raw_frame(&raw, 1, &record);
    drop(raw);
    let mut jobs = AggregateJobs::open(runtime.open_memory(&key).unwrap()).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        query().resume_distinct::<Amount>(&mut jobs, &handle, step.sequence, time(1), None),
        Err(Error::Corrupt)
    ));
    assert_eq!(*backing.borrow(), before);
    jobs.cancel(&handle).unwrap();
}

#[test]
fn first_occurrence_selects_an_ordered_index_and_binds_its_direction() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(&runtime, &[Some(8), Some(9), None, Some(9)], 0);
    let query = || db.query::<Row>().order_groups(GroupOrder::FirstOccurrence);
    let before = backing.borrow().clone();
    // An amount order would require materializing and sorting the input.
    assert!(matches!(
        query()
            .order_by(&[Row::AMOUNT])
            .start_distinct::<Amount>(&mut jobs, time(0), time(100)),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Row>()
            .order_by(&[Row::FLAG])
            .start_distinct::<Amount>(&mut jobs, time(0), time(100)),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(*backing.borrow(), before);

    let by_flag = || query().order_by(&[Row::FLAG]);
    let handle = by_flag()
        .descending()
        .start_distinct::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let first = by_flag()
        .descending()
        .work_limit(1)
        .resume_distinct::<Amount>(&mut jobs, &handle, 0, time(1), None)
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    let before = backing.borrow().clone();
    assert!(matches!(
        by_flag().resume_distinct::<Amount>(&mut jobs, &handle, first.sequence, time(1), None),
        Err(Error::InvalidJob)
    ));
    assert_eq!(*backing.borrow(), before);
    let done = by_flag()
        .descending()
        .resume_distinct::<Amount>(&mut jobs, &handle, first.sequence, time(1), None)
        .unwrap();
    let JobOutcome::Complete(page) = done.outcome else {
        panic!("expected completed keys")
    };
    // Reverse (flag, id) visits IDs 3, 1, 2, 0. IDs 3 and 1 share amount 9.
    assert_eq!(
        page.rows.iter().map(|r| r.amount).collect::<Vec<_>>(),
        [Some(9), None, Some(8)]
    );
    assert_eq!(done.examined, 4);
}

#[test]
fn retained_distinct_pages_descending_keys_with_their_own_byte_sizes() {
    use ic_db::GroupOrder;

    let runtime = host(VectorMemory::default());
    let (db, mut jobs) = create(&runtime, &[None, Some(9), Some(2), Some(9)], 0);
    let query = || db.query::<Row>().order_groups(GroupOrder::KeyDescending);
    let handle = query()
        .start_distinct::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    assert!(matches!(
        db.query::<Row>()
            .resume_distinct::<Amount>(&mut jobs, &handle, 0, time(1), None),
        Err(Error::InvalidJob)
    ));
    let page = query()
        .test_output_limit(17)
        .resume_distinct::<Amount>(&mut jobs, &handle, 0, time(1), None)
        .unwrap();
    let JobOutcome::Complete(first) = page.outcome else {
        panic!("expected completed keys")
    };
    assert_eq!(page.output_bytes, 17);
    assert_eq!(
        first.rows.iter().map(|r| r.amount).collect::<Vec<_>>(),
        [Some(9)]
    );
    let cursor = first.next.unwrap();
    assert!(matches!(
        query().test_output_limit(1).resume_distinct::<Amount>(
            &mut jobs,
            &handle,
            page.sequence,
            time(1),
            Some(&cursor)
        ),
        Err(Error::RowTooLarge)
    ));
    let rest = query()
        .test_output_limit(18)
        .resume_distinct::<Amount>(&mut jobs, &handle, page.sequence, time(1), Some(&cursor))
        .unwrap();
    let JobOutcome::Complete(last) = rest.outcome else {
        panic!("expected remaining keys")
    };
    assert_eq!(
        last.rows.iter().map(|r| r.amount).collect::<Vec<_>>(),
        [Some(2), None]
    );
    assert_eq!(rest.output_bytes, 18);
    assert!(last.next.is_none());
}

#[test]
fn retained_distinct_frame_refusal_preserves_checkpoint_and_cleanup() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (mut db, mut jobs) = create_stores(&runtime, schema());
    let mut batch = Batch::new();
    for id in 1..=6 {
        batch
            .insert(&Row {
                id,
                flag: 0,
                amount: None,
                payload: vec![id as u8; 3400],
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let handle = db
        .query::<Row>()
        .start_distinct::<Payload>(&mut jobs, time(0), time(100))
        .unwrap();
    let step = db
        .query::<Row>()
        .work_limit(4)
        .resume_distinct::<Payload>(&mut jobs, &handle, 0, time(1), None)
        .unwrap();
    assert_eq!(step.examined, 4);
    assert!(matches!(step.outcome, JobOutcome::Running));
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>().work_limit(1).resume_distinct::<Payload>(
            &mut jobs,
            &handle,
            step.sequence,
            time(1),
            None
        ),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    jobs.cancel(&handle).unwrap();
    let smaller = || db.query::<Row>().lte(Row::ID, 4_u64);
    let handle = smaller()
        .start_distinct::<Payload>(&mut jobs, time(1), time(100))
        .unwrap();
    let step = smaller()
        .limit(2)
        .test_output_limit(4000)
        .resume_distinct::<Payload>(&mut jobs, &handle, 0, time(2), None)
        .unwrap();
    let JobOutcome::Complete(page) = step.outcome else {
        panic!("expected completed page")
    };
    assert_eq!(page.rows[0].payload, vec![1; 3400]);
    assert_eq!(page.rows.len(), 1);
    let after = page.next.unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        smaller()
            .test_output_limit(3000)
            .resume_distinct::<Payload>(&mut jobs, &handle, step.sequence, time(3), Some(&after)),
        Err(Error::RowTooLarge)
    ));
    assert_eq!(*backing.borrow(), before);
    let retry = smaller()
        .limit(2)
        .resume_distinct::<Payload>(&mut jobs, &handle, step.sequence, time(3), Some(&after))
        .unwrap();
    let JobOutcome::Complete(page) = retry.outcome else {
        panic!("expected retry to complete")
    };
    assert_eq!(
        page.rows.iter().map(|r| r.payload[0]).collect::<Vec<_>>(),
        [2, 3]
    );
}

#[test]
fn retained_distinct_refuses_the_256th_key_without_advancing() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let values: Vec<_> = (0..256).map(Some).collect();
    let (mut db, mut jobs) = create(&runtime, &values, 0);
    let handle = db
        .query::<Row>()
        .start_distinct::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let mut sequence = 0;
    loop {
        let before = backing.borrow().clone();
        let result = db.query::<Row>().work_limit(32).resume_distinct::<Amount>(
            &mut jobs,
            &handle,
            sequence,
            time(1),
            None,
        );
        match result {
            Ok(step) => {
                assert!(matches!(step.outcome, JobOutcome::Running));
                assert!(step.sequence > sequence);
                sequence = step.sequence;
            }
            Err(Error::Limit) => {
                assert_eq!(*backing.borrow(), before);
                break;
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }
    let mut batch = Batch::new();
    batch.delete::<Row>(255).unwrap();
    db.commit(batch).unwrap();
    let step = db
        .query::<Row>()
        .resume_distinct::<Amount>(&mut jobs, &handle, sequence, time(2), None)
        .unwrap();
    assert!(matches!(step.outcome, JobOutcome::Invalidated));
    jobs.cancel(&handle).unwrap();
}

#[test]
fn scattered_distinct_keys_page_after_scanning_more_than_one_call() {
    let runtime = host(VectorMemory::default());
    // This fixture's domain is exactly 0..64, with no absent amounts.
    let values: Vec<_> = (0..521).map(|i| Some(i % 64)).collect();
    let (db, mut jobs) = create(&runtime, &values, 0);
    assert!(matches!(
        db.query::<Row>()
            .distinct()
            .work_limit(512)
            .fetch_as::<Amount>(),
        Err(Error::Limit)
    ));
    let revision = db.revision::<Row>().unwrap();
    let mut actual = Vec::new();
    let mut examined = 0;
    for lower in [0_i128, 32] {
        let query = || {
            db.query::<Row>()
                .filter_between(Row::AMOUNT, lower, lower + 31)
        };
        let handle = query()
            .start_group_count::<Amount>(&mut jobs, time(0), time(100))
            .unwrap();
        let mut sequence = 0;
        let mut cursor = None;
        for _ in 0..32 {
            let step = query()
                .work_limit(37)
                .group_page(5, cursor.as_ref())
                .resume_group_count::<Amount>(&mut jobs, &handle, sequence, time(1))
                .unwrap();
            assert_eq!(step.source_revision, revision);
            sequence = step.sequence;
            match step.outcome {
                JobOutcome::Running => assert_eq!(step.output_bytes, 0),
                JobOutcome::Complete(page) => {
                    assert!(page.rows.len() <= 5);
                    actual.extend(page.rows.into_iter().map(|row| row.key.amount));
                    cursor = page.next;
                    if cursor.is_none() {
                        examined += step.examined;
                        break;
                    }
                }
                other => panic!("unexpected job outcome: {other:?}"),
            }
        }
        jobs.cancel(&handle).unwrap();
    }
    assert_eq!(actual, (0..64).map(Some).collect::<Vec<_>>());
    // No amount index: each range scans the entire source exactly once.
    assert_eq!(examined, 2 * 521);
}

#[test]
fn grouped_jobs_count_complete_input_before_filtering_order_and_limit() {
    use ic_db::GroupOrder;
    use std::collections::BTreeMap;
    let values: Vec<_> = (0..521)
        .map(|i| if i % 7 == 0 { None } else { Some(i % 3) })
        .collect();
    let mut oracle = BTreeMap::new();
    for (i, amount) in values.iter().enumerate() {
        *oracle.entry((*amount, (i % 2) as u8)).or_insert(0_u64) += 1;
    }
    let mut expected: Vec<_> = oracle
        .into_iter()
        .filter(|(_, count)| *count >= 60)
        .collect();
    expected.sort_by(|(ka, a), (kb, b)| b.cmp(a).then_with(|| ka.cmp(kb)));
    expected.truncate(2);
    let host = host(VectorMemory::default());
    let (db, mut jobs) = create(&host, &values, 0);
    for work in [17, 128] {
        let query = || {
            db.query::<Row>()
                .filter_groups(Comparison::Ge, 60_u64)
                .order_groups(GroupOrder::AggregateDescending)
                .limit(2)
        };
        let handle = query()
            .start_group_count::<GroupKey>(&mut jobs, time(0), time(100))
            .unwrap();
        let mut sequence = 0;
        loop {
            let step = query()
                .work_limit(work)
                .resume_group_count::<GroupKey>(&mut jobs, &handle, sequence, time(1))
                .unwrap();
            let retry = query()
                .work_limit(1)
                .resume_group_count::<GroupKey>(&mut jobs, &handle, sequence, time(1))
                .unwrap();
            assert_eq!(
                (retry.sequence, retry.examined),
                (step.sequence, step.examined)
            );
            sequence = step.sequence;

            match step.outcome {
                JobOutcome::Running => assert_eq!(step.output_bytes, 0),
                JobOutcome::Complete(rows) => {
                    assert_eq!(step.examined, 521);
                    assert_eq!(
                        rows.rows
                            .into_iter()
                            .map(|r| ((r.key.amount, r.key.flag), r.count))
                            .collect::<Vec<_>>(),
                        expected
                    );
                    break;
                }
                _ => panic!("unexpected terminal state"),
            }
        }
        jobs.cancel(&handle).unwrap();
    }
}

#[test]
fn grouped_jobs_reopen_optional_keys_and_preserve_finished_boundary() {
    let backing = VectorMemory::default();
    let handle;
    {
        let host = host(backing.clone());
        let (db, mut jobs) = create(&host, &[Some(1), None, Some(1), Some(2)], 0);
        handle = db
            .query::<Row>()
            .start_group_count::<Amount>(&mut jobs, time(0), time(100))
            .unwrap();
        let step = db
            .query::<Row>()
            .work_limit(2)
            .resume_group_count::<Amount>(&mut jobs, &handle, 0, time(1))
            .unwrap();
        assert!(matches!(step.outcome, JobOutcome::Running));
    }
    let host = host(backing);
    let mut db = Database::open(
        host.open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        host.open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let step = db
        .query::<Row>()
        .resume_group_count::<Amount>(&mut jobs, &handle, 1, time(2))
        .unwrap();
    let JobOutcome::Complete(rows) = step.outcome else {
        panic!("missing groups")
    };
    assert_eq!(
        rows.rows
            .into_iter()
            .map(|r| (r.key.amount, r.count))
            .collect::<Vec<_>>(),
        vec![(None, 1), (Some(1), 2), (Some(2), 1)]
    );
    let pending = db
        .query::<Row>()
        .start_group_count::<Amount>(&mut jobs, time(2), time(100))
        .unwrap();
    let mut row = db.get::<Row>(0).unwrap().unwrap();
    row.amount = Some(9);
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<Row>()
            .resume_group_count::<Amount>(&mut jobs, &pending, 0, time(3))
            .unwrap()
            .outcome,
        JobOutcome::Invalidated
    ));
    let JobOutcome::Complete(rows) = db
        .query::<Row>()
        .resume_group_count::<Amount>(&mut jobs, &handle, 0, time(3))
        .unwrap()
        .outcome
    else {
        panic!("lost completed groups")
    };
    assert_eq!(
        rows.rows
            .into_iter()
            .map(|r| (r.key.amount, r.count))
            .collect::<Vec<_>>(),
        vec![(None, 1), (Some(1), 2), (Some(2), 1)]
    );
    assert!(matches!(
        db.query::<Row>()
            .resume_group_count::<Amount>(&mut jobs, &handle, 0, time(100))
            .unwrap()
            .outcome,
        JobOutcome::Expired
    ));
}

#[test]
fn grouped_job_bindings_and_output_refusals_leave_the_checkpoint_intact() {
    use ic_db::GroupOrder;
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (db, mut jobs) = create(&host, &[Some(1), Some(2), Some(3)], 0);
    let handle = db
        .query::<Row>()
        .start_group_count::<Flag>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .work_limit(2)
        .resume_group_count::<Flag>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .limit(1)
            .resume_group_count::<Flag>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .order_groups(GroupOrder::AggregateDescending)
            .resume_group_count::<Flag>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .filter_groups(Comparison::Ge, 1_u64)
            .resume_group_count::<Flag>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_count::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_count(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .test_output_limit(8)
            .resume_group_count::<Flag>(&mut jobs, &handle, 1, time(2)),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Row>()
            .test_output_limit(9)
            .resume_group_count::<Flag>(&mut jobs, &handle, 1, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    let JobOutcome::Complete(rows) = db
        .query::<Row>()
        .resume_group_count::<Flag>(&mut jobs, &handle, 1, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing groups")
    };
    assert_eq!(
        rows.rows
            .into_iter()
            .map(|r| (r.key.flag, r.count))
            .collect::<Vec<_>>(),
        vec![(0, 2), (1, 1)]
    );
}

#[test]
fn grouped_job_cardinality_refuses_even_when_output_filters_hide_every_group() {
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let values: Vec<_> = (0..33).map(Some).collect();
    let (db, mut jobs) = create(&host, &values, 0);
    let query = || {
        db.query::<Row>()
            .filter_groups(Comparison::Gt, u64::MAX)
            .limit(1)
    };
    let handle = query()
        .start_group_count::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let step = query()
        .work_limit(32)
        .resume_group_count::<Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert!(matches!(step.outcome, JobOutcome::Running));
    let before = backing.borrow().clone();
    assert!(matches!(
        query().resume_group_count::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    let retry = query()
        .resume_group_count::<Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert_eq!((retry.sequence, retry.examined), (1, 32));
}

///
/// LabelRow
/// Stores `label` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "label", value(item(prim = "Text", max_bytes = 1500)))
    )
)]
struct LabelRow {}

///
/// Label
/// Carries the `Text` value of `label`.
///

#[record(fields(field(name = "label", value(item(prim = "Text", max_bytes = 1500)))))]
struct Label {}
#[test]
fn grouped_job_frame_bound_is_independent_of_output_and_synchronous_state_limits() {
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (mut db, mut jobs) = create_stores(&host, Schema::new(&[LabelRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    for (id, c) in ('a'..='l').enumerate() {
        batch
            .insert(&LabelRow {
                id: id as u64,
                label: c.to_string().repeat(1400),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    assert_eq!(
        db.query::<LabelRow>()
            .group_count::<Label>()
            .unwrap()
            .value
            .len(),
        12
    );
    let query = || db.query::<LabelRow>().limit(1);
    let handle = query()
        .start_group_count::<Label>(&mut jobs, time(0), time(100))
        .unwrap();
    for sequence in 0..11 {
        query()
            .work_limit(1)
            .resume_group_count::<Label>(&mut jobs, &handle, sequence, time(1))
            .unwrap();
    }
    let before = backing.borrow().clone();
    assert!(matches!(
        query().resume_group_count::<Label>(&mut jobs, &handle, 11, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    let retry = query()
        .resume_group_count::<Label>(&mut jobs, &handle, 10, time(1))
        .unwrap();
    assert_eq!((retry.sequence, retry.examined), (11, 11));
    jobs.cancel(&handle).unwrap();
}

#[test]
fn grouped_jobs_handle_empty_sparse_and_corrupt_checkpoints() {
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let amounts = std::array::from_fn::<_, 8, _>(|id| Some(id as i128));
    let (db, mut jobs) = create(&host, &amounts, 0);
    let query = || {
        db.query::<Row>()
            .using_index("flag", &[0_u8.into()])
            .filter("amount", Comparison::Ge, 6_i128)
    };
    let handle = query()
        .start_group_count::<Flag>(&mut jobs, time(0), time(100))
        .unwrap();
    for sequence in 0..3 {
        let step = query()
            .work_limit(1)
            .resume_group_count::<Flag>(&mut jobs, &handle, sequence, time(1))
            .unwrap();
        assert!(matches!(step.outcome, JobOutcome::Running));
    }
    let JobOutcome::Complete(rows) = query()
        .resume_group_count::<Flag>(&mut jobs, &handle, 3, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing groups")
    };
    assert_eq!(
        rows.rows
            .into_iter()
            .map(|r| (r.key.flag, r.count))
            .collect::<Vec<_>>(),
        vec![(0, 1)]
    );
    let empty = || db.query::<Row>().filter("id", Comparison::Gt, 100_u64);
    let empty_handle = empty()
        .start_group_count::<Flag>(&mut jobs, time(0), time(100))
        .unwrap();
    let JobOutcome::Complete(rows) = empty()
        .resume_group_count::<Flag>(&mut jobs, &empty_handle, 0, time(1))
        .unwrap()
        .outcome
    else {
        panic!("missing completion")
    };
    assert!(rows.rows.is_empty());
    drop(jobs);
    let key = StableKey::parse("test.jobs.v1").unwrap();
    let raw = host.open_memory(&key).unwrap();
    let mut record = raw_frame(&raw, 1);
    *record.last_mut().unwrap() ^= 1;
    set_raw_frame(&raw, 1, &record);
    drop(raw);
    let mut jobs = AggregateJobs::open(host.open_memory(&key).unwrap()).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        query().resume_group_count::<Flag>(&mut jobs, &handle, 0, time(2)),
        Err(Error::Corrupt)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn retained_summary_binds_both_outputs_and_pages_after_reopen() {
    #[record(fields(field(name = "id", value(item(prim = "Nat128")))))]
    struct WideId {}
    #[record(fields(field(name = "id", value(item(prim = "Decimal", scale = 2)))))]
    struct MeanId {}
    #[record(fields(field(name = "id", value(item(prim = "Decimal", scale = 0)))))]
    struct WholeId {}

    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(&runtime, &[None; 4], 0);
    let query = || {
        db.query::<Row>().order_groups(GroupOrder::Summary(
            ic_db::SummaryMeasure::Sum,
            ic_db::SortDirection::Descending,
        ))
    };
    let handle = query()
        .start_group_summary::<Flag, Id, MeanId>(&mut jobs, time(0), time(100))
        .unwrap();
    let first = query()
        .work_limit(1)
        .resume_group_summary::<Flag, Id, MeanId>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    drop(jobs);
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let before = backing.borrow().clone();
    for (measure, direction) in [
        (
            ic_db::SummaryMeasure::Count,
            ic_db::SortDirection::Descending,
        ),
        (ic_db::SummaryMeasure::Sum, ic_db::SortDirection::Ascending),
    ] {
        assert!(matches!(
            db.query::<Row>()
                .order_groups(GroupOrder::Summary(measure, direction))
                .resume_group_summary::<Flag, Id, MeanId>(&mut jobs, &handle, 1, time(2)),
            Err(Error::InvalidJob)
        ));
    }
    assert!(matches!(
        query().resume_group_summary::<Flag, WideId, MeanId>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        query().resume_group_summary::<Flag, Id, WholeId>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_avg::<Flag, MeanId>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    let retry = query()
        .resume_group_summary::<Flag, Id, MeanId>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert_eq!((retry.sequence, retry.examined), (1, 1));
    assert!(matches!(
        query()
            .group_page(1, None)
            .test_output_limit(1)
            .resume_group_summary::<Flag, Id, MeanId>(&mut jobs, &handle, 1, time(2)),
        Err(Error::RowTooLarge)
    ));
    assert_eq!(*backing.borrow(), before);
    let completed = query()
        .group_page(1, None)
        .resume_group_summary::<Flag, Id, MeanId>(&mut jobs, &handle, 1, time(2))
        .unwrap();
    let JobOutcome::Complete(first) = completed.outcome else {
        panic!("expected completed summary")
    };
    let cursor = first.next.unwrap();
    let last = query()
        .group_page(1, Some(&cursor))
        .resume_group_summary::<Flag, Id, MeanId>(&mut jobs, &handle, completed.sequence, time(3))
        .unwrap();
    let JobOutcome::Complete(last) = last.outcome else {
        panic!("expected final page")
    };
    assert!(last.next.is_none());
    for (page, flag, sum, average) in [(first.rows, 1, 4, 2), (last.rows, 0, 2, 1)] {
        let [row] = page.as_slice() else {
            panic!("one group per page")
        };
        assert_eq!(
            (row.key.flag, row.count, row.sum.as_ref().unwrap().id),
            (flag, 2, sum)
        );
        assert_eq!(
            row.average.as_ref().unwrap().id.coefficient_at_scale(0),
            Some(average)
        );
    }
}

#[test]
fn grouped_numeric_jobs_complete_large_inputs_with_absent_groups_and_exact_totals() {
    let host = host(VectorMemory::default());
    let (mut db, mut jobs) = create_stores(&host, schema());
    for chunk in 0..18 {
        let mut batch = Batch::new();
        for id in chunk * 29..(chunk + 1) * 29 {
            batch
                .insert(&Row {
                    id,
                    flag: (id % 3) as u8,
                    amount: match id % 3 {
                        0 => None,
                        1 => Some(2),
                        _ => Some(7),
                    },
                    payload: vec![],
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
    }
    let sum = db
        .query::<Row>()
        .start_group_sum::<Flag, Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let avg = db
        .query::<Row>()
        .start_group_avg::<Flag, Average2>(&mut jobs, time(0), time(100))
        .unwrap();
    let mut sequence = 0;

    loop {
        let s = db
            .query::<Row>()
            .work_limit(37)
            .resume_group_sum::<Flag, Amount>(&mut jobs, &sum, sequence, time(1))
            .unwrap();
        let a = db
            .query::<Row>()
            .work_limit(37)
            .resume_group_avg::<Flag, Average2>(&mut jobs, &avg, sequence, time(1))
            .unwrap();
        let retry = db
            .query::<Row>()
            .work_limit(1)
            .resume_group_avg::<Flag, Average2>(&mut jobs, &avg, sequence, time(1))
            .unwrap();
        assert_eq!((retry.sequence, retry.examined), (a.sequence, a.examined));
        sequence = s.sequence;

        match (s.outcome, a.outcome) {
            (JobOutcome::Running, JobOutcome::Running) => {}
            (JobOutcome::Complete(s), JobOutcome::Complete(a)) => {
                assert_eq!(
                    s.rows
                        .into_iter()
                        .map(|r| (r.key.flag, r.value.and_then(|v| v.amount)))
                        .collect::<Vec<_>>(),
                    vec![(0, None), (1, Some(348)), (2, Some(1218))]
                );
                assert_eq!(
                    a.rows
                        .into_iter()
                        .map(|r| (
                            r.key.flag,
                            r.value.and_then(|v| v.amount).map(|n| n.to_string())
                        ))
                        .collect::<Vec<_>>(),
                    vec![(0, None), (1, Some("2".into())), (2, Some("7".into()))]
                );
                break;
            }
            _ => panic!("inconsistent completion"),
        }
    }
}

#[test]
fn grouped_averages_round_per_group_once_and_select_completed_values() {
    use ic_db::{Decimal, GroupOrder};
    for sign in [1, -1] {
        for work in [1, 3] {
            let host = host(VectorMemory::default());
            let (db, mut jobs) = create(
                &host,
                &[
                    Some(sign),
                    Some(2 * sign),
                    Some(4 * sign),
                    Some(5 * sign),
                    None,
                    None,
                ],
                0,
            );
            let query = || {
                db.query::<Row>()
                    .filter_groups(Comparison::Ge, Decimal::try_new(2 * sign, 0).unwrap())
                    .order_groups(GroupOrder::AggregateDescending)
                    .limit(1)
            };
            let handle = query()
                .start_group_avg::<Flag, Average0>(&mut jobs, time(0), time(100))
                .unwrap();
            let mut sequence = 0;

            loop {
                let step = query()
                    .work_limit(work)
                    .resume_group_avg::<Flag, Average0>(&mut jobs, &handle, sequence, time(1))
                    .unwrap();
                sequence = step.sequence;

                match step.outcome {
                    JobOutcome::Running => {}
                    JobOutcome::Complete(rows) => {
                        assert_eq!(rows.rows.len(), 1);
                        let row = &rows.rows[0];
                        assert_eq!(row.key.flag, if sign == 1 { 1 } else { 0 });
                        assert_eq!(
                            row.value.as_ref().unwrap().amount.unwrap().to_string(),
                            if sign == 1 { "4" } else { "-2" }
                        );
                        break;
                    }
                    _ => panic!("unexpected outcome"),
                }
            }
        }
    }
}

#[test]
fn grouped_numeric_reopen_keeps_cancelling_totals_and_denominators() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(
        &runtime,
        &[
            Some(i128::MAX),
            None,
            Some(i128::MAX),
            None,
            Some(-i128::MAX),
            None,
            Some(-i128::MAX),
            None,
            Some(1),
            None,
        ],
        0,
    );
    let sum = db
        .query::<Row>()
        .start_group_sum::<Flag, Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let avg = db
        .query::<Row>()
        .start_group_avg::<Flag, Average28>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .work_limit(4)
        .resume_group_sum::<Flag, Amount>(&mut jobs, &sum, 0, time(1))
        .unwrap();
    db.query::<Row>()
        .work_limit(4)
        .resume_group_avg::<Flag, Average28>(&mut jobs, &avg, 0, time(1))
        .unwrap();
    drop((db, jobs, runtime));
    let runtime = host(backing);
    let db = Database::open(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let JobOutcome::Complete(s) = db
        .query::<Row>()
        .resume_group_sum::<Flag, Amount>(&mut jobs, &sum, 1, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing sum")
    };
    let JobOutcome::Complete(a) = db
        .query::<Row>()
        .resume_group_avg::<Flag, Average28>(&mut jobs, &avg, 1, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing average")
    };
    assert_eq!(s.rows[0].value.as_ref().unwrap().amount, Some(1));
    assert!(s.rows[1].value.is_none());
    assert_eq!(
        a.rows[0]
            .value
            .as_ref()
            .unwrap()
            .amount
            .unwrap()
            .to_string(),
        "0.2"
    );
    assert!(a.rows[1].value.is_none());
}

#[test]
fn grouped_numeric_bindings_output_and_hidden_overflow_preserve_checkpoints() {
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (db, mut jobs) = create(&host, &[Some(i128::MAX), Some(1), Some(1), Some(1)], 0);
    let query = || {
        db.query::<Row>()
            .filter_groups(Comparison::Lt, 10_i128)
            .limit(1)
    };
    let sum = query()
        .start_group_sum::<Flag, Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let avg = db
        .query::<Row>()
        .start_group_avg::<Flag, Average0>(&mut jobs, time(0), time(100))
        .unwrap();
    query()
        .work_limit(2)
        .resume_group_sum::<Flag, Amount>(&mut jobs, &sum, 0, time(1))
        .unwrap();
    db.query::<Row>()
        .work_limit(2)
        .resume_group_avg::<Flag, Average0>(&mut jobs, &avg, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        query().resume_group_sum::<Flag, Amount>(&mut jobs, &sum, 1, time(2)),
        Err(Error::ArithmeticOverflow)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_avg::<Flag, Average2>(&mut jobs, &avg, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_sum::<Flag, Amount>(&mut jobs, &avg, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_count::<Flag>(&mut jobs, &avg, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_avg::<Id, Average0>(&mut jobs, &avg, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .test_output_limit(1)
            .resume_group_avg::<Flag, Average0>(&mut jobs, &avg, 1, time(2)),
        Err(Error::RowTooLarge)
    ));
    assert_eq!(*backing.borrow(), before);
    let retry = query()
        .resume_group_sum::<Flag, Amount>(&mut jobs, &sum, 0, time(1))
        .unwrap();
    assert_eq!((retry.sequence, retry.examined), (1, 2));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_avg::<Flag, Average0>(&mut jobs, &avg, 1, time(2))
            .unwrap()
            .outcome,
        JobOutcome::Complete(_)
    ));
}

#[test]
fn grouped_numeric_decimal_rounding_and_u256_survive_multiple_steps() {
    use ic_db::{Decimal, U256};
    let host = host(VectorMemory::default());
    let (mut db, mut jobs) = create_stores(&host, Schema::new(&[NumericRow::SCHEMA]).unwrap());
    let large = U256::ONE << 200;
    let mut batch = Batch::new();
    for (id, coefficient) in [124, 125, 125, 126].into_iter().enumerate() {
        batch
            .insert(&NumericRow {
                id: id as u64,
                price: Decimal::try_new(coefficient, 2).unwrap(),
                wide: large + U256::from((id / 2) as u64),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let sum = db
        .query::<NumericRow>()
        .start_group_sum::<Price, Wide>(&mut jobs, time(0), time(100))
        .unwrap();
    let avg = db
        .query::<NumericRow>()
        .start_group_avg::<Wide, Price>(&mut jobs, time(0), time(100))
        .unwrap();
    let decimal_sum = db
        .query::<NumericRow>()
        .start_group_sum::<Wide, Price>(&mut jobs, time(0), time(100))
        .unwrap();
    for sequence in 0..3 {
        assert!(matches!(
            db.query::<NumericRow>()
                .work_limit(1)
                .resume_group_sum::<Price, Wide>(&mut jobs, &sum, sequence, time(1))
                .unwrap()
                .outcome,
            JobOutcome::Running
        ));
        assert!(matches!(
            db.query::<NumericRow>()
                .work_limit(1)
                .resume_group_avg::<Wide, Price>(&mut jobs, &avg, sequence, time(1))
                .unwrap()
                .outcome,
            JobOutcome::Running
        ));
        db.query::<NumericRow>()
            .work_limit(1)
            .resume_group_sum::<Wide, Price>(&mut jobs, &decimal_sum, sequence, time(1))
            .unwrap();
    }
    let JobOutcome::Complete(s) = db
        .query::<NumericRow>()
        .work_limit(1)
        .resume_group_sum::<Price, Wide>(&mut jobs, &sum, 3, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing wide sums")
    };
    assert_eq!(
        s.rows
            .into_iter()
            .map(|r| r.value.unwrap().wide)
            .collect::<Vec<_>>(),
        vec![large, large * 2_u128 + U256::ONE, large + U256::ONE]
    );
    let JobOutcome::Complete(a) = db
        .query::<NumericRow>()
        .resume_group_avg::<Wide, Price>(&mut jobs, &avg, 3, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing decimal averages")
    };
    assert_eq!(
        a.rows
            .into_iter()
            .map(|r| r.value.unwrap().price.to_string())
            .collect::<Vec<_>>(),
        vec!["1.24", "1.26"]
    );
    let JobOutcome::Complete(s) = db
        .query::<NumericRow>()
        .resume_group_sum::<Wide, Price>(&mut jobs, &decimal_sum, 3, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing decimal sums")
    };
    assert_eq!(
        s.rows
            .into_iter()
            .map(|r| r.value.unwrap().price.to_string())
            .collect::<Vec<_>>(),
        vec!["2.49", "2.51"]
    );
}

#[test]
fn grouped_numeric_frame_refusal_preserves_progress_before_output_selection() {
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (mut db, mut jobs) = create_stores(&host, Schema::new(&[LabelRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    for (id, c) in ('a'..='l').enumerate() {
        batch
            .insert(&LabelRow {
                id: id as u64,
                label: c.to_string().repeat(1400),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let query = || {
        db.query::<LabelRow>()
            .limit(1)
            .filter_groups(Comparison::Gt, 100_u64)
    };
    let handle = query()
        .start_group_sum::<Label, Id>(&mut jobs, time(0), time(100))
        .unwrap();
    for sequence in 0..11 {
        query()
            .work_limit(1)
            .resume_group_sum::<Label, Id>(&mut jobs, &handle, sequence, time(1))
            .unwrap();
    }
    let before = backing.borrow().clone();
    assert!(matches!(
        query().resume_group_sum::<Label, Id>(&mut jobs, &handle, 11, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    let retry = query()
        .resume_group_sum::<Label, Id>(&mut jobs, &handle, 10, time(1))
        .unwrap();
    assert_eq!((retry.sequence, retry.examined), (11, 11));
}

#[test]
fn grouped_numeric_corrupt_absent_total_refuses_and_remains_cancellable() {
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (db, mut jobs) = create(&host, &[None, None], 0);
    let handle = db
        .query::<Row>()
        .start_group_avg::<Flag, Average2>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .work_limit(1)
        .resume_group_avg::<Flag, Average2>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    drop(jobs);
    let key = StableKey::parse("test.jobs.v1").unwrap();
    let raw = host.open_memory(&key).unwrap();
    let mut record = raw_frame(&raw, 1);
    *record.last_mut().unwrap() = 1; // Nonzero total with a zero present count.
    set_raw_frame(&raw, 1, &record);
    drop(raw);
    let mut jobs = AggregateJobs::open(host.open_memory(&key).unwrap()).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .resume_group_avg::<Flag, Average2>(&mut jobs, &handle, 1, time(2)),
        Err(Error::Corrupt)
    ));
    assert_eq!(*backing.borrow(), before);
    jobs.cancel(&handle).unwrap();
}

#[test]
fn retained_field_counts_distinguish_rows_presence_and_empty_values() {
    let host = host(VectorMemory::default());
    let values: Vec<_> = (0..520)
        .map(|i| if i % 2 == 0 { Some(0) } else { None })
        .collect();
    let (db, mut jobs) = create(&host, &values, 0);
    let count = db
        .query::<Row>()
        .start_count_field("amount", &mut jobs, time(0), time(100))
        .unwrap();
    let groups = db
        .query::<Row>()
        .start_group_count_field::<Flag>("amount", &mut jobs, time(0), time(100))
        .unwrap();
    let empty_blob = db
        .query::<Row>()
        .start_count_field("payload", &mut jobs, time(0), time(100))
        .unwrap();
    let mut sequence = 0;

    loop {
        let c = db
            .query::<Row>()
            .work_limit(37)
            .resume_count_field("amount", &mut jobs, &count, sequence, time(1))
            .unwrap();
        let g = db
            .query::<Row>()
            .work_limit(37)
            .resume_group_count_field::<Flag>("amount", &mut jobs, &groups, sequence, time(1))
            .unwrap();
        let b = db
            .query::<Row>()
            .work_limit(37)
            .resume_count_field("payload", &mut jobs, &empty_blob, sequence, time(1))
            .unwrap();
        let retry = db
            .query::<Row>()
            .work_limit(1)
            .resume_count_field("amount", &mut jobs, &count, sequence, time(1))
            .unwrap();
        assert_eq!(
            (retry.sequence, retry.examined, &retry.outcome),
            (c.sequence, c.examined, &c.outcome)
        );
        sequence = c.sequence;

        match (c.outcome, g.outcome, b.outcome) {
            (JobOutcome::Running, JobOutcome::Running, JobOutcome::Running) => {}
            (JobOutcome::Complete(c), JobOutcome::Complete(g), JobOutcome::Complete(b)) => {
                assert_eq!((c, b), (260, 520));
                assert_eq!(
                    g.rows
                        .into_iter()
                        .map(|r| (r.key.flag, r.count))
                        .collect::<Vec<_>>(),
                    vec![(0, 260), (1, 0)]
                );
                break;
            }
            _ => panic!("inconsistent completion"),
        }
    }
}

///
/// PresenceRow
/// Exercises indexed access by `note` and `id` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["note", "id"], unique, present_only),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "flag", value(item(prim = "Nat8"))),
        field(name = "note", value(opt, item(prim = "Text", max_bytes = 12))),
        field(name = "items", value(opt, many, max_items = 4, item(prim = "Nat8")))
    )
)]
struct PresenceRow {}

///
/// PresenceNote
/// Carries an optional `Text` value of `note`.
///

#[record(fields(field(name = "note", value(opt, item(prim = "Text", max_bytes = 12)))))]
struct PresenceNote {}

#[test]
fn retained_collection_membership_groups_matches_across_the_synchronous_scan_limit() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let schema = Schema::new(&[PresenceRow::SCHEMA]).unwrap();
    let (mut db, mut jobs) = create_stores(&runtime, schema.clone());
    for start in (0..520).step_by(32) {
        let mut batch = Batch::new();
        for id in start..(start + 32).min(520) {
            let flag = (id % 2) as u8;
            let items = match id {
                511 | 514 | 519 => Some(vec![2, flag, flag]),
                _ if flag == 0 => None,
                _ => Some(vec![2]),
            };
            batch
                .insert(&PresenceRow {
                    id,
                    flag,
                    note: None,
                    items,
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
    }
    let query = || {
        db.query::<PresenceRow>()
            .filter_fields("items", Comparison::Contains, "flag")
            .work_limit(512)
    };
    assert!(matches!(query().group_count::<Flag>(), Err(Error::Limit)));
    let handle = query()
        .start_group_count::<Flag>(&mut jobs, time(0), time(100))
        .unwrap();
    let first = query()
        .resume_group_count::<Flag>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    assert_eq!(first.examined, 512);
    drop((db, jobs, runtime));

    let runtime = host(backing.clone());
    let db = Database::open(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema,
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let saved = backing.borrow().clone();
    assert!(matches!(
        db.query::<PresenceRow>()
            .filter("items", Comparison::Contains, 1_u8)
            .resume_group_count::<Flag>(&mut jobs, &handle, first.sequence, time(2)),
        Err(Error::InvalidJob)
    ));
    assert_eq!(*backing.borrow(), saved);
    let complete = db
        .query::<PresenceRow>()
        .filter_fields("items", Comparison::Contains, "flag")
        .work_limit(9)
        .resume_group_count::<Flag>(&mut jobs, &handle, first.sequence, time(2))
        .unwrap();
    assert_eq!(complete.examined, 520);
    let JobOutcome::Complete(page) = complete.outcome else {
        panic!("eight remaining candidates should fit")
    };
    assert_eq!(
        page.rows
            .iter()
            .map(|r| (r.key.flag, r.count))
            .collect::<Vec<_>>(),
        [(0, 1), (1, 2)]
    );
    assert!(page.next.is_none());
}

#[test]
fn text_jobs_bind_input_and_completed_group_selection_across_reopen() {
    for (comparison, needle, count) in [
        (Comparison::StartsWith, "a\0", 2),
        (Comparison::EndsWith, "\0c", 1),
        (Comparison::Contains, "\0", 2),
    ] {
        text_job_reopen(comparison, needle, count);
    }
}

fn text_job_reopen(comparison: Comparison, needle: &str, expected_count: u64) {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let schema = Schema::new(&[PresenceRow::SCHEMA]).unwrap();
    let (mut db, mut jobs) = create_stores(&runtime, schema.clone());
    let mut batch = Batch::new();
    for (id, note) in [None, Some(""), Some("a\0b"), Some("a\0c"), Some("é")]
        .into_iter()
        .enumerate()
    {
        batch
            .insert(&PresenceRow {
                id: id as u64,
                flag: (id % 2) as u8,
                note: note.map(str::to_owned),
                items: None,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let count = db
        .query::<PresenceRow>()
        .filter("note", comparison, needle)
        .using_index("note,id", &[])
        .start_count(&mut jobs, time(0), time(100))
        .unwrap();
    let count_first = db
        .query::<PresenceRow>()
        .filter("note", comparison, needle)
        .using_index("note,id", &[])
        .work_limit(1)
        .resume_count(&mut jobs, &count, 0, time(1))
        .unwrap();
    assert!(matches!(count_first.outcome, JobOutcome::Running));
    let groups = db
        .query::<PresenceRow>()
        .filter_groups(comparison, needle)
        .start_group_max::<Flag, PresenceNote>(&mut jobs, time(0), time(100))
        .unwrap();
    let first = db
        .query::<PresenceRow>()
        .filter_groups(comparison, needle)
        .work_limit(1)
        .resume_group_max::<Flag, PresenceNote>(&mut jobs, &groups, 0, time(1))
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    let saved = backing.borrow().clone();
    assert!(matches!(
        db.query::<PresenceRow>()
            .filter_groups(comparison, "changed")
            .resume_group_max::<Flag, PresenceNote>(&mut jobs, &groups, first.sequence, time(1)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<PresenceRow>()
            .filter("note", Comparison::Eq, needle)
            .using_index("note,id", &[])
            .resume_count(&mut jobs, &count, count_first.sequence, time(1)),
        Err(Error::InvalidJob)
    ));
    assert_eq!(*backing.borrow(), saved);
    drop((db, jobs, runtime));
    let runtime = host(backing);
    let db = Database::open(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema,
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let count = db
        .query::<PresenceRow>()
        .filter("note", comparison, needle)
        .using_index("note,id", &[])
        .resume_count(&mut jobs, &count, count_first.sequence, time(2))
        .unwrap();
    assert_eq!(count.outcome, JobOutcome::Complete(expected_count));
    assert_eq!(
        count.examined,
        if comparison == Comparison::StartsWith {
            expected_count
        } else {
            4
        }
    );
    let mut sequence = first.sequence;
    for _ in 0..8 {
        let step = db
            .query::<PresenceRow>()
            .filter_groups(comparison, needle)
            .work_limit(1)
            .resume_group_max::<Flag, PresenceNote>(&mut jobs, &groups, sequence, time(2))
            .unwrap();
        sequence = step.sequence;

        match step.outcome {
            JobOutcome::Running => {}
            JobOutcome::Complete(page) => {
                assert_eq!(page.rows.len(), 1);
                assert_eq!(page.rows[0].key.flag, 1);
                assert_eq!(
                    page.rows[0].value.as_ref().unwrap().note.as_deref(),
                    Some("a\0c")
                );

                return;
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
    panic!("bounded text groups must complete");
}

#[test]
fn retained_presence_counts_empty_text_and_collections_and_reopens_zero_groups() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let schema = Schema::new(&[PresenceRow::SCHEMA]).unwrap();
    let (mut db, mut jobs) = create_stores(&runtime, schema.clone());
    let mut batch = Batch::new();
    for (id, note, items) in [
        (0, Some(""), Some(vec![])),
        (1, None, None),
        (2, Some("x"), Some(vec![0, 1])),
        (3, None, None),
    ] {
        batch
            .insert(&PresenceRow {
                id,
                flag: (id % 2) as u8,
                note: note.map(str::to_owned),
                items,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let count = db
        .query::<PresenceRow>()
        .start_count_field("items", &mut jobs, time(0), time(100))
        .unwrap();
    let groups = db
        .query::<PresenceRow>()
        .filter_groups(Comparison::Eq, 0_u64)
        .start_group_count_field::<Flag>("note", &mut jobs, time(0), time(100))
        .unwrap();
    db.query::<PresenceRow>()
        .work_limit(2)
        .resume_count_field("items", &mut jobs, &count, 0, time(1))
        .unwrap();
    db.query::<PresenceRow>()
        .filter_groups(Comparison::Eq, 0_u64)
        .work_limit(2)
        .resume_group_count_field::<Flag>("note", &mut jobs, &groups, 0, time(1))
        .unwrap();
    let selected = db
        .query::<PresenceRow>()
        .filter("items", Comparison::Ne, ic_db::IndexValue::Absent)
        .start_count(&mut jobs, time(0), time(100))
        .unwrap();
    let selected_step = db
        .query::<PresenceRow>()
        .filter("items", Comparison::Ne, ic_db::IndexValue::Absent)
        .work_limit(2)
        .resume_count(&mut jobs, &selected, 0, time(1))
        .unwrap();
    assert!(matches!(selected_step.outcome, JobOutcome::Running));
    drop((db, jobs, runtime));
    let runtime = host(backing);
    let db = Database::open(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema,
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        db.query::<PresenceRow>()
            .filter("items", Comparison::Eq, ic_db::IndexValue::Absent)
            .resume_count(&mut jobs, &selected, selected_step.sequence, time(2)),
        Err(Error::InvalidJob)
    ));
    assert_eq!(
        db.query::<PresenceRow>()
            .filter("items", Comparison::Ne, ic_db::IndexValue::Absent)
            .resume_count(&mut jobs, &selected, selected_step.sequence, time(2))
            .unwrap()
            .outcome,
        JobOutcome::Complete(2)
    );
    assert_eq!(
        db.query::<PresenceRow>()
            .resume_count_field("items", &mut jobs, &count, 1, time(2))
            .unwrap()
            .outcome,
        JobOutcome::Complete(2)
    );
    let JobOutcome::Complete(rows) = db
        .query::<PresenceRow>()
        .filter_groups(Comparison::Eq, 0_u64)
        .resume_group_count_field::<Flag>("note", &mut jobs, &groups, 1, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing groups")
    };
    assert_eq!(
        rows.rows
            .into_iter()
            .map(|r| (r.key.flag, r.count))
            .collect::<Vec<_>>(),
        vec![(1, 0)]
    );
}

#[test]
fn retained_field_binding_and_output_refusals_preserve_progress() {
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (db, mut jobs) = create(&host, &[None, Some(0), None], 0);
    let scalar = db
        .query::<Row>()
        .start_count_field("id", &mut jobs, time(0), time(100))
        .unwrap();
    let grouped = db
        .query::<Row>()
        .start_group_count_field::<Flag>("amount", &mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .work_limit(1)
        .resume_count_field("id", &mut jobs, &scalar, 0, time(1))
        .unwrap();
    db.query::<Row>()
        .work_limit(1)
        .resume_group_count_field::<Flag>("amount", &mut jobs, &grouped, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .start_count_field("missing", &mut jobs, time(0), time(100)),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_count_field("payload", &mut jobs, &scalar, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_count(&mut jobs, &scalar, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_count::<Flag>(&mut jobs, &grouped, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>().resume_group_count_field::<Flag>(
            "payload",
            &mut jobs,
            &grouped,
            1,
            time(2)
        ),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>().test_output_limit(1).resume_count_field(
            "id",
            &mut jobs,
            &scalar,
            1,
            time(2)
        ),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Row>()
            .test_output_limit(9)
            .resume_group_count_field::<Flag>("amount", &mut jobs, &grouped, 1, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    assert_eq!(
        db.query::<Row>()
            .resume_count_field("id", &mut jobs, &scalar, 1, time(2))
            .unwrap()
            .outcome,
        JobOutcome::Complete(3)
    );
}

#[test]
fn retained_distinct_counts_whole_tuples_across_chunks_and_reopen() {
    use std::collections::BTreeSet;
    let values: Vec<_> = (0..521)
        .map(|i| if i % 7 == 0 { None } else { Some(i % 3) })
        .collect();
    let expected = values
        .iter()
        .enumerate()
        .map(|(i, value)| (*value, (i % 2) as u8))
        .collect::<BTreeSet<_>>()
        .len() as u64;
    for work in [17, 128] {
        let backing = VectorMemory::default();
        let runtime = host(backing.clone());
        let (db, mut jobs) = create(&runtime, &values, 0);
        let handle = db
            .query::<Row>()
            .limit(1)
            .start_count_distinct::<GroupKey>(&mut jobs, time(0), time(100))
            .unwrap();
        let first = db
            .query::<Row>()
            .limit(1)
            .work_limit(work)
            .resume_count_distinct::<GroupKey>(&mut jobs, &handle, 0, time(1))
            .unwrap();
        assert!(matches!(first.outcome, JobOutcome::Running));
        drop((db, jobs, runtime));
        let runtime = host(backing);
        let db = Database::open(
            runtime
                .open_memory(&StableKey::parse("test.db.v1").unwrap())
                .unwrap(),
            schema(),
        )
        .unwrap();
        let mut jobs = AggregateJobs::open(
            runtime
                .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
                .unwrap(),
        )
        .unwrap();
        let mut sequence = first.sequence;

        loop {
            let step = db
                .query::<Row>()
                .work_limit(work)
                .resume_count_distinct::<GroupKey>(&mut jobs, &handle, sequence, time(2))
                .unwrap();
            let retry = db
                .query::<Row>()
                .work_limit(1)
                .resume_count_distinct::<GroupKey>(&mut jobs, &handle, sequence, time(2))
                .unwrap();
            assert_eq!(
                (retry.sequence, retry.examined, &retry.outcome),
                (step.sequence, step.examined, &step.outcome)
            );
            sequence = step.sequence;

            match step.outcome {
                JobOutcome::Running => {}
                JobOutcome::Complete(count) => {
                    assert_eq!(count, expected);
                    assert_eq!(step.examined, 521);
                    break;
                }
                _ => panic!("unexpected outcome"),
            }
        }
    }
}

#[test]
fn retained_distinct_empty_absent_and_text_keys_keep_exact_equality() {
    for values in [vec![], vec![None; 3], vec![None, Some(0), None, Some(0)]] {
        let runtime = host(VectorMemory::default());
        let (db, mut jobs) = create(&runtime, &values, 0);
        let handle = db
            .query::<Row>()
            .start_count_distinct::<Amount>(&mut jobs, time(0), time(100))
            .unwrap();
        let mut sequence = 0;

        loop {
            let step = db
                .query::<Row>()
                .work_limit(1)
                .resume_count_distinct::<Amount>(&mut jobs, &handle, sequence, time(1))
                .unwrap();
            sequence = step.sequence;
            if let JobOutcome::Complete(count) = step.outcome {
                assert_eq!(
                    count,
                    values
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len() as u64
                );
                break;
            }
        }
    }
    let runtime = host(VectorMemory::default());
    let (mut db, mut jobs) = create_stores(&runtime, Schema::new(&[LabelRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    for (id, label) in ["", "a\0b", "é", "", "é", "a\0b"].into_iter().enumerate() {
        batch
            .insert(&LabelRow {
                id: id as u64,
                label: label.into(),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let handle = db
        .query::<LabelRow>()
        .start_count_distinct::<Label>(&mut jobs, time(0), time(100))
        .unwrap();
    for sequence in 0..5 {
        assert!(matches!(
            db.query::<LabelRow>()
                .work_limit(1)
                .resume_count_distinct::<Label>(&mut jobs, &handle, sequence, time(1))
                .unwrap()
                .outcome,
            JobOutcome::Running
        ));
    }
    assert_eq!(
        db.query::<LabelRow>()
            .work_limit(1)
            .resume_count_distinct::<Label>(&mut jobs, &handle, 5, time(1))
            .unwrap()
            .outcome,
        JobOutcome::Complete(3)
    );
}

#[test]
fn retained_distinct_cardinality_and_frame_refusals_preserve_saved_sets() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let values: Vec<_> = (0..33).map(Some).collect();
    let (db, mut jobs) = create(&runtime, &values, 0);
    let handle = db
        .query::<Row>()
        .limit(1)
        .start_count_distinct::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .work_limit(32)
        .resume_count_distinct::<Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .limit(1)
            .resume_count_distinct::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    let retry = db
        .query::<Row>()
        .resume_count_distinct::<Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert_eq!((retry.sequence, retry.examined), (1, 32));

    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (mut db, mut jobs) = create_stores(&runtime, Schema::new(&[LabelRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    for (id, c) in ('a'..='l').enumerate() {
        batch
            .insert(&LabelRow {
                id: id as u64,
                label: c.to_string().repeat(1400),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    assert_eq!(
        db.query::<LabelRow>()
            .count_distinct::<Label>()
            .unwrap()
            .value,
        12
    );
    let handle = db
        .query::<LabelRow>()
        .start_count_distinct::<Label>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<LabelRow>()
        .work_limit(11)
        .resume_count_distinct::<Label>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<LabelRow>()
            .resume_count_distinct::<Label>(&mut jobs, &handle, 1, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn retained_distinct_binding_output_and_source_revision_are_enforced() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (mut db, mut jobs) = create(&runtime, &[Some(1), None, Some(1)], 0);
    let handle = db
        .query::<Row>()
        .start_count_distinct::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .work_limit(1)
        .resume_count_distinct::<Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .resume_count_distinct::<Id>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_count(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_count_field("amount", &mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .test_output_limit(1)
            .resume_count_distinct::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::RowTooLarge)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut row = db.get::<Row>(0).unwrap().unwrap();
    row.amount = Some(2);
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(
        db.query::<Row>()
            .resume_count_distinct::<Amount>(&mut jobs, &handle, 0, time(2))
            .unwrap()
            .outcome,
        JobOutcome::Invalidated
    );
}

#[test]
fn retained_distinct_malformed_and_duplicate_keys_refuse_intact() {
    for duplicate in [false, true] {
        let backing = VectorMemory::default();
        let runtime = host(backing.clone());
        let (db, mut jobs) = create(&runtime, &[None, None], 0);
        let handle = if duplicate {
            db.query::<Row>()
                .start_count_distinct::<Flag>(&mut jobs, time(0), time(100))
                .unwrap()
        } else {
            db.query::<Row>()
                .start_count_distinct::<Amount>(&mut jobs, time(0), time(100))
                .unwrap()
        };
        if duplicate {
            db.query::<Row>()
                .work_limit(2)
                .resume_count_distinct::<Flag>(&mut jobs, &handle, 0, time(1))
                .unwrap();
        } else {
            db.query::<Row>()
                .work_limit(1)
                .resume_count_distinct::<Amount>(&mut jobs, &handle, 0, time(1))
                .unwrap();
        }
        drop(jobs);
        let key = StableKey::parse("test.jobs.v1").unwrap();
        let raw = runtime.open_memory(&key).unwrap();
        let mut record = raw_frame(&raw, 1);
        *record.last_mut().unwrap() = if duplicate { 0 } else { 2 };
        set_raw_frame(&raw, 1, &record);
        drop(raw);
        let mut jobs = AggregateJobs::open(runtime.open_memory(&key).unwrap()).unwrap();
        let before = backing.borrow().clone();
        let result = if duplicate {
            db.query::<Row>()
                .resume_count_distinct::<Flag>(&mut jobs, &handle, 1, time(2))
        } else {
            db.query::<Row>()
                .resume_count_distinct::<Amount>(&mut jobs, &handle, 1, time(2))
        };
        assert!(matches!(result, Err(Error::Corrupt)));
        assert_eq!(*backing.borrow(), before);
        jobs.cancel(&handle).unwrap();
    }
}

#[test]
fn retained_group_distinct_matches_independent_sets_with_overlap_and_selection() {
    use ic_db::GroupOrder;
    use std::collections::{BTreeMap, BTreeSet};
    let runtime = host(VectorMemory::default());
    let (mut db, mut jobs) = create_stores(&runtime, schema());
    let mut oracle = BTreeMap::<u8, BTreeSet<Option<i128>>>::new();
    for chunk in (0..521_u64).collect::<Vec<_>>().chunks(32) {
        let mut batch = Batch::new();
        for &id in chunk {
            let flag = (id % 3) as u8;
            let amount = if flag == 2 || id % 7 == 0 {
                None
            } else {
                Some(i128::from(id % 5))
            };
            oracle.entry(flag).or_default().insert(amount);
            batch
                .insert(&Row {
                    id,
                    flag,
                    amount,
                    payload: vec![],
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
    }
    for selected in [false, true] {
        let query = || {
            let query = db.query::<Row>();

            if selected {
                query
                    .filter_groups(Comparison::Ge, 2_u64)
                    .order_groups(GroupOrder::AggregateDescending)
                    .limit(1)
            } else {
                query
            }
        };
        let handle = query()
            .start_group_count_distinct::<Flag, GroupKey>(&mut jobs, time(0), time(100))
            .unwrap();
        let mut expected: Vec<_> = oracle
            .iter()
            .map(|(&key, values)| (key, values.len() as u64))
            .collect();
        if selected {
            expected.retain(|(_, count)| *count >= 2);
            expected.sort_by(|(ka, a), (kb, b)| b.cmp(a).then_with(|| ka.cmp(kb)));
            expected.truncate(1);
        }
        let mut sequence = 0;

        loop {
            let step = query()
                .work_limit(19)
                .resume_group_count_distinct::<Flag, GroupKey>(
                    &mut jobs,
                    &handle,
                    sequence,
                    time(1),
                )
                .unwrap();
            let retry = query()
                .work_limit(128)
                .resume_group_count_distinct::<Flag, GroupKey>(
                    &mut jobs,
                    &handle,
                    sequence,
                    time(1),
                )
                .unwrap();
            assert_eq!(
                (retry.sequence, retry.examined),
                (step.sequence, step.examined)
            );
            sequence = step.sequence;

            match step.outcome {
                JobOutcome::Running => {}
                JobOutcome::Complete(rows) => {
                    assert_eq!(
                        rows.rows
                            .into_iter()
                            .map(|r| (r.key.flag, r.count))
                            .collect::<Vec<_>>(),
                        expected
                    );
                    assert_eq!(step.examined, 521);
                    break;
                }
                _ => panic!("unexpected outcome"),
            }
        }
    }
}

#[test]
fn retained_group_distinct_reopens_sets_and_binds_both_projections_and_selection() {
    use ic_db::GroupOrder;
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(
        &runtime,
        &[None, Some(1), None, Some(1), Some(0), Some(2)],
        0,
    );
    let handle = db
        .query::<Row>()
        .start_group_count_distinct::<Flag, Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .work_limit(2)
        .resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    drop((db, jobs, runtime));
    let runtime = host(backing.clone());
    let db = Database::open(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .resume_group_count_distinct::<Flag, Id>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_count_distinct::<Amount, Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .limit(1)
            .resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .order_groups(GroupOrder::AggregateDescending)
            .resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_count::<Flag>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_count_distinct::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .test_output_limit(9)
            .resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    let JobOutcome::Complete(rows) = db
        .query::<Row>()
        .resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 1, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing groups")
    };
    assert_eq!(
        rows.rows
            .into_iter()
            .map(|r| (r.key.flag, r.count))
            .collect::<Vec<_>>(),
        vec![(0, 2), (1, 2)]
    );
}

#[test]
fn retained_group_distinct_caps_groups_and_per_group_sets_before_selection() {
    for many_groups in [false, true] {
        let backing = VectorMemory::default();
        let runtime = host(backing.clone());
        let (mut db, mut jobs) = create_stores(&runtime, schema());
        for chunk in (0..34_u64).collect::<Vec<_>>().chunks(32) {
            let mut batch = Batch::new();
            for &id in chunk {
                // After filling the set, repeat its last member before adding a new one.
                let value = if id < 32 { id } else { id - 1 };
                batch
                    .insert(&Row {
                        id,
                        flag: if many_groups { value as u8 } else { 0 },
                        amount: Some(i128::from(value)),
                        payload: vec![],
                    })
                    .unwrap();
            }
            db.commit(batch).unwrap();
        }
        let query = || {
            db.query::<Row>()
                .filter_groups(Comparison::Gt, 1000_u64)
                .limit(1)
        };
        let handle = query()
            .start_group_count_distinct::<Flag, Amount>(&mut jobs, time(0), time(100))
            .unwrap();
        query()
            .work_limit(32)
            .resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 0, time(1))
            .unwrap();
        let before = backing.borrow().clone();
        assert!(matches!(
            query().resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 1, time(2)),
            Err(Error::Limit)
        ));
        assert_eq!(*backing.borrow(), before);
        let retry = query()
            .resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 0, time(1))
            .unwrap();
        assert_eq!((retry.sequence, retry.examined), (1, 32));
        let duplicate = query()
            .work_limit(1)
            .resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 1, time(2))
            .unwrap();
        assert!(matches!(duplicate.outcome, JobOutcome::Running));
        assert_eq!((duplicate.sequence, duplicate.examined), (2, 33));
        let before = backing.borrow().clone();
        assert!(matches!(
            query().resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 2, time(3)),
            Err(Error::Limit)
        ));
        assert_eq!(*backing.borrow(), before);
    }
}

#[test]
fn retained_group_distinct_frame_limit_covers_all_sets_together() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (mut db, mut jobs) = create_stores(&runtime, Schema::new(&[LabelRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    for (id, c) in ('a'..='l').enumerate() {
        batch
            .insert(&LabelRow {
                id: id as u64,
                label: c.to_string().repeat(1400),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    assert_eq!(
        db.query::<LabelRow>()
            .group_count_distinct::<Id, Label>()
            .unwrap()
            .value
            .len(),
        12
    );
    let query = || {
        db.query::<LabelRow>()
            .filter_groups(Comparison::Gt, 100_u64)
            .limit(1)
    };
    let handle = query()
        .start_group_count_distinct::<Id, Label>(&mut jobs, time(0), time(100))
        .unwrap();
    query()
        .work_limit(11)
        .resume_group_count_distinct::<Id, Label>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        query().resume_group_count_distinct::<Id, Label>(&mut jobs, &handle, 1, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn retained_group_distinct_empty_sparse_and_corrupt_sets_are_explicit() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(&runtime, &[None; 4], 0);
    let query = || db.query::<Row>().filter("id", Comparison::Ge, 3_u64);
    let handle = query()
        .start_group_count_distinct::<Flag, Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let step = query()
        .work_limit(1)
        .resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert_eq!(step.examined, 1);
    let JobOutcome::Complete(rows) = step.outcome else {
        panic!("missing groups")
    };
    assert_eq!(
        rows.rows
            .into_iter()
            .map(|r| (r.key.flag, r.count))
            .collect::<Vec<_>>(),
        vec![(1, 1)]
    );
    let empty = || db.query::<Row>().filter("id", Comparison::Gt, 100_u64);
    let empty_handle = empty()
        .start_group_count_distinct::<Flag, Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let JobOutcome::Complete(rows) = empty()
        .resume_group_count_distinct::<Flag, Amount>(&mut jobs, &empty_handle, 0, time(1))
        .unwrap()
        .outcome
    else {
        panic!("missing empty result")
    };
    assert!(rows.rows.is_empty());
    drop(jobs);
    let key = StableKey::parse("test.jobs.v1").unwrap();
    let raw = runtime.open_memory(&key).unwrap();
    let mut record = raw_frame(&raw, 1);
    *record.last_mut().unwrap() = 2; // Invalid optional-key tag within the group's set.
    set_raw_frame(&raw, 1, &record);
    drop(raw);
    let mut jobs = AggregateJobs::open(runtime.open_memory(&key).unwrap()).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        query().resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::Corrupt)
    ));
    assert_eq!(*backing.borrow(), before);
    jobs.cancel(&handle).unwrap();
}

#[test]
fn retained_group_distinct_allows_independent_full_sets() {
    let runtime = host(VectorMemory::default());
    let values: Vec<_> = (0..64).map(Some).collect();
    let (db, mut jobs) = create(&runtime, &values, 0);
    let handle = db
        .query::<Row>()
        .start_group_count_distinct::<Flag, Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .work_limit(32)
        .resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let JobOutcome::Complete(rows) = db
        .query::<Row>()
        .resume_group_count_distinct::<Flag, Amount>(&mut jobs, &handle, 1, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing groups")
    };
    assert_eq!(
        rows.rows
            .into_iter()
            .map(|row| (row.key.flag, row.count))
            .collect::<Vec<_>>(),
        vec![(0, 32), (1, 32)]
    );
}

#[test]
fn completed_count_pages_keep_order_limit_and_captured_revision() {
    use ic_db::{GroupCursor, GroupOrder};
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (mut db, mut jobs) = create(
        &runtime,
        &[
            None,
            Some(0),
            Some(1),
            Some(0),
            None,
            Some(1),
            Some(0),
            Some(2),
        ],
        0,
    );
    let handle = db
        .query::<Row>()
        .order_groups(GroupOrder::AggregateDescending)
        .limit(3)
        .start_group_count::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let first = db
        .query::<Row>()
        .order_groups(GroupOrder::AggregateDescending)
        .limit(3)
        .group_page(1, None)
        .resume_group_count::<Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let sequence = first.sequence;
    let revision = first.source_revision;
    let JobOutcome::Complete(page) = first.outcome else {
        panic!("missing page")
    };
    assert_eq!((page.rows[0].key.amount, page.rows[0].count), (Some(0), 3));
    let cursor = GroupCursor::from_bytes(&page.next.unwrap().to_bytes()).unwrap();
    // The completed result stays fixed even when a grouping key changes afterwards.
    let mut row = db.get::<Row>(0).unwrap().unwrap();
    row.amount = Some(9);
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    drop((db, jobs, runtime));
    let runtime = host(backing);
    let db = Database::open(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    for _ in 0..2 {
        let step = db
            .query::<Row>()
            .order_groups(GroupOrder::AggregateDescending)
            .limit(3)
            .group_page(2, Some(&cursor))
            .resume_group_count::<Amount>(&mut jobs, &handle, 0, time(2))
            .unwrap();
        assert_eq!(
            (step.sequence, step.source_revision, step.examined),
            (sequence, revision, 8)
        );
        let JobOutcome::Complete(page) = step.outcome else {
            panic!("missing completion")
        };
        assert_eq!(
            page.rows
                .into_iter()
                .map(|r| (r.key.amount, r.count))
                .collect::<Vec<_>>(),
            vec![(None, 2), (Some(1), 2)]
        );
        assert!(page.next.is_none());
    }
    let expired = db
        .query::<Row>()
        .order_groups(GroupOrder::AggregateDescending)
        .limit(3)
        .group_page(2, Some(&cursor))
        .resume_group_count::<Amount>(&mut jobs, &handle, sequence, time(100))
        .unwrap();
    assert!(matches!(expired.outcome, JobOutcome::Expired));
}

#[test]
fn completed_pages_retry_the_group_that_did_not_fit() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (mut db, mut jobs) = create_stores(&runtime, Schema::new(&[LabelRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    for (id, label) in ["a".to_owned(), "b".repeat(100), "c".into()]
        .into_iter()
        .enumerate()
    {
        batch
            .insert(&LabelRow {
                id: id as u64,
                label,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let handle = db
        .query::<LabelRow>()
        .start_group_count::<Label>(&mut jobs, time(0), time(100))
        .unwrap();
    let before = backing.borrow().clone();
    let mut unpaged = |bytes| {
        db.query::<LabelRow>()
            .test_output_limit(bytes)
            .resume_group_count::<Label>(&mut jobs, &handle, 0, time(1))
    };
    assert!(matches!(unpaged(20), Err(Error::RowTooLarge)));
    assert_eq!(*backing.borrow(), before);
    assert!(matches!(unpaged(120), Err(Error::Limit)));
    assert_eq!(*backing.borrow(), before);
    let step = db
        .query::<LabelRow>()
        .test_output_limit(20)
        .group_page(3, None)
        .resume_group_count::<Label>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert!(step.output_bytes <= 20);
    let JobOutcome::Complete(page) = step.outcome else {
        panic!("missing page")
    };
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].key.label, "a");
    let cursor = page.next.unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<LabelRow>()
            .test_output_limit(20)
            .group_page(3, Some(&cursor))
            .resume_group_count::<Label>(&mut jobs, &handle, 1, time(2)),
        Err(Error::RowTooLarge)
    ));
    assert_eq!(*backing.borrow(), before);
    let JobOutcome::Complete(page) = db
        .query::<LabelRow>()
        .test_output_limit(200)
        .group_page(3, Some(&cursor))
        .resume_group_count::<Label>(&mut jobs, &handle, 1, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing page")
    };
    assert_eq!(
        page.rows
            .into_iter()
            .map(|r| r.key.label)
            .collect::<Vec<_>>(),
        vec!["b".repeat(100), "c".into()]
    );
    assert!(page.next.is_none());
}

#[test]
fn completed_page_cursors_refuse_foreign_pending_and_out_of_range_jobs() {
    use ic_db::GroupCursor;
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(&runtime, &[Some(1); 4], 0);
    let handle = db
        .query::<Row>()
        .start_group_count::<Flag>(&mut jobs, time(0), time(100))
        .unwrap();
    let other = db
        .query::<Row>()
        .start_group_count::<Flag>(&mut jobs, time(0), time(100))
        .unwrap();
    let mut bytes = vec![3];
    bytes.extend(handle.to_bytes());
    bytes.push(1);
    let pending = GroupCursor::from_bytes(&bytes).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .group_page(1, Some(&pending))
            .resume_group_count::<Flag>(&mut jobs, &handle, 0, time(1)),
        Err(Error::InvalidCursor)
    ));
    assert_eq!(*backing.borrow(), before);
    let JobOutcome::Complete(page) = db
        .query::<Row>()
        .group_page(1, None)
        .resume_group_count::<Flag>(&mut jobs, &handle, 0, time(1))
        .unwrap()
        .outcome
    else {
        panic!("missing page")
    };
    let cursor = page.next.unwrap();
    let mut invalid = cursor.to_bytes();
    invalid[27] = 3;
    let out_of_range = GroupCursor::from_bytes(&invalid).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .group_page(1, Some(&cursor))
            .resume_group_count::<Flag>(&mut jobs, &other, 0, time(2)),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        db.query::<Row>()
            .group_page(1, Some(&out_of_range))
            .resume_group_count::<Flag>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        db.query::<Row>()
            .limit(1)
            .group_page(1, Some(&cursor))
            .resume_group_count::<Flag>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert_eq!(*backing.borrow(), before);
    for malformed in [
        vec![],
        vec![1; 29],
        {
            let mut b = cursor.to_bytes();
            b[27] = 0;

            b
        },
        {
            let mut b = cursor.to_bytes();
            b[0] = 2;

            b
        },
    ] {
        assert!(matches!(
            GroupCursor::from_bytes(&malformed),
            Err(Error::InvalidCursor)
        ));
    }
    jobs.cancel(&handle).unwrap();
    let replacement = db
        .query::<Row>()
        .start_group_count::<Flag>(&mut jobs, time(0), time(100))
        .unwrap();
    assert!(matches!(
        db.query::<Row>()
            .group_page(1, Some(&cursor))
            .resume_group_count::<Flag>(&mut jobs, &replacement, 0, time(2)),
        Err(Error::InvalidCursor)
    ));
}

#[test]
fn group_paging_is_only_a_retained_output_mode_and_never_hides_overflow() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(&runtime, &[Some(1), Some(i128::MAX), Some(1), Some(1)], 0);
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>().group_page(1, None).fetch(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Row>().group_page(1, None).count(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Row>().group_page(1, None).group_count::<Flag>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        db.query::<Row>()
            .group_page(1, None)
            .start_group_count::<Flag>(&mut jobs, time(0), time(100)),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(*backing.borrow(), before);
    let handle = db
        .query::<Row>()
        .limit(1)
        .start_group_sum::<Flag, Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .limit(1)
            .group_page(1, None)
            .resume_group_sum::<Flag, Amount>(&mut jobs, &handle, 0, time(1)),
        Err(Error::ArithmeticOverflow)
    ));
    assert_eq!(*backing.borrow(), before);
    for size in [0, 33] {
        assert!(matches!(
            db.query::<Row>()
                .limit(1)
                .group_page(size, None)
                .resume_group_sum::<Flag, Amount>(&mut jobs, &handle, 0, time(1)),
            Err(Error::Limit)
        ));
    }
}

#[test]
fn field_and_distinct_count_pages_use_completed_group_selection() {
    for distinct in [false, true] {
        let runtime = host(VectorMemory::default());
        let (db, mut jobs) = create(&runtime, &[None, Some(1), None, Some(2)], 0);
        let handle = if distinct {
            db.query::<Row>()
                .start_group_count_distinct::<Flag, Amount>(&mut jobs, time(0), time(100))
                .unwrap()
        } else {
            db.query::<Row>()
                .start_group_count_field::<Flag>("amount", &mut jobs, time(0), time(100))
                .unwrap()
        };
        let mut cursor = None;
        let mut sequence = 0;
        let mut actual = Vec::new();
        loop {
            let query = db.query::<Row>().group_page(1, cursor.as_ref());
            let step = if distinct {
                query
                    .resume_group_count_distinct::<Flag, Amount>(
                        &mut jobs,
                        &handle,
                        sequence,
                        time(1),
                    )
                    .unwrap()
            } else {
                query
                    .resume_group_count_field::<Flag>(
                        "amount",
                        &mut jobs,
                        &handle,
                        sequence,
                        time(1),
                    )
                    .unwrap()
            };
            sequence = step.sequence;
            let JobOutcome::Complete(page) = step.outcome else {
                panic!("missing page")
            };
            actual.extend(page.rows.into_iter().map(|row| (row.key.flag, row.count)));
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(actual, vec![(0, u64::from(distinct)), (1, 2)]);
    }
}

#[test]
fn numeric_pages_keep_absence_rounding_and_completed_order() {
    use ic_db::GroupOrder;
    for average in [false, true] {
        let runtime = host(VectorMemory::default());
        let (db, mut jobs) = create(&runtime, &[Some(1), None, Some(2), None], 0);
        let handle = if average {
            db.query::<Row>()
                .order_groups(GroupOrder::AggregateDescending)
                .start_group_avg::<Flag, Average0>(&mut jobs, time(0), time(100))
                .unwrap()
        } else {
            db.query::<Row>()
                .order_groups(GroupOrder::AggregateDescending)
                .start_group_sum::<Flag, Amount>(&mut jobs, time(0), time(100))
                .unwrap()
        };
        let mut cursor = None;
        let mut sequence = 0;
        let mut actual = Vec::new();
        loop {
            let query = db
                .query::<Row>()
                .order_groups(GroupOrder::AggregateDescending)
                .group_page(1, cursor.as_ref());
            if average {
                let step = query
                    .resume_group_avg::<Flag, Average0>(&mut jobs, &handle, sequence, time(1))
                    .unwrap();
                sequence = step.sequence;
                let JobOutcome::Complete(page) = step.outcome else {
                    panic!("missing page")
                };
                actual.extend(page.rows.into_iter().map(|r| {
                    (
                        r.key.flag,
                        r.value.and_then(|r| r.amount).map(|v| v.to_string()),
                    )
                }));
                cursor = page.next;
            } else {
                let step = query
                    .resume_group_sum::<Flag, Amount>(&mut jobs, &handle, sequence, time(1))
                    .unwrap();
                sequence = step.sequence;
                let JobOutcome::Complete(page) = step.outcome else {
                    panic!("missing page")
                };
                actual.extend(page.rows.into_iter().map(|r| {
                    (
                        r.key.flag,
                        r.value.and_then(|r| r.amount).map(|v| v.to_string()),
                    )
                }));
                cursor = page.next;
            }
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(
            actual,
            vec![(0, Some(if average { "2" } else { "3" }.into())), (1, None)]
        );
    }
}

#[test]
fn empty_completed_group_page_has_no_continuation() {
    let runtime = host(VectorMemory::default());
    let (db, mut jobs) = create(&runtime, &[Some(1)], 0);
    let query = || db.query::<Row>().filter_groups(Comparison::Gt, 10_u64);
    let handle = query()
        .start_group_count::<Flag>(&mut jobs, time(0), time(100))
        .unwrap();
    let step = query()
        .group_page(1, None)
        .test_output_limit(1)
        .resume_group_count::<Flag>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert_eq!(step.output_bytes, 0);
    let JobOutcome::Complete(page) = step.outcome else {
        panic!("missing completion")
    };
    assert!(page.rows.is_empty());
    assert!(page.next.is_none());
}

#[test]
fn retained_extrema_cross_chunks_retries_and_reopen_without_prefix_results() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(&runtime, &[None, Some(4), Some(-8), Some(99), Some(-8)], 0);
    let min = db
        .query::<Row>()
        .start_min::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let max = db
        .query::<Row>()
        .start_max::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    for (handle, maximum) in [(&min, false), (&max, true)] {
        let query = db.query::<Row>().work_limit(2);
        let step = if maximum {
            query.resume_max::<Amount>(&mut jobs, handle, 0, time(1))
        } else {
            query.resume_min::<Amount>(&mut jobs, handle, 0, time(1))
        }
        .unwrap();
        assert_eq!((step.sequence, step.examined, step.output_bytes), (1, 2, 0));
        assert!(matches!(step.outcome, JobOutcome::Running));
        let retry = db
            .query::<Row>()
            .resume_min::<Amount>(&mut jobs, &min, 0, time(1))
            .unwrap();
        assert_eq!((retry.sequence, retry.examined), (1, 2));
        assert!(matches!(retry.outcome, JobOutcome::Running));
    }
    drop((db, jobs, runtime));
    let runtime = host(backing);
    let mut db = Database::open(
        runtime
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap(),
        schema(),
    )
    .unwrap();
    let mut jobs = AggregateJobs::open(
        runtime
            .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
            .unwrap(),
    )
    .unwrap();
    for (handle, maximum, expected) in [(&min, false, -8), (&max, true, 99)] {
        let query = db.query::<Row>().limit(1);
        let step = if maximum {
            query.resume_max::<Amount>(&mut jobs, handle, 1, time(2))
        } else {
            query.resume_min::<Amount>(&mut jobs, handle, 1, time(2))
        }
        .unwrap();
        assert_eq!((step.examined, step.output_bytes), (5, 17));
        let JobOutcome::Complete(Some(value)) = step.outcome else {
            panic!("missing extremum")
        };
        assert_eq!(value.amount, Some(expected));
    }
    let mut batch = Batch::new();
    batch
        .insert(&Row {
            id: 5,
            flag: 0,
            amount: Some(-999),
            payload: vec![],
        })
        .unwrap();
    db.commit(batch).unwrap();
    let step = db
        .query::<Row>()
        .resume_min::<Amount>(&mut jobs, &min, 2, time(3))
        .unwrap();
    let JobOutcome::Complete(Some(value)) = step.outcome else {
        panic!("lost historical result")
    };
    assert_eq!(value.amount, Some(-8));
    assert!(matches!(
        db.query::<Row>()
            .resume_min::<Amount>(&mut jobs, &min, 2, time(100))
            .unwrap()
            .outcome,
        JobOutcome::Expired
    ));
}

#[test]
fn retained_extrema_empty_absent_zero_and_extreme_integers_are_distinct() {
    for (values, expected_min, expected_max) in [
        (vec![], None, None),
        (vec![None, None], None, None),
        (vec![None, Some(0)], Some(0), Some(0)),
        (
            vec![Some(i128::MAX), Some(i128::MIN)],
            Some(i128::MIN),
            Some(i128::MAX),
        ),
    ] {
        let runtime = host(VectorMemory::default());
        let (db, mut jobs) = create(&runtime, &values, 0);
        let min = db
            .query::<Row>()
            .start_min::<Amount>(&mut jobs, time(0), time(100))
            .unwrap();
        let max = db
            .query::<Row>()
            .start_max::<Amount>(&mut jobs, time(0), time(100))
            .unwrap();
        for (step, expected) in [
            (
                db.query::<Row>()
                    .resume_min::<Amount>(&mut jobs, &min, 0, time(1))
                    .unwrap(),
                expected_min,
            ),
            (
                db.query::<Row>()
                    .resume_max::<Amount>(&mut jobs, &max, 0, time(1))
                    .unwrap(),
                expected_max,
            ),
        ] {
            let JobOutcome::Complete(value) = step.outcome else {
                panic!("unfinished extremum")
            };
            assert_eq!(value.map(|r| r.amount.unwrap()), expected);
        }
    }
}

#[test]
fn retained_extrema_bind_operation_field_and_query_and_preserve_refused_progress() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (mut db, mut jobs) = create(&runtime, &[Some(4), Some(-8)], 0);
    let handle = db
        .query::<Row>()
        .start_min::<Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .work_limit(1)
        .resume_min::<Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .resume_max::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_min::<Id>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_sum::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .filter("flag", Comparison::Eq, 0_u8)
            .resume_min::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .test_output_limit(16)
            .resume_min::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Row>()
            .group_page(1, None)
            .resume_min::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut row = db.get::<Row>(0).unwrap().unwrap();
    row.amount = Some(3);
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    assert!(matches!(
        db.query::<Row>()
            .resume_min::<Amount>(&mut jobs, &handle, 0, time(2))
            .unwrap()
            .outcome,
        JobOutcome::Invalidated
    ));
}

#[test]
fn retained_text_extrema_restore_owned_encoding_and_replace_winners() {
    let runtime = host(VectorMemory::default());
    let (mut db, mut jobs) = create_stores(&runtime, Schema::new(&[LabelRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    // A length prefix must not determine ordering; embedded NUL must not truncate it.
    for (id, label) in ["z", "aa", "a\0z", "a\0a", "é"].into_iter().enumerate() {
        batch
            .insert(&LabelRow {
                id: id as u64,
                label: label.into(),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let min = db
        .query::<LabelRow>()
        .start_min::<Label>(&mut jobs, time(0), time(100))
        .unwrap();
    let max = db
        .query::<LabelRow>()
        .start_max::<Label>(&mut jobs, time(0), time(100))
        .unwrap();
    for sequence in 0..6 {
        let a = db
            .query::<LabelRow>()
            .work_limit(1)
            .resume_min::<Label>(&mut jobs, &min, sequence, time(1))
            .unwrap();
        let b = db
            .query::<LabelRow>()
            .work_limit(1)
            .resume_max::<Label>(&mut jobs, &max, sequence, time(1))
            .unwrap();

        match (a.outcome, b.outcome) {
            (JobOutcome::Running, JobOutcome::Running) => {}
            (JobOutcome::Complete(Some(a)), JobOutcome::Complete(Some(b))) => {
                assert_eq!(a.label, "a\0a");
                assert_eq!(b.label, "é");

                return;
            }
            _ => panic!("inconsistent extrema progress"),
        }
    }
    panic!("extrema did not complete");
}

#[test]
fn retained_extrema_corrupt_winners_refuse_without_repair_and_remain_cancellable() {
    for corruption in 0..3 {
        let backing = VectorMemory::default();
        let runtime = host(backing.clone());
        let (db, mut jobs) = create(&runtime, &[Some(4), Some(3)], 0);
        let handle = db
            .query::<Row>()
            .start_min::<Amount>(&mut jobs, time(0), time(100))
            .unwrap();
        db.query::<Row>()
            .resume_min::<Amount>(&mut jobs, &handle, 0, time(1))
            .unwrap();
        drop(jobs);
        let key = StableKey::parse("test.jobs.v1").unwrap();
        let raw = runtime.open_memory(&key).unwrap();
        let mut record = raw_frame(&raw, 1);
        // One optional i128 winner: 17 payload bytes after its u32 byte count.
        let payload = record.len() - 17;
        match corruption {
            0 => record[payload] = 0, // An absent operand can never be a winner.
            1 => record[payload] = 2, // Invalid optional tag.
            _ => record[payload - 4 - 8] = 2, // At most one retained winner.
        }
        set_raw_frame(&raw, 1, &record);
        drop(raw);
        let mut jobs = AggregateJobs::open(runtime.open_memory(&key).unwrap()).unwrap();
        let before = backing.borrow().clone();
        assert!(matches!(
            db.query::<Row>()
                .resume_min::<Amount>(&mut jobs, &handle, 1, time(2)),
            Err(Error::Corrupt)
        ));
        assert_eq!(*backing.borrow(), before);
        jobs.cancel(&handle).unwrap();
    }
}

///
/// WideLabelRow
/// Stores `label` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "label", value(item(prim = "Text", max_bytes = 4000)))
    )
)]
struct WideLabelRow {}

///
/// WideLabel
/// Carries the `Text` value of `label`.
///

#[record(fields(field(name = "label", value(item(prim = "Text", max_bytes = 4000)))))]
struct WideLabel {}

#[test]
fn retained_extrema_state_caps_and_final_output_budgets_are_separate() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (mut db, mut jobs) = create_stores(&runtime, Schema::new(&[WideLabelRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    for (id, label) in [(0, "a".to_string()), (1, "z".repeat(3990))] {
        batch.insert(&WideLabelRow { id, label }).unwrap();
    }
    db.commit(batch).unwrap();
    let max = db
        .query::<WideLabelRow>()
        .start_max::<WideLabel>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<WideLabelRow>()
        .work_limit(1)
        .resume_max::<WideLabel>(&mut jobs, &max, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<WideLabelRow>()
            .test_output_limit(5)
            .resume_max::<WideLabel>(&mut jobs, &max, 1, time(2)),
        Err(Error::RowTooLarge)
    ));
    assert_eq!(*backing.borrow(), before);
    // A near-row-sized winner now fits with its progress metadata; row bounds
    // no longer accidentally constrain the larger enclosing job record.
    let step = db
        .query::<WideLabelRow>()
        .resume_max::<WideLabel>(&mut jobs, &max, 1, time(2))
        .unwrap();
    let JobOutcome::Complete(Some(value)) = step.outcome else {
        panic!("missing maximum")
    };
    assert_eq!(value.label, "z".repeat(3990));
    // The large losing value does not need a durable copy for minimum.
    let min = db
        .query::<WideLabelRow>()
        .start_min::<WideLabel>(&mut jobs, time(0), time(100))
        .unwrap();
    let step = db
        .query::<WideLabelRow>()
        .test_output_limit(5)
        .resume_min::<WideLabel>(&mut jobs, &min, 0, time(2))
        .unwrap();
    let JobOutcome::Complete(Some(value)) = step.outcome else {
        panic!("missing minimum")
    };
    assert_eq!(value.label, "a");

    // Output budgets apply at completion, not to provisional winners.
    let runtime = host(VectorMemory::default());
    let (mut db, mut jobs) = create_stores(&runtime, Schema::new(&[LabelRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    for (id, label) in [(0, "z".repeat(1400)), (1, "a".into())] {
        batch.insert(&LabelRow { id, label }).unwrap();
    }
    db.commit(batch).unwrap();
    let min = db
        .query::<LabelRow>()
        .test_output_limit(5)
        .start_min::<Label>(&mut jobs, time(0), time(100))
        .unwrap();
    let first = db
        .query::<LabelRow>()
        .work_limit(1)
        .test_output_limit(5)
        .resume_min::<Label>(&mut jobs, &min, 0, time(1))
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    let step = db
        .query::<LabelRow>()
        .test_output_limit(5)
        .resume_min::<Label>(&mut jobs, &min, 1, time(2))
        .unwrap();
    let JobOutcome::Complete(Some(value)) = step.outcome else {
        panic!("missing final minimum")
    };
    assert_eq!(value.label, "a");
}

#[test]
fn retained_group_extrema_match_independent_winners_across_reopen_and_pages() {
    use ic_db::GroupOrder;
    use std::collections::BTreeMap;
    let values = (0..521)
        .map(|i| {
            if i % 2 == 0 {
                None
            } else {
                Some(i128::from(i % 19) - 9)
            }
        })
        .collect::<Vec<_>>();
    for maximum in [false, true] {
        let mut expected = BTreeMap::<u8, Option<i128>>::new();
        for (id, value) in values.iter().enumerate() {
            let winner = expected.entry((id % 2) as u8).or_default();
            if let Some(value) = value {
                *winner = Some(winner.map_or(*value, |old| {
                    if maximum {
                        old.max(*value)
                    } else {
                        old.min(*value)
                    }
                }));
            }
        }
        let backing = VectorMemory::default();
        let runtime = host(backing.clone());
        let (db, mut jobs) = create(&runtime, &values, 0);
        let query = db
            .query::<Row>()
            .order_groups(GroupOrder::AggregateAscending);
        let handle = if maximum {
            query.start_group_max::<Flag, Amount>(&mut jobs, time(0), time(100))
        } else {
            query.start_group_min::<Flag, Amount>(&mut jobs, time(0), time(100))
        }
        .unwrap();
        let query = db
            .query::<Row>()
            .order_groups(GroupOrder::AggregateAscending)
            .work_limit(17);
        let first = if maximum {
            query.resume_group_max::<Flag, Amount>(&mut jobs, &handle, 0, time(1))
        } else {
            query.resume_group_min::<Flag, Amount>(&mut jobs, &handle, 0, time(1))
        }
        .unwrap();
        assert_eq!((first.examined, first.sequence), (17, 1));
        assert!(matches!(first.outcome, JobOutcome::Running));
        drop((db, jobs, runtime));
        let runtime = host(backing);
        let mut db = Database::open(
            runtime
                .open_memory(&StableKey::parse("test.db.v1").unwrap())
                .unwrap(),
            schema(),
        )
        .unwrap();
        let mut jobs = AggregateJobs::open(
            runtime
                .open_memory(&StableKey::parse("test.jobs.v1").unwrap())
                .unwrap(),
        )
        .unwrap();
        let mut sequence = 1;
        let first_page = loop {
            let query = db
                .query::<Row>()
                .order_groups(GroupOrder::AggregateAscending)
                .work_limit(73)
                .group_page(1, None);
            let step = if maximum {
                query.resume_group_max::<Flag, Amount>(&mut jobs, &handle, sequence, time(2))
            } else {
                query.resume_group_min::<Flag, Amount>(&mut jobs, &handle, sequence, time(2))
            }
            .unwrap();
            sequence = step.sequence;

            match step.outcome {
                JobOutcome::Running => assert!(step.examined < 522),
                JobOutcome::Complete(page) => {
                    assert_eq!(step.examined, 521);
                    break page;
                }
                _ => panic!("unexpected job state"),
            }
        };
        assert_eq!(first_page.rows.len(), 1);
        assert_eq!(first_page.rows[0].key.flag, 0);
        assert!(first_page.rows[0].value.is_none());
        let cursor = first_page.next.unwrap();
        let mut row = db.get::<Row>(1).unwrap().unwrap();
        row.amount = Some(999);
        let mut batch = Batch::new();
        batch.replace(&row).unwrap();
        db.commit(batch).unwrap();
        for _ in 0..2 {
            let query = db
                .query::<Row>()
                .order_groups(GroupOrder::AggregateAscending)
                .group_page(1, Some(&cursor));
            let step = if maximum {
                query.resume_group_max::<Flag, Amount>(&mut jobs, &handle, sequence, time(3))
            } else {
                query.resume_group_min::<Flag, Amount>(&mut jobs, &handle, sequence, time(3))
            }
            .unwrap();
            assert_eq!(step.sequence, sequence);
            let JobOutcome::Complete(page) = step.outcome else {
                panic!("missing completed page")
            };
            assert!(page.next.is_none());
            assert_eq!(page.rows.len(), 1);
            assert_eq!(page.rows[0].key.flag, 1);
            assert_eq!(page.rows[0].value.as_ref().unwrap().amount, expected[&1]);
        }
    }
}

#[test]
fn retained_group_extrema_filter_order_ties_and_byte_pages_use_final_winners() {
    use ic_db::GroupOrder;
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (mut db, mut jobs) = create(&runtime, &[], 0);
    let mut batch = Batch::new();
    for (id, flag, amount) in [
        (0, 0, Some(1)),
        (1, 1, Some(5)),
        (2, 2, Some(-1)),
        (3, 3, None),
        (4, 0, Some(5)),
    ] {
        batch
            .insert(&Row {
                id,
                flag,
                amount,
                payload: vec![],
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let query = || {
        db.query::<Row>()
            .filter_groups(Comparison::Ge, 0_i128)
            .order_groups(GroupOrder::AggregateDescending)
            .limit(2)
    };
    let handle = query()
        .start_group_max::<Flag, Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        query()
            .group_page(2, None)
            .test_output_limit(17)
            .resume_group_max::<Flag, Amount>(&mut jobs, &handle, 0, time(1)),
        Err(Error::RowTooLarge)
    ));
    assert_eq!(*backing.borrow(), before);
    let step = query()
        .group_page(2, None)
        .test_output_limit(18)
        .resume_group_max::<Flag, Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let JobOutcome::Complete(page) = step.outcome else {
        panic!("missing maxima")
    };
    assert_eq!(page.rows.len(), 1);
    assert_eq!(
        (
            page.rows[0].key.flag,
            page.rows[0].value.as_ref().unwrap().amount
        ),
        (0, Some(5))
    );
    let next = page.next.unwrap();
    let step = query()
        .group_page(2, Some(&next))
        .test_output_limit(18)
        .resume_group_max::<Flag, Amount>(&mut jobs, &handle, step.sequence, time(2))
        .unwrap();
    let JobOutcome::Complete(page) = step.outcome else {
        panic!("missing second page")
    };
    assert_eq!(
        (
            page.rows[0].key.flag,
            page.rows[0].value.as_ref().unwrap().amount
        ),
        (1, Some(5))
    );
    assert!(page.next.is_none());
}

#[test]
fn retained_group_extrema_empty_input_absent_and_overlapping_keys() {
    for values in [vec![], vec![None, Some(0), None, Some(-4), Some(-4)]] {
        let runtime = host(VectorMemory::default());
        let (db, mut jobs) = create(&runtime, &values, 0);
        for maximum in [false, true] {
            let query = db.query::<Row>();
            let handle = if maximum {
                query.start_group_max::<Amount, Amount>(&mut jobs, time(0), time(100))
            } else {
                query.start_group_min::<Amount, Amount>(&mut jobs, time(0), time(100))
            }
            .unwrap();
            let query = db.query::<Row>();
            let step = if maximum {
                query.resume_group_max::<Amount, Amount>(&mut jobs, &handle, 0, time(1))
            } else {
                query.resume_group_min::<Amount, Amount>(&mut jobs, &handle, 0, time(1))
            }
            .unwrap();
            let JobOutcome::Complete(page) = step.outcome else {
                panic!("incomplete extrema")
            };
            let actual = page
                .rows
                .into_iter()
                .map(|r| (r.key.amount, r.value.and_then(|r| r.amount)))
                .collect::<Vec<_>>();
            let expected = values
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|v| (v, v))
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
            assert!(page.next.is_none());
        }
    }
}

#[test]
fn retained_group_extrema_bind_keys_operands_operation_and_selection() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(&runtime, &[Some(7), None, Some(1)], 0);
    let handle = db
        .query::<Row>()
        .start_group_min::<Flag, Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .work_limit(1)
        .resume_group_min::<Flag, Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        db.query::<Row>()
            .resume_group_max::<Flag, Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_min::<Amount, Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_min::<Flag, Id>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_group_sum::<Flag, Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .resume_min::<Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .limit(1)
            .resume_group_min::<Flag, Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .filter_groups(Comparison::Ge, 0_i128)
            .resume_group_min::<Flag, Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::InvalidJob)
    ));
    assert!(matches!(
        db.query::<Row>()
            .group_page(1, None)
            .start_group_min::<Flag, Amount>(&mut jobs, time(0), time(100)),
        Err(Error::InvalidQuery)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn retained_group_extrema_cardinality_and_frame_caps_ignore_output_selection() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (db, mut jobs) = create(&runtime, &vec![Some(1); 33], 0);
    let query = || {
        db.query::<Row>()
            .filter_groups(Comparison::Gt, 99_i128)
            .limit(1)
    };
    let handle = query()
        .start_group_min::<Id, Amount>(&mut jobs, time(0), time(100))
        .unwrap();
    query()
        .work_limit(32)
        .resume_group_min::<Id, Amount>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        query().resume_group_min::<Id, Amount>(&mut jobs, &handle, 1, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);

    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let (mut db, mut jobs) = create_stores(&runtime, Schema::new(&[LabelRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    for id in 0..12 {
        batch
            .insert(&LabelRow {
                id,
                label: "z".repeat(1400),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let query = || {
        db.query::<LabelRow>()
            .filter_groups(Comparison::Eq, "excluded")
            .limit(1)
    };
    let handle = query()
        .start_group_max::<Id, Label>(&mut jobs, time(0), time(100))
        .unwrap();
    query()
        .work_limit(11)
        .resume_group_max::<Id, Label>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(
        query().resume_group_max::<Id, Label>(&mut jobs, &handle, 1, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn retained_group_extrema_corrupt_winner_framing_refuses_intact() {
    for corruption in 0..3 {
        let backing = VectorMemory::default();
        let runtime = host(backing.clone());
        let (db, mut jobs) = create(&runtime, &[Some(4)], 0);
        let handle = db
            .query::<Row>()
            .start_group_min::<Flag, Amount>(&mut jobs, time(0), time(100))
            .unwrap();
        db.query::<Row>()
            .resume_group_min::<Flag, Amount>(&mut jobs, &handle, 0, time(1))
            .unwrap();
        drop(jobs);
        let key = StableKey::parse("test.jobs.v1").unwrap();
        let raw = runtime.open_memory(&key).unwrap();
        let mut record = raw_frame(&raw, 1);
        let winner = record.len() - 17;
        match corruption {
            0 => record[winner] = 0,
            1 => record[winner] = 2,
            _ => record[winner - 5] = 2,
        }
        set_raw_frame(&raw, 1, &record);
        drop(raw);
        let mut jobs = AggregateJobs::open(runtime.open_memory(&key).unwrap()).unwrap();
        let before = backing.borrow().clone();
        assert!(matches!(
            db.query::<Row>()
                .resume_group_min::<Flag, Amount>(&mut jobs, &handle, 1, time(2)),
            Err(Error::Corrupt)
        ));
        assert_eq!(*backing.borrow(), before);
        jobs.cancel(&handle).unwrap();
    }
}

#[test]
fn retained_group_text_extrema_restore_winners_and_apply_completed_order() {
    use ic_db::GroupOrder;
    let runtime = host(VectorMemory::default());
    let (mut db, mut jobs) = create_stores(&runtime, Schema::new(&[LabelRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    for (id, label) in ["z", "aa", "a\0z", "a\0a", "é"].into_iter().enumerate() {
        batch
            .insert(&LabelRow {
                id: id as u64,
                label: label.into(),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let query = || {
        db.query::<LabelRow>()
            .order_groups(GroupOrder::AggregateAscending)
    };
    let handle = query()
        .start_group_min::<Id, Label>(&mut jobs, time(0), time(100))
        .unwrap();
    for sequence in 0..6 {
        let step = query()
            .work_limit(1)
            .resume_group_min::<Id, Label>(&mut jobs, &handle, sequence, time(1))
            .unwrap();

        match step.outcome {
            JobOutcome::Running => {}
            JobOutcome::Complete(page) => {
                let actual = page
                    .rows
                    .into_iter()
                    .map(|r| (r.key.id, r.value.unwrap().label))
                    .collect::<Vec<_>>();
                assert_eq!(
                    actual,
                    [(3, "a\0a"), (2, "a\0z"), (1, "aa"), (0, "z"), (4, "é")]
                        .into_iter()
                        .map(|(k, v)| (k, v.to_string()))
                        .collect::<Vec<_>>()
                );

                return;
            }
            _ => panic!("unexpected group state"),
        }
    }
    panic!("groups did not complete");
}

///
/// UnitPrice
/// Gives `Decimal` a distinct schema identity.
///

#[newtype(item(prim = "Decimal", scale = 2))]
struct UnitPrice {}

///
/// Quantity
/// Gives `Int16` a distinct schema identity.
///

#[newtype(item(prim = "Int16"))]
struct Quantity {}

///
/// Line
/// Stores `flag`, `price` and `amount` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "flag", value(item(prim = "Nat8"))),
        field(name = "price", value(item(is = "UnitPrice"))),
        field(name = "amount", value(opt, item(is = "Quantity")))
    )
)]
struct Line {}

#[test]
fn numeric_newtype_inputs_share_widening_and_retained_decimal_rounding() {
    let runtime = host(VectorMemory::default());
    let (mut db, mut jobs) = create_stores(&runtime, Schema::new(&[Line::SCHEMA]).unwrap());
    let decimal = |n| ic_db::Decimal::try_new(n, 2).unwrap();
    let mut batch = Batch::new();
    for (id, flag, cents, quantity) in [
        (1, 0, 124, Some(i16::MAX)),
        (2, 0, 125, Some(1)),
        (3, 1, -100, None),
    ] {
        batch
            .insert(&Line {
                id,
                flag,
                price: UnitPrice(decimal(cents)),
                amount: quantity.map(Quantity),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let groups = db
        .query::<Line>()
        .group_sum::<Flag, Amount>()
        .unwrap()
        .value;
    assert_eq!(
        groups
            .into_iter()
            .map(|g| (g.key.flag, g.value.and_then(|r| r.amount)))
            .collect::<Vec<_>>(),
        [(0, Some(32768)), (1, None)]
    );
    let query = || db.query::<Line>().filter("flag", Comparison::Eq, 0_u8);
    assert_eq!(
        query().sum::<Price>().unwrap().value.unwrap().price,
        decimal(249)
    );
    assert!(matches!(
        query().sum::<WrongScale>(),
        Err(Error::InvalidQuery)
    ));
    let expected = Price {
        price: decimal(124),
    };
    assert_eq!(
        query().avg::<Price>().unwrap().value,
        Some(expected.clone())
    );
    let handle = query()
        .start_avg::<Price>(&mut jobs, time(0), time(100))
        .unwrap();
    let first = query()
        .work_limit(1)
        .resume_avg::<Price>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert!(matches!(first.outcome, JobOutcome::Running));
    let rest = query()
        .resume_avg::<Price>(&mut jobs, &handle, first.sequence, time(2))
        .unwrap();
    assert!(matches!(rest.outcome, JobOutcome::Complete(Some(value)) if value == expected));
}

///
/// NamedPrice
/// Carries the `UnitPrice` value of `price`.
///

#[record(fields(field(name = "price", value(item(is = "UnitPrice")))))]
struct NamedPrice {}

///
/// Discount
/// Gives `Decimal` a distinct schema identity.
///

#[newtype(item(prim = "Decimal", scale = 2))]
struct Discount {}

///
/// WrongUnit
/// Carries the `Discount` value of `price`.
///

#[record(fields(field(name = "price", value(item(is = "Discount")))))]
struct WrongUnit {}

///
/// OtherScale
/// Gives `Decimal` a distinct schema identity.
///

#[newtype(name = "UnitPrice", item(prim = "Decimal", scale = 3))]
struct OtherScale {}

///
/// ChangedUnit
/// Carries the `OtherScale` value of `price`.
///

#[record(fields(field(name = "price", value(item(is = "OtherScale")))))]
struct ChangedUnit {}

///
/// NamedAmount
/// Carries an optional `Quantity` value of `amount`.
///

#[record(fields(field(name = "amount", value(opt, item(is = "Quantity")))))]
struct NamedAmount {}

#[test]
fn numeric_results_preserve_exact_units_across_job_steps() {
    let runtime = host(VectorMemory::default());
    let (mut db, mut jobs) = create_stores(&runtime, Schema::new(&[Line::SCHEMA]).unwrap());
    let price = |cents| UnitPrice(ic_db::Decimal::try_new(cents, 2).unwrap());
    let mut batch = Batch::new();
    for (id, cents, amount) in [(1, 124, i16::MAX), (2, 125, 1)] {
        batch
            .insert(&Line {
                id,
                flag: 0,
                price: price(cents),
                amount: Some(Quantity(amount)),
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let query = || db.query::<Line>();
    assert_eq!(
        query().sum::<NamedPrice>().unwrap().value.unwrap().price,
        price(249)
    );
    assert!(matches!(
        query().sum::<NamedAmount>(),
        Err(Error::ArithmeticOverflow)
    ));
    assert!(matches!(
        query().avg::<WrongUnit>(),
        Err(Error::InvalidQuery)
    ));
    assert!(matches!(
        query().avg::<ChangedUnit>(),
        Err(Error::InvalidQuery)
    ));
    let groups = query().group_avg::<Flag, NamedPrice>().unwrap().value;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].value.as_ref().unwrap().price, price(124));

    let handle = query()
        .start_avg::<NamedPrice>(&mut jobs, time(0), time(100))
        .unwrap();
    let step = query()
        .work_limit(1)
        .resume_avg::<NamedPrice>(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert!(matches!(step.outcome, JobOutcome::Running));
    assert!(matches!(
        query().resume_avg::<Price>(&mut jobs, &handle, step.sequence, time(2)),
        Err(Error::InvalidJob)
    ));
    let last = query()
        .resume_avg::<NamedPrice>(&mut jobs, &handle, step.sequence, time(2))
        .unwrap();
    assert!(matches!(last.outcome, JobOutcome::Complete(Some(value)) if value.price == price(124)));
}

#[cfg(feature = "metrics")]
#[test]
fn metrics_job_calls_keep_lifecycle_separate_from_synchronous_queries() {
    let runtime = host(VectorMemory::default());
    let (db, mut jobs) = create_stores(&runtime, schema());
    let handle = db
        .query::<Row>()
        .start_count(&mut jobs, time(0), time(100))
        .unwrap();
    db.query::<Row>()
        .resume_count(&mut jobs, &handle, 0, time(1))
        .unwrap();
    assert!(
        db.query::<Row>()
            .resume_count(&mut jobs, &handle, 99, time(2))
            .is_err()
    );
    let totals = ic_db::metrics::snapshot();
    assert_eq!(totals.job_starts.total(), 1);
    assert_eq!(totals.job_resumes.total(), 2);
    assert_eq!(totals.job_failures.total(), 1);
    assert_eq!(totals.queries.total(), 0);
}
