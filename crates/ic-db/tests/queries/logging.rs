//! Operational failures have one bounded destination and preserve the DB result.

use super::*;
use ic_db::logging::{self, Level};
use std::cell::RefCell;

#[test]
fn logging_standalone_evicts_and_pages_without_recording_successes() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    db.query::<Entry>().count().unwrap();
    assert!(logging::page(None).entries.is_empty());
    for _ in 0..130 {
        assert!(matches!(
            db.query::<Entry>().try_one(),
            Err(Error::MultipleMatches)
        ));
    }
    let mut before = None;
    let mut sequences = Vec::new();
    loop {
        let page = logging::page(before);
        assert!(page.entries.len() <= 8);
        for entry in page.entries {
            assert_eq!(entry.source, "ic-db.query");
            assert_eq!(entry.message, "multiple rows matched");
            assert_eq!(entry.level, Level::Warn);
            sequences.push(entry.sequence);
        }
        before = page.next_before;
        if before.is_none() {
            break;
        }
    }
    assert_eq!(sequences, (2..130).rev().collect::<Vec<_>>());
    assert!(logging::initialize_with_logger(|_, _, _| {}).is_err());
}

#[test]
fn logging_host_receives_commit_refusal_once() {
    thread_local! { static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }
    logging::initialize_with_logger(|level, source, message| {
        EVENTS.with_borrow_mut(|entries| entries.push(format!("{level:?} {source} {message}")));
    })
    .unwrap();
    let runtime = host(VectorMemory::default());
    let mut db = create(&runtime, 2);
    let mut duplicate = Batch::new();
    duplicate.insert(&row(1)).unwrap();
    assert!(matches!(db.commit(duplicate), Err(Error::Exists)));
    EVENTS.with_borrow(|entries| {
        assert_eq!(
            entries.as_slice(),
            ["Warn ic-db.commit primary key already exists"]
        )
    });
    assert!(logging::page(None).entries.is_empty());
}
