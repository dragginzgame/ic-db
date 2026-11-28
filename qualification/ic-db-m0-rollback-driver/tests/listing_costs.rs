//! SQLite results and PocketIC costs for filtered and ordered document listings.

use ic_db_qualification_transport::fixture_wire::ByteBuf;
mod wire_call;

use ic_db_m0_rollback_driver::instance;
use wire_call::call;
use wire_call::wire_frame;

#[tokio::test]
#[ignore = "requires the query-measurements Wasm; run make m2-listing-costs"]
async fn ordered_listing_paths_match_sqlite_page_by_page() {
    type Sample = (Vec<u64>, Option<Vec<u8>>, u32, u32, u64);
    let wasm = ic_db_m0_rollback_driver::records_wasm();
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
    let outside = [(2_u64, ByteBuf::from(2_u64.to_be_bytes().to_vec()), 99_i64)];
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
                    rows.push((id, ByteBuf::from(id.to_be_bytes().to_vec()), score));
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
    // Identical projections and output limits. The upper bound determines
    // whether the leading index component is fixed or two entry IDs can interleave.
    for (shape, upper) in [(5_u8, 1_u32), (6, 2), (7, 1)] {
        let mut statement = sql.prepare(
            "SELECT id FROM docs WHERE entry_id BETWEEN 1 AND ?1 AND (?2=0 OR published=1) ORDER BY score DESC,id DESC",
        ).unwrap();
        let ordered = statement
            .query_map(rusqlite::params![upper, shape != 7], |r| {
                r.get::<_, u32>(0).map(u64::from)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let (ids, mut next, examined, fetched, instructions) =
            measured(shape, &None, 512, 160, None).await.unwrap();
        assert_eq!(ids, ordered[..20]);
        println!(
            "listing-range shape={shape} upper={upper} cursor={} examined={examined} fetched={fetched} instructions={instructions}",
            next.is_some()
        );
        assert_eq!(next.is_some(), shape != 6);
        if next.is_some() {
            let mut offset = ids.len();
            for _ in 0..130 {
                let (ids, cursor, examined, _, _) =
                    measured(shape, &next, 5, 160, None).await.unwrap();
                assert!(examined <= 5);
                assert_eq!(ids, ordered[offset..offset + ids.len()]);
                offset += ids.len();
                assert_ne!(cursor, next);
                next = cursor;
                if next.is_none() {
                    break;
                }
            }
            assert!(next.is_none());
            assert_eq!(offset, ordered.len());
        }
    }
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
            .map(|id| (id, ByteBuf::from(id.to_be_bytes().to_vec()), score))
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

#[tokio::test]
#[ignore = "requires the query-measurements Wasm; run make m2-filter-costs"]
async fn measured_membership_and_empty_text_return_expected_counts() {
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    // Eight published documents include two empty titles. The seeded unpublished
    // document is outside the selected index prefix.
    let rows: Vec<_> = (2..10_u64)
        .map(|id| (id, if id <= 3 { "" } else { "abc" }, true))
        .collect();
    call::<Result<(), u8>>(pic, canister, "text_index_batch", 23, &(rows, false))
        .await
        .unwrap();
    let measure = async |profile: &str, cases: &[(u8, &str, u64)]| {
        let before = pic.get_stable_memory(canister).await;
        for &(shape, label, expected) in cases {
            let (value, examined, fetched, instructions): (u64, u32, u32, u64) =
                call(pic, canister, "measured_filter_count", 193, &shape).await;
            assert_eq!(value, expected, "{profile} {label}");
            println!("filter-cost,{profile},{label},{value},{examined},{fetched},{instructions}");
        }
        assert_eq!(pic.get_stable_memory(canister).await, before);
    };
    measure(
        "early-hit",
        &[
            (0, "membership-count", 2),
            (1, "membership-exists", 1),
            (2, "empty-text-count", 2),
            (3, "ordinary-scan", 4),
        ],
    )
    .await;

    for (profile, width) in [("inline-blob", 8), ("external-blob", 1024)] {
        let payload = ByteBuf::from(vec![7; width]);
        call::<Result<(), u8>>(
            pic,
            canister,
            "replace_binary_object",
            14,
            &((payload, None::<ByteBuf>), false),
        )
        .await
        .unwrap();
        measure(
            profile,
            &[
                (4, "range-count", 1),
                (5, "validated", 1),
                (6, "point-hit", 1),
                (7, "point-miss", 0),
                (8, "point-threshold-two", 0),
            ],
        )
        .await;
    }

    // Put the only matching ID at the end of a category containing seven
    // nonmembers (1, 4, 7, 8, 9, 10, 11). Keep the probe list identical.
    for id in [1_u64, 2, 6, 7, 8, 9, 10, 11, 12] {
        let category = u64::from(id == 2 || id == 6);
        let mode = u8::from(id > 6);
        call::<Result<(), u8>>(pic, canister, "write", 4, &(id, category, id * 10, mode))
            .await
            .unwrap();
    }
    measure(
        "late-hit",
        &[(0, "membership-count", 1), (1, "membership-exists", 1)],
    )
    .await;
    owned.drop().await;
}

#[tokio::test]
#[ignore = "requires the query-measurements Wasm; run make m2-listing-costs"]
async fn ulid_time_listing_resumes_inside_a_millisecond() {
    type Sample = (Vec<[u8; 16]>, Option<Vec<u8>>, u32, u32, u32, u64, u64);
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;

    let rows: Vec<_> = [(99_u128, 1), (100, 16), (101, 5), (102, 1)]
        .into_iter()
        .flat_map(|(ms, count)| (0..count).map(move |suffix| ((ms << 80) | suffix).to_be_bytes()))
        .enumerate()
        .map(|(n, key)| (key, None::<u64>, n as u64))
        .collect();
    for chunk in rows.chunks(8) {
        call::<Result<(), u8>>(
            pic,
            canister,
            "key_batch",
            70,
            &(
                chunk,
                Vec::<(u64, Option<[u8; 16]>)>::new(),
                Vec::<[u8; 16]>::new(),
                Vec::<u64>::new(),
                false,
            ),
        )
        .await
        .unwrap();
    }
    let expected: Vec<_> = rows[1..rows.len() - 1].iter().rev().map(|r| r.0).collect();
    for ids_only in [true, false] {
        let mut cursor: Option<Vec<u8>> = None;
        for ids in expected.chunks(8) {
            let request = wire_frame::request(246, &(100_i64, 102_i64, cursor, ids_only));
            let reply = pic
                .query_call(canister, canister, "measured_ulid_listing", request)
                .await
                .unwrap();
            let (actual, next, examined, fetched, bytes, open, query): Sample =
                wire_frame::response(246, &reply);
            assert_eq!(actual, ids);
            assert_eq!(examined as usize, ids.len());
            assert_eq!(fetched, examined);
            assert_eq!(bytes as usize, ids.len() * if ids_only { 16 } else { 25 });
            assert_eq!(next.is_some(), ids.len() == 8);
            println!(
                "ulid-listing,ids_only={ids_only},bytes={bytes},rows={},examined={examined},fetched={fetched},open={open},query={query}",
                actual.len()
            );
            cursor = next;
        }
    }
    owned.drop().await;
}

#[tokio::test]
#[ignore = "requires the query-measurements Wasm; run make m2-bulk-selection"]
async fn projected_bulk_lookup_matches_full_and_fluent_reads() {
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    let measure = async |keys: &[u64], expected: &[Option<(i64, u64)>]| {
        let mut costs = Vec::new();
        for path in 0..3_u8 {
            let (actual, instructions): (Vec<Option<(i64, u64)>>, u64) =
                call(pic, canister, "measured_bulk_selection", 248, &(path, keys)).await;
            assert_eq!(actual, expected);
            costs.push(instructions);
        }
        costs
    };
    for width in [0, 64] {
        let note = (width > 0).then(|| "x".repeat(width));
        call::<Result<(), u8>>(
            pic,
            canister,
            "replace_document",
            8,
            &("template", note, true, None::<u64>, 1_i64, None::<i64>),
        )
        .await
        .unwrap();
        let rows: Vec<_> = (2..=32_u64)
            .map(|id| (id, ByteBuf::from(id.to_be_bytes().to_vec()), id as i64))
            .collect();
        for chunk in rows.chunks(8) {
            call::<Result<(), u8>>(pic, canister, "indexed_batch", 21, &(chunk, false))
                .await
                .unwrap();
        }
        for count in [1, 8, 32] {
            for mixed in [false, true] {
                let keys: Vec<_> = (1..=count)
                    .map(|id| match (mixed, id % 4) {
                        (true, 0) => 1,
                        (true, 1) => 99,
                        _ => id,
                    })
                    .collect();
                let expected: Vec<_> = keys
                    .iter()
                    .map(|&id| (id != 99).then_some((id as i64, id)))
                    .collect();
                let costs = measure(&keys, &expected).await;
                println!(
                    "bulk-selection,note={width},keys={count},mixed={mixed},instructions={costs:?}"
                );
            }
        }
    }
    // Interleave missing odd keys with hits in separate B-tree leaves.
    let rows: Vec<_> = (34..=544_u64)
        .step_by(2)
        .map(|id| (id, ByteBuf::from(id.to_be_bytes().to_vec()), id as i64))
        .collect();
    for chunk in rows.chunks(8) {
        call::<Result<(), u8>>(pic, canister, "indexed_batch", 21, &(chunk, false))
            .await
            .unwrap();
    }
    for (label, keys) in [
        (
            "scattered-hits",
            (0..32).rev().map(|i| 34 + 16 * i).collect::<Vec<u64>>(),
        ),
        (
            "scattered-misses",
            (0..32).rev().map(|i| 35 + 16 * i).collect(),
        ),
        ("one-repeated-hit", vec![34; 32]),
    ] {
        let expected: Vec<_> = keys
            .iter()
            .map(|&id| (id % 2 == 0).then_some((id as i64, id)))
            .collect();
        let costs = measure(&keys, &expected).await;
        println!("bulk-selection,note=64,keys=32,distribution={label},instructions={costs:?}");
    }
    owned.drop().await;
}

#[tokio::test]
#[ignore = "requires page-measurement Wasm and PocketIC"]
async fn primary_page_lookahead() {
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let predecessor = std::env::var_os("IC_DB_PAGE_PREDECESSOR")
        .map(|path| std::fs::read(path).unwrap())
        .unwrap_or_else(|| wasm.clone());
    let owned = instance().await.unwrap();
    let pic = &owned;
    for shape in 0..3_u8 {
        let id = pic.create_canister().await;
        pic.add_cycles(id, 3_000_000_000_000).await;
        pic.install_canister(id, predecessor.clone(), vec![], None)
            .await;
        call::<()>(pic, id, "seed_primary_pages", 250, &shape).await;
        for phase in ["fresh", "reopened"] {
            if phase == "reopened" {
                ic_db_m0_rollback_driver::upgrade_stopped(pic, id, wasm.clone(), vec![], None)
                    .await
                    .unwrap();
            }
            let before = pic.get_stable_memory(id).await;
            for limit in [1_u8, 8, 16, 32] {
                let mut after = None;
                loop {
                    let (rows, next, open, work): (Vec<(u64, ByteBuf)>, Option<u64>, u64, u64) =
                        call(pic, id, "measured_primary_page", 251, &(after, limit)).await;
                    let first = after.unwrap_or(0) + 1;
                    let last = (first + u64::from(limit) - 1).min(24);
                    assert_eq!(rows.len() as u64, (first..=last).count() as u64);
                    for ((key, payload), expected) in rows.iter().zip(first..=last) {
                        assert_eq!(*key, expected);
                        let size = if shape == 1 || (shape == 2 && matches!(expected, 9 | 17)) {
                            1024
                        } else {
                            8
                        };
                        assert_eq!(payload.as_ref(), vec![expected as u8; size].as_slice());
                    }
                    assert_eq!(next, (last < 24).then_some(last));
                    println!(
                        "primary,{phase},{shape},{limit},{first},{},{open},{work}",
                        rows.len()
                    );
                    if first > 24 {
                        break;
                    }
                    after = Some(next.unwrap_or(24)); // Also probe an explicitly exhausted cursor.
                }
            }
            assert_eq!(pic.get_stable_memory(id).await, before);
        }
    }
    owned.drop().await;
}
