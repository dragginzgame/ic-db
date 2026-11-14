//! Exact integer primary-key families share ordering, references and corruption checks.
use ic_db::{Batch, Database, Entity, Error, Schema, U256, entity};
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
// Keep the independent oracle outside the generated declarations: raw fixed-width
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
    let raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap()).unwrap();
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
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap())
                .unwrap();
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
    let mut raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&slot).unwrap()).unwrap();
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
        "45fae6814b3f66f892c7097123ed9d0dd47a29240692a0dce559fb0f9e847602"
    );
    for wrong in [WrongSignedness::SCHEMA, WrongWidth::SCHEMA] {
        assert!(matches!(
            Schema::new(&[SignedKey::SCHEMA, WideKey::SCHEMA, wrong]),
            Err(Error::InvalidSchema)
        ));
    }
}
