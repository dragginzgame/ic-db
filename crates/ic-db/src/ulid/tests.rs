#[test]
fn ic_stream_reads_identity_once_and_separates_principals_and_versions() {
    // A fresh native thread represents a fresh generator heap.
    let stream = |principal: u8, version: u64| {
        std::thread::spawn(move || {
            let first = super::pseudo::generate(0, || {
                (crate::Principal::from_slice(&[principal]), version)
            });
            let next = |time| {
                u128::from_be_bytes(
                    super::pseudo::generate(time, || -> ([u8; 1], u64) { panic!("reseeded") })
                        .to_bytes(),
                )
            };
            let suffix = u128::from_be_bytes(first.to_bytes());
            assert!(suffix < (1_u128 << 79));
            assert_eq!(next(999_999), suffix + 1);
            assert_eq!(next(1_000_000), (1 << 80) | suffix);
            assert_eq!(next(0), ((1 << 80) | suffix) + 1);
            assert_eq!(
                next(u64::MAX),
                (u128::from(u64::MAX / 1_000_000) << 80) | suffix
            );
            suffix
        })
        .join()
        .unwrap()
    };
    let first = stream(1, 9);
    assert_eq!(first, 0x6c3d_da0f_86be_dda2_5a2c);
    assert_ne!(first, stream(2, 9));
    assert_ne!(first, stream(1, 10));
    assert_eq!(first, stream(1, 9));
}

#[test]
fn owned_streams_advance_independently_of_one_shot_issuance() {
    let identity = || (crate::Principal::from_slice(&[4]), 17);
    let first = super::pseudo::generate(0, identity);
    let mut a = super::pseudo::owned(identity());
    let mut b = super::pseudo::owned(identity());
    let number = |id: super::Ulid| u128::from_be_bytes(id.to_bytes());
    let epoch = crate::Timestamp::from_millis(0);
    let (a0, b0) = (a.next_at(epoch).unwrap(), b.next_at(epoch).unwrap());
    assert_ne!(a0, first);
    assert_ne!(b0, first);
    assert_ne!(a0, b0);
    assert_eq!(number(a.next_at(epoch).unwrap()), number(a0) + 1);
    assert_eq!(number(b.next_at(epoch).unwrap()), number(b0) + 1);
    assert_eq!(
        number(super::pseudo::generate(0, identity)),
        number(first) + 1
    );
}
