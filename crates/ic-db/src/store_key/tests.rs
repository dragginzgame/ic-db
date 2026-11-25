use super::*;
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

#[test]
fn incompatible_stored_key_bound_refuses_at_open_and_preserves_backing() {
    let backing = VectorMemory::default();
    let mut host = MemoryRuntime::new(backing.clone()).unwrap();
    let slot = StableKey::parse("test.keys.v1").unwrap();
    host.bootstrap(
        &SealedDeclarationSnapshot::new(vec![MemoryRequest::new("test", "test.keys.v1").unwrap()])
            .unwrap(),
        &AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap(),
        &GenericAllocationPolicy,
    )
    .unwrap();
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::create(host.open_memory(&slot).unwrap());
    raw.insert(vec![7; 513], vec![]);
    assert_eq!(raw.get(&vec![7; 513]), Some(Vec::new()));
    drop(raw);
    let before = backing.borrow().clone();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            StableBTreeMap::<StoreKey, Vec<u8>, _>::load(host.open_memory(&slot).unwrap())
        }))
        .is_err()
    );
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn encoded_comparisons_match_owned_key_order() {
    let keys = [
        vec![],
        vec![0],
        vec![0, 255],
        vec![1],
        vec![255; MAX_INDEX_KEY_BYTES],
    ]
    .map(|bytes| StoreKey::new(bytes).unwrap());
    let compare = <StoreKey as ic_storage::adapter::Storable>::CMP_ENCODED.unwrap();
    for probe in &keys {
        for stored in &keys {
            assert_eq!(compare(probe, stored), probe.cmp(stored));
        }
    }
}

#[test]
fn complete_key_boundary_accepts_512_and_refuses_513() {
    let bytes = vec![7; 512];
    let key = StoreKey::new(bytes.clone()).unwrap();
    assert_eq!(&key[..], bytes);
    assert!(matches!(StoreKey::new(vec![7; 513]), Err(Error::Limit)));
}
