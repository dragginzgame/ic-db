//! Independent complete-sort/projection oracles for bounded live reads and ordered DISTINCT.

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

type Cursor = (Vec<u8>, [u64; 5]);
type Row = (u64, Option<u64>, u64);
type Page<T> = (Vec<T>, Option<Cursor>, u64, u64);
type Request = (u64, u64, u64, Option<Cursor>, u64, u64, u64);

#[tokio::test]
async fn bounded_reads() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M0_WASM").expect("run make m0-reads")).unwrap();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let seed = async || -> u64 {
        let reply = pic
            .update_call(
                canister,
                canister,
                "seed_reads",
                ic_wire::encode(
                    &(),
                    Encoding::Frame {
                        method: 4,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode(
            &reply,
            Encoding::Frame {
                method: 4,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let page = async |request: Request| -> (Result<Page<Row>, u8>, usize) {
        let reply = pic
            .query_call(
                canister,
                canister,
                "bounded_page",
                ic_wire::encode(
                    &request,
                    Encoding::Frame {
                        method: 6,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        (
            ic_wire::decode(
                &reply,
                Encoding::Frame {
                    method: 6,
                    direction: Direction::Response,
                },
                LIMITS,
            )
            .unwrap(),
            reply.len(),
        )
    };
    let distinct = async |token: Option<Cursor>| -> Page<Option<u64>> {
        let reply = pic
            .query_call(
                canister,
                canister,
                "distinct_page",
                ic_wire::encode(
                    &(token, 1_u64, 1_u64),
                    Encoding::Frame {
                        method: 7,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        ic_wire::decode::<Result<Page<Option<u64>>, u8>>(
            &reply,
            Encoding::Frame {
                method: 7,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
        .unwrap()
    };
    let change = async |id: u64, value: Option<(u64, u64)>| {
        let reply = pic
            .update_call(
                canister,
                canister,
                "change_read_row",
                ic_wire::encode(
                    &(id, value),
                    Encoding::Frame {
                        method: 5,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            ic_wire::decode::<Result<(), u8>>(
                &reply,
                Encoding::Frame {
                    method: 5,
                    direction: Direction::Response
                },
                LIMITS
            )
            .unwrap(),
            Ok(())
        );
    };
    seed().await;
    let source: Vec<Row> = vec![
        (1, Some(30), 20000),
        (2, Some(10), 90),
        (3, Some(30), 10),
        (4, None, 90),
        (5, Some(10), 10),
        (6, Some(40), 10),
        (7, None, 90),
        (8, Some(20), 10),
    ];
    let mut ordered = source.clone();
    ordered.sort_by_key(|&(id, score, _)| (score, id));
    let mut misleading_prefix = source[..2].to_vec();
    misleading_prefix.sort_by_key(|&(id, score, _)| (score, id));
    assert_ne!(misleading_prefix, ordered[..2]);
    assert_eq!(page((1, 0, 20000, None, 8, 2, 1024)).await.0, Err(4));
    assert_eq!(page((0, 1, 20000, None, 8, 2, 1024)).await.0, Err(4));
    assert_eq!(
        page((1, 1, 20000, None, 8, 8, 1024)).await.0.unwrap().0,
        ordered
    );
    assert_eq!(
        page((0, 0, 20000, None, 8, 8, 1024)).await.0.unwrap().0,
        source
    );
    assert_eq!(page((0, 0, 20000, None, 0, 8, 1024)).await.0, Err(1));
    assert_eq!(page((0, 0, 20000, None, 8, 8, 1)).await.0, Err(1));
    let before_reads = pic.get_stable_memory(canister).await;

    // Sparse residual predicates produce empty pages but move past every processed candidate.
    let mut token = None;
    let mut results = Vec::new();
    let mut total_examined = 0;
    let mut empty_partial = 0;
    for _ in 0..10 {
        let result = page((1, 1, 20, token.clone(), 1, 3, 1024)).await.0.unwrap();
        assert!(result.2 <= 1 && result.3 <= 1);
        total_examined += result.2;
        if result.0.is_empty() && result.1.is_some() {
            empty_partial += 1;
        }
        if let (Some(previous), Some(next)) = (&token, &result.1) {
            assert!(next.1[4] > previous.1[4]);
        }
        results.extend(result.0);
        token = result.1;
        if token.is_none() {
            break;
        }
    }
    assert!(token.is_none());
    assert_eq!(total_examined, 8);
    assert!(empty_partial >= 3);
    assert_eq!(
        results,
        ordered
            .iter()
            .filter(|row| row.2 <= 20)
            .copied()
            .collect::<Vec<_>>()
    );

    // Size the budget from a real one-row Wire reply, then check actual returned bytes.
    let (first, first_bytes) = page((0, 0, 20000, None, 8, 1, 1024)).await;
    let first = first.unwrap();
    assert_eq!(first.0, source[..1]);
    assert_eq!(
        page((0, 0, 20000, None, 8, 8, (first_bytes - 1) as u64))
            .await
            .0,
        Err(3)
    );
    let mut token = None;
    let mut byte_limited = Vec::new();
    let mut retries = 0;
    for _ in 0..12 {
        let (result, encoded_bytes) = page((0, 0, 20000, token, 8, 8, first_bytes as u64)).await;
        let result = result.unwrap();
        assert!(encoded_bytes <= first_bytes);
        // Examined-but-not-returned qualifying rows must be retried, not skipped.
        if result.2 > result.0.len() as u64 {
            retries += 1;
        }
        byte_limited.extend(result.0);
        token = result.1;
        if token.is_none() {
            break;
        }
    }
    assert!(token.is_none());
    assert!(retries > 0);
    assert_eq!(byte_limited, source);

    // Ordered projection has a different row shape and needs no cross-page hash set.
    let mut token = None;
    let mut unique = Vec::new();
    let mut examined = 0;
    let mut duplicate_pages = 0;
    for _ in 0..10 {
        let result = distinct(token).await;
        assert_eq!(result.3, 0, "covered DISTINCT must not fetch rows");
        examined += result.2;
        if result.0.is_empty() && result.1.is_some() {
            duplicate_pages += 1;
        }
        unique.extend(result.0);
        token = result.1;
        if token.is_none() {
            break;
        }
    }
    assert!(token.is_none());
    let mut expected: Vec<_> = source.iter().map(|row| row.1).collect();
    expected.sort();
    expected.dedup();
    assert_eq!(unique, expected);
    assert_eq!(unique, vec![None, Some(10), Some(20), Some(30), Some(40)]);
    assert_eq!(examined, 8);
    assert_eq!(duplicate_pages, 3);
    assert!(
        before_reads == pic.get_stable_memory(canister).await,
        "queries add no durable state"
    );

    // Cursor shape/identity is validated; caller-supplied logical positions are not trusted receipts.
    let old = first.1.unwrap();
    assert_eq!(
        page((1, 1, 20000, Some(old.clone()), 8, 8, 1024)).await.0,
        Err(2)
    );
    assert_eq!(
        page((0, 0, 20, Some(old.clone()), 8, 8, 1024)).await.0,
        Err(2)
    );
    let mut bad = old.clone();
    bad.1[1] = 999;
    assert_eq!(page((0, 0, 20000, Some(bad), 8, 8, 1024)).await.0, Err(2));
    let mut bad = old.clone();
    bad.1[4] = 99999;
    assert_eq!(page((0, 0, 20000, Some(bad), 8, 8, 1024)).await.0, Err(2));
    let other = pic.create_canister().await;
    pic.add_cycles(other, 2_000_000_000_000).await;
    pic.install_canister(other, wasm.clone(), vec![], None)
        .await;
    pic.update_call(
        other,
        other,
        "seed_reads",
        ic_wire::encode(
            &(),
            Encoding::Frame {
                method: 4,
                direction: Direction::Request,
            },
            LIMITS,
        )
        .unwrap(),
    )
    .await
    .unwrap();
    let reply = pic
        .query_call(
            other,
            other,
            "bounded_page",
            ic_wire::encode(
                &(
                    0_u64,
                    0_u64,
                    20000_u64,
                    Some(old.clone()),
                    8_u64,
                    8_u64,
                    1024_u64,
                ),
                Encoding::Frame {
                    method: 6,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        ic_wire::decode::<Result<Page<Row>, u8>>(
            &reply,
            Encoding::Frame {
                method: 6,
                direction: Direction::Response
            },
            LIMITS
        )
        .unwrap(),
        Err(2)
    );
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(
        page((0, 0, 20000, Some(old.clone()), 8, 8, 1024))
            .await
            .0
            .unwrap()
            .0,
        source[1..]
    );
    seed().await;
    assert_eq!(page((0, 0, 20000, Some(old), 8, 8, 1024)).await.0, Err(2));

    // Live ordering allows a returned identity to repeat and an unseen identity to be missed.
    change(3, None).await;
    let first = page((1, 1, 20000, None, 8, 2, 1024)).await.0.unwrap();
    assert_eq!(first.0.iter().map(|r| r.0).collect::<Vec<_>>(), vec![4, 7]);
    change(4, Some((50, 90))).await; // Already returned, now beyond cursor.
    change(6, Some((0, 10))).await; // Unseen, now behind cursor.
    change(5, None).await;
    change(3, Some((5, 10))).await; // Newly inserted beyond cursor.
    let rest = page((1, 1, 20000, first.1, 8, 8, 1024)).await.0.unwrap();
    assert_eq!(
        rest.0.iter().map(|r| r.0).collect::<Vec<_>>(),
        vec![3, 2, 8, 1, 4]
    );
    seed().await;
    let first = page((1, 1, 20, None, 8, 1, 1024)).await.0.unwrap();
    assert_eq!(first.0[0].0, 5);
    change(2, Some((10, 10))).await; // Enters predicate behind cursor: omitted.
    change(1, Some((30, 10))).await; // Enters ahead: included.
    change(3, Some((30, 90))).await; // Leaves predicate ahead: excluded.
    let rest = page((1, 1, 20, first.1, 8, 8, 1024)).await.0.unwrap();
    assert_eq!(
        rest.0.iter().map(|r| r.0).collect::<Vec<_>>(),
        vec![8, 1, 6]
    );
    println!(
        "read contracts: order refusal, sparse progress, Wire-byte retry/refusal, null DISTINCT, cursor binding and live update/insert/delete cases passed"
    );
    owned.drop().await;
}
