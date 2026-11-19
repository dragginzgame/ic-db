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
            if operation != 0 && !present {
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

    fn snapshot(&self, parent: u64, minimum: u64) -> Snapshot {
        let mut groups = BTreeMap::<u64, u64>::new();
        for &(parent, _) in self.items.values() {
            *groups.entry(parent).or_default() += 1;
        }
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
            groups.into_iter().collect(),
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
    canic_wire::encode(
        value,
        canic_wire::Encoding::Frame {
            method,
            direction: canic_wire::Direction::Request,
        },
        canic::db::RPC_LIMITS,
    )
    .unwrap()
}

fn application_reply<T: DeserializeOwned>(method: u64, bytes: &[u8]) -> T {
    canic_wire::decode(
        bytes,
        canic_wire::Encoding::Frame {
            method,
            direction: canic_wire::Direction::Response,
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
        let advance = async |sequence| count(CountAction::Resume(handle.clone(), sequence)).await;
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
        for sequence in 2..=6 {
            let expected = if sequence == 6 {
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
    let inspected = pic
        .query_call(
            canister,
            canister,
            "db_inspect",
            application_request(246, &false),
        )
        .await
        .unwrap();
    assert_eq!(
        application_reply::<Result<u64, u8>>(246, &inspected),
        Ok(model.parents.len() as u64)
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
            CountAction::Resume(handle.to_vec(), sequence),
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
        job(CountRequest::Run(false, CountAction::Resume(expiring, 0))).await,
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
            "refused-missing",
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
            CountAction::Resume(source_job.clone(), source_sequence),
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
    let list = async |phase: &str, model: &Model, requested_work: Option<u16>| {
        let work = requested_work.unwrap_or(8);
        let mut most_examined = 0;
        let mut after = None;
        let mut found = Vec::new();
        let mut instructions = 0;
        let mut examined = 0;
        let mut peak_instructions = 0;
        let mut empty_continued = 0;
        for page_number in 1..=model.items.len() + 1 {
            let request = Query::Items {
                target: false,
                parent: 1,
                minimum: 10,
                work: requested_work,
                after: after.clone(),
            };
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
            let page = result.unwrap();
            assert!(page.examined <= u64::from(work));
            assert!(page.ids.len() <= 2);
            peak_instructions = peak_instructions.max(cost);
            instructions += cost;
            examined += page.examined;
            most_examined = most_examined.max(page.examined);
            empty_continued += usize::from(page.ids.is_empty() && page.next.is_some());
            found.extend(page.ids);
            if page.next.is_none() {
                assert_eq!(found, model.snapshot(1, 10).5);
                assert_eq!(
                    examined as usize,
                    model
                        .items
                        .values()
                        .filter(|(parent, _)| *parent == 1)
                        .count()
                );
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
                    "skew/list phase={phase} work={work} peak_instructions={peak_instructions} pages={page_number} examined={examined} matched={} empty_continued={empty_continued} handler_instructions={instructions}",
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
            list(phase, model, work).await;
        }
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
    let resume = async |handle: &[u8], sequence| {
        let (CountReply::Advanced(next, outcome), cost) = count(CountRequest::Run(
            false,
            CountAction::Resume(handle.to_vec(), sequence),
        ))
        .await
        else {
            panic!("expected progress")
        };
        assert_eq!(next, sequence + 1);
        (outcome, cost)
    };
    compare_budgets("seeded", &model).await;
    let handle = start().await;
    assert_eq!(resume(&handle, 0).await.0, CountOutcome::Running);
    // Move a referenced item from the small parent into the listing.
    let before = pic.cycle_balance(canister).await;
    let cost = write(&mut model, vec![(1, 1, 128, 1, 10)]).await;
    let cycles = before - pic.cycle_balance(canister).await;
    let (outcome, invalidate_cost) = resume(&handle, 1).await;
    assert_eq!(outcome, CountOutcome::Invalidated);
    assert_eq!(
        count(CountRequest::Cancel(handle)).await.0,
        CountReply::Cancelled
    );
    println!(
        "skew/move handler_instructions={cost} update_balance_delta={cycles} invalidation_instructions={invalidate_cost}"
    );
    let handle = start().await;
    let (outcome, first_cost) = resume(&handle, 0).await;
    assert_eq!(outcome, CountOutcome::Running);
    compare_budgets("after-write", &model).await;
    ic_db_m0_rollback_driver::upgrade_stopped(&pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    compare_budgets("after-upgrade", &model).await;
    let mut advance_cost = first_cost;
    for sequence in 1..=model.items.len() as u64 {
        let (outcome, cost) = resume(&handle, sequence).await;
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
            pic.drop().await;
            return;
        }
        assert_eq!(outcome, CountOutcome::Running);
    }
    panic!("retained count did not finish");
}
