//! Display independently recorded schema fingerprints for comparison.

#[track_caller]
pub(super) fn check(actual: [u8; 32], expected: &str) {
    assert_eq!(actual.map(|byte| format!("{byte:02x}")).concat(), expected);
}
