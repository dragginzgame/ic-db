//! Typed column selection at the boundary of the shared projection executor.

use super::{Decoder, Entity, Error, Memory, Query, QueryPage, fetch_rows};
use std::marker::PhantomData;

/// A column of entity `E` whose stored value decodes to `T`.
pub struct Column<E, T> {
    position: u8,
    decode: fn(&mut Decoder<'_>) -> Result<T, Error>,
    entity: PhantomData<fn(E)>,
}

impl<E, T> Copy for Column<E, T> {}
impl<E, T> Clone for Column<E, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E, T> Column<E, T> {
    #[doc(hidden)]
    pub const fn new(position: u8, decode: fn(&mut Decoder<'_>) -> Result<T, Error>) -> Self {
        Self {
            position,
            decode,
            entity: PhantomData,
        }
    }
}

/// Field handles supplied to a selection by an authored entity.
pub trait SelectColumns: Entity {
    /// The generated fields, with their exact stored Rust types.
    type Columns;
    #[doc(hidden)]
    const COLUMNS: Self::Columns;
}

mod sealed {
    pub trait Sealed {}
}

/// A single selected column or a tuple of two to four columns from one entity.
pub trait Selection<E>: sealed::Sealed {
    /// The value returned for each matching row.
    type Output;
    #[doc(hidden)]
    fn positions(&self) -> impl AsRef<[u8]>;
    #[doc(hidden)]
    fn decode(&self, reader: &mut Decoder<'_>, order: &[usize]) -> Result<Self::Output, Error>;
}

impl<E, T> sealed::Sealed for Column<E, T> {}
impl<E, T> Selection<E> for Column<E, T> {
    type Output = T;
    fn positions(&self) -> impl AsRef<[u8]> {
        [self.position]
    }
    fn decode(&self, reader: &mut Decoder<'_>, _: &[usize]) -> Result<T, Error> {
        (self.decode)(reader)
    }
}

// Projected bytes are in durable field order. Decode each field once in that
// order, then assemble the tuple in the caller's order.
macro_rules! tuple_selection {
    ($( $index:tt $ty:ident $slot:ident ),+) => {
        impl<E, $($ty),+> sealed::Sealed for ($(Column<E, $ty>,)+) {}
        impl<E, $($ty),+> Selection<E> for ($(Column<E, $ty>,)+) {
            type Output = ($($ty,)+);
            fn positions(&self) -> impl AsRef<[u8]> { [$(self.$index.position),+] }
            fn decode(&self, reader: &mut Decoder<'_>, order: &[usize]) -> Result<Self::Output, Error> {
                $(let mut $slot = None;)+
                for &index in order {
                    match index {
                        $($index => $slot = Some((self.$index.decode)(reader)?),)+
                        _ => return Err(Error::Corrupt),
                    }
                }
                Ok(($($slot.ok_or(Error::Corrupt)?,)+))
            }
        }
    };
}
tuple_selection!(0 A a, 1 B b);
tuple_selection!(0 A a, 1 B b, 2 C c);
tuple_selection!(0 A a, 1 B b, 2 C c, 3 D d);

/// A bounded query with its result columns selected.
pub struct SelectedQuery<'a, E, M: Memory, S> {
    query: Query<'a, E, M>,
    selection: S,
}

impl<'a, E: SelectColumns, M: Memory> Query<'a, E, M> {
    /// Select a field or tuple before fetching, e.g. `.select(|e| (e.id, e.name))`.
    /// The selector runs once and receives field handles. Apply filters, order and cursor first.
    #[must_use]
    pub fn select<S: Selection<E>>(
        self,
        fields: impl FnOnce(E::Columns) -> S,
    ) -> SelectedQuery<'a, E, M, S> {
        SelectedQuery {
            query: self,
            selection: fields(E::COLUMNS),
        }
    }
}

impl<E: Entity, M: Memory, S: Selection<E>> SelectedQuery<'_, E, M, S> {
    /// Fetch the selected values with ordinary query bounds and continuation.
    /// # Errors
    /// Repeated columns refuse. Ordinary query, projection and cursor errors apply.
    pub fn fetch(self) -> Result<QueryPage<S::Output>, Error> {
        observe!(query, {
            let positions = self.selection.positions();
            let positions = positions.as_ref();
            let raw = fetch_rows(
                &*self.query.db.read()?,
                E::SCHEMA,
                self.query.request,
                Some(super::Projection::Columns(positions)),
            )?;
            let mut slots = [0, 1, 2, 3];
            let order = &mut slots[..positions.len()];
            order.sort_unstable_by_key(|&index| positions[index]);
            raw.decode_with(|reader| self.selection.decode(reader, order))
        })
    }
}
