//! Finite scalar boundary adapted from IcyDB's schema/atom at 6d421e3.
//! One canonical zero makes equality, hashing, collections and keys agree.
use std::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
};

// The same invariant applies to both widths; no generic runtime numeric layer.
macro_rules! finite_float {
    ($name:ident, $float:ty, $bits:ty, $width:literal) => {
        #[doc = concat!("Finite canonical `", stringify!($float), "`. Construction normalizes negative zero; NaN and infinities refuse.")]
        #[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
        #[serde(transparent)]
        pub struct $name($float);
        impl $name {
            /// Admit a finite primitive and normalize either signed zero to positive zero.
            #[must_use]
            pub fn try_new(value: $float) -> Option<Self> {
                value.is_finite().then(|| Self(if value == 0.0 { 0.0 } else { value }))
            }
            /// Return the finite primitive value.
            #[must_use]
            pub const fn get(self) -> $float { self.0 }

            // Stored data must already be canonical. Unlike input construction,
            // decoding must not silently repair a negative-zero representation.
            pub(crate) fn from_canonical_bits(bits: $bits) -> Option<Self> {
                let value = Self::try_new(<$float>::from_bits(bits))?;
                (value.0.to_bits() == bits).then_some(value)
            }

            pub(crate) fn ordered_bytes(self) -> [u8; $width] {
                let bits = self.0.to_bits();
                let sign = 1 << (<$bits>::BITS - 1);
                (if bits & sign != 0 { !bits } else { bits ^ sign }).to_be_bytes()
            }

            pub(crate) fn from_ordered_bytes(bytes: [u8; $width]) -> Option<Self> {
                let bits = <$bits>::from_be_bytes(bytes);
                let sign = 1 << (<$bits>::BITS - 1);
                Self::from_canonical_bits(if bits & sign != 0 { bits ^ sign } else { !bits })
            }
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::try_new(<$float>::deserialize(deserializer)?)
                    .ok_or_else(|| serde::de::Error::custom("expected a finite float"))
            }
        }
        impl Eq for $name {}
        impl Ord for $name {
            fn cmp(&self, other: &Self) -> Ordering { self.0.total_cmp(&other.0) }
        }
        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
        }
        impl Hash for $name {
            fn hash<H: Hasher>(&self, state: &mut H) { self.0.to_bits().hash(state); }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { fmt::Display::fmt(&self.0, f) }
        }
        impl From<$name> for $float {
            fn from(value: $name) -> Self { value.get() }
        }
    };
}
finite_float!(Float32, f32, u32, 4);
finite_float!(Float64, f64, u64, 8);
