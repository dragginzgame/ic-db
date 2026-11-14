//! Fixed User/Mapping witness using the existing three maps in a fresh fixture.
//! Rows hold user keys 1..16 and mapping keys 101..116. Mapping values encode
//! user * 1000 + principal; reverse-index keys encode user * 100 + mapping ID.
//! Principal zero stands for absence, excluded by this fixture's explicit
//! present-only unique-index membership. References still include absent values.

use super::{NAMES, open, runtime};
use ic_platform_runtime::cdk;
use std::collections::{BTreeMap, BTreeSet};

const MAX_COMMANDS: usize = 8;
const MAPPING: u64 = 100;
type Command = (u8, u64, u64, u64);

fn fail_at(selected: u8, boundary: u8) {
    if selected == boundary {
        cdk::api::trap("M0 injected batch failure");
    }
}

// Kinds: insert user, delete user, put mapping, delete mapping.
// Errors: bounds=1, duplicate target=2, existence=3, uniqueness=4, reference=5.
// Success reports examined reverse-index candidates, independently of row count.
#[ic_platform_runtime::cdk::update(
    guard = "crate::transport::admit::<3>",
    decode_with = "crate::transport::decode::<_, 3>",
    encode_with = "crate::transport::encode::<_, 3>"
)]
fn batch((commands, failure): (Vec<Command>, u8)) -> Result<u64, u8> {
    if commands.is_empty() || commands.len() > MAX_COMMANDS || failure > 5 {
        return Err(1);
    }
    let mut final_rows = BTreeMap::new();
    for &(kind, id, user, principal) in &commands {
        if kind > 3 || !(1..=16).contains(&id) {
            return Err(1);
        }
        if kind == 2 {
            if !(1..=16).contains(&user) || principal > 100 {
                return Err(1);
            }
        } else if user != 0 || principal != 0 {
            return Err(1);
        }
        let key = if kind < 2 { id } else { MAPPING + id };
        let value = match kind {
            0 => Some(0),
            2 => Some(user * 1000 + principal),
            _ => None,
        };
        if final_rows.insert(key, value).is_some() {
            return Err(2);
        }
    }
    let host = runtime();
    let mut rows = open(&host, NAMES[0]);
    let mut unique = open(&host, NAMES[1]);
    let mut reverse = open(&host, NAMES[2]);
    for &(kind, id, _, _) in &commands {
        let key = if kind < 2 { id } else { MAPPING + id };
        let exists = rows.contains_key(&key);
        if (kind == 0 && exists) || (matches!(kind, 1 | 3) && !exists) {
            return Err(3);
        }
    }
    let final_value = |key: u64| {
        final_rows
            .get(&key)
            .copied()
            .unwrap_or_else(|| rows.get(&key))
    };
    let mut claimed = BTreeSet::new();
    for (&key, &value) in &final_rows {
        if key <= MAPPING {
            continue;
        }
        if let Some(value) = value {
            let user = value / 1000;
            let principal = value % 1000;
            if final_value(user).is_none() {
                return Err(5);
            }
            if principal == 0 {
                continue;
            }
            if !claimed.insert(principal) {
                return Err(4);
            }
            if let Some(owner) = unique.get(&principal)
                && owner != key - MAPPING
                && final_value(MAPPING + owner).is_some_and(|v| v % 1000 == principal)
            {
                return Err(4);
            }
        }
    }
    let mut examined = 0;
    for (&user, value) in &final_rows {
        if user > MAPPING || value.is_some() {
            continue;
        }
        // Each skipped old reference needs a distinct command in this batch. One
        // extra candidate is enough to detect a surviving reference; never scan all mappings.
        for entry in reverse
            .range(user * 100..(user + 1) * 100)
            .take(MAX_COMMANDS + 1)
        {
            examined += 1;
            let mapping_id = *entry.key() % 100;
            if final_value(MAPPING + mapping_id).is_some_and(|v| v / 1000 == user) {
                return Err(5);
            }
        }
    }
    // Snapshot only touched old mappings. No persistent overlay or public prepared handle.
    let old: Vec<_> = final_rows
        .keys()
        .filter(|key| **key > MAPPING)
        .filter_map(|key| rows.get(key).map(|value| (*key - MAPPING, value)))
        .collect();
    for &(id, value) in &old {
        if value % 1000 != 0 {
            unique.remove(&(value % 1000));
        }
        fail_at(failure, 1);
        reverse.remove(&(value / 1000 * 100 + id));
        fail_at(failure, 2);
    }
    for (&key, &value) in &final_rows {
        match value {
            Some(value) => {
                rows.insert(key, value);
            }
            None => {
                rows.remove(&key);
            }
        }
        fail_at(failure, 3);
    }
    for (&key, &value) in &final_rows {
        if key <= MAPPING {
            continue;
        }
        if let Some(value) = value {
            let id = key - MAPPING;
            if value % 1000 != 0 {
                unique.insert(value % 1000, id);
            }
            fail_at(failure, 4);
            reverse.insert(value / 1000 * 100 + id, value % 1000);
            fail_at(failure, 5);
        }
    }
    Ok(examined)
}
