use super::retained::{resume_at, start_at};
use ic_db::Timestamp;

#[test]
fn retained_count_expires_at_its_deadline() {
    crate::initialize(());
    let selection = (
        "Entry".into(),
        "count".into(),
        (None, None, None),
        32,
        (None, None),
        None,
        super::RowFilters::default(),
    );
    let handle = start_at(&selection, Timestamp::from_millis(1_000)).unwrap();
    let request = |sequence| {
        (
            selection.clone(),
            handle.clone(),
            sequence,
            1,
            4096,
            0,
            None,
        )
    };
    let (sequence, phase, ..) = resume_at(request(0), Timestamp::from_millis(60_999)).unwrap();
    assert_eq!(phase, "Running");
    let (_, phase, ..) = resume_at(request(sequence), Timestamp::from_millis(61_000)).unwrap();
    assert_eq!(phase, "Expired");
}
