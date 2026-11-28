//! Cross-chunk references during copy, validation, cancellation and publication.

use crate::maintenance::{RawPage, raw_page};
use crate::{
    faults,
    handles::{Database, Maintenance, RetiredSource, SchemaCopy},
    maintenance::{COPY_SOURCE, COPY_TARGET},
    runtime,
};
use ic_db::design::prelude::*;
use ic_db::{Batch, Error, SchemaTransition};
use ic_storage::StableKey;
use records_schema::OptionalLink;

type ReferenceReply = (Result<(u32, bool), u8>, u64, u64, [u64; 4]);

// migration_references
// Advance the reference migration probe with optional failure injection.
// guard  : validates the request frame for method 144.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "migration_references",
    guard = crate::transport::admit::<144>,
    decode = crate::transport::decode_measured::<_, 144>,
    encode = crate::transport::encode::<_, 144>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn migration_references(
    ((operation, first, limit, bad, stop), counter): crate::transport::Measured<(
        u8,
        u16,
        u8,
        bool,
        u64,
    )>,
) -> ReferenceReply {
    let start = counter();
    faults::arm(if stop == u64::MAX { 0 } else { stop }, false, false);
    // Mark setup, Database::open, batch construction, and commit separately.
    let mut previous = start;
    let mut phases = [0; 4];
    let mut slots = phases.iter_mut();
    let mut mark = || {
        let now = counter();
        *slots.next().unwrap() = now - previous;
        previous = now;
    };
    let result = control(operation, first, limit, bad, &mut mark).map_err(|error| match error {
        Error::ReferenceViolation => 1,
        Error::Maintenance => 2,
        Error::InvalidTransition => 3,
        Error::Limit => 4,
        other => panic!("unexpected reference migration refusal: {other:?}"),
    });
    if stop == u64::MAX {
        crate::system::trap("reference migration trap after operation");
    }
    let (writes, _) = faults::finish();

    (result, writes, counter() - start, phases)
}

// migration_references_raw
// Page raw source or target entries for the reference migration oracle.
// guard  : validates the request frame for method 145.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "migration_references_raw",
    guard = crate::transport::admit::<145>,
    decode = crate::transport::decode::<_, 145>,
    encode = crate::transport::encode::<_, 145>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn migration_references_raw((target, after): (bool, Option<Vec<u8>>)) -> RawPage {
    raw_page(if target { COPY_TARGET } else { COPY_SOURCE }, after, 16)
}

///
/// Target
/// Exercises references to `Self` on rows keyed by `id`.
///

#[entity(
    store = records_schema::RecordsStore,
    name = "OptionalLink",
    pk(field = "id"),
    index(fields = ["next", "id"]),
    index(field = "rank", unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "next", value(opt, item(prim = "Nat64")), references(entity = "Self", index = "next,id")),
        field(name = "rank", value(item(prim = "Nat64")))
    )
)]
struct Target {}

fn control(
    operation: u8,
    first: u16,
    limit: u8,
    bad: bool,
    mark: &mut impl FnMut(),
) -> Result<(u32, bool), Error> {
    let source = Schema::new(&[OptionalLink::SCHEMA])?;
    let target = Schema::new(&[Target::SCHEMA])?;
    let transition =
        SchemaTransition::new(&source, &target, "reference-pairs", 1, &[u8::from(bad)])?;
    let host = runtime();
    let source_memory = host
        .open_memory(&StableKey::parse(COPY_SOURCE).unwrap())
        .unwrap();
    match operation {
        0 => {
            Database::create(source_memory, source, [12; 16])?;

            return Ok((0, false));
        }
        1 | 2 => {
            assert!(first < 256 && first.is_multiple_of(32));
            let mut db = Database::open(source_memory, source)?;
            let mut batch = Batch::new();
            for id in u64::from(first)..u64::from(first) + 32 {
                let row = OptionalLink {
                    id,
                    next: Some(if operation == 1 { id } else { (id + 128) % 256 }),
                };

                if operation == 1 {
                    batch.insert(&row)?;
                } else {
                    batch.replace(&row)?;
                }
            }
            db.commit(batch)?;

            return Ok((32, false));
        }
        3 => {
            Database::open(source_memory, source)?.begin_maintenance(&transition)?;

            return Ok((0, false));
        }
        8 => {
            let mut retired =
                RetiredSource::resume(source_memory, &transition, [12; 16], [13; 16])?;

            return Ok((0, retired.cleanup_step(usize::from(limit))?));
        }
        10 => {
            Database::open(source_memory, source)?;

            return Ok((0, true));
        }
        _ => {}
    }
    let target_memory = host
        .open_memory(&StableKey::parse(COPY_TARGET).unwrap())
        .unwrap();
    if operation == 11 {
        Database::open(target_memory, target)?;

        return Ok((0, true));
    }
    if operation == 12 {
        let mut db = Database::open(target_memory, target)?;
        let mut batch = Batch::new();
        batch.delete::<Target>(0)?;
        if first == 1 {
            batch.delete::<Target>(128)?;
        }
        db.commit(batch)?;

        return Ok((0, true));
    }
    if matches!(operation, 13..=15) {
        mark();
        let mut db = Database::open(target_memory, target)?;
        mark();
        let mut batch = Batch::new();
        if operation == 14 {
            // These 31 dependants do not include every surviving reference.
            batch.delete::<Target>(255)?;
            assert!(matches!(first, 1 | 224));
            for id in u64::from(first)..u64::from(first) + 31 {
                batch.delete::<Target>(id)?;
            }
        } else {
            assert!(first < 256 && first.is_multiple_of(32));
            for id in u64::from(first)..u64::from(first) + 32 {
                // The preceding pair-deletion witness already removed these rows.
                if matches!(id, 0 | 128) {
                    continue;
                }

                if operation == 13 {
                    batch.replace(&Target {
                        id,
                        next: Some(255),
                        rank: id + 1000,
                    })?;
                } else {
                    batch.delete::<Target>(id)?;
                }
            }
        }
        mark();
        let result = db.commit(batch);
        mark();
        result?;

        return Ok((0, true));
    }
    if operation == 4 {
        Maintenance::resume(source_memory, &transition)?.begin_copy(
            target_memory,
            source,
            target,
            [13; 16],
        )?;

        return Ok((0, false));
    }
    let mut copy = SchemaCopy::resume(source_memory, target_memory, source, target, &transition)?;

    match operation {
        5 => {
            let step =
                copy.copy_next::<OptionalLink, Target>(usize::from(limit), 4096, |row, inputs| {
                    Ok(Target {
                        id: row.id,
                        next: if row.id == 255 && inputs == [1] {
                            Some(999)
                        } else {
                            row.next
                        },
                        rank: row.id + 1000,
                    })
                })?;

            Ok((step.rows as u32, step.complete))
        }
        6 => Ok((0, copy.validate_next(usize::from(limit))?)),
        7 => {
            copy.publish()?;

            Ok((0, true))
        }
        9 => Ok((0, copy.cancel_step(usize::from(limit))?)),
        _ => panic!("unsupported reference migration control"),
    }
}
