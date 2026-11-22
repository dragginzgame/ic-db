//! Independent target rows/indexes, resumable progress and bounded cancellation.

use ic_db::design::prelude::*;
use ic_db::{
    AggregateJobs, Batch, CopyStatus, Database, Error, JobOutcome, Maintenance, RetiredSource,
    SchemaCopy, SchemaTransition,
};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

///
/// Source
/// Exercises an alternate `Thing` declaration under the same logical identity.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Thing",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat8")))
    )
)]
struct Source {}

///
/// Target
/// Exercises an alternate `Thing` declaration under the same logical identity.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Thing",
    pk(field = "id"),
    index(field = "value", unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat16")))
    )
)]
struct Target {}

///
/// ManagedSource
/// Exercises an alternate `Thing` declaration under the same logical identity.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Thing",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "created",
            managed = "created_at",
            value(item(prim = "Timestamp"))
        )
    )
)]
struct ManagedSource {}

///
/// TimedSource
/// Exercises an alternate `Timed` declaration under the same logical identity.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Timed",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat8"))),
        field(
            name = "created",
            managed = "created_at",
            value(item(prim = "Timestamp"))
        ),
        field(
            name = "updated",
            managed = "updated_at",
            value(item(prim = "Timestamp"))
        )
    )
)]
struct TimedSource {}

///
/// TimedTarget
/// Exercises an alternate `Timed` declaration under the same logical identity.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Timed",
    pk(field = "id"),
    index(field = "changed"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "value", value(item(prim = "Nat16"))),
        field(name = "born", managed = "created_at", value(item(prim = "Timestamp"))),
        field(
            name = "changed",
            managed = "updated_at",
            value(item(prim = "Timestamp"))
        )
    )
)]
struct TimedTarget {}

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

fn source() -> Schema {
    Schema::new(&[Source::SCHEMA]).unwrap()
}

fn target() -> Schema {
    Schema::new(&[Target::SCHEMA]).unwrap()
}

fn transition(version: u32) -> SchemaTransition {
    SchemaTransition::new(&source(), &target(), "widen", version, &[7]).unwrap()
}

fn key(name: &str) -> StableKey {
    StableKey::parse(name).unwrap()
}

fn host(memory: VectorMemory) -> MemoryRuntime<VectorMemory> {
    let mut host = MemoryRuntime::new(memory).unwrap();
    host.bootstrap(
        &SealedDeclarationSnapshot::new(
            ["copy.source.v1", "copy.target.v1", "copy.other.v1"]
                .iter()
                .map(|name| MemoryRequest::new("copy", name).unwrap())
                .collect(),
        )
        .unwrap(),
        &AllocationGrants::new(vec![MemoryAuthority::new("copy", "copy.").unwrap()]).unwrap(),
        &GenericAllocationPolicy,
    )
    .unwrap();

    host
}

fn create(host: &MemoryRuntime<VectorMemory>) -> Maintenance {
    let mut db = Database::create(
        host.open_memory(&key("copy.source.v1")).unwrap(),
        source(),
        [1; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    for value in 1..=5 {
        batch
            .insert(&Source {
                id: u64::from(value),
                value,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();

    db.begin_maintenance(&transition(1)).unwrap()
}

fn begin(host: &MemoryRuntime<VectorMemory>) -> SchemaCopy {
    create(host)
        .begin_copy(
            host.open_memory(&key("copy.target.v1")).unwrap(),
            source(),
            target(),
            [2; 16],
        )
        .unwrap()
}

fn resume(host: &MemoryRuntime<VectorMemory>) -> SchemaCopy {
    SchemaCopy::resume(
        host.open_memory(&key("copy.source.v1")).unwrap(),
        host.open_memory(&key("copy.target.v1")).unwrap(),
        source(),
        target(),
        &transition(1),
    )
    .unwrap()
}
// Match the consuming migration boundary, which also permits moving owned fields.
#[expect(clippy::needless_pass_by_value)]
fn transform(row: Source, inputs: &[u8]) -> Result<Target, Error> {
    Ok(Target {
        id: row.id,
        value: u16::from(row.value) + u16::from(inputs[0]),
    })
}

fn entries(host: &MemoryRuntime<VectorMemory>, name: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
    let raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(host.open_memory(&key(name)).unwrap());

    raw.iter()
        .map(|entry| (entry.key().clone(), entry.value()))
        .collect()
}

#[test]
fn publication_refuses_source_cursors_and_jobs_for_an_unchanged_entity() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let source = Schema::new(&[Other::SCHEMA, Source::SCHEMA]).unwrap();
    let target = Schema::new(&[Other::SCHEMA, Target::SCHEMA]).unwrap();
    let transition = SchemaTransition::new(&source, &target, "widen-with-jobs", 1, &[7]).unwrap();
    let mut db = Database::create(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source.clone(),
        [1; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    batch.insert(&Source { id: 1, value: 7 }).unwrap();
    for id in 1..=5 {
        batch.insert(&Other { id }).unwrap();
    }
    db.commit(batch).unwrap();
    let revision = db.revision::<Other>().unwrap();
    let cursor = db.query::<Other>().limit(2).fetch().unwrap().next.unwrap();
    let mut jobs =
        AggregateJobs::create(runtime.open_memory(&key("copy.other.v1")).unwrap(), [3; 16])
            .unwrap();
    let now = Timestamp::from_millis;
    let running = db
        .query::<Other>()
        .start_count(&mut jobs, now(0), now(100))
        .unwrap();
    let first = db
        .query::<Other>()
        .work_limit(2)
        .resume_count(&mut jobs, &running, 0, now(1))
        .unwrap();
    assert_eq!(first.examined, 2);
    assert!(matches!(first.outcome, JobOutcome::Running));
    let completed = db
        .query::<Other>()
        .start_count(&mut jobs, now(0), now(50))
        .unwrap();
    let done = db
        .query::<Other>()
        .resume_count(&mut jobs, &completed, 0, now(1))
        .unwrap();
    assert!(matches!(done.outcome, JobOutcome::Complete(5)));
    drop(jobs);

    let mut copy = db
        .begin_maintenance(&transition)
        .unwrap()
        .begin_copy(
            runtime.open_memory(&key("copy.target.v1")).unwrap(),
            source.clone(),
            target.clone(),
            [2; 16],
        )
        .unwrap();
    assert_eq!(copy.next_entity(), Some("Other"));
    assert_eq!(copy.copy_unchanged::<Other>(32, 4096).unwrap().rows, 5);
    assert!(copy.copy_next(32, 4096, transform).unwrap().complete);
    let mut validated = false;
    for _ in 0..32 {
        if copy.validate_next(2).unwrap() {
            validated = true;
            break;
        }
    }
    assert!(validated);
    drop(copy.publish().unwrap());
    drop(runtime);

    let runtime = host(memory.clone());
    let db = Database::open(runtime.open_memory(&key("copy.target.v1")).unwrap(), target).unwrap();
    let mut jobs =
        AggregateJobs::open(runtime.open_memory(&key("copy.other.v1")).unwrap()).unwrap();
    assert_eq!(db.incarnation(), [2; 16]);
    // The queried entity, rows and numeric revision match. They cannot confer
    // authority to reuse a source-bound position or completed historical result.
    assert_eq!(db.revision::<Other>().unwrap(), revision);
    assert_eq!(
        db.query::<Other>().fetch().unwrap().rows,
        (1..=5).map(|id| Other { id }).collect::<Vec<_>>()
    );
    let before = memory.borrow().clone();
    assert!(matches!(
        db.query::<Other>().after(&cursor).fetch(),
        Err(Error::InvalidCursor)
    ));
    for (handle, sequence) in [(&running, first.sequence), (&completed, done.sequence)] {
        assert!(matches!(
            db.query::<Other>()
                .resume_count(&mut jobs, handle, sequence, now(2)),
            Err(Error::InvalidJob)
        ));
    }
    assert!(matches!(
        Database::open(runtime.open_memory(&key("copy.source.v1")).unwrap(), source),
        Err(Error::Maintenance)
    ));
    assert_eq!(*memory.borrow(), before);

    // Cleanup interprets the supported job frame, not the retired source schema.
    jobs.cancel(&running).unwrap();
    assert_eq!(jobs.cleanup_expired(now(50), 1).unwrap(), 1);
    let fresh = db
        .query::<Other>()
        .start_count(&mut jobs, now(50), now(100))
        .unwrap();
    assert_ne!(fresh, running);
    assert_ne!(fresh, completed);
    let result = db
        .query::<Other>()
        .resume_count(&mut jobs, &fresh, 0, now(51))
        .unwrap();
    assert!(matches!(result.outcome, JobOutcome::Complete(5)));
}

#[test]
fn copy_rebuilds_indexes_and_resumes_from_its_own_checkpoint() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let mut copy = begin(&runtime);
    let step = copy.copy_next::<Source, Target>(4, 18, transform).unwrap();
    assert_eq!((step.rows, step.complete), (2, false));
    drop(copy);
    let original = entries(&runtime, "copy.source.v1");
    for name in ["copy.source.v1", "copy.target.v1"] {
        assert!(matches!(
            Database::open(runtime.open_memory(&key(name)).unwrap(), source()),
            Err(Error::Maintenance)
        ));
        assert!(
            Maintenance::resume(runtime.open_memory(&key(name)).unwrap(), &transition(1)).is_err()
        );
    }
    drop(runtime);
    let runtime = host(memory);
    let mut copy = resume(&runtime);
    let step = copy.copy_next::<Source, Target>(4, 128, transform).unwrap();
    assert_eq!((step.rows, step.complete), (3, true));
    assert_eq!(
        copy.copy_next::<Source, Target>(4, 128, transform)
            .unwrap()
            .rows,
        0
    );
    drop(copy);
    assert_eq!(entries(&runtime, "copy.source.v1"), original);
    let stored = entries(&runtime, "copy.target.v1");
    let mut expected = Vec::new();
    for id in 1..=5_u64 {
        let value = id as u16 + 7;
        let mut row_key = vec![2];
        row_key.extend(Target::SCHEMA.id.to_be_bytes());
        row_key.extend(id.to_be_bytes());
        let mut row = id.to_le_bytes().to_vec();
        row.extend(value.to_le_bytes());
        expected.push((row_key, row));
        let mut index = vec![3];
        index.extend(Target::SCHEMA.id.to_be_bytes());
        index.extend(Target::SCHEMA.indexes[0].id.to_be_bytes());
        index.extend(value.to_be_bytes());
        expected.push((index, id.to_be_bytes().to_vec()));
    }
    expected.sort();
    assert_eq!(
        stored
            .iter()
            .filter(|(key, _)| key[0] >= 2)
            .cloned()
            .collect::<Vec<_>>(),
        expected
    );
    let mut copy = resume(&runtime);
    assert!(!copy.cancel_step(1).unwrap());
    assert_eq!(copy.status(), CopyStatus::Cancelling);
    assert!(matches!(
        copy.copy_next::<Source, Target>(1, 9, transform),
        Err(Error::InvalidTransition)
    ));
    drop(copy);
    let mut copy = resume(&runtime);
    let mut calls = 0;
    assert_eq!(copy.status(), CopyStatus::Cancelling);
    while !copy.cancel_step(2).unwrap() {
        calls += 1;
        assert!(calls < 10);
    }
    assert_eq!(copy.status(), CopyStatus::Cancelled);
    drop(copy);
    assert_eq!(entries(&runtime, "copy.target.v1").len(), 1); // ownership tombstone only
    let db = Database::open(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source(),
    )
    .unwrap();
    assert_eq!(db.revision::<Source>().unwrap(), 1);
    for id in 1..=5 {
        assert_eq!(db.get::<Source>(id).unwrap().unwrap().value, id as u8);
    }
}

#[test]
fn refusals_leave_rows_and_checkpoint_unchanged_and_can_cancel() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let mut copy = begin(&runtime);
    let before = memory.borrow().clone();
    assert!(
        copy.copy_next::<Source, Target>(2, 18, |row, _| {
            if row.id == 2 {
                Err(Error::Limit)
            } else {
                Ok(Target {
                    id: row.id,
                    value: 1,
                })
            }
        })
        .is_err()
    );
    assert_eq!(*memory.borrow(), before);
    assert!(matches!(
        copy.copy_next::<Source, Target>(1, 9, |row, _| Ok(Target {
            id: row.id + 1,
            value: 1
        })),
        Err(Error::InvalidTransition)
    ));
    assert_eq!(*memory.borrow(), before);
    copy.copy_next::<Source, Target>(1, 9, |row, _| {
        Ok(Target {
            id: row.id,
            value: 1,
        })
    })
    .unwrap();
    let before = memory.borrow().clone();
    assert!(
        copy.copy_next::<Source, Target>(1, 9, |row, _| Ok(Target {
            id: row.id,
            value: 1
        }))
        .is_err()
    );
    assert_eq!(*memory.borrow(), before); // unique collision with an earlier chunk
    drop(copy);
    let mut copy = resume(&runtime);
    assert!(
        copy.copy_next::<Source, Target>(1, 9, |row, _| Ok(Target {
            id: row.id,
            value: 1
        }))
        .is_err()
    );
    while !copy.cancel_step(2).unwrap() {}
    drop(copy);
    Database::open(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source(),
    )
    .unwrap();
}

#[test]
fn ownership_refuses_wrong_pairs_versions_and_existing_target_allocations() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    drop(begin(&runtime));
    let before = memory.borrow().clone();
    for (left, right, version) in [
        ("copy.target.v1", "copy.source.v1", 1),
        ("copy.source.v1", "copy.target.v1", 2),
        ("copy.source.v1", "copy.other.v1", 1),
    ] {
        assert!(
            SchemaCopy::resume(
                runtime.open_memory(&key(left)).unwrap(),
                runtime.open_memory(&key(right)).unwrap(),
                source(),
                target(),
                &transition(version)
            )
            .is_err()
        );
        assert_eq!(*memory.borrow(), before);
    }
    let mut copy = resume(&runtime);
    assert!(copy.cancel_step(32).unwrap());
    drop(copy);
    let db = Database::open(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source(),
    )
    .unwrap();
    let maintenance = db.begin_maintenance(&transition(1)).unwrap();
    let before = memory.borrow().clone();
    assert!(
        maintenance
            .begin_copy(
                runtime.open_memory(&key("copy.target.v1")).unwrap(),
                source(),
                target(),
                [3; 16]
            )
            .is_err()
    );
    assert_eq!(*memory.borrow(), before); // even an empty cancelled target is not fresh
    Maintenance::resume(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        &transition(1),
    )
    .unwrap()
    .cancel();
}

#[test]
fn added_entity_requires_pristine_state_before_cutover() {
    for (revision, count) in [(1_u64, 0_u64), (0, 1)] {
        let runtime = host(VectorMemory::default());
        let source = source();
        let target = Schema::new(&[Other::SCHEMA, Source::SCHEMA]).unwrap();
        let transition = SchemaTransition::new(&source, &target, "add-other", 1, &[]).unwrap();
        let source_memory = || runtime.open_memory(&key("copy.source.v1")).unwrap();
        let target_memory = || runtime.open_memory(&key("copy.target.v1")).unwrap();
        let mut copy = Database::create(source_memory(), source.clone(), [1; 16])
            .unwrap()
            .begin_maintenance(&transition)
            .unwrap()
            .begin_copy(target_memory(), source.clone(), target.clone(), [2; 16])
            .unwrap();
        assert!(copy.copy_unchanged::<Source>(1, 4096).unwrap().complete);
        drop(copy);
        let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(target_memory());
        let mut metadata_key = vec![1];
        metadata_key.extend(Other::SCHEMA.id.to_be_bytes());
        assert_eq!(raw.get(&metadata_key), Some(vec![0; 16]));
        raw.insert(
            metadata_key,
            [revision.to_le_bytes(), count.to_le_bytes()].concat(),
        );
        drop(raw);
        let mut copy = SchemaCopy::resume(
            source_memory(),
            target_memory(),
            source,
            target,
            &transition,
        )
        .unwrap();
        assert!(!copy.validate_next(32).unwrap()); // source pass
        assert!(matches!(copy.validate_next(32), Err(Error::Corrupt)));
        assert!(matches!(copy.publish(), Err(Error::InvalidTransition)));
    }
}

#[test]
fn migrated_row_can_link_to_a_new_entity_after_publication() {
    ///
    /// LinkedOther
    /// Exercises references to `Source` on rows keyed by `id`.
    ///

    #[entity(
        store = ic_db::testing::FixtureStore,
        name = "Other",
        pk(field = "id"),
        index(field = "thing"),
        fields(
            field(name = "id", value(item(prim = "Nat64"))),
            field(name = "related", value(many, max_items = 2, item(prim = "Nat64")), references(entity = "Source")),
            field(
                name = "thing",
                value(opt, item(prim = "Nat64")),
                references(entity = "Source")
            )
        )
    )]
    struct LinkedOther {}

    for premature_link in [0, 1, 2] {
        let runtime = host(VectorMemory::default());
        let source = Schema::new(&[Other::SCHEMA]).unwrap();
        let target = Schema::new(&[LinkedOther::SCHEMA, Source::SCHEMA]).unwrap();
        let transition =
            SchemaTransition::new(&source, &target, "link-thing", 1, &[premature_link]).unwrap();
        let source_memory = || runtime.open_memory(&key("copy.source.v1")).unwrap();
        let mut db = Database::create(source_memory(), source.clone(), [1; 16]).unwrap();
        let mut batch = Batch::new();
        batch.insert(&Other { id: 7 }).unwrap();
        db.commit(batch).unwrap();
        let mut copy = db
            .begin_maintenance(&transition)
            .unwrap()
            .begin_copy(
                runtime.open_memory(&key("copy.target.v1")).unwrap(),
                source.clone(),
                target,
                [2; 16],
            )
            .unwrap();
        assert!(
            copy.copy_next::<Other, LinkedOther>(2, 4096, |row, inputs| {
                Ok(LinkedOther {
                    id: row.id,
                    thing: (inputs == [1]).then_some(0),
                    related: if inputs == [2] { vec![0, 0] } else { vec![] },
                })
            })
            .unwrap()
            .complete
        );
        assert!(!copy.validate_next(32).unwrap()); // source pass
        if premature_link != 0 {
            assert!(matches!(
                copy.validate_next(32),
                Err(Error::ReferenceViolation)
            ));
            assert!(copy.cancel_step(32).unwrap());
            drop(copy);
            let db = Database::open(source_memory(), source).unwrap();
            assert!(db.get::<Other>(7).unwrap().is_some());
            continue;
        }

        assert!(copy.validate_next(32).unwrap());
        let mut db = copy.publish().unwrap();
        let mut row = db.get::<LinkedOther>(7).unwrap().unwrap();
        assert_eq!(row.thing, None);
        row.thing = Some(0);
        let mut batch = Batch::new();
        batch.replace(&row).unwrap();
        batch.insert(&Source { id: 0, value: 9 }).unwrap();
        db.commit(batch).unwrap();
        assert_eq!(
            db.index_prefix::<LinkedOther>("thing", &[0_u64.into()], 2)
                .unwrap(),
            [row]
        );
        let mut batch = Batch::new();
        batch.delete::<Source>(0).unwrap();
        assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    }
}

#[test]
fn unsupported_shapes_refuse_before_target_initialization() {
    for (source_schema, target_schema) in [
        (
            Schema::new(&[Source::SCHEMA, Other::SCHEMA]).unwrap(),
            Schema::new(&[Target::SCHEMA]).unwrap(),
        ),
        (source(), Schema::new(&[Other::SCHEMA]).unwrap()),
        (Schema::new(&[ManagedSource::SCHEMA]).unwrap(), target()),
        (source(), Schema::new(&[ManagedSource::SCHEMA]).unwrap()),
    ] {
        let memory = VectorMemory::default();
        let runtime = host(memory.clone());
        let transition =
            SchemaTransition::new(&source_schema, &target_schema, "unsupported", 1, &[]).unwrap();
        let db = Database::create(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            source_schema.clone(),
            [1; 16],
        )
        .unwrap();
        let maintenance = db.begin_maintenance(&transition).unwrap();
        let before = memory.borrow().clone();
        assert!(matches!(
            maintenance.begin_copy(
                runtime.open_memory(&key("copy.target.v1")).unwrap(),
                source_schema,
                target_schema,
                [2; 16]
            ),
            Err(Error::InvalidTransition)
        ));
        assert_eq!(*memory.borrow(), before);
        Maintenance::resume(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            &transition,
        )
        .unwrap()
        .cancel();
    }
}

#[test]
fn empty_source_completes_and_cancels_within_one_entry() {
    let runtime = host(VectorMemory::default());
    let db = Database::create(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source(),
        [1; 16],
    )
    .unwrap();
    let mut copy = db
        .begin_maintenance(&transition(1))
        .unwrap()
        .begin_copy(
            runtime.open_memory(&key("copy.target.v1")).unwrap(),
            source(),
            target(),
            [2; 16],
        )
        .unwrap();
    let step = copy.copy_next::<Source, Target>(1, 9, transform).unwrap();
    assert_eq!((step.rows, step.complete), (0, true));
    assert!(matches!(copy.cancel_step(0), Err(Error::Limit)));
    assert!(copy.cancel_step(1).unwrap());
    drop(copy);
    Database::open(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source(),
    )
    .unwrap();
}

#[test]
fn malformed_and_future_copy_checkpoints_refuse_intact() {
    for case in 0..4 {
        let memory = VectorMemory::default();
        let runtime = host(memory.clone());
        drop(begin(&runtime));
        let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(
            runtime.open_memory(&key("copy.target.v1")).unwrap(),
        );
        let mut frame = raw.get(&vec![0]).unwrap();
        match case {
            0 => {
                frame.pop();
            }
            1 => frame.push(0),
            3 => frame[..8].copy_from_slice(b"ICDBCP01"),
            _ => {
                let phase = frame.len() - 5;
                frame[phase] = 255;
            }
        }
        raw.insert(vec![0], frame);
        drop(raw);
        let before = memory.borrow().clone();
        assert!(
            Database::open(
                runtime.open_memory(&key("copy.target.v1")).unwrap(),
                target()
            )
            .is_err()
        );
        assert!(
            SchemaCopy::resume(
                runtime.open_memory(&key("copy.source.v1")).unwrap(),
                runtime.open_memory(&key("copy.target.v1")).unwrap(),
                source(),
                target(),
                &transition(1)
            )
            .is_err()
        );
        assert_eq!(*memory.borrow(), before);
    }
}

// Exercise the envelope decoder independently of the writer. Resume separately
// checks that the decoded position exists in the specific frozen source.
#[test]
fn copy_position_frames_accept_only_the_canonical_encoding() {
    let runtime = host(VectorMemory::default());
    drop(begin(&runtime));
    let lease = || runtime.open_memory(&key("copy.target.v1")).unwrap();
    let manifest = vec![0];
    let mut header = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(lease())
        .get(&manifest)
        .unwrap();
    assert!(header.ends_with(&[1, 0, 0, 0, 0])); // first entity, empty cursor
    header.truncate(header.len() - 5);
    for (position, valid) in [
        (0, false),
        (31, true),
        (32, true),
        (127, true),
        (128, false),
    ] {
        let frame = [header.as_slice(), &[9, position], &[0; 4]].concat();
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(lease()).insert(manifest.clone(), frame);
        let outcome = Database::open(lease(), target());
        assert!(
            matches!(
                (valid, outcome),
                (true, Err(Error::Maintenance)) | (false, Err(Error::Corrupt))
            ),
            "checkpoint position {position}"
        );
    }
}

fn finish_copy(copy: &mut SchemaCopy) {
    for _ in 0..8 {
        if copy
            .copy_next::<Source, Target>(2, 18, transform)
            .unwrap()
            .complete
        {
            return;
        }
    }
    panic!("copy must make progress");
}

#[test]
fn publication_requires_resumable_validation_and_keeps_source_retired() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let copy = begin(&runtime);
    assert_eq!(copy.status(), CopyStatus::Copying("Thing"));
    let before = memory.borrow().clone();
    assert!(matches!(copy.publish(), Err(Error::InvalidTransition)));
    assert_eq!(*memory.borrow(), before);
    let mut copy = resume(&runtime);
    assert!(matches!(
        copy.validate_next(2),
        Err(Error::InvalidTransition)
    ));
    finish_copy(&mut copy);
    let before = memory.borrow().clone();
    assert!(matches!(copy.publish(), Err(Error::InvalidTransition)));
    assert_eq!(*memory.borrow(), before);
    let source_entries = entries(&runtime, "copy.source.v1");
    for call in 0..32 {
        let mut copy = resume(&runtime);
        assert_eq!(copy.status(), CopyStatus::Validating);
        let ready = copy.validate_next(2).unwrap();
        assert!(matches!(
            copy.copy_next::<Source, Target>(2, 18, transform),
            Err(Error::InvalidTransition)
        ));
        drop(copy);
        assert!(
            Database::open(
                runtime.open_memory(&key("copy.target.v1")).unwrap(),
                target()
            )
            .is_err()
        );
        if ready {
            break;
        }
        assert!(call < 31, "validation must progress");
    }
    let copy = resume(&runtime);
    assert_eq!(copy.status(), CopyStatus::ReadyToPublish);
    let mut db = copy.publish().unwrap();
    assert_eq!(db.incarnation(), [2; 16]);
    assert_eq!(db.get::<Target>(1).unwrap().unwrap().value, 8);
    assert_eq!(
        db.index_prefix::<Target>("value", &[8_u16.into()], 2)
            .unwrap()[0]
            .id,
        1
    );
    let mut batch = Batch::new();
    batch.replace(&Target { id: 1, value: 100 }).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    assert_eq!(
        &entries(&runtime, "copy.source.v1")[1..],
        &source_entries[1..]
    );
    let before = memory.borrow().clone();
    assert!(matches!(
        Database::open(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            source()
        ),
        Err(Error::Maintenance)
    ));
    assert!(
        Database::open(
            runtime.open_memory(&key("copy.target.v1")).unwrap(),
            source()
        )
        .is_err()
    );
    assert!(
        Maintenance::resume(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            &transition(1)
        )
        .is_err()
    );
    assert!(
        SchemaCopy::resume(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            runtime.open_memory(&key("copy.target.v1")).unwrap(),
            source(),
            target(),
            &transition(1)
        )
        .is_err()
    );
    assert_eq!(*memory.borrow(), before);
    let db = Database::open(
        runtime.open_memory(&key("copy.target.v1")).unwrap(),
        target(),
    )
    .unwrap();
    assert_eq!(db.get::<Target>(1).unwrap().unwrap().value, 100);
}

#[test]
fn incomplete_or_inconsistent_collections_never_validate_or_publish() {
    for case in 0..10 {
        let memory = VectorMemory::default();
        let runtime = host(memory.clone());
        let mut copy = begin(&runtime);
        finish_copy(&mut copy);
        drop(copy);
        let name = if case >= 8 {
            "copy.source.v1"
        } else {
            "copy.target.v1"
        };
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key(name)).unwrap());
        let row = raw.keys().find(|key| key[0] == 2).unwrap();
        let index = raw.keys().find(|key| key[0] == 3);
        match case {
            0 | 8 => {
                raw.remove(&row);
            }
            1 => {
                raw.remove(&index.unwrap());
            }
            2 => {
                raw.insert(index.unwrap(), 5_u64.to_be_bytes().to_vec());
            }
            3 => {
                let mut stale = index.unwrap();
                stale[9..].copy_from_slice(&999_u16.to_be_bytes());
                raw.insert(stale, 1_u64.to_be_bytes().to_vec());
            }
            4 => {
                raw.insert(row, vec![0]);
            }
            5 => {
                raw.insert(vec![4], vec![0]);
            }
            6 => {
                let revision = raw.keys().find(|key| key[0] == 1).unwrap();
                raw.remove(&revision);
            }
            7 => {
                let mut extra = row;
                extra[5..].copy_from_slice(&99_u64.to_be_bytes());
                let mut bytes = 99_u64.to_le_bytes().to_vec();
                bytes.extend(999_u16.to_le_bytes());
                raw.insert(extra, bytes);
            }
            _ => {
                let mut bytes = 99_u64.to_le_bytes().to_vec();
                bytes.push(1);
                raw.insert(row, bytes);
            }
        }
        drop(raw);
        let mut copy = resume(&runtime);
        for call in 0..32 {
            let before = memory.borrow().clone();

            match copy.validate_next(2) {
                Err(_) => {
                    assert_eq!(*memory.borrow(), before);
                    break;
                }
                Ok(ready) => {
                    assert!(!ready, "corruption case {case} validated");
                    assert!(call < 31);
                }
            }
        }
        let before = memory.borrow().clone();
        assert!(matches!(copy.publish(), Err(Error::InvalidTransition)));
        assert_eq!(*memory.borrow(), before);
        let mut copy = resume(&runtime);
        for call in 0..32 {
            if copy.cancel_step(2).unwrap() {
                break;
            }
            assert!(call < 31);
        }
    }
}

#[test]
fn retired_cleanup_binds_ownership_preserves_target_and_keeps_source_closed() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let copy = begin(&runtime);
    drop(copy);
    let before = memory.borrow().clone();
    assert!(
        RetiredSource::resume(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            &transition(1),
            [1; 16],
            [2; 16]
        )
        .is_err()
    );
    assert_eq!(*memory.borrow(), before);
    let mut copy = resume(&runtime);
    finish_copy(&mut copy);
    for call in 0..32 {
        if copy.validate_next(2).unwrap() {
            break;
        }
        assert!(call < 31);
    }
    drop(copy.publish().unwrap());
    let target_entries = entries(&runtime, "copy.target.v1");
    let retired_manifest = entries(&runtime, "copy.source.v1")[0].clone();
    for (name, version, source_inc, target_inc) in [
        ("copy.target.v1", 1, [1; 16], [2; 16]),
        ("copy.source.v1", 2, [1; 16], [2; 16]),
        ("copy.source.v1", 1, [9; 16], [2; 16]),
        ("copy.source.v1", 1, [1; 16], [9; 16]),
    ] {
        let before = memory.borrow().clone();
        assert!(
            RetiredSource::resume(
                runtime.open_memory(&key(name)).unwrap(),
                &transition(version),
                source_inc,
                target_inc
            )
            .is_err()
        );
        assert_eq!(*memory.borrow(), before);
    }
    let mut remaining = entries(&runtime, "copy.source.v1").len();
    for call in 0..16 {
        let mut retired = RetiredSource::resume(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            &transition(1),
            [1; 16],
            [2; 16],
        )
        .unwrap();
        let before = memory.borrow().clone();
        for limit in [0, 33] {
            assert!(matches!(retired.cleanup_step(limit), Err(Error::Limit)));
        }
        assert_eq!(*memory.borrow(), before);
        let complete = retired.cleanup_step(1).unwrap(); // first chunk removes revision only
        drop(retired);
        let now = entries(&runtime, "copy.source.v1");
        assert_eq!(now[0], retired_manifest);
        assert_eq!(now.len(), remaining - 1);
        remaining = now.len();
        assert_eq!(entries(&runtime, "copy.target.v1"), target_entries);
        assert!(
            Database::open(
                runtime.open_memory(&key("copy.source.v1")).unwrap(),
                source()
            )
            .is_err()
        );
        if complete {
            assert_eq!(remaining, 1);
            break;
        }
        assert!(call < 15);
    }
    let mut retired = RetiredSource::resume(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        &transition(1),
        [1; 16],
        [2; 16],
    )
    .unwrap();
    let before = memory.borrow().clone();
    assert!(retired.cleanup_step(32).unwrap());
    assert_eq!(*memory.borrow(), before);
    drop(retired);
    assert!(
        Maintenance::resume(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            &transition(1)
        )
        .is_err()
    );
    assert!(
        Database::create(
            runtime.open_memory(&key("copy.source.v1")).unwrap(),
            source(),
            [9; 16]
        )
        .is_err()
    );
    let mut db = Database::open(
        runtime.open_memory(&key("copy.target.v1")).unwrap(),
        target(),
    )
    .unwrap();
    let mut batch = Batch::new();
    batch.replace(&Target { id: 1, value: 100 }).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.get::<Target>(1).unwrap().unwrap().value, 100);
}

#[test]
fn migration_preserves_managed_roles_and_resumes_normal_time_policy() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let source = Schema::new(&[TimedSource::SCHEMA]).unwrap();
    let target = Schema::new(&[TimedTarget::SCHEMA]).unwrap();
    let transition = SchemaTransition::new(&source, &target, "timed", 1, &[]).unwrap();
    let mut db = Database::create(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source.clone(),
        [1; 16],
    )
    .unwrap();
    let mut batch = Batch::at_for_test(Timestamp::from_millis(-10));
    for id in 1..=2 {
        batch
            .insert_input(TimedSource::insert().id(id).value(3))
            .unwrap();
    }
    db.commit(batch).unwrap();
    let mut row = db.get::<TimedSource>(2).unwrap().unwrap();
    row.value = 4;
    let mut batch = Batch::at_for_test(Timestamp::from_millis(20));
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    let mut copy = db
        .begin_maintenance(&transition)
        .unwrap()
        .begin_copy(
            runtime.open_memory(&key("copy.target.v1")).unwrap(),
            source.clone(),
            target.clone(),
            [2; 16],
        )
        .unwrap();
    // A changed timestamp on the second row must refuse the entire prepared chunk.
    for role in 0..2 {
        let before = memory.borrow().clone();
        let transform: fn(TimedSource, &[u8]) -> Result<TimedTarget, Error> = if role == 0 {
            |row, _| {
                let mut result = timed_transform(row, &[])?;
                if result.id == 2 {
                    result.born = Timestamp::from_millis(0);
                }

                Ok(result)
            }
        } else {
            |row, _| {
                let mut result = timed_transform(row, &[])?;
                if result.id == 2 {
                    result.changed = Timestamp::from_millis(21);
                }

                Ok(result)
            }
        };
        assert!(matches!(
            copy.copy_next(2, 128 * 1024, transform),
            Err(Error::ManagedField)
        ));
        assert_eq!(*memory.borrow(), before);
    }
    assert_eq!(
        copy.copy_next(1, 128 * 1024, timed_transform).unwrap().rows,
        1
    );
    drop(copy);
    let mut copy = SchemaCopy::resume(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        runtime.open_memory(&key("copy.target.v1")).unwrap(),
        source.clone(),
        target.clone(),
        &transition,
    )
    .unwrap();
    while !copy
        .copy_next(1, 128 * 1024, timed_transform)
        .unwrap()
        .complete
    {}
    drop(copy);
    // Independently inspect the exact target encoding, including renamed fields.
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(
        runtime.open_memory(&key("copy.target.v1")).unwrap(),
    );
    let rows: Vec<_> = raw
        .iter()
        .filter(|entry| entry.key()[0] == 2)
        .map(|entry| (entry.key().clone(), entry.value()))
        .collect();
    for (index, (_, bytes)) in rows.iter().enumerate() {
        // Stored field order is canonical by name: born, changed, id, value.
        let mut expected = (-10_i64).to_le_bytes().to_vec();
        expected.extend((if index == 0 { -10_i64 } else { 20_i64 }).to_le_bytes());
        expected.extend(((index + 1) as u64).to_le_bytes());
        expected.extend((13 + index as u16).to_le_bytes());
        assert_eq!(*bytes, expected);
    }
    // A well-formed but changed creation time must also fail final validation.
    let mut damaged = rows[1].1.clone();
    damaged[..8].copy_from_slice(&0_i64.to_le_bytes());
    raw.insert(rows[1].0.clone(), damaged);
    drop(raw);
    let mut copy = SchemaCopy::resume(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        runtime.open_memory(&key("copy.target.v1")).unwrap(),
        source.clone(),
        target.clone(),
        &transition,
    )
    .unwrap();
    let before = memory.borrow().clone();
    assert!(matches!(copy.validate_next(32), Err(Error::ManagedField)));
    assert_eq!(*memory.borrow(), before);
    drop(copy);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(
        runtime.open_memory(&key("copy.target.v1")).unwrap(),
    );
    raw.insert(rows[1].0.clone(), rows[1].1.clone());
    drop(raw);
    let mut copy = SchemaCopy::resume(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        runtime.open_memory(&key("copy.target.v1")).unwrap(),
        source,
        target.clone(),
        &transition,
    )
    .unwrap();
    while !copy.validate_next(1).unwrap() {}
    let mut db = copy.publish().unwrap();
    let mut row = db.get::<TimedTarget>(2).unwrap().unwrap();
    assert!(matches!(
        Batch::new().insert(&row),
        Err(Error::ManagedField)
    ));
    row.value = 99;
    let mut batch = Batch::at_for_test(Timestamp::from_millis(30));
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let db = Database::open(runtime.open_memory(&key("copy.target.v1")).unwrap(), target).unwrap();
    let row = db.get::<TimedTarget>(2).unwrap().unwrap();
    assert_eq!(
        (row.born.as_millis(), row.changed.as_millis(), row.value),
        (-10, 30, 99)
    );
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "matches the consuming migration callback"
)]
fn timed_transform(row: TimedSource, _: &[u8]) -> Result<TimedTarget, Error> {
    Ok(TimedTarget {
        id: row.id,
        value: u16::from(row.value) + 10,
        born: row.created,
        changed: row.updated,
    })
}

///
/// Link
/// Exercises references to `UserOld` and `Self` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "A_Link",
    pk(field = "id"),
    index(field = "user"),
    index(field = "next"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "user",
            value(item(prim = "Nat64")),
            references(entity = "UserOld")
        ),
        field(
            name = "next",
            value(opt, item(prim = "Nat64")),
            references(entity = "Self")
        )
    )
)]
struct Link {}

///
/// UserOld
/// Exercises references to `Link` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Z_User",
    pk(field = "id"),
    index(field = "link"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "link",
            value(opt, item(prim = "Nat64")),
            references(entity = "Link")
        ),
        field(name = "value", value(item(prim = "Nat8")))
    )
)]
struct UserOld {}

///
/// UserNew
/// Exercises references to `Link` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Z_User",
    pk(field = "id"),
    index(field = "link"),
    index(field = "value", unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "link",
            value(opt, item(prim = "Nat64")),
            references(entity = "Link")
        ),
        field(name = "value", value(item(prim = "Nat16")))
    )
)]
struct UserNew {}

fn multi_schemas() -> (Schema, Schema, SchemaTransition) {
    // Deliberately supply schemas in different orders; canonical names own progress.
    let source = Schema::new(&[UserOld::SCHEMA, Link::SCHEMA, Other::SCHEMA]).unwrap();
    let target = Schema::new(&[Other::SCHEMA, Link::SCHEMA, UserNew::SCHEMA]).unwrap();
    let transition = SchemaTransition::new(&source, &target, "whole-db", 1, &[]).unwrap();

    (source, target, transition)
}

fn multi_begin(runtime: &MemoryRuntime<VectorMemory>) -> SchemaCopy {
    let (source, target, transition) = multi_schemas();
    let mut db = Database::create(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        source.clone(),
        [1; 16],
    )
    .unwrap();
    let mut batch = Batch::new();
    batch
        .insert(&Link {
            id: 1,
            user: 0,
            next: None,
        })
        .unwrap();
    batch
        .insert(&UserOld {
            id: 0,
            link: Some(1),
            value: 7,
        })
        .unwrap();
    db.commit(batch).unwrap(); // final-state cycle, with zero as a real target key

    db.begin_maintenance(&transition)
        .unwrap()
        .begin_copy(
            runtime.open_memory(&key("copy.target.v1")).unwrap(),
            source,
            target,
            [2; 16],
        )
        .unwrap()
}

fn multi_resume(runtime: &MemoryRuntime<VectorMemory>) -> SchemaCopy {
    let (source, target, transition) = multi_schemas();

    SchemaCopy::resume(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        runtime.open_memory(&key("copy.target.v1")).unwrap(),
        source,
        target,
        &transition,
    )
    .unwrap()
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "matches consuming transform API"
)]
fn widen_user(row: UserOld, _: &[u8]) -> Result<UserNew, Error> {
    Ok(UserNew {
        id: row.id,
        link: row.link,
        value: u16::from(row.value) + 100,
    })
}

#[test]
fn whole_database_copy_resumes_across_entities_and_validates_cycles() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let mut copy = multi_begin(&runtime);
    assert_eq!(copy.next_entity(), Some("A_Link"));
    let before = memory.borrow().clone();
    assert!(matches!(
        copy.copy_next(2, 4096, widen_user),
        Err(Error::InvalidTransition)
    ));
    assert!(matches!(
        copy.copy_next::<Link, UserNew>(2, 4096, |row, _| Ok(UserNew {
            id: row.id,
            link: None,
            value: 0,
        })),
        Err(Error::InvalidTransition)
    ));
    assert!(matches!(
        copy.validate_next(32),
        Err(Error::InvalidTransition)
    ));
    assert_eq!(*memory.borrow(), before);
    // The referenced user has not been copied yet. Target stays private.
    assert!(!copy.copy_unchanged::<Link>(2, 4096).unwrap().complete);
    assert_eq!(copy.next_entity(), Some("Other"));
    drop(copy);
    let (_, target, _) = multi_schemas();
    assert!(matches!(
        Database::open(runtime.open_memory(&key("copy.target.v1")).unwrap(), target),
        Err(Error::Maintenance)
    ));
    let mut copy = multi_resume(&runtime);
    assert_eq!(copy.next_entity(), Some("Other"));
    assert_eq!(copy.copy_unchanged::<Other>(1, 4096).unwrap().rows, 0);
    assert_eq!(copy.next_entity(), Some("Z_User"));
    let before = memory.borrow().clone();
    assert!(copy.copy_unchanged::<UserOld>(2, 4096).is_err());
    assert_eq!(*memory.borrow(), before);
    assert!(copy.copy_next(2, 4096, widen_user).unwrap().complete);
    assert_eq!(copy.next_entity(), None);
    drop(copy);
    // Independent raw counts: manifest, all three revisions, two rows, four indexes.
    let raw = entries(&runtime, "copy.target.v1");
    assert_eq!(raw.iter().filter(|(key, _)| key[0] == 1).count(), 3);
    assert_eq!(raw.iter().filter(|(key, _)| key[0] == 2).count(), 2);
    let mut copy = multi_resume(&runtime);
    for _ in 0..40 {
        if copy.validate_next(1).unwrap() {
            break;
        }
        drop(copy);
        copy = multi_resume(&runtime);
    }
    let mut db = copy.publish().unwrap();
    assert_eq!(db.get::<Link>(1).unwrap().unwrap().user, 0);
    assert_eq!(db.get::<UserNew>(0).unwrap().unwrap().value, 107);
    assert!(db.page::<Other>(None, 1).unwrap().rows.is_empty());
    let (_, _, transition) = multi_schemas();
    let mut retired = RetiredSource::resume(
        runtime.open_memory(&key("copy.source.v1")).unwrap(),
        &transition,
        [1; 16],
        [2; 16],
    )
    .unwrap();
    let mut cleaned = false;
    for _ in 0..32 {
        if retired.cleanup_step(1).unwrap() {
            cleaned = true;
            break;
        }
    }
    assert!(cleaned);
    drop(retired);
    assert_eq!(entries(&runtime, "copy.source.v1").len(), 1);
    assert_eq!(
        db.index_prefix::<UserNew>("value", &[107_u16.into()], 1)
            .unwrap()
            .len(),
        1
    );
    // Ordinary inserts/deletes still enforce references after cutover.
    let mut batch = Batch::new();
    batch
        .insert(&Link {
            id: 2,
            user: 99,
            next: None,
        })
        .unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    let mut batch = Batch::new();
    batch.delete::<UserNew>(0).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    let mut batch = Batch::new();
    batch.delete::<Link>(1).unwrap();
    batch.delete::<UserNew>(0).unwrap();
    db.commit(batch).unwrap();
}

#[test]
fn invalid_completed_reference_refuses_publication_and_cancels_to_original_source() {
    let memory = VectorMemory::default();
    let runtime = host(memory.clone());
    let mut copy = multi_begin(&runtime);
    copy.copy_unchanged::<Link>(2, 4096).unwrap();
    copy.copy_unchanged::<Other>(2, 4096).unwrap();
    copy.copy_next::<UserOld, UserNew>(2, 4096, |row, inputs| {
        let mut row = widen_user(row, inputs)?;
        row.link = Some(999);

        Ok(row)
    })
    .unwrap();
    assert!(!copy.validate_next(32).unwrap()); // source validates independently
    let before = memory.borrow().clone();
    assert!(matches!(
        copy.validate_next(32),
        Err(Error::ReferenceViolation)
    ));
    assert_eq!(*memory.borrow(), before);
    assert!(matches!(copy.publish(), Err(Error::InvalidTransition)));
    let mut copy = multi_resume(&runtime);
    for _ in 0..32 {
        if copy.cancel_step(1).unwrap() {
            break;
        }
    }
    drop(copy);
    let (source, _, _) = multi_schemas();
    let db = Database::open(runtime.open_memory(&key("copy.source.v1")).unwrap(), source).unwrap();
    assert_eq!(db.get::<UserOld>(0).unwrap().unwrap().link, Some(1));
}

#[test]
fn missing_empty_entity_revision_and_unknown_copy_positions_refuse_intact() {
    for malformed_position in [false, true] {
        let memory = VectorMemory::default();
        let runtime = host(memory.clone());
        let mut copy = multi_begin(&runtime);
        copy.copy_unchanged::<Link>(2, 4096).unwrap();
        if !malformed_position {
            copy.copy_unchanged::<Other>(2, 4096).unwrap();
            copy.copy_next(2, 4096, widen_user).unwrap();
        }
        drop(copy);
        let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(
            runtime.open_memory(&key("copy.target.v1")).unwrap(),
        );
        if malformed_position {
            let mut frame = raw.get(&vec![0]).unwrap();
            let entity = frame.len() - 5;
            frame[entity] = 31; // valid envelope ordinal, outside this three-entity schema
            raw.insert(vec![0], frame);
        } else {
            let mut revision = vec![1];
            revision.extend(Other::SCHEMA.id.to_be_bytes());
            raw.remove(&revision);
        }
        drop(raw);
        let before = memory.borrow().clone();
        if malformed_position {
            let (source, target, transition) = multi_schemas();
            assert!(matches!(
                SchemaCopy::resume(
                    runtime.open_memory(&key("copy.source.v1")).unwrap(),
                    runtime.open_memory(&key("copy.target.v1")).unwrap(),
                    source,
                    target,
                    &transition
                ),
                Err(Error::Corrupt)
            ));
        } else {
            let mut copy = multi_resume(&runtime);
            // The paired metadata check sees a missing target entry immediately.
            assert!(copy.validate_next(32).is_err());
        }
        assert_eq!(*memory.borrow(), before);
    }
}

// The two declarations differ only in the index named by the reference.
macro_rules! listing_schema {
    ($(#[$doc:meta])* $row:ident, $index:tt, [$($field:tt),+]) => {
        $(#[$doc])*
        #[entity(
            store = ic_db::testing::FixtureStore,
            name = "Document",
            pk(field = "id"),
            index(fields = [$($field),+]),
            fields(
                field(name = "id", value(item(prim = "Nat64"))),
                field(name = "entry_id", value(item(prim = "Nat64")), references(entity = "Other", index = $index)),
                field(name = "published", value(item(prim = "Bool"))),
                field(name = "score", value(item(prim = "Int64")))
            )
        )]
        struct $row {}
    };
}

listing_schema!(
    ///
    /// OldDocument
    /// Orders all documents of one entry by score.
    ///
    OldDocument,
    "entry_id,score",
    ["entry_id", "score"]
);

listing_schema!(
    ///
    /// NewDocument
    /// Groups an entry’s documents by publication state before ordering by score.
    ///
    NewDocument,
    "entry_id,published,score",
    ["entry_id", "published", "score"]
);

#[test]
fn populated_listing_index_replacement_resumes_and_preserves_references() {
    let runtime = host(VectorMemory::default());
    let source_memory = || runtime.open_memory(&key("copy.source.v1")).unwrap();
    let target_memory = || runtime.open_memory(&key("copy.target.v1")).unwrap();
    let source = Schema::new(&[OldDocument::SCHEMA, Other::SCHEMA]).unwrap();
    let target = Schema::new(&[NewDocument::SCHEMA, Other::SCHEMA]).unwrap();
    let transition = SchemaTransition::new(&source, &target, "listing-index", 1, &[]).unwrap();
    let mut db = Database::create(source_memory(), source.clone(), [1; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&Other { id: 1 }).unwrap();
    for (id, published, score) in [(1, true, 7), (2, false, 99), (3, true, 7), (4, true, -1)] {
        batch
            .insert(&OldDocument {
                id,
                entry_id: 1,
                published,
                score,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();
    let first = db
        .query::<OldDocument>()
        .eq(OldDocument::ENTRY_ID, 1_u64)
        .eq(OldDocument::PUBLISHED, true)
        .order_by(&[OldDocument::SCORE, OldDocument::ID])
        .descending()
        .limit(1)
        .fetch()
        .unwrap();
    assert_eq!(first.rows[0].id, 3);
    let old_cursor = first.next.unwrap();

    let mut copy = db
        .begin_maintenance(&transition)
        .unwrap()
        .begin_copy(target_memory(), source.clone(), target.clone(), [2; 16])
        .unwrap();
    let transform = |row: OldDocument, _: &[u8]| {
        Ok(NewDocument {
            id: row.id,
            entry_id: row.entry_id,
            published: row.published,
            score: row.score,
        })
    };
    assert_eq!(copy.copy_next(1, 4096, transform).unwrap().rows, 1);
    drop(copy);
    assert!(matches!(
        Database::open(source_memory(), source.clone()),
        Err(Error::Maintenance)
    ));
    assert!(matches!(
        Database::open(target_memory(), target.clone()),
        Err(Error::Maintenance)
    ));
    let mut copy = SchemaCopy::resume(
        source_memory(),
        target_memory(),
        source,
        target.clone(),
        &transition,
    )
    .unwrap();
    assert_eq!(copy.copy_next(4, 4096, transform).unwrap().rows, 3);
    assert!(copy.copy_unchanged::<Other>(2, 4096).unwrap().complete);
    assert!((0..64).any(|_| copy.validate_next(1).unwrap()));
    drop(copy.publish().unwrap());
    let stored_rows = |name| {
        entries(&runtime, name)
            .into_iter()
            .filter(|(key, _)| key[0] == 2)
            .collect::<Vec<_>>()
    };
    let old_rows = stored_rows("copy.source.v1");
    assert_eq!(old_rows.len(), 5);
    assert_eq!(stored_rows("copy.target.v1"), old_rows);
    let mut db = Database::open(target_memory(), target).unwrap();
    let query = || {
        db.query::<NewDocument>()
            .eq(NewDocument::ENTRY_ID, 1_u64)
            .eq(NewDocument::PUBLISHED, true)
            .order_by(&[NewDocument::SCORE, NewDocument::ID])
            .descending()
            .limit(1)
    };
    assert!(matches!(
        query().after(&old_cursor).fetch(),
        Err(Error::InvalidCursor)
    ));
    let mut after = None;
    for expected in [3, 1, 4] {
        let mut q = query();
        if let Some(cursor) = &after {
            q = q.after(cursor);
        }
        let page = q.fetch().unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].id, expected);
        after = page.next;
    }
    if let Some(cursor) = after {
        let end = query().after(&cursor).fetch().unwrap();
        assert!(end.rows.is_empty() && end.next.is_none());
    }
    let mut batch = Batch::new();
    batch.delete::<Other>(1).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    let mut batch = Batch::new();
    for id in 1..=4 {
        batch.delete::<NewDocument>(id).unwrap();
    }
    batch.delete::<Other>(1).unwrap();
    db.commit(batch).unwrap();
    assert!(db.get::<Other>(1).unwrap().is_none());
}
