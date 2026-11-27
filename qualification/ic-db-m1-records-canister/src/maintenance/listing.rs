//! Installed recovery probe for replacing a populated document reference index.

use super::{COPY_SOURCE, COPY_TARGET, TRANSITION_VERSION};
use crate::{
    handles::{Database, Query, SchemaCopy},
    runtime,
};
use ic_db::{Batch, Error, QueryCursor, SchemaTransition, design::prelude::*};
use ic_storage::StableKey;
use records_schema::{Document, Entry, RecordsStore};

// maintenance_listing
// Exercise one fixed index transition across installed executions.
// guard  : validates method 191's request.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "maintenance_listing",
    guard = crate::transport::admit::<191>,
    decode = crate::transport::decode::<_, 191>,
    encode = crate::transport::encode::<_, 191>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn maintenance_listing((operation, after): (u8, Option<Vec<u8>>)) -> Result<Page, String> {
    step(operation, after.as_deref()).map_err(|error| format!("{error:?}"))
}

///
/// After
/// Keeps the source document layout and assigns the wider index to its reference.
///

#[entity(
    store = RecordsStore,
    name = "Document",
    pk(field = "id"),
    index(fields = ["entry_id", "published", "score"]),
    index(fields = ["published", "title"]),
    index(fields = ["entry_id", "priority"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "entry_id", value(item(prim = "Nat64")), references(entity = "Entry", index = "entry_id,published,score")),
        field(name = "title", value(item(prim = "Text", max_bytes = 32))),
        field(name = "note", value(opt, item(prim = "Text", max_bytes = 64))),
        field(name = "published", value(item(prim = "Bool"))),
        field(name = "score", value(item(prim = "Int64"))),
        field(name = "previous_score", value(opt, item(prim = "Int64"))),
        field(name = "priority", value(opt, item(prim = "Nat64")))
    )
)]
struct After {}

fn replace_index(row: Document, _: &[u8]) -> Result<After, Error> {
    Ok(After {
        id: row.id,
        entry_id: row.entry_id,
        title: row.title,
        note: row.note,
        published: row.published,
        score: row.score,
        previous_score: row.previous_score,
        priority: row.priority,
    })
}

type Page = (Vec<u64>, Option<Vec<u8>>);

fn step(operation: u8, after: Option<&[u8]>) -> Result<Page, Error> {
    let source = Schema::new(&[Document::SCHEMA, Entry::SCHEMA])?;
    let target = Schema::new(&[After::SCHEMA, Entry::SCHEMA])?;
    let transition =
        SchemaTransition::new(&source, &target, "document-index", TRANSITION_VERSION, &[])?;
    let host = runtime();
    let memory = |name| host.open_memory(&StableKey::parse(name).unwrap()).unwrap();
    match operation {
        0 => {
            let mut db = Database::create(memory(COPY_SOURCE), source.clone(), [1; 16])?;
            let mut batch = Batch::new();
            batch.insert(&Entry {
                id: 1,
                category: 0,
                amount: 0,
            })?;
            for (id, published, score) in
                [(1, true, 7), (2, false, 99), (3, true, 7), (4, true, -1)]
            {
                batch.insert(&Document {
                    id,
                    entry_id: 1,
                    title: format!("doc{id}"),
                    note: None,
                    published,
                    score,
                    previous_score: None,
                    priority: Some(id),
                })?;
            }
            db.commit(batch)?;
            let first = source_page(&db, None)?;
            let mut copy = db.begin_maintenance(&transition)?.begin_copy(
                memory(COPY_TARGET),
                source,
                target,
                [2; 16],
            )?;
            assert_eq!(copy.copy_next(1, 4096, replace_index)?.rows, 1);

            return Ok(first);
        }
        1 | 2 | 5 => {
            let mut copy = SchemaCopy::resume(
                memory(COPY_SOURCE),
                memory(COPY_TARGET),
                source,
                target,
                &transition,
            )?;
            if operation == 1 {
                assert_eq!(copy.copy_next(4, 4096, replace_index)?.rows, 3);
                assert!(copy.copy_unchanged::<Entry>(2, 4096)?.complete);
            } else if operation == 5 {
                copy.cancel_step(1)?;
            } else if copy.validate_next(4)? {
                copy.publish()?;
            }
        }
        3 => {
            let db = Database::open(memory(COPY_TARGET), target)?;
            let query = db
                .query::<After>()
                .eq(After::ENTRY_ID, 1_u64)
                .eq(After::PUBLISHED, true)
                .order_by(&[After::SCORE, After::ID]);

            return listing_page(query, after);
        }
        4 => {
            let mut db = Database::open(memory(COPY_TARGET), target)?;
            let mut batch = Batch::new();
            batch.delete::<Entry>(1)?;
            db.commit(batch)?;
        }
        6 => return source_page(&Database::open(memory(COPY_SOURCE), source)?, after),
        _ => return Err(Error::InvalidQuery),
    }

    Ok((vec![], None))
}

fn source_page(db: &Database, after: Option<&[u8]>) -> Result<Page, Error> {
    let query = db
        .query::<Document>()
        .eq(Document::ENTRY_ID, 1_u64)
        .eq(Document::PUBLISHED, true)
        .order_by(&[Document::SCORE, Document::ID]);

    listing_page(query, after)
}

fn listing_page<E: Entity<Key = u64>>(
    query: Query<'_, E>,
    after: Option<&[u8]>,
) -> Result<Page, Error> {
    let cursor = after.map(QueryCursor::from_bytes).transpose()?;
    let page = query.descending().limit(2).after(cursor.as_ref()).fetch()?;

    Ok((
        page.rows.iter().map(Entity::key).collect(),
        page.next.map(|cursor| cursor.to_bytes()),
    ))
}
