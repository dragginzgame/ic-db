//! Check every returned ID against SQLite, including ties across page boundaries.

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};
use rusqlite::{Connection, params};

const BY_PROJECT: u8 = 0;
const BY_TIME: u8 = 1;
const BY_PRICE: u8 = 2;
const BY_PRICE_AND_TIME: u8 = 3;

type Page = (Vec<u64>, Option<Vec<u8>>, usize, usize);

fn request(method: u64, value: &impl serde::Serialize) -> Vec<u8> {
    ic_wire::encode(
        value,
        Encoding::Frame {
            method,
            direction: Direction::Request,
        },
        LIMITS,
    )
    .unwrap()
}

fn response<T: serde::de::DeserializeOwned>(method: u64, bytes: &[u8]) -> T {
    ic_wire::decode(
        bytes,
        Encoding::Frame {
            method,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap()
}

#[tokio::test]
async fn ranked_listing_matches_sqlite_on_each_layout() {
    let wasm = std::fs::read(
        std::env::var_os("IC_DB_RECORDS_BENCHMARK_WASM").expect("run make m2-ranked-listing"),
    )
    .unwrap();
    let sql = Connection::open_in_memory().unwrap();
    sql.execute_batch("CREATE TABLE items(id INTEGER PRIMARY KEY, project_id INTEGER, open INTEGER, cents INTEGER, updated_at INTEGER)").unwrap();
    let pic = instance().await.unwrap();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 100_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    for block in 0_u32..16 {
        let mut rows = Vec::new();
        for id in block * 8 + 1..=block * 8 + 8 {
            let project = if id % 8 == 0 { 2_u32 } else { 1 };
            let open = id % 5 != 0;
            let cents = if id % 11 < 2 { 10_000_i64 } else { 10_001 };
            let millis = i64::from(id / 5 % 7);
            sql.execute(
                "INSERT INTO items VALUES(?,?,?,?,?)",
                params![id, project, open, cents, millis],
            )
            .unwrap();
            rows.push((u64::from(id), u64::from(project), open, cents, millis));
        }
        let reply = pic
            .update_call(canister, canister, "seed_listing", request(11, &rows))
            .await
            .unwrap();
        response::<()>(11, &reply);
    }
    let expected_ids = |cents: i64| {
        sql.prepare(
            "SELECT id FROM items WHERE project_id=1 AND open AND cents<=?
             ORDER BY updated_at DESC, id DESC",
        )
        .unwrap()
        .query_map([cents], |row| row.get::<_, u32>(0).map(u64::from))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    let expected = expected_ids(10_000);
    let candidates: u32 = sql
        .query_row(
            "SELECT COUNT(*) FROM items WHERE project_id=1 AND open",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let read = async |layout: u8, cents: i64, work: usize, limit: usize, after: Option<Vec<u8>>| {
        let reply = pic
            .query_call(
                canister,
                canister,
                "ranked_listing",
                request(12, &(layout, cents, work, limit, after)),
            )
            .await
            .unwrap();
        response::<(Option<Page>, u64)>(12, &reply)
    };
    let mut ids = Vec::new();
    let mut cursor = None;
    let (mut pages, mut empty, mut examined, mut fetched, mut total) = (0, 0, 0, 0, 0);
    loop {
        assert!(pages <= 128);
        let (page, instructions) = read(BY_PRICE_AND_TIME, 10_000, 4, 2, cursor.clone()).await;
        let (rows, next, work, reads) = page.unwrap();
        assert!(work <= 4 && rows.len() <= 2);
        empty += usize::from(rows.is_empty() && next.is_some());
        ids.extend(rows);
        pages += 1;
        examined += work;
        fetched += reads;
        total += instructions;
        if next.is_none() {
            break;
        }
        assert_ne!(next, cursor);
        cursor = next;
    }
    assert_eq!(ids, expected);
    assert!(empty > 0 && expected.len() > 2);
    assert_eq!(examined, candidates as usize);
    println!(
        "dual index: rows={} pages={pages} empty_continued={empty} examined={examined} fetched={fetched} instructions={total}",
        ids.len()
    );
    let (page, instructions) = read(BY_TIME, 10_000, 128, 20, None).await;
    let (ids, next, work, reads) = page.unwrap();
    assert_eq!(ids, expected);
    assert!(next.is_none());
    println!("indexed: limit=20 examined={work} fetched={reads} instructions={instructions}");

    // Even the last candidate cannot prove that no better row follows it.
    assert!(
        read(BY_PROJECT, 10_000, candidates as usize, 2, None)
            .await
            .0
            .is_none()
    );
    for limit in [2, 32] {
        let (page, instructions) =
            read(BY_PROJECT, 10_000, candidates as usize + 1, limit, None).await;
        let (ids, next, work, reads) = page.unwrap();
        assert_eq!(ids, expected[..limit.min(expected.len())]);
        assert_eq!(work, candidates as usize);
        assert!(next.is_none());
        println!(
            "complete sort: limit={limit} examined={work} fetched={reads} instructions={instructions}"
        );
    }
    for cents in [9_999, 10_000, 10_001] {
        let matches = expected_ids(cents);
        let mut time_work = None;
        for layout in [BY_TIME, BY_PROJECT, BY_PRICE, BY_PRICE_AND_TIME] {
            let (page, instructions) = read(layout, cents, 128, 2, None).await;
            let (ids, next, examined, fetched) = page.unwrap();
            assert_eq!(ids, matches[..2.min(matches.len())]);
            assert_eq!(
                next.is_some(),
                matches!(layout, BY_TIME | BY_PRICE_AND_TIME) && matches.len() > 2
            );
            if layout == BY_TIME {
                time_work = Some((examined, fetched));
            } else if layout == BY_PRICE_AND_TIME {
                assert_eq!(Some((examined, fetched)), time_work);
            }
            if layout == BY_PRICE {
                assert_eq!(examined, matches.len());
                assert_eq!(fetched, matches.len());
            } else if layout == BY_PROJECT || matches.is_empty() {
                assert_eq!(examined, candidates as usize);
            } else if matches.len() == candidates as usize {
                assert_eq!(examined, 2);
            }
            println!(
                "first two: cents={cents} layout={layout} ids={ids:?} examined={examined} fetched={fetched} instructions={instructions}"
            );
        }
    }
    pic.drop().await;
}
