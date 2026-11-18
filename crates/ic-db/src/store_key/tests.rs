use super::*;
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

#[test]
fn oversized_stored_key_traps_and_preserves_backing() {
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
    let map = StableBTreeMap::<StoreKey, Vec<u8>, _>::load(host.open_memory(&slot).unwrap());
    let before = backing.borrow().clone();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || map.get(&StoreKey::new(vec![7]).unwrap())
        ))
        .is_err()
    );
    assert_eq!(*backing.borrow(), before);
    // The read trap is terminal for this runtime; do not continue database work.
}

#[test]
fn complete_key_boundary_accepts_512_and_refuses_513() {
    let bytes = vec![7; 512];
    let key = StoreKey::new(bytes.clone()).unwrap();
    assert_eq!(&key[..], bytes);
    assert!(matches!(StoreKey::new(vec![7; 513]), Err(Error::Limit)));
}
