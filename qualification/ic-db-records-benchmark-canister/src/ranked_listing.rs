//! Compare the same ranked listing with four physical index layouts.

use super::{database, measure, transport};
use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error, QueryCursor};

type Input = (u64, u64, bool, i64, i64);
type Page = (Vec<u64>, Option<QueryCursor>, usize, usize);

macro_rules! listing_entity {
    ($name:ident, $([$($component:tt),+]),+) => {
        /// Qualification item carrying its project's listing fields.
        #[entity(
            store = crate::RecordsStore,
            pk(field = "id"),
            $(index(fields = [$($component),+]),)+
            fields(
                field(name = "id", value(item(prim = "Nat64"))),
                field(name = "project_id", value(item(prim = "Nat64"))),
                field(name = "open", value(item(prim = "Bool"))),
                field(name = "price", value(item(prim = "Decimal", scale = 2))),
                field(name = "updated_at", value(item(prim = "Timestamp")))
            )
        )]
        pub(super) struct $name {}

        impl $name {
            fn seed(batch: &mut Batch, (id, project_id, open, cents, millis): Input) {
                batch.insert_input(Self::insert().id(id).project_id(project_id).open(open)
                    .price(Decimal::try_new(i128::from(cents), 2).unwrap())
                    .updated_at(Timestamp::from_millis(millis))).unwrap();
            }

            fn page<const FIXED_OWNER: bool>(db: &Database, ceiling: Decimal, work: usize, limit: usize, after: Option<&QueryCursor>) -> Result<Page, Error> {
                let prefix = [1_u64.into(), true.into()];
                let query = db.query::<Self>();
                let mut query = if FIXED_OWNER {
                    query.using_index("project_id,open", &prefix)
                        .filter_fields(Self::ID, ic_db::Comparison::Eq, Self::PROJECT_ID)
                } else {
                    query.eq(Self::PROJECT_ID, 1_u64).eq(Self::OPEN, true)
                        .order_by(&[Self::UPDATED_AT, Self::ID]).descending()
                }.lte(Self::PRICE, ceiling).limit(limit).work_limit(work);
                if let Some(cursor) = after {
                    query = query.after(cursor);
                }
                let page = query.fetch()?;
                Ok((page.rows.into_iter().map(|item| item.id).collect(), page.next, page.examined, page.fetched))
            }
        }
    };
}

listing_entity!(ByTime, ["project_id", "open", "updated_at"]);
listing_entity!(ByProject, ["project_id", "open"]);
listing_entity!(ByPrice, ["project_id", "open", "price"]);

listing_entity!(
    ByPriceAndTime,
    ["project_id", "open", "price"],
    ["project_id", "open", "updated_at"]
);

// seed_listing
// Insert the same rows into all four layouts in one bounded batch.
// guard  : validates the request frame for method 11.
// limits : 64 KiB request, 64 KiB reply; at most 8 input rows.
#[ic_platform_runtime::update(runtime = ic_platform_runtime, name = "seed_listing",
    guard = transport::admit::<11>, decode = transport::decode::<_, 11>,
    encode = transport::encode::<_, 11>, max_bytes = 65_536, on_error = crate::transport::endpoint_error)]
fn seed_listing(rows: Vec<Input>) {
    assert!(rows.len() <= 8);
    let mut batch = Batch::new();
    for row in rows {
        ByTime::seed(&mut batch, row);
        ByProject::seed(&mut batch, row);
        ByPrice::seed(&mut batch, row);
        ByPriceAndTime::seed(&mut batch, row);
    }
    database().commit(batch).unwrap();
}

// ranked_listing
// Return one page, its work counters and handler instructions. None reports Error::Limit.
// guard  : validates the request frame for method 12.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::query(runtime = ic_platform_runtime, name = "ranked_listing",
    guard = transport::admit::<12>, decode = transport::decode::<_, 12>,
    encode = transport::encode::<_, 12>, max_bytes = 65_536, on_error = crate::transport::endpoint_error)]
fn ranked_listing(
    (layout, cents, work, limit, after): (u8, i64, usize, usize, Option<QueryCursor>),
) -> (Option<Page>, u64) {
    measure(|| {
        let db = database();
        let read = match layout {
            0 => ByProject::page::<false>,
            1 => ByTime::page::<false>,
            2 => ByPrice::page::<false>,
            3 => ByPriceAndTime::page::<false>,
            4 => ByProject::page::<true>,
            _ => panic!("unknown listing layout"),
        };
        let ceiling = Decimal::try_new(i128::from(cents), 2).unwrap();
        match read(&db, ceiling, work, limit, after.as_ref()) {
            Ok(page) => Some(page),
            Err(Error::Limit) => None,
            Err(error) => panic!("unexpected ranked listing error: {error:?}"),
        }
    })
}
