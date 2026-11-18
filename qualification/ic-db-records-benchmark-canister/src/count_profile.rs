//! Read-only cost decomposition for this fixed fixture, never an alternate executor.
use super::{Entry, KEY, database, runtime, transport};
use ic_db::{Comparison, Entity, Error};
use ic_storage::{StableKey, adapter::StableBTreeMap};
use std::ops::Bound;

#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "count_profile",
    guard = transport::admit::<9>,
    decode = transport::decode::<_, 9>,
    encode = transport::encode::<_, 9>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn count_profile((mode, operand): (u8, Option<u64>)) -> (Option<u64>, u64, u64) {
    let start = ic_platform_runtime::raw::instruction_counter();
    if matches!(mode, 0 | 1 | 5..=8) {
        let db = database();
        let opened = ic_platform_runtime::raw::instruction_counter();
        let mut query = db.query::<Entry>().work_limit(512);
        if matches!(mode, 7 | 8) {
            query = query.filter("id", Comparison::Ge, 1_u64).filter(
                "id",
                Comparison::Lt,
                operand.expect("exclusive primary upper bound"),
            );
        } else if let Some(category) = operand {
            if mode != 5 {
                query = query.filter("category", Comparison::Eq, category);
            }
            if mode == 1 || mode == 5 {
                query = query.using_index("category,id", &[category.into()]);
            }
        }
        let count = match mode {
            5 => query.count_index_entries(),
            6 | 7 => query.count(),
            _ => query.count_validated(),
        };
        let result = match count {
            Ok(value) => Some(value.value),
            Err(Error::Limit) => None,
            Err(error) => panic!("unexpected profile refusal: {error:?}"),
        };
        return (
            result,
            opened - start,
            ic_platform_runtime::raw::instruction_counter() - opened,
        );
    }

    // Raw supplier controls deliberately omit database integrity/admission work.
    // Fixed physical prefixes and owner bytes are specific to this Nat64 fixture.
    // The driver checks their totals independently and verifies backing preservation.
    assert!((2..=4).contains(&mode));
    let host = runtime();
    let map = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
    );
    let opened = ic_platform_runtime::raw::instruction_counter();
    let mut prefix = vec![if mode == 2 { 2 } else { 3 }];
    prefix.extend(Entry::ID.to_be_bytes());
    if mode == 2 {
        assert!(operand.is_none());
    } else {
        prefix.extend(Entry::SCHEMA.indexes[0].id.to_be_bytes());
        prefix.extend(operand.unwrap().to_be_bytes());
    }
    let mut end = prefix.clone();
    while end.last() == Some(&255) {
        end.pop();
    }
    *end.last_mut().unwrap() += 1;
    let mut count = 0;
    for entry in map.range((Bound::Included(prefix), Bound::Excluded(end))) {
        // Reading payloads is intentional: key-only iteration is a different cost.
        let bytes = entry.value();
        assert!(!bytes.is_empty());
        if mode == 4 {
            assert_eq!(bytes.len(), 8);
            let mut owner = vec![2];
            owner.extend(Entry::ID.to_be_bytes());
            owner.extend(bytes);
            assert!(!map.get(&owner).unwrap().is_empty());
        }
        count += 1;
        assert!(count <= 512, "fixture-only raw traversal bound");
    }
    (
        Some(count),
        opened - start,
        ic_platform_runtime::raw::instruction_counter() - opened,
    )
}
