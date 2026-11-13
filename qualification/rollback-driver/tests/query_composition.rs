//! Small fixed-input composition witnesses, not a fluent builder or query executor.
//! Native accumulation establishes semantics only; Wasm job recovery is tested separately.

use std::{cmp::Reverse, collections::BTreeMap, collections::BTreeSet};

// (category, value), in entity-ID order. Equal projected tuples still have different IDs.
const INPUT: [(u8, u64); 4] = [(b'A', 10), (b'B', 20), (b'A', 30), (b'B', 20)];
// None means the value field was not selected, not a nullable stored value.
type Projected = (u8, Option<u64>);

#[derive(Debug, PartialEq, Eq)]
enum Progress<T> {
    Incomplete,
    Complete(T),
}

#[derive(Debug, PartialEq, Eq)]
struct Page<T> {
    rows: Vec<T>,
    next: Option<usize>,
}

// Offset pagination is safe only within this fixed completed output, not live entity pages.
fn page<T: Clone>(output: &[T], after: usize, limit: usize) -> Result<Page<T>, &'static str> {
    if !(1..=4).contains(&limit) || after > output.len() {
        return Err("output bound");
    }
    let end = (after + limit).min(output.len());
    Ok(Page {
        rows: output[after..end].to_vec(),
        next: (end < output.len()).then_some(end),
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Distinct {
    position: usize,
    keys: BTreeSet<Projected>,
}

impl Distinct {
    // The caller fixes projection and capacity for the entire run. No public cursor/handle.
    fn advance(
        &mut self,
        work: usize,
        tuple: bool,
        capacity: usize,
    ) -> Result<Progress<Vec<Projected>>, &'static str> {
        if !(1..=4).contains(&work) || !(1..=4).contains(&capacity) {
            return Err("input bound");
        }
        let end = (self.position + work).min(INPUT.len());
        let mut keys = self.keys.clone();
        for &(category, value) in &INPUT[self.position..end] {
            let key = (category, tuple.then_some(value));
            if !keys.contains(&key) && keys.len() == capacity {
                return Err("distinct capacity");
            }
            keys.insert(key);
        }
        self.keys = keys;
        self.position = end;
        Ok(if end == INPUT.len() {
            Progress::Complete(self.keys.iter().copied().collect())
        } else {
            Progress::Incomplete
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Grouped {
    position: usize,
    sums: BTreeMap<u8, u64>,
}

impl Grouped {
    // Fixed stages: row value >= minimum; group category; sum; sum > 35; sum DESC/category ASC.
    fn advance(
        &mut self,
        work: usize,
        minimum: u64,
        capacity: usize,
    ) -> Result<Progress<Vec<(u8, u64)>>, &'static str> {
        if !(1..=4).contains(&work) || !(1..=4).contains(&capacity) {
            return Err("input bound");
        }
        let end = (self.position + work).min(INPUT.len());
        let mut sums = self.sums.clone();
        for &(category, value) in &INPUT[self.position..end] {
            if value < minimum {
                continue;
            }
            if !sums.contains_key(&category) && sums.len() == capacity {
                return Err("group capacity");
            }
            let sum = sums.entry(category).or_default();
            *sum = sum.checked_add(value).ok_or("sum overflow")?;
        }
        self.sums = sums;
        self.position = end;
        if end != INPUT.len() {
            return Ok(Progress::Incomplete);
        }
        // At most four retained groups: finalization has its own structural bound.
        let mut output: Vec<_> = self
            .sums
            .iter()
            .filter(|(_, sum)| **sum > 35)
            .map(|(&category, &sum)| (category, sum))
            .collect();
        output.sort_unstable_by_key(|&(category, sum)| (Reverse(sum), category));
        Ok(Progress::Complete(output))
    }
}

#[test]
fn distinct_uses_the_selected_projection_before_count_or_output_limit() {
    for work in 1..=4 {
        for tuple in [false, true] {
            let mut run = Distinct::default();
            let output = loop {
                match run.advance(work, tuple, 4).unwrap() {
                    Progress::Incomplete => assert!(run.position < INPUT.len()),
                    Progress::Complete(output) => break output,
                }
            };
            let expected = if tuple {
                vec![(b'A', Some(10)), (b'A', Some(30)), (b'B', Some(20))]
            } else {
                vec![(b'A', None), (b'B', None)]
            };
            assert_eq!(output, expected);
            // Count is over the complete selected distinct projection, before output paging.
            assert_eq!(output.len(), if tuple { 3 } else { 2 });
            let mut after = 0;
            let mut combined = Vec::new();
            loop {
                let result = page(&output, after, 1).unwrap();
                combined.extend(result.rows);
                match result.next {
                    Some(next) => after = next,
                    None => break,
                }
            }
            assert_eq!(combined, expected);
        }
    }
    // Page-local distinct counts would incorrectly count A and B twice.
    assert_eq!(
        INPUT
            .chunks(2)
            .map(|chunk| chunk.iter().map(|row| row.0).collect::<BTreeSet<_>>().len())
            .sum::<usize>(),
        4
    );
}

#[test]
fn grouping_finishes_before_group_filter_order_and_output_limit() {
    for work in 1..=4 {
        let mut run = Grouped::default();
        let output = loop {
            match run.advance(work, 0, 4).unwrap() {
                Progress::Incomplete => assert!(run.position < INPUT.len()),
                Progress::Complete(output) => break output,
            }
        };
        assert_eq!(output, vec![(b'A', 40), (b'B', 40)]);
        assert_eq!(
            page(&output, 0, 1).unwrap(),
            Page {
                rows: vec![(b'A', 40)],
                next: Some(1)
            }
        );
        assert_eq!(
            page(&output, 1, 1).unwrap(),
            Page {
                rows: vec![(b'B', 40)],
                next: None
            }
        );
    }
    let mut run = Grouped::default();
    assert_eq!(run.advance(2, 0, 4), Ok(Progress::Incomplete));
    assert_eq!(run.sums, BTreeMap::from([(b'A', 10), (b'B', 20)]));
    assert_eq!(
        run.advance(2, 0, 4),
        Ok(Progress::Complete(vec![(b'A', 40), (b'B', 40)]))
    );
}

#[test]
fn row_filter_precedes_grouping_and_empty_results_are_explicit() {
    let mut run = Grouped::default();
    assert_eq!(run.advance(2, 20, 4), Ok(Progress::Incomplete));
    assert_eq!(
        run.advance(2, 20, 4),
        Ok(Progress::Complete(vec![(b'B', 40)]))
    );
    // A has sum 30 after the row filter; applying only the group filter would retain A=40.
    assert_eq!(run.sums, BTreeMap::from([(b'A', 30), (b'B', 40)]));

    for minimum in [30, 100] {
        let mut run = Grouped::default();
        assert_eq!(run.advance(4, minimum, 4), Ok(Progress::Complete(vec![])));
    }
    assert_eq!(
        page::<(u8, u64)>(&[], 0, 1).unwrap(),
        Page {
            rows: vec![],
            next: None
        }
    );
    assert_eq!(page(&[1], 0, 0), Err("output bound"));
    assert_eq!(page(&[1], 2, 1), Err("output bound"));
}

#[test]
fn retained_state_bounds_refuse_without_truncating_the_mathematical_answer() {
    let mut distinct = Distinct::default();
    assert_eq!(distinct.advance(1, false, 1), Ok(Progress::Incomplete));
    let before = distinct.clone();
    assert_eq!(distinct.advance(1, false, 1), Err("distinct capacity"));
    assert_eq!(distinct, before);
    // Cardinality applies after projection: the full tuple needs three keys, category only two.
    assert!(matches!(
        Distinct::default().advance(4, false, 2),
        Ok(Progress::Complete(_))
    ));
    let mut tuples = Distinct::default();
    assert_eq!(tuples.advance(4, true, 2), Err("distinct capacity"));
    assert_eq!(tuples, Distinct::default());

    let mut grouped = Grouped::default();
    assert_eq!(grouped.advance(1, 0, 1), Ok(Progress::Incomplete));
    let before = grouped.clone();
    assert_eq!(grouped.advance(1, 0, 1), Err("group capacity"));
    assert_eq!(grouped, before);
    // A work limit of zero would make no progress; it is refused, never reported as completion.
    assert_eq!(grouped.advance(0, 0, 1), Err("input bound"));
    assert_eq!(distinct.advance(0, false, 1), Err("input bound"));
}
