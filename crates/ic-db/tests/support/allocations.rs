use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryBacking, MemoryRequest,
    MemoryRuntime, SealedDeclarationSnapshot,
};

pub fn host_with<M: MemoryBacking>(backing: M, names: &[&str]) -> MemoryRuntime<M> {
    let mut host = MemoryRuntime::new(backing).unwrap();
    let requests = names
        .iter()
        .map(|&name| MemoryRequest::new("test", name).unwrap())
        .collect();
    let req = SealedDeclarationSnapshot::new(requests).unwrap();
    let grants =
        AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap();
    host.bootstrap(&req, &grants, &GenericAllocationPolicy)
        .unwrap();

    host
}
