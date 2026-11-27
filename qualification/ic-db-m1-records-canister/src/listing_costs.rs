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
        0 | 1 | 4 | 5 | 6 | 7 => Document::SCORE,
        2 => Document::PREVIOUS_SCORE,
        _ => return Err("unknown listing shape".into()),
    };
    let documents = db.query::<Document>();
    let input = match shape {
        5 | 7 => documents.filter_between(Document::ENTRY_ID, 1_u64, 1_u64),
        6 => documents.filter_between(Document::ENTRY_ID, 1_u64, 2_u64),
        _ => documents.eq(Document::ENTRY_ID, 1_u64),
    };
    let mut q = input
        .order_by(&[field, Document::ID])
        .descending()
        .limit(20)
        .work_limit(work as usize)
        .test_output_limit(bytes as usize);
    if !matches!(shape, 4 | 7) {
        q = q.eq(Document::PUBLISHED, true);
    }
    if let Some(max_id) = max_id {
        q = q.lte(Document::ID, max_id);
    }
    if shape == 1 {
        q = q.using_index("entry_id,score", &[1_u64.into()]);
    }
    let q = q.after(cursor.as_ref());
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

// measured_filter_count
// Measure filtered counts and first-match existence including database open.
// guard  : validates the request frame for method 193.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "measured_filter_count",
    guard = crate::transport::admit::<193>,
    decode = crate::transport::decode_measured::<_, 193>,
    encode = crate::transport::encode::<_, 193>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn measured_filter_count((shape, counter): crate::transport::Measured<u8>) -> (u64, u32, u32, u64) {
    use crate::schema::Entry;

    let start = counter();
    let host = runtime();
    let db = open(&host);
    let (value, examined, fetched) = match shape {
        0 | 1 => {
            let q = db
                .query::<Entry>()
                .using_index("category,id", &[0_u64.into()])
                .filter_in(Entry::ID, &[0_u64, 2, 3, 6, 12]);
            if shape == 0 {
                let a = q.count().unwrap();
                (a.value, a.examined, a.fetched)
            } else {
                let a = q.exists().unwrap();
                (u64::from(a.value), a.examined, a.fetched)
            }
        }
        2 => {
            let a = db
                .query::<Document>()
                .using_index("published,title", &[true.into()])
                .filter_length(Document::TITLE, ic_db::Comparison::Eq, 0)
                .count()
                .unwrap();
            (a.value, a.examined, a.fetched)
        }
        3 => {
            let a = db
                .query::<Entry>()
                .gte(Entry::AMOUNT, 30_u64)
                .count_validated()
                .unwrap();
            (a.value, a.examined, a.fetched)
        }
        4 | 5 => {
            use crate::schema::BinaryObject;
            let q = db.query::<BinaryObject>().gte(BinaryObject::ID, 1_u64);
            let a = if shape == 4 {
                q.count()
            } else {
                q.count_validated()
            }
            .unwrap();
            (a.value, a.examined, a.fetched)
        }
        6..=8 => {
            use crate::schema::BinaryObject;
            let id = if shape == 7 { 2_u64 } else { 1_u64 };
            let q = db
                .query::<BinaryObject>()
                .eq(BinaryObject::ID, id)
                .work_limit(1);
            if shape == 8 {
                let a = q.has_at_least(2).unwrap();
                (u64::from(a.value), a.examined, a.fetched)
            } else {
                let a = q.count().unwrap();
                (a.value, a.examined, a.fetched)
            }
        }
        _ => panic!("unknown filter shape"),
    };
    (value, examined as u32, fetched as u32, counter() - start)
}

// measured_ulid_listing
// Page ULID primary keys by descending embedded time and measure store open and query work.
// guard  : validates the request frame for method 246.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "measured_ulid_listing",
    guard = crate::transport::admit::<246>,
    decode = crate::transport::decode_measured::<_, 246>,
    encode = crate::transport::encode::<_, 246>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn measured_ulid_listing(
    ((start_ms, end_ms, after, ids_only), counter): crate::transport::Measured<UlidListingRequest>,
) -> UlidListingSample {
    use crate::schema::KeyRow;
    use ic_db::Timestamp;
    let start = counter();
    let host = runtime();
    let db = open(&host);
    let opened = counter();
    let cursor = decode_cursor(after.as_deref()).unwrap();
    let query = db
        .query::<KeyRow>()
        .filter_ulid_time(
            KeyRow::ID,
            Timestamp::from_millis(start_ms)..Timestamp::from_millis(end_ms),
        )
        .order_desc(KeyRow::ID)
        .after(cursor.as_ref())
        .limit(8);
    if ids_only {
        let page = query.select(|row| row.id).fetch().unwrap();
        ulid_sample(page, |id| id, opened - start, counter() - opened)
    } else {
        let page = query.fetch().unwrap();
        ulid_sample(page, |row| row.id, opened - start, counter() - opened)
    }
}

fn ulid_sample<T>(
    page: ic_db::QueryPage<T>,
    id: impl Fn(T) -> ic_db::Ulid,
    open: u64,
    query: u64,
) -> UlidListingSample {
    (
        page.rows.into_iter().map(|r| id(r).to_bytes()).collect(),
        page.next.map(|c| c.to_bytes()),
        page.examined as u32,
        page.fetched as u32,
        page.output_bytes as u32,
        open,
        query,
    )
}

type UlidListingRequest = (i64, i64, Option<Vec<u8>>, bool);
type UlidListingSample = (Vec<[u8; 16]>, Option<Vec<u8>>, u32, u32, u32, u64, u64);

// measured_bulk_selection
// Compare three primary lookup paths on the same stored rows and ordered keys.
// guard  : validates the request frame for method 248.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "measured_bulk_selection",
    guard = crate::transport::admit::<248>,
    decode = crate::transport::decode_measured::<_, 248>,
    encode = crate::transport::encode::<_, 248>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn measured_bulk_selection(
    ((path, keys), counter): crate::transport::Measured<(u8, Vec<u64>)>,
) -> (Vec<Option<(i64, u64)>>, u64) {
    let host = runtime();
    let db = open(&host);
    let start = counter();
    let rows = match path {
        0 => keys
            .iter()
            .map(|&id| {
                db.query::<Document>()
                    .eq(Document::ID, id)
                    .select(|r| (r.score, r.id))
                    .first()
                    .unwrap()
            })
            .collect(),
        1 => db
            .get_many::<Document>(&keys)
            .fetch()
            .unwrap()
            .into_iter()
            .map(|row| row.map(|r| (r.score, r.id)))
            .collect(),
        2 => db
            .get_many::<Document>(&keys)
            .select(|r| (r.score, r.id))
            .fetch()
            .unwrap(),
        _ => panic!("unknown bulk selection path"),
    };
    (rows, counter() - start)
}

#[ic_platform_runtime::update(
    runtime = ic_platform_runtime, name = "seed_primary_pages",
    guard = crate::transport::admit::<250>, decode = crate::transport::decode::<_, 250>,
    encode = crate::transport::encode::<_, 250>, max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn seed_primary_pages(shape: u8) {
    use crate::schema::BinaryObject;
    let host = runtime();
    let mut db = open(&host);
    for id in 1..=24 {
        let size = match shape {
            0 => 8,
            1 => 1024,
            2 if matches!(id, 9 | 17) => 1024,
            2 => 8,
            _ => panic!("unknown shape"),
        };
        let row = BinaryObject {
            id,
            payload: vec![id as u8; size],
            optional: None,
        };
        let mut batch = ic_db::Batch::new();
        if id == 1 {
            batch.replace(&row).unwrap();
        } else {
            batch.insert(&row).unwrap();
        }
        db.commit(batch).unwrap();
    }
}

type PrimaryPageSample = (Vec<(u64, ic_wire::ByteBuf)>, Option<u64>, u64, u64);
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime, name = "measured_primary_page",
    guard = crate::transport::admit::<251>, decode = crate::transport::decode_measured::<_, 251>,
    encode = crate::transport::encode::<_, 251>, max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn measured_primary_page(
    ((after, limit), counter): crate::transport::Measured<(Option<u64>, u8)>,
) -> PrimaryPageSample {
    let start = counter();
    let host = runtime();
    let db = open(&host);
    let opened = counter();
    let page = db
        .page::<crate::schema::BinaryObject>(after, usize::from(limit))
        .unwrap();
    let end = counter();
    (
        page.rows
            .into_iter()
            .map(|r| (r.id, r.payload.into()))
            .collect(),
        page.next_after,
        opened - start,
        end - opened,
    )
}
