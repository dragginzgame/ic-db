//! Canonical decimal values adapted from frozen IcyDB 6d421e3.
//! Exact comparison and text parsing; aggregate arithmetic is a separate contract.
use std::{cmp::Ordering, fmt, str::FromStr};

/// Exact base-ten value with a signed 128-bit mantissa and scale 0–28.
/// Construction removes fractional trailing zeros; all zero values share one
/// representation. No floating-point conversion or implicit rounding is used.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Decimal {
    mantissa: i128,
    scale: u32,
}
impl Decimal {
    /// Canonical zero.
    pub const ZERO: Self = Self {
        mantissa: 0,
        scale: 0,
    };
    /// Smallest scale-zero value.
    pub const MIN: Self = Self {
        mantissa: i128::MIN,
        scale: 0,
    };
    /// Largest scale-zero value.
    pub const MAX: Self = Self {
        mantissa: i128::MAX,
        scale: 0,
    };
    /// Maximum fractional scale after exact normalization.
    pub const MAX_SCALE: u32 = 28;
    /// Construct exactly, removing fractional trailing zeros. Excess scale is
    /// accepted only if normalization brings it into range without rounding.
    #[must_use]
    pub const fn try_new(mut mantissa: i128, mut scale: u32) -> Option<Self> {
        if mantissa == 0 {
            return Some(Self::ZERO);
        }
        while scale > 0 && mantissa % 10 == 0 {
            mantissa /= 10;
            scale -= 1;
        }
        if scale > Self::MAX_SCALE {
            None
        } else {
            Some(Self { mantissa, scale })
        }
    }

    /// Canonical signed coefficient.
    #[must_use]
    pub const fn mantissa(self) -> i128 {
        self.mantissa
    }

    /// Canonical fractional scale.
    #[must_use]
    pub const fn scale(self) -> u32 {
        self.scale
    }
}
impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        // Adapt IcyDB's bounded exact alignment. If multiplication overflows,
        // its sign determines the order beyond the other i128 coefficient.
        match self.scale.cmp(&other.scale) {
            Ordering::Equal => self.mantissa.cmp(&other.mantissa),
            Ordering::Less => self
                .mantissa
                .checked_mul(10_i128.pow(other.scale - self.scale))
                .map_or_else(|| self.mantissa.cmp(&0), |v| v.cmp(&other.mantissa)),
            Ordering::Greater => other
                .mantissa
                .checked_mul(10_i128.pow(self.scale - other.scale))
                .map_or_else(|| 0.cmp(&other.mantissa), |v| self.mantissa.cmp(&v)),
        }
    }
}
impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
/// Invalid syntax or a value outside Decimal's exact coefficient/scale domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseDecimalError;
impl fmt::Display for ParseDecimalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid or out-of-range decimal")
    }
}
impl std::error::Error for ParseDecimalError {}

impl FromStr for Decimal {
    type Err = ParseDecimalError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        let (negative, unsigned) = if let Some(rest) = input.strip_prefix('-') {
            (true, rest)
        } else {
            (false, input.strip_prefix('+').unwrap_or(input))
        };
        let mut parts = unsigned.split('.');
        let integer = parts.next().ok_or(ParseDecimalError)?;
        let fractional = parts.next().unwrap_or("");
        if parts.next().is_some()
            || (integer.is_empty() && fractional.is_empty())
            || !integer
                .bytes()
                .chain(fractional.bytes())
                .all(|b| b.is_ascii_digit())
        {
            return Err(ParseDecimalError);
        }
        let scale = u32::try_from(fractional.len()).map_err(|_| ParseDecimalError)?;
        // Subtract negative digits directly, retaining the full i128::MIN domain.
        let mantissa = integer
            .bytes()
            .chain(fractional.bytes())
            .try_fold(0_i128, |value, digit| {
                let value = value.checked_mul(10)?;
                let digit = i128::from(digit - b'0');
                if negative {
                    value.checked_sub(digit)
                } else {
                    value.checked_add(digit)
                }
            })
            .ok_or(ParseDecimalError)?;
        Self::try_new(mantissa, scale).ok_or(ParseDecimalError)
    }
}
impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mantissa < 0 {
            f.write_str("-")?;
        }
        let mut digits = self.mantissa.unsigned_abs().to_string();
        let scale = self.scale as usize;
        if scale == 0 {
            return f.write_str(&digits);
        }
        if digits.len() <= scale {
            f.write_str("0.")?;
            for _ in digits.len()..scale {
                f.write_str("0")?;
            }
            f.write_str(&digits)
        } else {
            let fractional = digits.split_off(digits.len() - scale);
            write!(f, "{digits}.{fractional}")
        }
    }
}

impl Decimal {
    /// Return the exact coefficient at a declared scale, or refuse excess
    /// precision, coefficient overflow or a scale above 28. Never rounds.
    #[must_use]
    pub fn coefficient_at_scale(self, scale: u32) -> Option<i128> {
        if scale > Self::MAX_SCALE || scale < self.scale {
            return None;
        }
        self.mantissa.checked_mul(10_i128.pow(scale - self.scale))
    }

    // IcyDB's bounded sign/exponent/digits representation. Equivalent values
    // share bytes without attaching a schema or a conversion layer to operands.
    pub(crate) fn append_ordered(self, out: &mut Vec<u8>) {
        if self.mantissa == 0 {
            out.push(1);
            return;
        }
        let mut digits = [0_u8; 39];
        let mut position = digits.len();
        let mut magnitude = self.mantissa.unsigned_abs();
        while magnitude != 0 {
            position -= 1;
            digits[position] = b'0' + (magnitude % 10) as u8;
            magnitude /= 10;
        }
        let exponent = (digits.len() - position - 1) as i32 - self.scale as i32;
        let mut exponent = exponent.to_be_bytes();
        exponent[0] ^= 0x80;
        let negative = self.mantissa < 0;
        out.push(if negative { 0 } else { 2 });
        out.extend(
            exponent
                .into_iter()
                .chain(digits[position..].iter().copied())
                .map(|b| if negative { !b } else { b }),
        );
        out.push(if negative { 255 } else { 0 });
    }

    pub(crate) fn read_ordered(bytes: &[u8]) -> Option<(Self, usize)> {
        let sign = *bytes.first()?;
        if sign == 1 {
            return Some((Self::ZERO, 1));
        }
        if sign != 0 && sign != 2 {
            return None;
        }
        let negative = sign == 0;
        let undo = |b: u8| if negative { !b } else { b };
        let mut exponent: [u8; 4] = bytes.get(1..5)?.try_into().ok()?;
        exponent = exponent.map(undo);
        exponent[0] ^= 0x80;
        let exponent = i32::from_be_bytes(exponent);
        let mut mantissa = 0_i128;
        for (n, byte) in bytes.get(5..)?.iter().take(40).enumerate() {
            let digit = undo(*byte);
            if digit == 0 {
                if n == 0 || mantissa == 0 {
                    return None;
                }
                let scale = (n as i64 - 1).checked_sub(i64::from(exponent))?;
                let scale = u32::try_from(scale).ok()?;
                let value = Self::try_new(mantissa, scale)?;
                // No redundant fractional zeros, excess scale or alternate zero.
                if value.scale != scale {
                    return None;
                }
                return Some((value, 6 + n));
            }
            if !digit.is_ascii_digit() || (n == 0 && digit == b'0') || n == 39 {
                return None;
            }
            mantissa = mantissa.checked_mul(10)?;
            let digit = i128::from(digit - b'0');
            mantissa = if negative {
                mantissa.checked_sub(digit)?
            } else {
                mantissa.checked_add(digit)?
            };
        }
        None
    }
}
