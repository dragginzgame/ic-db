//! Exact account/subaccount bytes and one identity for the default account.
use ic_db::{
    Account, Batch, Database, Entity, Error, IndexValue, Principal, Schema, Subaccount, entity,
    map, set,
};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};
#[entity(
    pk(field="id"),
    index(fields=["zone","value","id"]),
    index(fields=["zone","sub","id"]),
    index(field="maybe",unique,present_only),
    fields(
        field(name="id",value(item(prim="Nat64"))),
        field(name="maybe",value(opt,item(prim="Account"))),
        field(name="sub",value(item(prim="Subaccount"))),
        field(name="value",value(item(prim="Account"))),
        field(name="zone",value(item(prim="Bool")))
    )
)]
struct AccountRow {}
#[set(item(prim = "Account"), max_items = 4)]
struct Owners {}
#[map(
    key(prim = "Account"),
    value(opt, item(prim = "Subaccount")),
    max_items = 3
)]
struct Allocations {}
#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "list", value(many, max_items = 2, item(prim = "Account"))),
        field(name = "map", value(item(is = "Allocations"))),
        field(name = "set", value(item(is = "Owners")))
    )
)]
struct Collections {}
fn account(owner: &[u8], subaccount: Option<Subaccount>) -> Account {
    Account {
        owner: Principal::from_slice(owner),
        subaccount,
    }
}

fn accounts() -> Vec<Account> {
    vec![
        account(&[], None),
        account(&[], Some([255; 32])),
        account(&[4], None),
        account(&[4], Some([1; 32])),
        account(&[4], Some([255; 32])),
        account(&[0, 0], None),
        account(&[0, 0], Some([1; 32])),
        account(&[255; 29], Some([255; 32])),
    ]
}

fn host(backing: VectorMemory) -> MemoryRuntime<VectorMemory> {
    let mut host = MemoryRuntime::new(backing).unwrap();
    let req =
        SealedDeclarationSnapshot::new(vec![MemoryRequest::new("test", "test.db.v1").unwrap()])
            .unwrap();
    let grants =
        AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap();
    host.bootstrap(&req, &grants, &GenericAllocationPolicy)
        .unwrap();
    host
}

fn key(value: IndexValue<'_>) -> Vec<u8> {
    ic_db::__private::encode_index(&[(value, false)], false).unwrap()
}

#[test]
fn default_identity_and_exact_fixed_bytes_share_one_canonical_representation() {
    // Independent Node byte framing and SHA-256.
    assert_eq!(
        Schema::new(&[AccountRow::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "b6df0493c64fd993efe97eb5069d00785b48e29ce1e8991a786a14965cb1354e"
    );
    for len in 0..=29 {
        let owner = vec![0xa5; len];
        let default = account(&owner, None);
        let zero = account(&owner, Some([0; 32]));
        assert_eq!(default, zero);
        assert_eq!(HashSet::from([default, zero]).len(), 1);
        assert_eq!(BTreeSet::from([default, zero]).len(), 1);
        assert_eq!(key(default.into()), key(zero.into()));
        for subaccount in [None, Some([0; 32]), Some([1; 32]), Some([255; 32])] {
            let value = account(&owner, subaccount);
            let row = AccountRow {
                id: 7,
                maybe: Some(value),
                sub: [255; 32],
                value,
                zone: true,
            };
            let decoded = AccountRow::decode(&row.encode().unwrap()).unwrap();
            assert_eq!(decoded, row);
            assert_eq!(
                decoded.value.subaccount,
                subaccount.filter(|s| *s != [0; 32])
            );
        }
    }
    let sequence: Subaccount = std::array::from_fn(|i| i as u8 + 1);
    let row = AccountRow {
        id: 7,
        maybe: None,
        sub: sequence,
        value: account(&[4], Some([0; 32])),
        zone: true,
    };
    let mut golden = vec![7, 0, 0, 0, 0, 0, 0, 0, 0];
    golden.extend(1_u8..=32);
    golden.extend([1, 0, 0, 0, 4]);
    golden.extend([0; 32]);
    golden.push(1);
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(AccountRow::decode(&golden).unwrap().value.subaccount, None);
    // The optional Account itself is not the same as an absent subaccount.
    let mut present = row.clone();
    present.maybe = Some(account(&[], None));
    assert_ne!(row.encode().unwrap(), present.encode().unwrap());
    for end in 0..golden.len() {
        assert!(matches!(
            AccountRow::decode(&golden[..end]),
            Err(Error::Corrupt)
        ));
    }
    let mut excess = golden.clone();
    excess.push(0);
    assert!(matches!(AccountRow::decode(&excess), Err(Error::Corrupt)));
    let mut bad = golden.clone();
    bad[41..45].copy_from_slice(&30_u32.to_le_bytes());
    assert!(matches!(AccountRow::decode(&bad), Err(Error::Corrupt)));
    let mut short = golden;
    short.remove(9);
    assert!(matches!(AccountRow::decode(&short), Err(Error::Corrupt)));
}

#[test]
fn ordered_components_follow_owner_then_effective_subaccount() {
    let values = accounts();
    for (i, a) in values.iter().enumerate() {
        for (j, b) in values.iter().enumerate() {
            assert_eq!(a.cmp(b), i.cmp(&j));
            assert_eq!(key((*a).into()).cmp(&key((*b).into())), i.cmp(&j));
        }
    }
    let mut literal = vec![1, 4];
    literal.extend([255; 32]);
    assert_eq!(key(account(&[4], Some([255; 32])).into()), literal);
    let bytes: Subaccount = std::array::from_fn(|i| i as u8);
    assert_eq!(key(bytes.into()), bytes);
    for (a, b) in [([0; 32], [1; 32]), ([1; 32], [255; 32])] {
        assert!(key(a.into()) < key(b.into()));
    }
}

#[test]
fn indexed_default_account_refusal_noop_and_final_state_exchange() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[AccountRow::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [8; 16]).unwrap();
    let mut rows: Vec<_> = accounts()
        .into_iter()
        .enumerate()
        .map(|(n, value)| AccountRow {
            id: n as u64 + 1,
            maybe: Some(value),
            sub: [n as u8; 32],
            value,
            zone: true,
        })
        .collect();
    let mut batch = Batch::new();
    for r in rows.iter().rev() {
        batch.insert(r).unwrap();
    }
    db.commit(batch).unwrap();
    for index in ["zone,value,id", "zone,sub,id"] {
        assert_eq!(
            db.index_prefix::<AccountRow>(index, &[true.into()], 32)
                .unwrap()
                .into_iter()
                .map(|r| r.id)
                .collect::<Vec<_>>(),
            (1..=8).collect::<Vec<_>>()
        );
    }
    assert!(matches!(
        db.index_prefix::<AccountRow>(
            "zone,value,id",
            &[true.into(), Principal::anonymous().into()],
            32
        ),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.index_prefix::<AccountRow>("zone,sub,id", &[true.into(), ic_db::U256::ZERO.into()], 32),
        Err(Error::InvalidOperand)
    ));
    let before = backing.borrow().clone();
    let zero = account(&[4], Some([0; 32]));
    let row = AccountRow {
        value: zero,
        maybe: Some(zero),
        ..rows[2].clone()
    };
    let mut b = Batch::new();
    b.replace(&row).unwrap();
    db.commit(b).unwrap();
    assert_eq!(*backing.borrow(), before);
    let mut b = Batch::new();
    b.insert(&AccountRow { id: 9, ..row }).unwrap();
    assert!(matches!(db.commit(b), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    let first = rows[2].maybe;
    rows[2].maybe = rows[7].maybe;
    rows[7].maybe = first;
    let mut b = Batch::new();
    b.replace(&rows[2]).unwrap();
    b.replace(&rows[7]).unwrap();
    db.commit(b).unwrap();
    assert_eq!(
        db.index_prefix::<AccountRow>("maybe", &[zero.into()], 32)
            .unwrap()[0]
            .id,
        8
    );
    drop(db);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    for row in rows {
        assert_eq!(db.get::<AccountRow>(row.id).unwrap(), Some(row));
    }
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    let index = AccountRow::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "zone,value,id")
        .unwrap();
    let mut physical = vec![3];
    physical.extend(AccountRow::ID.to_be_bytes());
    physical.extend(index.id.to_be_bytes());
    physical.extend([1, 1, 4]);
    physical.extend([255; 32]);
    physical.extend(5_u64.to_be_bytes());
    physical.extend(5_u64.to_be_bytes());
    assert!(raw.get(&physical).is_some());
    raw.remove(&physical);
    physical[10] = 30;
    raw.insert(physical, 5_u64.to_be_bytes().to_vec());
    drop(raw);
    let db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert!(matches!(
        db.index_prefix::<AccountRow>("zone,value,id", &[true.into()], 32),
        Err(Error::Corrupt)
    ));
}

#[test]
fn collections_cannot_hide_duplicate_default_accounts_or_malformed_owners() {
    let default = account(&[], None);
    let zero = account(&[], Some([0; 32]));
    let other = account(&[4], Some([1; 32]));
    let row = Collections {
        id: 1,
        list: vec![default, zero],
        map: Allocations(BTreeMap::from([(default, None), (other, Some([255; 32]))])),
        set: Owners(BTreeSet::from([default, zero, other])),
    };
    assert_eq!(row.set.0.len(), 2);
    let bytes = row.encode().unwrap();
    assert_eq!(Collections::decode(&bytes).unwrap(), row);
    // Empty-owner accounts occupy 36 bytes; the first map key starts at 88.
    let mut duplicate = bytes.clone();
    duplicate.splice(125..162, vec![0; 36]);
    assert!(matches!(
        Collections::decode(&duplicate),
        Err(Error::Corrupt)
    ));
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[Collections::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [8; 16]).unwrap();
    let mut b = Batch::new();
    b.insert(&row).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    let mut rk = vec![2];
    rk.extend(Collections::ID.to_be_bytes());
    rk.extend(1_u64.to_be_bytes());
    let mut bad = bytes;
    bad[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    raw.insert(rk, bad);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.delete::<Collections>(1).unwrap();
    assert!(matches!(db.commit(b), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}

#[entity(
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "values", value(many, max_items = 64, item(prim = "Account")))
    )
)]
struct LargeAccounts {}
#[test]
fn account_lists_respect_the_row_byte_budget() {
    let mut row = LargeAccounts {
        id: 1,
        values: vec![account(&[255; 29], Some([255; 32])); 62],
    };
    let bytes = row.encode().unwrap();
    assert_eq!(bytes.len(), 4042);
    assert_eq!(LargeAccounts::decode(&bytes).unwrap(), row);
    row.values.push(row.values[0]);
    assert!(matches!(row.encode(), Err(Error::Limit)));
    let mut bad = bytes;
    bad[8..12].copy_from_slice(&63_u32.to_le_bytes());
    bad.extend([29, 0, 0, 0]);
    bad.extend([255; 61]);
    assert!(matches!(LargeAccounts::decode(&bad), Err(Error::Corrupt)));
}
