//! Canonical ULID value, adapted from IcyDB's schema-owned atom.
//! Issuers use host-supplied time; the value owns its byte and text formats.
use std::{fmt, str::FromStr};

#[cfg(target_arch = "wasm32")]
mod canister;
mod generator;
#[cfg(not(target_arch = "wasm32"))]
mod native;
mod pseudo;
#[cfg(test)]
mod tests;
#[cfg(target_arch = "wasm32")]
use canister as platform;
#[cfg(not(target_arch = "wasm32"))]
use native as platform;

pub use generator::{UlidGenerationError, UlidGenerator};
pub use pseudo::SeededUlidGenerator;

/// Failure to obtain fresh entropy for a ULID.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum UlidEntropyError {
    /// The native entropy source failed or the IC management call produced no successful reply.
    Unavailable,
    /// The management protocol frame was invalid.
    InvalidFrame,
}

impl fmt::Display for UlidEntropyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "ULID entropy unavailable",
            Self::InvalidFrame => "invalid ULID entropy frame",
        })
    }
}

impl std::error::Error for UlidEntropyError {}

/// A 128-bit ULID with exact 16-byte storage and canonical uppercase text.
///
/// Use [`Self::generate_prand`] for synchronous issuance on the IC or a native host.
/// Construct supplied identifiers with [`Self::from_bytes`], [`Self::from_u128`] or parsing.
#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Ulid([u8; 16]);

impl Ulid {
    /// Minimum (nil) identifier.
    pub const MIN: Self = Self([0; 16]);
    /// Maximum identifier; all 128 bits are valid data.
    pub const MAX: Self = Self([255; 16]);
    fn from_millis_and_suffix(millis: u64, suffix: [u8; 10]) -> Self {
        let mut bytes = [0; 16];
        bytes[..6].copy_from_slice(&millis.to_be_bytes()[2..]);
        bytes[6..].copy_from_slice(&suffix);
        Self(bytes)
    }

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
