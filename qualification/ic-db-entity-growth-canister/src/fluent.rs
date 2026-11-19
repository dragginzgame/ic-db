//! Reachable fluent call sites; mapping runs only after database execution returns.
use super::{Database, Entity, Observation, Row};
use ic_db::Comparison as C;

pub(super) fn operate<E: Entity<Key = u64>>(
    db: &Database,
    mode: u8,
    input: Row,
    project: impl Fn(&E) -> Row,
) -> Observation {
    let query = db.query::<E>().limit(2);
    // Runtime input prevents a constant empty result. Every branch keeps the
    // same entity result/transport type and already reachable row executor.
    // Use distinct terminal call sites; do not force executor sharing by first
    // returning one Query from the match and fetching only once in the fixture.
    let page = match mode {
        10 => query
            .filter("amount", C::Ge, input.2)
            .order_by(&["id"])
            .fetch(),
        #[cfg(feature = "fluent-shapes")]
        11 => query
            .filter("amount", C::Le, input.2)
            .order_by(&["id"])
            .fetch(),
        #[cfg(feature = "fluent-shapes")]
        12 => query
            .filter_any(&[
                ("amount", C::Eq, input.2.into()),
                ("id", C::Eq, input.0.into()),
            ])
            .fetch(),
        #[cfg(feature = "fluent-shapes")]
        13 => query
            .filter("amount", C::Ge, input.2)
            .order_by(&["id"])
            .descending()
            .fetch(),
        #[cfg(feature = "fluent-shapes")]
        14 => query.order_by(&["amount"]).descending().fetch(),
        #[cfg(feature = "fluent-shapes")]
        15 => query
            .filter("category", C::Eq, input.1)
            .order_by(&["id"])
            .fetch(),
        #[cfg(feature = "fluent-shapes")]
        16 => query
            .filter("id", C::Ge, input.0)
            .filter("id", C::Le, input.2)
            .fetch(),
        #[cfg(feature = "fluent-shapes")]
        17 => query
            .filter_not_in("category", &[input.1, input.2])
            .order_by(&["id"])
            .fetch(),
        #[cfg(feature = "fluent-maps")]
        20..=27 => query
            .filter("amount", C::Ge, input.2)
            .order_by(&["id"])
            .fetch(),
        _ => panic!("unsupported fluent fixture shape"),
    }
    .unwrap();
    // Keep continuation available until after mapping: these samples intentionally
    // consume only the first page. A mapping closure never enters the executor.
    let rows = page.rows.iter().map(project);
    #[cfg(not(feature = "fluent-maps"))]
    let rows = rows.collect();
    #[cfg(feature = "fluent-maps")]
    let rows = match mode {
        20 => rows
            .map(|(id, category, amount)| (id.wrapping_add(input.0), category, amount))
            .collect(),
        21 => rows
            .map(|(id, category, amount)| (id, category.wrapping_add(input.1), amount))
            .collect(),
        22 => rows
            .map(|(id, category, amount)| (id, category, amount.wrapping_add(input.2)))
            .collect(),
        23 => rows
            .map(|(id, category, amount)| (id ^ input.0, category, amount))
            .collect(),
        24 => rows
            .map(|(id, category, amount)| (id, category ^ input.1, amount))
            .collect(),
        25 => rows
            .map(|(id, category, amount)| (id, category, amount ^ input.2))
            .collect(),
        26 => rows
            .map(|(id, category, amount)| (amount, category, id))
            .collect(),
        27 => rows
            .map(|(id, category, amount)| (id, amount, category))
            .collect(),
        _ => rows.collect(),
    };
    (
        None,
        rows,
        vec![],
        db.revision::<E>().unwrap(),
        db.schema().entities().len() as u8,
    )
}
