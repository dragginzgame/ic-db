use super::*;
use ic_storage::{
    AllocationGrants, BackingIdentity, GenericAllocationPolicy, Memory, MemoryAuthority,
    MemoryBacking, MemoryRequest, MemoryRuntime, SealedDeclarationSnapshot, StableKey,
    VectorMemory, adapter::StableBTreeMap,
};
use std::{cell::Cell, rc::Rc};

struct Observed {
    backing: VectorMemory,
    oversized_reads: Rc<Cell<usize>>,
}
impl MemoryBacking for Observed {
    fn backing_identity(&self) -> BackingIdentity {
        self.backing.backing_identity()
    }
}
impl Memory for Observed {
    fn size(&self) -> u64 {
        self.backing.size()
    }
    fn grow(&self, pages: u64) -> i64 {
        self.backing.grow(pages)
    }
    fn read(&self, offset: u64, bytes: &mut [u8]) {
        if bytes.len() == 513 {
            self.oversized_reads.set(self.oversized_reads.get() + 1);
        }
        self.backing.read(offset, bytes);
    }
    fn write(&self, offset: u64, bytes: &[u8]) {
        self.backing.write(offset, bytes);
    }
}

#[test]
fn supplier_refuses_overlong_key_before_payload_read() {
    let backing = VectorMemory::default();
    let oversized_reads = Rc::new(Cell::new(0));
    let mut host = MemoryRuntime::new(Observed {
        backing: backing.clone(),
        oversized_reads: oversized_reads.clone(),
    })
    .unwrap();
    let slot = StableKey::parse("test.keys.v1").unwrap();
    host.bootstrap(
        &SealedDeclarationSnapshot::new(vec![MemoryRequest::new("test", "test.keys.v1").unwrap()])
            .unwrap(),
        &AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap(),
        &GenericAllocationPolicy,
    )
    .unwrap();
    let mut raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::create(host.open_memory(&slot).unwrap()).unwrap();
    raw.insert(vec![7; 513], vec![]);
    oversized_reads.set(0);
    assert_eq!(raw.get(&vec![7; 513]), Some(Vec::new()));
    assert!(
        oversized_reads.get() > 0,
        "the observer must detect a raw key payload read"
    );
    drop(raw);
    let map =
        StableBTreeMap::<StoreKey, Vec<u8>, _>::load(host.open_memory(&slot).unwrap()).unwrap();
    oversized_reads.set(0);
    let before = backing.borrow().clone();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || map.get(&StoreKey::new(vec![7]).unwrap())
        ))
        .is_err()
    );
    assert_eq!(oversized_reads.get(), 0);
    assert_eq!(*backing.borrow(), before);
    // The read trap is terminal for this runtime; do not continue database work.
}

#[test]
fn complete_codec_boundary_accepts_512_and_refuses_513() {
    let bytes = vec![7; 512];
    let key = StoreKey::from_bytes(Cow::Borrowed(&bytes)).unwrap();
    assert_eq!(key.to_bytes().unwrap().as_ref(), bytes);
    assert!(matches!(StoreKey::new(vec![7; 513]), Err(Error::Limit)));
    assert!(matches!(
        StoreKey::from_bytes(Cow::Borrowed(&[7; 513])),
        Err(CodecError::Bounds)
    ));
}
