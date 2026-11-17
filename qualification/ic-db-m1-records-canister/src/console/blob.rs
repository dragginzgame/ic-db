//! Fixed Blob result shapes for the local console; the engine owns all query work.
use super::{Page, display, display_groups, display_scalar, extremum};
use crate::{Backing, schema::BinaryObject};
use ic_db::{Entity, Error, Query};
use std::fmt::Write;

#[ic_db::record(fields(field(name = "payload", value(item(prim = "Blob", max_bytes = 1024)))))]
pub(super) struct Payload {}
#[ic_db::record(fields(field(
    name = "optional",
    value(opt, item(prim = "Blob", max_bytes = 32))
)))]
pub(super) struct Optional {}

pub(super) fn query(
    db: &ic_db::Database<Backing>,
    minimum: Option<u64>,
    category: Option<u64>,
    entry_id: Option<u64>,
) -> Result<Query<'_, BinaryObject, Backing>, String> {
    if minimum.is_some() || category.is_some() || entry_id.is_some() {
        return Err("--min, --category and --entry-id do not apply to BinaryObject".into());
    }
    Ok(db.query::<BinaryObject>())
}

pub(super) fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(2 + bytes.len() * 2);
    text.push_str("0x");
    for byte in bytes {
        write!(text, "{byte:02x}").expect("writing to String cannot fail");
    }
    text
}

pub(super) fn execute<E: Entity>(
    query: Query<'_, E, Backing>,
    select: &str,
) -> Result<Page, Error> {
    match select {
        "payload" => query
            .fetch_as::<Payload>()
            .map(|p| display(p, &["payload"], |r| vec![hex(&r.payload)])),
        "optional" => query.fetch_as::<Optional>().map(|p| {
            display(p, &["optional"], |r| {
                vec![r.optional.map_or_else(|| "—".into(), |v| hex(&v))]
            })
        }),
        "min:payload" | "max:payload" => {
            extremum(query, select == "max:payload", "payload", |r: Payload| {
                hex(&r.payload)
            })
        }
        "min:optional" | "max:optional" => extremum(
            query,
            select == "max:optional",
            "optional",
            |r: Optional| r.optional.map_or_else(|| "—".into(), |v| hex(&v)),
        ),
        "distinct-count:payload" => query
            .count_distinct::<Payload>()
            .map(|r| display_scalar(&r, "count")),
        "payload,count" => query
            .group_count::<Payload>()
            .map(|r| display_groups(r, "payload", |k| hex(&k.payload))),
        "payload,count:optional" => query
            .group_count_field::<Payload>("optional")
            .map(|r| display_groups(r, "payload", |k| hex(&k.payload))),
        _ => Err(Error::InvalidQuery),
    }
}
