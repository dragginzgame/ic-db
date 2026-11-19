//! Inspect the schema exported by the renamed dependency.

use accounts as directory;

fn main() {
    let schema = directory::model::Accounts::compiled();
    let names: Vec<_> = schema.entities().iter().map(|entity| entity.name).collect();
    println!("{names:?} {:02x?}", schema.fingerprint());
}
