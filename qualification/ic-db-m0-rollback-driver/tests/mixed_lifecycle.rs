//! Independent map oracle across mixed writes, exact jobs, upgrade and maintenance.
//! No product dependency, schema descriptors, codecs or query execution in the oracle.

mod artifact;

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};

use ic_db_qualification_transport::mixed::{
    Command, CountAction, CountOutcome, CountReply, CountRequest, ItemPage, Query, QueryReply,
    Reply, Snapshot, Update,
};

#[derive(Clone, Default, PartialEq, Eq, Debug)]
struct Model {
    parents: BTreeMap<u64, u64>,
    items: BTreeMap<u64, (u64, u128)>,
    links: BTreeMap<u64, u64>,
    revisions: [u64; 3],
}
impl Model {
    fn apply(&mut self, commands: &[Command]) -> Result<(), u8> {
        let mut next = self.clone();
        let mut addressed = BTreeSet::new();
        for &(entity, operation, id, a, b) in commands {
            if !addressed.insert((entity, id)) {
                return Err(3);
            }
            let present = match entity {
                0 => self.parents.contains_key(&id),
                1 => self.items.contains_key(&id),
                2 => self.links.contains_key(&id),
                _ => panic!("invalid oracle entity"),
            };
            if operation == 0 && present {
                return Err(1);
            }
            if matches!(operation, 1 | 3) && !present {
                return Err(2);
            }

            match (entity, operation) {
                (0, 2) => {
                    next.parents.remove(&id);
                }
                (0, 0 | 1) => {
                    next.parents.insert(id, a);
                }
                (1, 2) => {
                    next.items.remove(&id);
                }
                (1, 0 | 1) => {
                    next.items.insert(id, (a, u128::from(b)));
                }
                (1, 3) => {
                    next.items.get_mut(&id).unwrap().1 += u128::from(b);
                }
                (2, 2) => {
                    next.links.remove(&id);
                }
                (2, 0 | 1) => {
                    next.links.insert(id, a);
                }
                _ => panic!("invalid oracle command"),
            }
        }
        if next.parents.values().collect::<BTreeSet<_>>().len() != next.parents.len() {
            return Err(4);
        }
        if next
            .items
            .values()
            .any(|(parent, _)| !next.parents.contains_key(parent))
            || next
                .links
                .values()
                .any(|item| !next.items.contains_key(item))
        {
            return Err(5);
        }
        for (entity, changed) in [
            next.parents != self.parents,
            next.items != self.items,
            next.links != self.links,
        ]
        .into_iter()
        .enumerate()
        {
            next.revisions[entity] += u64::from(changed);
        }
        *self = next;

        Ok(())
    }

    fn group_counts(&self) -> Vec<(u64, u64)> {
        let mut groups = BTreeMap::<u64, u64>::new();
        for &(parent, _) in self.items.values() {
            *groups.entry(parent).or_default() += 1;
        }
        groups.into_iter().collect()
    }

    fn snapshot(&self, parent: u64, minimum: u64) -> Snapshot {
        let mut ordered: Vec<_> = self
            .items
            .iter()
            .map(|(&id, &(_, score))| (score, id))
            .collect();
        ordered.sort_unstable();

        (
            self.parents.iter().map(|(&id, &code)| (id, code)).collect(),
            self.items
                .iter()
                .map(|(&id, &(parent, score))| (id, parent, score))
                .collect(),
            self.links.iter().map(|(&id, &item)| (id, item)).collect(),
            [
                self.parents.len() as u64,
                self.items.len() as u64,
                self.links.len() as u64,
            ],
            self.revisions,
            self.items
                .iter()
                .filter(|(_, row)| row.0 == parent && row.1 >= u128::from(minimum))
                .map(|(&id, _)| id)
                .collect(),
            self.group_counts(),
            ordered.into_iter().map(|(_, id)| id).collect(),
        )
    }
}
fn encode<T: Serialize>(id: u64, value: &T) -> Vec<u8> {
    ic_wire::encode(
        value,
        Encoding::Frame {
            method: id,
            direction: Direction::Request,
        },
        LIMITS,
    )
    .unwrap()
}

fn decode<T: DeserializeOwned>(id: u64, bytes: &[u8]) -> T {
    ic_wire::decode(
        bytes,
        Encoding::Frame {
            method: id,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap()
}

fn application_request<T: Serialize>(method: u64, value: &T) -> Vec<u8> {
    ic_wire::encode(
        value,
        ic_wire::Encoding::Frame {
            method,
            direction: ic_wire::Direction::Request,
        },
        canic::db::RPC_LIMITS,
    )
    .unwrap()
}

fn application_reply<T: DeserializeOwned>(method: u64, bytes: &[u8]) -> T {
    ic_wire::decode(
        bytes,
        ic_wire::Encoding::Frame {
            method,
            direction: ic_wire::Direction::Response,
        },
        canic::db::RPC_LIMITS,
    )
    .unwrap()
}

fn seed_batches() -> [(&'static str, Vec<Command>); 2] {
    [
        (
            "seed-parents",
            (1..=4).map(|id| (0, 0, id, id * 10, 0)).collect(),
        ),
        (
            "seed-dependants",
            (1..=12)
                .flat_map(|id| [(2, 0, id, id, 0), (1, 0, id, id % 4 + 1, id * 10)])
                .collect(),
        ),
    ]
}

#[tokio::test]
async fn compare_retained_count_advance_at_the_same_checkpoint() {
    use ic_testkit::pocket_ic::Time;

    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M4_WASM").expect("run make m4-lifecycle")).unwrap();
    // Fix both clocks so the job generation, expiry and observed timestamps agree.
    let clock = |offset_seconds| {
        Time::from_nanos_since_unix_epoch((2_000_000_000_u64 + offset_seconds) * 1_000_000_000)
    };
    let mut warm_observation = None;
    for upgraded in [false, true] {
        let pic = instance().await.unwrap();
        pic.set_time(clock(0)).await;
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 10_000_000_000_000).await;
        pic.install_canister(canister, wasm.clone(), vec![], None)
            .await;
        let call = async |request| {
            let bytes = pic
                .update_call(
                    canister,
                    canister,
                    "db_write",
                    application_request(241, &request),
                )
                .await
                .unwrap();

            application_reply::<Reply>(241, &bytes)
        };
        for (_, commands) in seed_batches() {
            assert!(matches!(
                call(Update::Batch(false, commands)).await,
                Reply::Batch(Ok(()), _)
            ));
        }
        let count = async |action| {
            let Reply::Count(result, instructions) =
                call(Update::Count(CountRequest::Run(false, action))).await
            else {
                panic!("expected count reply")
            };

            (result.unwrap(), instructions)
        };
        let (CountReply::Started(handle), _) = count(CountAction::Start).await else {
            panic!("expected handle")
        };
        let advance =
            async |sequence| count(CountAction::Resume(handle.clone(), sequence, 2)).await;
        assert_eq!(
            advance(0).await.0,
            CountReply::Advanced(1, CountOutcome::Running)
        );
        let checkpoint = pic.get_stable_memory(canister).await;
        pic.set_time(clock(600)).await;
        if upgraded {
            ic_db_m0_rollback_driver::upgrade_stopped(&pic, canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
            assert!(pic.get_stable_memory(canister).await == checkpoint);
        }
        pic.set_time(clock(601)).await;
        // Settle the simulated idle interval before sampling the update balance.
        pic.tick().await;
        let before = pic.cycle_balance(canister).await;
        let (result, instructions) = advance(1).await;
        let cycles = before - pic.cycle_balance(canister).await;
        assert_eq!(result, CountReply::Advanced(2, CountOutcome::Running));
        let next_checkpoint = pic.get_stable_memory(canister).await;
        assert!(next_checkpoint != checkpoint);
        let observation = (handle.clone(), checkpoint, next_checkpoint);
        if let Some(reference) = &warm_observation {
            assert!(
                reference == &observation,
                "handle and both checkpoints must match byte for byte"
            );
        } else {
            warm_observation = Some(observation);
        }
        println!(
            "opening/count-advance upgraded={upgraded}: handler_instructions={instructions} update_balance_delta={cycles}"
        );
        for sequence in 2..=5 {
            let expected = if sequence == 5 {
                CountOutcome::Complete(12)
            } else {
                CountOutcome::Running
            };
            assert_eq!(
                advance(sequence).await.0,
                CountReply::Advanced(sequence + 1, expected)
            );
        }
        pic.drop().await;
    }
}

#[tokio::test]
#[ignore = "compares the records and mixed-lifecycle release artifacts"]
async fn compare_records_host_composition() {
    for (variant, variable, mounted) in [
        ("records", "IC_DB_M1_RECORDS_WASM", false),
        ("mixed", "IC_DB_M4_WASM", true),
    ] {
        let path = std::env::var_os(variable).expect(variable);
        let (wasm, facts) = artifact::read(std::fs::File::open(path).unwrap()).unwrap();
        assert_eq!(facts.exports.contains("canister_update db_write"), mounted);
        assert_eq!(facts.exports.contains("canister_query db_query"), mounted);
        assert!(!facts.exports.contains("canister_update fault_batch"));
        let endpoints = facts
            .exports
            .iter()
            .filter(|name| {
                name.starts_with("canister_query ") || name.starts_with("canister_update ")
            })
            .count();
        let pic = instance().await.unwrap();
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 10_000_000_000_000).await;
        let before = pic.cycle_balance(canister).await;
        pic.install_canister(canister, wasm.clone(), vec![], None)
            .await;
        let installed = pic.cycle_balance(canister).await;
        // Replenish the install-code allowance outside the measured upgrade interval.
        pic.advance_time(std::time::Duration::from_secs(600)).await;
        pic.tick().await;
        let before_upgrade = pic.cycle_balance(canister).await;
        let backing = pic.get_stable_memory(canister).await;
        ic_db_m0_rollback_driver::upgrade_stopped(&pic, canister, wasm, vec![], None)
            .await
            .unwrap();
        let upgraded = pic.cycle_balance(canister).await;
        assert!(pic.get_stable_memory(canister).await == backing);
        let category = async |key: u64| {
            let bytes = pic
                .query_call(canister, canister, "category", encode(5, &key))
                .await
                .unwrap();
            decode::<Vec<(u64, u64)>>(5, &bytes)
        };
        assert_eq!(category(1).await, [(1, 11), (3, 31), (5, 51)]);
        // Restore the original row between measurements so both moves maintain
        // the same index entries with the same request and reply.
        for (phase, key, amount) in [
            ("first-after-upgrade", 0_u64, 12_u64),
            ("restore", 1, 11),
            ("warm", 0, 12),
        ] {
            let before_write = pic.cycle_balance(canister).await;
            let bytes = pic
                .update_call(
                    canister,
                    canister,
                    "write",
                    encode(4, &(1_u64, key, amount, 0_u8)),
                )
                .await
                .unwrap();
            let charge = before_write - pic.cycle_balance(canister).await;
            decode::<Result<(), u8>>(4, &bytes).unwrap();
            let (mut even, mut odd) = (vec![(2, 20), (4, 40), (6, 60)], vec![(3, 31), (5, 51)]);
            if key == 0 {
                even.insert(0, (1, amount));
            } else {
                odd.insert(0, (1, amount));
            }
            assert_eq!(category(0).await, even);
            assert_eq!(category(1).await, odd);
            println!("host-write variant={variant} phase={phase} cycles={charge}");
        }
        println!(
            "host-composition variant={variant} wasm={} code={} endpoints={endpoints} seeded_stable_bytes={} install_cycles={} stopped_upgrade_cycles={}",
            facts.identity.bytes,
            facts.code_section_bytes,
            backing.len(),
            before - installed,
            before_upgrade - upgraded
        );
        pic.drop().await;
    }
}

#[tokio::test]
async fn mixed_history_matches_independent_model_through_upgrade_and_copy() {
    let path = std::env::var_os("IC_DB_M4_WASM").expect("run make m4-lifecycle");
    let (wasm, facts) = artifact::read(std::fs::File::open(path).unwrap()).unwrap();
    assert!(facts.exports.contains("canister_update db_write"));
    assert!(!facts.exports.contains("canister_update mixed_job"));
    assert!(!facts.exports.contains("canister_query mixed_page"));
    assert!(!facts.exports.contains("canister_update fault_batch"));
    assert!(!facts.exports.contains("canister_query fault_status"));
    println!(
        "mixed-lifecycle host: {:?}, code={} bytes (includes existing qualification endpoints)",
        facts.identity, facts.code_section_bytes
    );
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let before = pic.get_stable_memory(canister).await;
    let anonymous = ic_testkit::pocket_ic::CanisterId::anonymous();
    let denied = pic
        .update_call(
            canister,
            anonymous,
            "db_write",
            application_request(241, &Update::Batch(false, vec![(0, 0, 1, 10, 0)])),
        )
        .await
        .unwrap_err();
    assert!(
        denied
            .reject_message
            .contains("mixed workload caller denied"),
        "{denied:?}"
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let update = async |method: &str, bytes: Vec<u8>| {
        pic.update_call(canister, canister, method, bytes)
            .await
            .unwrap()
    };
    let query = async |caller, request: Query| {
        pic.query_call(
            canister,
            caller,
            "db_query",
            application_request(242, &request),
        )
        .await
    };
    let read = async |target: bool, parent: u64, minimum: u64| {
        let bytes = query(canister, Query::Snapshot(target, parent, minimum))
            .await
            .unwrap();
        let QueryReply::Snapshot(result, instructions) = application_reply(242, &bytes) else {
            panic!("expected snapshot reply")
        };

        (result, instructions)
    };
    let items = async |target, minimum, work, after| {
        let bytes = query(
            canister,
            Query::Items {
                target,
                parent: 1,
                work,
                minimum,
                after,
            },
        )
        .await
        .unwrap();
        let QueryReply::Items(result, instructions) = application_reply(242, &bytes) else {
            panic!("expected items reply")
        };

        (result, instructions)
    };
    let upgrade = async || {
        let before = pic.get_stable_memory(canister).await;
        pic.advance_time(std::time::Duration::from_secs(600)).await;
        pic.tick().await;
        ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm.clone(), vec![], None)
            .await
            .unwrap();
        assert_eq!(pic.get_stable_memory(canister).await, before);
    };
    let write = async |target: bool, commands: &[Command]| {
        let bytes = update(
            "db_write",
            application_request(241, &Update::Batch(target, commands.to_vec())),
        )
        .await;
        let Reply::Batch(result, instructions) = application_reply(241, &bytes) else {
            panic!("expected batch reply")
        };

        (result, instructions)
    };
    let mut samples = BTreeMap::<&str, Vec<(u64, u128)>>::new();
    let mut read_samples = Vec::new();
    let mut check =
        async |model: &mut Model, target: bool, commands: Vec<Command>, label: &'static str| {
            let previous = model.clone();
            let expected = model.apply(&commands);
            let backing = pic.get_stable_memory(canister).await;
            let cycles = pic.cycle_balance(canister).await;
            let (actual, instructions) = write(target, &commands).await;
            let cycles = cycles - pic.cycle_balance(canister).await;
            assert_eq!(actual, expected, "{label}");
            if *model == previous {
                assert_eq!(
                    pic.get_stable_memory(canister).await,
                    backing,
                    "refusal/no-op: {label}"
                );
            }
            samples
                .entry(label)
                .or_default()
                .push((instructions, cycles));
            // Verify all rows and both aggregate/count paths after every attempted batch.
            for parent in 1..=4 {
                let (actual, instructions) = read(target, parent, 25).await;
                read_samples.push(instructions);
                assert_eq!(
                    actual.unwrap(),
                    model.snapshot(parent, 25),
                    "{label}, parent {parent}"
                );
            }
        };
    let mut model = Model::default();
    for (label, commands) in seed_batches() {
        check(&mut model, false, commands, label).await;
    }
    // Repeat identical operations on unchanged rows across one executable upgrade.
    // A query can initialize the host heap only for its own execution.
    let measured_page = async |phase: &str| {
        let (page, instructions) = items(false, 0, Some(1), None).await;
        let page = page.unwrap();
        assert_eq!(page.ids, [4]);
        assert_eq!(page.examined, 1);
        println!("opening/indexed-page {phase}: handler_instructions={instructions}");

        page.next.expect("more items for this parent")
    };
    let measured_batch = async |phase: &str| {
        let before = pic.cycle_balance(canister).await;
        let (result, instructions) = write(false, &[(1, 1, 1, 2, 10)]).await;
        let cycles = before - pic.cycle_balance(canister).await;
        assert_eq!(result, Ok(()));
        println!(
            "opening/identical-replace {phase}: handler_instructions={instructions} update_balance_delta={cycles}"
        );
    };
    let backing = pic.get_stable_memory(canister).await;
    let cursor = measured_page("seeded").await;
    measured_batch("seeded").await;
    upgrade().await;
    assert_eq!(measured_page("first-query-after-upgrade").await, cursor);
    assert_eq!(measured_page("second-query-after-upgrade").await, cursor);
    measured_batch("first-update-after-upgrade").await;
    assert_eq!(measured_page("after-first-update").await, cursor);
    measured_batch("after-first-update").await;
    assert_eq!(pic.get_stable_memory(canister).await, backing);
    // Inspection authorization must run before even a malformed frame is decoded.
    let refused = pic
        .query_call(canister, anonymous, "db_inspect", vec![])
        .await
        .unwrap_err();
    assert!(
        refused
            .reject_message
            .contains("mixed workload caller denied"),
        "{refused:?}"
    );
    use canic::db::inspection::{Admission, INSPECT, Response};
    let inspect = async || {
        let bytes = pic
            .query_call(
                canister,
                canister,
                "db_inspect",
                application_request(INSPECT, &()),
            )
            .await
            .unwrap();
        application_reply::<Response>(INSPECT, &bytes)
    };
    let inspected = inspect().await;
    assert_eq!(
        inspected.admission,
        Admission::Readable {
            incarnation: [21; 16]
        }
    );
    let job = async |request: CountRequest| {
        let bytes = update(
            "db_write",
            application_request(241, &Update::Count(request)),
        )
        .await;
        let Reply::Count(result, _) = application_reply(241, &bytes) else {
            panic!("expected count reply")
        };

        result
    };
    let start = async |target| {
        let CountReply::Started(handle) = job(CountRequest::Run(target, CountAction::Start))
            .await
            .unwrap()
        else {
            panic!("expected handle")
        };

        handle
    };
    let advance = async |target, handle: &[u8], sequence| {
        let CountReply::Advanced(next, outcome) = job(CountRequest::Run(
            target,
            CountAction::Resume(handle.to_vec(), sequence, 2),
        ))
        .await
        .unwrap() else {
            panic!("expected progress")
        };

        (next, outcome)
    };
    let finish_count = async |target, handle: &[u8], mut sequence, expected: u64| {
        for _ in 0..=expected {
            let (next, state) = advance(target, handle, sequence).await;
            match state {
                CountOutcome::Complete(value) => {
                    assert_eq!(value, expected);

                    return next;
                }
                CountOutcome::Running => assert!(next > sequence),
                unexpected => panic!("unexpected count outcome {unexpected:?}"),
            }
            sequence = next;
        }
        panic!("bounded count did not finish");
    };
    let handle = start(false).await;
    let before = pic.get_stable_memory(canister).await;
    let denied = pic
        .update_call(
            canister,
            anonymous,
            "db_write",
            application_request(241, &Update::Count(CountRequest::Cancel(handle.clone()))),
        )
        .await
        .unwrap_err();
    assert!(
        denied
            .reject_message
            .contains("mixed workload caller denied"),
        "{denied:?}"
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let (sequence, state) = advance(false, &handle, 0).await;
    assert_eq!(state, CountOutcome::Running);
    let mut cursor: Option<Vec<u8>> = None;
    let mut matches = Vec::new();
    let mut empty_continued = 0;
    for step in 0..4 {
        let (result, instructions) = items(false, 100, Some(1), cursor.clone()).await;
        let ItemPage {
            ids: rows,
            next,
            examined,
            ..
        } = result.unwrap();
        assert!(examined <= 1);
        println!(
            "sparse listing page {step}: examined={examined} returned={} instructions={instructions}",
            rows.len()
        );
        if rows.is_empty() && next.is_some() {
            empty_continued += 1;
        }
        matches.extend(rows);
        if next.is_none() {
            break;
        }
        assert_ne!(next, cursor);
        assert!(step < 3, "sparse listing must complete");
        if step == 0 {
            assert_eq!(empty_continued, 1);
            upgrade().await;
            let before = pic.get_stable_memory(canister).await;
            let denied = query(
                anonymous,
                Query::Items {
                    target: false,
                    parent: 1,
                    minimum: 100,
                    work: Some(1),
                    after: next.clone(),
                },
            )
            .await
            .unwrap_err();
            assert!(
                denied
                    .reject_message
                    .contains("mixed workload caller denied"),
                "{denied:?}"
            );
            assert!(matches!(
                items(false, 101, Some(1), next.clone()).await.0,
                Err(8)
            ));
            assert!(matches!(
                items(false, 100, Some(1), Some(vec![0xff])).await.0,
                Err(8)
            ));
            assert_eq!(pic.get_stable_memory(canister).await, before);
        }
        cursor = next;
    }
    assert_eq!(empty_continued, 2);
    assert_eq!(matches, model.snapshot(1, 100).5);
    check(
        &mut model,
        false,
        vec![(0, 1, 1, 999, 0)],
        "unrelated-write",
    )
    .await;
    check(&mut model, false, vec![(1, 1, 1, 2, 10)], "no-op").await;
    check(
        &mut model,
        false,
        vec![(1, 0, 500, 999, 1)],
        "refused-reference",
    )
    .await;
    upgrade().await;
    let (sequence, state) = advance(false, &handle, sequence).await;
    assert_eq!(
        state,
        CountOutcome::Running,
        "unrelated, refused and no-op writes must preserve input revision"
    );
    check(&mut model, false, vec![(1, 3, 1, 0, 1)], "read-modify").await;
    assert_eq!(
        advance(false, &handle, sequence).await.1,
        CountOutcome::Invalidated
    );
    assert_eq!(
        job(CountRequest::Cancel(handle)).await,
        Ok(CountReply::Cancelled)
    );
    let handle = start(false).await;
    let sequence = finish_count(false, &handle, 0, model.items.len() as u64).await;
    check(&mut model, false, vec![(1, 0, 50, 1, 50)], "insert-item").await;
    let (_, completed) = advance(false, &handle, sequence).await;
    assert_eq!(
        completed,
        CountOutcome::Complete(12),
        "completed result retains its original input"
    );
    assert_eq!(
        job(CountRequest::Cancel(handle)).await,
        Ok(CountReply::Cancelled)
    );
    check(&mut model, false, vec![(1, 2, 50, 0, 0)], "delete-item").await;

    let expiring = start(false).await;
    pic.advance_time(std::time::Duration::from_secs(3601)).await;
    pic.tick().await;
    assert_eq!(advance(false, &expiring, 0).await.1, CountOutcome::Expired);
    assert_eq!(
        job(CountRequest::Cancel(expiring.clone())).await,
        Ok(CountReply::Cancelled)
    );
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        job(CountRequest::Run(
            false,
            CountAction::Resume(expiring, 0, 2)
        ))
        .await,
        Err("InvalidJob".into())
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);

    // Fixed reproducible histories, with changing keys, values and batch order.
    for round in 0..12_u64 {
        let id = 100 + round;
        let base = round % 12 + 1;
        check(
            &mut model,
            false,
            vec![
                (2, 0, id, id, 0),
                (1, 0, id, id, round * 7),
                (0, 0, id, id * 100, 0),
            ],
            "insert-chain",
        )
        .await;
        check(
            &mut model,
            false,
            vec![(1, 3, base, 0, round + 1)],
            "read-modify",
        )
        .await;
        let score = u64::try_from(model.items[&base].1).unwrap();
        let new_parent = model.items[&base].0 % 4 + 1;
        check(
            &mut model,
            false,
            vec![(1, 1, base, new_parent, score)],
            "repoint",
        )
        .await;
        let (a, b) = (model.parents[&1], model.parents[&2]);
        check(
            &mut model,
            false,
            vec![(0, 1, 2, a, 0), (0, 1, 1, b, 0)],
            "unique-swap",
        )
        .await;
        check(
            &mut model,
            false,
            vec![(0, 0, id + 1000, a, 0)],
            "refused-unique",
        )
        .await;
        let parent = model.items[&base].0;
        check(
            &mut model,
            false,
            vec![(0, 2, parent, 0, 0)],
            "refused-parent-delete",
        )
        .await;
        check(
            &mut model,
            false,
            vec![(1, 0, id + 1000, 999, 0)],
            "refused-reference",
        )
        .await;
        check(
            &mut model,
            false,
            vec![(1, 2, base, 0, 0)],
            "refused-item-delete",
        )
        .await;
        check(
            &mut model,
            false,
            vec![(0, 2, id, 0, 0), (1, 2, id, 0, 0), (2, 2, id, 0, 0)],
            "delete-chain",
        )
        .await;
        check(
            &mut model,
            false,
            vec![(1, 3, base, 0, 1), (1, 3, base, 0, 2)],
            "refused-duplicate",
        )
        .await;
        let (parent, score) = model.items[&base];
        check(
            &mut model,
            false,
            vec![(1, 1, base, parent, u64::try_from(score).unwrap())],
            "no-op",
        )
        .await;
        check(
            &mut model,
            false,
            vec![(0, 0, id, 10000 + id, 0), (1, 2, 9999, 0, 0)],
            "insert-and-delete-absent",
        )
        .await;
        if round == 3 || round == 7 {
            upgrade().await;
            assert_eq!(read(false, 1, 25).await.0.unwrap(), model.snapshot(1, 25));
        }
    }
    let migrate = async |operation: u8| {
        decode::<(u64, bool, u64)>(244, &update("mixed_migrate", encode(244, &operation)).await)
    };
    // The dependent-first batch is valid on either live schema.
    let chain = [(2, 0, 51, 51, 0), (1, 0, 51, 51, 1051), (0, 0, 51, 510, 0)];
    // Keep the same logical request across the score widening and copy.
    let source_page = items(false, 1030, Some(1), None).await.0.unwrap();
    assert!(source_page.ids.is_empty());
    let source_cursor = source_page.next.expect("continued source page");
    let source_job = start(false).await;
    let (source_sequence, outcome) = advance(false, &source_job, 0).await;
    assert_eq!(outcome, CountOutcome::Running);
    let resume_source_job = async |target| {
        job(CountRequest::Run(
            target,
            CountAction::Resume(source_job.clone(), source_sequence, 2),
        ))
        .await
    };
    let expect_closed = async |target: bool| {
        let before = pic.get_stable_memory(canister).await;
        assert_eq!(resume_source_job(target).await, Err("Maintenance".into()));
        assert_eq!(write(target, &chain).await.0, Err(7));
        assert!(matches!(
            items(target, 1030, Some(1), Some(source_cursor.clone()))
                .await
                .0,
            Err(7)
        ));
        assert_eq!(pic.get_stable_memory(canister).await, before);
    };
    migrate(0).await;
    assert_eq!(
        inspect().await,
        Response {
            schema: inspected.schema,
            admission: Admission::Maintenance
        }
    );
    assert_eq!(read(false, 1, 25).await.0, Err(7));
    assert_eq!(read(true, 1, 25).await.0, Err(7));
    let mut copied = 0;
    for step in 0..64 {
        let (rows, done, _) = migrate(1).await;
        assert!(rows <= 2);
        copied += rows;
        if step == 0 {
            assert!(
                !done,
                "exercise write admission during unfinished maintenance"
            );
            upgrade().await;
            expect_closed(false).await;
            expect_closed(true).await;
        }
        if done {
            break;
        }
        assert!(step < 63, "copy must progress");
    }
    assert_eq!(
        copied,
        (model.parents.len() + model.items.len() + model.links.len()) as u64
    );
    for step in 0..128 {
        let (_, done, _) = migrate(2).await;
        if step == 0 {
            assert!(
                !done,
                "exercise write admission during unfinished maintenance"
            );
            upgrade().await;
            expect_closed(false).await;
            expect_closed(true).await;
        }
        if done {
            break;
        }
        assert!(step < 127, "validation must progress");
    }
    migrate(3).await;
    for row in model.items.values_mut() {
        row.1 += 1000;
    }
    // New incarnation starts its own revisions; copying commits two rows at a time.
    model.revisions = [
        model.parents.len().div_ceil(2) as u64,
        model.items.len().div_ceil(2) as u64,
        model.links.len().div_ceil(2) as u64,
    ];
    assert_eq!(read(true, 1, 25).await.0.unwrap(), model.snapshot(1, 25));
    assert!(matches!(
        items(true, 1030, Some(1), Some(source_cursor.clone()))
            .await
            .0,
        Err(8)
    ));
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(resume_source_job(true).await, Err("InvalidJob".into()));
    assert_eq!(pic.get_stable_memory(canister).await, before);
    let target_page = items(true, 1030, Some(1), None).await.0.unwrap();
    let mut listed = target_page.ids;
    let mut after = target_page.next.expect("continued target page");
    check(
        &mut model,
        true,
        vec![(1, 3, 1, 0, 7)],
        "target-read-modify",
    )
    .await;
    let mut target_count = None;
    for step in 0..128 {
        let (_, done, _) = migrate(4).await;
        if step == 0 {
            assert!(
                !done,
                "exercise writes with retired allocations still pending"
            );
            expect_closed(false).await;
            check(&mut model, true, chain.to_vec(), "target-insert-chain").await;
            assert_eq!(
                job(CountRequest::Cancel(source_job.clone())).await,
                Ok(CountReply::Cancelled)
            );
            let handle = start(true).await;
            let (sequence, state) = advance(true, &handle, 0).await;
            assert_eq!(state, CountOutcome::Running);
            target_count = Some((handle, sequence));
            upgrade().await;
        }
        assert_eq!(
            read(true, 2, 1030).await.0.unwrap(),
            model.snapshot(2, 1030)
        );
        if done {
            break;
        }
        assert!(step < 127, "cleanup must progress");
    }
    upgrade().await;
    assert_eq!(
        read(true, 3, 1030).await.0.unwrap(),
        model.snapshot(3, 1030)
    );
    assert_eq!(read(false, 1, 0).await.0, Err(7));
    let (handle, sequence) = target_count.expect("count started during cleanup");
    // A delayed cancellation from before cutover must leave the new checkpoint intact.
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        job(CountRequest::Cancel(source_job)).await,
        Err("InvalidJob".into())
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    // Simulate losing the first target advance reply and resending its original sequence.
    assert_eq!(
        advance(true, &handle, 0).await,
        (sequence, CountOutcome::Running)
    );
    finish_count(true, &handle, sequence, model.items.len() as u64).await;
    assert_eq!(
        job(CountRequest::Cancel(handle)).await,
        Ok(CountReply::Cancelled)
    );
    // The intervening writes leave the membership of this parent listing intact.
    // Resume the target cursor obtained before cleanup and check every remaining page.
    for step in 0..=model.items.len() {
        let page = items(true, 1030, None, Some(after.clone()))
            .await
            .0
            .unwrap();
        assert!(page.examined <= 8);
        listed.extend(page.ids);
        let Some(next) = page.next else {
            break;
        };
        assert_ne!(after, next);
        after = next;
        assert!(step < model.items.len(), "target listing must finish");
    }
    let expected = model.snapshot(1, 1030).5;
    assert!(!expected.is_empty());
    assert_eq!(listed, expected);
    drop(check);
    println!(
        "complete read checks: n={} instructions={}..{} (all rows, metadata, two indexes, grouping)",
        read_samples.len(),
        read_samples.iter().min().unwrap(),
        read_samples.iter().max().unwrap()
    );
    for (label, values) in samples {
        println!(
            "{label}: n={} instructions={}..{} cycles={}..{}",
            values.len(),
            values.iter().map(|v| v.0).min().unwrap(),
            values.iter().map(|v| v.0).max().unwrap(),
            values.iter().map(|v| v.1).min().unwrap(),
            values.iter().map(|v| v.1).max().unwrap()
        );
    }
    owned.drop().await;
}

#[tokio::test]
async fn skewed_listing_and_retained_count_across_write_and_upgrade() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M4_WASM").expect("run make m4-lifecycle")).unwrap();
    let pic = instance().await.unwrap();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;

    let update = async |request: Update| {
        let bytes = pic
            .update_call(
                canister,
                canister,
                "db_write",
                application_request(241, &request),
            )
            .await
            .unwrap();

        application_reply::<Reply>(241, &bytes)
    };
    let write = async |model: &mut Model, commands: Vec<Command>| {
        model.apply(&commands).unwrap();
        let Reply::Batch(result, instructions) = update(Update::Batch(false, commands)).await
        else {
            panic!("expected batch reply")
        };
        result.unwrap();

        instructions
    };
    let mut model = Model::default();
    write(&mut model, vec![(0, 0, 1, 10, 0), (0, 0, 2, 20, 0)]).await;
    for chunk in (1..=128).collect::<Vec<u64>>().chunks(16) {
        let mut batch = Vec::new();
        for &id in chunk {
            // Every eighth item belongs to the small parent. Sparse high scores
            // in the large parent force empty continued pages.
            batch.push((1, 0, id, if id % 8 == 0 { 2 } else { 1 }, id % 11));
            if id % 8 == 0 {
                batch.push((2, 0, id, id, 0));
            }
        }
        write(&mut model, batch).await;
    }
    let read_page = async |request: Query| {
        let bytes = pic
            .query_call(
                canister,
                canister,
                "db_query",
                application_request(242, &request),
            )
            .await
            .unwrap();
        let QueryReply::Items(result, cost) = application_reply(242, &bytes) else {
            panic!("expected items reply")
        };
        (result.unwrap(), cost)
    };
    let page = async |parent, work, after| {
        read_page(Query::Items {
            target: false,
            parent,
            minimum: 10,
            work,
            after,
        })
        .await
    };
    let list = async |phase: &str,
                      model: &Model,
                      parent: u64,
                      requested_work: Option<u16>,
                      continuation: Option<(u64, Vec<u8>)>| {
        let work = requested_work.unwrap_or(8);
        let mut most_examined = 0;
        use std::ops::Bound::{Excluded, Unbounded};
        let (start, mut after) = match continuation {
            Some((id, cursor)) => (Excluded(id), Some(cursor)),
            None => (Unbounded, None),
        };
        let mut found = Vec::new();
        let mut instructions = 0;
        let mut examined = 0;
        let mut peak_instructions = 0;
        let mut empty_continued = 0;
        for page_number in 1..=model.items.len() + 1 {
            let (page, cost) = page(parent, requested_work, after.clone()).await;
            assert!(page.examined <= u64::from(work));
            assert!(page.ids.len() <= 2);
            peak_instructions = peak_instructions.max(cost);
            instructions += cost;
            examined += page.examined;
            most_examined = most_examined.max(page.examined);
            empty_continued += usize::from(page.ids.is_empty() && page.next.is_some());
            found.extend(page.ids);
            if page.next.is_none() {
                let candidates = model
                    .items
                    .range((start, Unbounded))
                    .filter(|(_, (owner, _))| *owner == parent);
                let expected: Vec<_> = candidates
                    .clone()
                    .filter(|(_, (_, score))| *score >= 10)
                    .map(|(&id, _)| id)
                    .collect();
                assert_eq!(found, expected);
                assert_eq!(examined as usize, candidates.count());
                if requested_work.is_none() {
                    assert_eq!(
                        most_examined, 8,
                        "sparse listing exercises the application default"
                    );
                }
                if work == 1 {
                    assert!(empty_continued > 0);
                }
                println!(
                    "skew/list phase={phase} parent={parent} work={work} peak_instructions={peak_instructions} pages={page_number} examined={examined} matched={} empty_continued={empty_continued} handler_instructions={instructions}",
                    found.len()
                );

                return;
            }
            assert_ne!(page.next, after);
            after = page.next;
        }
        panic!("listing did not finish");
    };
    let compare_budgets = async |phase: &str, model: &Model| {
        for work in [Some(1), None, Some(32)] {
            list(phase, model, 1, work, None).await;
        }
        list(phase, model, 2, None, None).await;
    };
    let count = async |request| {
        let Reply::Count(result, instructions) = update(Update::Count(request)).await else {
            panic!("expected count reply")
        };

        (result.unwrap(), instructions)
    };
    let start = async || {
        let (CountReply::Started(handle), _) =
            count(CountRequest::Run(false, CountAction::Start)).await
        else {
            panic!("expected handle")
        };

        handle
    };
    let resume = async |handle: &[u8], sequence, work| {
        let (CountReply::Advanced(next, outcome), cost) = count(CountRequest::Run(
            false,
            CountAction::Resume(handle.to_vec(), sequence, work),
        ))
        .await
        else {
            panic!("expected progress")
        };
        assert_eq!(next, sequence + 1);

        (outcome, cost)
    };
    compare_budgets("seeded", &model).await;
    // Hold the source revision fixed and measure only the update calls that advance each job.
    for work in [2, 16, 128] {
        let handle = start().await;
        let before = pic.cycle_balance(canister).await;
        let mut instructions = 0;
        let mut peak = 0;
        for sequence in 0..=model.items.len() as u64 {
            let (outcome, cost) = resume(&handle, sequence, work).await;
            instructions += cost;
            peak = peak.max(cost);
            if let CountOutcome::Complete(value) = outcome {
                assert_eq!(value, model.items.len() as u64);
                // These small rows reach the candidate limit before the input-byte limit.
                let advances = sequence + 1;
                let full_scan = value.div_ceil(u64::from(work));
                assert_eq!(advances, full_scan);
                let cycles = before - pic.cycle_balance(canister).await;
                println!(
                    "skew/warm-count work={work} advances={advances} result={value} instructions={instructions} peak={peak} cycles={cycles}"
                );
                break;
            }
            assert_eq!(outcome, CountOutcome::Running);
            assert!(sequence < model.items.len() as u64, "count must finish");
        }
        assert_eq!(
            count(CountRequest::Cancel(handle)).await.0,
            CountReply::Cancelled
        );
    }
    // Both jobs see the same source revision and get eight turns before the same write.
    let slow = start().await;
    let fast = start().await;
    let expected_count = model.items.len() as u64;
    for sequence in 0..8 {
        assert_eq!(resume(&slow, sequence, 2).await.0, CountOutcome::Running);
        let expected = if sequence == 7 {
            CountOutcome::Complete(expected_count)
        } else {
            CountOutcome::Running
        };
        assert_eq!(resume(&fast, sequence, 16).await.0, expected);
    }
    // Move a referenced item from the small parent into the listing.
    let before = pic.cycle_balance(canister).await;
    let cost = write(&mut model, vec![(1, 1, 128, 1, 10)]).await;
    let cycles = before - pic.cycle_balance(canister).await;
    let (outcome, invalidate_cost) = resume(&slow, 8, 2).await;
    assert_eq!(outcome, CountOutcome::Invalidated);
    // A caller that missed the terminal reply retries its original sequence.
    assert_eq!(
        resume(&fast, 7, 16).await.0,
        CountOutcome::Complete(expected_count)
    );
    for handle in [slow, fast] {
        assert_eq!(
            count(CountRequest::Cancel(handle)).await.0,
            CountReply::Cancelled
        );
    }
    println!(
        "skew/move handler_instructions={cost} update_balance_delta={cycles} invalidation_instructions={invalidate_cost}"
    );
    let handle = start().await;
    let (outcome, first_cost) = resume(&handle, 0, 2).await;
    assert_eq!(outcome, CountOutcome::Running);
    compare_budgets("after-write", &model).await;
    ic_db_m0_rollback_driver::upgrade_stopped(&pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    compare_budgets("after-upgrade", &model).await;
    let mut advance_cost = first_cost;
    for sequence in 1.. {
        assert!(
            sequence <= model.items.len() as u64,
            "retained count did not finish"
        );
        let (outcome, cost) = resume(&handle, sequence, 2).await;
        advance_cost += cost;
        if sequence == 1 {
            assert_eq!(outcome, CountOutcome::Running);
            compare_budgets("after-first-update", &model).await;
        }
        if let CountOutcome::Complete(value) = outcome {
            assert_eq!(value, model.items.len() as u64);
            println!(
                "skew/count advances={} result={value} total_advance_instructions={advance_cost}",
                sequence + 1
            );
            assert_eq!(
                count(CountRequest::Cancel(handle)).await.0,
                CountReply::Cancelled
            );
            break;
        }
        assert_eq!(outcome, CountOutcome::Running);
    }
    // The first eight candidates for parent 1 end at ID 9. No row
    // matches yet, so the client only has the opaque continuation.
    let (first, _) = page(1, None, None).await;
    assert!(first.ids.is_empty());
    assert_eq!(first.examined, 8);
    let after = first.next.expect("sparse first page");
    let (control, _) = page(1, None, Some(after.clone())).await;
    assert_eq!(control.ids, [10]);

    // Change membership behind and ahead of the position, delete its
    // owner, and append a new match. Resume the original token.
    write(
        &mut model,
        vec![
            (1, 1, 3, 1, 10),
            (1, 2, 9, 0, 0),
            (1, 1, 10, 2, 10),
            (1, 1, 12, 1, 10),
            (1, 0, 130, 1, 10),
        ],
    )
    .await;
    list("live-after-batch", &model, 1, None, Some((9, after))).await;
    // All four rows qualify throughout. Only their order changes between pages.
    write(
        &mut model,
        vec![
            (1, 0, 131, 1, 101),
            (1, 0, 132, 1, 100),
            (1, 0, 133, 1, 100),
            (1, 0, 134, 1, 101),
        ],
    )
    .await;
    let ranked = async |after| {
        read_page(Query::RankedItems {
            minimum: 100,
            after,
        })
        .await
        .0
    };
    let first = ranked(None).await;
    assert_eq!(first.ids, [132, 133]);
    let after = first.next.expect("two more ranked items");
    assert_eq!(ranked(Some(after.clone())).await.ids, [131, 134]);
    write(&mut model, vec![(1, 1, 132, 1, 101), (1, 1, 131, 1, 100)]).await;
    let second = ranked(Some(after)).await;
    let boundary = (100_u128, 133_u64);
    let mut remaining: Vec<_> = model
        .items
        .iter()
        .map(|(&id, &(_, score))| (score, id))
        .filter(|key| *key > boundary)
        .collect();
    remaining.sort_unstable();
    assert_eq!(
        second.ids,
        remaining.iter().map(|key| key.1).collect::<Vec<_>>()
    );
    assert_eq!(second.ids, [132, 134]);
    if let Some(after) = second.next {
        let end = ranked(Some(after)).await;
        assert!(end.ids.is_empty() && end.next.is_none());
    }
    pic.drop().await;
}

#[tokio::test]
async fn retained_counts_by_parent_selectivity() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M4_WASM").expect("run make m4-lifecycle")).unwrap();
    let pic = instance().await.unwrap();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    let update = async |request| {
        let bytes = pic
            .update_call(
                canister,
                canister,
                "db_write",
                application_request(241, &request),
            )
            .await
            .unwrap();
        application_reply::<Reply>(241, &bytes)
    };
    let write = async |commands| {
        assert!(matches!(
            update(Update::Batch(false, commands)).await,
            Reply::Batch(Ok(()), _)
        ));
    };
    write(vec![(0, 0, 1, 10, 0), (0, 0, 2, 20, 0)]).await;
    let items: Vec<Command> = (1..=511)
        .map(|id| {
            let parent = if id % 16 < 13 { 1 } else { 2 };
            (1, 0, id, parent, id)
        })
        .collect();
    for chunk in items.chunks(16) {
        write(chunk.to_vec()).await;
    }
    let expected = items.iter().filter(|item| item.3 == 1).count() as u64;
    assert_eq!(expected, 415);
    let read = async |request| {
        let bytes = pic
            .query_call(
                canister,
                canister,
                "db_query",
                application_request(242, &request),
            )
            .await
            .unwrap();
        application_reply::<QueryReply>(242, &bytes)
    };
    for (label, request, expected) in [
        ("primary-hit", Query::ParentById(1), Some((1, 10))),
        ("primary-miss", Query::ParentById(99), None),
        ("unique-hit", Query::ParentByCode(10), Some((1, 10))),
        ("unique-miss", Query::ParentByCode(99), None),
    ] {
        let QueryReply::Parent(row, work) = read(request).await else {
            panic!("expected parent")
        };
        assert_eq!(row.unwrap(), expected);
        println!("ordinary-read {label} instructions={work}");
    }
    for parent in [1, 2, 3] {
        let QueryReply::Count(result, work) = read(Query::ParentCount(parent)).await else {
            panic!("expected count")
        };
        let matches = items.iter().filter(|item| item.3 == parent).count() as u64;
        assert_eq!(result.unwrap(), (matches, matches, 0));
        println!("ordinary-read count parent={parent} matches={matches} instructions={work}");
    }
    for minimum in [0, 12] {
        let mut cursor = None;
        let mut found = Vec::new();
        for page_number in 1..=2 {
            let QueryReply::Items(result, work) = read(Query::Items {
                target: false,
                parent: 1,
                minimum,
                work: None,
                after: cursor.take(),
            })
            .await
            else {
                panic!("expected listing")
            };
            let page = result.unwrap();
            assert_eq!(page.fetched, page.examined);
            assert!(page.examined <= 8 && page.ids.len() <= 2);
            if minimum == 12 && page_number == 1 {
                assert!(page.ids.is_empty());
                assert_eq!(page.examined, 8);
            }
            println!(
                "ordinary-read listing minimum={minimum} page={page_number} examined={} fetched={} returned={} instructions={work}",
                page.examined,
                page.fetched,
                page.ids.len()
            );
            found.extend(page.ids);
            cursor = page.next;
            assert!(cursor.is_some());
        }
        let expected: Vec<_> = items
            .iter()
            .filter(|item| item.3 == 1 && item.4 >= minimum)
            .take(if minimum == 0 { 4 } else { 2 })
            .map(|item| item.2)
            .collect();
        assert_eq!(found, expected);
    }
    let count = async |request| {
        let Reply::Count(reply, work) = update(Update::Count(request)).await else {
            panic!("expected count reply")
        };
        (reply.unwrap(), work)
    };
    let start = async |parent, indexed| {
        let (CountReply::Started(handle), work) = count(CountRequest::ForParent {
            parent,
            indexed,
            action: CountAction::Start,
        })
        .await
        else {
            panic!("expected count handle")
        };
        (handle, work)
    };
    let advance = async |parent, indexed, handle: &[u8], sequence, budget| {
        let (reply, work) = count(CountRequest::ForParent {
            parent,
            indexed,
            action: CountAction::Resume(handle.to_vec(), sequence, budget),
        })
        .await;
        let CountReply::Advanced(next, outcome) = reply else {
            panic!("expected progress")
        };
        assert_eq!(next, sequence + 1);
        (outcome, work)
    };
    // Compare identical work budgets first, then exercise the exact final boundary.
    for parent in [1, 2, 3] {
        let matches = items.iter().filter(|item| item.3 == parent).count();
        for indexed in [false, true] {
            let candidates = if indexed { matches } else { items.len() };
            let last = candidates.saturating_sub(1) % 128 + 1;
            let mut schedules: Vec<(u16, u16)> = vec![(128, 128)];
            if last > 1 && last < 128 {
                let last = u16::try_from(last).unwrap();
                schedules.extend([(128, last), (128, last - 1)]);
            }
            // Split the 96-entry sparse index range into two, three or four updates.
            if parent == 2 && indexed {
                schedules.extend([(48, 48), (32, 32), (24, 24)]);
            }
            for (work_limit, final_budget) in schedules {
                let full_chunks = candidates.saturating_sub(1) / usize::from(work_limit);
                let before = pic.cycle_balance(canister).await;
                let (handle, mut total) = start(parent, indexed).await;
                let mut peak = total;
                let mut advances = 0;
                let mut remaining = candidates;
                for budget in std::iter::repeat_n(work_limit, full_chunks).chain([final_budget, 1])
                {
                    let (outcome, work) = advance(parent, indexed, &handle, advances, budget).await;
                    advances += 1;
                    remaining = remaining.saturating_sub(usize::from(budget));
                    total += work;
                    peak = peak.max(work);
                    if remaining == 0 {
                        assert_eq!(outcome, CountOutcome::Complete(matches as u64));
                        break;
                    }
                    assert_eq!(outcome, CountOutcome::Running);
                }
                assert_eq!(remaining, 0);
                let cycles = before - pic.cycle_balance(canister).await;
                let (reply, cleanup) = count(CountRequest::Cancel(handle)).await;
                assert_eq!(reply, CountReply::Cancelled);
                println!(
                    "parent={parent} indexed={indexed} matches={matches} work_limit={work_limit} final_budget={final_budget} advances={advances} start_and_advance_instructions={total} peak_instructions={peak} start_and_advance_cycles={cycles} cleanup_instructions={cleanup}"
                );
            }
        }
    }

    // Give each plan the same number of advance opportunities before changing
    // an item owned by parent 1. The selected parent (2) keeps all its matches.
    let matches = items.iter().filter(|item| item.3 == 2).count();
    for (indexed, budget) in [(false, 128_u16), (true, 48), (true, 128)] {
        let candidates = if indexed { matches } else { items.len() };
        let required = candidates.div_ceil(usize::from(budget));
        for write_after in [0, 1, 2, 4] {
            let before = pic.cycle_balance(canister).await;
            let mut balance = before;
            let mut job_cycles = 0;
            let mut peak = 0;
            let mut record = |phase: &str, instructions, previous, current| {
                let cycles: u128 = previous - current;
                job_cycles += cycles;
                peak = peak.max(instructions);
                println!(
                    "count-call indexed={indexed} budget={budget} write_after={write_after} phase={phase} instructions={instructions} cycles={cycles}"
                );
            };
            let (handle, work) = start(2, indexed).await;
            let after_start = pic.cycle_balance(canister).await;
            record("start", work, balance, after_start);
            balance = after_start;
            for step in 0..required.min(write_after) {
                let (outcome, work) = advance(2, indexed, &handle, step as u64, budget).await;
                let after = pic.cycle_balance(canister).await;
                record("advance", work, balance, after);
                balance = after;
                if step + 1 == required {
                    assert_eq!(outcome, CountOutcome::Complete(matches as u64));
                } else {
                    assert_eq!(outcome, CountOutcome::Running);
                }
            }
            let before_write = balance;
            write(vec![(1, 3, 1, 0, 1)]).await;
            let after_write = pic.cycle_balance(canister).await;
            balance = after_write;
            let completed = required <= write_after;
            if !completed {
                let (outcome, work) =
                    advance(2, indexed, &handle, write_after as u64, budget).await;
                let after = pic.cycle_balance(canister).await;
                record("invalidate", work, balance, after);
                balance = after;
                assert_eq!(outcome, CountOutcome::Invalidated);
            }
            let (reply, cleanup) = count(CountRequest::Cancel(handle)).await;
            assert_eq!(reply, CountReply::Cancelled);
            let end = pic.cycle_balance(canister).await;
            record("cleanup", cleanup, balance, end);
            assert_eq!(job_cycles, (before - before_write) + (after_write - end));
            println!(
                "count-contention indexed={indexed} budget={budget} write_after={write_after} completed={completed} job_cycles={job_cycles} peak_job_instructions={peak}"
            );
        }
    }

    // The two parent filters have disjoint matches but share the Item revision.
    let (dense, _) = start(1, false).await;
    let (small, _) = start(2, false).await;
    for (parent, handle) in [(1, &dense), (2, &small)] {
        assert_eq!(
            advance(parent, false, handle, 0, 64).await.0,
            CountOutcome::Running
        );
    }
    write(vec![(1, 0, 512, 2, 512)]).await;
    for (parent, handle, expected_after_write) in [
        (1, dense, expected),
        (2, small, items.len() as u64 - expected + 1),
    ] {
        assert_eq!(
            advance(parent, false, &handle, 1, 64).await.0,
            CountOutcome::Invalidated
        );
        assert_eq!(
            count(CountRequest::Cancel(handle)).await.0,
            CountReply::Cancelled
        );
        let (retry, _) = start(parent, false).await;
        let mut outcome = CountOutcome::Running;
        for sequence in 0..5 {
            outcome = advance(parent, false, &retry, sequence, 128).await.0;
            if outcome != CountOutcome::Running {
                break;
            }
        }
        assert_eq!(outcome, CountOutcome::Complete(expected_after_write));
        assert_eq!(
            count(CountRequest::Cancel(retry)).await.0,
            CountReply::Cancelled
        );
    }
    pic.drop().await;
}

#[tokio::test]
async fn compare_mounted_read_modify_write_batch_sizes() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M4_WASM").expect("run make m4-lifecycle")).unwrap();
    let updates: Vec<Command> = (1..=12).map(|id| (1, 3, id, 0, 150 - 11 * id)).collect();
    for width in [1, 4, 12] {
        let pic = instance().await.unwrap();
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 10_000_000_000_000).await;
        pic.install_canister(canister, wasm.clone(), vec![], None)
            .await;
        let write = async |commands| {
            let bytes = pic
                .update_call(
                    canister,
                    canister,
                    "db_write",
                    application_request(241, &Update::Batch(false, commands)),
                )
                .await
                .unwrap();
            let Reply::Batch(result, work) = application_reply(241, &bytes) else {
                panic!("expected batch reply")
            };
            (result, work)
        };
        let mut model = Model::default();
        for (_, commands) in seed_batches() {
            model.apply(&commands).unwrap();
            write(commands).await.0.unwrap();
        }
        let before = pic.cycle_balance(canister).await;
        let mut total = 0;
        let mut peak = 0;
        for chunk in updates.chunks(width) {
            model.apply(chunk).unwrap();
            let (result, work) = write(chunk.to_vec()).await;
            result.unwrap();
            total += work;
            peak = peak.max(work);
        }
        let cycles = before - pic.cycle_balance(canister).await;
        let verify = async |model: &Model| {
            for parent in 1..=4 {
                let bytes = pic
                    .query_call(
                        canister,
                        canister,
                        "db_query",
                        application_request(242, &Query::Snapshot(false, parent, 143)),
                    )
                    .await
                    .unwrap();
                let QueryReply::Snapshot(result, _) = application_reply(242, &bytes) else {
                    panic!("expected snapshot reply")
                };
                assert_eq!(result.unwrap(), model.snapshot(parent, 143));
            }
        };
        verify(&model).await;
        println!(
            "score-batch width={width} updates={} total_instructions={total} peak_instructions={peak} cycles={cycles}",
            updates.len() / width
        );
        // A second pass ends with a reference to a missing parent.
        let (earlier, last) = updates.split_at(updates.len() - width);
        for chunk in earlier.chunks(width) {
            model.apply(chunk).unwrap();
            write(chunk.to_vec()).await.0.unwrap();
        }
        let mut invalid = last.to_vec();
        *invalid.last_mut().unwrap() = (1, 1, 12, 999, 0);
        let before_refusal = pic.get_stable_memory(canister).await;
        assert_eq!(model.apply(&invalid), Err(5));
        assert_eq!(write(invalid).await.0, Err(5));
        assert!(pic.get_stable_memory(canister).await == before_refusal);
        verify(&model).await;

        // Retry only the refused batch with its valid final command.
        model.apply(last).unwrap();
        write(last.to_vec()).await.0.unwrap();
        verify(&model).await;
        pic.drop().await;
    }
}

#[tokio::test]
async fn compare_returned_managed_row_with_read_after_commit() {
    use ic_testkit::pocket_ic::Time;

    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M4_WASM").expect("run make m4-lifecycle")).unwrap();
    let mut control = None;
    for returning in [false, true] {
        let pic = instance().await.unwrap();
        let canister = pic.create_canister().await;
        pic.add_cycles(canister, 10_000_000_000_000).await;
        pic.install_canister(canister, wasm.clone(), vec![], None)
            .await;
        let seeded = pic
            .update_call(
                canister,
                canister,
                "managed_write",
                encode(82, &(1_u64, "pending", 100_i64, 5_u8, false)),
            )
            .await
            .unwrap();
        decode::<Result<(), u8>>(82, &seeded).unwrap();
        let mut checkpoint = None;
        for (step, body) in ["confirmed", "confirmed", "ready"].into_iter().enumerate() {
            let millis = 2_000_000_000_000_u64 + 1000 * step as u64;
            pic.set_time(Time::from_nanos_since_unix_epoch(millis * 1_000_000))
                .await;
            pic.tick().await;
            let before = pic.cycle_balance(canister).await;
            let bytes = pic
                .update_call(
                    canister,
                    canister,
                    "db_write",
                    application_request(
                        241,
                        &Update::ReplaceManaged {
                            returning,
                            body: body.into(),
                        },
                    ),
                )
                .await
                .unwrap();
            let cycles = before - pic.cycle_balance(canister).await;
            let Reply::Managed(row, instructions) = application_reply(241, &bytes) else {
                panic!("expected managed row");
            };
            let backing = pic.get_stable_memory(canister).await;
            if step == 0 {
                let (id, stored, created, updated) = row.unwrap();
                assert_eq!((id, stored.as_str(), created), (1, "confirmed", 100));
                assert_eq!(updated, millis as i64);
                let bytes = pic
                    .query_call(canister, canister, "managed_rows", encode(83, &updated))
                    .await
                    .unwrap();
                type State = (Vec<(u64, String, i64, i64)>, Vec<u64>, u64);
                assert_eq!(
                    decode::<State>(83, &bytes),
                    (
                        vec![(1, stored, 100, updated), (2, "ready".into(), 100, updated)],
                        vec![1, 2],
                        2
                    )
                );
                checkpoint = Some((backing, updated));
            } else {
                let (saved, updated) = checkpoint.as_ref().unwrap();
                assert!(saved == &backing);
                if step == 1 {
                    assert_eq!(row.unwrap(), (1, "confirmed".into(), 100, *updated));
                } else {
                    assert_eq!(row, Err(4));
                }
            }
            println!(
                "managed replace returning={returning} step={step} handler_instructions={instructions} update_balance_delta={cycles}"
            );
        }
        if returning {
            assert!(control == checkpoint);
        } else {
            control = checkpoint;
        }
        pic.drop().await;
    }
}

#[tokio::test]
async fn managed_upsert_replays_across_upgrade_and_refuses_a_conflicting_batch() {
    use ic_testkit::pocket_ic::Time;

    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M4_WASM").expect("run make m4-lifecycle")).unwrap();
    let pic = instance().await.unwrap();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;

    let upsert = async |body: &str, second: Option<&str>| {
        let request = Update::UpsertManaged(body.into(), second.map(str::to_owned));
        let bytes = pic
            .update_call(
                canister,
                canister,
                "db_write",
                application_request(241, &request),
            )
            .await
            .unwrap();
        let Reply::Managed(row, _) = application_reply(241, &bytes) else {
            panic!("expected managed row")
        };
        row
    };
    let clock = async |millis: i64| {
        pic.set_time(Time::from_nanos_since_unix_epoch(millis as u64 * 1_000_000))
            .await;
        pic.tick().await;
    };
    let created = 2_000_000_000_000;
    let updated = created + 1_000;
    clock(created).await;
    assert_eq!(
        upsert("pending", None).await.unwrap(),
        (1, "pending".into(), created, created)
    );
    clock(updated).await;
    let first = (1, "confirmed".into(), created, updated);
    assert_eq!(upsert("confirmed", Some("receipt")).await.unwrap(), first);

    let saved = pic.get_stable_memory(canister).await;
    clock(updated + 600_000).await;
    ic_db_m0_rollback_driver::upgrade_stopped(&pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(upsert("confirmed", Some("receipt")).await.unwrap(), first);
    assert!(pic.get_stable_memory(canister).await == saved);
    assert_eq!(upsert("conflict", Some("conflict")).await, Err(4));
    assert!(pic.get_stable_memory(canister).await == saved);

    type State = (Vec<(u64, String, i64, i64)>, Vec<u64>, u64);
    let rows = vec![first, (2, "receipt".into(), updated, updated)];
    for (time, ids) in [(created, vec![]), (updated, vec![1, 2])] {
        let bytes = pic
            .query_call(canister, canister, "managed_rows", encode(83, &time))
            .await
            .unwrap();
        let (stored, indexed, revision) = decode::<State>(83, &bytes);
        assert_eq!(stored, rows);
        assert_eq!(indexed, ids);
        assert_eq!(revision, 2);
    }
    pic.drop().await;
}

#[tokio::test]
async fn grouped_count_under_interleaved_writes() {
    let wasm =
        std::fs::read(std::env::var_os("IC_DB_M4_WASM").expect("run make m4-lifecycle")).unwrap();
    let pic = instance().await.unwrap();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 10_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;

    let update = async |request| {
        let before = pic.cycle_balance(canister).await;
        let bytes = pic
            .update_call(
                canister,
                canister,
                "db_write",
                application_request(241, &request),
            )
            .await
            .unwrap();
        (
            application_reply::<Reply>(241, &bytes),
            before - pic.cycle_balance(canister).await,
        )
    };
    let write = async |model: &mut Model, commands: Vec<Command>| {
        model.apply(&commands).unwrap();
        assert!(matches!(
            update(Update::Batch(false, commands)).await.0,
            Reply::Batch(Ok(()), _)
        ));
    };
    let count = async |request| {
        let (Reply::Count(result, instructions), cycles) = update(Update::Count(request)).await
        else {
            panic!("expected count response")
        };
        (result.unwrap(), instructions, cycles)
    };
    let start = async || {
        let (CountReply::Started(handle), work, charge) =
            count(CountRequest::Run(false, CountAction::StartGrouped)).await
        else {
            panic!("expected grouped handle")
        };
        (handle, work, charge)
    };
    let request = |handle: &[u8], sequence| {
        CountRequest::Run(
            false,
            CountAction::ResumeGrouped(handle.to_vec(), sequence, 3),
        )
    };
    let mut model = Model::default();
    for (_, commands) in seed_batches() {
        write(&mut model, commands).await;
    }
    // Move the first scanned item on consecutive attempts, including just
    // before the final chunk.
    for (schedule, move_item) in [
        ("quiet", None),
        ("unrelated", None),
        ("source-early", Some((1, 1))),
        ("source-late", Some((2, 3))),
        ("source-before-completion", Some((3, 4))),
        ("quiet-retry", None),
    ] {
        let (handle, mut instructions, mut cycles) = start().await;
        let mut invalidated = false;
        let mut examined = 0;
        for sequence in 0..4 {
            if schedule == "unrelated" && sequence > 0 {
                write(&mut model, vec![(0, 1, 1, 100 + sequence, 0)]).await;
            }
            if let Some((at, parent)) = move_item
                && sequence == at
            {
                write(&mut model, vec![(1, 1, 1, parent, 10)]).await;
                invalidated = true;
            }
            let (reply, work, charge) = count(request(&handle, sequence)).await;
            let CountReply::GroupAdvanced(next, seen, outcome) = reply else {
                panic!("expected grouped advance")
            };
            assert_eq!(next, sequence + 1);
            instructions += work;
            cycles += charge;
            if invalidated {
                assert_eq!(outcome, CountOutcome::Invalidated);
                assert_eq!(seen, examined);
            } else if outcome == CountOutcome::Running {
                assert_eq!(seen, examined + 3);
                assert!(sequence < 3);
                examined = seen;
                continue;
            } else {
                assert!(
                    move_item.is_none(),
                    "job finished before its scheduled move"
                );
                assert_eq!(outcome, CountOutcome::Groups(model.group_counts()));
                assert_eq!(seen, model.items.len() as u64);
            }
            println!(
                "grouped/{schedule} advances={next} examined={seen} invalidated={invalidated} start_and_advance_instructions={instructions} start_and_advance_cycles={cycles} last_advance_instructions={work} last_advance_cycles={charge}"
            );
            break;
        }
        let (reply, work, charge) = count(CountRequest::Cancel(handle)).await;
        assert_eq!(reply, CountReply::Cancelled);
        println!("grouped/{schedule} cleanup_instructions={work} cleanup_cycles={charge}");
    }
    // Two advances commit, but their replies never reach the application.
    // Retrying the older request after upgrade must return the latest checkpoint.
    // Control the creation time so retries can straddle the exact expiry millisecond.
    let started_ms = 2_000_000_000_000_u64;
    let clock =
        |millis| ic_testkit::pocket_ic::Time::from_nanos_since_unix_epoch(millis * 1_000_000);
    pic.set_time(clock(started_ms)).await;
    let (handle, _, _) = start().await;
    count(request(&handle, 0)).await;
    count(request(&handle, 1)).await;
    ic_db_m0_rollback_driver::upgrade_stopped(&pic, canister, wasm.clone(), vec![], None)
        .await
        .unwrap();
    let (checkpoint, _, _) = count(request(&handle, 0)).await;
    assert_eq!(
        checkpoint,
        CountReply::GroupAdvanced(2, 6, CountOutcome::Running)
    );
    let before_refusal = pic.get_stable_memory(canister).await;
    let (reply, _) = update(Update::Count(request(&handle, 3))).await;
    assert!(matches!(reply, Reply::Count(Err(reason), _) if reason == "InvalidJob"));
    assert!(pic.get_stable_memory(canister).await == before_refusal);
    assert_eq!(
        count(request(&handle, 2)).await.0,
        CountReply::GroupAdvanced(3, 9, CountOutcome::Running)
    );
    let expected = model.group_counts();
    count(request(&handle, 3)).await; // Lose the completion reply as well.
    let (unfinished, _, _) = start().await;
    count(request(&unfinished, 0)).await;
    write(&mut model, vec![(1, 1, 1, 2, 10)]).await;
    // The same source write must invalidate the unfinished job on a stale retry.
    assert_eq!(
        count(request(&unfinished, 0)).await.0,
        CountReply::GroupAdvanced(2, 3, CountOutcome::Invalidated)
    );
    assert_eq!(
        count(CountRequest::Cancel(unfinished)).await.0,
        CountReply::Cancelled
    );
    assert_ne!(model.group_counts(), expected);
    let (reply, _, _) = count(request(&handle, 3)).await;
    assert_eq!(
        reply,
        CountReply::GroupAdvanced(4, 12, CountOutcome::Groups(expected))
    );
    let deadline = started_ms + 3_600_000;
    pic.set_time(clock(deadline - 1)).await;
    assert_eq!(count(request(&handle, 3)).await.0, reply);
    pic.set_time(clock(deadline)).await;
    assert_eq!(
        count(request(&handle, 3)).await.0,
        CountReply::GroupAdvanced(5, 12, CountOutcome::Expired)
    );

    // Force reuse of this specific slot without interpreting any opaque handle bytes.
    let mut occupied = Vec::new();
    for index in 0..15 {
        if index == 5 {
            pic.set_time(clock(deadline + 60_000)).await;
        }
        occupied.push(start().await.0);
    }
    let (full, _) = update(Update::Count(CountRequest::Run(
        false,
        CountAction::StartGrouped,
    )))
    .await;
    assert!(matches!(full, Reply::Count(Err(reason), _) if reason == "JobCapacity"));
    assert_eq!(
        count(CountRequest::Cancel(handle.clone())).await.0,
        CountReply::Cancelled
    );
    let (replacement, _, _) = start().await;
    assert_ne!(handle, replacement);
    let checkpoint = pic.get_stable_memory(canister).await;
    for delayed in [request(&handle, 3), CountRequest::Cancel(handle)] {
        let (reply, _) = update(Update::Count(delayed)).await;
        assert!(matches!(reply, Reply::Count(Err(reason), _) if reason == "InvalidJob"));
        assert!(pic.get_stable_memory(canister).await == checkpoint);
    }
    assert_eq!(
        count(request(&replacement, 0)).await.0,
        CountReply::GroupAdvanced(1, 3, CountOutcome::Running)
    );
    // Five handles now expire together. The other eleven have a minute left.
    pic.set_time(clock(deadline + 3_600_000)).await;
    let mut refilled = Vec::new();
    for (sequence, expected_removed) in [(1, 2), (2, 2), (3, 1)] {
        let (removed, work, cycles) = count(CountRequest::CleanupExpired(2)).await;
        assert_eq!(removed, CountReply::Removed(expected_removed));
        println!("grouped/sweep removed={expected_removed} instructions={work} cycles={cycles}");
        if sequence == 1 {
            // Reoccupy both freed slots before the next sweep. A fresh executable
            // must discover the remaining expired jobs from the durable directory.
            for _ in 0..2 {
                refilled.push(start().await.0);
            }
            ic_db_m0_rollback_driver::upgrade_stopped(&pic, canister, wasm.clone(), vec![], None)
                .await
                .unwrap();
        }
        assert_eq!(
            count(request(&replacement, sequence)).await.0,
            CountReply::GroupAdvanced(
                sequence + 1,
                (sequence + 1) * 3,
                if sequence == 3 {
                    CountOutcome::Groups(model.group_counts())
                } else {
                    CountOutcome::Running
                }
            )
        );
    }
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(
        count(CountRequest::CleanupExpired(2)).await.0,
        CountReply::Removed(0)
    );
    for expired in occupied.drain(..5) {
        let (reply, _) = update(Update::Count(request(&expired, 0))).await;
        assert!(matches!(reply, Reply::Count(Err(reason), _) if reason == "InvalidJob"));
    }
    assert!(pic.get_stable_memory(canister).await == before);
    assert_eq!(
        count(request(&replacement, 3)).await.0,
        CountReply::GroupAdvanced(4, 12, CountOutcome::Groups(model.group_counts()))
    );
    for handle in &refilled {
        assert_eq!(
            count(request(handle, 0)).await.0,
            CountReply::GroupAdvanced(1, 3, CountOutcome::Running)
        );
    }
    occupied.extend(refilled);
    occupied.push(replacement);
    for live in occupied {
        assert_eq!(
            count(CountRequest::Cancel(live)).await.0,
            CountReply::Cancelled
        );
    }
    pic.drop().await;
}
