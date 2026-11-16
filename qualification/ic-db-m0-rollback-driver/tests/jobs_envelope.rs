//! Cardinality, retained-byte and completed-output boundaries in the real engine.
use ic_db_m0_rollback_driver::{LIMITS, instance, upgrade_stopped};
use ic_wire::{Direction, Encoding};
use pocket_ic::{CanisterId, nonblocking::PocketIc};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;

mod artifact;

type Step = (u64, u64, u64, u64, u8, Option<String>, u64);
type Page = (u64, u8, Vec<(u64, u64)>, Option<Vec<u8>>, u64);

async fn call<T: DeserializeOwned>(
    pic: &PocketIc,
    canister: CanisterId,
    name: &str,
    method: u64,
    request: &impl Serialize,
) -> T {
    let reply = pic
        .update_call(
            canister,
            canister,
            name,
            ic_wire::encode(
                request,
                Encoding::Frame {
                    method,
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
            method,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap()
}

async fn advance(
    pic: &PocketIc,
    canister: CanisterId,
    handle: &[u8],
    operation: u8,
    sequence: u64,
    work: u32,
) -> Result<Step, String> {
    let before = pic.cycle_balance(canister).await;
    let (result, instructions): (Result<Step, String>, u64) = call(
        pic,
        canister,
        "job_advance_cost",
        115,
        &(handle, operation, sequence, work, 1_i64, 0_u64, false),
    )
    .await;
    let cycles = before
        .checked_sub(pic.cycle_balance(canister).await)
        .unwrap();
    eprintln!(
        "job-envelope op={operation} seq={sequence} work={work} progress={:?} instructions={instructions} cycles={cycles}",
        result.as_ref().map(|s| (s.0, s.2, s.4))
    );
    assert!(instructions > 0 && cycles > 0);
    result
}

#[tokio::test]
async fn retained_limits_finalization_and_pages_preserve_exact_checkpoints() {
    let (wasm, facts) = artifact::read(
        std::fs::File::open(
            std::env::var_os("IC_DB_M2_JOBS_WASM").expect("run make m2-job-envelope"),
        )
        .unwrap(),
    )
    .unwrap();
    eprintln!(
        "job-envelope wasm_bytes={} code_bytes={} sha256={}",
        facts.identity.bytes,
        facts.code_section_bytes,
        facts
            .identity
            .sha256
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    assert!(facts.exports.contains("canister_update job_advance_cost"));
    assert!(facts.exports.contains("canister_update job_page_cost"));
    assert!(facts.exports.contains("canister_update job_control_cost"));
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let start = async |operation: u8| {
        call::<Result<Vec<u8>, String>>(
            pic,
            canister,
            "job_start",
            110,
            &(operation, 0_i64, 100_i64, 0_u64),
        )
        .await
        .unwrap()
    };
    let cancel = async |handle: &[u8]| {
        call::<Result<u64, String>>(
            pic,
            canister,
            "job_control",
            112,
            &(0_u8, handle, 1_i64, 1_u32),
        )
        .await
        .unwrap();
    };
    let write = async |id: u64, category: u64, amount: u64| {
        call::<Result<(), u8>>(
            pic,
            canister,
            "write",
            4,
            &(id, category, amount, u8::from(id > 6)),
        )
        .await
        .unwrap();
    };
    let upgrade = async || {
        let before = pic.get_stable_memory(canister).await;
        upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(pic.get_stable_memory(canister).await, before);
    };
    let page = async |handle: &[u8], sequence: u64, after: &Option<Vec<u8>>, bytes: u32| {
        let before = pic.cycle_balance(canister).await;
        let (result, instructions): (Result<Page, String>, u64) = call(
            pic,
            canister,
            "job_page_cost",
            116,
            &(handle, sequence, after, 3_u32, bytes, 1_i64, 0_u64, false),
        )
        .await;
        let cycles = before
            .checked_sub(pic.cycle_balance(canister).await)
            .unwrap();
        eprintln!(
            "job-envelope page seq={sequence} after={} bytes={bytes} instructions={instructions} cycles={cycles}",
            after.is_some()
        );
        assert!(instructions > 0 && cycles > 0);
        result
    };

    // Exactly 32 groups with unequal counts and ties. Independent tuples define
    // the result, including HAVING, aggregate order and key tie-breaking.
    let mut counts = BTreeMap::<u64, u64>::new();
    let mut sums = BTreeMap::<u64, u64>::new();
    let mut rows = 0_u64;
    for category in 0..32 {
        for _ in 0..1 + category % 5 {
            rows += 1;
            write(rows, category, rows * 10).await;
            *counts.entry(category).or_default() += 1;
            *sums.entry(category).or_default() += rows * 10;
        }
    }
    // Exercise arithmetic finalization at the same maximum group cardinality.
    // Consecutive multiples of ten have integral means, avoiding reuse of the
    // engine's decimal arithmetic or encoding in the expected result.
    for operation in [4, 5] {
        let expected = counts
            .iter()
            .map(|(key, count)| {
                if operation == 4 {
                    format!("{key}:{}", sums[key])
                } else {
                    assert_eq!(sums[key] % count, 0);
                    format!("{key}:{}", sums[key] / count)
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let handle = start(operation).await;
        let prefix = advance(pic, canister, &handle, operation, 0, rows as u32)
            .await
            .unwrap();
        assert_eq!((prefix.2, prefix.4), (rows, 0));
        let complete = advance(pic, canister, &handle, operation, prefix.0, 1)
            .await
            .unwrap();
        assert_eq!(
            (complete.2, complete.4, complete.5.as_deref()),
            (rows, 1, Some(expected.as_str()))
        );
        cancel(&handle).await;
    }
    let mut expected: Vec<_> = counts.into_iter().filter(|(_, n)| *n >= 2).collect();
    expected.sort_by_key(|(key, count)| (std::cmp::Reverse(*count), *key));
    assert_eq!(expected.len(), 25);
    let grouped = start(3).await;
    let distinct = start(8).await;
    let mut sequences = [0, 0];
    let mut examined = 0;
    while examined < rows {
        let work = (rows - examined).min(16) as u32;
        let step = advance(pic, canister, &grouped, 3, sequences[0], work)
            .await
            .unwrap();
        examined += u64::from(work);
        assert_eq!((step.2, step.4, step.5), (examined, 0, None));
        sequences[0] = step.0;
    }
    // Duplicates at the 32-key limit must still be admissible.
    let step = advance(pic, canister, &distinct, 8, 0, rows as u32)
        .await
        .unwrap();
    assert_eq!((step.2, step.4), (rows, 0));
    sequences[1] = step.0;
    upgrade().await;
    let before = pic.get_stable_memory(canister).await;
    assert!(page(&grouped, sequences[0], &None, 1).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    // A stale sequence reads the saved prefix without advancing or double counting.
    let retry = advance(pic, canister, &grouped, 3, sequences[0] - 1, 16)
        .await
        .unwrap();
    assert_eq!((retry.0, retry.2, retry.4), (sequences[0], rows, 0));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let mut after = None;
    let mut actual = Vec::new();
    let pages = expected.len().div_ceil(3);
    for index in 0..pages {
        let p = page(&grouped, sequences[0], &after, 128).await.unwrap();
        assert_eq!(p.1, 1);
        sequences[0] = p.0;
        actual.extend_from_slice(&p.2);
        // Completed output replay is exact, including across heap loss.
        let backing = pic.get_stable_memory(canister).await;
        if after.is_none() {
            upgrade().await;
        }
        assert_eq!(page(&grouped, sequences[0], &after, 128).await.unwrap(), p);
        assert_eq!(pic.get_stable_memory(canister).await, backing);
        after = p.3;
        assert_eq!(after.is_some(), index + 1 < pages);
    }
    assert_eq!(actual, expected);
    let complete = advance(pic, canister, &distinct, 8, sequences[1], 16)
        .await
        .unwrap();
    assert_eq!(
        (complete.2, complete.4, complete.5.as_deref()),
        (rows, 1, Some("32"))
    );
    cancel(&grouped).await;
    cancel(&distinct).await;

    // The 33rd group/key is refused without erasing an accepted checkpoint.
    write(rows + 1, 32, 1).await;
    for operation in [3, 8] {
        let handle = start(operation).await;
        let saved = advance(pic, canister, &handle, operation, 0, rows as u32)
            .await
            .unwrap();
        let before = pic.get_stable_memory(canister).await;
        assert!(
            advance(pic, canister, &handle, operation, saved.0, 1)
                .await
                .is_err()
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
        upgrade().await;
        assert!(
            advance(pic, canister, &handle, operation, saved.0, 1)
                .await
                .is_err()
        );
        assert_eq!(pic.get_stable_memory(canister).await, before);
        let retry = advance(pic, canister, &handle, operation, 0, 1)
            .await
            .unwrap();
        assert_eq!((retry.0, retry.2, retry.4), (saved.0, rows, 0));
        cancel(&handle).await;
        // Explicit cancellation releases capacity for a different, completable job.
        let count = start(0).await;
        let step = advance(pic, canister, &count, 0, 0, 128).await.unwrap();
        assert_eq!(
            (step.2, step.4, step.5),
            (rows + 1, 1, Some((rows + 1).to_string()))
        );
        cancel(&count).await;
    }

    // All 32 groups can now retain 32 numeric values each. A wider DISTINCT
    // tuple on the same input reaches the independent 16 KiB frame boundary.
    let bytes_canister = pic.create_canister().await;
    pic.add_cycles(bytes_canister, 10_000_000_000_000).await;
    pic.install_canister(bytes_canister, wasm.clone(), vec![], None)
        .await;
    for id in 1_u64..=1024 {
        call::<Result<(), u8>>(
            pic,
            bytes_canister,
            "write",
            4,
            &(id, (id - 1) % 32, (id - 1) / 32, u8::from(id > 6)),
        )
        .await
        .unwrap();
    }
    let numeric = call::<Result<Vec<u8>, String>>(
        pic,
        bytes_canister,
        "job_start",
        110,
        &(16_u8, 0_i64, 100_i64, 0_u64),
    )
    .await
    .unwrap();
    let mut sequence = 0;
    for examined in (32..=1024).step_by(32) {
        let step = advance(pic, bytes_canister, &numeric, 16, sequence, 32)
            .await
            .unwrap();
        assert_eq!((step.2, step.4), (examined, 0));
        sequence = step.0;
        if examined == 512 {
            upgrade_stopped(pic, bytes_canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
        }
    }
    let done = advance(pic, bytes_canister, &numeric, 16, sequence, 1)
        .await
        .unwrap();
    let expected = (0..32)
        .map(|key| format!("{key}:32"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!((done.2, done.4, done.5), (1024, 1, Some(expected)));
    call::<Result<u64, String>>(
        pic,
        bytes_canister,
        "job_control",
        112,
        &(0_u8, &numeric, 1_i64, 1_u32),
    )
    .await
    .unwrap();
    let handle = call::<Result<Vec<u8>, String>>(
        pic,
        bytes_canister,
        "job_start",
        110,
        &(17_u8, 0_i64, 100_i64, 0_u64),
    )
    .await
    .unwrap();
    let mut sequence = 0;
    let mut examined = 0;
    loop {
        let before = pic.get_stable_memory(bytes_canister).await;
        match advance(pic, bytes_canister, &handle, 17, sequence, 16).await {
            Ok(step) => {
                assert_eq!(
                    step.4, 0,
                    "byte-bound witness must refuse before completion"
                );
                sequence = step.0;
                examined = step.2;
            }
            Err(_) => {
                assert!((512..1024).contains(&examined));
                assert_eq!(pic.get_stable_memory(bytes_canister).await, before);
                upgrade_stopped(pic, bytes_canister, wasm.clone(), vec![], None)
                    .await
                    .unwrap();
                assert!(
                    advance(pic, bytes_canister, &handle, 17, sequence, 16)
                        .await
                        .is_err()
                );
                assert_eq!(pic.get_stable_memory(bytes_canister).await, before);
                break;
            }
        }
    }
    // Recover remaining usable capacity one row at a time, then prove the same
    // unchanged input refuses even at minimum work; this is a state bound.
    loop {
        let before = pic.get_stable_memory(bytes_canister).await;
        match advance(pic, bytes_canister, &handle, 17, sequence, 1).await {
            Ok(step) => {
                assert_eq!((step.2, step.4), (examined + 1, 0));
                sequence = step.0;
                examined = step.2;
            }
            Err(_) => {
                assert_eq!(pic.get_stable_memory(bytes_canister).await, before);
                break;
            }
        }
    }
    eprintln!(
        "job-envelope whole-frame last accepted candidates={examined}; next candidate refused with 32 groups and fewer than 32 distinct tuples per group"
    );
    call::<Result<u64, String>>(
        pic,
        bytes_canister,
        "job_control",
        112,
        &(0_u8, &handle, 1_i64, 1_u32),
    )
    .await
    .unwrap();
    let fresh = call::<Result<Vec<u8>, String>>(
        pic,
        bytes_canister,
        "job_start",
        110,
        &(0_u8, 0_i64, 100_i64, 0_u64),
    )
    .await
    .unwrap();
    assert_ne!(fresh, handle);
    let prefix = advance(pic, bytes_canister, &fresh, 0, 0, 512)
        .await
        .unwrap();
    assert_eq!((prefix.2, prefix.4), (512, 0));
    let prefix = advance(pic, bytes_canister, &fresh, 0, prefix.0, 512)
        .await
        .unwrap();
    assert_eq!((prefix.2, prefix.4), (1024, 0));
    let done = advance(pic, bytes_canister, &fresh, 0, prefix.0, 1)
        .await
        .unwrap();
    assert_eq!((done.2, done.4, done.5.as_deref()), (1024, 1, Some("1024")));
    call::<Result<u64, String>>(
        pic,
        bytes_canister,
        "job_control",
        112,
        &(0_u8, &fresh, 1_i64, 1_u32),
    )
    .await
    .unwrap();
    full_jobs_cleanup(pic, bytes_canister, &wasm, examined).await;
    owned.drop().await;
}

// Reuse the independently established largest checkpoint for this tuple shape.
// All sixteen slots, including invalidated state, must remain safely reclaimable.
async fn full_jobs_cleanup(pic: &PocketIc, canister: CanisterId, wasm: &[u8], retained_rows: u64) {
    let start = async |operation: u8, now: i64, expires: i64| {
        call::<Result<Vec<u8>, String>>(
            pic,
            canister,
            "job_start",
            110,
            &(operation, now, expires, 0_u64),
        )
        .await
    };
    let control = async |operation: u8,
                         handle: &[u8],
                         now: i64,
                         limit: u32,
                         fault: u64,
                         trap: bool| {
        let before = pic.cycle_balance(canister).await;
        let reply = pic
            .update_call(
                canister,
                canister,
                "job_control_cost",
                ic_wire::encode(
                    &((operation, handle, now, limit), fault, trap),
                    Encoding::Frame {
                        method: 117,
                        direction: Direction::Request,
                    },
                    LIMITS,
                )
                .unwrap(),
            )
            .await;
        let cycles = before
            .checked_sub(pic.cycle_balance(canister).await)
            .unwrap();
        let bytes = reply?;
        let (result, instructions, writes): (Result<u64, String>, u64, u64) = ic_wire::decode(
            &bytes,
            Encoding::Frame {
                method: 117,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap();
        eprintln!(
            "job-lifecycle op={operation} now={now} limit={limit} result={result:?} instructions={instructions} cycles={cycles} writes={writes}"
        );
        assert!(instructions > 0 && cycles > 0);
        Ok::<_, pocket_ic::RejectResponse>((result, writes))
    };
    let fill = async |handle: &[u8]| {
        let prefix = advance(pic, canister, handle, 17, 0, 512).await.unwrap();
        let full = advance(
            pic,
            canister,
            handle,
            17,
            prefix.0,
            u32::try_from(retained_rows - 512).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!((full.2, full.4, full.5), (retained_rows, 0, None));
        full.0
    };
    let mut handles = Vec::new();
    let mut sequences = Vec::new();
    for _ in 0..16 {
        let handle = start(17, 0, 100).await.unwrap();
        sequences.push(fill(&handle).await);
        handles.push(handle);
    }
    let full = pic.get_stable_memory(canister).await;
    assert!(start(17, 0, 100).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, full);
    upgrade_stopped(pic, canister, wasm.to_vec(), vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, full);

    // A source change invalidates an unfinished result without dropping its
    // retained payload or freeing the slot implicitly. Cleanup must handle it.
    control(2, &[], 1, 1, 0, false).await.unwrap().0.unwrap();
    let invalid = advance(pic, canister, &handles[0], 17, sequences[0], 1)
        .await
        .unwrap();
    assert_eq!(
        (invalid.0, invalid.2, invalid.4, invalid.5),
        (sequences[0] + 1, retained_rows, 2, None)
    );
    let before = pic.get_stable_memory(canister).await;
    assert!(start(17, 0, 100).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);

    // Cancellation also rolls back after a supplier write. Replacement reuses
    // logical capacity without allowing the old handle to alias the new job.
    assert!(control(0, &handles[1], 1, 1, 1, false).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        control(0, &handles[1], 1, 1, 0, false)
            .await
            .unwrap()
            .0
            .unwrap(),
        0
    );
    let old = handles[1].clone();
    handles[1] = start(17, 1, 100).await.unwrap();
    assert!(!handles.iter().any(|handle| handle == &old));
    fill(&handles[1]).await;
    let before = pic.get_stable_memory(canister).await;
    assert!(control(0, &old, 1, 1, 0, false).await.unwrap().0.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(control(1, &[], 99, 16, 0, false).await.unwrap(), (Ok(0), 0));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(control(1, &[], 100, 0, 0, false).await.unwrap().0.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);

    // A cleanup with several removals either commits the chunk or rolls it back.
    assert!(control(1, &[], 100, 3, 1, false).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert!(control(1, &[], 100, 3, 0, true).await.is_err());
    assert_eq!(pic.get_stable_memory(canister).await, before);
    upgrade_stopped(pic, canister, wasm.to_vec(), vec![], None)
        .await
        .unwrap();
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for (limit, removed) in [(1, 1), (3, 3), (16, 12), (16, 0)] {
        assert_eq!(
            control(1, &[], 100, limit, 0, false)
                .await
                .unwrap()
                .0
                .unwrap(),
            removed
        );
    }
    // Every slot can be occupied again. Stale cancellation must preserve all
    // replacements, and expiry remains the only automatic cleanup criterion.
    let mut replacements = Vec::new();
    for _ in 0..16 {
        let fresh = start(17, 100, 200).await.unwrap();
        assert!(!handles.contains(&fresh) && fresh != old);
        assert!(!replacements.contains(&fresh));
        // Fill again at the new host time so a maximum cleanup batch also
        // exercises sixteen large frames, rather than empty count jobs.
        for (sequence, work) in [
            (0_u64, 512_u32),
            (1, u32::try_from(retained_rows - 512).unwrap()),
        ] {
            let (step, _): (Result<Step, String>, u64) = call(
                pic,
                canister,
                "job_advance_cost",
                115,
                &(&fresh, 17_u8, sequence, work, 101_i64, 0_u64, false),
            )
            .await;
            let step = step.unwrap();
            assert_eq!(step.4, 0);
            assert_eq!(step.2, if sequence == 0 { 512 } else { retained_rows });
        }
        replacements.push(fresh);
    }
    let before = pic.get_stable_memory(canister).await;
    for handle in &handles {
        assert!(
            control(0, handle, 100, 1, 0, false)
                .await
                .unwrap()
                .0
                .is_err()
        );
    }
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        control(1, &[], 199, 16, 0, false).await.unwrap(),
        (Ok(0), 0)
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        control(1, &[], 200, 16, 0, false).await.unwrap().0.unwrap(),
        16
    );
    eprintln!(
        "job-lifecycle: 16 near-limit frames invalidated/cancelled/expired; rollback, upgrade and full-slot reuse passed"
    );
}
