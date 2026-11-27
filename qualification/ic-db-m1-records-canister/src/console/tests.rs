use super::retained::{resume_at, start_at};
use ic_db::Timestamp;

#[test]
fn retained_count_expires_at_its_deadline() {
    crate::initialize(());
    let selection = crate::transport::RetainedSelection {
        entity: "Entry".into(),
        select: "count".into(),
        entity_filters: (None, None, None),
        limit: 32,
        group_bounds: (None, None),
        group_order: None,
        filters: super::RowFilters::default(),
        order: Default::default(),
    };
    let handle = start_at(&selection, Timestamp::from_millis(1_000)).unwrap();
    let request = |sequence| crate::transport::RetainedResume {
        selection: selection.clone(),
        handle: handle.clone(),
        sequence,
        work: 1,
        bytes: 4096,
        page_size: 0,
        after: None,
    };
    let first = resume_at(request(0), Timestamp::from_millis(60_999)).unwrap();
    assert_eq!(first.phase, crate::transport::RetainedPhase::Running);
    let expired = resume_at(request(first.sequence), Timestamp::from_millis(61_000)).unwrap();
    assert_eq!(expired.phase, crate::transport::RetainedPhase::Expired);
}
