#[test]
fn fixture_initialization_and_category_query_run_without_ic_calls() {
    super::initialize(());
    assert_eq!(super::category(1), [(1, 11), (3, 31), (5, 51)]);
}

#[test]
fn envelope_batch_can_be_exercised_without_a_meter() {
    super::initialize(());
    super::envelope::write(0, 2, 0, false, 48).unwrap();
    let host = super::runtime();
    let db = super::open(&host);
    assert_eq!(db.cardinality::<super::schema::EnvelopeRow>().unwrap(), 2);
    let row = db.get::<super::schema::EnvelopeRow>(2).unwrap().unwrap();
    assert_eq!(row.a.as_bytes(), &[0; 48]);
}
