//! Disposable native supplier comparison. Count backing operations, without timing native code.

use ic_storage::{
    AllocationGrants, BackingIdentity, GenericAllocationPolicy, Memory, MemoryAuthority,
    MemoryBacking, MemoryRequest, MemoryRuntime, SealedDeclarationSnapshot, StableKey,
    VectorMemory, adapter::StableBTreeMap,
};
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
struct Backing {
    memory: VectorMemory,
    reads: Cell<u64>,
    read_bytes: Cell<u64>,
    writes: Cell<u64>,
    write_bytes: Cell<u64>,
}

impl MemoryBacking for Backing {
    fn backing_identity(&self) -> BackingIdentity {
        self.memory.backing_identity()
    }
}

impl Memory for Backing {
    fn size(&self) -> u64 {
        self.memory.size()
    }

    fn grow(&self, pages: u64) -> i64 {
        self.memory.grow(pages)
    }

    fn read(&self, offset: u64, bytes: &mut [u8]) {
        self.reads.set(self.reads.get() + 1);
        self.read_bytes
            .set(self.read_bytes.get() + bytes.len() as u64);
        self.memory.read(offset, bytes);
    }

    fn write(&self, offset: u64, bytes: &[u8]) {
        self.writes.set(self.writes.get() + 1);
        self.write_bytes
            .set(self.write_bytes.get() + bytes.len() as u64);
        self.memory.write(offset, bytes);
    }
}

impl Backing {
    fn take(&self) -> (u64, u64, u64, u64) {
        (
            self.reads.take(),
            self.read_bytes.take(),
            self.writes.take(),
            self.write_bytes.take(),
        )
    }
}

fn run(payload: usize, discard: bool) {
    let backing = Rc::new(Backing::default());
    let mut host = MemoryRuntime::new(Rc::clone(&backing)).unwrap();
    let key = "probe.jobs.v1";
    let requests =
        SealedDeclarationSnapshot::new(vec![MemoryRequest::new("probe", key).unwrap()]).unwrap();
    let grants =
        AllocationGrants::new(vec![MemoryAuthority::new("probe", "probe.").unwrap()]).unwrap();
    host.bootstrap(&requests, &grants, &GenericAllocationPolicy)
        .unwrap();
    let allocation = host.open_memory(&StableKey::parse(key).unwrap()).unwrap();
    let map = StableBTreeMap::create(allocation);
    #[cfg(feature = "historical")]
    let map = map.unwrap();
    let mut map = map;
    let header = vec![42; 36];
    map.insert(0_u8, header.clone());
    for slot in 1_u8..=16 {
        map.insert(slot, vec![slot; payload]);
    }
    backing.take();
    for slot in 1_u8..=16 {
        assert_eq!(map.get(&slot).unwrap(), vec![slot; payload]);
    }
    let inspect = backing.take();
    assert_eq!(inspect.2, 0);
    for slot in 1_u8..=16 {
        #[cfg(feature = "historical")]
        if discard {
            assert!(map.remove_discard(&slot));
            continue;
        }
        assert_eq!(map.remove(&slot).unwrap(), vec![slot; payload]);
    }
    let remove = backing.take();
    assert_eq!(map.len(), 1);
    assert_eq!(map.get(&0), Some(header));
    println!("payload={payload} discard={discard} inspect={inspect:?} remove={remove:?}");
}

fn main() {
    for payload in [128, 4096, 16000] {
        run(payload, false);
        #[cfg(feature = "historical")]
        run(payload, true);
    }
}
