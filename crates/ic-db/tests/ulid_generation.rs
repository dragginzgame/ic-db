//! Issuance uses independent literal bytes and integer ordering as its oracle.
use ic_db::{Timestamp, Ulid, UlidGenerationError, UlidGenerator};

#[test]
fn timestamp_and_suffix_are_exact_including_epoch_and_maximum() {
    let mut generator = UlidGenerator::new();
    assert_eq!(generator.last(), None);
    let first = generator
        .generate(Timestamp::from_millis(0), [0x12; 10])
        .unwrap();
    assert_eq!(
        first.to_bytes(),
        [0, 0, 0, 0, 0, 0, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18]
    );
    let maximum = generator
        .generate(Timestamp::from_millis((1_i64 << 48) - 1), [255; 10])
        .unwrap();
    assert_eq!(maximum, Ulid::MAX);
    assert_eq!(generator.last(), Some(maximum));
}

#[test]
fn same_and_backward_times_increment_without_replacing_randomness() {
    let mut generator = UlidGenerator::new();
    let first = generator
        .generate(Timestamp::from_millis(1000), [0; 10])
        .unwrap();
    let same = generator
        .generate(Timestamp::from_millis(1000), [255; 10])
        .unwrap();
    let backward = generator
        .generate(Timestamp::from_millis(999), [255; 10])
        .unwrap();
    let later = generator
        .generate(Timestamp::from_millis(1001), [7; 10])
        .unwrap();
    assert_eq!(u128::from_be_bytes(first.to_bytes()), 1000_u128 << 80);
    assert_eq!(u128::from_be_bytes(same.to_bytes()), (1000_u128 << 80) + 1);
    assert_eq!(
        u128::from_be_bytes(backward.to_bytes()),
        (1000_u128 << 80) + 2
    );
    assert!(first < same && same < backward && backward < later);
    assert_eq!(&later.to_bytes()[6..], &[7; 10]);
}

#[test]
fn refusals_preserve_state_and_later_time_can_recover_suffix_exhaustion() {
    let mut generator = UlidGenerator::new();
    for millis in [-1, i64::MIN, 1_i64 << 48, i64::MAX] {
        assert_eq!(
            generator.generate(Timestamp::from_millis(millis), [0; 10]),
            Err(UlidGenerationError::InvalidTimestamp)
        );
        assert_eq!(generator.last(), None);
    }
    let last = generator
        .generate(Timestamp::from_millis(1000), [255; 10])
        .unwrap();
    for millis in [0, 999, 1000] {
        assert_eq!(
            generator.generate(Timestamp::from_millis(millis), [0; 10]),
            Err(UlidGenerationError::Exhausted)
        );
        assert_eq!(generator.last(), Some(last));
    }
    // Invalid time refuses even when it could otherwise select the increment branch.
    assert_eq!(
        generator.generate(Timestamp::from_millis(-1), [0; 10]),
        Err(UlidGenerationError::InvalidTimestamp)
    );
    assert_eq!(generator.last(), Some(last));
    assert_eq!(
        u128::from_be_bytes(
            generator
                .generate(Timestamp::from_millis(1001), [0; 10])
                .unwrap()
                .to_bytes()
        ),
        1001_u128 << 80
    );
    let mut terminal = UlidGenerator::resume_after(Ulid::MAX);
    assert_eq!(
        terminal.generate(Timestamp::from_millis(0), [0; 10]),
        Err(UlidGenerationError::Exhausted)
    );
    assert_eq!(terminal.last(), Some(Ulid::MAX));
}

#[test]
fn restoring_the_last_issued_value_continues_but_resetting_can_repeat() {
    let mut generator = UlidGenerator::new();
    let timestamp = Timestamp::from_millis(42);
    let first = generator.generate(timestamp, [3; 10]).unwrap();
    let second = generator.generate(timestamp, [3; 10]).unwrap();
    let mut restored = UlidGenerator::resume_after(generator.last().unwrap());
    let next = restored
        .generate(Timestamp::from_millis(41), [9; 10])
        .unwrap();
    assert_eq!(
        u128::from_be_bytes(next.to_bytes()),
        u128::from_be_bytes(second.to_bytes()) + 1
    );
    // This negative control rules out promises of automatic upgrade/global uniqueness.
    assert_eq!(
        UlidGenerator::new().generate(timestamp, [3; 10]).unwrap(),
        first
    );
}
