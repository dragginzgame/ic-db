//! Live index positions across writes and upgrades; a tuple oracle is independent
//! of the engine's key codec and cursor decoder.

mod wire_frame;
use ic_db_m0_rollback_driver::{instance, upgrade_stopped};
use wire_frame::{request, response};

type Page = (Vec<(u64, u64)>, Option<Vec<u8>>, u32, u32, u32, u64);

#[tokio::test]
async fn live_order_and_filter_changes_resume_from_the_old_position_after_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let pic = instance().await.unwrap();
    for descending in [false, true] {
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 3_000_000_000_000).await;
        pic.install_canister(canister, wasm.clone(), vec![], None)
            .await;
        // Mirror the scenario so the same obligations apply in both directions.
        let id = |position: u64| if descending { 7 - position } else { position };
        let mut model = (1..=6)
            .map(|position| {
                (
                    id(position),
                    1_u64,
                    if matches!(position, 4 | 5) { 0 } else { 100 },
                )
            })
            .collect::<Vec<_>>();
        let write = async |row: (u64, u64, u64), insert: bool| {
            let reply = pic
                .update_call(
                    canister,
                    canister,
                    "write",
                    request(4, &(row.0, row.1, row.2, u8::from(insert))),
                )
                .await
                .unwrap();
            assert_eq!(response::<Result<(), u8>>(4, &reply), Ok(()));
        };
        for &row in &model {
            write(row, false).await;
        }
        let query = async |cursor: Option<Vec<u8>>, minimum: u64, bytes: u32| {
            let reply = pic
                .query_call(
                    canister,
                    canister,
                    "fluent_ordered",
                    request(
                        98,
                        &(
                            cursor,
                            descending,
                            None::<u64>,
                            minimum,
                            1_u32,
                            1_u32,
                            bytes,
                        ),
                    ),
                )
                .await
                .unwrap();

            response::<Result<Page, u8>>(98, &reply)
        };
        let first = query(None, 50, 16).await.unwrap();
        assert_eq!(first.0, [(id(1), 100)]);
        assert_eq!((first.2, first.3, first.4), (1, 1, 16));
        let original = first.1.unwrap();
        let old_position = (1, id(1));
        let ahead = if descending { 0 } else { 2 };
        let behind = if descending { 2 } else { 0 };
        for row in [
            (id(1), ahead, 100),  // Returned row moves after the cursor: repeats.
            (id(2), behind, 100), // Unseen row moves before it: omitted.
            (id(3), 1, 0),        // Unseen row leaves the predicate.
            (id(4), 1, 100),      // Previously rejected row enters ahead: included.
            (id(5), behind, 100), // Enters behind: omitted.
        ] {
            write(row, false).await;
            *model.iter_mut().find(|r| r.0 == row.0).unwrap() = row;
        }
        for row in [(7, if descending { 0 } else { 1 }, 100), (8, behind, 100)] {
            write(row, true).await;
            model.push(row);
        }
        // Plain tuple order and ordinary numeric predicates define the oracle.
        let mut expected = model
            .iter()
            .copied()
            .filter(|&(id, category, amount)| {
                amount >= 50
                    && if descending {
                        (category, id) < old_position
                    } else {
                        (category, id) > old_position
                    }
            })
            .collect::<Vec<_>>();
        expected.sort_by_key(|&(id, category, _)| (category, id));
        if descending {
            expected.reverse();
        }
        let expected = expected
            .into_iter()
            .map(|(id, _, amount)| (id, amount))
            .collect::<Vec<_>>();
        assert_eq!(
            expected,
            [(id(4), 100), (id(6), 100), (7, 100), (id(1), 100)]
        );
        let before = pic.get_stable_memory(canister).await;
        upgrade_stopped(&pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(query(Some(original.clone()), 51, 16).await.unwrap_err(), 1);
        let mut cursor = Some(original);
        let mut actual = Vec::new();
        let mut empty_pages = 0;
        for _ in 0..16 {
            let page = query(cursor.clone(), 50, 16).await.unwrap();
            assert!(page.2 <= 1 && page.3 <= 1 && page.4 <= 16);
            if page.0.is_empty() && page.1.is_some() {
                empty_pages += 1;
            }
            if !page.0.is_empty() {
                // A too-small reply cannot consume the candidate. Retrying the
                // same logical position with sufficient bytes returns that row.
                assert_eq!(query(cursor.clone(), 50, 15).await.unwrap_err(), 2);
                let retry = query(cursor.clone(), 50, 16).await.unwrap();
                assert_eq!((&retry.0, &retry.1), (&page.0, &page.1));
            }
            if page.1.is_some() {
                assert_ne!(page.1, cursor);
            }
            actual.extend(page.0);
            cursor = page.1;
            if cursor.is_none() {
                break;
            }
        }
        assert!(cursor.is_none(), "bounded enumeration must finish");
        assert!(empty_pages > 0, "residual misses still make progress");
        assert_eq!(actual, expected);
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    pic.drop().await;
}

#[tokio::test]
async fn deleted_and_reinserted_cursor_rows_do_not_restart_the_range() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M1_RECORDS_WASM").expect("run make m2-queries"))
            .unwrap();
    let pic = instance().await.unwrap();
    for descending in [false, true] {
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 3_000_000_000_000).await;
        pic.install_canister(canister, wasm.clone(), vec![], None)
            .await;
        let write = async |id: u64, category: u64, amount: u64, insert: bool| {
            let reply = pic
                .update_call(
                    canister,
                    canister,
                    "write",
                    request(4, &(id, category, amount, u8::from(insert))),
                )
                .await
                .unwrap();
            assert_eq!(response::<Result<(), u8>>(4, &reply), Ok(()));
        };
        let page = async |cursor: Option<Vec<u8>>| {
            let reply = pic
                .query_call(
                    canister,
                    canister,
                    "fluent_ordered",
                    request(
                        98,
                        &(cursor, descending, None::<u64>, 0_u64, 1_u32, 1_u32, 16_u32),
                    ),
                )
                .await
                .unwrap();

            response::<Result<Page, u8>>(98, &reply).unwrap()
        };
        // Place the referenced row first in either direction. Deleting it and
        // its dependent together exercises final-batch integrity as well.
        let first_category = if descending { 2 } else { 0 };
        for id in 1..=6 {
            write(id, if id == 1 { first_category } else { 1 }, 100, false).await;
        }
        let first = page(None).await;
        assert_eq!(first.0, [(1, 100)]);
        let original = first.1.expect("five rows remain");
        let reply = pic
            .update_call(canister, canister, "reference_batch", request(10, &2_u8))
            .await
            .unwrap();
        assert_eq!(response::<Result<(), u8>>(10, &reply), Ok(()));
        let before = pic.get_stable_memory(canister).await;
        upgrade_stopped(&pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        let after_delete = page(Some(original.clone())).await;
        let expected = if descending {
            vec![6, 5, 4, 3, 2]
        } else {
            vec![2, 3, 4, 5, 6]
        };
        assert_eq!(after_delete.0, [(expected[0], 100)]);
        assert_eq!(pic.get_stable_memory(canister).await, before);

        // A new occupant of the exact cursor key stays behind its exclusive
        // position. A fresh enumeration must nevertheless see its new payload.
        write(1, first_category, 999, true).await;
        let before = pic.get_stable_memory(canister).await;
        upgrade_stopped(&pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(page(None).await.0, [(1, 999)]);
        let after_reinsert = page(Some(original.clone())).await;
        assert_eq!(after_reinsert.0, after_delete.0);
        assert_eq!(after_reinsert.1, after_delete.1);
        let mut cursor = Some(original);
        let mut actual = Vec::new();
        for _ in 0..8 {
            let next = page(cursor.clone()).await;
            assert!(next.2 <= 1 && next.3 <= 1 && next.4 <= 16);
            if next.1.is_some() {
                assert_ne!(next.1, cursor);
            }
            actual.extend(next.0);
            cursor = next.1;
            if cursor.is_none() {
                break;
            }
        }
        assert!(cursor.is_none());
        assert_eq!(
            actual,
            expected.into_iter().map(|id| (id, 100)).collect::<Vec<_>>()
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    pic.drop().await;
}
