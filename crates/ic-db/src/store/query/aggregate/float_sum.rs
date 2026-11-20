//! Exact signed sums in units of the smallest subnormal of the input format.

use crate::{Error, Float32, Float64};

// Capacity includes up to one million finite inputs and a sign bit.
pub(super) type Binary32Sum = FloatSum<5, 23, 8>;
pub(super) type Binary64Sum = FloatSum<34, 52, 11>;

#[derive(Clone)]
pub(super) struct FloatSum<const WORDS: usize, const FRAC: usize, const EXP: usize> {
    // Little-endian two's complement, in units of the smallest subnormal.
    total: [u64; WORDS],
}

impl<const WORDS: usize, const FRAC: usize, const EXP: usize> Default
    for FloatSum<WORDS, FRAC, EXP>
{
    fn default() -> Self {
        Self { total: [0; WORDS] }
    }
}

impl Binary32Sum {
    pub(super) fn add(&mut self, value: Float32) {
        self.add_bits(u64::from(value.get().to_bits()));
    }

    pub(super) fn finish(&self, divisor: u64) -> Result<Float32, Error> {
        Float32::from_canonical_bits(self.rounded_bits(divisor)? as u32).ok_or(Error::Corrupt)
    }
}

impl Binary64Sum {
    pub(super) fn add(&mut self, value: Float64) {
        self.add_bits(value.get().to_bits());
    }

    pub(super) fn finish(&self, divisor: u64) -> Result<Float64, Error> {
        Float64::from_canonical_bits(self.rounded_bits(divisor)?).ok_or(Error::Corrupt)
    }
}

impl<const WORDS: usize, const FRAC: usize, const EXP: usize> FloatSum<WORDS, FRAC, EXP> {
    const EXP_MASK: u64 = (1 << EXP) - 1;
    pub(super) const BYTES: usize = WORDS * 8;

    fn add_bits(&mut self, bits: u64) {
        let exponent = ((bits >> FRAC) & Self::EXP_MASK) as usize;
        let mantissa = (bits & ((1 << FRAC) - 1)) | (u64::from(exponent != 0) << FRAC);
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
            (*word, carry) = if bits >> (FRAC + EXP) == 0 {
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
        if bytes.len() != Self::BYTES {
            return Err(Error::Corrupt);
        }
        for (word, encoded) in self.total.iter_mut().zip(bytes.as_chunks::<8>().0) {
            *word = u64::from_le_bytes(*encoded);
        }
        // Maximum finite significand times the count, shifted into subnormal units.
        let maximum = u128::from(present) * u128::from((1_u64 << (FRAC + 1)) - 1);
        let shift = Self::EXP_MASK as usize - 2;
        let offset = shift % 64;
        let mut bound = [0; WORDS];
        let mut high = maximum >> (64 - offset);
        bound[shift / 64] = (maximum << offset) as u64;
        for word in &mut bound[shift / 64 + 1..] {
            *word = high as u64;
            high >>= 64;
        }
        if high == 0 && self.magnitude().1.iter().rev().gt(bound.iter().rev()) {
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

    fn rounded_bits(&self, divisor: u64) -> Result<u64, Error> {
        let (negative, mut words) = self.magnitude();
        let divisor = u128::from(divisor);
        let mut remainder = 0;
        if divisor != 1 {
            // Long division in subnormal units. Keep the fractional unit for the
            // final rounding decision, including underflow and near-midpoints.
            for word in words.iter_mut().rev() {
                let dividend = (remainder << 64) | u128::from(*word);
                *word = (dividend / divisor) as u64;
                remainder = dividend % divisor;
            }
        }
        let highest = words
            .iter()
            .rposition(|&w| w != 0)
            .map_or(0, |top| top * 64 + 63 - words[top].leading_zeros() as usize);
        if highest > Self::EXP_MASK as usize - 2 + FRAC {
            return Err(Error::ArithmeticOverflow);
        }
        let shift = highest.saturating_sub(FRAC);
        let index = shift / 64;
        let offset = shift % 64;
        let mut leading = words[index] >> offset;
        if offset != 0 {
            leading |= words[index + 1] << (64 - offset);
        }
        if shift == 0 {
            if 2 * remainder > divisor || (2 * remainder == divisor && leading & 1 != 0) {
                leading += 1;
            }
        } else {
            let guard = (shift - 1) % 64;
            let guard_word = (shift - 1) / 64;
            let half = 1_u64 << guard;
            let sticky = remainder != 0
                || words[guard_word] & (half - 1) != 0
                || words[..guard_word].iter().any(|&v| v != 0);
            if words[guard_word] & half != 0 && (sticky || leading & 1 != 0) {
                leading += 1;
            }
        }
        // The implicit leading bit supplies the low exponent unit for normal results.
        let magnitude = ((shift as u64) << FRAC) + leading;
        if magnitude >= (Self::EXP_MASK << FRAC) {
            return Err(Error::ArithmeticOverflow);
        }
        Ok(magnitude | (u64::from(negative && magnitude != 0) << (FRAC + EXP)))
    }
}

#[cfg(test)]
fn check(inputs: &[Option<f64>], expected: Result<Option<u64>, ()>) {
    for reverse in [false, true] {
        for split in 0..=inputs.len() {
            let mut sum = Binary64Sum::default();
            let mut present = 0;
            for i in 0..=inputs.len() {
                if i == split {
                    // Simulate restoring numeric state through little-endian bytes.
                    let saved = sum.checkpoint();
                    sum = Binary64Sum::default();
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
                .finish(1)
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
    let mut sum = Binary64Sum::default();
    for _ in 0..500_000 {
        sum.add(Float64::try_new(max).unwrap());
    }
    for _ in 0..500_000 {
        sum.add(Float64::try_new(-max).unwrap());
    }
    assert_eq!(sum.finish(1).unwrap(), Float64::default());
}

#[test]
fn checkpoint_refuses_an_impossible_sum_for_its_count() {
    let mut sum = Binary64Sum::default();
    for value in [f64::MAX, -f64::MAX] {
        let mut source = Binary64Sum::default();
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
        assert!(matches!(sum.finish(1), Err(Error::ArithmeticOverflow)));
    }
}

#[test]
fn averages_round_the_exact_quotient_once() {
    let tiny = f64::from_bits(1);
    let next = f64::from_bits(1.0_f64.to_bits() + 1);
    for (inputs, expected) in [
        (vec![f64::MAX, f64::MAX], f64::MAX),
        (vec![-tiny, 0.0], 0.0),
        (vec![tiny, tiny, 0.0], tiny),
        (vec![tiny, 2.0 * tiny], 2.0 * tiny),
        (vec![1.0, next], 1.0),
        // One fractional subnormal unit in the quotient breaks a normal midpoint tie.
        (vec![2.0, next * 2.0, tiny, 0.0], next),
    ] {
        let mut total = Binary64Sum::default();
        for value in &inputs {
            total.add(Float64::try_new(*value).unwrap());
        }
        assert_eq!(
            total.finish(inputs.len() as u64).unwrap().get().to_bits(),
            expected.to_bits()
        );
    }
}

#[test]
fn binary32_uses_its_own_units_and_rounds_directly() {
    let next = f32::from_bits(1_f32.to_bits() + 1);
    let tiny = f32::from_bits(1);
    for (values, divisor, expected) in [
        (vec![1.0, 2_f32.powi(-24), tiny], 1, next),
        (vec![2.0, 2.0 * next, tiny, 0.0], 4, next),
        (vec![f32::MAX, tiny, -f32::MAX], 1, tiny),
        (vec![f32::MAX, f32::MAX], 2, f32::MAX),
        (vec![-tiny, 0.0], 2, 0.0),
    ] {
        let mut sum = Binary32Sum::default();
        for value in &values {
            sum.add(Float32::try_new(*value).unwrap());
        }
        let saved = sum.checkpoint();
        let mut restored = Binary32Sum::default();
        restored
            .restore(values.len() as u64, saved.as_flattened())
            .unwrap();
        assert_eq!(
            restored.finish(divisor).unwrap().get().to_bits(),
            expected.to_bits()
        );
    }
    let mut sum = Binary32Sum::default();
    for _ in 0..2 {
        sum.add(Float32::try_new(f32::MAX).unwrap());
    }
    assert!(matches!(sum.finish(1), Err(Error::ArithmeticOverflow)));
    assert!(matches!(
        Binary32Sum::default().restore(1, sum.checkpoint().as_flattened()),
        Err(Error::Corrupt)
    ));
}
