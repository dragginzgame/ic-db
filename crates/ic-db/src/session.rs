//! Synchronous access to a database attached by the host.
use crate::{Batch, Database, Entity, Error};
use std::{
    cell::{Ref, RefCell},
    rc::Rc,
};

/// A cloneable handle to one attached database and its existing storage lease.
/// The host attaches once using [`Self::new`] and hands each caller a clone.
/// Cloning takes no database borrow and performs no storage access.
///
/// Use a handle within one synchronous message execution. Drop read guards and
/// query builders before a write or an `await`, and reacquire a handle from the
/// host in a callback. A handle supplies neither a snapshot nor a transaction
/// spanning calls. Each batch uses the ordinary enclosing-message commit rules.
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

    /// Read an owned entity by primary key, releasing the borrow before returning.
    ///
    /// # Errors
    /// Returns a borrow conflict or the original [`Database::get`] error.
    pub fn get<E: Entity>(&self, key: E::Key) -> Result<Option<E>, Error> {
        self.read()?.get::<E>(key)
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
