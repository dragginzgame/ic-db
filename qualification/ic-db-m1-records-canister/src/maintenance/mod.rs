//! Private maintenance controls; production authorization/endpoints belong to Canic.

#[cfg(feature = "aggregate-jobs")]
mod jobs;
mod listing;

use crate::raw_map::RawMap;
use crate::{
    Document, Entry, KEY, RecordsStore,
    handles::{Database, Maintenance, RetiredSource, SchemaCopy},
    runtime,
    schema::ManagedRow,
};
use ic_db::design::prelude::*;
use ic_db::{Batch, EntitySchema, Error, SchemaTransition};
use ic_storage::{StableKey, adapter::StableCell};

#[cfg(feature = "maintenance-faults")]
pub(super) type RawPage = (
    Vec<(
        Vec<u8>,
        ic_db_qualification_transport::fixture_wire::ByteBuf,
    )>,
    Option<Vec<u8>>,
);

#[cfg(feature = "maintenance-faults")]
pub(super) fn raw_page(key: &str, after: Option<Vec<u8>>, limit: usize) -> RawPage {
    use std::ops::Bound;
    assert!(after.as_ref().is_none_or(|key| key.len() <= 512));
    let host = runtime();
    let raw = RawMap::load(host.open_memory(&StableKey::parse(key).unwrap()).unwrap());
    let lower = after.map_or(Bound::Unbounded, Bound::Excluded);
    let rows: Vec<_> = raw
        .range((lower, Bound::Unbounded))
        .take(limit)
        .map(|entry| (entry.key().0.clone(), entry.value().into()))
        .collect();
    let next = (rows.len() == limit).then(|| rows.last().unwrap().0.clone());
    (rows, next)
}

// maintenance_control
// Start, cancel or inspect the selected maintenance transition.
// guard  : validates the request frame for method 133.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "maintenance_control",
    guard = crate::transport::admit::<133>,
    decode = crate::transport::decode::<_, 133>,
    encode = crate::transport::encode::<_, 133>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn maintenance_control((operation, inputs, trap): (u8, Vec<u8>, bool)) -> Result<(), String> {
    let (source, transition) = declaration(&inputs)?;
    let host = runtime();
    let memory = host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap();
    match operation {
        0 => {
            Database::open(memory, source)
                .and_then(|db| db.begin_maintenance(&transition))
                .map_err(|error| format!("{error:?}"))?;
        }
        1 | 2 => {
            let maintenance =
                Maintenance::resume(memory, &transition).map_err(|error| format!("{error:?}"))?;
            if operation == 2 {
                maintenance.cancel();
            }
        }
        _ => return Err("unsupported maintenance control".into()),
    }
    if trap {
        crate::system::trap("maintenance fixture trap after operation");
    }

    Ok(())
}

// maintenance_source
// Read a bounded page from the preserved source.
// guard  : validates the request frame for method 134.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "maintenance_source",
    guard = crate::transport::admit::<134>,
    decode = crate::transport::decode::<_, 134>,
    encode = crate::transport::encode::<_, 134>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn maintenance_source(
    (after, rows, bytes): (Option<Vec<u8>>, u8, u32),
) -> Result<SourcePage, String> {
    let (source, transition) = declaration(&[9])?;
    let cursor =
        crate::queries::decode_cursor(after.as_deref()).map_err(|error| format!("{error:?}"))?;
    let host = runtime();
    let memory = host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap();
    let maintenance =
        Maintenance::resume(memory, &transition).map_err(|error| format!("{error:?}"))?;
    let page = maintenance
        .source_page::<Entry>(&source, cursor.as_ref(), usize::from(rows), bytes as usize)
        .map_err(|error| format!("{error:?}"))?;

    Ok((
        page.rows
            .into_iter()
            .map(|row| (row.id, row.category, row.amount))
            .collect(),
        page.next.map(|cursor| cursor.to_bytes()),
    ))
}

// maintenance_copy
// Advance the selected copy phase with a bounded chunk.
// guard  : validates the request frame for method 135.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "maintenance_copy",
    guard = crate::transport::admit::<135>,
    decode = crate::transport::decode::<_, 135>,
    encode = crate::transport::encode::<_, 135>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn maintenance_copy((operation, limit, trap): (u8, u8, bool)) -> Result<(u64, bool), String> {
    let result = copy_control(operation, limit).map_err(|error| format!("{error:?}"))?;
    if trap {
        crate::system::trap("copy fixture trap after operation");
    }

    Ok(result)
}

// maintenance_copy_entries
// Inspect physical entries on the source or target side of the copy.
// guard  : validates the request frame for method 136.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "maintenance_copy_entries",
    guard = crate::transport::admit::<136>,
    decode = crate::transport::decode::<_, 136>,
    encode = crate::transport::encode::<_, 136>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn maintenance_copy_entries(target: bool) -> CopyEntries {
    let host = runtime();
    let (key, entry) = if target {
        (COPY_TARGET, EntryV2::SCHEMA)
    } else {
        (COPY_SOURCE, Entry::SCHEMA)
    };
    let key = StableKey::parse(key).unwrap();
    let schema = Schema::new(&[entry]).unwrap();
    let open = Database::open(host.open_memory(&key).unwrap(), schema).is_ok();
    let raw = RawMap::load(host.open_memory(&key).unwrap());

    (
        open,
        raw.iter()
            .take(32)
            .map(|entry| (entry.key().0.clone(), entry.value()))
            .collect(),
    )
}

// maintenance_managed_copy
// Exercise managed-field preservation through a schema copy.
// guard  : validates the request frame for method 137.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "maintenance_managed_copy",
    guard = crate::transport::admit::<137>,
    decode = crate::transport::decode::<_, 137>,
    encode = crate::transport::encode::<_, 137>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn maintenance_managed_copy((operation, trap): (u8, bool)) -> Result<ManagedCopyReply, String> {
    let result = managed_copy_control(operation).map_err(|error| format!("{error:?}"))?;
    if trap {
        crate::system::trap("managed copy fixture trap");
    }

    Ok(result)
}

// maintenance_whole_copy
// Exercise the whole-database copy and publication sequence.
// guard  : validates the request frame for method 138.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "maintenance_whole_copy",
    guard = crate::transport::admit::<138>,
    decode = crate::transport::decode::<_, 138>,
    encode = crate::transport::encode::<_, 138>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn maintenance_whole_copy((operation, trap): (u8, bool)) -> Result<WholeCopyReply, String> {
    let result = whole_copy_control(operation).map_err(|error| format!("{error:?}"))?;
    if trap {
        crate::system::trap("whole database copy fixture trap");
    }

    Ok(result)
}

// maintenance_next_copy
// Exercise a subsequent copy after the initial transition.
// guard  : validates the request frame for method 139.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "maintenance_next_copy",
    guard = crate::transport::admit::<139>,
    decode = crate::transport::decode::<_, 139>,
    encode = crate::transport::encode::<_, 139>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn maintenance_next_copy((operation, trap): (u8, bool)) -> Result<WholeCopyReply, String> {
    let result = next_copy_control(operation).map_err(|error| format!("{error:?}"))?;
    if trap {
        crate::system::trap("successive database copy fixture trap");
    }

    Ok(result)
}

const TRANSITION_VERSION: u32 = if cfg!(feature = "maintenance-v2") {
    2
} else {
    1
};

pub const COPY_SOURCE: &str = "m1.copy.source.v1";
pub const COPY_TARGET: &str = "m1.copy.target.v1";
pub const COPY_ROUTE: &str = "m1.copy.route.v1";
pub const COPY_NEXT: &str = "m1.copy.next.v1";

fn copy_memory(
    host: &ic_storage::MemoryRuntime<crate::Backing>,
    name: &str,
) -> ic_storage::RuntimeMemory<crate::Backing> {
    host.open_memory(&StableKey::parse(name).unwrap()).unwrap()
}

///
/// EntryV2
/// Exercises an alternate `Entry` declaration under the same logical identity.
///

#[entity(
    store = records_schema::RecordsStore,
    name = "Entry",
    pk(field = "id"),
    index(fields = ["category", "id"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat64"))),
        field(name = "amount", value(item(prim = "Nat128")))
    )
)]
struct EntryV2 {}

#[expect(
    clippy::needless_pass_by_value,
    reason = "copy_next transfers ownership of the decoded source row"
)]
fn widen_entry(row: Entry, _: &[u8]) -> Result<EntryV2, Error> {
    Ok(EntryV2 {
        id: row.id,
        category: row.category,
        amount: u128::from(row.amount) + 100,
    })
}

fn declaration(inputs: &[u8]) -> Result<(Schema, SchemaTransition), String> {
    let source = RecordsStore::compiled();
    let entities: Vec<_> = source
        .entities()
        .iter()
        .map(|entity| {
            if entity.id == EntryV2::SCHEMA.id {
                EntryV2::SCHEMA
            } else {
                *entity
            }
        })
        .collect();
    let target = Schema::new(&entities).map_err(|error| format!("{error:?}"))?;
    let transition = SchemaTransition::new(
        &source,
        &target,
        "widen-entry-amount",
        TRANSITION_VERSION,
        inputs,
    )
    .map_err(|error| format!("{error:?}"))?;

    Ok((source, transition))
}

type SourcePage = (Vec<(u64, u64, u64)>, Option<Vec<u8>>);

fn copy_control(operation: u8, limit: u8) -> Result<(u64, bool), Error> {
    let host = runtime();
    let target = Schema::new(&[EntryV2::SCHEMA])?;
    if operation == 6 {
        let memory = copy_memory(&host, COPY_TARGET);
        let mut db = Database::open(memory, target)?;
        let mut batch = Batch::new();
        batch.replace(&EntryV2 {
            id: 1,
            category: 1,
            amount: 999,
        })?;
        db.commit(batch)?;

        return Ok((1, true));
    }
    let source = Schema::new(&[Entry::SCHEMA])?;
    let transition =
        SchemaTransition::new(&source, &target, "copy-entry", TRANSITION_VERSION, &[])?;
    let source_memory = copy_memory(&host, COPY_SOURCE);
    if operation == 0 {
        let mut db = Database::create(source_memory, source, [3; 16])?;
        let mut batch = Batch::new();
        for id in 1..=6 {
            batch.insert(&Entry {
                id,
                category: id % 2,
                amount: id * 10,
            })?;
        }
        db.commit(batch)?;
        db.begin_maintenance(&transition)?;

        return Ok((0, false));
    }
    if operation == 7 {
        let mut retired = RetiredSource::resume(source_memory, &transition, [3; 16], [4; 16])?;

        return Ok((0, retired.cleanup_step(usize::from(limit))?));
    }
    let target_memory = copy_memory(&host, COPY_TARGET);
    if operation == 1 {
        Maintenance::resume(source_memory, &transition)?.begin_copy(
            target_memory,
            source,
            target,
            [4; 16],
        )?;

        return Ok((0, false));
    }
    let mut copy = SchemaCopy::resume(source_memory, target_memory, source, target, &transition)?;

    match operation {
        2 => {
            let step = copy.copy_next(usize::from(limit), 128 * 1024, widen_entry)?;

            Ok((step.rows as u64, step.complete))
        }
        3 => Ok((0, copy.cancel_step(usize::from(limit))?)),
        4 => Ok((0, copy.validate_next(usize::from(limit))?)),
        5 => {
            copy.publish()?;

            Ok((0, true))
        }
        _ => Err(Error::InvalidTransition),
    }
}

// Independent supplier inspection, not a product escape hatch for candidate reads.
type CopyEntries = (bool, Vec<(Vec<u8>, Vec<u8>)>);

///
/// ManagedV2
/// Exercises an alternate `ManagedRow` declaration under the same logical identity.
///

#[entity(
    store = records_schema::RecordsStore,
    name = "ManagedRow",
    pk(field = "id"),
    index(field = "body", unique),
    index(field = "changed"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "body", value(item(prim = "Text", max_bytes = 64))),
        field(name = "born", managed = "created_at", value(item(prim = "Timestamp"))),
        field(
            name = "changed",
            managed = "updated_at",
            value(item(prim = "Timestamp"))
        )
    )
)]
struct ManagedV2 {}

type ManagedCopyReply = (bool, Vec<(u64, String, i64, i64)>);

fn managed_copy_control(operation: u8) -> Result<ManagedCopyReply, Error> {
    let source = Schema::new(&[ManagedRow::SCHEMA])?;
    let target = Schema::new(&[ManagedV2::SCHEMA])?;
    let transition =
        SchemaTransition::new(&source, &target, "preserve-times", TRANSITION_VERSION, &[])?;
    let host = runtime();
    let source_memory = copy_memory(&host, COPY_SOURCE);
    let target_memory = copy_memory(&host, COPY_TARGET);
    if operation == 0 {
        let mut db = Database::create(source_memory, source.clone(), [5; 16])?;
        let mut batch = Batch::at_for_test(Timestamp::from_millis(-10));
        batch.insert_input(ManagedRow::insert().id(1).body("first".into()))?;
        batch.insert_input(ManagedRow::insert().id(2).body("second".into()))?;
        db.commit(batch)?;
        let mut row = db.get::<ManagedRow>(2)?.ok_or(Error::Missing)?;
        row.body = "updated".into();
        let mut batch = Batch::at_for_test(Timestamp::from_millis(20));
        batch.replace(&row)?;
        db.commit(batch)?;
        db.begin_maintenance(&transition)?
            .begin_copy(target_memory, source, target, [6; 16])?;

        return Ok((false, vec![]));
    }
    if operation == 5 || operation == 6 {
        let mut db = Database::open(target_memory, target)?;
        let millis = if operation == 6 {
            let mut row = db.get::<ManagedV2>(2)?.ok_or(Error::Missing)?;
            row.body = "after cutover".into();
            let mut batch = Batch::at_for_test(Timestamp::from_millis(30));
            batch.replace(&row)?;
            db.commit(batch)?;

            30
        } else {
            20
        };
        let rows =
            db.index_prefix::<ManagedV2>("changed", &[Timestamp::from_millis(millis).into()], 32)?;

        return Ok((
            true,
            rows.into_iter()
                .map(|row| {
                    (
                        row.id,
                        row.body,
                        row.born.as_millis(),
                        row.changed.as_millis(),
                    )
                })
                .collect(),
        ));
    }
    let mut copy = SchemaCopy::resume(source_memory, target_memory, source, target, &transition)?;
    let complete = match operation {
        1 => {
            copy.copy_next::<ManagedRow, ManagedV2>(1, 128 * 1024, |row, _| {
                Ok(ManagedV2 {
                    id: row.id,
                    body: row.body,
                    born: row.created,
                    changed: row.updated,
                })
            })?
            .complete
        }
        2 => {
            copy.copy_next::<ManagedRow, ManagedV2>(2, 128 * 1024, |row, _| {
                Ok(ManagedV2 {
                    id: row.id,
                    body: row.body,
                    born: row.created,
                    changed: if row.id == 2 {
                        Timestamp::from_millis(21)
                    } else {
                        row.updated
                    },
                })
            })?
            .complete
        }
        3 => copy.validate_next(2)?,
        4 => {
            copy.publish()?;

            true
        }
        _ => return Err(Error::InvalidTransition),
    };

    Ok((complete, vec![]))
}

///
/// CopyMapping
/// Exercises references to `Entry` on rows keyed by `id`.
///

#[entity(
    store = records_schema::RecordsStore,
    name = "A_Mapping",
    pk(field = "id"),
    index(field = "entry"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "entry",
            value(item(prim = "Nat64")),
            references(entity = "Entry")
        )
    )
)]
struct CopyMapping {}

///
/// CopyEmpty
/// Exercises an alternate `Z_Empty` declaration under the same logical identity.
///

#[entity(
    store = records_schema::RecordsStore,
    name = "Z_Empty",
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
struct CopyEmpty {}

type WholeCopyReply = (bool, Option<String>, Vec<(u64, u64, u128)>);

fn whole_schema(entry: &'static EntitySchema) -> Result<Schema, Error> {
    Schema::new(&[entry, CopyMapping::SCHEMA, CopyEmpty::SCHEMA])
}

fn whole_declaration() -> Result<(Schema, Schema, SchemaTransition), Error> {
    let source = whole_schema(Entry::SCHEMA)?;
    let target = whole_schema(EntryV2::SCHEMA)?;
    let transition =
        SchemaTransition::new(&source, &target, "whole-database", TRANSITION_VERSION, &[])?;

    Ok((source, target, transition))
}

fn whole_copy_control(operation: u8) -> Result<WholeCopyReply, Error> {
    let (source, target, transition) = whole_declaration()?;
    let host = runtime();
    let source_memory = copy_memory(&host, COPY_SOURCE);
    let target_memory = copy_memory(&host, COPY_TARGET);
    let jobs_enabled = cfg!(feature = "aggregate-jobs");
    if operation == 0 || (jobs_enabled && matches!(operation, 10 | 11)) {
        let db = if operation == 11 {
            Database::open(source_memory, source.clone())?
        } else {
            let mut db = Database::create(source_memory, source.clone(), [7; 16])?;
            let mut batch = Batch::new();
            for id in 0..2 {
                batch.insert(&CopyMapping {
                    id: id + 1,
                    entry: id,
                })?;
                batch.insert(&Entry {
                    id,
                    category: 1,
                    amount: id * 10,
                })?;
            }
            db.commit(batch)?;
            if operation == 10 {
                return Ok((false, None, vec![]));
            }

            db
        };
        let copy = db.begin_maintenance(&transition)?.begin_copy(
            target_memory,
            source,
            target,
            [8; 16],
        )?;

        return Ok((false, copy.next_entity().map(str::to_owned), vec![]));
    }
    if operation == 6 {
        let mut db = Database::open(target_memory, target)?;
        let mut batch = Batch::new();
        batch.delete::<EntryV2>(0)?;
        // A real ordinary-write refusal, projected as the response's boolean.
        let restricted = matches!(db.commit(batch), Err(Error::ReferenceViolation));
        let rows = db.index_prefix::<EntryV2>("category,id", &[1_u64.into()], 32)?;

        return Ok((
            restricted,
            None,
            rows.into_iter()
                .map(|row| (row.id, row.category, row.amount))
                .collect(),
        ));
    }
    if operation == 9 {
        let db = Database::open(source_memory, source)?;
        let rows = db
            .page::<Entry>(None, 32)?
            .rows
            .into_iter()
            .map(|row| (row.id, row.category, u128::from(row.amount)))
            .collect();

        return Ok((true, None, rows));
    }
    let mut copy = SchemaCopy::resume(source_memory, target_memory, source, target, &transition)?;
    let complete = match operation {
        1 => copy.copy_unchanged::<CopyMapping>(2, 4096)?.complete,
        2 => copy.copy_next(2, 4096, widen_entry)?.complete,
        3 => copy.copy_unchanged::<CopyEmpty>(2, 4096)?.complete,
        4 => copy.validate_next(2)?,
        5 => {
            copy.publish()?;

            return Ok((true, None, vec![]));
        }
        7 => copy.cancel_step(2)?,
        8 => {
            copy.copy_next::<CopyMapping, CopyMapping>(2, 4096, |mut row, _| {
                if row.id == 2 {
                    row.entry = 99;
                }

                Ok(row)
            })?
            .complete
        }
        _ => return Err(Error::InvalidTransition),
    };

    Ok((complete, copy.next_entity().map(str::to_owned), vec![]))
}

///
/// EntryV3
/// Exercises an alternate `Entry` declaration under the same logical identity.
///

#[entity(
    store = records_schema::RecordsStore,
    name = "Entry",
    pk(field = "id"),
    index(fields = ["category", "id"]),
    index(field = "note"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat64"))),
        field(name = "amount", value(item(prim = "Nat128"))),
        field(name = "note", value(item(prim = "Text", max_bytes = 32)))
    )
)]
struct EntryV3 {}

// The third allocation is fresh fixture memory, not a reset/reuse mechanism.
fn next_copy_control(operation: u8) -> Result<WholeCopyReply, Error> {
    let host = runtime();
    if operation == 7 {
        let (_, _, previous) = whole_declaration()?;
        let memory = copy_memory(&host, COPY_SOURCE);
        let mut retired = RetiredSource::resume(memory, &previous, [7; 16], [8; 16])?;

        return Ok((retired.cleanup_step(2)?, None, vec![]));
    }
    let source = whole_schema(EntryV2::SCHEMA)?;
    let source_memory = copy_memory(&host, COPY_TARGET);
    let route_memory = || copy_memory(&host, COPY_ROUTE);
    let load_route = || StableCell::<u8, _>::load(route_memory()).map_err(|_| Error::Corrupt);
    if operation == 0 {
        let mut db = Database::open(source_memory, source)?;
        let mut batch = Batch::new();
        batch.replace(&EntryV2 {
            id: 0,
            category: 1,
            amount: 999,
        })?;
        batch.insert(&EntryV2 {
            id: 2,
            category: 1,
            amount: 222,
        })?;
        batch.insert(&CopyMapping { id: 3, entry: 2 })?;
        db.commit(batch)?;
        StableCell::create(route_memory(), 2_u8);

        return Ok((true, None, vec![]));
    }
    let selected = if operation == 13 {
        Some(*load_route()?.get())
    } else {
        None
    };
    if operation == 10 || selected == Some(2) {
        let db = Database::open(source_memory, source)?;
        let rows = db
            .page::<EntryV2>(None, 32)?
            .rows
            .into_iter()
            .map(|row| (row.id, row.category, row.amount))
            .collect();

        return Ok((true, None, rows));
    }
    let target = Schema::new(&[
        EntryV3::SCHEMA,
        CopyMapping::SCHEMA,
        CopyEmpty::SCHEMA,
        ManagedRow::SCHEMA,
        Document::SCHEMA,
    ])?;
    let transition = SchemaTransition::new(
        &source,
        &target,
        "whole-database-note",
        TRANSITION_VERSION,
        &[],
    )?;
    if operation == 1 {
        Database::open(source_memory, source)?.begin_maintenance(&transition)?;

        return Ok((false, None, vec![]));
    }
    if operation == 8 {
        let mut retired = RetiredSource::resume(source_memory, &transition, [8; 16], [9; 16])?;

        return Ok((retired.cleanup_step(2)?, None, vec![]));
    }
    let target_memory = copy_memory(&host, COPY_NEXT);
    if let Some(version) = selected {
        if version != 3 {
            return Err(Error::Corrupt);
        }
        let db = Database::open(target_memory, target)?;
        let rows = db
            .page::<EntryV3>(None, 32)?
            .rows
            .into_iter()
            .map(|row| (row.id, row.category, row.amount))
            .collect();

        return Ok((true, None, rows));
    }
    if operation == 2 {
        let copy = Maintenance::resume(source_memory, &transition)?.begin_copy(
            target_memory,
            source,
            target,
            [9; 16],
        )?;

        return Ok((false, copy.next_entity().map(str::to_owned), vec![]));
    }
    if operation == 6 {
        let mut db = Database::open(target_memory, target)?;
        let mut batch = Batch::new();
        batch.delete::<EntryV3>(2)?;
        let restricted = matches!(db.commit(batch), Err(Error::ReferenceViolation));
        let rows = db.index_prefix::<EntryV3>("note", &["v3".into()], 32)?;

        return Ok((
            restricted,
            None,
            rows.into_iter()
                .map(|row| (row.id, row.category, row.amount))
                .collect(),
        ));
    }
    if matches!(operation, 11 | 12) {
        let mut db = Database::open(target_memory, target)?;
        let count = db.query::<ManagedRow>().count()?.value;
        let mut batch = Batch::at_for_test(Timestamp::from_millis(30));
        batch.insert_input(ManagedRow::insert().id(1).body("first".into()))?;
        // Transfer Entry 2's only inbound reference from a copied mapping to a new document.
        // Operation 12 points outside the copied rows to exercise refusal of the whole batch.
        batch.delete::<CopyMapping>(3)?;
        batch.insert(&Document {
            id: 1,
            entry_id: if operation == 12 { 99 } else { 2 },
            title: "first".into(),
            note: None,
            published: true,
            score: 1,
            previous_score: None,
            priority: None,
        })?;
        db.commit(batch)?;
        let documents = db.index_prefix::<Document>("entry_id,score", &[2_u64.into()], 2)?;

        return Ok((
            count == 0
                && db.get::<ManagedRow>(1)?.is_some()
                && db.get::<CopyMapping>(3)?.is_none()
                && matches!(documents.as_slice(), [doc] if doc.id == 1 && doc.entry_id == 2),
            None,
            vec![],
        ));
    }
    let mut copy = SchemaCopy::resume(source_memory, target_memory, source, target, &transition)?;
    let complete = match operation {
        3 => match copy.next_entity() {
            Some("A_Mapping") => copy.copy_unchanged::<CopyMapping>(2, 4096)?.complete,
            Some("Entry") => {
                copy.copy_next::<EntryV2, EntryV3>(2, 4096, |row, _| {
                    Ok(EntryV3 {
                        id: row.id,
                        category: row.category,
                        amount: row.amount + 1000,
                        note: "v3".into(),
                    })
                })?
                .complete
            }
            Some("Z_Empty") => copy.copy_unchanged::<CopyEmpty>(2, 4096)?.complete,
            _ => return Err(Error::InvalidTransition),
        },
        4 => copy.validate_next(2)?,
        5 => {
            let mut route = load_route()?;
            if *route.get() != 2 {
                return Err(Error::InvalidTransition);
            }
            copy.publish()?;
            route.set(3);

            return Ok((true, None, vec![]));
        }
        9 => copy.cancel_step(2)?,
        _ => return Err(Error::InvalidTransition),
    };

    Ok((complete, copy.next_entity().map(str::to_owned), vec![]))
}
