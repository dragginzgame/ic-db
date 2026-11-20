//! Route typed operations to the single-store session admitted by the host.

use crate::{
    CompiledCanister, CompiledStore, DatabaseSession, Entity, Error, Query, QuerySession, Store,
    UniqueKey,
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

    fn session<E: Entity<Store: MountedStore<Canister = C>>>(&self) -> &DatabaseSession {
        &self.stores[E::Store::SLOT]
    }

    /// Build a live query in the entity's declared store.
    #[must_use]
    pub fn query<'a, E: Entity<Store: MountedStore<Canister = C>>>(&self) -> Query<'a, E> {
        self.session::<E>().query()
    }

    /// Read an entity by its primary key.
    /// # Errors
    /// Propagates the owning store's lookup errors.
    pub fn get<E: Entity<Store: MountedStore<Canister = C>>>(
        &self,
        key: E::Key,
    ) -> Result<Option<E>, Error> {
        self.session::<E>().get::<E>(key)
    }

    /// Read up to 32 primary keys in input order, preserving duplicates and missing slots.
    /// # Errors
    /// Propagates the owning store session’s lookup and limit errors.
    pub fn get_many<E: Entity<Store: MountedStore<Canister = C>>>(
        &self,
        keys: &[E::Key],
    ) -> Result<Vec<Option<E>>, Error> {
        self.session::<E>().get_many::<E>(keys)
    }

    /// Read an entity using its generated unique key.
    /// # Errors
    /// Propagates the owning store's unique lookup errors.
    pub fn get_unique<K: UniqueKey<Entity: Entity<Store: MountedStore<Canister = C>>>>(
        &self,
        key: &K,
    ) -> Result<Option<K::Entity>, Error> {
        self.session::<K::Entity>().get_unique(key)
    }

    /// Insert one row in its declared store.
    /// # Errors
    /// Propagates batch preparation and commit errors.
    pub fn insert<E: Entity<Store: MountedStore<Canister = C>>>(
        &self,
        row: &E,
    ) -> Result<(), Error> {
        self.session::<E>().insert(row)
    }

    /// Replace one row in its declared store.
    /// # Errors
    /// Propagates batch preparation and commit errors.
    pub fn replace<E: Entity<Store: MountedStore<Canister = C>>>(
        &self,
        row: &E,
    ) -> Result<(), Error> {
        self.session::<E>().replace(row)
    }

    /// Delete one row from its declared store by primary key.
    /// # Errors
    /// Propagates batch preparation and commit errors.
    pub fn delete<E: Entity<Store: MountedStore<Canister = C>>>(
        &self,
        key: E::Key,
    ) -> Result<(), Error> {
        self.session::<E>().delete::<E>(key)
    }
}

impl<C: CompiledCanister, E: Entity<Store: MountedStore<Canister = C>>> QuerySession<E>
    for CanisterSession<C>
{
    fn entity_query<'a>(&self) -> Query<'a, E> {
        self.query()
    }
}
