//! Aggregate plans seek the admitted index range and validate residual conditions.

use super::{AmountOnly, Batch, C, CategoryOnly, Entry, IndexValue, VectorMemory, create, host};
use ic_db::design::prelude::*;

#[test]
fn covered_count_prefers_the_exact_unique_key() {
    ///
    /// CodedRow
    /// Exercises competing code and score indexes on rows keyed by `id`.
    ///

    #[entity(
        store = ic_db::testing::FixtureStore,
        pk(field = "id"),
        index(name = "by_code_and_id", fields = ["code", "id"]),
        index(field = "code", unique),
        index(field = "score"),
        index(field = "active"),
        fields(
            field(name = "id", value(item(prim = "Nat64"))),
            field(name = "active", value(item(prim = "Bool"))),
            field(name = "code", value(item(prim = "Nat64"))),
            field(name = "score", value(item(prim = "Nat64")))
        )
    )]
    struct CodedRow {}

    let fixture = ic_db::testing::Fixture::new("test.count.v1");
    let mut db = fixture
        .create(Schema::new(&[CodedRow::SCHEMA]).unwrap(), [1; 16])
        .unwrap();
    let mut batch = Batch::new();
    for (id, code, score) in [(1, 7, 0), (2, 0, 1), (3, u64::MAX, 2)] {
        batch
            .insert(&CodedRow {
                id,
                code,
                score,
                active: id != 2,
            })
            .unwrap();
    }
    db.commit(batch).unwrap();

    // The compound index sorts first by name. Only the unique index proves
    // exhaustion immediately after visiting its single matching entry.
    for (code, expected) in [(7_u64, 1), (8, 0)] {
        let query = || db.query::<CodedRow>().eq("code", code).work_limit(1);
        let count = query().count().unwrap();
        assert_eq!(
            (count.value, count.examined, count.fetched),
            (expected as u64, expected, 0)
        );
        let checked = query().count_validated().unwrap();
        assert_eq!(
            (checked.value, checked.examined, checked.fetched),
            (expected as u64, expected, expected)
        );
    }
    // Counting a field scans rows; a unique index alone does not enable point probes.
    let query = || {
        db.query::<CodedRow>()
            .filter_in(CodedRow::CODE, &[0_u64, u64::MAX])
            .gte(CodedRow::SCORE, 2_u64)
            .work_limit(2)
    };
    let count = query().count_field(CodedRow::ID).unwrap();
    assert_eq!((count.value, count.examined, count.fetched), (1, 1, 1));
    let checked = query().count_validated().unwrap();
    assert_eq!(
        (checked.value, checked.examined, checked.fetched),
        (1, 2, 2)
    );
    // Both activity states cover the entire non-unique index. Only score narrows the scan.
    let count = db
        .query::<CodedRow>()
        .filter_in(CodedRow::ACTIVE, &[true, false])
        .gte(CodedRow::SCORE, 2_u64)
        .work_limit(2)
        .count_field(CodedRow::ID)
        .unwrap();
    assert_eq!((count.value, count.examined, count.fetched), (1, 1, 1));
}

#[test]
fn aggregate_unique_lookup_checks_remaining_filters_in_one_visit() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    for (value, lower_id, expected, visited) in
        [(8_u64, 2_u64, true, 1), (4, 5, false, 1), (9, 2, false, 0)]
    {
        let query = || {
            db.query::<Entry>()
                .eq("category", 0_u8)
                .eq("maybe", value)
                .filter("id", C::Ge, lower_id)
                .work_limit(1)
        };
        let count = query().count().unwrap();
        assert_eq!(
            (count.value, count.examined, count.fetched),
            (u64::from(expected), visited, visited)
        );
        let found = query().exists().unwrap();
        assert_eq!(
            (found.value, found.examined, found.fetched),
            (expected, visited, visited)
        );
    }
}

#[test]
fn aggregate_ranks_sparse_presence_below_a_narrow_range() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    let query = || db.query::<Entry>();
    for full_range in [
        query().gte(Entry::MAYBE, 0_u64),
        query().lte(Entry::MAYBE, u64::MAX),
        query().ne(Entry::MAYBE, IndexValue::Absent),
    ] {
        let count = full_range
            .gte(Entry::AMOUNT, 70_u64)
            .work_limit(3)
            .count_field(Entry::ID)
            .unwrap();
        assert_eq!((count.value, count.examined, count.fetched), (2, 2, 2));
    }
    let members = || query().filter_in(Entry::MAYBE, &[0_u64, u64::MAX]);
    let scan = members()
        .gte(Entry::AMOUNT, 70_u64)
        .work_limit(3)
        .count_field(Entry::ID)
        .unwrap();
    assert_eq!((scan.value, scan.examined, scan.fetched), (0, 2, 2));
    let probes = members().work_limit(2).count_validated().unwrap();
    assert_eq!((probes.value, probes.examined, probes.fetched), (0, 2, 0));
    // Presence alone can still avoid the two rows omitted from the sparse index.
    let present = query()
        .ne(Entry::MAYBE, IndexValue::Absent)
        .work_limit(7)
        .count_field(Entry::ID)
        .unwrap();
    assert_eq!(
        (present.value, present.examined, present.fetched),
        (6, 6, 6)
    );
}

#[test]
fn aggregate_owner_range_checks_residual_rows() {
    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 1);
    // Only IDs 6 and 8 lie in this category and range. The amount lives in
    // the owner row and rejects ID 6. Three units also allow proving exhaustion.
    let query = || {
        db.query::<Entry>()
            .eq("category", 0_u8)
            .filter("id", C::Ge, 6_u64)
            .filter("amount", C::Ge, 70_u64)
            .work_limit(3)
    };
    for result in [query().count(), query().count_validated()] {
        let count = result.unwrap();
        assert_eq!((count.value, count.examined, count.fetched), (1, 2, 2));
    }
    let found = query().exists().unwrap();
    assert_eq!((found.value, found.examined, found.fetched), (true, 2, 2));

    // ID 2 matches the other OR branch, outside the ID >= 8 interval.
    let alternatives = db
        .query::<Entry>()
        .eq("category", 0_u8)
        .filter_any(&[("id", C::Ge, 8_u64), ("amount", C::Eq, 20_u64)])
        .count()
        .unwrap();
    assert_eq!(
        (
            alternatives.value,
            alternatives.examined,
            alternatives.fetched
        ),
        (2, 4, 4)
    );
}

#[test]
fn value_aggregates_seek_one_unique_owner_and_check_residuals() {
    #[record(fields(field(name = "amount", value(item(prim = "Decimal", scale = 0)))))]
    struct AverageAmount {}

    let runtime = host(VectorMemory::default());
    let db = create(&runtime, 21);
    let query = |key: u64| db.query::<Entry>().eq(Entry::MAYBE, key).work_limit(1);
    let sum = query(8).sum::<AmountOnly>().unwrap();
    assert_eq!(
        (sum.examined, sum.fetched, sum.value.unwrap().amount),
        (1, 1, 80)
    );
    let groups = query(8).group_min::<CategoryOnly, AmountOnly>().unwrap();
    assert_eq!(
        (groups.examined, groups.fetched, groups.value.len()),
        (1, 1, 1)
    );
    assert_eq!(groups.value[0].value.as_ref().unwrap().amount, 80);
    let avg = query(8).avg::<AverageAmount>().unwrap();
    assert_eq!(
        (
            avg.examined,
            avg.fetched,
            avg.value.unwrap().amount.to_string()
        ),
        (1, 1, "80".into())
    );
    let broad = db
        .query::<Entry>()
        .filter(Entry::CATEGORY, C::Ge, 0_u8)
        .sum::<AmountOnly>()
        .unwrap();
    assert_eq!(
        (broad.examined, broad.fetched, broad.value.unwrap().amount),
        (8, 8, 360)
    );
    let miss = query(99).max::<AmountOnly>().unwrap();
    assert_eq!((miss.examined, miss.fetched, miss.value), (0, 0, None));
    let filtered = query(8)
        .eq(Entry::CATEGORY, 1_u8)
        .sum::<AmountOnly>()
        .unwrap();
    assert_eq!(
        (filtered.examined, filtered.fetched, filtered.value),
        (1, 1, None)
    );
    // Both absent rows lie outside the present-only unique index.
    let absent = db
        .query::<Entry>()
        .filter_absent(Entry::MAYBE)
        .sum::<AmountOnly>()
        .unwrap();
    assert_eq!(absent.value.unwrap().amount, 90);
}
