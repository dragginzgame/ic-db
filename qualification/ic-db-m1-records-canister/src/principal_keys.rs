//! Principal identities use the ordinary typed batch, references and page path.

use super::{Batch, Principal, open, runtime};
use ic_db::Error;
use records_schema::PrincipalKey;

// principal_keys_write
// Apply the supplied principal-key mutations with optional rollback injection.
// guard  : validates the request frame for method 84.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "principal_keys_write",
    guard = crate::transport::admit::<84>,
    decode = crate::transport::decode::<_, 84>,
    encode = crate::transport::encode::<_, 84>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn principal_keys_write((mode, rows, trap): (u8, Vec<Row>, bool)) -> Result<(), u8> {
    if rows.len() > 32 {
        return Err(4);
    }
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    for (id, next, rank) in rows {
        let row = PrincipalKey {
            id: Principal::try_from_slice(&id).map_err(|_| 4_u8)?,
            next: next
                .as_deref()
                .map(Principal::try_from_slice)
                .transpose()
                .map_err(|_| 4_u8)?,
            rank,
        };
        match mode {
            0 => batch.insert(&row),
            1 => batch.replace(&row),
            2 => batch.delete::<PrincipalKey>(row.id),
            _ => return Err(4),
        }
        .unwrap();
    }
    match db.commit(batch) {
        Ok(()) => {}
        Err(Error::ReferenceViolation) => return Err(1),
        Err(Error::UniqueViolation) => return Err(2),
        Err(Error::Exists) => return Err(3),
        Err(error) => panic!("unexpected Principal write refusal: {error:?}"),
    }
    if trap {
        crate::system::trap("after Principal key commit");
    }

    Ok(())
}

// principal_keys_page
// Page principal keys and inspect the selected rank.
// guard  : validates the request frame for method 85.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "principal_keys_page",
    guard = crate::transport::admit::<85>,
    decode = crate::transport::decode::<_, 85>,
    encode = crate::transport::encode::<_, 85>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn principal_keys_page((after, limit, rank): (Option<Vec<u8>>, u8, u64)) -> Page {
    let host = runtime();
    let db = open(&host);
    let page = db
        .page::<PrincipalKey>(
            after.as_deref().map(Principal::from_slice),
            usize::from(limit),
        )
        .unwrap();
    let rows = page
        .rows
        .into_iter()
        .map(|row| {
            // Check direct typed lookup independently of page traversal.
            assert_eq!(db.get::<PrincipalKey>(row.id).unwrap(), Some(row.clone()));

            (
                row.id.as_slice().to_vec(),
                row.next.map(|p| p.as_slice().to_vec()),
                row.rank,
            )
        })
        .collect();
    let owners = db
        .index_prefix::<PrincipalKey>("rank", &[rank.into()], 32)
        .unwrap()
        .into_iter()
        .map(|row| row.id.as_slice().to_vec())
        .collect();

    (
        rows,
        page.next_after.map(|p| p.as_slice().to_vec()),
        db.revision::<PrincipalKey>().unwrap(),
        owners,
    )
}

type Row = (Vec<u8>, Option<Vec<u8>>, u64);
type Page = (Vec<Row>, Option<Vec<u8>>, u64, Vec<Vec<u8>>);
