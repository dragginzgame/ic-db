//! Compare every reopened key and value with an independent in-memory map.
use ic_stable_structures::{BTreeMap as StableMap, VectorMemory};
use std::collections::{BTreeMap, BTreeSet};

fn verify(seed: usize, batch_size: usize) {
    let memory = VectorMemory::default();
    let mut tree = StableMap::new(memory.clone());
    let mut expected = BTreeMap::new();
    for n in 0..1024 {
        let k = 2 * ((n * (2 * seed + 1) + seed) % 1024) as u64;
        let bytes = vec![k as u8; if k % 13 == 0 { 5000 } else { 15 }];
        tree.insert(k, bytes.clone());
        expected.insert(k, bytes);
    }
    assert_eq!(tree.remove_many(BTreeSet::new()), 0);
    let order: Vec<_> = (0..1024)
        .map(|n| 2 * ((n * (2 * seed + 3) + 17) % 1024) as u64)
        .collect();
    for batch in order.chunks(batch_size) {
        // Mix existing and missing keys in physical order. Retrying the
        // same set, including after the final root disappears, removes zero.
        let keys: BTreeSet<_> = batch.iter().flat_map(|&k| [k, k + 1]).collect();
        assert_eq!(tree.remove_many(keys.clone()), batch.len() as u64);
        assert_eq!(tree.remove_many(keys), 0);
        for key in batch {
            expected.remove(key).unwrap();
        }
        drop(tree);
        tree = StableMap::load(memory.clone());
        assert_eq!(tree.len(), expected.len() as u64);
        assert!(
            tree.iter()
                .map(|entry| (*entry.key(), entry.value()))
                .eq(expected.iter().map(|(&k, v)| (k, v.clone())))
        );
    }
    tree.insert(42, vec![1, 2, 3]);
    assert_eq!(tree.get(&42), Some(vec![1, 2, 3]));
}

fn main() {
    for seed in [0, 1, 73, 301] {
        for size in [1, 32, 256, 1024] {
            verify(seed, size);
        }
    }
    println!("16 bulk-delete and reopen comparisons passed");
}
