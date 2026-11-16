//! Optional high-fan-out workload; the frozen engine and scalar fixture are unchanged.

use super::{db, measure};
use ic_db_icydb_baseline_schema::{EnvelopeRow, EnvelopeRowInsert, EnvelopeRowReplace};
use icydb::{
    db::{TypedOperationError, WriteCell},
    types::Id,
};

#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "envelope_comparison_write",
    guard = crate::transport::admit::<93>,
    decode = crate::transport::decode::<_, 93>,
    encode = crate::transport::encode::<_, 93>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn envelope_comparison_write(
    (mode, generation, text_len): (u8, u8, u16),
) -> (Result<(), u16>, u64) {
    assert!(mode <= 1 && generation <= 1 && matches!(text_len, 48 | 1018));
    measure(|| {
        let database = db().unwrap();
        let mut batch = database.trusted_typed_write_batch();
        for id in 1_u64..=32 {
            let text = "\0".repeat(usize::from(text_len));
            let mut a = text.clone();
            if generation == 1 {
                a.replace_range(..1, "\u{1}");
            }
            if mode == 0 {
                batch
                    .push(EnvelopeRowInsert {
                        id: WriteCell::Value(Id::from_key(id)),
                        a: WriteCell::Value(a),
                        b: WriteCell::Value(text.clone()),
                        c: WriteCell::Value(text.clone()),
                        d: WriteCell::Value(text),
                        padding: WriteCell::Value(Vec::new().into()),
                    })
                    .unwrap();
            } else {
                batch
                    .push(EnvelopeRowReplace {
                        id: Id::from_key(id),
                        a: WriteCell::Value(a),
                        b: WriteCell::Value(text.clone()),
                        c: WriteCell::Value(text.clone()),
                        d: WriteCell::Value(text),
                        padding: WriteCell::Value(Vec::new().into()),
                    })
                    .unwrap();
            }
        }
        match batch.execute() {
            Ok(_) => Ok(()),
            Err(TypedOperationError::Database(error)) => Err(error.code().raw()),
            Err(error) => panic!("unexpected adapter refusal: {error:?}"),
        }
    })
}

#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "envelope_row",
    guard = crate::transport::admit::<91>,
    decode = crate::transport::decode::<_, 91>,
    encode = crate::transport::encode::<_, 91>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn envelope_row(id: u64) -> (String, String, String, String) {
    measure(|| {
        let row = db()
            .unwrap()
            .get::<EnvelopeRow>(Id::from_key(id))
            .unwrap()
            .unwrap();
        assert!(row.padding.is_empty());
        (row.a, row.b, row.c, row.d)
    })
    .0
}

#[ic_platform_runtime::query(
    runtime = ic_platform_runtime,
    name = "envelope_listing",
    guard = crate::transport::admit::<92>,
    decode = crate::transport::decode::<_, 92>,
    encode = crate::transport::encode::<_, 92>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn envelope_listing(generation: u8) -> Vec<u64> {
    assert!(generation <= 1);
    measure(|| {
        let mut text = "\0".repeat(48);
        if generation == 1 {
            text.replace_range(..1, "\u{1}");
        }
        db().unwrap()
            .query::<EnvelopeRow>()
            .unwrap()
            .filter(icydb::db::query::FieldRef::new("a").eq(text))
            .order_by(icydb::db::query::asc("b"))
            .order_by(icydb::db::query::asc("c"))
            .order_by(icydb::db::query::asc("d"))
            .order_by(icydb::db::query::asc("id"))
            .limit(32)
            .execute_live_page(None)
            .unwrap()
            .rows
            .into_iter()
            .map(|row| row.id)
            .collect()
    })
    .0
}
