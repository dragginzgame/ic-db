//! Private maintenance controls; production authorization/endpoints belong to Canic.
use crate::{Entry, KEY, compiled, runtime};
use ic_db::{
    Batch, Database, Entity, Error, Maintenance, QueryCursor, Schema, SchemaCopy, SchemaTransition,
};
use ic_platform_runtime::cdk;
use ic_storage::StableKey;

pub const COPY_SOURCE: &str = "m1.copy.source.v1";
pub const COPY_TARGET: &str = "m1.copy.target.v1";

#[ic_db::entity(name = "Entry", pk(field = "id", source = "external"),
    index(fields = ["category", "id"]), fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat64"))),
        field(name = "amount", value(item(prim = "Nat128")))
))]
struct EntryV2 {}

fn declaration(inputs: &[u8]) -> Result<(Schema, SchemaTransition), String> {
    let version = if cfg!(feature = "maintenance-v2") {
        2
    } else {
        1
    };
    let source = compiled();
    let entities: Vec<_> = source
        .entities()
        .iter()
        .map(|entity| {
            if entity.id == EntryV2::ID {
                EntryV2::SCHEMA
            } else {
                *entity
            }
        })
        .collect();
    let target = Schema::new(&entities).map_err(|error| format!("{error:?}"))?;
    let transition = SchemaTransition::new(&source, &target, "widen-entry-amount", version, inputs)
        .map_err(|error| format!("{error:?}"))?;
    Ok((source, transition))
}

#[cdk::update(
    guard = "crate::transport::admit::<133>",
    decode_with = "crate::transport::decode::<_, 133>",
    encode_with = "crate::transport::encode::<_, 133>"
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
        cdk::api::trap("maintenance fixture trap after operation");
    }
    Ok(())
}

type SourcePage = (Vec<(u64, u64, u64)>, Option<Vec<u8>>);

#[cdk::query(
    guard = "crate::transport::admit::<134>",
    decode_with = "crate::transport::decode::<_, 134>",
    encode_with = "crate::transport::encode::<_, 134>"
)]
fn maintenance_source(
    (after, rows, bytes): (Option<Vec<u8>>, u8, u32),
) -> Result<SourcePage, String> {
    let (source, transition) = declaration(&[9])?;
    let cursor = after
        .as_deref()
        .map(QueryCursor::from_bytes)
        .transpose()
        .map_err(|error| format!("{error:?}"))?;
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

fn copy_declaration() -> Result<(Schema, Schema, SchemaTransition), Error> {
    let source = Schema::new(&[Entry::SCHEMA])?;
    let target = Schema::new(&[EntryV2::SCHEMA])?;
    let version = if cfg!(feature = "maintenance-v2") {
        2
    } else {
        1
    };
    let transition = SchemaTransition::new(&source, &target, "copy-entry", version, &[])?;
    Ok((source, target, transition))
}

fn copy_control(operation: u8, limit: u8) -> Result<(u64, bool), Error> {
    let (source, target, transition) = copy_declaration()?;
    let host = runtime();
    let source_memory = host
        .open_memory(&StableKey::parse(COPY_SOURCE).unwrap())
        .unwrap();
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
    let target_memory = host
        .open_memory(&StableKey::parse(COPY_TARGET).unwrap())
        .unwrap();
    if operation == 1 {
        Maintenance::resume(source_memory, &transition)?.begin_copy(
            target_memory,
            source,
            target,
            [4; 16],
        )?;
        return Ok((0, false));
    }
    if operation == 6 {
        let mut db = Database::open(target_memory, target)?;
        let mut batch = Batch::new();
        batch.replace(&EntryV2 {
            id: 1,
            category: 1,
            amount: 999,
        })?;
        db.commit(batch)?;
        return Ok((1, true));
    }
    let mut copy = SchemaCopy::resume(source_memory, target_memory, source, target, &transition)?;
    match operation {
        2 => {
            let step =
                copy.copy_next::<Entry, EntryV2>(usize::from(limit), 128 * 1024, |row, _| {
                    Ok(EntryV2 {
                        id: row.id,
                        category: row.category,
                        amount: u128::from(row.amount) + 100,
                    })
                })?;
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

#[cdk::update(
    guard = "crate::transport::admit::<135>",
    decode_with = "crate::transport::decode::<_, 135>",
    encode_with = "crate::transport::encode::<_, 135>"
)]
fn maintenance_copy((operation, limit, trap): (u8, u8, bool)) -> Result<(u64, bool), String> {
    let result = copy_control(operation, limit).map_err(|error| format!("{error:?}"))?;
    if trap {
        cdk::api::trap("copy fixture trap after operation");
    }
    Ok(result)
}

// Independent supplier inspection, not a product escape hatch for candidate reads.
type CopyEntries = (bool, Vec<(Vec<u8>, Vec<u8>)>);

#[cdk::query(
    guard = "crate::transport::admit::<136>",
    decode_with = "crate::transport::decode::<_, 136>",
    encode_with = "crate::transport::encode::<_, 136>"
)]
fn maintenance_copy_entries(target: bool) -> CopyEntries {
    let host = runtime();
    let key = StableKey::parse(if target { COPY_TARGET } else { COPY_SOURCE }).unwrap();
    let (source_schema, target_schema, _) = copy_declaration().unwrap();
    let open = Database::open(
        host.open_memory(&key).unwrap(),
        if target { target_schema } else { source_schema },
    )
    .is_ok();
    let raw = ic_storage::adapter::StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(
        host.open_memory(&key).unwrap(),
    )
    .unwrap();
    (
        open,
        raw.iter()
            .take(32)
            .map(|entry| (entry.key().clone(), entry.value()))
            .collect(),
    )
}
