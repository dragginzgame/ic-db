//! Canonical ULID value, adapted from IcyDB's schema-owned atom.
//! Generation, clocks and entropy are separate write-policy responsibilities.
use std::{fmt, str::FromStr};

mod generator;
pub use generator::{UlidGenerationError, UlidGenerator};

/// A 128-bit ULID with exact 16-byte storage and canonical uppercase text.
///
/// This value has no automatic generator or randomness requirement. Construct
/// supplied identifiers with [`Self::from_bytes`], [`Self::from_u128`] or parsing.
#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Ulid([u8; 16]);

impl Ulid {
    /// Minimum (nil) identifier.
    pub const MIN: Self = Self([0; 16]);
    /// Maximum identifier; all 128 bits are valid data.
    pub const MAX: Self = Self([255; 16]);
    /// Construct from exactly 16 bytes, timestamp prefix first.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Construct from the unsigned big-endian integer representation.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(value.to_be_bytes())
    }

    /// Return the exact 16-byte representation.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 16] {
        self.0
    }
}

/// Text was not an exact canonical uppercase 26-character ULID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UlidParseError;

impl fmt::Display for UlidParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a canonical uppercase 26-character ULID")
    }
}
impl std::error::Error for UlidParseError {}
impl fmt::Display for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&::ulid::Ulid::from_bytes(self.0), f)
    }
}
impl fmt::Debug for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string(), f)
    }
}
impl FromStr for Ulid {
    type Err = UlidParseError;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let value = text.parse::<::ulid::Ulid>().map_err(|_| UlidParseError)?;
        if value.to_string() != text {
            return Err(UlidParseError);
        }
        Ok(Self::from_bytes(value.to_bytes()))
    }
}
