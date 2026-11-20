//! Fluent terminal accounting with standalone and adapter-owned storage.

use super::*;
use ic_db::metrics::{Metrics, MetricsStorage, StorageConflict, initialize_with_metrics, snapshot};
use std::rc::Rc;

#[test]
fn metrics_standalone_counts_only_completed_query_output() {
    let runtime = host(VectorMemory::default());
    let mut db = create(&runtime, 1);
    let page = db.query::<Entry>().limit(2).fetch().unwrap();
    let count = db.query::<Entry>().count().unwrap();
    assert_eq!(count.value, 8);
    assert!(db.query::<Entry>().try_one().is_err());
    let mut duplicate = Batch::new();
    duplicate.insert(&row(1)).unwrap();
    assert!(db.commit(duplicate).is_err());
    let totals = snapshot();
    assert_eq!(totals.queries.total(), 3);
    assert_eq!(totals.query_failures.total(), 1);
    assert_eq!(totals.examined.total(), page.examined as u128);
    assert_eq!(totals.fetched.total(), page.fetched as u128);
    assert_eq!(totals.returned.total(), 3);
    assert_eq!(totals.output_bytes.total(), (page.output_bytes + 8) as u128);
    assert_eq!(totals.commits.total(), 2);
    assert_eq!(totals.commit_failures.total(), 1);
    assert_eq!(initialize_with_metrics(Rc::default()), Err(StorageConflict));
}

#[test]
fn metrics_adapter_shares_one_recorder_across_reattachment() {
    assert_eq!(snapshot(), Metrics::default());
    let storage = Rc::new(MetricsStorage::default());
    initialize_with_metrics(storage.clone()).unwrap();
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 2);
    db.query::<Entry>().first_as::<Summary>().unwrap();
    initialize_with_metrics(storage.clone()).unwrap();
    assert_eq!(initialize_with_metrics(Rc::default()), Err(StorageConflict));
    assert_eq!(storage.snapshot(), snapshot());
    assert_eq!(snapshot().queries.total(), 1);
    assert_eq!(snapshot().returned.total(), 1);
}
