//! Bounded migration envelope and real supplier growth refusal, fixture-only.

use crate::{
    faults,
    handles::{Database, Maintenance, RetiredSource, SchemaCopy},
    maintenance::{COPY_NEXT, COPY_SOURCE, COPY_TARGET},
    runtime,
};
use ic_db::design::prelude::*;
use ic_db::{Batch, Error, SchemaTransition};
use ic_storage::{StableKey, adapter::StableBTreeMap};
use records_schema::EnvelopeRow;
use std::ops::Bound;

// Counts include host reopen/allocation admission, product work and supplier calls.
// Typed refusal is returned normally; it must not be turned into a trap by the probe.

// migration_capacity
// Run a bounded migration operation under the selected supplier limits.
// guard  : validates the request frame for method 140.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "migration_capacity",
    guard = crate::transport::admit::<140>,
    decode = crate::transport::decode_measured::<_, 140>,
    encode = crate::transport::encode::<_, 140>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn migration_capacity(
    (((second, operation), first, limit, bytes, deny_growth, stop_after), counter): crate::transport::Measured<CapacityRequest>,
) -> (Result<(u64, bool), u8>, u64, u64, u64) {
    let start = counter();
    faults::arm(stop_after, deny_growth, false);
    let result = control(second, operation, first, limit, bytes).map_err(|error| match error {
        Failure::Database(Error::Limit) => 1,
        Failure::Database(Error::RowTooLarge) => 2,
        Failure::Database(Error::Exists) => 3,
        Failure::Database(error) => panic!("unexpected capacity refusal: {error:?}"),
        Failure::Runtime => 4,
    });
    let (writes, grows) = faults::finish();

    (result, writes, grows, counter() - start)
}

// migration_capacity_raw
// Page physical entries to check migration capacity and preservation.
// guard  : validates the request frame for method 141.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "migration_capacity_raw",
    guard = crate::transport::admit::<141>,
    decode = crate::transport::decode::<_, 141>,
    encode = crate::transport::encode::<_, 141>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn migration_capacity_raw((second, after): (bool, Option<Vec<u8>>)) -> RawPage {
    assert!(after.as_ref().is_none_or(|key| key.len() <= 512));
    let host = runtime();
    let raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(
        host.open_memory(&StableKey::parse(if second { COPY_NEXT } else { COPY_TARGET }).unwrap())
            .unwrap(),
    );
    let lower = after.map_or(Bound::Unbounded, Bound::Excluded);
    // Match the existing write-envelope inspector: two maximal rows fit Wire.
    let rows: Vec<_> = raw
        .range((lower, Bound::Unbounded))
        .take(2)
        .map(|entry| (entry.key().clone(), entry.value()))
        .collect();
    let next = (rows.len() == 2).then(|| rows.last().unwrap().0.clone());

    (rows, next)
}

///
/// Source
/// Exercises an alternate `EnvelopeRow` declaration under the same logical identity.
///

#[entity(
    store = records_schema::RecordsStore,
    name = "EnvelopeRow",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "a", value(item(prim = "Text", max_bytes = 1024))),
        field(name = "b", value(item(prim = "Text", max_bytes = 1024))),
        field(name = "c", value(item(prim = "Text", max_bytes = 1024))),
        field(name = "d", value(item(prim = "Text", max_bytes = 1024))),
        field(name = "padding", value(item(prim = "Blob", max_bytes = 4096)))
    )
)]
struct Source {}

enum Failure {
    Database(Error),
    Runtime,
}
impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Database(error)
    }
}

fn control(
    second: bool,
    operation: u8,
    first: u16,
    limit: u8,
    bytes: u32,
) -> Result<(u64, bool), Failure> {
    let indexed = Schema::new(&[EnvelopeRow::SCHEMA])?;
    let (source, target, source_key, target_key, source_id, target_id, name) = if second {
        assert!(operation >= 2);
        (
            indexed,
            Schema::new(&[EnvelopeRow::SCHEMA, records_schema::Entry::SCHEMA])?,
            COPY_TARGET,
            COPY_NEXT,
            11,
            12,
            "add-empty-entry",
        )
    } else {
        (
            Schema::new(&[Source::SCHEMA])?,
            indexed,
            COPY_SOURCE,
            COPY_TARGET,
            10,
            11,
            "build-eight-indexes",
        )
    };
    let transition = SchemaTransition::new(&source, &target, name, 1, &[])?;
    let host = runtime();
    let source_memory = host
        .open_memory(&StableKey::parse(source_key).unwrap())
        .map_err(|_| Failure::Runtime)?;
    if operation == 0 {
        Database::create(source_memory, source, [source_id; 16])?;

        return Ok((0, false));
    }
    if operation == 1 {
        assert!(matches!(first, 1 | 33 | 65 | 97));
        let mut db = Database::open(source_memory, source)?;
        let mut batch = Batch::new();
        for id in u64::from(first)..u64::from(first) + 32 {
            let text = "\0".repeat(60);
            batch.insert(&Source {
                id,
                a: format!("abcdefg{text}"),
                b: text.clone(),
                c: text.clone(),
                d: text,
                padding: vec![0; 3821],
            })?;
        }
        db.commit(batch)?;

        return Ok((32, false));
    }
    if operation == 2 {
        Database::open(source_memory, source)?.begin_maintenance(&transition)?;

        return Ok((0, false));
    }
    if operation == 7 {
        let mut retired =
            RetiredSource::resume(source_memory, &transition, [source_id; 16], [target_id; 16])?;

        return Ok((0, retired.cleanup_step(usize::from(limit))?));
    }
    let target_memory = host
        .open_memory(&StableKey::parse(target_key).unwrap())
        .map_err(|_| Failure::Runtime)?;
    if operation == 3 {
        Maintenance::resume(source_memory, &transition)?.begin_copy(
            target_memory,
            source,
            target,
            [target_id; 16],
        )?;

        return Ok((0, false));
    }
    let mut copy = SchemaCopy::resume(source_memory, target_memory, source, target, &transition)?;

    match operation {
        4 if second => {
            let step = copy.copy_unchanged::<EnvelopeRow>(usize::from(limit), bytes as usize)?;

            Ok((step.rows as u64, step.complete))
        }
        4 => {
            let step = copy.copy_next::<Source, EnvelopeRow>(
                usize::from(limit),
                bytes as usize,
                |row, _| {
                    Ok(EnvelopeRow {
                        id: row.id,
                        a: row.a,
                        b: row.b,
                        c: row.c,
                        d: row.d,
                        padding: row.padding,
                    })
                },
            )?;

            Ok((step.rows as u64, step.complete))
        }
        5 => Ok((0, copy.validate_next(usize::from(limit))?)),
        6 => {
            copy.publish()?;

            Ok((0, true))
        }
        _ => panic!("unsupported capacity control"),
    }
}

type CapacityRequest = ((bool, u8), u16, u8, u32, bool, u64);
type RawPage = (Vec<(Vec<u8>, Vec<u8>)>, Option<Vec<u8>>);
