#[test]
fn fixture_initialization_and_category_query_run_without_ic_calls() {
    super::initialize(());
    assert_eq!(super::category(1), [(1, 11), (3, 31), (5, 51)]);
}
