//! Database-owned unsigned 256-bit value. Ethnum supplies arithmetic and hex Serde.
use ethnum::U256 as Uint;
use std::{fmt, ops, str::FromStr};

/// Unsigned 256-bit integer, encoded as a hexadecimal quantity by Serde.
#[repr(transparent)]
#[derive(
    Clone, Copy, Default, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct U256(Uint);

impl U256 {
    /// Smallest representable value.
    pub const MIN: Self = Self(Uint::MIN);
    /// Zero.
    pub const ZERO: Self = Self(Uint::ZERO);
    /// One.
    pub const ONE: Self = Self(Uint::ONE);
    /// Largest representable value.
    pub const MAX: Self = Self(Uint::MAX);

    /// Widen an unsigned 128-bit integer.
    #[must_use]
    pub const fn new(value: u128) -> Self {
        Self(Uint::new(value))
    }

    /// Return the high and low 128-bit words.
    #[must_use]
    pub const fn into_words(self) -> (u128, u128) {
        self.0.into_words()
    }

    /// Construct from high and low 128-bit words.
    #[must_use]
    pub const fn from_words(high: u128, low: u128) -> Self {
        Self(Uint::from_words(high, low))
    }

    /// Construct from exactly 32 big-endian bytes.
    #[must_use]
    pub fn from_be_bytes(bytes: [u8; 32]) -> Self {
        Self(Uint::from_be_bytes(bytes))
    }

    /// Return exactly 32 big-endian bytes.
    #[must_use]
    pub fn to_be_bytes(self) -> [u8; 32] {
        self.0.to_be_bytes()
    }

    /// Add two values, returning None on overflow.
    #[must_use]
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.0).map(Self)
    }
}

impl From<U256> for Uint {
    fn from(value: U256) -> Self {
        value.0
    }
}

macro_rules! from_unsigned {
    ($($ty:ty),+) => {$(
        impl From<$ty> for U256 {
            fn from(value: $ty) -> Self { Self(Uint::from(value)) }
        }
    )+};
}
from_unsigned!(Uint, u8, u16, u32, u64, u128, bool);

impl FromStr for U256 {
    type Err = <Uint as FromStr>::Err;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

macro_rules! format_uint {
    ($($trait:ident),+) => {$(
        impl fmt::$trait for U256 {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::$trait::fmt(&self.0, f)
            }
        }
    )+};
}
format_uint!(Debug, Display, LowerHex, UpperHex, Binary, Octal);

macro_rules! arithmetic {
    ($($trait:ident::$method:ident),+) => {$(
        impl ops::$trait for U256 {
            type Output = Self;
            fn $method(self, rhs: Self) -> Self { Self(self.0.$method(rhs.0)) }
        }
        impl ops::$trait<u128> for U256 {
            type Output = Self;
            fn $method(self, rhs: u128) -> Self { Self(self.0.$method(rhs)) }
        }
    )+};
}
arithmetic!(Add::add, Sub::sub, Mul::mul, Div::div, Rem::rem);

impl ops::Shl<u32> for U256 {
    type Output = Self;
    fn shl(self, shift: u32) -> Self {
        Self(self.0 << shift)
    }
}

impl ops::Shr<u32> for U256 {
    type Output = Self;
    fn shr(self, shift: u32) -> Self {
        Self(self.0 >> shift)
    }
}
