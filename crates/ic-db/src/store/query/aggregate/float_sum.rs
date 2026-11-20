//! Exact signed sums in units of the smallest binary64 subnormal.

use crate::{Error, Float64};

const WORDS: usize = 34;
const FRACTION: u64 = (1 << 52) - 1;

#[derive(Clone)]
pub(super) struct FloatSum {
    // Little-endian two's complement, in units of the smallest subnormal.
    // At most one million finite operands require 2118 magnitude bits.
    total: [u64; WORDS],
}

impl Default for FloatSum {
    fn default() -> Self {
        Self { total: [0; WORDS] }
    }
}

impl FloatSum {
    pub(super) const BYTES: usize = WORDS * 8;

    pub(super) fn add(&mut self, value: Float64) {
        let bits = value.get().to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as usize;
        let mantissa = (bits & FRACTION) | (u64::from(exponent != 0) << 52);
        let shift = exponent.saturating_sub(1);
        let offset = shift % 64;
        let low = mantissa << offset;
        let high = if offset == 0 {
            0
        } else {
            mantissa >> (64 - offset)
        };
        let mut carry = false;
        for (i, word) in self.total[shift / 64..].iter_mut().enumerate() {
            let term = match i {
                0 => low,
                1 => high,
                _ if !carry => break,
                _ => 0,
            };
            (*word, carry) = if bits >> 63 == 0 {
                word.carrying_add(term, carry)
            } else {
                word.borrowing_sub(term, carry)
            };
        }
    }

    pub(super) fn checkpoint(&self) -> [[u8; 8]; WORDS] {
        self.total.map(u64::to_le_bytes)
    }

    pub(super) fn restore(&mut self, present: u64, bytes: &[u8]) -> Result<(), Error> {
        let encoded: &[u8; Self::BYTES] = bytes.try_into().map_err(|_| Error::Corrupt)?;
        for (word, encoded) in self.total.iter_mut().zip(encoded.as_chunks::<8>().0) {
            *word = u64::from_le_bytes(*encoded);
        }
        // Every term is at most (2^53 - 1) << 2045 subnormal units.
        let maximum = u128::from(present) * u128::from((1_u64 << 53) - 1);
        let mut bound = [0; WORDS];
        bound[31] = (maximum << 61) as u64;
        bound[32] = (maximum >> 3) as u64;
        bound[33] = (maximum >> 67) as u64;
        if self.magnitude().1.iter().rev().gt(bound.iter().rev()) {
            return Err(Error::Corrupt);
        }
        Ok(())
    }

    fn magnitude(&self) -> (bool, [u64; WORDS]) {
        let mut words = self.total;
        let negative = words[WORDS - 1] >> 63 != 0;
        if negative {
            let mut carry = true;
            for word in &mut words {
                (*word, carry) = (!*word).overflowing_add(u64::from(carry));
            }
        }
        (negative, words)
    }

    pub(super) fn finish(&self) -> Result<Float64, Error> {
        let (negative, words) = self.magnitude();
        let Some(top) = words.iter().rposition(|&w| w != 0) else {
            return Ok(Float64::default());
        };
        let highest = top * 64 + 63 - words[top].leading_zeros() as usize;
        if highest > 2097 {
            return Err(Error::ArithmeticOverflow);
        }
        let shift = highest.saturating_sub(52);
        let index = shift / 64;
        let offset = shift % 64;
        let mut leading = words[index] >> offset;
        if offset != 0 {
            leading |= words[index + 1] << (64 - offset);
        }
        if shift != 0 {
            let guard = (shift - 1) % 64;
            let guard_word = (shift - 1) / 64;
            let half = 1_u64 << guard;
            let sticky =
                words[guard_word] & (half - 1) != 0 || words[..guard_word].iter().any(|&v| v != 0);
            if words[guard_word] & half != 0 && (sticky || leading & 1 != 0) {
                leading += 1;
            }
        }
        // The implicit leading bit supplies the low exponent unit for normal results.
        let magnitude = ((shift as u64) << 52) + leading;
        if magnitude >= f64::INFINITY.to_bits() {
            return Err(Error::ArithmeticOverflow);
        }
        Float64::from_canonical_bits(magnitude | (u64::from(negative) << 63)).ok_or(Error::Corrupt)
    }
}

#[cfg(test)]
fn check(inputs: &[Option<f64>], expected: Result<Option<u64>, ()>) {
    for reverse in [false, true] {
        for split in 0..=inputs.len() {
            let mut sum = FloatSum::default();
            let mut present = 0;
            for i in 0..=inputs.len() {
                if i == split {
                    // Simulate restoring numeric state through little-endian bytes.
                    let saved = sum.checkpoint();
                    sum = FloatSum::default();
                    sum.restore(present, saved.as_flattened()).unwrap();
                }
                if i < inputs.len()
                    && let Some(value) = inputs[if reverse { inputs.len() - 1 - i } else { i }]
                {
                    sum.add(Float64::try_new(value).unwrap());
                    present += 1;
                }
            }
            let result = sum
                .finish()
                .map(|x| (present != 0).then(|| x.get().to_bits()))
                .map_err(|_| ());
            assert_eq!(result, expected);
        }
    }
}

#[test]
fn verify_float_sum() {
    let tiny = f64::from_bits(1);
    let max = f64::MAX;
    let half = 2_f64.powi(-53);
    check(&[None], Ok(None));
    check(&[Some(-0.0)], Ok(Some(0)));
    check(
        &[Some(1e16), None, Some(1.0), Some(-1e16)],
        Ok(Some(1.0_f64.to_bits())),
    );
    check(&[Some(max), Some(max), Some(-max)], Ok(Some(max.to_bits())));
    check(
        &[Some(max), Some(-max), Some(-tiny)],
        Ok(Some((-tiny).to_bits())),
    );
    check(&[Some(1.0), Some(half)], Ok(Some(1.0_f64.to_bits())));
    check(
        &[Some(1.0), Some(half), Some(tiny)],
        Ok(Some(1.0_f64.to_bits() + 1)),
    );
    check(
        &[Some(f64::from_bits(1.0_f64.to_bits() + 1)), Some(half)],
        Ok(Some(1.0_f64.to_bits() + 2)),
    );
    check(
        &[Some(f64::MIN_POSITIVE), Some(-tiny)],
        Ok(Some(f64::MIN_POSITIVE.to_bits() - 1)),
    );
    check(&[Some(max), Some(tiny)], Ok(Some(max.to_bits())));
    check(&[Some(max), Some(2_f64.powi(970))], Err(()));
    check(&[Some(-max), Some(-max)], Err(()));
    let mut sum = FloatSum::default();
    for _ in 0..500_000 {
        sum.add(Float64::try_new(max).unwrap());
    }
    for _ in 0..500_000 {
        sum.add(Float64::try_new(-max).unwrap());
    }
    assert_eq!(sum.finish().unwrap(), Float64::default());
}

#[test]
fn checkpoint_refuses_an_impossible_sum_for_its_count() {
    let mut sum = FloatSum::default();
    for value in [f64::MAX, -f64::MAX] {
        let mut source = FloatSum::default();
        source.add(Float64::try_new(value).unwrap());
        source.add(Float64::try_new(value).unwrap());
        let saved = source.checkpoint();
        assert!(matches!(
            sum.restore(1, saved.as_flattened()),
            Err(Error::Corrupt)
        ));
        assert!(matches!(
            sum.restore(2, &saved.as_flattened()[1..]),
            Err(Error::Corrupt)
        ));
        sum.restore(2, saved.as_flattened()).unwrap();
        assert!(matches!(sum.finish(), Err(Error::ArithmeticOverflow)));
    }
}
