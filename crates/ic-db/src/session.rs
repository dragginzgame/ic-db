//! Synchronous access to a database attached by the host.
use crate::{Batch, Database, Entity, Error, IndexValue, Query};
use std::{
    cell::{Ref, RefCell},
    rc::Rc,
};

/// Named lookup components generated for a declared unique index.
///
/// All declared components are required.
/// ```compile_fail,E0063
/// # use ic_db::{entity, DatabaseSession};
/// # #[entity(pk(field="id"), index(fields=["issuer","subject"], unique), fields(
/// #   field(name="id", value(item(prim="Nat64"))), field(name="issuer", value(item(prim="Nat64"))),
/// #   field(name="subject", value(item(prim="Nat64")))))]
/// # struct User {}
/// let _ = UserByIssuerAndSubject { issuer: 1 };
/// ```
///
/// Component types are exact.
/// ```compile_fail,E0308
/// # use ic_db::{entity, DatabaseSession};
/// # #[entity(pk(field="id"), index(fields=["issuer","subject"], unique), fields(
/// #   field(name="id", value(item(prim="Nat64"))), field(name="issuer", value(item(prim="Nat64"))),
/// #   field(name="subject", value(item(prim="Nat64")))))]
/// # struct User {}
/// let _ = UserByIssuerAndSubject { issuer: true, subject: 1 };
/// ```
///
/// The key determines the result entity.
/// ```compile_fail,E0308
/// # use ic_db::{entity, DatabaseSession};
/// # #[entity(pk(field="id"), index(fields=["issuer","subject"], unique), fields(
/// #   field(name="id", value(item(prim="Nat64"))), field(name="issuer", value(item(prim="Nat64"))),
/// #   field(name="subject", value(item(prim="Nat64")))))]
/// # struct User {}
/// fn wrong(db: &DatabaseSession) -> Option<u64> {
///     db.get_unique(&UserByIssuerAndSubject { issuer: 1, subject: 2 }).unwrap()
/// }
/// ```
///
/// Only unique indexes generate keys.
/// ```compile_fail,E0422
/// # use ic_db::entity;
/// # #[entity(pk(field="id"), index(field="owner"), fields(
/// #   field(name="id", value(item(prim="Nat64"))), field(name="owner", value(item(prim="Nat64")))))]
/// # struct User {}
/// let _ = UserByOwner { owner: 1 };
/// ```
pub trait UniqueKey {
    /// Entity owning the index.
    type Entity: Entity;
    /// Logical name of the declared unique index.
    #[doc(hidden)]
    const INDEX: &'static str;
    /// Present components in declared index order.
    #[doc(hidden)]
    fn components(&self) -> impl AsRef<[IndexValue<'_>]>;
}

/// A cloneable handle to one attached database and its existing storage lease.
/// The host attaches once using [`Self::new`] and hands each caller a clone.
/// Cloning takes no database borrow and performs no storage access.
///
/// Use a handle within one synchronous message execution. Drop explicit read
/// guards before writing. Reacquire a handle from the host after an `await`. A handle supplies neither a snapshot nor a transaction
/// spanning calls. Each batch uses the ordinary enclosing-message commit rules.
///
/// Each `insert`, `replace` or `delete` commits a separate batch in the
/// enclosing message. Submit dependent changes together:
/// ```
/// # use ic_db::{Batch, DatabaseSession, Entity, Error};
/// # fn change_together<E: Entity>(session: &DatabaseSession, first: &E, second: &E) -> Result<(), Error> {
/// let mut batch = Batch::new();
/// batch.replace(first)?;
/// batch.replace(second)?;
/// session.commit(batch)?;
/// # Ok(()) }
/// ```
///
/// Engine work and output limits apply to each query. Cloning creates no budget.
/// Hosts enforcing a cumulative request allowance must share the same allowance
/// across helpers in that request and establish a fresh one at the next boundary.
#[derive(Clone)]
pub struct DatabaseSession(Rc<RefCell<Database>>);

impl DatabaseSession {
    /// Wrap an already-created or reopened database. Its allocation remains
    /// leased until the last handle is dropped.
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self(Rc::new(RefCell::new(database)))
    }

    /// Recover the attached database for a lifecycle operation requiring ownership.
    /// # Errors
    /// Returns this same handle if another handle still owns the attachment.
    pub fn try_into_database(self) -> Result<Database, Self> {
        Rc::try_unwrap(self.0)
            .map(RefCell::into_inner)
            .map_err(Self)
    }

    /// Borrow the database for fluent queries or a sequence of synchronous reads.
    /// A builder from `read.query::<E>()` borrows this guard. Commits can resume
    /// after the guard is dropped, even if several session handles still exist.
    ///
    /// # Errors
    /// Returns [`Error::BorrowConflict`] if a commit has exclusive access.
    pub fn read(&self) -> Result<Ref<'_, Database>, Error> {
        self.0.try_borrow().map_err(|_| Error::BorrowConflict)
    }

    /// Build a live query over this attachment. Execution sees writes committed
    /// before the terminal call. An exclusive borrow at execution returns
    /// [`Error::BorrowConflict`]. The builder owns a session handle and borrows
    /// only its predicate and cursor inputs.
    #[must_use]
    pub fn query<'a, E: Entity>(&self) -> Query<'a, E> {
        Query::from_session(self.0.clone())
    }

    /// Read an owned entity by primary key, releasing the borrow before returning.
    ///
    /// # Errors
    /// Returns a borrow conflict or the original [`Database::get`] error.
    pub fn get<E: Entity>(&self, key: E::Key) -> Result<Option<E>, Error> {
        self.read()?.get::<E>(key)
    }

    /// Look up an owned row using a generated unique key, such as
    /// `UserByIssuerAndSubject { issuer, subject }`.
    /// # Errors
    /// Propagates session borrow, schema, key admission and row integrity errors.
    pub fn get_unique<K: UniqueKey>(&self, key: &K) -> Result<Option<K::Entity>, Error> {
        let components = key.components();
        let values = components.as_ref();
        if !K::Entity::SCHEMA
            .indexes
            .iter()
            .any(|index| index.name == K::INDEX && index.is_unique_lookup(values.len()))
        {
            return Err(Error::InvalidQuery);
        }
        self.read()?
            .query::<K::Entity>()
            .using_index(K::INDEX, values)
            .work_limit(1)
            .try_one()
    }

    /// Insert one entity as one batch.
    /// # Errors
    /// Propagates batch preparation and commit errors.
    pub fn insert<E: Entity>(&self, row: &E) -> Result<(), Error> {
        let mut batch = Batch::new();
        batch.insert(row)?;
        self.commit(batch)
    }

    /// Replace one existing entity as one batch.
    /// # Errors
    /// Propagates batch preparation and commit errors.
    pub fn replace<E: Entity>(&self, row: &E) -> Result<(), Error> {
        let mut batch = Batch::new();
        batch.replace(row)?;
        self.commit(batch)
    }

    /// Delete one entity by primary key as one batch.
    /// # Errors
    /// Propagates batch preparation and commit errors.
    pub fn delete<E: Entity>(&self, key: E::Key) -> Result<(), Error> {
        let mut batch = Batch::new();
        batch.delete::<E>(key)?;
        self.commit(batch)
    }

    /// Commit one batch through the attached database.
    ///
    /// # Errors
    /// Returns [`Error::BorrowConflict`] before mutation if a borrow is active,
    /// or the original [`Database::commit`] error.
    pub fn commit(&self, batch: Batch) -> Result<(), Error> {
        self.0
            .try_borrow_mut()
            .map_err(|_| Error::BorrowConflict)?
            .commit(batch)
    }
}
