//! Exercise real host constructors and Rust name resolution, not manual registration.
mod schema;

use ic_db::{Entity, Schema};
use schema::{cycle, destination, source};

ic_db::include_schema!();

#[test]
fn constructors_collect_full_paths_and_compiler_resolved_references() {
    assert_eq!(
        source::Link::declaration_path(),
        concat!(module_path!(), "::schema::source::Link")
    );
    assert_eq!(
        source::Link::SCHEMA.fields[1].reference.unwrap().entity,
        destination::Target::SCHEMA.id
    );
    assert_eq!(
        cycle::Link::SCHEMA.fields[1].reference.unwrap().entity,
        cycle::Link::SCHEMA.id
    );
    let fingerprint = Schema::new(&[
        cycle::Link::SCHEMA,
        source::Link::SCHEMA,
        destination::Target::SCHEMA,
    ])
    .unwrap()
    .fingerprint();
    assert_eq!(compiled().fingerprint(), fingerprint);
}
