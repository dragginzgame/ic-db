//! Scalar primary keys share ordering, references and corruption checks.
use ic_db::{Account, Batch, Database, Entity, Error, Principal, Schema, U256, entity};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};
use std::fmt::Debug;
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
// Keep the independent oracle outside the generated declarations: raw literal
// vectors below specify byte order; this helper never calls a product key encoder.
fn exercise<E: Entity + Clone + PartialEq + Debug>(
    ids: &[E::Key],
    encoded: &[Vec<u8>],
    make: impl Fn(E::Key, Option<E::Key>, u64) -> E,
) where
    E::Key: Debug,
{
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[E::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [13; 16]).unwrap();
    let mut rows: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(n, &id)| make(id, Some(ids[(n + 1) % ids.len()]), n as u64))
        .collect();
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.insert(&rows[0]).unwrap();
    assert!(matches!(db.commit(b), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut b = Batch::new();
    for row in rows.iter().rev() {
        b.insert(row).unwrap();
    }
    db.commit(b).unwrap();
    for (row, &id) in rows.iter().zip(ids) {
        assert_eq!(db.get::<E>(id).unwrap(), Some(row.clone()));
    }
    let first = db.page::<E>(None, 2).unwrap();
    assert_eq!(first.rows, rows[..2]);
    assert_eq!(first.next_after, Some(ids[1]));
    assert_eq!(db.page::<E>(first.next_after, 32).unwrap().rows, rows[2..]);
    assert!(
        db.page::<E>(Some(*ids.last().unwrap()), 2)
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(
        db.index_prefix::<E>("zone,id", &[true.into()], 32).unwrap(),
        rows
    );
    drop(db);
    let raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    let index = E::SCHEMA.indexes.iter().find(|i| i.name == "rank").unwrap();
    let mut index_prefix = vec![3];
    index_prefix.extend(E::ID.to_be_bytes());
    index_prefix.extend(index.id.to_be_bytes());
    let reverse = E::SCHEMA
        .indexes
        .iter()
        .find(|i| i.name == "next,id")
        .unwrap();
    for (n, key) in encoded.iter().enumerate() {
        let mut physical = vec![2];
        physical.extend(E::ID.to_be_bytes());
        physical.extend(key);
        assert_eq!(raw.get(&physical).unwrap(), rows[n].encode().unwrap());
        let mut unique = index_prefix.clone();
        unique.extend((n as u64).to_be_bytes());
        assert_eq!(raw.get(&unique).unwrap(), *key);
        let mut dependent = vec![3];
        dependent.extend(E::ID.to_be_bytes());
        dependent.extend(reverse.id.to_be_bytes());
        dependent.push(1);
        dependent.extend(&encoded[(n + 1) % encoded.len()]);
        dependent.extend(key);
        dependent.extend(key);
        assert_eq!(raw.get(&dependent).unwrap(), *key);
    }
    drop(raw);
    let mut unique = index_prefix;
    unique.extend(0_u64.to_be_bytes());
    let owner = &encoded[0];
    let mut long = owner.clone();
    long.push(0);
    for invalid in [owner[..owner.len() - 1].to_vec(), long] {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
        raw.insert(unique.clone(), invalid);
        drop(raw);
        let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
        let before = backing.borrow().clone();
        assert!(matches!(
            db.index_prefix::<E>("rank", &[0_u64.into()], 1),
            Err(Error::Corrupt)
        ));
        let mut b = Batch::new();
        b.delete::<E>(ids[0]).unwrap();
        assert!(matches!(db.commit(b), Err(Error::Corrupt)));
        assert_eq!(*backing.borrow(), before);
        drop(db);
    }
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
    raw.insert(unique, owner.clone());
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    let before = backing.borrow().clone();
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    db.commit(b).unwrap();
    assert_eq!(*backing.borrow(), before);
    let mut b = Batch::new();
    b.delete::<E>(ids[0]).unwrap();
    assert!(matches!(db.commit(b), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    let duplicate = make(ids[0], Some(ids[1]), 1);
    let mut b = Batch::new();
    b.replace(&duplicate).unwrap();
    assert!(matches!(db.commit(b), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    let last = rows.len() - 1;
    rows[0] = make(ids[0], Some(ids[1]), last as u64);
    rows[last] = make(ids[last], Some(ids[0]), 0);
    let mut b = Batch::new();
    b.replace(&rows[0]).unwrap();
    b.replace(&rows[last]).unwrap();
    db.commit(b).unwrap();
    drop(db);
    let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
    assert_eq!(db.page::<E>(None, 32).unwrap().rows, rows);
    assert_eq!(
        db.index_prefix::<E>("rank", &[0_u64.into()], 1).unwrap()[0].key(),
        ids[last]
    );
    let mut b = Batch::new();
    for &id in ids {
        b.delete::<E>(id).unwrap();
    }
    db.commit(b).unwrap();
    assert!(db.page::<E>(None, 32).unwrap().rows.is_empty());
    assert_eq!(db.revision::<E>().unwrap(), 3);
}
macro_rules! unsigned_case {
    ($module:ident,$prim:literal,$ty:ty,$width:literal)=>{mod $module {
        use super::*;
        #[entity(pk(field="id"),index(field="rank",unique),index(fields=["zone","id"]),index(fields=["next","id"]),fields(field(name="id",value(item(prim=$prim))),field(name="next",value(opt,item(prim=$prim)),references(entity="Self")),field(name="rank",value(item(prim="Nat64"))),field(name="zone",value(item(prim="Bool")))))]
        struct Row {}
        #[test] fn full_unsigned_domain(){let mut one=vec![0;$width];one[$width-1]=1;let mut penultimate=vec![255;$width];penultimate[$width-1]=254;
            exercise(&[0,1,<$ty>::MAX-1,<$ty>::MAX],&[vec![0;$width],one,penultimate,vec![255;$width]],|id,next,rank|Row{id,next,rank,zone:true});}
    }};
}
macro_rules! signed_case {
    ($module:ident,$prim:literal,$ty:ty,$width:literal)=>{mod $module {
        use super::*;
        #[entity(pk(field="id"),index(field="rank",unique),index(fields=["zone","id"]),index(fields=["next","id"]),fields(field(name="id",value(item(prim=$prim))),field(name="next",value(opt,item(prim=$prim)),references(entity="Self")),field(name="rank",value(item(prim="Nat64"))),field(name="zone",value(item(prim="Bool")))))]
        struct Row {}
        #[test] fn full_signed_domain(){let mut negative=vec![255;$width];negative[0]=127;let mut zero=vec![0;$width];zero[0]=128;let mut one=zero.clone();one[$width-1]+=1;
            exercise(&[<$ty>::MIN,-1,0,1,<$ty>::MAX],&[vec![0;$width],negative,zero,one,vec![255;$width]],|id,next,rank|Row{id,next,rank,zone:true});}
    }};
}
unsigned_case!(nat8, "Nat8", u8, 1);
unsigned_case!(nat16, "Nat16", u16, 2);
unsigned_case!(nat32, "Nat32", u32, 4);
unsigned_case!(nat64, "Nat64", u64, 8);
unsigned_case!(nat128, "Nat128", u128, 16);
signed_case!(int8, "Int8", i8, 1);
signed_case!(int16, "Int16", i16, 2);
signed_case!(int32, "Int32", i32, 4);
signed_case!(int64, "Int64", i64, 8);
signed_case!(int128, "Int128", i128, 16);
#[entity(pk(field="id"),index(field="rank",unique),index(fields=["zone","id"]),index(fields=["next","id"]),fields(field(name="id",value(item(prim="U256"))),field(name="next",value(opt,item(prim="U256")),references(entity="Self")),field(name="rank",value(item(prim="Nat64"))),field(name="zone",value(item(prim="Bool")))))]
struct U256Row {}

#[entity(pk(field="id"),index(field="rank",unique),index(fields=["zone","id"]),index(fields=["next","id"]),fields(
    field(name="id",value(item(prim="Principal"))),
    field(name="next",value(opt,item(prim="Principal")),references(entity="Self")),
    field(name="rank",value(item(prim="Nat64"))),
    field(name="zone",value(item(prim="Bool")))
))]
struct PrincipalRow {}

#[entity(pk(field="id"),index(field="rank",unique),index(fields=["zone","id"]),index(fields=["next","id"]),fields(
    field(name="id",value(item(prim="Subaccount"))),
    field(name="next",value(opt,item(prim="Subaccount")),references(entity="Self")),
    field(name="rank",value(item(prim="Nat64"))),
    field(name="zone",value(item(prim="Bool")))
))]
struct SubaccountRow {}

#[entity(pk(field="id"),index(field="rank",unique),index(fields=["zone","id"]),index(fields=["next","id"]),fields(
    field(name="id",value(item(prim="Account"))),
    field(name="next",value(opt,item(prim="Account")),references(entity="Self")),
    field(name="rank",value(item(prim="Nat64"))),
    field(name="zone",value(item(prim="Bool")))
))]
struct AccountRow {}

#[test]
fn subaccount_keys_preserve_all_32_bytes() {
    let mut low = [0; 32];
    low[31] = 1;
    let mut high = [0; 32];
    high[0] = 1;
    let ids = [[0; 32], low, high, [255; 32]];
    exercise(&ids, &ids.map(|id| id.to_vec()), |id, next, rank| {
        SubaccountRow {
            id,
            next,
            rank,
            zone: true,
        }
    });
}

#[test]
fn account_keys_order_owner_then_effective_subaccount() {
    let inputs = [
        (&[][..], None),
        (&[][..], Some([255; 32])),
        (&[9][..], None),
        (&[1, 0][..], None),
        (&[255; 29][..], Some([255; 32])),
    ];
    let ids: Vec<_> = inputs
        .iter()
        .map(|(owner, subaccount)| Account {
            owner: Principal::from_slice(owner),
            subaccount: *subaccount,
        })
        .collect();
    // Independently frame the declared owner lengths and effective bytes.
    let frames: Vec<_> = [0, 0, 1, 2, 29]
        .into_iter()
        .zip(inputs)
        .map(|(len, (owner, sub))| {
            let mut b = vec![len];
            b.extend(owner);
            b.extend(sub.unwrap_or([0; 32]));
            b
        })
        .collect();
    exercise(&ids, &frames, |id, next, rank| AccountRow {
        id,
        next,
        rank,
        zone: true,
    });
}

#[test]
fn account_default_alias_is_one_primary_and_reference_identity() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[AccountRow::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [13; 16]).unwrap();
    let absent = Account {
        owner: Principal::from_slice(&[9]),
        subaccount: None,
    };
    let zero = Account {
        subaccount: Some([0; 32]),
        ..absent
    };
    let row = AccountRow {
        id: absent,
        next: Some(zero),
        rank: 0,
        zone: true,
    };
    let mut b = Batch::new();
    b.insert(&row).unwrap();
    db.commit(b).unwrap();
    assert_eq!(db.get::<AccountRow>(zero).unwrap(), Some(row.clone()));
    let saved = db.get::<AccountRow>(absent).unwrap().unwrap();
    assert_eq!(saved.id.subaccount, None);
    assert_eq!(saved.next.unwrap().subaccount, None);
    let before = backing.borrow().clone();
    let alias = AccountRow {
        id: zero,
        next: Some(absent),
        ..row.clone()
    };
    let mut b = Batch::new();
    b.insert(&alias).unwrap();
    assert!(matches!(db.commit(b), Err(Error::Exists)));
    let mut b = Batch::new();
    b.replace(&alias).unwrap();
    db.commit(b).unwrap();
    let mut b = Batch::new();
    b.delete::<AccountRow>(absent).unwrap();
    b.delete::<AccountRow>(zero).unwrap();
    assert!(matches!(db.commit(b), Err(Error::DuplicateTarget)));
    assert_eq!(*backing.borrow(), before);
    assert!(
        db.page::<AccountRow>(Some(zero), 1)
            .unwrap()
            .rows
            .is_empty()
    );
    let mut b = Batch::new();
    b.delete::<AccountRow>(zero).unwrap();
    db.commit(b).unwrap();
    assert!(db.get::<AccountRow>(absent).unwrap().is_none());
}

#[test]
fn malformed_account_and_subaccount_owners_refuse_intact() {
    malformed_owners(
        &SubaccountRow {
            id: [0; 32],
            next: None,
            rank: 0,
            zone: true,
        },
        vec![vec![], vec![0; 31], vec![0; 33], vec![1; 32]],
    );
    let mut oversized = vec![30];
    oversized.extend([0; 62]);
    let mut wrong_owner = vec![1, 8];
    wrong_owner.extend([0; 32]);
    let mut truncated = vec![1, 9];
    truncated.extend([0; 31]);
    let mut trailing = vec![1, 9];
    trailing.extend([0; 33]);
    malformed_owners(
        &AccountRow {
            id: Account {
                owner: Principal::from_slice(&[9]),
                subaccount: None,
            },
            next: None,
            rank: 0,
            zone: true,
        },
        vec![vec![], oversized, wrong_owner, truncated, trailing],
    );
}

#[test]
fn principals_keep_length_first_order_and_empty_identity() {
    let payloads = [vec![], vec![9], vec![1, 0], vec![0; 29], vec![255; 29]];
    let ids: Vec<_> = payloads.iter().map(|p| Principal::from_slice(p)).collect();
    let mut low = vec![29];
    low.extend([0; 29]);
    let mut high = vec![29];
    high.extend([255; 29]);
    assert!(ids.windows(2).all(|p| p[0] < p[1]));
    exercise(
        &ids,
        &[vec![0], vec![1, 9], vec![2, 1, 0], low, high],
        |id, next, rank| PrincipalRow {
            id,
            next,
            rank,
            zone: true,
        },
    );
}

#[test]
fn malformed_principal_owners_refuse_without_mutating_backing() {
    let mut oversized = vec![30];
    oversized.extend([1; 30]);
    malformed_owners(
        &PrincipalRow {
            id: Principal::from_slice(&[9]),
            next: None,
            rank: 0,
            zone: true,
        },
        vec![vec![], vec![2, 9], vec![1, 9, 0], oversized, vec![1, 8]],
    );
}

fn malformed_owners<E: Entity>(row: &E, invalid: Vec<Vec<u8>>) {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let slot = StableKey::parse("test.db.v1").unwrap();
    let schema = || Schema::new(&[E::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&slot).unwrap(), schema(), [13; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(row).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let index = E::SCHEMA.indexes.iter().find(|i| i.name == "rank").unwrap();
    let mut key = vec![3];
    key.extend(E::ID.to_be_bytes());
    key.extend(index.id.to_be_bytes());
    key.extend(0_u64.to_be_bytes());
    // Missing length, truncation, trailing bytes, oversized principal, and a
    // valid but wrong owner all fail at the storage boundary.
    for invalid in invalid {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap());
        raw.insert(key.clone(), invalid);
        drop(raw);
        let mut db = Database::open(runtime.open_memory(&slot).unwrap(), schema()).unwrap();
        let before = backing.borrow().clone();
        assert!(matches!(
            db.index_prefix::<E>("rank", &[0_u64.into()], 1),
            Err(Error::Corrupt)
        ));
        let mut batch = Batch::new();
        batch.delete::<E>(row.key()).unwrap();
        assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
        assert_eq!(*backing.borrow(), before);
    }
}
#[test]
fn u256_keeps_high_half() {
    let mut one = vec![0; 32];
    one[31] = 1;
    let mut high = vec![0; 32];
    high[15] = 1;
    exercise(
        &[U256::ZERO, U256::ONE, U256::from_words(1, 0), U256::MAX],
        &[vec![0; 32], one, high, vec![255; 32]],
        |id, next, rank| U256Row {
            id,
            next,
            rank,
            zone: true,
        },
    );
}

#[entity(pk(field="id"),index(field="rank",unique),index(fields=["peer","id"]),fields(
    field(name="id",value(item(prim="Int128"))),
    field(name="peer",value(opt,item(prim="U256")),references(entity="WideKey")),
    field(name="rank",value(item(prim="Nat64")))
))]
pub(crate) struct SignedKey {}
#[entity(pk(field="id"),index(field="rank",unique),index(fields=["peer","id"]),fields(
    field(name="id",value(item(prim="U256"))),
    field(name="peer",value(opt,item(prim="Int128")),references(entity="SignedKey")),
    field(name="rank",value(item(prim="Nat64")))
))]
pub(crate) struct WideKey {}
#[entity(
    pk(field = "id"),
    index(field = "target"),
    fields(
        field(name = "id", value(item(prim = "Nat8"))),
        field(
            name = "target",
            value(item(prim = "Nat128")),
            references(entity = "SignedKey")
        )
    )
)]
struct WrongSignedness {}
#[entity(
    pk(field = "id"),
    index(field = "target"),
    fields(
        field(name = "id", value(item(prim = "Nat8"))),
        field(
            name = "target",
            value(item(prim = "Int64")),
            references(entity = "SignedKey")
        )
    )
)]
struct WrongWidth {}
#[test]
fn reference_types_are_exact_and_schema_identity_is_independent() {
    assert_eq!(
        Schema::new(&[SignedKey::SCHEMA, WideKey::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "aff5474ffb42e4093e383b9fd543f5bda640d70623c610394e9d3e6ed30af7bf"
    );
    for wrong in [WrongSignedness::SCHEMA, WrongWidth::SCHEMA] {
        assert!(matches!(
            Schema::new(&[SignedKey::SCHEMA, WideKey::SCHEMA, wrong]),
            Err(Error::InvalidSchema)
        ));
    }
}
