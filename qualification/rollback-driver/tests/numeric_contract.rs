//! Native semantic witnesses, not production scalar types or a universal value layer.
//! Candidate key bytes are checked against independent exact/standard numeric oracles.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

#[derive(Clone, Copy, Debug)]
struct Decimal {
    coefficient: i128,
    scale: u8,
}

// Fixed 41-byte decimal key: sign class, biased decimal exponent, 39 padded digits.
// Equal numbers across scales have the same key. Negative magnitude bytes reverse.
fn decimal_key(value: Decimal) -> Result<[u8; 41], &'static str> {
    if value.scale > 38 {
        return Err("unsupported scale");
    }
    let mut key = [0; 41];
    if value.coefficient == 0 {
        key[0] = 1;
        return Ok(key);
    }
    let digits = value.coefficient.unsigned_abs().to_string();
    key[0] = if value.coefficient < 0 { 0 } else { 2 };
    key[1] = (128 + digits.len() as i16 - i16::from(value.scale)) as u8;
    key[2..].fill(b'0');
    key[2..2 + digits.len()].copy_from_slice(digits.as_bytes());
    if value.coefficient < 0 {
        for byte in &mut key[1..] {
            *byte = !*byte;
        }
    }
    Ok(key)
}

// Independent oracle: scale both magnitudes to 10^-38 in 77 decimal digits.
// No exponent-key encoding, string normalization, float or overflowing multiplication.
fn exact_decimal_cmp(left: Decimal, right: Decimal) -> Ordering {
    let magnitude = |value: Decimal| {
        let mut digits = [0_u8; 77];
        let mut integer = value.coefficient.unsigned_abs();
        let mut position = 77 - (38 - usize::from(value.scale));
        while integer != 0 {
            position -= 1;
            digits[position] = (integer % 10) as u8;
            integer /= 10;
        }
        digits
    };
    match left.coefficient.signum().cmp(&right.coefficient.signum()) {
        Ordering::Equal => {
            let order = magnitude(left).cmp(&magnitude(right));
            if left.coefficient < 0 {
                order.reverse()
            } else {
                order
            }
        }
        order => order,
    }
}

#[test]
fn decimal_order_equality_uniqueness_and_grouping_agree() {
    let mut values = Vec::new();
    for coefficient in [
        i128::MIN,
        i128::MIN + 1,
        -1001,
        -100,
        -10,
        -1,
        0,
        1,
        10,
        100,
        1001,
        i128::MAX - 1,
        i128::MAX,
    ] {
        for scale in [0, 1, 2, 9, 18, 37, 38] {
            values.push(Decimal { coefficient, scale });
        }
    }
    for &left in &values {
        for &right in &values {
            assert_eq!(
                decimal_key(left).unwrap().cmp(&decimal_key(right).unwrap()),
                exact_decimal_cmp(left, right),
                "{left:?} vs {right:?}"
            );
        }
    }
    // Grouping/uniqueness use identical canonical equality; the oracle groups by numeric comparison.
    let mut groups = BTreeMap::<_, usize>::new();
    let mut oracle: Vec<(Decimal, usize)> = Vec::new();
    let mut uniqueness = BTreeSet::new();
    for &value in &values {
        let duplicate = oracle
            .iter_mut()
            .find(|(old, _)| exact_decimal_cmp(*old, value) == Ordering::Equal);
        let is_new = if let Some((_, count)) = duplicate {
            *count += 1;
            false
        } else {
            oracle.push((value, 1));
            true
        };
        let key = decimal_key(value).unwrap();
        assert_eq!(uniqueness.insert(key), is_new);
        *groups.entry(key).or_default() += 1;
    }
    oracle.sort_by(|left, right| exact_decimal_cmp(left.0, right.0));
    assert_eq!(
        groups.values().copied().collect::<Vec<_>>(),
        oracle.iter().map(|entry| entry.1).collect::<Vec<_>>()
    );
    assert_eq!(groups.len(), uniqueness.len());
    assert_eq!(
        decimal_key(Decimal {
            coefficient: 10,
            scale: 1
        }),
        decimal_key(Decimal {
            coefficient: 100,
            scale: 2
        })
    );
    assert!(
        decimal_key(Decimal {
            coefficient: 0,
            scale: 39
        })
        .is_err()
    );
    assert!(
        decimal_key(Decimal {
            coefficient: 1,
            scale: u8::MAX
        })
        .is_err()
    );
    // Integer-to-float collapse is a concrete invalid shortcut for exact numeric identity.
    let low = 1_i128 << 100;
    assert_eq!(low as f64, (low + 1) as f64);
    assert_ne!(
        decimal_key(Decimal {
            coefficient: low,
            scale: 0
        }),
        decimal_key(Decimal {
            coefficient: low + 1,
            scale: 0
        })
    );
    println!(
        "decimal: {} values / {} exact pair comparisons; {} distinct groups",
        values.len(),
        values.len() * values.len(),
        groups.len()
    );
}

fn float_key(value: f64) -> Result<[u8; 8], &'static str> {
    if !value.is_finite() {
        return Err("non-finite input");
    }
    let bits = if value == 0.0 { 0 } else { value.to_bits() };
    let key = if bits >> 63 == 1 {
        !bits
    } else {
        bits ^ (1_u64 << 63)
    };
    Ok(key.to_be_bytes())
}

#[test]
fn finite_float_order_and_zero_equality_agree() {
    let values = [
        -f64::MAX,
        -1.0,
        -f64::MIN_POSITIVE,
        -f64::from_bits(1),
        -0.0,
        0.0,
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        1.0,
        f64::MAX,
    ];
    for left in values {
        for right in values {
            assert_eq!(
                float_key(left).unwrap().cmp(&float_key(right).unwrap()),
                left.partial_cmp(&right).unwrap()
            );
        }
    }
    assert_eq!(float_key(-0.0), float_key(0.0));
    assert_eq!(
        values
            .map(|value| float_key(value).unwrap())
            .into_iter()
            .collect::<BTreeSet<_>>()
            .len(),
        9
    );
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(float_key(value).is_err());
    }
}

// A little-endian four-limb stand-in, not adoption of a new public U256 implementation.
fn u256_key(limbs: [u64; 4]) -> [u8; 32] {
    let mut key = [0; 32];
    for (slot, limb) in limbs.iter().rev().enumerate() {
        key[slot * 8..slot * 8 + 8].copy_from_slice(&limb.to_be_bytes());
    }
    key
}

#[test]
fn unsigned_256_bit_keys_preserve_all_limbs() {
    let values = [
        [0; 4],
        [1, 0, 0, 0],
        [u64::MAX, 0, 0, 0],
        [0, 1, 0, 0],
        [0, 0, 1, 0],
        [0, 0, 0, 1],
        [0, 0, 0, 1 << 63],
        [u64::MAX; 4],
    ];
    for left in values {
        for right in values {
            assert_eq!(
                u256_key(left).cmp(&u256_key(right)),
                left.iter().rev().cmp(right.iter().rev())
            );
        }
    }
    assert_eq!(u256_key([u64::MAX; 4]), [255; 32]);
}

fn composite_key(value: Option<Decimal>, id: i64) -> Result<Vec<u8>, &'static str> {
    let mut key = vec![u8::from(value.is_some())];
    match value {
        Some(value) => key.extend(decimal_key(value)?),
        None => key.extend([0; 41]),
    }
    key.extend(((id as u64) ^ (1_u64 << 63)).to_be_bytes());
    Ok(key)
}

#[test]
fn optional_decimal_composite_order_matches_component_order() {
    let values = [
        None,
        Some(Decimal {
            coefficient: -1,
            scale: 38,
        }),
        Some(Decimal {
            coefficient: 0,
            scale: 0,
        }),
        Some(Decimal {
            coefficient: 10,
            scale: 1,
        }),
        Some(Decimal {
            coefficient: 100,
            scale: 2,
        }),
    ];
    let mut rows = Vec::new();
    for value in values {
        for id in [i64::MIN, -1, 0, i64::MAX] {
            rows.push((value, id));
        }
    }
    for &(left, left_id) in &rows {
        for &(right, right_id) in &rows {
            let primary = match (left, right) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                (Some(a), Some(b)) => exact_decimal_cmp(a, b),
            };
            assert_eq!(
                composite_key(left, left_id)
                    .unwrap()
                    .cmp(&composite_key(right, right_id).unwrap()),
                primary.then(left_id.cmp(&right_id))
            );
        }
    }
    assert_ne!(
        composite_key(None, 0),
        composite_key(
            Some(Decimal {
                coefficient: 0,
                scale: 0
            }),
            0
        )
    );
}
