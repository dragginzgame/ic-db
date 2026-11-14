//! Millisecond value semantics adapted from frozen IcyDB 6d421e3.
//! No clock, managed-field policy or transport serialization.

use std::{
    fmt::{self, Display, Formatter},
    ops::{Add, AddAssign, Sub, SubAssign},
};

use crate::Date;

/// Nonnegative millisecond duration. Constructors from larger units and arithmetic
/// operators saturate; text parsing refuses overflow. No clock or timer is owned.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Duration(u64);

impl Duration {
    /// Zero milliseconds.
    pub const ZERO: Self = Self(0);
    /// Minimum duration.
    pub const MIN: Self = Self(u64::MIN);
    /// Maximum duration.
    pub const MAX: Self = Self(u64::MAX);

    const MS_PER_SEC: u64 = 1_000;
    const SECS_PER_MIN: u64 = 60;
    const MINS_PER_HOUR: u64 = 60;
    const HOURS_PER_DAY: u64 = 24;
    const DAYS_PER_WEEK: u64 = 7;

    /// Construct from milliseconds.
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// Convert nonnegative signed milliseconds.
    #[must_use]
    pub const fn try_from_i64(millis: i64) -> Option<Self> {
        if millis < 0 {
            None
        } else {
            Some(Self(millis.cast_unsigned()))
        }
    }

    /// Construct from microseconds, truncating sub-millisecond precision.
    #[must_use]
    pub const fn from_micros_truncating(micros: u64) -> Self {
        Self(micros / Self::MS_PER_SEC)
    }

    /// Construct from nanoseconds, truncating sub-millisecond precision.
    #[must_use]
    pub const fn from_nanos_truncating(nanos: u64) -> Self {
        Self(nanos / 1_000_000)
    }

    /// Construct from seconds with saturation.
    #[must_use]
    pub const fn from_secs(seconds: u64) -> Self {
        Self(seconds.saturating_mul(Self::MS_PER_SEC))
    }

    /// Construct from minutes with saturation.
    #[must_use]
    pub const fn from_minutes(minutes: u64) -> Self {
        Self(
            minutes
                .saturating_mul(Self::SECS_PER_MIN)
                .saturating_mul(Self::MS_PER_SEC),
        )
    }

    /// Construct from hours with saturation.
    #[must_use]
    pub const fn from_hours(hours: u64) -> Self {
        Self(
            hours
                .saturating_mul(Self::MINS_PER_HOUR)
                .saturating_mul(Self::SECS_PER_MIN)
                .saturating_mul(Self::MS_PER_SEC),
        )
    }

    /// Construct from days with saturation.
    #[must_use]
    pub const fn from_days(days: u64) -> Self {
        Self(
            days.saturating_mul(Self::HOURS_PER_DAY)
                .saturating_mul(Self::MINS_PER_HOUR)
                .saturating_mul(Self::SECS_PER_MIN)
                .saturating_mul(Self::MS_PER_SEC),
        )
    }

    /// Construct from weeks with saturation.
    #[must_use]
    pub const fn from_weeks(weeks: u64) -> Self {
        Self(
            weeks
                .saturating_mul(Self::DAYS_PER_WEEK)
                .saturating_mul(Self::HOURS_PER_DAY)
                .saturating_mul(Self::MINS_PER_HOUR)
                .saturating_mul(Self::SECS_PER_MIN)
                .saturating_mul(Self::MS_PER_SEC),
        )
    }

    /// Return milliseconds.
    #[must_use]
    pub const fn as_millis(self) -> u64 {
        self.0
    }

    /// Return whole seconds.
    #[must_use]
    pub const fn as_secs(self) -> u64 {
        self.0 / Self::MS_PER_SEC
    }

    /// Return whole minutes.
    #[must_use]
    pub const fn as_minutes(self) -> u64 {
        self.0 / (Self::SECS_PER_MIN * Self::MS_PER_SEC)
    }

    /// Return whole hours.
    #[must_use]
    pub const fn as_hours(self) -> u64 {
        self.0 / (Self::MINS_PER_HOUR * Self::SECS_PER_MIN * Self::MS_PER_SEC)
    }

    /// Return whole days.
    #[must_use]
    pub const fn as_days(self) -> u64 {
        self.0 / (Self::HOURS_PER_DAY * Self::MINS_PER_HOUR * Self::SECS_PER_MIN * Self::MS_PER_SEC)
    }

    /// Return whole weeks.
    #[must_use]
    pub const fn as_weeks(self) -> u64 {
        self.0
            / (Self::DAYS_PER_WEEK
                * Self::HOURS_PER_DAY
                * Self::MINS_PER_HOUR
                * Self::SECS_PER_MIN
                * Self::MS_PER_SEC)
    }

    /// Parse integer milliseconds or `ms`, `s`, `m`, `h`, or `d` suffixes.
    ///
    /// # Errors
    ///
    /// Returns [`TimeParseError::InvalidDuration`] for malformed or overflowing
    /// input.
    pub fn parse_flexible(input: &str) -> Result<Self, TimeParseError> {
        let (digits, multiplier) = if let Some(value) = input.strip_suffix("ms") {
            (value, 1)
        } else if let Some(value) = input.strip_suffix('s') {
            (value, Self::MS_PER_SEC)
        } else if let Some(value) = input.strip_suffix('m') {
            (value, Self::SECS_PER_MIN * Self::MS_PER_SEC)
        } else if let Some(value) = input.strip_suffix('h') {
            (
                value,
                Self::MINS_PER_HOUR * Self::SECS_PER_MIN * Self::MS_PER_SEC,
            )
        } else if let Some(value) = input.strip_suffix('d') {
            (
                value,
                Self::HOURS_PER_DAY * Self::MINS_PER_HOUR * Self::SECS_PER_MIN * Self::MS_PER_SEC,
            )
        } else {
            (input, 1)
        };
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(TimeParseError::InvalidDuration);
        }
        let value = digits
            .parse::<u64>()
            .map_err(|_| TimeParseError::InvalidDuration)?;
        value
            .checked_mul(multiplier)
            .map(Self)
            .ok_or(TimeParseError::InvalidDuration)
    }
}

impl Add for Duration {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        Self(self.0.saturating_add(other.0))
    }
}

impl AddAssign for Duration {
    fn add_assign(&mut self, other: Self) {
        self.0 = self.0.saturating_add(other.0);
    }
}

impl From<u64> for Duration {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl Sub for Duration {
    type Output = Self;

    fn sub(self, other: Self) -> Self::Output {
        Self(self.0.saturating_sub(other.0))
    }
}

impl SubAssign for Duration {
    fn sub_assign(&mut self, other: Self) {
        self.0 = self.0.saturating_sub(other.0);
    }
}

/// Signed Unix-millisecond timestamp, including instants before 1970.
/// Sub-millisecond conversions floor; arithmetic operators saturate only after
/// the complete wide operation. This value never reads the clock.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Unix epoch.
    pub const EPOCH: Self = Self(0);
    /// Minimum timestamp.
    pub const MIN: Self = Self(i64::MIN);
    /// Maximum timestamp.
    pub const MAX: Self = Self(i64::MAX);

    const MILLIS_PER_SEC: i64 = 1_000;

    /// Construct from seconds with saturation.
    #[must_use]
    pub const fn from_secs(seconds: i64) -> Self {
        Self(seconds.saturating_mul(Self::MILLIS_PER_SEC))
    }

    /// Construct from milliseconds.
    #[must_use]
    pub const fn from_millis(millis: i64) -> Self {
        Self(millis)
    }

    /// Convert signed milliseconds.
    #[must_use]
    pub const fn try_from_i64(millis: i64) -> Option<Self> {
        Some(Self(millis))
    }

    /// Convert unsigned milliseconds.
    #[must_use]
    pub fn try_from_u64(millis: u64) -> Option<Self> {
        i64::try_from(millis).ok().map(Self)
    }

    /// Construct from microseconds, flooring to Unix milliseconds.
    #[must_use]
    pub const fn from_micros(micros: i64) -> Self {
        Self(micros.div_euclid(Self::MILLIS_PER_SEC))
    }

    /// Construct from nanoseconds, flooring to Unix milliseconds.
    #[must_use]
    pub const fn from_nanos(nanos: i64) -> Self {
        Self(nanos.div_euclid(1_000_000))
    }

    /// Parse strict RFC3339 text.
    ///
    /// # Errors
    ///
    /// Returns [`TimeParseError::InvalidTimestamp`] for malformed or
    /// out-of-range input.
    pub fn parse_rfc3339(input: &str) -> Result<Self, TimeParseError> {
        let bytes = input.as_bytes();
        if bytes.len() < 20
            || bytes.get(4) != Some(&b'-')
            || bytes.get(7) != Some(&b'-')
            || bytes.get(10) != Some(&b'T')
            || bytes.get(13) != Some(&b':')
            || bytes.get(16) != Some(&b':')
        {
            return Err(TimeParseError::InvalidTimestamp);
        }
        let date = input
            .get(..10)
            .and_then(Date::parse)
            .ok_or(TimeParseError::InvalidTimestamp)?;
        let hour = parse_u8(&bytes[11..13])?;
        let minute = parse_u8(&bytes[14..16])?;
        let second = parse_u8(&bytes[17..19])?;

        let mut cursor = 19;
        let nanoseconds = if bytes.get(cursor) == Some(&b'.') {
            cursor += 1;
            let start = cursor;
            while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                cursor += 1;
            }
            if cursor == start {
                return Err(TimeParseError::InvalidTimestamp);
            }
            parse_fractional_nanoseconds(&bytes[start..cursor])?
        } else {
            0
        };
        let (sign, offset_hour, offset_minute) = parse_offset(&bytes[cursor..])?;
        if hour > 23 || minute > 59 || second > 59 || offset_hour > 23 || offset_minute > 59 {
            return Err(TimeParseError::InvalidTimestamp);
        }
        // Four-digit calendar years and a sub-day offset fit comfortably in i64
        // milliseconds. The fractional contribution is nonnegative, preserving
        // floor semantics even for instants before the Unix epoch.
        let local_seconds = i64::from(date.as_days_since_epoch()) * 86_400
            + i64::from(hour) * 3_600
            + i64::from(minute) * 60
            + i64::from(second);
        let offset_seconds =
            i64::from(sign) * (i64::from(offset_hour) * 3_600 + i64::from(offset_minute) * 60);
        Ok(Self(
            (local_seconds - offset_seconds) * Self::MILLIS_PER_SEC
                + i64::from(nanoseconds / 1_000_000),
        ))
    }

    /// Parse integer milliseconds or strict RFC3339 text.
    ///
    /// # Errors
    ///
    /// Returns a typed timestamp parse error.
    pub fn parse_flexible(input: &str) -> Result<Self, TimeParseError> {
        input
            .parse::<i64>()
            .map(Self)
            .or_else(|_| Self::parse_rfc3339(input))
    }

    /// Return Unix milliseconds.
    #[must_use]
    pub const fn as_millis(self) -> i64 {
        self.0
    }

    /// Return whole Unix seconds.
    #[must_use]
    pub const fn as_secs(self) -> i64 {
        self.0.div_euclid(Self::MILLIS_PER_SEC)
    }

    fn from_wide_millis_saturating(millis: i128) -> Self {
        match i64::try_from(millis) {
            Ok(millis) => Self(millis),
            Err(_) if millis.is_negative() => Self::MIN,
            Err(_) => Self::MAX,
        }
    }
}

impl Add<Duration> for Timestamp {
    type Output = Self;

    fn add(self, duration: Duration) -> Self::Output {
        Self::from_wide_millis_saturating(i128::from(self.0) + i128::from(duration.as_millis()))
    }
}

impl AddAssign<Duration> for Timestamp {
    fn add_assign(&mut self, duration: Duration) {
        *self = *self + duration;
    }
}

impl Display for Timestamp {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

impl From<i64> for Timestamp {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl Sub<Duration> for Timestamp {
    type Output = Self;

    fn sub(self, duration: Duration) -> Self::Output {
        Self::from_wide_millis_saturating(i128::from(self.0) - i128::from(duration.as_millis()))
    }
}

impl Sub for Timestamp {
    type Output = Duration;

    fn sub(self, other: Self) -> Self::Output {
        if self.0 <= other.0 {
            return Duration::ZERO;
        }
        Duration::from_millis(
            u64::try_from(i128::from(self.0) - i128::from(other.0)).unwrap_or(u64::MAX),
        )
    }
}

impl SubAssign<Duration> for Timestamp {
    fn sub_assign(&mut self, duration: Duration) {
        *self = *self - duration;
    }
}

fn parse_u8(bytes: &[u8]) -> Result<u8, TimeParseError> {
    bytes
        .iter()
        .try_fold(0_u8, |value, byte| {
            byte.checked_sub(b'0')
                .filter(|digit| *digit <= 9)
                .and_then(|digit| value.checked_mul(10)?.checked_add(digit))
        })
        .ok_or(TimeParseError::InvalidTimestamp)
}

fn parse_fractional_nanoseconds(bytes: &[u8]) -> Result<u32, TimeParseError> {
    let mut value = 0_u32;
    for byte in bytes.iter().take(9) {
        let digit = byte
            .checked_sub(b'0')
            .filter(|digit| *digit <= 9)
            .ok_or(TimeParseError::InvalidTimestamp)?;
        value = value
            .checked_mul(10)
            .and_then(|current| current.checked_add(u32::from(digit)))
            .ok_or(TimeParseError::InvalidTimestamp)?;
    }
    for _ in bytes.len().min(9)..9 {
        value = value
            .checked_mul(10)
            .ok_or(TimeParseError::InvalidTimestamp)?;
    }
    Ok(value)
}

fn parse_offset(bytes: &[u8]) -> Result<(i8, u8, u8), TimeParseError> {
    match bytes {
        [b'Z'] => Ok((1, 0, 0)),
        [sign @ (b'+' | b'-'), hour0, hour1, b':', minute0, minute1] => Ok((
            if *sign == b'+' { 1 } else { -1 },
            parse_u8(&[*hour0, *hour1])?,
            parse_u8(&[*minute0, *minute1])?,
        )),
        _ => Err(TimeParseError::InvalidTimestamp),
    }
}

/// Invalid or out-of-range human-readable time input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeParseError {
    /// Duration text or its scaled millisecond value was invalid.
    InvalidDuration,
    /// Timestamp text or its components were invalid.
    InvalidTimestamp,
}
impl Display for TimeParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidDuration => "invalid millisecond duration",
            Self::InvalidTimestamp => "invalid Unix-millisecond timestamp",
        })
    }
}
impl std::error::Error for TimeParseError {}
