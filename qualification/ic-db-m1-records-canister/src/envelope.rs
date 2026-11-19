//! Maximum structural write shape using ordinary product APIs and the real supplier.
//! Raw inspection is private qualification transport, never a product query lane.
use super::{Batch, Entity, KEY, open, runtime};
use ic_db::Error;
use ic_db_m1_records_schema::EnvelopeRow;
use ic_storage::{StableKey, adapter::StableBTreeMap};
use std::ops::Bound;

pub(super) fn write(
    mode: u8,
    count: u8,
    generation: u8,
    oversized: bool,
    text_len: usize,
) -> Result<(), Error> {
    assert!(count <= 33 && generation <= 1);
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for id in 1..=u64::from(count) {
        if mode == 2 {
            batch.delete::<EnvelopeRow>(id)?;
            continue;
        }
        // Maximal probe: 512-byte keys and unindexed padding to a 4096-byte row.
        let text = "\0".repeat(text_len);
        let mut a = if text_len == 60 {
            format!("abcdefg{text}")
        } else {
            text.clone()
        };
        if generation == 1 {
            let offset = if text_len == 60 { 7 } else { 0 };
            a.replace_range(offset..offset + 1, "\u{1}");
        }
        if oversized {
            a.push('\0');
        }
        let row = EnvelopeRow {
            id,
            a,
            b: text.clone(),
            c: text.clone(),
            d: text,
            padding: if text_len == 60 {
                vec![0; 3821]
            } else {
                Vec::new()
            },
        };
        match mode {
            0 => batch.insert(&row)?,
            1 => batch.replace(&row)?,
            _ => panic!("invalid fixture mode"),
        }
    }
    db.commit(batch)
}

/// Measure a bounded batch at the selected record and index sizes.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "envelope_write",
    guard = crate::transport::admit::<88>,
    decode = crate::transport::decode_measured::<_, 88>,
    encode = crate::transport::encode::<_, 88>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn envelope_write(
    ((mode, count, generation, oversized, trap), counter): crate::transport::Measured<(
        u8,
        u8,
        u8,
        bool,
        bool,
    )>,
) -> (Result<(), u8>, u64) {
    // Includes reopen, row construction, batch preparation and commit. Transport
    // decode/encode is outside this interval; the driver measures whole-call cycles.
    let start = counter();
    let result = match write(mode, count, generation, oversized, 60) {
        Ok(()) => Ok(()),
        Err(Error::Limit) => Err(1),
        Err(Error::Exists) => Err(2),
        Err(error) => panic!("unexpected envelope refusal: {error:?}"),
    };
    if trap {
        crate::system::trap("after envelope commit");
    }
    (result, counter() - start)
}

/// Inspect the selected index and generation after an envelope write.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "envelope_read",
    guard = crate::transport::admit::<89>,
    decode = crate::transport::decode_measured::<_, 89>,
    encode = crate::transport::encode::<_, 89>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn envelope_read(
    ((index, generation), counter): crate::transport::Measured<(u8, u8)>,
) -> (Vec<u64>, u64, u64) {
    assert!(index < 8 && generation <= 1);
    let start = counter();
    let host = runtime();
    let db = open(&host);
    let index = &EnvelopeRow::SCHEMA.indexes[usize::from(index)];
    let mut leading = "\0".repeat(60);
    if index.fields[0] == "a" {
        leading.insert_str(0, "abcdefg");
        if generation == 1 {
            leading.replace_range(7..8, "\u{1}");
        }
    }
    let rows = db
        .index_prefix::<EnvelopeRow>(index.name, &[leading.as_str().into()], 32)
        .unwrap();
    let ids = rows.into_iter().map(|row| row.id).collect();
    let revision = db.revision::<EnvelopeRow>().unwrap();
    (ids, revision, counter() - start)
}

/// Read the fixture’s four text fields by row ID.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "envelope_row",
    guard = crate::transport::admit::<91>,
    decode = crate::transport::decode::<_, 91>,
    encode = crate::transport::encode::<_, 91>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn envelope_row(id: u64) -> (String, String, String, String) {
    let host = runtime();
    let db = open(&host);
    let row = db.get::<EnvelopeRow>(id).unwrap().unwrap();
    assert!(row.padding.is_empty());
    (row.a, row.b, row.c, row.d)
}

/// Measure the matched write workload at the requested text length.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "envelope_comparison_write",
    guard = crate::transport::admit::<93>,
    decode = crate::transport::decode_measured::<_, 93>,
    encode = crate::transport::encode::<_, 93>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn envelope_comparison_write(
    ((mode, generation, text_len), counter): crate::transport::Measured<(u8, u8, u16)>,
) -> (Result<(), u16>, u64) {
    assert!(mode <= 1 && matches!(text_len, 48 | 61 | 1018));
    let start = counter();
    let result = match write(mode, 32, generation, false, usize::from(text_len)) {
        Ok(()) => Ok(()),
        Err(Error::Limit) => Err(1),
        Err(error) => panic!("unexpected comparison refusal: {error:?}"),
    };
    (result, counter() - start)
}

/// List matching fixture IDs for the requested generation.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "envelope_listing",
    guard = crate::transport::admit::<92>,
    decode = crate::transport::decode::<_, 92>,
    encode = crate::transport::encode::<_, 92>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn envelope_listing(generation: u8) -> Vec<u64> {
    assert!(generation <= 1);
    let mut text = "\0".repeat(48);
    if generation == 1 {
        text.replace_range(..1, "\u{1}");
    }
    let host = runtime();
    let db = open(&host);
    db.index_prefix::<EnvelopeRow>(
        EnvelopeRow::SCHEMA.indexes[0].name,
        &[text.as_str().into()],
        32,
    )
    .unwrap()
    .into_iter()
    .map(|row| row.id)
    .collect()
}

type RawPage = Vec<(Vec<u8>, Vec<u8>)>;

/// Page physical entries for the independent write-envelope oracle.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "envelope_raw",
    guard = crate::transport::admit::<90>,
    decode = crate::transport::decode::<_, 90>,
    encode = crate::transport::encode::<_, 90>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn envelope_raw((tag, after): (u8, Option<Vec<u8>>)) -> RawPage {
    assert!(matches!(tag, 2 | 3));
    let host = runtime();
    let raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(
        host.open_memory(&StableKey::parse(KEY).unwrap()).unwrap(),
    );
    let mut prefix = vec![tag];
    prefix.extend(EnvelopeRow::SCHEMA.id.to_be_bytes());
    let lower = match after {
        Some(key) => {
            assert!(key.len() <= 512 && key.starts_with(&prefix));
            Bound::Excluded(key)
        }
        None => Bound::Included(prefix.clone()),
    };
    // Two maximal rows fit the transport bound. Continue from the last key.
    raw.range((lower, Bound::Unbounded))
        .take_while(|entry| entry.key().starts_with(&prefix))
        .take(2)
        .map(|entry| (entry.key().clone(), entry.value()))
        .collect()
}
