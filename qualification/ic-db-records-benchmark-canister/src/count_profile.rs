//! Read-only cost decomposition for this fixed fixture, never an alternate executor.

use super::{Entry, KEY, database, runtime, transport};
use crate::raw_map::RawKey;
use ic_db::{Comparison, Entity, Error};
use ic_storage::{StableKey, adapter::StableBTreeMap};
use std::ops::Bound;

// count_profile
// Measure database opening and the selected count separately.
// guard  : validates the request frame for method 9.
// limits : 64 KiB request, 64 KiB reply.
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
    if matches!(mode, 0 | 1 | 5..=14) {
        let db = database();
        let opened = ic_platform_runtime::raw::instruction_counter();
        let mut query = db.query::<Entry>();
        // Mode 11 keeps the application default. Threshold and refusal probes use smaller budgets.
        if mode != 11 {
            query = query.work_limit(match mode {
                12 => 7,
                13 => 8,
                14 => 31,
                _ => 512,
            });
        }
        if matches!(mode, 7 | 8 | 12..=14) {
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
        // Same amount predicate as the minimum-zero grouped-count workload.
        if matches!(mode, 9 | 10) {
            query = query.filter("amount", Comparison::Ge, 0_u64);
        }
        if matches!(mode, 12 | 13) {
            let result = match query.has_at_least(8) {
                Ok(answer) => Some(u64::from(answer.value)),
                Err(Error::Limit) => None,
                Err(error) => panic!("unexpected threshold error: {error:?}"),
            };
            return (
                result,
                opened - start,
                ic_platform_runtime::raw::instruction_counter() - opened,
            );
        }
        let count = match mode {
            5 => query.count_index_entries(),
            6 | 7 | 9 | 11 | 14 => query.count(),
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
    let map = StableBTreeMap::<RawKey, Vec<u8>, _>::load(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
    );
    let opened = ic_platform_runtime::raw::instruction_counter();
    let mut prefix = vec![if mode == 2 { 2 } else { 3 }];
    prefix.extend(Entry::SCHEMA.id.to_be_bytes());
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
    for entry in map.range((
        Bound::Included(RawKey(prefix)),
        Bound::Excluded(RawKey(end)),
    )) {
        // Reading payloads is intentional: key-only iteration is a different cost.
        let bytes = entry.value();
        assert!(!bytes.is_empty());
        if mode == 4 {
            assert_eq!(bytes.len(), 8);
            let mut owner = vec![2];
            owner.extend(Entry::SCHEMA.id.to_be_bytes());
            owner.extend(bytes);
            assert!(!map.get(&RawKey(owner)).unwrap().is_empty());
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
