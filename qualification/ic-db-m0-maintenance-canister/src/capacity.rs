//! Reuse only the fully retired source slot after the fixed migration succeeds.
//! This candidate is never published: the existing v2 rows/index remain live.

use super::{FORMAT, KEYS, Map, PHASE, RETIRED, SCHEMA, fail_at, field, map, runtime, target};
use ic_storage::{DefaultMemoryImpl, StableKey, adapter::StableBTreeMap};

type Candidate = StableBTreeMap<u64, Vec<u8>, DefaultMemoryImpl>;
// Separate fixed probe state: absent/idle, copying, complete. No generic job state.
const STATE: u64 = 6;
const POSITION: u64 = 7;
const WIDTH: u64 = 8;
type Inspection = ([u64; 5], Vec<u8>, Vec<(u64, u64)>);

fn eligible(control: &Map) -> Result<(), u8> {
    if SCHEMA != 2 || field(control, FORMAT)? != 2 || field(control, PHASE)? != 0 {
        return Err(3);
    }
    if field(control, RETIRED)? != 0 {
        return Err(3);
    }
    Ok(())
}

// Start/reset or copy at most two rows. Width is an explicit fixed probe input.
// Errors reuse the fixture's closed-phase=3 and invalid-bounds=5 codes.
#[ic_platform_runtime::cdk::update(
    guard = "crate::transport::admit::<5>",
    decode_with = "crate::transport::decode::<_, 5>",
    encode_with = "crate::transport::encode::<_, 5>"
)]
fn capacity_step((start, width, failure): (bool, u64, u8)) -> Result<(), u8> {
    if failure > 3 || (start && !matches!(width, 8192 | 98_304)) || (!start && width != 0) {
        return Err(5);
    }
    let host = runtime();
    let mut control = map(&host, 0);
    eligible(&control)?;
    let state = control.get(&STATE).unwrap_or(0);
    if start {
        if !matches!(state, 0 | 2) || (state == 0 && !map(&host, 1).is_empty()) {
            return Err(3);
        }
        // Only this recorded retired slot is disposable. Release custody before reset.
        drop(
            Candidate::reset(
                host.open_memory(&StableKey::parse(KEYS[1]).unwrap())
                    .unwrap(),
            )
            .unwrap(),
        );
        fail_at(failure, 1);
        control.insert(STATE, 1);
        control.insert(POSITION, 0);
        control.insert(WIDTH, width);
        return Ok(());
    }
    if state != 1 {
        return Err(3);
    }
    let mut candidate = Candidate::load(
        host.open_memory(&StableKey::parse(KEYS[1]).unwrap())
            .unwrap(),
    )
    .unwrap();
    let source = target(&host);
    let width = field(&control, WIDTH)? as usize;
    let position = field(&control, POSITION)?;
    for id in (position + 1)..=(position + 2).min(6) {
        let source_bytes = source.get(&id).unwrap();
        let mut bytes = vec![id as u8; width];
        bytes[..16].copy_from_slice(&source_bytes);
        candidate.insert(id, bytes);
        fail_at(failure, 2);
        control.insert(POSITION, id);
        fail_at(failure, 3);
    }
    if field(&control, POSITION)? == 6 {
        control.insert(STATE, 2);
    }
    Ok(())
}

// Full row bytes are independently checked in <=8 KiB pieces by the driver.
#[ic_platform_runtime::cdk::query(
    guard = "crate::transport::admit::<6>",
    decode_with = "crate::transport::decode::<_, 6>",
    encode_with = "crate::transport::encode::<_, 6>"
)]
fn capacity_inspect((id, offset): (u64, u64)) -> Result<Inspection, u8> {
    if id > 6 || offset > 98_304 || (id == 0 && offset != 0) {
        return Err(5);
    }
    let host = runtime();
    let control = map(&host, 0);
    eligible(&control)?;
    let state = field(&control, STATE)?;
    let candidate = Candidate::load(
        host.open_memory(&StableKey::parse(KEYS[1]).unwrap())
            .unwrap(),
    )
    .unwrap();
    let bytes = if id == 0 {
        Vec::new()
    } else {
        let bytes = candidate.get(&id).ok_or(5)?;
        if offset as usize > bytes.len() {
            return Err(5);
        }
        bytes[offset as usize..bytes.len().min(offset as usize + 8192)].to_vec()
    };
    let count = candidate.len();
    let (_, pages) = candidate.allocated_pages();
    Ok((
        [
            host.diagnostics().unwrap().physical_pages,
            pages,
            count,
            field(&control, POSITION)?,
            state,
        ],
        bytes,
        map(&host, 3)
            .iter()
            .map(|entry| (*entry.key(), entry.value()))
            .collect(),
    ))
}
