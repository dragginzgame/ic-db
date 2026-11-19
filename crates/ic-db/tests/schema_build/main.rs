//! Exercise real host constructors and Rust name resolution, not manual registration.
#![cfg(feature = "build")]

use ic_db::{Entity, Schema};
use schema::{cycle, destination, source};

ic_db::build_schema!();

#[test]
fn constructors_collect_full_paths_and_compiler_resolved_references() {
    assert_eq!(
        source::Link::declaration_path(),
        concat!(module_path!(), "::src::schema::source::Link")
    );
    assert_eq!(
        source::Link::SCHEMA.fields[1].reference.unwrap().entity,
        destination::Target::SCHEMA.id
    );
    assert_eq!(
        cycle::Link::SCHEMA.fields[1].reference.unwrap().entity,
        cycle::Link::SCHEMA.id
    );
    let output =
        ic_db::build::generate(concat!(module_path!(), "::src::schema"), "crate::schema").unwrap();
    // Same short names in distinct modules remain separate declarations.
    for path in ["destination::Target", "source::Link", "cycle::Link"] {
        assert!(output.contains(&format!(
            "<crate::schema::{path} as ::ic_db::Entity>::SCHEMA"
        )));
    }
    let fingerprint = Schema::new(&[
        cycle::Link::SCHEMA,
        source::Link::SCHEMA,
        destination::Target::SCHEMA,
    ])
    .unwrap()
    .fingerprint();
    assert!(output.contains(&format!("schema.fingerprint(), {fingerprint:?}")));
    assert_eq!(
        ic_db::build::generate(concat!(module_path!(), "::src::schema"), "crate::schema").unwrap(),
        output
    );
}
