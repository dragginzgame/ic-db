//! Binary Serde shapes used in application endpoint inspection.
use crate::{Date, Decimal, Duration, Float32, Float64, Timestamp, U256, Ulid};

/// Project a DB-owned value through its binary Serde layout.
#[doc(hidden)]
#[macro_export]
macro_rules! __wire_as {
    ($value:ty, $layout:ty) => {
        impl $crate::__private::ic_wire::typescript::Wire for $value {
            fn schema(
                context: &mut $crate::__private::ic_wire::typescript::SchemaContext,
            ) -> ::core::result::Result<
                $crate::__private::ic_wire::typescript::__private::Value,
                $crate::__private::ic_wire::typescript::GenerationError,
            > {
                context.describe::<$layout>()
            }
        }
    };
}

__wire_as!(Ulid, [u8; 16]);
__wire_as!(crate::QueryCursor, Vec<u8>);
__wire_as!(Date, i32);
__wire_as!(Decimal, str);
__wire_as!(Duration, u64);
__wire_as!(Timestamp, i64);
__wire_as!(Float32, ic_wire::FiniteF32);
__wire_as!(Float64, ic_wire::FiniteF64);

__wire_as!(U256, str);
