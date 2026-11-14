//! Literal omission rules use the ordinary product batch and constraints.
use crate::{open, runtime, schema::DefaultRow};
use ic_db::{Batch, Error};
use ic_platform_runtime::cdk;

type Row = (u64, u16, bool, Option<String>);
fn view(row: DefaultRow) -> Row {
    (row.id, row.count, row.enabled, row.label)
}

#[cdk::update(
    guard = "crate::transport::admit::<80>",
    decode_with = "crate::transport::decode::<_, 80>",
    encode_with = "crate::transport::encode::<_, 80>"
)]
fn default_write((id, mode, trap): (u64, u8, bool)) -> Result<Row, u8> {
    let host = runtime();
    let mut db = open(&host);
    let mut batch = Batch::new();
    let row = match mode {
        0 => batch.insert_input(DefaultRow::insert().id(id)),
        1 => batch.insert_input(
            DefaultRow::insert()
                .id(id)
                .count(0)
                .enabled(false)
                .label(None),
        ),
        2 => {
            let mut row = db.get::<DefaultRow>(id).unwrap().ok_or(4_u8)?;
            row.count = 0;
            row.enabled = false;
            row.label = None;
            batch.replace(&row).map_err(|_| 4_u8)?;
            Ok(row)
        }
        3 => batch.insert_input(DefaultRow::insert()),
        4 => batch.insert_input(DefaultRow::insert().id(id).label(Some("too long!".into()))),
        _ => return Err(4),
    }
    .map_err(|error| match error {
        Error::MissingField(_) => 1_u8,
        Error::Limit => 2,
        _ => 4,
    })?;
    db.commit(batch).map_err(|_| 3_u8)?;
    if trap {
        cdk::api::trap("after default commit");
    }
    Ok(view(row))
}

#[cdk::query(
    guard = "crate::transport::admit::<81>",
    decode_with = "crate::transport::decode::<_, 81>",
    encode_with = "crate::transport::encode::<_, 81>"
)]
fn default_rows() -> (Vec<Row>, Vec<u64>, u64) {
    let host = runtime();
    let db = open(&host);
    (
        db.page::<DefaultRow>(None, 32)
            .unwrap()
            .rows
            .into_iter()
            .map(view)
            .collect(),
        db.index_prefix::<DefaultRow>("label", &["draft".into()], 32)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect(),
        db.revision::<DefaultRow>().unwrap(),
    )
}
