//! Installed managed-time semantics through the real bounded batch.
use crate::{open, runtime, schema::ManagedRow};
use ic_db::{Batch, Error, Timestamp};
use ic_platform_runtime::cdk;

type Row = (u64, String, i64, i64);
fn view(row: ManagedRow) -> Row {
    (
        row.id,
        row.body,
        row.created.as_millis(),
        row.updated.as_millis(),
    )
}

#[cdk::update(
    guard = "crate::transport::admit::<82>",
    decode_with = "crate::transport::decode::<_, 82>",
    encode_with = "crate::transport::encode::<_, 82>"
)]
fn managed_write((id, body, millis, mode, trap): (u64, String, i64, u8, bool)) -> Result<(), u8> {
    let host = runtime();
    let mut db = open(&host);
    let time = Timestamp::from_millis(millis);
    let mut batch = if mode == 4 {
        Batch::new()
    } else {
        Batch::at(time)
    };
    let prepare = match mode {
        0 | 4 => batch
            .insert_input(ManagedRow::insert().id(id).body(body))
            .map(|_| ()),
        1..=3 => {
            let mut row = db.get::<ManagedRow>(id).unwrap().ok_or(5_u8)?;
            row.body = body;
            if mode == 2 {
                row.created = time;
            }
            if mode == 3 {
                row.updated = time;
            }
            batch.replace(&row)
        }
        // Two insertions must share this one captured batch time.
        5 => {
            batch
                .insert_input(ManagedRow::insert().id(id).body(body))
                .map_err(error)?;
            batch
                .insert_input(ManagedRow::insert().id(id + 1).body("second".into()))
                .map(|_| ())
        }
        _ => return Err(5),
    };
    prepare.map_err(error)?;
    db.commit(batch).map_err(error)?;
    if trap {
        cdk::api::trap("after managed commit");
    }
    Ok(())
}
#[expect(
    clippy::needless_pass_by_value,
    reason = "Result::map_err consumes the error through this fixture callback."
)]
fn error(error: Error) -> u8 {
    match error {
        Error::MissingOperationTime => 1,
        Error::ManagedField => 2,
        Error::TimestampRegression => 3,
        Error::UniqueViolation => 4,
        _ => 5,
    }
}

#[cdk::query(
    guard = "crate::transport::admit::<83>",
    decode_with = "crate::transport::decode::<_, 83>",
    encode_with = "crate::transport::encode::<_, 83>"
)]
fn managed_rows(millis: i64) -> (Vec<Row>, Vec<u64>, u64) {
    let host = runtime();
    let db = open(&host);
    (
        db.page::<ManagedRow>(None, 32)
            .unwrap()
            .rows
            .into_iter()
            .map(view)
            .collect(),
        db.index_prefix::<ManagedRow>("updated", &[Timestamp::from_millis(millis).into()], 32)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect(),
        db.revision::<ManagedRow>().unwrap(),
    )
}
