//! Compare four installed projection shapes with the same bounded SQLite listing.
mod artifact;

use ic_db_m0_rollback_driver::{LIMITS, instance, upgrade_stopped};
use ic_testkit::pocket_ic::{CanisterId, RejectResponse, nonblocking::PocketIc};
use ic_wire::{Direction, Encoding};
use rusqlite::Connection;
use std::{fs::File, path::PathBuf};

type Row = (u64, Option<(u16, u16)>);
type Reply = (Vec<Row>, Option<Vec<u8>>, u32, u32, u32, u64);

async fn listing(
    pic: &PocketIc,
    canister: CanisterId,
    mode: u8,
    sparse: bool,
    cursor: Option<&[u8]>,
) -> Result<Reply, RejectResponse> {
    let frame = |direction| Encoding::Frame {
        method: 1,
        direction,
    };
    let request =
        ic_wire::encode(&(mode, sparse, cursor), frame(Direction::Request), LIMITS).unwrap();
    let bytes = pic
        .query_call(canister, canister, "listing", request)
        .await?;
    Ok(ic_wire::decode(&bytes, frame(Direction::Response), LIMITS).unwrap())
}

#[derive(Debug, Default)]
struct ExpectedPage {
    rows: Vec<(u64, u16, u16)>,
    examined: u32,
}

fn sqlite_pages(sparse: bool) -> Vec<ExpectedPage> {
    let sql = Connection::open_in_memory().unwrap();
    sql.execute_batch(
        "CREATE VIEW listing AS
        WITH RECURSIVE ids(id) AS (VALUES(0) UNION ALL SELECT id+1 FROM ids WHERE id<23)
        SELECT id, (7*id%24)/3 AS rank, 11*id AS score, id%4<>3 AS phase FROM ids;",
    )
    .unwrap();
    let mut statement = sql
        .prepare(
            "SELECT id,score,rank,(NOT ?1 OR (rank>=2 AND id%5<>2))
        FROM listing WHERE phase ORDER BY rank,id",
        )
        .unwrap();
    let candidates = statement
        .query_map([sparse], |r| {
            Ok((
                u64::from(r.get::<_, u32>(0)?),
                r.get(1)?,
                r.get(2)?,
                r.get::<_, bool>(3)?,
            ))
        })
        .unwrap();
    let mut pages = vec![ExpectedPage::default()];
    for candidate in candidates {
        let page = pages.last_mut().unwrap();
        let (id, score, rank, ready) = candidate.unwrap();
        page.examined += 1;
        if ready {
            page.rows.push((id, score, rank));
        }
        if page.rows.len() == 2 || page.examined == 4 {
            pages.push(ExpectedPage::default());
        }
    }
    if sparse {
        assert!(pages[0].rows.is_empty() && pages[0].examined == 4);
    }
    pages
}

#[tokio::test]
#[ignore = "run make m2-column-listing"]
async fn installed_projection_pages_match_sqlite() {
    let dir = PathBuf::from(std::env::var_os("IC_DB_LISTING_DIR").unwrap());
    let pic = instance().await.unwrap();
    let shapes = [
        ("entity", 538, true),
        ("named", 16, false),
        ("scalar", 16, false),
        ("tuple", 20, true),
    ];
    for (build, modes) in [
        ("entity", 0..1),
        ("named", 1..2),
        ("scalar", 2..3),
        ("tuple", 3..4),
        ("records", 0..2),
        ("with-scalar", 0..3),
        ("all", 0..4),
    ] {
        let (wasm, facts) =
            artifact::read(File::open(dir.join(format!("column-listing-{build}.wasm"))).unwrap())
                .unwrap();
        assert!(facts.exports.contains("canister_query listing"));
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 2_000_000_000_000).await;
        pic.install_canister(canister, wasm, vec![], None).await;
        for mode in modes {
            let (shape, width, details) = shapes[mode];
            for sparse in [false, true] {
                let expected = sqlite_pages(sparse);
                let mut cursor: Option<Vec<u8>> = None;
                let mut instructions = 0;
                let mut owners = 0;
                let mut output = 0;
                for (i, expected_page) in expected.iter().enumerate() {
                    let (rows, next, examined, fetched, size, cost) =
                        listing(&pic, canister, mode as u8, sparse, cursor.as_deref())
                            .await
                            .unwrap();
                    let expected_rows: Vec<_> = expected_page
                        .rows
                        .iter()
                        .map(|&(id, score, rank)| (id, details.then_some((score, rank))))
                        .collect();
                    assert_eq!(rows, expected_rows, "{shape} page {i}");
                    assert_eq!(
                        (examined, fetched),
                        (expected_page.examined, expected_page.examined),
                        "{shape} page {i}"
                    );
                    assert_eq!(size, width * rows.len() as u32);
                    assert_eq!(next.is_some(), i + 1 < expected.len(), "{shape} page {i}");
                    println!(
                        "{build}/{shape} sparse={sparse} page={i} rows={} examined={examined} owners={fetched} bytes={size} instructions={cost}",
                        rows.len()
                    );
                    cursor = next;
                    instructions += cost;
                    owners += fetched;
                    output += size;
                }
                println!(
                    "{build}/{shape} sparse={sparse} wasm={} code={} pages={} owners={owners} bytes={output} instructions={instructions}",
                    facts.identity.bytes,
                    facts.code_section_bytes,
                    expected.len()
                );
            }
        }
    }
    pic.drop().await;
}

#[tokio::test]
#[ignore = "run make m2-column-listing"]
async fn sparse_scalar_cursor_survives_adding_other_projection_shapes() {
    let dir = PathBuf::from(std::env::var_os("IC_DB_LISTING_DIR").unwrap());
    let pic = instance().await.unwrap();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 2_000_000_000_000).await;
    let wasm = |shape| std::fs::read(dir.join(format!("column-listing-{shape}.wasm"))).unwrap();
    pic.install_canister(canister, wasm("scalar"), vec![], None)
        .await;

    let first = listing(&pic, canister, 2, true, None).await.unwrap();
    assert!(first.0.is_empty());
    assert_eq!(first.2, 4);
    let cursor = first.1.unwrap();
    upgrade_stopped(&pic, canister, wasm("all"), vec![], None)
        .await
        .unwrap();
    // The named ID record has the same field and byte width as the scalar.
    for mode in [0, 1, 3] {
        let err = listing(&pic, canister, mode, true, Some(&cursor))
            .await
            .unwrap_err();
        assert!(err.reject_message.contains("InvalidCursor"), "{err:?}");
    }

    let pages = sqlite_pages(true);
    let mut after = Some(cursor);
    for expected in &pages[1..] {
        let page = listing(&pic, canister, 2, true, Some(after.as_deref().unwrap()))
            .await
            .unwrap();
        let rows: Vec<_> = expected.rows.iter().map(|&(id, _, _)| (id, None)).collect();
        assert_eq!(page.0, rows);
        assert_eq!(page.2, expected.examined);
        after = page.1;
    }
    assert!(after.is_none());
    pic.drop().await;
}
