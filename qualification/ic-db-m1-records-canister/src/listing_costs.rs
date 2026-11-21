//! Private PocketIC measurements for the document listing and its write costs.

use crate::{
    open,
    queries::{IdOnly, decode_cursor},
    runtime,
    schema::Document,
};

// measured_listing
// Measure hinted, automatic and complete-sort document listings.
// guard  : validates the request frame for method 189.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "measured_listing",
    guard = crate::transport::admit::<189>,
    decode = crate::transport::decode_measured::<_, 189>,
    encode = crate::transport::encode::<_, 189>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn measured_listing(
    ((shape, after, work, bytes, max_id), counter): crate::transport::Measured<ListingRequest>,
) -> Result<ListingSample, String> {
    let start = counter();
    let host = runtime();
    let db = open(&host);
    let cursor = decode_cursor(after.as_deref()).unwrap();
    let field = match shape {
        0 | 1 | 4 => Document::SCORE,
        2 => Document::PREVIOUS_SCORE,
        _ => return Err("unknown listing shape".into()),
    };
    let mut q = db
        .query::<Document>()
        .eq(Document::ENTRY_ID, 1_u64)
        .order_by(&[field, Document::ID])
        .descending()
        .limit(20)
        .work_limit(work as usize)
        .test_output_limit(bytes as usize);
    if shape != 4 {
        q = q.eq(Document::PUBLISHED, true);
    }
    if let Some(max_id) = max_id {
        q = q.lte(Document::ID, max_id);
    }
    if shape == 1 {
        q = q.using_index("entry_id,score", &[1_u64.into()]);
    }
    if let Some(cursor) = &cursor {
        q = q.after(cursor);
    }
    let page = q.fetch_as::<IdOnly>().map_err(|e| format!("{e:?}"))?;

    Ok((
        page.rows.into_iter().map(|r| r.id).collect(),
        page.next.map(|c| c.to_bytes()),
        page.examined as u32,
        page.fetched as u32,
        counter() - start,
    ))
}

// measured_listing_count
// Time the exact filtered listing total from open through ordinary count.
// guard  : validates the request frame for method 192.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "measured_listing_count",
    guard = crate::transport::admit::<192>,
    decode = crate::transport::decode_measured::<_, 192>,
    encode = crate::transport::encode::<_, 192>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn measured_listing_count(
    (work, counter): crate::transport::Measured<u32>,
) -> (Result<(u64, u32, u32), String>, u64) {
    let start = counter();
    let host = runtime();
    let db = open(&host);
    let result = db
        .query::<Document>()
        .eq(Document::ENTRY_ID, 1_u64)
        .eq(Document::PUBLISHED, true)
        .work_limit(work as usize)
        .count()
        .map(|a| (a.value, a.examined as u32, a.fetched as u32))
        .map_err(|e| format!("{e:?}"));

    (result, counter() - start)
}

// measured_listing_write
// Time one document write including database open and any caller-side read.
// guard  : validates the request frame for method 190.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "measured_listing_write",
    guard = crate::transport::admit::<190>,
    decode = crate::transport::decode_measured::<_, 190>,
    encode = crate::transport::encode::<_, 190>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn measured_listing_write(
    (operation, counter): crate::transport::Measured<u8>,
) -> (u64, Option<(bool, i64, Option<String>)>) {
    let start = counter();
    let host = runtime();
    let mut db = open(&host);
    let mut batch = ic_db::Batch::new();
    let id = 131;
    match operation {
        0 => batch
            .insert(&Document {
                id,
                entry_id: 1,
                title: "probe".into(),
                note: None,
                published: true,
                score: 20,
                previous_score: None,
                priority: None,
            })
            .unwrap(),
        1..=3 => {
            let mut row = db.get::<Document>(id).unwrap().unwrap();
            match operation {
                1 => row.note = Some("changed".into()),
                2 => row.score = 21,
                _ => row.published = false,
            }
            batch.replace(&row).unwrap()
        }
        4 => batch.delete::<Document>(id).unwrap(),
        _ => panic!("unknown listing write"),
    }
    db.commit(batch).unwrap();
    let instructions = counter() - start;
    let saved = db
        .get::<Document>(id)
        .unwrap()
        .map(|row| (row.published, row.score, row.note));

    (instructions, saved)
}

type ListingRequest = (u8, Option<Vec<u8>>, u32, u32, Option<u64>);

type ListingSample = (Vec<u64>, Option<Vec<u8>>, u32, u32, u64);
