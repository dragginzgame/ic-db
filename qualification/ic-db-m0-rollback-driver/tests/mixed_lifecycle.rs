//! Independent map oracle across mixed writes, exact jobs, upgrade and maintenance.
//! No product dependency, schema descriptors, codecs or query execution in the oracle.
mod artifact;
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};

type Command = (u8, u8, u64, u64, u64);
type Snapshot = (
    Vec<(u64, u64)>,
    Vec<(u64, u64, u128)>,
    Vec<(u64, u64)>,
    [u64; 3],
    [u64; 3],
    Vec<u64>,
    Vec<(u64, u64)>,
    Vec<u64>,
);
type Job = (Vec<u8>, u64, u8, Option<u64>);
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

#[tokio::test]
async fn mixed_history_matches_independent_model_through_upgrade_and_copy() {
    let path = std::env::var_os("IC_DB_M4_WASM").expect("run make m4-lifecycle");
    let (wasm, facts) = artifact::read(std::fs::File::open(path).unwrap()).unwrap();
    assert!(facts.exports.contains("canister_update mixed_write"));
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
    let update = async |method: &str, bytes: Vec<u8>| {
        pic.update_call(canister, canister, method, bytes)
            .await
            .unwrap()
    };
    let read = async |target: bool, parent: u64, minimum: u64| {
        let bytes = pic
            .query_call(
                canister,
                canister,
                "mixed_read",
                encode(242, &(target, parent, minimum)),
            )
            .await
            .unwrap();
        decode::<(Result<Snapshot, u8>, u64)>(242, &bytes)
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
    let mut samples = BTreeMap::<&str, Vec<(u64, u128)>>::new();
    let mut read_samples = Vec::new();
    let mut check =
        async |model: &mut Model, target: bool, commands: Vec<Command>, label: &'static str| {
            let previous = model.clone();
            let expected = model.apply(&commands);
            let backing = pic.get_stable_memory(canister).await;
            let cycles = pic.cycle_balance(canister).await;
            let bytes = update("mixed_write", encode(241, &(target, commands))).await;
            let cycles = cycles - pic.cycle_balance(canister).await;
            let (actual, instructions) = decode::<(Result<(), u8>, u64)>(241, &bytes);
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
    check(
        &mut model,
        false,
        (1..=4).map(|id| (0, 0, id, id * 10, 0)).collect(),
        "seed-parents",
    )
    .await;
    check(
        &mut model,
        false,
        (1..=12)
            .flat_map(|id| [(2, 0, id, id, 0), (1, 0, id, id % 4 + 1, id * 10)])
            .collect(),
        "seed-dependants",
    )
    .await;
    let job = async |operation: u8, handle: Vec<u8>, sequence: u64| {
        decode::<Job>(
            243,
            &update("mixed_job", encode(243, &(operation, handle, sequence))).await,
        )
    };
    let (handle, sequence, _, _) = job(0, vec![], 0).await;
    let (_, sequence, state, _) = job(1, handle.clone(), sequence).await;
    assert_eq!(state, 0);
    let mut cursor: Option<Vec<u8>> = None;
    let mut matches = Vec::new();
    let mut empty_continued = 0;
    for step in 0..4 {
        let bytes = pic
            .query_call(canister, canister, "mixed_page", encode(245, &cursor))
            .await
            .unwrap();
        let (rows, next, examined, instructions): (Vec<u64>, Option<Vec<u8>>, u64, u64) =
            decode(245, &bytes);
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
    let (_, sequence, state, _) = job(1, handle.clone(), sequence).await;
    assert_eq!(
        state, 0,
        "unrelated, refused and no-op writes must preserve input revision"
    );
    check(&mut model, false, vec![(1, 3, 1, 0, 1)], "read-modify").await;
    assert_eq!(job(1, handle.clone(), sequence).await.2, 2);
    job(2, handle, 0).await;
    let (handle, mut sequence, _, _) = job(0, vec![], 0).await;
    for attempt in 0..=12 {
        let (_, next, state, value) = job(1, handle.clone(), sequence).await;
        sequence = next;
        if state == 1 {
            assert_eq!(value, Some(12));
            break;
        }
        assert!(attempt < 12 && state == 0, "bounded job must make progress");
    }
    check(&mut model, false, vec![(1, 0, 50, 1, 50)], "insert-item").await;
    let completed = job(1, handle.clone(), sequence).await;
    assert_eq!(
        (completed.2, completed.3),
        (1, Some(12)),
        "completed result retains its original input"
    );
    job(2, handle, 0).await;
    check(&mut model, false, vec![(1, 2, 50, 0, 0)], "delete-item").await;

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
    migrate(0).await;
    assert_eq!(read(false, 1, 25).await.0, Err(7));
    assert_eq!(read(true, 1, 25).await.0, Err(7));
    let mut copied = 0;
    for step in 0..64 {
        let (rows, done, _) = migrate(1).await;
        assert!(rows <= 2);
        copied += rows;
        if step == 2 {
            upgrade().await;
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
        if step == 2 {
            upgrade().await;
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
    check(
        &mut model,
        true,
        vec![(1, 3, 1, 0, 7)],
        "target-read-modify",
    )
    .await;
    for step in 0..128 {
        let (_, done, _) = migrate(4).await;
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
