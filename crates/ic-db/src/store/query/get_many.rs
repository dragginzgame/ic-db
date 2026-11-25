//! Positional primary-key lookups over a live database or session.

use super::{Database, Projection, QuerySource, SelectColumns, Selection};
use crate::{Entity, EntitySchema, Error, Id, IndexValue, PrimaryKey, primary::RowKey};
use ic_storage::{DefaultMemoryImpl, Memory};
use std::{cell::RefCell, rc::Rc};

/// A bulk lookup that reads whole rows by default. Call `select` for individual columns.
#[must_use = "call fetch to execute the lookup"]
pub struct GetMany<'a, E: Entity, M: Memory = DefaultMemoryImpl> {
    db: QuerySource<'a, M>,
    keys: Result<Vec<E::Key>, Error>,
}

/// A bulk lookup with selected result columns in caller order.
#[must_use = "call fetch to execute the lookup"]
pub struct SelectedGetMany<'a, E: Entity, S, M: Memory = DefaultMemoryImpl> {
    lookup: GetMany<'a, E, M>,
    selection: S,
}

impl<M: Memory> Database<M> {
    /// Build a lookup for up to 32 raw primary keys or entity-typed IDs, returning one slot per input position.
    /// Each distinct key is looked up once. Missing keys yield `None`; repeats have independent output slots.
    /// An empty slice needs an element type, e.g. `&[] as &[u64]` for a Nat64 key.
    /// ```compile_fail
    /// use ic_db::{Database, Entity, Id};
    /// fn wrong<A: Entity, B: Entity<Key = A::Key>>(db: &Database, ids: &[Id<A>]) {
    ///     let _ = db.get_many::<B>(ids);
    /// }
    /// ```
    pub fn get_many<E: Entity>(&self, keys: &[impl Copy + Into<Id<E>>]) -> GetMany<'_, E, M> {
        GetMany::new(QuerySource::Direct(self), keys)
    }
}

impl<'a, E: Entity, M: Memory> GetMany<'a, E, M> {
    fn new(db: QuerySource<'a, M>, keys: &[impl Copy + Into<Id<E>>]) -> Self {
        let keys = if keys.len() <= u32::BITS as usize {
            Ok(keys.iter().map(|&key| key.into().key()).collect())
        } else {
            Err(Error::Limit)
        };
        Self { db, keys }
    }

    pub(crate) fn from_session(
        db: Rc<RefCell<Database<M>>>,
        keys: &[impl Copy + Into<Id<E>>],
    ) -> Self {
        Self::new(QuerySource::Session(db), keys)
    }

    /// Read the entity at each supplied key. Unknown keys yield `None`.
    /// # Errors
    /// More than 32 positions return `Limit`. Borrow, schema and row validation errors propagate.
    pub fn fetch(self) -> Result<Vec<Option<E>>, Error> {
        self.fetch_with(|_, key, bytes| super::super::decode_primary::<E>(bytes, key))
    }

    /// Select a field or tuple using the same field handles as a fluent query.
    pub fn select<S: Selection<E>>(
        self,
        fields: impl FnOnce(E::Columns) -> S,
    ) -> SelectedGetMany<'a, E, S, M>
    where
        E: SelectColumns,
    {
        SelectedGetMany {
            lookup: self,
            selection: fields(E::COLUMNS),
        }
    }

    fn fetch_with<R>(
        self,
        decode: impl Fn(&EntitySchema, E::Key, &[u8]) -> Result<R, Error>,
    ) -> Result<Vec<Option<R>>, Error> {
        let db = self.db.read()?;
        let keys = self.keys?;
        let entity = db.schema.admits(E::SCHEMA)?;
        db.get_many_typed(entity, &keys, decode)
    }
}

impl<E: SelectColumns, S: Selection<E>, M: Memory> SelectedGetMany<'_, E, S, M> {
    /// Read the selected columns at each supplied key.
    /// # Errors
    /// Repeated columns return `InvalidQuery`. Ordinary bulk lookup errors apply.
    pub fn fetch(self) -> Result<Vec<Option<S::Output>>, Error> {
        let selection = self.selection;
        let positions = selection.positions();
        let positions = positions.as_ref();
        Projection::Columns(positions).mask(E::SCHEMA)?;
        let primary = E::SCHEMA
            .field_position(E::SCHEMA.primary_key)
            .ok_or(Error::Corrupt)?;
        self.lookup.fetch_with(|entity, key, bytes| {
            let mut spans = [(0, 0); crate::schema::MAX_FIELDS];
            let mut values = [IndexValue::Absent; crate::schema::MAX_FIELDS];
            entity.inspect_fields(bytes, 1 << primary, &mut values, Some(&mut spans))?;
            if RowKey::from_index(values[primary])? != key.into_row_key() {
                return Err(Error::Corrupt);
            }
            selection.decode_spans(bytes, &spans)
        })
    }
}
