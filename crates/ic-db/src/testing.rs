//! Native database fixtures for application tests. Enable the `test-support`
//! feature on an `ic-db` dev-dependency.

use crate::{Database, Error, Schema};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    RuntimeMemory, SealedDeclarationSnapshot, StableKey, VectorMemory,
};

/// Retains a named allocation across independent database handles. Each create
/// or open acquires a fresh lease. Drop the previous database before reopening.
///
/// ```
/// use ic_db::{Batch, Entity, Schema, entity, testing::Fixture};
///
/// #[entity(pk(field="id"), fields(field(name="id", value(item(prim="Nat64")))))]
/// struct Profile {}
///
/// let schema = Schema::new(&[Profile::SCHEMA])?;
/// let fixture = Fixture::new("example.profiles.v1");
/// let mut db = fixture.create(schema.clone(), [7; 16])?;
/// let mut batch = Batch::new();
/// batch.insert(&Profile { id: 42 })?;
/// db.commit(batch)?;
/// drop(db);
/// assert_eq!(fixture.open(schema)?.get::<Profile>(42)?.unwrap().id, 42);
/// # Ok::<(), ic_db::Error>(())
/// ```
pub struct Fixture {
    allocation: Box<dyn Fn() -> RuntimeMemory<VectorMemory>>,
}

impl Fixture {
    /// Allocate a new vector-backed store and leave its database uninitialized.
    /// # Panics
    /// Panics if the supplied name is an invalid IC Storage stable key.
    #[must_use]
    pub fn new(key: &str) -> Self {
        let key = StableKey::parse(key).expect("invalid fixture allocation key");
        let (namespace, _) = key.as_str().rsplit_once('.').expect("versioned stable key");
        let grants = AllocationGrants::new(vec![
            MemoryAuthority::new("fixture", format!("{namespace}.")).unwrap(),
        ])
        .unwrap();
        let declarations = SealedDeclarationSnapshot::new(vec![
            MemoryRequest::new("fixture", key.as_str()).unwrap(),
        ])
        .unwrap();
        let backing = VectorMemory::default();
        Self::with_allocation(move || {
            let mut runtime = MemoryRuntime::new(backing.clone())
                .expect("drop the previous database before reopening its backing");
            runtime
                .bootstrap(&declarations, &grants, &GenericAllocationPolicy)
                .expect("fixture storage bootstrap");
            runtime.open_memory(&key).expect("fixture allocation")
        })
    }

    /// Use the application's own named allocation. The function must return a
    /// fresh exclusive lease on the same backing and key each time. It is called
    /// once at construction to establish the allocation, then once per operation.
    /// # Panics
    /// Propagates panics from the supplied allocation function.
    #[must_use]
    pub fn with_allocation(allocation: impl Fn() -> RuntimeMemory<VectorMemory> + 'static) -> Self {
        drop(allocation());
        Self {
            allocation: Box::new(allocation),
        }
    }

    /// Create the database using the supplied install incarnation.
    /// # Errors
    /// Returns the original [`Database::create`] refusal.
    /// # Panics
    /// Propagates allocation and supplier panics, including overlapping leases.
    pub fn create(
        &self,
        schema: Schema,
        incarnation: [u8; 16],
    ) -> Result<Database<VectorMemory>, Error> {
        Database::create((self.allocation)(), schema, incarnation)
    }

    /// Reopen the retained allocation under the supplied compiled schema.
    /// # Errors
    /// Returns the original [`Database::open`] refusal.
    /// # Panics
    /// Propagates allocation and supplier panics, including overlapping leases.
    pub fn open(&self, schema: Schema) -> Result<Database<VectorMemory>, Error> {
        Database::open((self.allocation)(), schema)
    }
}
