//! Explicit native comparison of the three engines on the same scalar fixture.
use ic_db::{Batch, Comparison, Database, Entity, Schema, SortDirection, entity, record};
use ic_db_icydb_baseline_schema::{Entry as Legacy, EntryInsert as LegacyInsert, EntryReplace};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory,
};
use icydb::{db::WriteCell, types::Id};
use rusqlite::{Connection, params};
use std::{hint::black_box, time::Instant};

icydb::__icydb_start_actor!();

fn icydb_storage_grants() -> Result<icydb::db::AllocationGrants, icydb::db::AdmissionError> {
    icydb::db::AllocationGrants::new(vec![icydb::db::MemoryAuthority::new(
        "icydb_icdb_baseline",
        "icydb.icdb_baseline.",
    )?])
}

fn settle() {
    for _ in 0..100 {
        if icydb::db::with_request_execution(|| {
            __icydb_generated::startup_driver_attempt().unwrap()
        }) {
            return;
        }
    }
    panic!("baseline startup did not settle");
}

#[test]
#[ignore = "explicit native performance comparison"]
fn native_matrix() {
    let path = std::env::var_os("IC_DB_NATIVE_MATRIX_CSV").expect("set IC_DB_NATIVE_MATRIX_CSV");
    let mut csv = String::from("profile,population,case,ic_db_ns,icydb_ns,sqlite_ns\n");
    for skew in [false, true] {
        csv += &std::thread::spawn(move || profile(skew)).join().unwrap();
    }
    std::fs::write(path, csv).unwrap();
}

#[entity(pk(field="id"),index(fields=["category","id"]),fields(
    field(name="id",value(item(prim="Nat64"))),field(name="category",value(item(prim="Nat64"))),
    field(name="amount",value(item(prim="Nat64")))))]
struct Entry {}
#[record(fields(field(name = "category", value(item(prim = "Nat64")))))]
struct Category {}
#[record(fields(field(name = "amount", value(item(prim = "Nat64")))))]
struct Amount {}

type Row = (u64, u64, u64);
#[derive(Debug, PartialEq)]
enum ResultSet {
    Rows(Vec<Row>),
    Count(u64),
    Groups(Vec<(u64, u64)>),
    Refused,
}
#[derive(Clone, Copy)]
enum Read {
    Lookup(u64),
    Prefix(u64),
    Count(Option<u64>),
    List(u8, u64, u64, usize),
    Group(u8, Option<u64>, u64),
}

impl Read {
    fn name(self) -> String {
        match self {
            Self::Lookup(id) => format!("lookup-{id}"),
            Self::Prefix(c) => format!("prefix-{c}"),
            Self::Count(c) => format!("count-{}", category_label(c)),
            Self::List(m, c, v, n) => format!("listing-{m}-c{c}-min{v}-limit{n}"),
            Self::Group(m, c, v) => format!("group-{m}-c{}-min{v}", category_label(c)),
        }
    }
}
fn category_label(c: Option<u64>) -> String {
    c.map_or_else(|| "all".into(), |n| n.to_string())
}

fn cases(n: u64) -> Vec<Read> {
    let mut out = Vec::new();
    for id in [1, n / 2, n, n + 1] {
        out.push(Read::Lookup(id));
    }
    for c in [0, 3, 99] {
        out.push(Read::Prefix(c));
    }
    for c in [None, Some(0), Some(3), Some(99)] {
        out.push(Read::Count(c));
    }
    for m in 0..4 {
        let cats: &[u64] = if m == 2 { &[0] } else { &[0, 3, 99] };
        for &c in cats {
            for v in [0, 500, 950, 1001] {
                for n in [1, 8, 32] {
                    out.push(Read::List(m, c, v, n));
                }
            }
        }
    }
    for c in [None, Some(0), Some(3), Some(99)] {
        for v in [0, 500, 950, 1001] {
            for m in 0..4 {
                out.push(Read::Group(m, c, v));
            }
        }
    }
    assert_eq!(out.len(), 195);
    out
}

fn ic_read(db: &Database<VectorMemory>, case: Read) -> ResultSet {
    let rows = |items: Vec<Entry>| {
        ResultSet::Rows(
            items
                .into_iter()
                .map(|r| (r.id, r.category, r.amount))
                .collect(),
        )
    };
    let q = db.query::<Entry>().work_limit(512);
    match case {
        Read::Lookup(id) => rows(db.get::<Entry>(id).unwrap().into_iter().collect()),
        Read::Prefix(c) => rows(
            db.index_prefix::<Entry>("category,id", &[c.into()], 10)
                .unwrap(),
        ),
        Read::Count(None) => ResultSet::Count(db.cardinality::<Entry>().unwrap()),
        Read::Count(Some(c)) => ResultSet::Count(
            q.using_index("category,id", &[c.into()])
                .filter("category", Comparison::Eq, c)
                .count_validated()
                .unwrap()
                .value,
        ),
        Read::List(m, c, v, n) => {
            let q = q
                .filter(
                    "category",
                    if m == 2 {
                        Comparison::Ge
                    } else {
                        Comparison::Eq
                    },
                    c,
                )
                .filter("amount", Comparison::Ge, v)
                .limit(n);
            let q = match m {
                0 => q.order_by(&["id"]),
                1 => q.order_by(&["id"]).descending(),
                2 => q.order_by(&["category", "id"]),
                3 => q.order_by_each(&[
                    ("amount", SortDirection::Descending),
                    ("id", SortDirection::Ascending),
                ]),
                _ => unreachable!(),
            };
            rows(q.fetch().unwrap().rows)
        }
        Read::Group(m, c, v) => {
            let q = q
                .filter(
                    "category",
                    if c.is_some() {
                        Comparison::Eq
                    } else {
                        Comparison::Ge
                    },
                    c.unwrap_or(0),
                )
                .filter("amount", Comparison::Ge, v);
            if m == 0 {
                return ResultSet::Groups(
                    q.group_count::<Category>()
                        .unwrap()
                        .value
                        .into_iter()
                        .map(|r| (r.key.category, r.count))
                        .collect(),
                );
            }
            let groups = match m {
                1 => q.group_sum::<Category, Amount>(),
                2 => q.group_min::<Category, Amount>(),
                3 => q.group_max::<Category, Amount>(),
                _ => unreachable!(),
            }
            .unwrap()
            .value;
            ResultSet::Groups(
                groups
                    .into_iter()
                    .map(|r| (r.key.category, r.value.unwrap().amount))
                    .collect(),
            )
        }
    }
}

fn legacy_read(case: Read) -> ResultSet {
    use icydb::db::query::{FieldRef, asc, count, desc, max_by, min_by, sum};
    use icydb::value::PublicValue;
    icydb::db::with_request_execution(|| {
        let db = db().unwrap();
        let tuple = |r: &Legacy| (r.id, r.category, r.amount);
        if let Read::Lookup(id) = case {
            return ResultSet::Rows(
                db.get::<Legacy>(Id::from_key(id))
                    .unwrap()
                    .iter()
                    .map(tuple)
                    .collect(),
            );
        }
        let q = db.query::<Legacy>().unwrap();
        match case {
            Read::Lookup(_) => unreachable!(),
            Read::Count(c) => {
                let q = if let Some(c) = c {
                    q.filter(FieldRef::new("category").eq(c))
                } else {
                    q
                };
                ResultSet::Count(q.execute_exact_count().unwrap())
            }
            Read::Prefix(c) => ResultSet::Rows(
                q.filter(FieldRef::new("category").eq(c))
                    .order_by(asc("id"))
                    .limit(10)
                    .execute_live_page(None)
                    .unwrap()
                    .rows
                    .iter()
                    .map(tuple)
                    .collect(),
            ),
            Read::List(m, c, v, n) => {
                let q = q
                    .filter(FieldRef::new("amount").gte(v))
                    .filter(if m == 2 {
                        FieldRef::new("category").gte(0_u64)
                    } else {
                        FieldRef::new("category").eq(c)
                    })
                    .limit(n as u32);
                let q = match m {
                    0 => q.order_by(asc("id")),
                    1 => q.order_by(desc("id")),
                    2 => q.order_by(asc("category")).order_by(asc("id")),
                    3 => q.order_by(desc("amount")).order_by(asc("id")),
                    _ => unreachable!(),
                };
                match q.execute_live_page(None) {
                    Ok(page) => ResultSet::Rows(page.rows.iter().map(tuple).collect()),
                    Err(icydb::db::TypedOperationError::Database(e))
                        if m == 3 && e.code().raw() == 174 =>
                    {
                        ResultSet::Refused
                    }
                    Err(e) => panic!("unexpected baseline error {e:?}"),
                }
            }
            Read::Group(m, c, v) => {
                let agg = match m {
                    0 => count(),
                    1 => sum("amount"),
                    2 => min_by("amount"),
                    3 => max_by("amount"),
                    _ => unreachable!(),
                };
                let g = q
                    .filter(c.map_or_else(
                        || FieldRef::new("category").gte(0_u64),
                        |v| FieldRef::new("category").eq(v),
                    ))
                    .filter(FieldRef::new("amount").gte(v))
                    .group_by("category")
                    .aggregate(agg)
                    .grouped_limits(4, 65536)
                    .execute_grouped()
                    .unwrap();
                assert!(g.next_cursor.is_none());
                ResultSet::Groups(
                    g.rows
                        .iter()
                        .map(|r| {
                            let PublicValue::Nat64(k) = r.group_key()[0].as_public() else {
                                panic!("category")
                            };
                            let v = match r.aggregate_values()[0].as_public() {
                                PublicValue::Nat64(v) => *v,
                                PublicValue::Decimal(v) => v.to_u64().unwrap(),
                                _ => panic!("aggregate"),
                            };
                            (*k, v)
                        })
                        .collect(),
                )
            }
        }
    })
}

// Construct SQL and bindings outside the timer. Prepare, execute and collect
// the rows inside each timed call; no cached SQLite statement is used.
struct Sql {
    text: String,
    args: Vec<i64>,
}
impl Sql {
    fn new(case: Read) -> Self {
        let fields = "SELECT id,category,amount FROM entry";
        let (text, args) = match case {
            Read::Lookup(id) => (format!("{fields} WHERE id=?"), vec![id as i64]),
            Read::Prefix(c) => (
                format!("{fields} WHERE category=? ORDER BY id LIMIT 10"),
                vec![c as i64],
            ),
            Read::Count(None) => ("SELECT count(*) FROM entry".into(), vec![]),
            Read::Count(Some(c)) => (
                "SELECT count(*) FROM entry WHERE category=?".into(),
                vec![c as i64],
            ),
            Read::List(m, c, v, n) => {
                let order = ["id", "id DESC", "category,id", "amount DESC,id"][m as usize];
                let cmp = if m == 2 { ">=" } else { "=" };
                (
                    format!("{fields} WHERE category{cmp}? AND amount>=? ORDER BY {order} LIMIT ?"),
                    vec![c as i64, v as i64, n as i64],
                )
            }
            Read::Group(m, c, v) => {
                let agg = ["count(*)", "sum(amount)", "min(amount)", "max(amount)"][m as usize];
                let cmp = if c.is_some() { "=" } else { ">=" };
                (
                    format!(
                        "SELECT category,{agg} FROM entry WHERE category{cmp}? AND amount>=? GROUP BY category ORDER BY category"
                    ),
                    vec![c.unwrap_or(0) as i64, v as i64],
                )
            }
        };
        Self { text, args }
    }

    fn run(&self, db: &Connection, case: Read) -> ResultSet {
        let mut stmt = db.prepare(&self.text).unwrap();
        let mut rs = stmt.query(rusqlite::params_from_iter(&self.args)).unwrap();
        let mut rows = Vec::new();
        let mut groups = Vec::new();
        while let Some(r) = rs.next().unwrap() {
            let get = |i| u64::try_from(r.get::<_, i64>(i).unwrap()).unwrap();
            match case {
                Read::Group(..) => groups.push((get(0), get(1))),
                Read::Count(_) => return ResultSet::Count(get(0)),
                _ => rows.push((get(0), get(1), get(2))),
            }
        }
        if matches!(case, Read::Group(..)) {
            ResultSet::Groups(groups)
        } else {
            ResultSet::Rows(rows)
        }
    }
}

fn create_ic() -> Database<VectorMemory> {
    let mut host = MemoryRuntime::new(VectorMemory::default()).unwrap();
    let key = "bench.native.v1";
    host.bootstrap(
        &SealedDeclarationSnapshot::new(vec![MemoryRequest::new("bench", key).unwrap()]).unwrap(),
        &AllocationGrants::new(vec![MemoryAuthority::new("bench", "bench.").unwrap()]).unwrap(),
        &GenericAllocationPolicy,
    )
    .unwrap();
    Database::create(
        host.open_memory(&StableKey::parse(key).unwrap()).unwrap(),
        Schema::new(&[Entry::SCHEMA]).unwrap(),
        [41; 16],
    )
    .unwrap()
}

fn write_ic(db: &mut Database<VectorMemory>, r: Row, insert: bool) {
    let mut batch = Batch::new();
    let row = Entry {
        id: r.0,
        category: r.1,
        amount: r.2,
    };
    if insert {
        batch.insert(&row).unwrap()
    } else {
        batch.replace(&row).unwrap()
    };
    db.commit(batch).unwrap();
}

fn write_legacy(r: Row, insert: bool) {
    icydb::db::with_request_execution(|| {
        let db = db().unwrap();
        let mut b = db.trusted_typed_write_batch();
        if insert {
            b.push(LegacyInsert {
                id: WriteCell::Value(Id::from_key(r.0)),
                category: WriteCell::Value(r.1),
                amount: WriteCell::Value(r.2),
            })
            .unwrap();
        } else {
            b.push(EntryReplace {
                id: Id::from_key(r.0),
                category: WriteCell::Value(r.1),
                amount: WriteCell::Value(r.2),
            })
            .unwrap();
        }
        b.execute().unwrap();
    });
    settle();
}

fn write_sql(db: &Connection, r: Row, insert: bool) {
    let sql = if insert {
        "INSERT INTO entry VALUES (?1,?2,?3)"
    } else {
        "UPDATE entry SET category=?2, amount=?3 WHERE id=?1"
    };
    assert_eq!(
        db.execute(sql, params![r.0 as i64, r.1 as i64, r.2 as i64])
            .unwrap(),
        1
    );
}

const SAMPLES: usize = 9;
fn median(values: &mut [u128]) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

#[expect(
    clippy::needless_range_loop,
    reason = "rotate engine order each round while recording samples by engine"
)]
fn profile(skew: bool) -> String {
    use std::fmt::Write;
    let label = if skew { "skewed" } else { "balanced" };
    icydb::db::provide_boot_entropy([41; 32]).unwrap();
    settle();
    let mut ic = create_ic();
    let sql = Connection::open_in_memory().unwrap();
    sql.execute_batch("CREATE TABLE entry(id INTEGER PRIMARY KEY,category INTEGER NOT NULL,amount INTEGER NOT NULL); CREATE INDEX category_id ON entry(category,id)").unwrap();
    let mut output = String::new();
    let mut existing = 0;
    for n in [32, 128, 511] {
        let mut inserts = [Vec::new(), Vec::new(), Vec::new()];
        for id in existing + 1..=n {
            let c = if skew && id % 16 < 13 { 0 } else { id % 4 };
            let r = (id, c, (id * 37 % 101) * 10);
            for offset in 0..3 {
                let e = (id as usize + offset) % 3;
                let t = Instant::now();
                match e {
                    0 => write_ic(&mut ic, r, true),
                    1 => write_legacy(r, true),
                    _ => write_sql(&sql, r, true),
                }
                inserts[e].push(t.elapsed().as_nanos());
            }
        }
        existing = n;
        writeln!(
            output,
            "{label},{n},insert,{},{},{}",
            median(&mut inserts[0]),
            median(&mut inserts[1]),
            median(&mut inserts[2])
        )
        .unwrap();
        for case in cases(n) {
            let query = Sql::new(case);
            let expected = query.run(&sql, case);
            let run = |e| match e {
                0 => ic_read(&ic, case),
                1 => legacy_read(case),
                _ => query.run(&sql, case),
            };
            let mut timings = [[0_u128; SAMPLES]; 3];
            let refused = matches!(case, Read::List(3, ..));
            for e in 0..3 {
                let actual = run(e);
                if e == 1 && refused {
                    assert_eq!(actual, ResultSet::Refused)
                } else {
                    assert_eq!(actual, expected, "{}", case.name())
                }
            }
            for i in 0..SAMPLES {
                for offset in 0..3 {
                    let e = (i + offset) % 3;
                    let t = Instant::now();
                    let actual = black_box(run(e));
                    timings[e][i] = t.elapsed().as_nanos();
                    if e == 1 && refused {
                        assert_eq!(actual, ResultSet::Refused)
                    } else {
                        assert_eq!(actual, expected)
                    }
                }
            }
            let legacy = if refused {
                "refused".into()
            } else {
                median(&mut timings[1]).to_string()
            };
            writeln!(
                output,
                "{label},{n},{},{},{legacy},{}",
                case.name(),
                median(&mut timings[0]),
                median(&mut timings[2])
            )
            .unwrap();
        }
        for (name, change_category) in [("replace-nonindexed", false), ("replace-indexed", true)] {
            let mut times = [[0_u128; SAMPLES]; 3];
            let id = n;
            let original = (
                id,
                if skew && id % 16 < 13 { 0 } else { id % 4 },
                (id * 37 % 101) * 10,
            );
            for i in 0..SAMPLES {
                let r = (id, original.1 + u64::from(change_category), original.2 + 1);
                for offset in 0..3 {
                    let e = (i + offset) % 3;
                    let t = Instant::now();
                    match e {
                        0 => write_ic(&mut ic, r, false),
                        1 => write_legacy(r, false),
                        _ => write_sql(&sql, r, false),
                    }
                    times[e][i] = t.elapsed().as_nanos();
                    let stored = match e {
                        0 => ic_read(&ic, Read::Lookup(id)),
                        1 => legacy_read(Read::Lookup(id)),
                        _ => Sql::new(Read::Lookup(id)).run(&sql, Read::Lookup(id)),
                    };
                    assert_eq!(stored, ResultSet::Rows(vec![r]));
                    match e {
                        0 => write_ic(&mut ic, original, false),
                        1 => write_legacy(original, false),
                        _ => write_sql(&sql, original, false),
                    }
                }
            }
            writeln!(
                output,
                "{label},{n},{name},{},{},{}",
                median(&mut times[0]),
                median(&mut times[1]),
                median(&mut times[2])
            )
            .unwrap();
        }
    }
    output
}
