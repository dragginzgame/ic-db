//! Fixed local demonstration; product manifests own all migration progress.
use crate::{Backing, runtime};
use ic_db::{Batch, Database, Entity, Error, RetiredSource, Schema, SchemaCopy, SchemaTransition};
use ic_storage::StableKey;

pub(crate) const SOURCE: &str = "m1.console.migration.source.v1";
pub(crate) const TARGET: &str = "m1.console.migration.target.v1";

#[ic_db::entity(
    name = "Item",
    pk(field = "id", source = "external"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "amount", value(item(prim = "Nat64")))
    )
)]
struct Before {}

#[ic_db::entity(name = "Item", pk(field = "id", source = "external"),
    index(fields = ["label"]), fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "amount", value(item(prim = "Nat128"))),
        field(name = "label", value(item(prim = "Text", max_bytes = 16)))
))]
struct After {}

#[expect(
    clippy::needless_pass_by_value,
    reason = "SchemaCopy consumes each decoded source row"
)]
fn transform(row: Before, _: &[u8]) -> Result<After, Error> {
    Ok(After {
        id: row.id,
        amount: u128::from(row.amount),
        label: "migrated".into(),
    })
}

// Private fixed command numbers, not a schema language or a general admin API:
// seed, freeze, copy, validate, publish, cleanup, target read, source read.
type Reply = (bool, Vec<Vec<String>>);

/// Run the selected step of the console migration demonstration.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "migration_demo",
    guard = crate::transport::admit::<142>,
    decode = crate::transport::decode::<_, 142>,
    encode = crate::transport::encode::<_, 142>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn migration_demo(action: u8) -> Result<Reply, String> {
    run(action).map_err(|error| error.to_string())
}

fn run(action: u8) -> Result<Reply, Error> {
    let host = runtime();
    let source_schema = Schema::new(&[Before::SCHEMA])?;
    let target_schema = Schema::new(&[After::SCHEMA])?;
    let transition =
        SchemaTransition::new(&source_schema, &target_schema, "console-item-label", 1, &[])?;
    let source = || {
        host.open_memory(&StableKey::parse(SOURCE).unwrap())
            .unwrap()
    };
    let target = || {
        host.open_memory(&StableKey::parse(TARGET).unwrap())
            .unwrap()
    };
    let resume = || {
        SchemaCopy::resume(
            source(),
            target(),
            source_schema.clone(),
            target_schema.clone(),
            &transition,
        )
    };
    let complete = match action {
        0 => {
            let mut db = Database::create(source(), source_schema, [20; 16])?;
            let mut batch = Batch::new();
            for id in 1..=6 {
                batch.insert(&Before {
                    id,
                    amount: id * 10,
                })?;
            }
            db.commit(batch)?;
            return source_rows(&db);
        }
        1 => {
            Database::open(source(), source_schema.clone())?
                .begin_maintenance(&transition)?
                .begin_copy(target(), source_schema, target_schema, [21; 16])?;
            true
        }
        2 => {
            let step = resume()?.copy_next::<Before, After>(2, 4096, transform)?;
            return Ok((step.complete, vec![vec![step.rows.to_string()]]));
        }
        3 => resume()?.validate_next(2)?,
        4 => {
            resume()?.publish()?;
            true
        }
        5 => RetiredSource::resume(source(), &transition, [20; 16], [21; 16])?.cleanup_step(2)?,
        6 => {
            let db = Database::open(target(), target_schema)?;
            // Read through the rebuilt index, not a client-side sort or filter.
            let rows = db.index_prefix::<After>("label", &["migrated".into()], 32)?;
            return Ok((
                true,
                rows.into_iter()
                    .map(|row| vec![row.id.to_string(), row.amount.to_string(), row.label])
                    .collect(),
            ));
        }
        7 => return source_rows(&Database::open(source(), source_schema)?),
        _ => return Err(Error::InvalidTransition),
    };
    Ok((complete, vec![]))
}

fn source_rows(db: &Database<Backing>) -> Result<Reply, Error> {
    Ok((
        true,
        db.page::<Before>(None, 32)?
            .rows
            .into_iter()
            .map(|row| vec![row.id.to_string(), row.amount.to_string()])
            .collect(),
    ))
}
