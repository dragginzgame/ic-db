//! Consumer metrics and refusal checks independent of producer implementation.

mod artifact;

use sha2::{Digest, Sha256};
use std::io::{Read, repeat};

// One exported no-argument function, with a four-byte code-section payload.
const MODULE: &[u8] = &[
    0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2, 1, 0, 7, 5, 1, 1, 102, 0, 0, 10, 4, 1, 2,
    0, 11,
];

#[test]
fn metrics_commit_complete_original_bytes() {
    let (bytes, facts) = artifact::read(MODULE).unwrap();
    assert_eq!(bytes, MODULE);
    assert_eq!(facts.identity.bytes, 31);
    assert_eq!(
        facts.identity.sha256.as_slice(),
        Sha256::digest(MODULE).as_slice()
    );
    assert_eq!(facts.code_section_bytes, 4);
    assert_eq!(facts.exports.into_iter().collect::<Vec<_>>(), ["f"]);
}

#[test]
fn incomplete_invalid_and_duplicate_export_modules_refuse() {
    for end in [0, 7, MODULE.len() - 1] {
        assert!(artifact::read(&MODULE[..end]).is_err());
    }
    let mut invalid = MODULE.to_vec();
    invalid[24] = 1; // Only function index zero exists.
    assert!(artifact::read(invalid.as_slice()).is_err());
    let mut duplicate = MODULE[..18].to_vec();
    duplicate.extend([7, 9, 2, 1, b'f', 0, 0, 1, b'f', 0, 0]);
    duplicate.extend(&MODULE[25..]);
    assert!(artifact::read(duplicate.as_slice()).is_err());
    assert!(artifact::read(&b"\0asm\x0d\0\x01\0"[..]).is_err());
}

#[test]
fn input_and_custom_section_limits_remain_explicit() {
    assert!(artifact::read(repeat(0).take(32 * 1024 * 1024 + 1)).is_err());
    let mut module = MODULE.to_vec();
    for _ in 0..32 {
        module.extend([0, 1, 0]); // Empty-name custom section.
    }
    let (_, facts) = artifact::read(module.as_slice()).unwrap();
    assert_eq!(facts.code_section_bytes, 4);
    module.extend([0, 1, 0]);
    assert!(artifact::read(module.as_slice()).is_err());
}
