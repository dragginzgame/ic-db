use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, VectorMemory,
};

pub fn host(backing: VectorMemory) -> MemoryRuntime<VectorMemory> {
    let mut host = MemoryRuntime::new(backing).unwrap();
    let req =
        SealedDeclarationSnapshot::new(vec![MemoryRequest::new("test", "test.db.v1").unwrap()])
            .unwrap();
    let grants =
        AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap();
    host.bootstrap(&req, &grants, &GenericAllocationPolicy)
        .unwrap();

    host
}
