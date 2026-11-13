//! Eight-row ordered-read witness. Fresh installations only; not a product query engine.
//! Rows encode score * 65536 + price; score zero is the optional value None.

use super::{NAMES, open, runtime};
use ic_platform_runtime::cdk;
use ic_wire::{Limits, native};
use std::ops::Bound::{Excluded, Unbounded};

type Cursor = (Vec<u8>, [u64; 5]); // Canister, incarnation, contract, mode, predicate, position.
type Row = (u64, Option<u64>, u64);
type Page<T> = (Vec<T>, Option<Cursor>, u64, u64); // Results, continuation, examined, fetched.
const MAX_PRICE: u64 = 20_000;
const LIMITS: Limits = Limits {
    bytes: 4096,
    allocation: 65_536,
    nodes: 1024,
    depth: 32,
};

fn cursor(incarnation: u64, mode: u64, price: u64, position: u64) -> Cursor {
    (
        cdk::api::canister_self().as_slice().to_vec(),
        [incarnation, 1, mode, price, position],
    )
}

fn position(token: Option<&Cursor>, incarnation: u64, mode: u64, price: u64) -> Result<u64, u8> {
    let Some((canister, fields)) = token else {
        return Ok(0);
    };
    if canister != cdk::api::canister_self().as_slice()
        || fields[..4] != [incarnation, 1, mode, price]
    {
        return Err(2);
    }
    let key = fields[4];
    if (mode == 0 && !(1..=8).contains(&key))
        || (mode != 0 && (key > 10008 || !(1..=8).contains(&(key % 100))))
    {
        return Err(2);
    }
    Ok(key)
}

fn optional(score: u64) -> Option<u64> {
    (score != 0).then_some(score)
}

#[ic_wire_endpoint::update(id = 4)]
fn seed_reads() -> u64 {
    let host = runtime();
    let mut rows = open(&host, NAMES[0]);
    let mut metadata = open(&host, NAMES[1]);
    let mut index = open(&host, NAMES[2]);
    let incarnation = metadata.get(&0).unwrap_or(0).checked_add(1).unwrap();
    let old: Vec<_> = rows
        .iter()
        .take(8)
        .map(|entry| (*entry.key(), entry.value()))
        .collect();
    for (id, value) in old {
        rows.remove(&id);
        index.remove(&(value / 65536 * 100 + id));
    }
    assert!(
        rows.is_empty() && index.is_empty(),
        "do not mix probe layouts"
    );
    for (id, score, price) in [
        (1, 30, 20000),
        (2, 10, 90),
        (3, 30, 10),
        (4, 0, 90),
        (5, 10, 10),
        (6, 40, 10),
        (7, 0, 90),
        (8, 20, 10),
    ] {
        rows.insert(id, score * 65536 + price);
        index.insert(score * 100 + id, id);
    }
    metadata.insert(0, incarnation);
    incarnation
}

#[ic_wire_endpoint::update(id = 5)]
fn change_read_row((id, replacement): (u64, Option<(u64, u64)>)) -> Result<(), u8> {
    if !(1..=8).contains(&id)
        || replacement.is_some_and(|(score, price)| score > 100 || price > MAX_PRICE)
    {
        return Err(1);
    }
    let host = runtime();
    let mut rows = open(&host, NAMES[0]);
    let mut index = open(&host, NAMES[2]);
    if let Some(old) = rows.remove(&id) {
        index.remove(&(old / 65536 * 100 + id));
    }
    if let Some((score, price)) = replacement {
        rows.insert(id, score * 65536 + price);
        index.insert(score * 100 + id, id);
    }
    Ok(())
}

// Errors: bounds=1, cursor mismatch=2, oversized row=3, unsupported order/path=4.
#[ic_wire_endpoint::query(id = 6)]
fn bounded_page(
    (order, access, maximum_price, token, work, limit, bytes): (
        u64,
        u64,
        u64,
        Option<Cursor>,
        u64,
        u64,
        u64,
    ),
) -> Result<Page<Row>, u8> {
    if order > 1
        || access > 1
        || maximum_price > MAX_PRICE
        || !(1..=8).contains(&work)
        || !(1..=8).contains(&limit)
        || bytes > 1024
    {
        return Err(1);
    }
    if access != order {
        return Err(4);
    }
    let host = runtime();
    let rows = open(&host, NAMES[0]);
    let incarnation = open(&host, NAMES[1]).get(&0).ok_or(2)?;
    let index = open(&host, NAMES[2]);
    let mut last = position(token.as_ref(), incarnation, order, maximum_price)?;
    let make_cursor = |key| cursor(incarnation, order, maximum_price, key);
    // Reserve a complete progress envelope, including maximal counters/position.
    let reserve: Page<Row> = (vec![], Some(make_cursor(10008)), 8, 8);
    if native::encode_response(6, &Ok::<_, u8>(reserve), LIMITS)
        .unwrap()
        .len() as u64
        > bytes
    {
        return Err(1);
    }
    let mut traversal = if order == 0 {
        rows.range((Excluded(last), Unbounded))
    } else {
        index.range((Excluded(last), Unbounded))
    };
    let mut output = Vec::new();
    let mut examined = 0;
    let mut exhausted = false;
    while examined < work && output.len() < limit as usize {
        let Some(entry) = traversal.next() else {
            exhausted = true;
            break;
        };
        examined += 1;
        let key = *entry.key();
        let (id, value) = if order == 0 {
            (key, entry.value())
        } else {
            (entry.value(), rows.get(&entry.value()).unwrap())
        };
        let price = value % 65536;
        if price > maximum_price {
            last = key;
            continue;
        }
        let row = (id, optional(value / 65536), price);
        let mut proposed = output.clone();
        proposed.push(row);
        let page = (proposed, Some(make_cursor(key)), examined, examined);
        if native::encode_response(6, &Ok::<_, u8>(&page), LIMITS)
            .unwrap()
            .len() as u64
            > bytes
        {
            let single = (vec![row], Some(make_cursor(key)), examined, examined);
            if native::encode_response(6, &Ok::<_, u8>(single), LIMITS)
                .unwrap()
                .len() as u64
                > bytes
            {
                return Err(3);
            }
            // This row fits a fresh response: leave progress before it, so it is retried.
            break;
        }
        output.push(row);
        last = key;
    }
    Ok((
        output,
        (!exhausted).then(|| make_cursor(last)),
        examined,
        examined,
    ))
}

// Only the unfiltered ordered-key DISTINCT witness is admitted here.
#[ic_wire_endpoint::query(id = 7)]
fn distinct_page(
    (token, work, limit): (Option<Cursor>, u64, u64),
) -> Result<Page<Option<u64>>, u8> {
    if !(1..=8).contains(&work) || !(1..=8).contains(&limit) {
        return Err(1);
    }
    let host = runtime();
    let incarnation = open(&host, NAMES[1]).get(&0).ok_or(2)?;
    let index = open(&host, NAMES[2]);
    let mut last = position(token.as_ref(), incarnation, 2, MAX_PRICE)?;
    let mut previous = token.as_ref().map(|_| last / 100);
    let mut traversal = index.range((Excluded(last), Unbounded));
    let mut output = Vec::new();
    let mut examined = 0;
    let mut exhausted = false;
    while examined < work && output.len() < limit as usize {
        let Some(entry) = traversal.next() else {
            exhausted = true;
            break;
        };
        examined += 1;
        last = *entry.key();
        let score = last / 100;
        if previous != Some(score) {
            output.push(optional(score));
        }
        previous = Some(score);
    }
    Ok((
        output,
        (!exhausted).then(|| cursor(incarnation, 2, MAX_PRICE, last)),
        examined,
        0,
    ))
}
