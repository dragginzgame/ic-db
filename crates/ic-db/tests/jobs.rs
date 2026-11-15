//! Exact retained aggregates: independent results, retries, invalidation and reclamation.
use ic_db::{
    AggregateJobs, Batch, Comparison, Database, Entity, Error, JobHandle, JobOutcome, Schema,
    Timestamp, entity,
};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory,
};

#[entity(
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
#[entity(
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
struct Other {}
#[ic_db::record(fields(field(name = "amount", value(opt, item(prim = "Int128")))))]
struct Amount {}
#[ic_db::record(fields(field(name = "id", value(item(prim = "Nat64")))))]
struct Id {}
fn time(ms: i64) -> Timestamp {
    Timestamp::from_millis(ms)
}
fn host(backing: VectorMemory) -> MemoryRuntime<VectorMemory> {
    let mut host = MemoryRuntime::new(backing).unwrap();
    let declarations = SealedDeclarationSnapshot::new(vec![
        MemoryRequest::new("test", "test.db.v1").unwrap(),
        MemoryRequest::new("test", "test.jobs.v1").unwrap(),
    ])
    .unwrap();
    let grants =
        AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap();
    host.bootstrap(&declarations, &grants, &GenericAllocationPolicy)
        .unwrap();
    host
}
fn schema() -> Schema {
    Schema::new(&[Row::SCHEMA, Other::SCHEMA]).unwrap()
}
fn create_stores(
    host: &MemoryRuntime<VectorMemory>,
    schema: Schema,
) -> (Database<VectorMemory>, AggregateJobs<VectorMemory>) {
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
) -> (Database<VectorMemory>, AggregateJobs<VectorMemory>) {
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
fn count_and_sum_resume_beyond_synchronous_budget_without_prefix_results() {
    let host = host(VectorMemory::default());
    let (db, mut jobs) = create(&host, &vec![Some(7); 520], 0);
    assert!(matches!(
        db.query::<Row>().work_limit(512).count(),
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
    let (db, mut jobs) = create(&host, &vec![Some(1); 64], 3500);
    let query = || {
        db.query::<Row>()
            .using_index("flag", &[0_u8.into()])
            .filter("id", Comparison::Ge, 60_u64)
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
    let step = db
        .query::<Row>()
        .resume_sum::<Amount>(
            &mut jobs,
            &JobHandle::from_bytes(&handle).unwrap(),
            1,
            time(2),
        )
        .unwrap();
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
            .byte_limit(1)
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
fn unsupported_store_and_malformed_job_refuse_without_cleanup_damage() {
    use ic_storage::adapter::StableBTreeMap;
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
    let mut raw: StableBTreeMap<u8, Vec<u8>, _> =
        StableBTreeMap::load(host.open_memory(&key).unwrap()).unwrap();
    raw.insert(2, vec![255]);
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
    let mut raw: StableBTreeMap<u8, Vec<u8>, _> =
        StableBTreeMap::load(host.open_memory(&key).unwrap()).unwrap();
    let mut header = raw.get(&0).unwrap();
    header[0] ^= 1;
    raw.insert(0, header);
    drop(raw);
    let before = backing.borrow().clone();
    assert!(AggregateJobs::open(host.open_memory(&key).unwrap()).is_err());
    assert_eq!(*backing.borrow(), before);
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "price", value(item(prim = "Decimal", scale = 2))),
        field(name = "wide", value(item(prim = "U256")))
    )
)]
struct NumericRow {}
#[ic_db::record(fields(field(name = "price", value(item(prim = "Decimal", scale = 2)))))]
struct Price {}
#[ic_db::record(fields(field(name = "price", value(item(prim = "Decimal", scale = 3)))))]
struct WrongScale {}
#[ic_db::record(fields(field(name = "wide", value(item(prim = "U256")))))]
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

#[ic_db::record(fields(field(name = "amount", value(opt, item(prim = "Decimal", scale = 0)))))]
struct Average0 {}
#[ic_db::record(fields(field(name = "amount", value(opt, item(prim = "Decimal", scale = 2)))))]
struct Average2 {}
#[ic_db::record(fields(field(name = "amount", value(opt, item(prim = "Decimal", scale = 28)))))]
struct Average28 {}
#[ic_db::record(fields(field(name = "wide", value(item(prim = "Decimal", scale = 0)))))]
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
            .byte_limit(1)
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

#[ic_db::record(fields(field(name = "flag", value(item(prim = "Nat8")))))]
struct Flag {}
#[ic_db::record(fields(
    field(name = "amount", value(opt, item(prim = "Int128"))),
    field(name = "flag", value(item(prim = "Nat8")))
))]
struct GroupKey {}

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
                        rows.into_iter()
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
        rows.into_iter()
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
        rows.into_iter()
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
            .byte_limit(8)
            .resume_group_count::<Flag>(&mut jobs, &handle, 1, time(2)),
        Err(Error::RowTooLarge)
    ));
    assert!(matches!(
        db.query::<Row>()
            .byte_limit(9)
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
        rows.into_iter()
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

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "label", value(item(prim = "Text", max_bytes = 1500)))
    )
)]
struct LabelRow {}
#[ic_db::record(fields(field(name = "label", value(item(prim = "Text", max_bytes = 1500)))))]
struct Label {}
#[test]
fn grouped_job_frame_bound_is_independent_of_output_and_synchronous_state_limits() {
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (mut db, mut jobs) = create_stores(&host, Schema::new(&[LabelRow::SCHEMA]).unwrap());
    let mut batch = Batch::new();
    for (id, c) in ['a', 'b', 'c'].into_iter().enumerate() {
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
        3
    );
    let query = || db.query::<LabelRow>().limit(1);
    let handle = query()
        .start_group_count::<Label>(&mut jobs, time(0), time(100))
        .unwrap();
    for sequence in 0..2 {
        query()
            .work_limit(1)
            .resume_group_count::<Label>(&mut jobs, &handle, sequence, time(1))
            .unwrap();
    }
    let before = backing.borrow().clone();
    assert!(matches!(
        query().resume_group_count::<Label>(&mut jobs, &handle, 2, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    let retry = query()
        .resume_group_count::<Label>(&mut jobs, &handle, 1, time(1))
        .unwrap();
    assert_eq!((retry.sequence, retry.examined), (2, 2));
    jobs.cancel(&handle).unwrap();
}

#[test]
fn grouped_jobs_handle_empty_sparse_and_corrupt_checkpoints() {
    use ic_storage::adapter::StableBTreeMap;
    let backing = VectorMemory::default();
    let host = host(backing.clone());
    let (db, mut jobs) = create(&host, &[Some(1); 8], 0);
    let query = || {
        db.query::<Row>()
            .using_index("flag", &[0_u8.into()])
            .filter("id", Comparison::Ge, 6_u64)
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
        rows.into_iter()
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
    assert!(rows.is_empty());
    drop(jobs);
    let key = StableKey::parse("test.jobs.v1").unwrap();
    let mut raw: StableBTreeMap<u8, Vec<u8>, _> =
        StableBTreeMap::load(host.open_memory(&key).unwrap()).unwrap();
    let mut record = raw.get(&1).unwrap();
    *record.last_mut().unwrap() ^= 1;
    raw.insert(1, record);
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
                    s.into_iter()
                        .map(|r| (r.key.flag, r.value.and_then(|v| v.amount)))
                        .collect::<Vec<_>>(),
                    vec![(0, None), (1, Some(348)), (2, Some(1218))]
                );
                assert_eq!(
                    a.into_iter()
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
                        assert_eq!(rows.len(), 1);
                        let row = &rows[0];
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
    assert_eq!(s[0].value.as_ref().unwrap().amount, Some(1));
    assert!(s[1].value.is_none());
    assert_eq!(
        a[0].value.as_ref().unwrap().amount.unwrap().to_string(),
        "0.2"
    );
    assert!(a[1].value.is_none());
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
            .byte_limit(1)
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
    for sequence in 0..4 {
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
        .resume_group_sum::<Price, Wide>(&mut jobs, &sum, 4, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing wide sums")
    };
    assert_eq!(
        s.into_iter()
            .map(|r| r.value.unwrap().wide)
            .collect::<Vec<_>>(),
        vec![large, large * 2_u128 + U256::ONE, large + U256::ONE]
    );
    let JobOutcome::Complete(a) = db
        .query::<NumericRow>()
        .resume_group_avg::<Wide, Price>(&mut jobs, &avg, 4, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing decimal averages")
    };
    assert_eq!(
        a.into_iter()
            .map(|r| r.value.unwrap().price.to_string())
            .collect::<Vec<_>>(),
        vec!["1.24", "1.26"]
    );
    let JobOutcome::Complete(s) = db
        .query::<NumericRow>()
        .resume_group_sum::<Wide, Price>(&mut jobs, &decimal_sum, 4, time(2))
        .unwrap()
        .outcome
    else {
        panic!("missing decimal sums")
    };
    assert_eq!(
        s.into_iter()
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
    for (id, c) in ['a', 'b', 'c'].into_iter().enumerate() {
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
    for sequence in 0..2 {
        query()
            .work_limit(1)
            .resume_group_sum::<Label, Id>(&mut jobs, &handle, sequence, time(1))
            .unwrap();
    }
    let before = backing.borrow().clone();
    assert!(matches!(
        query().resume_group_sum::<Label, Id>(&mut jobs, &handle, 2, time(2)),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    let retry = query()
        .resume_group_sum::<Label, Id>(&mut jobs, &handle, 1, time(1))
        .unwrap();
    assert_eq!((retry.sequence, retry.examined), (2, 2));
}

#[test]
fn grouped_numeric_corrupt_absent_total_refuses_and_remains_cancellable() {
    use ic_storage::adapter::StableBTreeMap;
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
    let mut raw: StableBTreeMap<u8, Vec<u8>, _> =
        StableBTreeMap::load(host.open_memory(&key).unwrap()).unwrap();
    let mut record = raw.get(&1).unwrap();
    *record.last_mut().unwrap() = 1; // Nonzero total with a zero present count.
    raw.insert(1, record);
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
