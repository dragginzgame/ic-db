//! Fixed Blob result shapes for the local console; the engine owns all query work.

use super::{Page, display, display_groups, display_scalar, extremum, hex};
use crate::{handles::Query, schema::BinaryObject, transport::PayloadFilters};
use ic_db::design::prelude::*;
use ic_db::{Comparison, Error};

///
/// Payload
/// Carries the `Blob` value of `payload`.
///

#[record(fields(field(name = "payload", value(item(prim = "Blob", max_bytes = 1024)))))]
pub(super) struct Payload {}

///
/// Optional
/// Carries an optional `Blob` value of `optional`.
///

#[record(fields(field(name = "optional", value(opt, item(prim = "Blob", max_bytes = 32)))))]
pub(super) struct Optional {}

pub(super) fn query(
    db: &crate::handles::Database,
    minimum: Option<u64>,
    category: Option<u64>,
    entry_id: Option<u64>,
) -> Result<Query<'_, BinaryObject>, String> {
    if minimum.is_some() || category.is_some() || entry_id.is_some() {
        return Err("--min, --category and --entry-id do not apply to BinaryObject".into());
    }

    Ok(db.query::<BinaryObject>())
}

pub(super) fn optional_hex(value: Option<Vec<u8>>) -> String {
    value.map_or_else(|| "—".into(), |v| hex(&v))
}

pub(super) fn filter<'a, E: Entity>(
    mut query: Query<'a, E>,
    filters: &'a PayloadFilters,
) -> Result<Query<'a, E>, String> {
    for (comparison, operand) in [
        (Comparison::Ge, &filters.minimum),
        (Comparison::Le, &filters.maximum),
        (Comparison::StartsWith, &filters.prefix),
        (Comparison::EndsWith, &filters.suffix),
        (Comparison::Contains, &filters.contains),
    ] {
        if let Some(operand) = operand.as_deref() {
            if E::SCHEMA.name != "BinaryObject" {
                return Err("payload filters apply to BinaryObject".into());
            }
            query = query.filter("payload", comparison, operand);
        }
    }

    Ok(query)
}

pub(super) fn execute<E: Entity>(query: Query<'_, E>, select: &str) -> Result<Page, Error> {
    match select {
        "payload" => query
            .fetch_as::<Payload>()
            .map(|p| display(p, &["payload"], |r| vec![hex(&r.payload)])),
        "optional" => query
            .fetch_as::<Optional>()
            .map(|p| display(p, &["optional"], |r| vec![optional_hex(r.optional)])),
        "min:payload" | "max:payload" => {
            extremum(query, select == "max:payload", "payload", |r: Payload| {
                hex(&r.payload)
            })
        }
        "min:optional" | "max:optional" => extremum(
            query,
            select == "max:optional",
            "optional",
            |r: Optional| optional_hex(r.optional),
        ),
        "distinct-count:payload" => query
            .count_distinct::<Payload>()
            .map(|r| display_scalar(r, "count")),
        "distinct-count:optional" => query
            .count_distinct::<Optional>()
            .map(|r| display_scalar(r, "count")),
        "payload,count" => query
            .group_count::<Payload>()
            .map(|r| display_groups(r, "payload", |k| hex(&k.payload))),
        "payload,count:optional" => query
            .group_count_field::<Payload>("optional")
            .map(|r| display_groups(r, "payload", |k| hex(&k.payload))),
        "optional,count" => query
            .group_count::<Optional>()
            .map(|r| display_groups(r, "optional", |k| optional_hex(k.optional))),
        "optional,count:optional" => query
            .group_count_field::<Optional>("optional")
            .map(|r| display_groups(r, "optional", |k| optional_hex(k.optional))),
        _ => Err(Error::InvalidQuery),
    }
}
