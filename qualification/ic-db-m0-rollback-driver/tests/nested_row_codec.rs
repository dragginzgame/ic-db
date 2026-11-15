//! One fixed recursive row witnesses the compiled storage traversal contract.
//! No schema interpreter, generated codec or general container engine lives here.

use ic_wire::{Encoding, Limits};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const LIMITS: Limits = Limits {
    bytes: 512,
    // Wire charges conservative traversal allowances, not actual heap bytes.
    // Separate from eight semantic nodes and the 512-byte encoded-row bound.
    allocation: 64 * 1024,
    nodes: 512,
    depth: 32,
};

#[derive(Clone, Debug, PartialEq)]
enum State {
    Weighted(u64),
    Active,
}

// Application declaration order deliberately differs from stored field order.
#[derive(Clone, Debug, PartialEq)]
struct Node {
    state: State,
    label: String,
    children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
struct Row {
    tags: BTreeSet<String>,
    root: Node,
    note: Option<String>,
    id: u64,
    counters: BTreeMap<String, u64>,
}

// Compiled name order: children, label, state. A recursive type graph is finite;
// each value still has separate depth, node, child-count and text-byte bounds.
#[derive(Clone, Deserialize, Serialize)]
struct StoredNode {
    children: Vec<StoredNode>,
    label: String,
    // Name order Active=0, Weighted=1, independent of the Rust enum order above.
    // An actual generated codec writes the selected variant's payload directly.
    state: (u8, Option<u64>),
}

// counters, id, note, root, tags. Decode collections into sequences first:
// deserializing directly into maps/sets could discard duplicate evidence.
type StoredRow = (
    Vec<(String, u64)>,
    u64,
    Option<String>,
    StoredNode,
    Vec<String>,
);

#[derive(Debug, PartialEq)]
enum Refusal {
    Codec,
    Bound,
    CollectionOrder,
    Variant,
}

fn encode_node(node: &Node, depth: usize, remaining: &mut usize) -> Result<StoredNode, Refusal> {
    if depth > 3 || *remaining == 0 || node.children.len() > 3 || node.label.len() > 8 {
        return Err(Refusal::Bound);
    }
    *remaining -= 1;
    Ok(StoredNode {
        children: node
            .children
            .iter()
            .map(|child| encode_node(child, depth + 1, remaining))
            .collect::<Result<_, _>>()?,
        label: node.label.clone(),
        state: match node.state {
            State::Active => (0, None),
            State::Weighted(value) => (1, Some(value)),
        },
    })
}

fn decode_node(node: StoredNode, depth: usize, remaining: &mut usize) -> Result<Node, Refusal> {
    if depth > 3 || *remaining == 0 || node.children.len() > 3 || node.label.len() > 8 {
        return Err(Refusal::Bound);
    }
    *remaining -= 1;
    let state = match node.state {
        (0, None) => State::Active,
        (1, Some(value)) => State::Weighted(value),
        _ => return Err(Refusal::Variant),
    };
    Ok(Node {
        children: node
            .children
            .into_iter()
            .map(|child| decode_node(child, depth + 1, remaining))
            .collect::<Result<_, _>>()?,
        label: node.label,
        state,
    })
}

fn row_bounds(
    counters: &[(String, u64)],
    note: Option<&str>,
    tags: &[String],
) -> Result<(), Refusal> {
    if counters.len() > 3
        || tags.len() > 3
        || counters.iter().any(|(key, _)| key.len() > 8)
        || tags.iter().any(|tag| tag.len() > 8)
        || note.is_some_and(|note| note.len() > 8)
    {
        return Err(Refusal::Bound);
    }
    if counters.windows(2).any(|pair| pair[0].0 >= pair[1].0)
        || tags.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(Refusal::CollectionOrder);
    }
    Ok(())
}

fn encode(row: &Row) -> Result<Vec<u8>, Refusal> {
    // Bound typed collections before materializing the storage projection.
    if row.counters.len() > 3
        || row.tags.len() > 3
        || row
            .counters
            .keys()
            .chain(row.tags.iter())
            .chain(row.note.iter())
            .any(|text| text.len() > 8)
    {
        return Err(Refusal::Bound);
    }
    let counters = row
        .counters
        .iter()
        .map(|(key, value)| (key.clone(), *value))
        .collect::<Vec<_>>();
    let tags = row.tags.iter().cloned().collect::<Vec<_>>();
    row_bounds(&counters, row.note.as_deref(), &tags)?;
    let root = encode_node(&row.root, 1, &mut 8)?;
    ic_wire::encode(
        &(counters, row.id, row.note.clone(), root, tags),
        Encoding::Payload,
        LIMITS,
    )
    .map_err(|_| Refusal::Codec)
}

fn decode(bytes: &[u8], limits: Limits) -> Result<Row, Refusal> {
    let (counters, id, note, root, tags): StoredRow =
        ic_wire::decode(bytes, Encoding::Payload, limits).map_err(|_| Refusal::Codec)?;
    row_bounds(&counters, note.as_deref(), &tags)?;
    Ok(Row {
        id,
        note,
        root: decode_node(root, 1, &mut 8)?,
        counters: counters.into_iter().collect(),
        tags: tags.into_iter().collect(),
    })
}

fn leaf(label: &str) -> Node {
    Node {
        state: State::Active,
        label: label.into(),
        children: Vec::new(),
    }
}

fn sample() -> Row {
    Row {
        id: 7,
        root: leaf("oak"),
        note: None,
        counters: BTreeMap::from([("x".into(), 2)]),
        tags: BTreeSet::from(["b".into(), "a".into()]),
    }
}

fn maximize_node(node: &mut Node) {
    node.label = "éééé".into();
    node.state = State::Weighted(u64::MAX);
    for child in &mut node.children {
        maximize_node(child);
    }
}

#[test]
fn storage_order_and_enum_tag_match_literal_bytes() {
    let row = sample();
    // Literal Wire payload: one map entry, id, absent note, leaf, two sorted tags.
    let expected = b"\x01\x01x\x02\x07\x00\x00\x03oak\x00\x00\x02\x01a\x01b";
    assert_eq!(encode(&row).unwrap(), expected);
    assert_eq!(decode(expected, LIMITS).unwrap(), row);
    let mut weighted = row;
    weighted.root.state = State::Weighted(u64::MAX);
    assert_eq!(
        decode(&encode(&weighted).unwrap(), LIMITS).unwrap(),
        weighted
    );
}

#[test]
fn collections_preserve_their_declared_equality() {
    let mut first = sample();
    first.counters = [("z".into(), 9), ("a".into(), 1)].into_iter().collect();
    let mut second = first.clone();
    second.counters = [("a".into(), 1), ("z".into(), 9)].into_iter().collect();
    second.tags = ["a".into(), "b".into()].into_iter().collect();
    assert_eq!(encode(&first).unwrap(), encode(&second).unwrap());
    first.root.children = vec![leaf("a"), leaf("b")];
    second.root.children = vec![leaf("b"), leaf("a")];
    assert_ne!(encode(&first).unwrap(), encode(&second).unwrap());
    second = first.clone();
    second.note = Some(String::new());
    assert_ne!(encode(&first).unwrap(), encode(&second).unwrap());
}

#[test]
fn recursive_values_obey_database_bounds_in_both_directions() {
    let mut row = sample();
    row.root.children = vec![leaf("éééé"), leaf("b"), leaf("c")];
    row.root.children[0].children = vec![leaf("d"), leaf("e")];
    row.root.children[1].children = vec![leaf("f"), leaf("g")];
    maximize_node(&mut row.root);
    row.id = u64::MAX;
    row.note = Some("éééé".into());
    row.tags = ["aaaaaaaa", "bbbbbbbb", "cccccccc"]
        .map(String::from)
        .into();
    row.counters = ["aaaaaaaa", "bbbbbbbb", "cccccccc"]
        .map(|key| (key.into(), u64::MAX))
        .into();
    assert_eq!(decode(&encode(&row).unwrap(), LIMITS).unwrap(), row); // Eight nodes.
    let valid = row.clone();
    row.root.children[2].children.push(leaf("ninth"));
    assert_eq!(encode(&row), Err(Refusal::Bound));
    row = valid.clone();
    row.root.children[0].label.push('x');
    assert_eq!(encode(&row), Err(Refusal::Bound));
    row = sample();
    row.root.children = vec![leaf("one"); 4];
    assert_eq!(encode(&row), Err(Refusal::Bound));
    row.root = leaf("a");
    for _ in 0..3 {
        row.root = Node {
            state: State::Active,
            label: "a".into(),
            children: vec![row.root],
        };
    }
    assert_eq!(encode(&row), Err(Refusal::Bound));
    let mut stored: StoredRow =
        ic_wire::decode(&encode(&valid).unwrap(), Encoding::Payload, LIMITS).unwrap();
    stored.3.children[0].children[0].children.push(StoredNode {
        children: vec![],
        label: "too deep".into(),
        state: (0, None),
    });
    assert_eq!(
        decode(
            &ic_wire::encode(&stored, Encoding::Payload, LIMITS).unwrap(),
            LIMITS
        ),
        Err(Refusal::Bound)
    );
}

#[test]
fn malformed_collections_and_variants_are_not_silently_repaired() {
    let stored: StoredRow =
        ic_wire::decode(&encode(&sample()).unwrap(), Encoding::Payload, LIMITS).unwrap();
    for keys in [vec!["x", "x"], vec!["z", "a"]] {
        let mut bad = stored.clone();
        bad.0 = keys.iter().map(|key| ((*key).into(), 1)).collect();
        assert_eq!(
            decode(
                &ic_wire::encode(&bad, Encoding::Payload, LIMITS).unwrap(),
                LIMITS
            ),
            Err(Refusal::CollectionOrder)
        );
        bad = stored.clone();
        bad.4 = keys.into_iter().map(String::from).collect();
        assert_eq!(
            decode(
                &ic_wire::encode(&bad, Encoding::Payload, LIMITS).unwrap(),
                LIMITS
            ),
            Err(Refusal::CollectionOrder)
        );
    }
    for variant in [(2, None), (0, Some(7)), (1, None)] {
        let mut bad = stored.clone();
        bad.3.state = variant;
        assert_eq!(
            decode(
                &ic_wire::encode(&bad, Encoding::Payload, LIMITS).unwrap(),
                LIMITS
            ),
            Err(Refusal::Variant)
        );
    }
    let valid = encode(&sample()).unwrap();
    for limits in [
        Limits { depth: 1, ..LIMITS },
        Limits { nodes: 1, ..LIMITS },
        Limits {
            allocation: 0,
            ..LIMITS
        },
        Limits { bytes: 1, ..LIMITS },
    ] {
        assert_eq!(decode(&valid, limits), Err(Refusal::Codec));
    }
}
