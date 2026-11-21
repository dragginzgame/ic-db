//! SQLite results and PocketIC costs for filtered and ordered document listings.

mod wire_call;

use ic_db_m0_rollback_driver::instance;
use wire_call::call;

#[tokio::test]
#[ignore = "requires the query-measurements Wasm; run make m2-listing-costs"]
async fn ordered_listing_paths_match_sqlite_page_by_page() {
    type Sample = (Vec<u64>, Option<Vec<u8>>, u32, u32, u64);
    let wasm = std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").unwrap()).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    let installed_bytes = pic.get_stable_memory(canister).await.len();
    let sql = rusqlite::Connection::open_in_memory().unwrap();
    sql.execute_batch("CREATE TABLE docs(id INTEGER PRIMARY KEY, entry_id INTEGER, score INTEGER, published INTEGER);
        INSERT INTO docs VALUES (1,1,-10,0);").unwrap();
    let set_template = async |visible, previous_score: Option<i64>| {
        call::<Result<(), u8>>(
            pic,
            canister,
            "replace_document",
            8,
            &(
                "template",
                None::<String>,
                visible,
                None::<u64>,
                -10_i64,
                previous_score,
            ),
        )
        .await
        .unwrap();
    };
    // Move the visible template to entry 2 and seed a high-scoring distractor.
    set_template(true, None).await;
    call::<Result<(), u8>>(pic, canister, "reference_batch", 10, &3_u8)
        .await
        .unwrap();
    let outside = [(
        2_u64,
        ic_wire::ByteBuf::from(2_u64.to_be_bytes().to_vec()),
        99_i64,
    )];
    call::<Result<(), u8>>(pic, canister, "indexed_batch", 21, &(&outside[..], false))
        .await
        .unwrap();
    sql.execute("INSERT INTO docs VALUES (2,2,99,1)", [])
        .unwrap();
    call::<Result<(), u8>>(pic, canister, "write", 4, &(1_u64, 1_u64, 11_u64, 1_u8))
        .await
        .unwrap();

    // replace_document selects entry 1; indexed_batch copies its visibility.
    for visible in [true, false] {
        for score in 0..12_i64 {
            let mut rows = Vec::new();
            for id in (3..=130_u64).filter(|id| (id % 12) as i64 == score) {
                if (score % 4 == 0 || id % 13 == 0) == visible {
                    rows.push((id, ic_wire::ByteBuf::from(id.to_be_bytes().to_vec()), score));
                    sql.execute(
                        "INSERT INTO docs VALUES (?1,1,?2,?3)",
                        rusqlite::params![id as i64, score, visible],
                    )
                    .unwrap();
                }
            }
            set_template(visible, Some(score)).await;
            for chunk in rows.chunks(8) {
                call::<Result<(), u8>>(pic, canister, "indexed_batch", 21, &(chunk, false))
                    .await
                    .unwrap();
            }
        }
    }
    let check_count = async |population: u32, covered: bool| {
        let total: u32 = sql
            .query_row(
                "SELECT COUNT(*) FROM docs WHERE entry_id=1 AND published=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        for work in [total + 1, 512] {
            let (result, instructions): (Result<(u64, u32, u32), String>, u64) =
                call(pic, canister, "measured_listing_count", 192, &work).await;
            let candidates = if covered { total } else { population - 1 };
            if work > candidates {
                let fetched = if covered { 0 } else { candidates };
                assert_eq!(result, Ok((u64::from(total), candidates, fetched)));
            } else {
                assert_eq!(result.as_ref().unwrap_err(), "Limit");
            }
            println!(
                "listing-count,population={population},work={work},result={result:?},instructions={instructions}"
            );
        }
    };
    let ordered_ids = || {
        sql.prepare(
            "SELECT id FROM docs WHERE entry_id=1 AND published=1 ORDER BY score DESC,id DESC",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, u32>(0).map(u64::from))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    let expected = ordered_ids();
    let before = pic.get_stable_memory(canister).await;
    check_count(130, false).await;
    let measured =
        async |shape: u8, cursor: &Option<Vec<u8>>, work: u32, bytes: u32, max_id: Option<u64>| {
            call::<Result<Sample, String>>(
                pic,
                canister,
                "measured_listing",
                189,
                &(shape, cursor, work, bytes, max_id),
            )
            .await
        };
    for (shape, work, bytes) in [("full", 128, 160), ("sparse", 5, 160), ("byte", 32, 16)] {
        let (mut auto, mut hint) = (None, None);
        let mut offset = 0;
        let mut saw_boundary = false;
        for number in 1..=150 {
            let a = measured(0, &auto, work, bytes, None).await.unwrap();
            let h = measured(1, &hint, work, bytes, None).await.unwrap();
            assert_eq!(
                (&a.0, a.1.is_some(), a.2, a.3),
                (&h.0, h.1.is_some(), h.2, h.3)
            );
            let end = offset + a.0.len();
            assert_eq!(a.0, expected[offset..end]);
            assert!(a.2 <= work && a.3 <= a.2);
            assert!(a.0.len() <= 20 && a.0.len() * 8 <= bytes as usize);
            println!(
                "listing-page,{shape},{number},{},{},{},{},{}",
                a.0.len(),
                a.2,
                a.3,
                a.4,
                h.4
            );
            saw_boundary |= match shape {
                "full" => a.0.len() == 20,
                "sparse" => a.0.is_empty() && a.1.is_some() && a.2 == work,
                "byte" if a.0.len() == 2 && a.1.is_some() => {
                    let wider = measured(0, &auto, work, 160, None).await.unwrap();

                    wider
                        .0
                        .get(2)
                        .is_some_and(|id| Some(id) == expected.get(end))
                }
                _ => false,
            };
            offset = end;
            if a.1.is_none() {
                break;
            }
            assert!(number < 150 && a.1 != auto && h.1 != hint);
            auto = a.1;
            hint = h.1;
        }
        assert_eq!(offset, expected.len());
        assert!(saw_boundary, "{shape} fixture missed its page boundary");
    }
    // Every published entry-1 row carries its score in both fields. Only score
    // has the compound index, so previous_score requires a complete sort.
    let top = sql
        .prepare("SELECT id FROM docs WHERE entry_id=1 AND published=1 AND id<=100 ORDER BY score DESC,id DESC LIMIT 20")
        .unwrap()
        .query_map([], |r| r.get::<_, u32>(0).map(u64::from))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let indexed = measured(0, &None, 256, 160, Some(100)).await.unwrap();
    let sorted = measured(2, &None, 256, 160, Some(100)).await.unwrap();
    assert_eq!(indexed.0, top);
    assert_eq!(sorted.0, top);
    assert!(indexed.1.is_some() && sorted.1.is_none());
    assert!(sorted.2 > indexed.2);
    let sparse = measured(0, &None, 1, 160, Some(100)).await.unwrap();
    assert!(sparse.0.is_empty() && sparse.1.is_some() && sparse.2 == 1);
    assert_eq!(
        measured(2, &None, 1, 160, Some(100)).await.unwrap_err(),
        "Limit"
    );
    // Repeat in reverse order on the warm canister before comparing costs.
    for (shape, path, sample) in [(2, "sorted", sorted), (0, "indexed", indexed)] {
        let again = measured(shape, &None, 256, 160, Some(100)).await.unwrap();
        assert_eq!(
            (&again.0, &again.1, again.2, again.3),
            (&sample.0, &sample.1, sample.2, sample.3)
        );
        println!(
            "listing-top20,{path},rows={},examined={},fetched={},instructions={},repeat={}",
            sample.0.len(),
            sample.2,
            sample.3,
            sample.4,
            again.4
        );
    }
    let mut eligible = sql.prepare("SELECT id FROM docs WHERE entry_id=1 AND published=1 AND id<=?1 ORDER BY score DESC,id DESC").unwrap();
    for max_id in [20_u32, 60, 100, 130] {
        let ids = eligible
            .query_map([max_id], |row| row.get::<_, u32>(0).map(u64::from))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let indexed = measured(0, &None, 256, 160, Some(u64::from(max_id)))
            .await
            .unwrap();
        assert_eq!(indexed.0, &ids[..ids.len().min(20)]);
        assert_eq!(indexed.1.is_some(), ids.len() > 20);
        assert_eq!(indexed.2, indexed.3);
        println!(
            "listing-selectivity,max_id={max_id},eligible={},returned={},examined={},fetched={},instructions={}",
            ids.len(),
            indexed.0.len(),
            indexed.2,
            indexed.3,
            indexed.4
        );
    }
    let all = sql
        .prepare("SELECT id FROM docs WHERE entry_id=1 AND id<=100 ORDER BY score DESC,id DESC")
        .unwrap()
        .query_map([], |r| r.get::<_, u32>(0).map(u64::from))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let first = measured(4, &None, 256, 160, Some(100)).await.unwrap();
    assert_eq!(first.0, all[..20]);
    println!(
        "listing-all-states,examined={},fetched={},instructions={},continued={}",
        first.2,
        first.3,
        first.4,
        first.1.is_some()
    );
    assert!(first.1.is_some());
    let second = measured(4, &first.1, 256, 160, Some(100)).await.unwrap();
    assert_eq!(second.0, all[20..40]);

    let restricted = call::<Result<(), u8>>(pic, canister, "reference_batch", 10, &1_u8).await;
    assert_eq!(restricted, Err(1)); // Surviving documents still reference entry 1.
    assert_eq!(pic.get_stable_memory(canister).await, before);
    println!(
        "listing-storage,installed={installed_bytes},seeded={}",
        before.len()
    );
    for round in 1..=3 {
        for (op, expected) in [
            (0_u8, Some((true, 20, None))),
            (1, Some((true, 20, Some("changed")))),
            (2, Some((true, 21, Some("changed")))),
            (3, Some((false, 21, Some("changed")))),
            (4, None),
        ] {
            let (instructions, saved): (u64, Option<(bool, i64, Option<String>)>) =
                call(pic, canister, "measured_listing_write", 190, &op).await;
            assert_eq!(
                saved.as_ref().map(|(published, score, note)| (
                    *published,
                    *score,
                    note.as_deref()
                )),
                expected
            );
            println!("listing-write,{round},{op},{instructions}");
        }
    }
    println!(
        "listing-storage,after_delete={}",
        pic.get_stable_memory(canister).await.len()
    );
    // Cross the one-call work bound after the fixed-population cost measurements.
    // Both sort fields carry the same rank. Ties span pages and the newest
    // candidates occur beyond the first 512 rows in primary-key order.
    for score in [30_i64, 31] {
        set_template(true, Some(score)).await;
        let rows: Vec<_> = (131..=530_u64)
            .filter(|id| id % 2 == (score % 2) as u64)
            .map(|id| (id, ic_wire::ByteBuf::from(id.to_be_bytes().to_vec()), score))
            .collect();
        for chunk in rows.chunks(8) {
            call::<Result<(), u8>>(pic, canister, "indexed_batch", 21, &(chunk, false))
                .await
                .unwrap();
        }
        for (id, _, rank) in rows {
            sql.execute(
                "INSERT INTO docs VALUES (?1,1,?2,1)",
                (i64::try_from(id).unwrap(), rank),
            )
            .unwrap();
        }
    }
    // Restore the hidden template so its two sort values cannot affect the listing.
    set_template(false, Some(-10)).await;
    check_count(530, false).await;
    let expected = ordered_ids();
    assert_eq!(
        measured(2, &None, 512, 160, None).await.unwrap_err(),
        "Limit"
    );
    let mut after = None;
    let mut returned = 0;
    let mut pages = 0;
    let mut total_instructions = 0;
    loop {
        let (ids, next, examined, _, instructions) =
            measured(0, &after, 512, 160, None).await.unwrap();
        let end = returned + ids.len();
        assert_eq!(ids, expected[returned..end]);
        if pages == 0 {
            assert_eq!(ids.len(), 20);
            println!("listing-large-first,examined={examined},instructions={instructions}");
        }
        returned = end;
        total_instructions += instructions;
        pages += 1;
        if next.is_none() {
            break;
        }
        assert!(pages <= expected.len() && next != after);
        after = next;
    }
    assert_eq!(returned, expected.len());
    println!("listing-large,returned={returned},pages={pages},instructions={total_instructions}");
    owned.drop().await;
}
