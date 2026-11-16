//! Cross-entity Account/Subaccount identities through the ordinary product API.
use super::{Batch, Principal, open, runtime};
use ic_db::{Account, Error, Subaccount};
use ic_db_m1_records_schema::{AccountKey, SubaccountKey};

type Identity = (Vec<u8>, Option<Subaccount>);
type AccountRow = (Identity, Option<Subaccount>, u64);
type SubaccountRow = (Subaccount, Option<Identity>, u64);
type Page = (
    Vec<AccountRow>,
    Option<Identity>,
    Vec<SubaccountRow>,
    Option<Subaccount>,
    (u64, u64),
    Vec<Identity>,
);

fn account((owner, subaccount): Identity) -> Account {
    Account {
        owner: Principal::from_slice(&owner),
        subaccount,
    }
}
fn identity(value: Account) -> Identity {
    (value.owner.as_slice().to_vec(), value.subaccount)
}

#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "account_keys_write",
    guard = crate::transport::admit::<86>,
    decode = crate::transport::decode::<_, 86>,
    encode = crate::transport::encode::<_, 86>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn account_keys_write(
    (mode, accounts, subs, trap): (u8, Vec<AccountRow>, Vec<SubaccountRow>, bool),
) -> Result<(), u8> {
    assert!(accounts.len() + subs.len() <= 32);
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for (id, peer, rank) in accounts {
        let row = AccountKey {
            id: account(id),
            peer,
            rank,
        };
        match mode {
            0 => batch.insert(&row),
            1 => batch.replace(&row),
            2 => batch.delete::<AccountKey>(row.id),
            _ => panic!("invalid fixture mode"),
        }
        .unwrap();
    }
    for (id, peer, rank) in subs {
        let row = SubaccountKey {
            id,
            peer: peer.map(account),
            rank,
        };
        match mode {
            0 => batch.insert(&row),
            1 => batch.replace(&row),
            2 => batch.delete::<SubaccountKey>(row.id),
            _ => panic!("invalid fixture mode"),
        }
        .unwrap();
    }
    match db.commit(batch) {
        Ok(()) => {}
        Err(Error::ReferenceViolation) => return Err(1),
        Err(Error::UniqueViolation) => return Err(2),
        Err(Error::Exists) => return Err(3),
        Err(Error::DuplicateTarget) => return Err(4),
        Err(error) => panic!("unexpected Account key refusal: {error:?}"),
    }
    if trap {
        ic_platform_runtime::system::trap("after Account key commit");
    }
    Ok(())
}

#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "account_keys_page",
    guard = crate::transport::admit::<87>,
    decode = crate::transport::decode::<_, 87>,
    encode = crate::transport::encode::<_, 87>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn account_keys_page(
    (after, sub_after, limit, rank): (Option<Identity>, Option<Subaccount>, u8, u64),
) -> Page {
    let host = runtime();
    let db = open(&host);
    let accounts = db
        .page::<AccountKey>(after.map(account), usize::from(limit))
        .unwrap();
    let subs = db
        .page::<SubaccountKey>(sub_after, usize::from(limit))
        .unwrap();
    let rows = accounts
        .rows
        .into_iter()
        .map(|row| {
            let alias = Account {
                subaccount: Some(*row.id.effective_subaccount()),
                ..row.id
            };
            assert_eq!(db.get::<AccountKey>(alias).unwrap(), Some(row.clone()));
            (identity(row.id), row.peer, row.rank)
        })
        .collect();
    let sub_rows = subs
        .rows
        .into_iter()
        .map(|row| {
            assert_eq!(db.get::<SubaccountKey>(row.id).unwrap(), Some(row.clone()));
            (row.id, row.peer.map(identity), row.rank)
        })
        .collect();
    let matched = db
        .index_prefix::<AccountKey>("rank", &[rank.into()], 32)
        .unwrap()
        .into_iter()
        .map(|row| identity(row.id))
        .collect();
    (
        rows,
        accounts.next_after.map(identity),
        sub_rows,
        subs.next_after,
        (
            db.revision::<AccountKey>().unwrap(),
            db.revision::<SubaccountKey>().unwrap(),
        ),
        matched,
    )
}
