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
        Default::default(),
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
    let first = resume_at(request(0), Timestamp::from_millis(60_999)).unwrap();
    assert_eq!(first.phase, crate::transport::RetainedPhase::Running);
    let expired = resume_at(request(first.sequence), Timestamp::from_millis(61_000)).unwrap();
    assert_eq!(expired.phase, crate::transport::RetainedPhase::Expired);
}
