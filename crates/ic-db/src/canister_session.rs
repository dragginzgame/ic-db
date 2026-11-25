//! Route typed operations to the single-store session admitted by the host.

use crate::{
    Batch, CompiledCanister, CompiledStore, DatabaseSession, Entity, Error, InsertInput, Query,
    QuerySession, Store, UniqueKey,
};
use std::{marker::PhantomData, rc::Rc};

/// Implemented by schema assembly for stores in a compiled canister roster.
#[doc(hidden)]
pub trait MountedStore: Store {
    /// Executable-local position in the owner's roster.
    const SLOT: usize;
}

/// All store sessions for one declared canister. Clones share the same leases.
pub struct CanisterSession<C> {
    stores: Rc<[DatabaseSession]>,
    owner: PhantomData<fn() -> C>,
}

impl<C> Clone for CanisterSession<C> {
    fn clone(&self) -> Self {
        Self {
            stores: self.stores.clone(),
            owner: PhantomData,
        }
    }
}

impl<C: CompiledCanister> CanisterSession<C> {
    /// Attach every store once, in compiled roster order. The host must acquire
    /// the lease named by each descriptor's key and create or reopen it with
    /// that descriptor's schema. A failed attachment drops collected handles.
    /// A host creating stores must trap its lifecycle message on failure to
    /// roll back any preceding creates.
    ///
    /// # Errors
    /// Returns the first host error without exposing a partially attached session.
    pub fn try_attach<F>(
        attach: impl FnMut(&CompiledStore) -> Result<DatabaseSession, F>,
    ) -> Result<Self, F> {
        let stores = C::STORES
            .iter()
            .map(attach)
            .collect::<Result<Rc<[_]>, _>>()?;
        Ok(Self {
            stores,
            owner: PhantomData,
        })
    }

    fn session<S: MountedStore<Canister = C>>(&self) -> &DatabaseSession {
        &self.stores[S::SLOT]
    }

    /// Prepare dependent changes in one store, inferred from the entities added.
    #[must_use]
    pub fn batch<S: MountedStore<Canister = C>>(&self) -> StoreBatch<'_, S> {
        StoreBatch {
            session: self.session::<S>(),
            batch: Batch::new(),
            store: PhantomData,
        }
    }

    /// Build a live query in the entity's declared store.
    #[must_use]
    pub fn query<'a, E: Entity<Store: MountedStore<Canister = C>>>(&self) -> Query<'a, E> {
        self.session::<E::Store>().query()
    }

    /// Read an entity by its primary key.
    /// # Errors
    /// Propagates the owning store's lookup errors.
    pub fn get<E: Entity<Store: MountedStore<Canister = C>>>(
        &self,
        key: impl Into<crate::Id<E>>,
    ) -> Result<Option<E>, Error> {
        self.session::<E::Store>().get::<E>(key)
    }

    /// Build a live bulk lookup in the entity's store. The read borrow starts at `fetch`.
    pub fn get_many<E: Entity<Store: MountedStore<Canister = C>>>(
        &self,
        keys: &[impl Copy + Into<crate::Id<E>>],
    ) -> crate::GetMany<'static, E> {
        self.session::<E::Store>().get_many::<E>(keys)
    }

    /// Read an entity using its generated unique key.
    /// # Errors
    /// Propagates the owning store's unique lookup errors.
    pub fn get_unique<K: UniqueKey<Entity: Entity<Store: MountedStore<Canister = C>>>>(
        &self,
        key: &K,
    ) -> Result<Option<K::Entity>, Error> {
        self.session::<<K::Entity as Entity>::Store>()
            .get_unique(key)
    }

    /// Insert one row in its declared store.
    /// # Errors
    /// Propagates batch preparation and commit errors.
    pub fn insert<E: Entity<Store: MountedStore<Canister = C>>>(
        &self,
        row: &E,
    ) -> Result<(), Error> {
        self.session::<E::Store>().insert(row)
    }

    /// Insert one generated input in its declared store and return the committed row.
    /// Managed fields receive the current operation time.
    /// # Errors
    /// Propagates [`DatabaseSession::insert_input`] preparation and commit errors.
    pub fn insert_input<I: InsertInput<Entity: Entity<Store: MountedStore<Canister = C>>>>(
        &self,
        input: I,
    ) -> Result<I::Entity, Error> {
        self.session::<<I::Entity as Entity>::Store>()
            .insert_input(input)
    }

    /// Insert in the selected unique key's store, preserving its existing owner on replay.
    /// # Errors
    /// Propagates the owning session's [conditional insert](DatabaseSession::insert_if_absent) errors.
    pub fn insert_if_absent<K: UniqueKey<Entity: Entity<Store: MountedStore<Canister = C>>>>(
        &self,
        input: impl InsertInput<Entity = K::Entity>,
    ) -> Result<crate::InsertOutcome<K::Entity>, Error> {
        self.session::<<K::Entity as Entity>::Store>()
            .insert_if_absent::<K>(input)
    }

    /// Replace one row in its declared store and return the stored value.
    /// # Errors
    /// Propagates batch preparation and commit errors.
    pub fn replace<E: Entity<Store: MountedStore<Canister = C>>>(
        &self,
        row: &E,
    ) -> Result<E, Error> {
        self.session::<E::Store>().replace(row)
    }

    /// Insert or replace by primary key in the declared store and return the stored row.
    /// # Errors
    /// Propagates [`DatabaseSession::upsert`] errors.
    pub fn upsert<E: Entity<Store: MountedStore<Canister = C>>>(
        &self,
        row: &E,
    ) -> Result<E, Error> {
        self.session::<E::Store>().upsert(row)
    }

    /// Delete one row from its declared store by primary key.
    /// # Errors
    /// Propagates batch preparation and commit errors.
    pub fn delete<E: Entity<Store: MountedStore<Canister = C>>>(
        &self,
        key: impl Into<crate::Id<E>>,
    ) -> Result<(), Error> {
        self.session::<E::Store>().delete::<E>(key)
    }
}

impl<C: CompiledCanister, E: Entity<Store: MountedStore<Canister = C>>> QuerySession<E>
    for CanisterSession<C>
{
    fn entity_query<'a>(&self) -> Query<'a, E> {
        self.query()
    }
}

/// A synchronous batch bound to one mounted store. Dropping it discards its prepared changes.
pub struct StoreBatch<'a, S: Store> {
    session: &'a DatabaseSession,
    batch: Batch,
    store: PhantomData<fn() -> S>,
}

impl<S: Store> StoreBatch<'_, S> {
    /// Queue an insertion in this store.
    /// # Errors
    /// Propagates [`Batch::insert`] admission errors.
    pub fn insert<E: Entity<Store = S>>(&mut self, row: &E) -> Result<(), Error> {
        self.batch.insert(row)
    }

    /// Queue an insertion using the entity's generated input and return the prepared row.
    /// # Errors
    /// Propagates [`Batch::insert_input`] admission errors.
    pub fn insert_input<I: InsertInput<Entity: Entity<Store = S>>>(
        &mut self,
        input: I,
    ) -> Result<I::Entity, Error> {
        self.batch.insert_input(input)
    }

    /// Queue a replacement in this store.
    /// # Errors
    /// Propagates [`Batch::replace`] admission errors.
    pub fn replace<E: Entity<Store = S>>(&mut self, row: &E) -> Result<(), Error> {
        self.batch.replace(row)
    }

    /// Queue an insertion or replacement by primary key in this store.
    /// # Errors
    /// Propagates [`Batch::upsert`] admission errors.
    pub fn upsert<E: Entity<Store = S>>(&mut self, row: &E) -> Result<(), Error> {
        self.batch.upsert(row)
    }

    /// Queue a deletion in this store.
    /// # Errors
    /// Propagates [`Batch::delete`] admission errors.
    pub fn delete<E: Entity<Store = S>>(
        &mut self,
        key: impl Into<crate::Id<E>>,
    ) -> Result<(), Error> {
        self.batch.delete::<E>(key)
    }

    /// Apply the prepared changes through the owning store session.
    /// # Errors
    /// Propagates [`DatabaseSession::commit`] borrow and commit errors.
    pub fn commit(self) -> Result<(), Error> {
        self.session.commit(self.batch)
    }

    /// Commit all queued changes and return one inserted or replaced row from this store.
    /// # Errors
    /// Propagates [`DatabaseSession::commit_returning`] borrow, selection and commit errors.
    pub fn commit_returning<E: Entity<Store = S>>(
        self,
        key: impl Into<crate::Id<E>>,
    ) -> Result<E, Error> {
        self.session.commit_returning(self.batch, key)
    }
}
