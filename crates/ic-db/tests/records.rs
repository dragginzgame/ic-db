//! First public journey: real generated records, governed storage and final-state batches.

use enum_values::Lifecycle as State;
use ic_db::design::prelude::*;
use ic_db::{Batch, Database, Error};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

///
/// Entry
/// Exercises indexed access by `category` and `id` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["category", "id"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat64"))),
        field(name = "amount", value(item(prim = "Nat64")))
    )
)]
struct Entry {}

///
/// TokenRow
/// Exercises indexed access by `token` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(field = "token", unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "token", value(item(prim = "Nat64")))
    )
)]
struct TokenRow {}

///
/// Changed
/// Exercises an alternate `Entry` declaration under the same logical identity.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Entry",
    pk(field = "id"),
    index(fields = ["category", "id"]),
    fields(
        field(name = "value", value(item(prim = "Nat64"))),
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat64")))
    )
)]
struct Changed {}

///
/// Reordered
/// Exercises an alternate `Entry` declaration under the same logical identity.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "Entry",
    pk(field = "id"),
    index(fields = ["category", "id"]),
    fields(
        field(name = "amount", value(item(prim = "Nat64"))),
        field(name = "category", value(item(prim = "Nat64"))),
        field(name = "id", value(item(prim = "Nat64")))
    )
)]
struct Reordered {}

///
/// Document
/// Stores `title`, `note`, `published` and `priority` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "title", value(item(prim = "Text", max_bytes = 32))),
        field(name = "note", value(opt, item(prim = "Text", max_bytes = 64))),
        field(name = "published", value(item(prim = "Bool"))),
        field(name = "priority", value(opt, item(prim = "Nat64")))
    )
)]
struct Document {}

///
/// WideText
/// Stores `enabled` and `text` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "enabled", value(opt, item(prim = "Bool"))),
        field(name = "text", value(item(prim = "Text", max_bytes = 4096)))
    )
)]
struct WideText {}

///
/// SignedRow
/// Stores `delta` and `previous` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "delta", value(item(prim = "Int64"))),
        field(name = "previous", value(opt, item(prim = "Int64")))
    )
)]
struct SignedRow {}

///
/// WideRow
/// Stores `signed`, `unsigned`, `optional_signed` and `optional_unsigned` under the `id`
/// primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "signed", value(item(prim = "Int128"))),
        field(name = "unsigned", value(item(prim = "Nat128"))),
        field(name = "optional_signed", value(opt, item(prim = "Int128"))),
        field(name = "optional_unsigned", value(opt, item(prim = "Nat128")))
    )
)]
struct WideRow {}

///
/// BinaryRow
/// Stores `payload` and `optional` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 1024))),
        field(name = "optional", value(opt, item(prim = "Blob", max_bytes = 32)))
    )
)]
struct BinaryRow {}

///
/// EmptyBlob
/// Stores `payload` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 0)))
    )
)]
struct EmptyBlob {}

///
/// EmptyText
/// Stores `payload` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(prim = "Text", max_bytes = 0)))
    )
)]
struct EmptyText {}

///
/// WideBlob
/// Stores `payload` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 4096)))
    )
)]
struct WideBlob {}

///
/// CodecNames
/// Stores `writer` and `row` under the `reader` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "reader"),
    fields(
        field(name = "reader", value(item(prim = "Nat64"))),
        field(name = "writer", value(opt, item(prim = "Text", max_bytes = 8))),
        field(name = "row", value(item(prim = "Bool")))
    )
)]
struct CodecNames {}

///
/// NarrowRow
/// Exercises stored representations across 12 fields sharing the primary key `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "signed8", value(item(prim = "Int8"))),
        field(name = "signed16", value(item(prim = "Int16"))),
        field(name = "signed32", value(item(prim = "Int32"))),
        field(name = "unsigned8", value(item(prim = "Nat8"))),
        field(name = "unsigned16", value(item(prim = "Nat16"))),
        field(name = "unsigned32", value(item(prim = "Nat32"))),
        field(name = "optional_signed8", value(opt, item(prim = "Int8"))),
        field(name = "optional_signed16", value(opt, item(prim = "Int16"))),
        field(name = "optional_signed32", value(opt, item(prim = "Int32"))),
        field(name = "optional_unsigned8", value(opt, item(prim = "Nat8"))),
        field(name = "optional_unsigned16", value(opt, item(prim = "Nat16"))),
        field(name = "optional_unsigned32", value(opt, item(prim = "Nat32"))),
    )
)]
struct NarrowRow {}

///
/// InspectionRow
/// Exercises indexed access by `category` and `id` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["category", "id"]),
    fields(
        field(name = "a_blob", value(item(prim = "Blob", max_bytes = 2048))),
        field(name = "a_note", value(opt, item(prim = "Text", max_bytes = 64))),
        field(name = "category", value(item(prim = "Nat64"))),
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "z_flag", value(item(prim = "Bool"))),
        field(name = "z_text", value(item(prim = "Text", max_bytes = 64)))
    )
)]
struct InspectionRow {}

///
/// InspectionDependent
/// Exercises references to `InspectionRow` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["parent_id", "id"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "note", value(item(prim = "Text", max_bytes = 32))),
        field(name = "parent_id", value(item(prim = "Nat64")), references(entity = "InspectionRow", index = "parent_id,id"))
    )
)]
struct InspectionDependent {}

///
/// Parent
/// Exercises a row containing only its `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
struct Parent {}

///
/// Child
/// Exercises references to `Parent` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["parent_id", "id"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "parent_id", value(item(prim = "Nat64")), references(entity = "Parent", index = "parent_id,id"))
    )
)]
struct Child {}

///
/// Node
/// Exercises references to `Self` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(field = "next"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "next",
            value(item(prim = "Nat64")),
            references(entity = "Self", index = "next")
        )
    )
)]
struct Node {}

///
/// OptionalLink
/// Exercises references to `Self` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["next", "id"]),
    index(fields = ["alternate", "id"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "next", value(opt, item(prim = "Nat64")), references(entity = "Self", index = "next,id")),
        field(name = "alternate", value(opt, item(prim = "Nat64")), references(entity = "Self", index = "alternate,id"))
    )
)]
struct OptionalLink {}

fn schema() -> Schema {
    Schema::new(&[TokenRow::SCHEMA, Entry::SCHEMA]).unwrap()
}

fn host(backing: VectorMemory) -> MemoryRuntime<VectorMemory> {
    let mut host = MemoryRuntime::new(backing).unwrap();
    let declarations =
        SealedDeclarationSnapshot::new(vec![MemoryRequest::new("test", "test.db.v1").unwrap()])
            .unwrap();
    let grants =
        AllocationGrants::new(vec![MemoryAuthority::new("test", "test.").unwrap()]).unwrap();
    host.bootstrap(&declarations, &grants, &GenericAllocationPolicy)
        .unwrap();

    host
}

#[test]
fn owned_codec_refuses_incomplete_rows_and_unbounded_lengths() {
    // Authored names must not shadow the generated decoder's local variables.
    let named = CodecNames {
        reader: 7,
        writer: Some("bytes".into()),
        row: true,
    };
    let bytes = [
        7, 0, 0, 0, 0, 0, 0, 0, 1, 1, 5, 0, 0, 0, b'b', b'y', b't', b'e', b's',
    ];
    assert_eq!(named.encode().unwrap(), bytes);
    assert_eq!(CodecNames::decode(&bytes).unwrap(), named);
    let valid = [
        1, 0, 0, 0, 0, 0, 0, 0, 1, 2, 0, 0, 0, 0, 255, 3, 0, 0, 0, 0, 128, 255,
    ];
    assert!(BinaryRow::decode(&valid).is_ok());
    for end in 0..valid.len() {
        assert!(matches!(
            BinaryRow::decode(&valid[..end]),
            Err(Error::Corrupt)
        ));
    }
    for malformed in [
        // u32::MAX must refuse before allocating, on native and 32-bit Wasm.
        vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255],
        // Big-endian 1 is not a one-byte payload under the little-endian contract.
        vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 42],
        [valid.as_slice(), &[0]].concat(),
        vec![0; 4097],
    ] {
        assert!(matches!(BinaryRow::decode(&malformed), Err(Error::Corrupt)));
    }
    let full = WideBlob {
        id: 1,
        payload: vec![255; 4084],
    };
    let bytes = full.encode().unwrap();
    assert_eq!(bytes.len(), 4096); // eight-byte key plus four-byte length
    assert_eq!(WideBlob::decode(&bytes).unwrap(), full);
    let over = WideBlob {
        id: 1,
        payload: vec![255; 4085],
    };
    assert!(matches!(over.encode(), Err(Error::Limit)));
}

#[test]
fn previous_codec_identities_are_refused_without_changing_storage() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let contract = || Schema::new(&[Entry::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [7; 16]).unwrap();
    let mut batch = Batch::new();
    batch
        .insert(&Entry {
            id: 7,
            category: 2,
            amount: 42,
        })
        .unwrap();
    db.commit(batch).unwrap();
    drop(db);
    // Earlier numbered contracts: row codecs v1 (Wire), v2 (owned varint),
    // and v3 (fixed width). Admission refuses before any row decoding.
    // Admission must refuse before interpreting those bytes under the new codec.
    for old in [
        "daeb2925a2a9e739d630f56189a8956a63e88fe624984a25034c5600ca2e7b60",
        "cf31e2057b1a4137cc59547765556c77ad8aaad97bf235b66d962da134e9d4aa",
        "a1e8982196ded1c94a3625762e178927acc467051a8c37210a4e84c7c76ab851",
        // Named schema with owned row bytes, before complete-key semantics v2.
        "0b7e8c6d325ad8572c3bc388975ceebf7d26a03c266ea2e724ecf7b078aac3db",
    ] {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
        let mut manifest = raw.get(&vec![0]).unwrap();
        for i in 0..32 {
            manifest[8 + i] = u8::from_str_radix(&old[i * 2..i * 2 + 2], 16).unwrap();
        }
        raw.insert(vec![0], manifest);
        raw.insert(vec![2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 7], vec![42, 2, 7]);
        drop(raw);
        let before = backing.borrow().clone();
        assert!(matches!(
            Database::open(runtime.open_memory(&key).unwrap(), contract()),
            Err(Error::IncompatibleSchema)
        ));
        assert_eq!(*backing.borrow(), before);
    }
}

#[test]
fn generated_codec_and_identity_ignore_declaration_order_but_not_names() {
    let row = Entry {
        id: 7,
        category: 2,
        amount: 42,
    };
    let golden = [
        42, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(Entry::decode(&golden).unwrap(), row);
    // Carry IcyDB's exact-width refusal cases to complete schema-selected rows.
    for end in 0..golden.len() {
        assert!(matches!(Entry::decode(&golden[..end]), Err(Error::Corrupt)));
    }
    assert!(matches!(
        Entry::decode(&[golden.as_slice(), &[0]].concat()),
        Err(Error::Corrupt)
    ));
    let one = Schema::new(&[Entry::SCHEMA]).unwrap();
    // Independent literal grammar assembled with Node framing/UTF-8 lengths;
    // digest verified independently, not the product's schema serializer.
    assert_eq!(
        one.fingerprint().map(|b| format!("{b:02x}")).concat(),
        "34c3b07f55b52646501835ddd758029761e7c40901f48610559c670b50b20626"
    );
    assert_eq!(
        one.fingerprint(),
        Schema::new(&[Reordered::SCHEMA]).unwrap().fingerprint()
    );
    assert_ne!(
        one.fingerprint(),
        Schema::new(&[Changed::SCHEMA]).unwrap().fingerprint()
    );
    assert!(Schema::new(&[Entry::SCHEMA, Changed::SCHEMA]).is_err());
    assert_eq!(
        Entry::SCHEMA
            .fields
            .iter()
            .map(|field| field.name)
            .collect::<Vec<_>>(),
        ["amount", "category", "id"]
    );
    assert_eq!(Entry::SCHEMA.indexes[0].fields, ["category", "id"]);
}

#[test]
fn signed_values_preserve_exact_boundaries_and_absence_across_reopen() {
    let row = SignedRow {
        id: 7,
        delta: -1,
        previous: Some(0),
    };
    // IcyDB's fixed little-endian scalar payloads, without its per-slot envelope.
    // Optional presence is a separate 0/1 byte; signed values use two's complement.
    let mut present_zero = vec![255; 8];
    present_zero.extend([7, 0, 0, 0, 0, 0, 0, 0, 1]);
    present_zero.extend([0; 8]);
    assert_eq!(row.encode().unwrap(), present_zero);
    let minimum = vec![0, 0, 0, 0, 0, 0, 0, 128, 7, 0, 0, 0, 0, 0, 0, 0, 0];
    let min_row = SignedRow {
        id: 7,
        delta: i64::MIN,
        previous: None,
    };
    assert_eq!(min_row.encode().unwrap(), minimum);
    assert_eq!(SignedRow::decode(&minimum).unwrap(), min_row);
    let maximum = vec![
        255, 255, 255, 255, 255, 255, 255, 127, 7, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let max_row = SignedRow {
        id: 7,
        delta: i64::MAX,
        previous: None,
    };
    assert_eq!(max_row.encode().unwrap(), maximum);
    assert_eq!(SignedRow::decode(&maximum).unwrap(), max_row);
    for valid in [&minimum, &maximum, &present_zero] {
        for end in 0..valid.len() {
            assert!(matches!(
                SignedRow::decode(&valid[..end]),
                Err(Error::Corrupt)
            ));
        }
        assert!(matches!(
            SignedRow::decode(&[valid.as_slice(), &[0]].concat()),
            Err(Error::Corrupt)
        ));
    }
    let mut invalid_option = minimum;
    invalid_option[16] = 2;
    assert!(matches!(
        SignedRow::decode(&invalid_option),
        Err(Error::Corrupt)
    ));

    let contract = || Schema::new(&[SignedRow::SCHEMA]).unwrap();
    // Independent Node byte framing and SHA-256 schema vector.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "324c35cdb4ded35300f3dd06b351efe8ec9f38be98c3f0d8712d033344a8cc8a"
    );
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [5; 16]).unwrap();
    let values = [i64::MIN, -65, -64, -1, 0, 1, 63, 64, i64::MAX];
    let rows: Vec<_> = values
        .into_iter()
        .enumerate()
        .map(|(id, delta)| SignedRow {
            id: id as u64,
            delta,
            previous: if id % 2 == 0 { Some(delta) } else { None },
        })
        .collect();
    let mut batch = Batch::new();
    for row in &rows {
        batch.insert(row).unwrap();
    }
    db.commit(batch).unwrap();
    assert_eq!(db.page::<SignedRow>(None, 32).unwrap().rows, rows);
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.replace(&rows[0]).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(*backing.borrow(), before);
    drop(db);
    drop(runtime);
    let runtime = host(backing.clone());
    let mut fields = SignedRow::SCHEMA.fields.to_vec();
    fields[0].value = ic_db::FieldType::Nat64;
    // Metadata-only incompatible executable; opening must refuse before decoding.
    let unsigned = Box::leak(Box::new(ic_db::EntitySchema {
        fields: Box::leak(fields.into_boxed_slice()),
        ..*SignedRow::SCHEMA
    }));
    assert!(matches!(
        Database::open(
            runtime.open_memory(&key).unwrap(),
            Schema::new(&[unsigned]).unwrap()
        ),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(db.page::<SignedRow>(None, 32).unwrap().rows, rows);
    let changed = SignedRow {
        id: 0,
        delta: i64::MAX,
        previous: Some(i64::MIN),
    };
    let mut batch = Batch::new();
    batch.replace(&changed).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.get::<SignedRow>(0).unwrap(), Some(changed));
    assert_eq!(db.revision::<SignedRow>().unwrap(), 2);
}

#[test]
fn wide_integers_preserve_all_bits_and_require_exact_widths() {
    let minimum = WideRow {
        id: 7,
        signed: i128::MIN,
        unsigned: u128::MAX,
        optional_signed: None,
        optional_unsigned: None,
    };
    // Same exact-width rule as IcyDB's owned scalar codec, extended to 128 bits.
    // Canonical order: id, absent options, signed little-endian, unsigned.
    let mut bytes = vec![7, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    bytes.extend([0; 15]);
    bytes.push(128);
    bytes.extend([255; 16]);
    assert_eq!(minimum.encode().unwrap(), bytes);
    assert_eq!(WideRow::decode(&bytes).unwrap(), minimum);
    let mut maximum = minimum;
    maximum.signed = i128::MAX;
    bytes[10..25].fill(255);
    bytes[25] = 127;
    assert_eq!(maximum.encode().unwrap(), bytes);
    assert_eq!(WideRow::decode(&bytes).unwrap(), maximum);
    for end in 0..bytes.len() {
        assert!(matches!(
            WideRow::decode(&bytes[..end]),
            Err(Error::Corrupt)
        ));
    }
    assert!(matches!(
        WideRow::decode(&[bytes.as_slice(), &[0]].concat()),
        Err(Error::Corrupt)
    ));
    let zero = WideRow {
        id: 7,
        signed: 0,
        unsigned: 0,
        optional_signed: Some(0),
        optional_unsigned: Some(0),
    };
    let mut zero_bytes = vec![0; 74];
    zero_bytes[0] = 7;
    zero_bytes[8] = 1;
    zero_bytes[25] = 1;
    assert_eq!(zero.encode().unwrap(), zero_bytes);
    assert_eq!(WideRow::decode(&zero_bytes).unwrap(), zero);
    let absent = WideRow {
        optional_signed: None,
        optional_unsigned: None,
        ..zero
    };
    let mut absent_bytes = vec![0; 42];
    absent_bytes[0] = 7;
    assert_eq!(absent.encode().unwrap(), absent_bytes);
    assert_ne!(zero, absent);

    let contract = || Schema::new(&[WideRow::SCHEMA]).unwrap();
    // Independent Node byte framing + SHA-256 contract vector.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "4b181c9d29c86462df255b917118521fa3926c6971bfe8aa96a9a8fd08f4eefb"
    );
    let values = [
        (i128::MIN, u128::MAX),
        (i128::MAX, 1_u128 << 127),
        (-(1_i128 << 100) - 1, (1_u128 << 100) + 1),
        (i128::from(i64::MIN) - 1, u128::from(u64::MAX) + 1),
        (i128::from(i64::MAX) + 1, u128::from(u64::MAX)),
        (-1, 1),
        (0, 0),
        (1, 127),
    ];
    let rows: Vec<_> = values
        .into_iter()
        .enumerate()
        .map(|(id, (signed, unsigned))| WideRow {
            id: id as u64,
            signed,
            unsigned,
            optional_signed: Some(signed),
            optional_unsigned: Some(unsigned),
        })
        .collect();
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [6; 16]).unwrap();
    let mut batch = Batch::new();
    for row in &rows {
        batch.insert(row).unwrap();
    }
    db.commit(batch).unwrap();
    assert_eq!(db.page::<WideRow>(None, 32).unwrap().rows, rows);
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.replace(&rows[0]).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(*backing.borrow(), before);
    drop(db);
    drop(runtime);
    let runtime = host(backing.clone());
    for replacement in [ic_db::FieldType::Int64, ic_db::FieldType::Nat128] {
        let mut fields = WideRow::SCHEMA.fields.to_vec();
        fields[3].value = replacement;
        let altered = Box::leak(Box::new(ic_db::EntitySchema {
            fields: Box::leak(fields.into_boxed_slice()),
            ..*WideRow::SCHEMA
        }));
        assert!(matches!(
            Database::open(
                runtime.open_memory(&key).unwrap(),
                Schema::new(&[altered]).unwrap()
            ),
            Err(Error::IncompatibleSchema)
        ));
        assert_eq!(*backing.borrow(), before);
    }
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(db.page::<WideRow>(None, 32).unwrap().rows, rows);
    let changed = WideRow {
        id: 0,
        signed: i128::MAX,
        unsigned: 1_u128 << 100,
        optional_signed: None,
        optional_unsigned: None,
    };
    let mut batch = Batch::new();
    batch.replace(&changed).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.get::<WideRow>(0).unwrap(), Some(changed));
    assert_eq!(db.revision::<WideRow>().unwrap(), 2);
}

#[test]
fn mixed_values_preserve_absence_and_enforce_bytes_on_write_and_reopen() {
    use ic_db::{EntitySchema, FieldType};
    let row = Document {
        id: 1,
        title: "é".into(),
        note: None,
        published: false,
        priority: Some(0),
    };
    // id, note, priority, published, title; fixed integers and u32 byte lengths.
    let golden = [
        1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0xc3, 0xa9,
    ];
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(Document::decode(&golden).unwrap(), row);
    let mut bad_option = golden.to_vec();
    bad_option[8] = 2;
    let mut bad_bool = golden.to_vec();
    bad_bool[18] = 2;
    let mut bad_utf8 = golden.to_vec();
    bad_utf8[23] = 255;
    for malformed in [bad_option, bad_bool, bad_utf8] {
        assert!(matches!(Document::decode(&malformed), Err(Error::Corrupt)));
    }
    let mut changed = row.clone();
    changed.note = Some(String::new());
    assert_ne!(changed.encode().unwrap(), golden);
    assert_eq!(
        Document::decode(&changed.encode().unwrap()).unwrap(),
        changed
    );
    changed.title = "é".repeat(16);
    changed.note = Some("é".repeat(32));
    assert_eq!(
        Document::decode(&changed.encode().unwrap()).unwrap(),
        changed
    );
    changed.title.push('x');
    assert!(matches!(changed.encode(), Err(Error::Limit)));
    // Independent payload producer bypasses application validation: decode must
    // still refuse the oversized text under the admitted field contract.
    let mut bytes = vec![
        1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 33, 0, 0, 0,
    ];
    bytes.extend_from_slice(changed.title.as_bytes());
    assert!(matches!(Document::decode(&bytes), Err(Error::Corrupt)));
    changed.title.clear();
    changed.note = Some("x".repeat(65));
    assert!(matches!(changed.encode(), Err(Error::Limit)));
    let wide = WideText {
        id: 1,
        enabled: None,
        text: "x".repeat(4096),
    };
    assert!(wide.validate().is_ok());
    assert!(matches!(wide.encode(), Err(Error::Limit))); // full row framing exceeds 4 KiB
    for enabled in [None, Some(false), Some(true)] {
        let row = WideText {
            id: u64::MAX,
            enabled,
            text: String::new(),
        };
        assert_eq!(WideText::decode(&row.encode().unwrap()).unwrap(), row);
    }
    let contract = || Schema::new(&[Entry::SCHEMA, Document::SCHEMA]).unwrap();
    let fingerprint = contract().fingerprint();
    // Mixed-type contract assembled independently with independent Node byte framing, UTF-8
    // name lengths and OpenSSL SHA-256, alongside the unchanged Nat64 vector.
    assert_eq!(
        fingerprint.map(|b| format!("{b:02x}")).concat(),
        "3adaa8dc2c8318fae48cc3dce3f736a612e6b4957f08e86695db9defa935414e"
    );
    for variant in 0..3 {
        let mut fields = Document::SCHEMA.fields.to_vec();
        match variant {
            0 => fields[4].value = FieldType::Text { max_bytes: 33 },
            1 => fields[3].optional = true,
            _ => fields[3].value = FieldType::Nat64,
        }
        // Metadata-only sensitivity control; never use this mismatched codec to persist.
        let altered = Box::leak(Box::new(EntitySchema {
            fields: Box::leak(fields.into_boxed_slice()),
            ..*Document::SCHEMA
        }));
        assert_ne!(
            Schema::new(&[Entry::SCHEMA, altered])
                .unwrap()
                .fingerprint(),
            fingerprint
        );
    }
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [3; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    assert!(matches!(batch.replace(&changed), Err(Error::Limit)));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(*backing.borrow(), before);
    let mut row = row;
    row.note = Some(String::new());
    row.published = true;
    row.priority = None;
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.revision::<Document>().unwrap(), 2);
    assert_eq!(db.revision::<Entry>().unwrap(), 0);
    drop(db);
    drop(runtime);
    let runtime = host(backing);
    let db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(db.get::<Document>(1).unwrap(), Some(row));
    assert_eq!(db.revision::<Document>().unwrap(), 2);
}

#[test]
fn writes_reopen_and_final_state_constraints_use_real_generated_types() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), schema(), [1; 16]).unwrap();
    let mut batch = Batch::new();
    batch
        .insert(&Entry {
            id: 1,
            category: 2,
            amount: 10,
        })
        .unwrap();
    batch
        .insert(&Entry {
            id: 2,
            category: 2,
            amount: 20,
        })
        .unwrap();
    batch.insert(&TokenRow { id: 1, token: 3 }).unwrap();
    batch.insert(&TokenRow { id: 2, token: 4 }).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.revision::<Entry>().unwrap(), 1);
    assert_eq!(db.revision::<TokenRow>().unwrap(), 1);
    assert_eq!(db.get::<Entry>(1).unwrap().unwrap().amount, 10);
    assert!(db.get::<Entry>(99).unwrap().is_none());
    assert!(matches!(db.get::<Changed>(1), Err(Error::UnknownEntity)));
    assert_eq!(
        db.index_prefix::<Entry>("category,id", &[2_u64.into()], 3)
            .unwrap()
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    let page = db.page::<Entry>(None, 1).unwrap();
    assert_eq!(page.rows[0].id, 1);
    assert_eq!(db.page::<Entry>(page.next_after, 3).unwrap().rows[0].id, 2);
    assert!(db.page::<Entry>(Some(u64::MAX), 3).unwrap().rows.is_empty());
    assert!(matches!(db.page::<Entry>(None, 0), Err(Error::Limit)));

    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch
        .replace(&Entry {
            id: 1,
            category: 2,
            amount: 10,
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert_eq!(*backing.borrow(), before);

    let mut batch = Batch::new();
    batch
        .insert(&Entry {
            id: 3,
            category: 8,
            amount: 30,
        })
        .unwrap();
    batch
        .insert(&Entry {
            id: 3,
            category: 9,
            amount: 31,
        })
        .unwrap();
    assert!(matches!(db.commit(batch), Err(Error::DuplicateTarget)));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    batch.insert(&TokenRow { id: 3, token: 5 }).unwrap();
    batch.insert(&TokenRow { id: 4, token: 5 }).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);

    let mut batch = Batch::new();
    batch.replace(&TokenRow { id: 1, token: 4 }).unwrap();
    batch.replace(&TokenRow { id: 2, token: 3 }).unwrap();
    batch
        .replace(&Entry {
            id: 1,
            category: 9,
            amount: 11,
        })
        .unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.revision::<TokenRow>().unwrap(), 2);
    assert_eq!(
        db.index_prefix::<TokenRow>("token", &[4_u64.into()], 3)
            .unwrap()[0]
            .id,
        1
    );
    assert_eq!(
        db.index_prefix::<Entry>("category,id", &[2_u64.into()], 3)
            .unwrap()[0]
            .id,
        2
    );
    assert_eq!(
        db.index_prefix::<Entry>("category,id", &[9_u64.into()], 3)
            .unwrap()[0]
            .amount,
        11
    );
    let mut batch = Batch::new();
    batch.insert(&TokenRow { id: 3, token: 4 }).unwrap();
    batch.delete::<TokenRow>(1).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(
        db.index_prefix::<TokenRow>("token", &[4_u64.into()], 3)
            .unwrap()[0]
            .id,
        3
    );
    drop(db);
    drop(runtime);

    let runtime = host(backing.clone());
    let db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
    assert_eq!(db.incarnation(), [1; 16]);
    assert_eq!(db.get::<Entry>(1).unwrap().unwrap().amount, 11);
    assert_eq!(db.revision::<TokenRow>().unwrap(), 3);
    drop(db);
    let before = backing.borrow().clone();
    assert!(matches!(
        Database::open(
            runtime.open_memory(&key).unwrap(),
            Schema::new(&[Changed::SCHEMA]).unwrap()
        ),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
    assert!(matches!(
        Database::create(runtime.open_memory(&key).unwrap(), schema(), [2; 16]),
        Err(Error::Exists)
    ));
    assert_eq!(*backing.borrow(), before);
    // Independent corruption control removes authority, never reconstructs it.
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    // Literal physical keys independently check row/index representation. A stale
    // index pointing at a decodable row must not silently return that wrong row.
    let row = vec![2, 156, 191, 237, 51, 0, 0, 0, 0, 0, 0, 0, 1];
    let original = vec![
        11, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(raw.get(&row), Some(original.clone()));
    let index = vec![
        3, 156, 191, 237, 51, 16, 241, 135, 42, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 1, 0,
        0, 0, 0, 0, 0, 0, 1,
    ];
    assert_eq!(raw.get(&index), Some(1_u64.to_be_bytes().to_vec()));
    raw.insert(index.clone(), 2_u64.to_be_bytes().to_vec());
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
    assert!(matches!(
        db.index_prefix::<Entry>("category,id", &[9_u64.into()], 3),
        Err(Error::Corrupt)
    ));
    // The fluent scan must reject the divergent owner before a residual filter
    // can discard it. Both the index entry and its owner row decode successfully.
    assert!(matches!(
        db.query::<Entry>()
            .using_index("category,id", &[9_u64.into()])
            .filter("amount", ic_db::Comparison::Eq, 0_u64)
            .fetch(),
        Err(Error::Corrupt)
    ));
    // Editing an unrelated field still verifies the owner before retaining its entry.
    let corrupt = backing.borrow().clone();
    let mut batch = Batch::new();
    batch
        .replace(&Entry {
            id: 1,
            category: 9,
            amount: 12,
        })
        .unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), corrupt);
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    raw.insert(index, 1_u64.to_be_bytes().to_vec());
    drop(raw);
    // Each read boundary must still reject malformed bytes and a decoded key
    // that disagrees with its physical row key after redundant decodes are removed.
    let mut wrong_key = original.clone();
    wrong_key[16] = 2;
    for corrupt in [vec![0x80], wrong_key] {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
        raw.insert(row.clone(), corrupt);
        drop(raw);
        let before = backing.borrow().clone();
        let db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
        assert!(matches!(db.get::<Entry>(1), Err(Error::Corrupt)));
        assert!(matches!(db.page::<Entry>(None, 1), Err(Error::Corrupt)));
        assert!(matches!(
            db.index_prefix::<Entry>("category,id", &[9_u64.into()], 1),
            Err(Error::Corrupt)
        ));
        assert_eq!(*backing.borrow(), before);
    }
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    raw.insert(row, original);
    raw.remove(&vec![0]);
    drop(raw);
    let before = backing.borrow().clone();
    assert!(matches!(
        Database::open(runtime.open_memory(&key).unwrap(), schema()),
        Err(Error::Corrupt)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn references_use_final_batch_state_and_restricted_indexed_deletion() {
    use ic_db::EntitySchema;
    // Independent Node byte framing + SHA-256 vector, including the reference tag,
    // target identity and reverse-index identity. Existing scalar vectors stay fixed.
    assert_eq!(
        Schema::new(&[Parent::SCHEMA, Child::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "2669a982e08ec0931505e7d268d3e97329afef6b24b85f83c326801e670b08ec"
    );
    assert!(matches!(
        Schema::new(&[Child::SCHEMA]),
        Err(Error::InvalidSchema)
    ));
    let contract = || Schema::new(&[Parent::SCHEMA, Child::SCHEMA, Node::SCHEMA]).unwrap();
    for target in [None, Some(Node::SCHEMA.id)] {
        let mut fields = Child::SCHEMA.fields.to_vec();
        fields[1].reference = target.map(|entity| ic_db::ReferenceSchema {
            entity_name: "Node",
            entity,
            index: Child::SCHEMA.indexes[0].id,
        });
        // Metadata-only sensitivity control; never persist through a mismatched codec.
        let altered = Box::leak(Box::new(EntitySchema {
            fields: Box::leak(fields.into_boxed_slice()),
            ..*Child::SCHEMA
        }));
        assert_ne!(
            Schema::new(&[Parent::SCHEMA, altered, Node::SCHEMA])
                .unwrap()
                .fingerprint(),
            contract().fingerprint()
        );
    }
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [4; 16]).unwrap();
    let mut batch = Batch::new();
    batch
        .insert(&Child {
            id: 1,
            parent_id: 1,
        })
        .unwrap(); // dependent before parent
    batch.insert(&Parent { id: 1 }).unwrap();
    batch.insert(&Parent { id: 2 }).unwrap();
    batch
        .insert(&Child {
            id: 2,
            parent_id: 1,
        })
        .unwrap();
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch
        .insert(&Child {
            id: 3,
            parent_id: 99,
        })
        .unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    let mut batch = Batch::new();
    batch.delete::<Parent>(1).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    // Removing just one dependent still leaves a survivor; no partial delete.
    let mut batch = Batch::new();
    batch.delete::<Parent>(1).unwrap();
    batch.delete::<Child>(1).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    batch.delete::<Parent>(1).unwrap();
    batch
        .replace(&Child {
            id: 1,
            parent_id: 2,
        })
        .unwrap();
    batch.delete::<Child>(2).unwrap();
    db.commit(batch).unwrap();
    assert!(db.get::<Parent>(1).unwrap().is_none());
    assert!(
        db.index_prefix::<Child>("parent_id,id", &[1_u64.into()], 32)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.index_prefix::<Child>("parent_id,id", &[2_u64.into()], 32)
            .unwrap(),
        vec![Child {
            id: 1,
            parent_id: 2
        }]
    );
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.delete::<Parent>(2).unwrap();
    batch
        .insert(&Child {
            id: 3,
            parent_id: 2,
        })
        .unwrap();
    batch.delete::<Child>(1).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    // Final-state cycles and self references require no topological executor.
    let mut batch = Batch::new();
    batch.insert(&Node { id: 1, next: 2 }).unwrap();
    batch.insert(&Node { id: 2, next: 1 }).unwrap();
    batch.insert(&Node { id: 3, next: 3 }).unwrap();
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.delete::<Node>(1).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    for id in 1..=3 {
        batch.delete::<Node>(id).unwrap();
    }
    db.commit(batch).unwrap();
    // Put the surviving dependent just beyond 31 staged reverse-index removals.
    // Also fill the adjacent parent prefix, which must not block deletion of 3.
    let mut batch = Batch::new();
    batch.insert(&Parent { id: 3 }).unwrap();
    for id in 100..131 {
        batch.insert(&Child { id, parent_id: 3 }).unwrap();
    }
    db.commit(batch).unwrap();
    let mut batch = Batch::new();
    batch
        .insert(&Child {
            id: 131,
            parent_id: 3,
        })
        .unwrap();
    batch.insert(&Parent { id: 4 }).unwrap();
    db.commit(batch).unwrap();
    for chunk in 0..8 {
        let mut batch = Batch::new();
        for offset in 0..32 {
            batch
                .insert(&Child {
                    id: 200 + chunk * 32 + offset,
                    parent_id: 4,
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
    }
    let remove_family = || {
        let mut batch = Batch::new();
        batch.delete::<Parent>(3).unwrap();
        for id in 100..131 {
            batch.delete::<Child>(id).unwrap();
        }
        batch
    };
    let before = backing.borrow().clone();
    assert!(matches!(
        db.commit(remove_family()),
        Err(Error::ReferenceViolation)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    batch
        .replace(&Child {
            id: 131,
            parent_id: 4,
        })
        .unwrap();
    db.commit(batch).unwrap();
    db.commit(remove_family()).unwrap();
    drop(db);
    drop(runtime);
    let runtime = host(backing.clone());
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.delete::<Parent>(2).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    batch.delete::<Parent>(2).unwrap();
    batch.delete::<Child>(1).unwrap();
    db.commit(batch).unwrap();
    assert!(db.get::<Parent>(3).unwrap().is_none());
    assert!(
        db.index_prefix::<Child>("parent_id,id", &[3_u64.into()], 32)
            .unwrap()
            .is_empty()
    );
    assert_eq!(db.cardinality::<Child>().unwrap(), 257);
    let prefix = [4_u64.into()];
    let children = || {
        db.query::<Child>()
            .using_index("parent_id,id", &prefix)
            .limit(32)
    };
    let mut page = children().fetch().unwrap();
    let mut ids = Vec::new();
    loop {
        for child in page.rows {
            assert_eq!(child.parent_id, 4);
            ids.push(child.id);
        }
        let Some(cursor) = page.next else { break };
        page = children().after(&cursor).fetch().unwrap();
    }
    assert_eq!(
        ids,
        std::iter::once(131).chain(200..456).collect::<Vec<_>>()
    );
}

#[test]
fn stored_key_inspection_validates_unselected_fields_before_mutating() {
    let row = InspectionRow {
        a_blob: vec![255; 1024],
        a_note: None,
        category: 2,
        id: 1,
        z_flag: true,
        z_text: "é".into(),
    };
    // Independent layout includes variable bytes before keys and text after them.
    let mut original = vec![0, 4, 0, 0];
    original.extend([255; 1024]);
    original.push(0);
    original.extend([2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
    original.extend([1, 2, 0, 0, 0, 0xc3, 0xa9]);
    assert_eq!(row.encode().unwrap(), original);
    let contract = || Schema::new(&[InspectionRow::SCHEMA, InspectionDependent::SCHEMA]).unwrap();
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [8; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    let changed = InspectionRow {
        category: 3,
        a_note: Some(String::new()),
        ..row.clone()
    };
    let mut batch = Batch::new();
    batch.replace(&changed).unwrap();
    db.commit(batch).unwrap();
    assert!(
        db.index_prefix::<InspectionRow>("category,id", &[2_u64.into()], 3)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.index_prefix::<InspectionRow>("category,id", &[3_u64.into()], 3)
            .unwrap(),
        [changed]
    );
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let physical_key = vec![2, 16, 218, 7, 153, 0, 0, 0, 0, 0, 0, 0, 1];
    let mut oversized_blob = original.clone();
    oversized_blob[..4].copy_from_slice(&[1, 8, 0, 0]); // 2049 exceeds field bound
    let mut bad_option = original.clone();
    bad_option[1028] = 2;
    let mut bad_bool = original.clone();
    bad_bool[1045] = 2;
    let mut bad_utf8 = original.clone();
    bad_utf8[1051] = 255;
    let oversized_text = [&original[..1046], &[65, 0, 0, 0], &[b'x'; 65]].concat();
    let mut wrong_key = original.clone();
    wrong_key[1037] = 2;
    let dependent = InspectionDependent {
        id: 1,
        note: "ok".into(),
        parent_id: 1,
    };
    for malformed in [
        oversized_blob,
        bad_option,
        bad_bool,
        bad_utf8,
        oversized_text,
        wrong_key,
        original[..1051].to_vec(),
        [original.as_slice(), &[0]].concat(),
    ] {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
        raw.insert(physical_key.clone(), malformed);
        drop(raw);
        let before = backing.borrow().clone();
        let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
        let mut replace = Batch::new();
        replace.replace(&row).unwrap();
        assert!(matches!(db.commit(replace), Err(Error::Corrupt)));
        let mut delete = Batch::new();
        delete.delete::<InspectionRow>(1).unwrap();
        assert!(matches!(db.commit(delete), Err(Error::Corrupt)));
        // Target-existence checking must validate the target's unselected fields.
        let mut reference = Batch::new();
        reference.insert(&dependent).unwrap();
        assert!(matches!(db.commit(reference), Err(Error::Corrupt)));
        assert_eq!(*backing.borrow(), before);
    }
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    raw.insert(physical_key, original);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let mut batch = Batch::new();
    batch.insert(&dependent).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    // Reverse-reference inspection also rejects corruption outside its key fields.
    let dependent_key = vec![2, 25, 91, 71, 26, 0, 0, 0, 0, 0, 0, 0, 1];
    let mut invalid = vec![1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 255];
    invalid.extend([1, 0, 0, 0, 0, 0, 0, 0]);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    raw.insert(dependent_key.clone(), invalid);
    drop(raw);
    let before = backing.borrow().clone();
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let mut batch = Batch::new();
    batch.delete::<InspectionRow>(1).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    raw.insert(dependent_key, dependent.encode().unwrap());
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let mut batch = Batch::new();
    batch.delete::<InspectionRow>(1).unwrap();
    batch.delete::<InspectionDependent>(1).unwrap();
    db.commit(batch).unwrap();
    assert!(
        db.index_prefix::<InspectionRow>("category,id", &[2_u64.into()], 3)
            .unwrap()
            .is_empty()
    );
    assert!(
        db.page::<InspectionDependent>(None, 3)
            .unwrap()
            .rows
            .is_empty()
    );
}

#[test]
fn narrow_integers_preserve_width_signedness_and_absence_across_reopen() {
    let row = NarrowRow {
        id: 1,
        signed8: i8::MIN,
        signed16: i16::MIN,
        signed32: i32::MIN,
        unsigned8: u8::MAX,
        unsigned16: u16::MAX,
        unsigned32: u32::MAX,
        optional_signed8: None,
        optional_signed16: None,
        optional_signed32: None,
        optional_unsigned8: None,
        optional_unsigned16: None,
        optional_unsigned32: None,
    };
    // IcyDB's narrow-integer width/extrema cases, with our schema-selected framing.
    // id, six absent options, signed16/32/8, unsigned16/32/8.
    let mut golden = vec![1, 0, 0, 0, 0, 0, 0, 0];
    golden.extend([0; 6]);
    golden.extend([0, 128, 0, 0, 0, 128, 128]);
    golden.extend([255; 7]);
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(golden.len(), 28);
    assert_eq!(NarrowRow::decode(&golden).unwrap(), row);
    let contract = || Schema::new(&[NarrowRow::SCHEMA]).unwrap();
    // Independent Node byte framing + SHA-256 contract vector.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "906a491993e3a46d027520e23c71cd249d57db45cafabd84fed5123c5d747d66"
    );
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [9; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    let mut last = row;
    for (s8, s16, s32, u8, u16, u32) in [
        (i8::MIN, i16::MIN, i32::MIN, 0, 0, 0),
        (-1, -1, -1, u8::MAX, u16::MAX, u32::MAX),
        (0, 0, 0, 0, 0, 0),
        (1, 1, 1, 1, 1, 1),
        (i8::MAX, i16::MAX, i32::MAX, 128, 32768, 2147483648),
    ] {
        let value = NarrowRow {
            id: 1,
            signed8: s8,
            signed16: s16,
            signed32: s32,
            unsigned8: u8,
            unsigned16: u16,
            unsigned32: u32,
            optional_signed8: Some(s8),
            optional_signed16: Some(s16),
            optional_signed32: Some(s32),
            optional_unsigned8: Some(u8),
            optional_unsigned16: Some(u16),
            optional_unsigned32: Some(u32),
        };
        // Standard-library byte conversion is the independent oracle; no product
        // encoder is used to produce these expected field payloads.
        let payloads = [
            s16.to_le_bytes().to_vec(),
            s32.to_le_bytes().to_vec(),
            s8.to_le_bytes().to_vec(),
            u16.to_le_bytes().to_vec(),
            u32.to_le_bytes().to_vec(),
            u8.to_le_bytes().to_vec(),
        ];
        let mut expected = vec![1, 0, 0, 0, 0, 0, 0, 0];
        for payload in &payloads {
            expected.push(1);
            expected.extend(payload);
        }
        for payload in &payloads {
            expected.extend(payload);
        }
        assert_eq!(expected.len(), 42);
        assert_eq!(value.encode().unwrap(), expected);
        assert_eq!(NarrowRow::decode(&expected).unwrap(), value);
        for end in 0..expected.len() {
            assert!(matches!(
                NarrowRow::decode(&expected[..end]),
                Err(Error::Corrupt)
            ));
        }
        assert!(matches!(
            NarrowRow::decode(&[expected.as_slice(), &[0]].concat()),
            Err(Error::Corrupt)
        ));
        for position in [8, 11, 16, 18, 21, 26] {
            let mut invalid = expected.clone();
            invalid[position] = 2;
            assert!(matches!(NarrowRow::decode(&invalid), Err(Error::Corrupt)));
        }
        let mut batch = Batch::new();
        batch.replace(&value).unwrap();
        db.commit(batch).unwrap();
        assert_eq!(db.get::<NarrowRow>(1).unwrap(), Some(value.clone()));
        last = value;
    }
    assert_eq!(db.revision::<NarrowRow>().unwrap(), 6);
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.replace(&last).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(*backing.borrow(), before);
    drop(db);
    drop(runtime);
    let runtime = host(backing.clone());
    // Changing width or signedness cannot reinterpret old bytes, even where an
    // individual value would fit both types. Optionality is also contractual.
    for (name, replacement) in [
        ("signed8", ic_db::FieldType::Nat8),
        ("signed16", ic_db::FieldType::Int32),
        ("signed32", ic_db::FieldType::Int64),
        ("unsigned8", ic_db::FieldType::Nat16),
        ("unsigned16", ic_db::FieldType::Nat32),
        ("unsigned32", ic_db::FieldType::Int32),
    ] {
        let mut fields = NarrowRow::SCHEMA.fields.to_vec();
        fields
            .iter_mut()
            .find(|field| field.name == name)
            .unwrap()
            .value = replacement;
        let changed = Box::leak(Box::new(ic_db::EntitySchema {
            fields: Box::leak(fields.into_boxed_slice()),
            ..*NarrowRow::SCHEMA
        }));
        assert!(matches!(
            Database::open(
                runtime.open_memory(&key).unwrap(),
                Schema::new(&[changed]).unwrap()
            ),
            Err(Error::IncompatibleSchema)
        ));
        assert_eq!(*backing.borrow(), before);
    }
    let db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(db.get::<NarrowRow>(1).unwrap(), Some(last));
    assert_eq!(db.revision::<NarrowRow>().unwrap(), 6);
}

#[test]
fn binary_values_keep_framing_bounds_and_absence_without_text_conversion() {
    let row = BinaryRow {
        id: 1,
        payload: vec![0, 255, 128],
        optional: None,
    };
    // Independent length-framed bytes: id, absent option, payload length, raw bytes.
    let golden = [1, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 255, 128];
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(BinaryRow::decode(&golden).unwrap(), row);
    let present = BinaryRow {
        optional: Some(Blob::new()),
        ..row.clone()
    };
    assert_eq!(
        present.encode().unwrap(),
        [
            1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 3, 0, 0, 0, 0, 255, 128
        ]
    );
    assert_ne!(present, row);
    let mut bad_option = golden.to_vec();
    bad_option[8] = 2;
    for malformed in [
        bad_option,
        golden[..15].to_vec(),
        [golden.as_slice(), &[0]].concat(),
    ] {
        assert!(matches!(BinaryRow::decode(&malformed), Err(Error::Corrupt)));
    }
    // Binary fields are bulk bytes, with field and whole-row bounds.
    let full = BinaryRow {
        id: 1,
        payload: vec![255; 1024],
        optional: Some(vec![128; 32]),
    };
    assert_eq!(BinaryRow::decode(&full.encode().unwrap()).unwrap(), full);
    for (payload, optional, independently_encoded) in [
        (
            vec![255; 1025],
            None,
            [vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 4, 0, 0], vec![255; 1025]].concat(),
        ),
        (
            Blob::new(),
            Some(vec![0; 33]),
            [
                vec![1, 0, 0, 0, 0, 0, 0, 0, 1, 33, 0, 0, 0],
                vec![0; 33],
                vec![0; 4],
            ]
            .concat(),
        ),
    ] {
        let invalid = BinaryRow {
            id: 1,
            payload,
            optional,
        };
        assert!(matches!(invalid.encode(), Err(Error::Limit)));
        assert!(matches!(
            BinaryRow::decode(&independently_encoded),
            Err(Error::Corrupt)
        ));
    }
    let empty = EmptyBlob {
        id: 1,
        payload: Blob::new(),
    };
    assert!(Schema::new(&[EmptyBlob::SCHEMA]).is_ok());
    // IcyDB's accepted-length regression also covers an empty-only Text contract.
    let empty_text = EmptyText {
        id: 1,
        payload: String::new(),
    };
    assert!(Schema::new(&[EmptyText::SCHEMA]).is_ok());
    assert_eq!(
        EmptyText::decode(&empty_text.encode().unwrap()).unwrap(),
        empty_text
    );
    assert!(matches!(
        EmptyText {
            id: 1,
            payload: "x".into()
        }
        .encode(),
        Err(Error::Limit)
    ));
    assert_eq!(EmptyBlob::decode(&empty.encode().unwrap()).unwrap(), empty);
    assert!(matches!(
        EmptyBlob {
            id: 1,
            payload: vec![0]
        }
        .encode(),
        Err(Error::Limit)
    ));
    let wide = WideBlob {
        id: 1,
        payload: vec![0; 4096],
    };
    assert!(wide.validate().is_ok());
    assert!(matches!(wide.encode(), Err(Error::Limit))); // framing exceeds whole-row envelope

    let contract = || Schema::new(&[BinaryRow::SCHEMA]).unwrap();
    // Independent Node byte framing + SHA-256 schema vector.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "e0f4be4328cfae8a0ccc16fcdc0e51554ce2f0b40cd30693941b3898ce90be3d"
    );
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [7; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    let mut batch = Batch::new();
    batch.replace(&full).unwrap();
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.replace(&full).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(*backing.borrow(), before);
    drop(db);
    drop(runtime);
    let runtime = host(backing.clone());
    for replacement in [
        ic_db::FieldType::Blob { max_bytes: 1023 },
        ic_db::FieldType::Text { max_bytes: 1024 },
    ] {
        let mut fields = BinaryRow::SCHEMA.fields.to_vec();
        fields[2].value = replacement;
        let altered = Box::leak(Box::new(ic_db::EntitySchema {
            fields: Box::leak(fields.into_boxed_slice()),
            ..*BinaryRow::SCHEMA
        }));
        assert!(matches!(
            Database::open(
                runtime.open_memory(&key).unwrap(),
                Schema::new(&[altered]).unwrap()
            ),
            Err(Error::IncompatibleSchema)
        ));
        assert_eq!(*backing.borrow(), before);
    }
    let db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(db.get::<BinaryRow>(1).unwrap(), Some(full));
    assert_eq!(db.revision::<BinaryRow>().unwrap(), 2);
}

///
/// PrincipalRow
/// Stores `owner` and `optional` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "owner", value(item(prim = "Principal"))),
        field(name = "optional", value(opt, item(prim = "Principal")))
    )
)]
struct PrincipalRow {}

#[test]
fn principals_preserve_raw_identity_absence_and_reject_corrupt_storage() {
    use ic_db::Principal;
    let row = PrincipalRow {
        id: 1,
        owner: Principal::anonymous(),
        optional: None,
    };
    // Literal row: id, absent option, u32 length, anonymous byte. Not text/Serde.
    let golden = [1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 4];
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(PrincipalRow::decode(&golden).unwrap(), row);
    let contract = || Schema::new(&[PrincipalRow::SCHEMA]).unwrap();
    // Independent Node byte framing and SHA-256 contract vector.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "581058955e0ecb8d7af7a60486c6815dafd161387504754299bdaa5940ea2770"
    );

    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [7; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    // IcyDB's canonical-identity cases: management, anonymous, short opaque and
    // maximum-length bytes. Zero bytes are significant, never trailing padding.
    for bytes in [
        vec![],
        vec![4],
        vec![2, 7],
        vec![0],
        vec![0, 0],
        vec![0xAB; 29],
    ] {
        let value = PrincipalRow {
            owner: Principal::try_from_slice(&bytes).unwrap(),
            optional: Some(Principal::management_canister()),
            ..row
        };
        assert_eq!(value.owner.as_slice(), bytes);
        let mut expected = vec![1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]; // Some(empty)
        expected.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        expected.extend_from_slice(&bytes);
        assert_eq!(value.encode().unwrap(), expected);
        assert_eq!(PrincipalRow::decode(&expected).unwrap(), value);
        let mut batch = Batch::new();
        batch.replace(&value).unwrap();
        db.commit(batch).unwrap();
        assert_eq!(db.get::<PrincipalRow>(1).unwrap(), Some(value.clone()));
        let before = backing.borrow().clone();
        let mut batch = Batch::new();
        batch.replace(&value).unwrap();
        db.commit(batch).unwrap();
        assert_eq!(*backing.borrow(), before);
    }
    assert!(Principal::try_from_slice(&[0; 30]).is_err());
    let final_row = db.get::<PrincipalRow>(1).unwrap().unwrap();
    let before = backing.borrow().clone();
    drop(db);
    drop(runtime);
    let runtime = host(backing.clone());
    let db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(db.get::<PrincipalRow>(1).unwrap(), Some(final_row));
    drop(db);
    // Same length framing does not make a Blob the same admitted type.
    let mut fields = PrincipalRow::SCHEMA.fields.to_vec();
    fields[2].value = ic_db::FieldType::Blob { max_bytes: 29 };
    let altered = Box::leak(Box::new(ic_db::EntitySchema {
        fields: Box::leak(fields.into_boxed_slice()),
        ..*PrincipalRow::SCHEMA
    }));
    assert!(matches!(
        Database::open(
            runtime.open_memory(&key).unwrap(),
            Schema::new(&[altered]).unwrap()
        ),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);

    let mut invalid = Vec::new();
    for end in 0..golden.len() {
        invalid.push(golden[..end].to_vec());
    }
    let mut bad_tag = golden.to_vec();
    bad_tag[8] = 2;
    invalid.push(bad_tag);
    let mut trailing = golden.to_vec();
    trailing.push(0);
    invalid.push(trailing);
    for length in [30_u32, u32::MAX] {
        let mut bad = golden[..9].to_vec();
        bad.extend_from_slice(&length.to_le_bytes());
        bad.extend_from_slice(&[0; 30]);
        invalid.push(bad);
    }
    let mut optional_too_long = golden[..8].to_vec();
    optional_too_long.extend_from_slice(&[1, 30, 0, 0, 0]);
    optional_too_long.extend_from_slice(&[0; 30]);
    optional_too_long.extend_from_slice(&[0, 0, 0, 0]);
    invalid.push(optional_too_long);
    let mut physical_key = vec![2];
    physical_key.extend_from_slice(&334826855_u32.to_be_bytes());
    physical_key.extend_from_slice(&1_u64.to_be_bytes());
    for bytes in invalid {
        assert!(matches!(PrincipalRow::decode(&bytes), Err(Error::Corrupt)));
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
        raw.insert(physical_key.clone(), bytes);
        drop(raw);
        let before = backing.borrow().clone();
        let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
        assert!(matches!(db.get::<PrincipalRow>(1), Err(Error::Corrupt)));
        // Shared old-row inspection must validate even an unselected identity.
        let mut batch = Batch::new();
        batch.replace(&row).unwrap();
        assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
        let mut batch = Batch::new();
        batch.delete::<PrincipalRow>(1).unwrap();
        assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
        assert_eq!(*backing.borrow(), before);
    }
}

// Numeric ordering is checked against Rust's Ord, not another key encoder.
macro_rules! integer_index_case {
    ($test:ident, $prim:literal, $ty:ty, $values:expr) => {
        #[test]
        fn $test() {
            ///
            /// Indexed
            /// Exercises compound index ordering for the selected numeric type.
            ///

            #[entity(store = ic_db::testing::FixtureStore, pk(field = "id"), index(fields = ["bucket", "value"]), fields(field(name = "id", value(item(prim = "Nat64"))), field(name = "bucket", value(item(prim = "Nat64"))), field(name = "value", value(item(prim = $prim)))))]
            struct Indexed {}
            let contract = || Schema::new(&[Indexed::SCHEMA]).unwrap();
            let backing = VectorMemory::default();
            let runtime = host(backing.clone());
            let key = StableKey::parse("test.db.v1").unwrap();
            let mut db =
                Database::create(runtime.open_memory(&key).unwrap(), contract(), [1; 16]).unwrap();
            let values: [$ty; 5] = $values;
            let mut expected = Vec::new();
            let mut batch = Batch::new();
            for (i, value) in values.into_iter().rev().enumerate() {
                // Equal secondary values must use primary identity as the tie-breaker.
                for id in [i as u64 + 1, i as u64 + 10] {
                    let row = Indexed {
                        id,
                        bucket: 7,
                        value,
                    };
                    batch.insert(&row).unwrap();
                    expected.push(row);
                }
            }
            db.commit(batch).unwrap();
            expected.sort_by_key(|row| (row.value, row.id));
            assert_eq!(
                db.index_prefix::<Indexed>("bucket,value", &[7_u64.into()], 32)
                    .unwrap(),
                expected
            );
            for row in &expected {
                let matches = db
                    .index_prefix::<Indexed>("bucket,value", &[7_u64.into(), row.value.into()], 32)
                    .unwrap();
                assert_eq!(matches.len(), 2);
                assert!(matches.iter().all(|found| found.value == row.value));
            }
            assert!(matches!(
                db.index_prefix::<Indexed>("bucket,value", &[7_i64.into()], 1),
                Err(Error::InvalidOperand)
            ));
            assert!(matches!(
                db.index_prefix::<Indexed>(
                    "bucket,value",
                    &[7_u64.into(), ic_db::Principal::anonymous().into()],
                    1
                ),
                Err(Error::InvalidOperand)
            ));
            let mut changed = expected[0].clone();
            changed.value = values[4];
            let mut batch = Batch::new();
            batch.replace(&changed).unwrap();
            batch.delete::<Indexed>(expected[1].id).unwrap();
            db.commit(batch).unwrap();
            let deleted = expected[1].id;
            expected.retain(|row| row.id != changed.id && row.id != deleted);
            expected.push(changed);
            expected.sort_by_key(|row| (row.value, row.id));
            drop(db);
            drop(runtime);
            let runtime = host(backing);
            let db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
            assert_eq!(
                db.index_prefix::<Indexed>("bucket,value", &[7_u64.into()], 32)
                    .unwrap(),
                expected
            );
            assert_eq!(db.revision::<Indexed>().unwrap(), 2);
        }
    };
}
integer_index_case!(index_nat8, "Nat8", u8, [0, 1, 127, 128, u8::MAX]);
integer_index_case!(index_nat16, "Nat16", u16, [0, 1, 32767, 32768, u16::MAX]);
integer_index_case!(
    index_nat32,
    "Nat32",
    u32,
    [0, 1, 2147483647, 2147483648, u32::MAX]
);
integer_index_case!(
    index_nat64,
    "Nat64",
    u64,
    [0, 1, u64::MAX / 2, u64::MAX / 2 + 1, u64::MAX]
);
integer_index_case!(
    index_nat128,
    "Nat128",
    u128,
    [0, 1, u128::MAX / 2, u128::MAX / 2 + 1, u128::MAX]
);
integer_index_case!(index_int8, "Int8", i8, [i8::MIN, -1, 0, 1, i8::MAX]);
integer_index_case!(index_int16, "Int16", i16, [i16::MIN, -1, 0, 1, i16::MAX]);
integer_index_case!(index_int32, "Int32", i32, [i32::MIN, -1, 0, 1, i32::MAX]);
integer_index_case!(index_int64, "Int64", i64, [i64::MIN, -1, 0, 1, i64::MAX]);
integer_index_case!(
    index_int128,
    "Int128",
    i128,
    [i128::MIN, -1, 0, 1, i128::MAX]
);

///
/// IndexedIdentity
/// Exercises references to `Entry` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(field = "owner", unique),
    index(fields = ["parent", "owner", "rank"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "parent", value(item(prim = "Nat64")), references(entity = "Entry", index = "parent,owner,rank")),
        field(name = "owner", value(item(prim = "Principal"))),
        field(name = "rank", value(item(prim = "Int8")))
    )
)]
struct IndexedIdentity {}

#[test]
fn principal_indexes_preserve_order_compound_boundaries_and_final_state_uniqueness() {
    use ic_db::{IndexValue, Principal};
    let contract = || Schema::new(&[Entry::SCHEMA, IndexedIdentity::SCHEMA]).unwrap();
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [1; 16]).unwrap();
    let mut batch = Batch::new();
    batch
        .insert(&Entry {
            id: 1,
            category: 0,
            amount: 0,
        })
        .unwrap();
    let mut expected = Vec::new();
    for (i, bytes) in [
        vec![255],
        vec![],
        vec![4],
        vec![0, 0],
        vec![0],
        vec![1],
        vec![1, 0],
        vec![255; 29],
        vec![0; 29],
    ]
    .into_iter()
    .enumerate()
    {
        let row = IndexedIdentity {
            id: i as u64 + 1,
            parent: 1,
            owner: Principal::try_from_slice(&bytes).unwrap(),
            rank: -1,
        };
        batch.insert(&row).unwrap();
        expected.push(row);
    }
    db.commit(batch).unwrap();
    expected.sort_by_key(|row| row.owner);
    assert_eq!(
        db.index_prefix::<IndexedIdentity>("parent,owner,rank", &[1_u64.into()], 32)
            .unwrap(),
        expected
    );
    for row in &expected {
        assert_eq!(
            db.index_prefix::<IndexedIdentity>("owner", &[row.owner.into()], 32)
                .unwrap(),
            std::slice::from_ref(row)
        );
        assert_eq!(
            db.index_prefix::<IndexedIdentity>(
                "parent,owner,rank",
                &[1_u64.into(), row.owner.into(), (-1_i8).into()],
                32
            )
            .unwrap(),
            std::slice::from_ref(row)
        );
    }
    for operands in [
        vec![],
        vec![1_u64.into()],
        vec![Principal::anonymous().into(), 1_u64.into()],
    ] {
        assert!(
            db.index_prefix::<IndexedIdentity>("owner", &operands, 1)
                .is_err()
        );
    }
    assert!(matches!(
        db.index_prefix::<IndexedIdentity>(
            "parent,owner,rank",
            &[
                1_u64.into(),
                Principal::anonymous().into(),
                IndexValue::Int64(-1)
            ],
            1
        ),
        Err(Error::InvalidOperand)
    ));
    let before = backing.borrow().clone();
    for collide in [false, true] {
        let mut batch = Batch::new();
        let owner = if collide {
            Principal::try_from_slice(&[9]).unwrap()
        } else {
            expected[0].owner
        };
        batch
            .insert(&IndexedIdentity {
                id: 20,
                parent: 1,
                owner,
                rank: 0,
            })
            .unwrap();
        if collide {
            batch
                .insert(&IndexedIdentity {
                    id: 21,
                    parent: 1,
                    owner,
                    rank: 1,
                })
                .unwrap();
        }
        assert!(matches!(db.commit(batch), Err(Error::UniqueViolation)));
        assert_eq!(*backing.borrow(), before);
    }
    let mut batch = Batch::new();
    batch.delete::<Entry>(1).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    // Exchange unique values; final-state admission must release both old keys first.
    let mut left = expected[0].clone();
    let mut right = expected[1].clone();
    std::mem::swap(&mut left.owner, &mut right.owner);
    let mut batch = Batch::new();
    batch.replace(&left).unwrap();
    batch.replace(&right).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(
        db.index_prefix::<IndexedIdentity>("owner", &[left.owner.into()], 1)
            .unwrap(),
        [left.clone()]
    );
    let mut batch = Batch::new();
    batch.delete::<IndexedIdentity>(left.id).unwrap();
    let replacement = IndexedIdentity {
        id: 30,
        rank: i8::MIN,
        ..left
    };
    batch.insert(&replacement).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(
        db.index_prefix::<IndexedIdentity>("owner", &[replacement.owner.into()], 1)
            .unwrap(),
        std::slice::from_ref(&replacement)
    );
    drop(db);
    // Literal persisted key: entity31, index2, parent1, Principal[0], Int8::MIN, id30.
    let physical = vec![
        3, 66, 196, 205, 224, 22, 173, 240, 168, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 30,
    ];
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    assert_eq!(raw.get(&physical), Some(30_u64.to_be_bytes().to_vec()));
    raw.insert(physical.clone(), right.id.to_be_bytes().to_vec());
    drop(raw);
    let before = backing.borrow().clone();
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert!(matches!(
        db.index_prefix::<IndexedIdentity>("parent,owner,rank", &[1_u64.into()], 32),
        Err(Error::Corrupt)
    ));
    let mut batch = Batch::new();
    batch.delete::<IndexedIdentity>(30).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    raw.insert(physical, 30_u64.to_be_bytes().to_vec());
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let mut batch = Batch::new();
    for row in db
        .index_prefix::<IndexedIdentity>("parent,owner,rank", &[1_u64.into()], 32)
        .unwrap()
    {
        batch.delete::<IndexedIdentity>(row.id).unwrap();
    }
    batch.delete::<Entry>(1).unwrap();
    db.commit(batch).unwrap();
    assert!(
        db.index_prefix::<IndexedIdentity>("parent,owner,rank", &[1_u64.into()], 32)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn principal_component_order_matches_the_identity_owner_at_every_length() {
    let mut principals = vec![ic_db::Principal::management_canister()];
    for len in 1..=29 {
        for byte in [0, 1, 4, 255] {
            principals.push(ic_db::Principal::try_from_slice(&vec![byte; len]).unwrap());
        }
    }
    let encoded: Vec<_> = principals
        .iter()
        .map(|&owner| {
            IndexedIdentity {
                id: 1,
                parent: 1,
                owner,
                rank: 0,
            }
            .__row_info()
            .indexes
            .remove(0)
        })
        .collect();
    // IcyDB's length-before-contents regression: byte-prefix principals and
    // significant zero bytes must agree with the canonical identity owner.
    for (i, left) in principals.iter().enumerate() {
        for (j, right) in principals.iter().enumerate() {
            assert_eq!(encoded[i].cmp(&encoded[j]), left.cmp(right));
        }
    }
}

#[test]
fn runtime_schema_admission_rejects_unsupported_index_shapes() {
    for (value, optional) in [
        (ic_db::FieldType::Bool, true),
        (ic_db::FieldType::Text { max_bytes: 32 }, true),
        (ic_db::FieldType::Blob { max_bytes: 32 }, true),
        (
            ic_db::FieldType::List {
                item: &ic_db::FieldType::Nat8,
                max_items: 32,
            },
            false,
        ),
        (ic_db::FieldType::Principal, true),
    ] {
        let mut fields = IndexedIdentity::SCHEMA.fields.to_vec();
        let owner = fields
            .iter_mut()
            .find(|field| field.name == "owner")
            .unwrap();
        owner.value = value;
        owner.optional = optional;
        let descriptor = Box::leak(Box::new(ic_db::EntitySchema {
            fields: Box::leak(fields.into_boxed_slice()),
            ..*IndexedIdentity::SCHEMA
        }));
        assert!(matches!(
            Schema::new(&[Entry::SCHEMA, descriptor]),
            Err(Error::InvalidSchema)
        ));
    }
}

///
/// TextIndex
/// Exercises indexed access by `bucket`, `first` and `second` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["bucket", "first", "second"]),
    index(fields = ["first", "second"], unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "bucket", value(item(prim = "Bool"))),
        field(name = "first", value(item(prim = "Text", max_bytes = 128))),
        field(name = "second", value(item(prim = "Text", max_bytes = 32)))
    )
)]
struct TextIndex {}

#[test]
fn text_indexes_preserve_exact_order_and_component_boundaries() {
    let contract = || Schema::new(&[TextIndex::SCHEMA]).unwrap();
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [1; 16]).unwrap();
    let tuples = [
        ("a", "\0b"),
        ("a\0", "b"),
        ("a", "b"),
        ("aa", ""),
        ("", ""),
        ("\0", ""),
        ("é", ""),
        ("e\u{301}", ""),
        ("A", ""),
        ("a\0b", ""),
        ("a", ""),
        ("😀", ""),
        ("a", "\0"),
    ];
    let mut expected = Vec::new();
    let mut batch = Batch::new();
    for (i, (first, second)) in tuples.into_iter().enumerate() {
        let row = TextIndex {
            id: i as u64 + 1,
            bucket: false,
            first: first.into(),
            second: second.into(),
        };
        batch.insert(&row).unwrap();
        expected.push(row);
    }
    // Boolean is a separate exact domain, with an independent prefix.
    let other = TextIndex {
        id: 20,
        bucket: true,
        first: "other".into(),
        second: String::new(),
    };
    batch.insert(&other).unwrap();
    db.commit(batch).unwrap();
    expected.sort_by(|a, b| (&a.first, &a.second, a.id).cmp(&(&b.first, &b.second, b.id)));
    assert_eq!(
        db.index_prefix::<TextIndex>("bucket,first,second", &[false.into()], 32)
            .unwrap(),
        expected
    );
    assert_eq!(
        db.index_prefix::<TextIndex>("bucket,first,second", &[true.into()], 32)
            .unwrap(),
        [other]
    );
    for row in &expected {
        assert_eq!(
            db.query::<TextIndex>()
                .eq(TextIndex::FIRST, &row.first)
                .eq(TextIndex::SECOND, &row.second)
                .first()
                .unwrap()
                .as_ref(),
            Some(row)
        );
        let exact = db
            .index_prefix::<TextIndex>(
                "first,second",
                &[row.first.as_str().into(), row.second.as_str().into()],
                32,
            )
            .unwrap();
        assert_eq!(exact, std::slice::from_ref(row));
    }
    // The candidate is outside a one-row primary scan. Reverse predicate order
    // relative to the declaration to exercise lookup by field identity.
    let lookup = || {
        db.query::<TextIndex>()
            .eq("second", "")
            .eq("first", "😀")
            .work_limit(1)
    };
    // Emitted by the v0.23.11 primary traversal after examining id 1.
    let old_cursor = ic_db::QueryCursor::from_bytes(&[
        1, 0, 172, 95, 112, 201, 43, 229, 236, 254, 194, 250, 11, 21, 116, 140, 213, 24, 7, 72,
        221, 43, 145, 87, 213, 51, 87, 252, 163, 112, 75, 148, 170, 240, 2, 197, 121, 161, 226, 0,
        0, 0, 0, 0, 0, 0, 1,
    ])
    .unwrap();
    let resumed = lookup().after(&old_cursor).work_limit(32).fetch().unwrap();
    assert_eq!(resumed.rows.len(), 1);
    assert_eq!(resumed.rows[0].first, "😀");
    assert!(resumed.next.is_none());
    let page = lookup().limit(1).fetch().unwrap();
    assert_eq!((page.examined, page.fetched), (1, 1));
    assert_eq!(page.rows[0].first, "😀");
    assert!(page.next.is_none());
    assert_eq!(lookup().first().unwrap().unwrap().first, "😀");
    let ordered = lookup().order_by(&["bucket"]).descending().fetch().unwrap();
    assert_eq!((ordered.examined, ordered.fetched), (1, 1));
    assert_eq!(ordered.rows, page.rows);
    let explicit = db
        .query::<TextIndex>()
        .using_index("first,second", &["😀".into(), "".into()])
        .order_by(&["bucket"])
        .work_limit(1)
        .fetch()
        .unwrap();
    assert_eq!(explicit.rows, page.rows);
    assert!(explicit.next.is_none());
    assert!(ordered.next.is_none());
    assert!(matches!(
        lookup().order_by(&["missing"]).first(),
        Err(Error::InvalidQuery)
    ));
    assert!(lookup().eq("bucket", true).first().unwrap().is_none());
    assert!(matches!(
        db.query::<TextIndex>()
            .eq("first", "😀")
            .work_limit(1)
            .first(),
        Err(Error::Limit)
    ));
    let alternatives = db
        .query::<TextIndex>()
        .eq("second", "")
        .filter_any(&[
            ("first", ic_db::Comparison::Eq, "😀".into()),
            ("first", ic_db::Comparison::Eq, "aa".into()),
        ])
        .fetch()
        .unwrap();
    assert_eq!(alternatives.rows.len(), 2);
    let residual = lookup().eq("bucket", true).fetch().unwrap();
    assert!(residual.rows.is_empty() && residual.next.is_none());
    assert_eq!(residual.examined, 1);
    let miss = db
        .query::<TextIndex>()
        .eq("first", "missing")
        .eq("second", "")
        .work_limit(1)
        .fetch()
        .unwrap();
    assert!(miss.rows.is_empty() && miss.next.is_none());
    assert_eq!(miss.examined, 0);

    let starts_with_a: Vec<_> = expected
        .iter()
        .filter(|row| row.first == "a")
        .cloned()
        .collect();
    assert_eq!(
        db.index_prefix::<TextIndex>("bucket,first,second", &[false.into(), "a".into()], 32)
            .unwrap(),
        starts_with_a
    );
    // Prefix means whole leading fields, not a starts-with predicate on text.
    assert!(
        db.index_prefix::<TextIndex>("first,second", &["e".into()], 32)
            .unwrap()
            .is_empty()
    );
    let before = backing.borrow().clone();
    assert!(matches!(
        db.index_prefix::<TextIndex>("bucket,first,second", &[0_u8.into()], 32),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.index_prefix::<TextIndex>("first,second", &[false.into()], 32),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.index_prefix::<TextIndex>("first,second", &["é".repeat(65).as_str().into()], 32),
        Err(Error::Limit)
    ));
    let mut batch = Batch::new();
    batch
        .insert(&TextIndex {
            id: 99,
            bucket: true,
            ..expected[0].clone()
        })
        .unwrap();
    assert!(matches!(db.commit(batch), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    // An unchanged replacement must not advance revisions or touch backing.
    let mut batch = Batch::new();
    batch.replace(&expected[0]).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(*backing.borrow(), before);
    // Simultaneously exchange complete unique tuples.
    let left = TextIndex {
        id: 1,
        bucket: true,
        first: "a\0".into(),
        second: "b".into(),
    };
    let right = TextIndex {
        id: 2,
        bucket: false,
        first: "a".into(),
        second: "\0b".into(),
    };
    let mut batch = Batch::new();
    batch.replace(&left).unwrap();
    batch.replace(&right).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(
        db.index_prefix::<TextIndex>("first,second", &["a\0".into(), "b".into()], 32)
            .unwrap(),
        std::slice::from_ref(&left)
    );
    assert_eq!(
        db.index_prefix::<TextIndex>("first,second", &["a".into(), "\0b".into()], 32)
            .unwrap(),
        std::slice::from_ref(&right)
    );
    let mut batch = Batch::new();
    batch.delete::<TextIndex>(1).unwrap();
    db.commit(batch).unwrap();
    assert!(
        db.index_prefix::<TextIndex>("first,second", &["a\0".into(), "b".into()], 32)
            .unwrap()
            .is_empty()
    );
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    // Independent literal unique key: entity32/index2, "a", "NULb".
    let physical = vec![
        3, 197, 121, 161, 226, 150, 181, 59, 31, 97, 0, 0, 0, 255, 98, 0, 0,
    ];
    assert_eq!(raw.get(&physical), Some(2_u64.to_be_bytes().to_vec()));
    raw.insert(physical, 3_u64.to_be_bytes().to_vec());
    drop(raw);
    let before = backing.borrow().clone();
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert!(matches!(
        db.index_prefix::<TextIndex>("first,second", &["a".into(), "\0b".into()], 32),
        Err(Error::Corrupt)
    ));
    let mut batch = Batch::new();
    batch.delete::<TextIndex>(2).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}

///
/// LargeTextIndex
/// Exercises indexed access by `text` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(field = "text", unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "text", value(item(prim = "Text", max_bytes = 4096)))
    )
)]
struct LargeTextIndex {}

#[test]
fn indexed_text_obeys_complete_key_bound_while_retaining_row_bounds() {
    let contract = || Schema::new(&[LargeTextIndex::SCHEMA]).unwrap();
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [1; 16]).unwrap();
    // Unique key: 9 namespace bytes + 501 escaped text bytes + 2 terminator bytes.
    let row = LargeTextIndex {
        id: 1,
        text: format!("{}a", "\0".repeat(250)),
    };
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(
        db.index_prefix::<LargeTextIndex>("text", &[row.text.as_str().into()], 1)
            .unwrap(),
        std::slice::from_ref(&row)
    );
    let before = backing.borrow().clone();
    let oversized_key = format!("{}ab", "\0".repeat(250));
    let mut batch = Batch::new();
    batch
        .replace(&LargeTextIndex {
            text: oversized_key.clone(),
            ..row.clone()
        })
        .unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Limit)));
    assert!(matches!(
        db.index_prefix::<LargeTextIndex>("text", &[oversized_key.as_str().into()], 1),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    assert!(matches!(
        batch.replace(&LargeTextIndex {
            id: 1,
            text: "\0".repeat(4085)
        }),
        Err(Error::Limit)
    ));
    assert!(matches!(
        db.index_prefix::<LargeTextIndex>("text", &["\0".repeat(4097).as_str().into()], 1),
        Err(Error::Limit)
    ));
    assert_eq!(*backing.borrow(), before);
    drop(db);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(
        db.index_prefix::<LargeTextIndex>("text", &[row.text.as_str().into()], 1)
            .unwrap(),
        [row]
    );
    let mut batch = Batch::new();
    batch.delete::<LargeTextIndex>(1).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.revision::<LargeTextIndex>().unwrap(), 2);
}

///
/// OptionalIndex
/// Exercises references to `Entry` on rows keyed by `id`.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    index(fields = ["parent", "alias", "code"]),
    index(fields = ["alias", "code"], unique, present_only),
    index(fields = ["active", "principal"]),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "parent", value(item(prim = "Nat64")), references(entity = "Entry", index = "parent,alias,code")),
        field(name = "alias", value(opt, item(prim = "Text", max_bytes = 32))),
        field(name = "code", value(opt, item(prim = "Int64"))),
        field(name = "active", value(opt, item(prim = "Bool"))),
        field(name = "principal", value(opt, item(prim = "Principal")))
    )
)]
struct OptionalIndex {}

#[test]
fn optional_indexes_preserve_absence_and_complete_unique_membership() {
    use ic_db::{IndexValue, Principal};
    let contract = || Schema::new(&[Entry::SCHEMA, OptionalIndex::SCHEMA]).unwrap();
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [1; 16]).unwrap();
    let mut batch = Batch::new();
    batch
        .insert(&Entry {
            id: 1,
            category: 0,
            amount: 0,
        })
        .unwrap();
    let inputs = [
        (None, None, None, None),
        (
            None,
            None,
            Some(false),
            Some(Principal::management_canister()),
        ),
        (Some(""), None, Some(true), Some(Principal::anonymous())),
        (None, Some(0), None, Some(Principal::management_canister())),
        (Some(""), Some(0), Some(false), None),
        (Some("a"), Some(-1), None, None),
        (None, Some(0), None, None),
    ];
    let mut expected = Vec::new();
    for (i, (alias, code, active, principal)) in inputs.into_iter().enumerate() {
        let row = OptionalIndex {
            id: i as u64 + 1,
            parent: 1,
            alias: alias.map(str::to_owned),
            code,
            active,
            principal,
        };
        batch.insert(&row).unwrap();
        expected.push(row);
    }
    db.commit(batch).unwrap();
    let absent = db
        .query::<OptionalIndex>()
        .eq("alias", IndexValue::Absent)
        .eq("code", 0_i64)
        .fetch()
        .unwrap();
    assert_eq!(
        absent.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        [4, 7]
    );
    expected.sort_by(|a, b| (&a.alias, a.code, a.id).cmp(&(&b.alias, b.code, b.id)));
    assert_eq!(
        db.index_prefix::<OptionalIndex>("parent,alias,code", &[1_u64.into()], 32)
            .unwrap(),
        expected
    );
    assert_eq!(
        db.index_prefix::<OptionalIndex>(
            "parent,alias,code",
            &[1_u64.into(), IndexValue::Absent],
            32
        )
        .unwrap()
        .iter()
        .map(|r| r.id)
        .collect::<Vec<_>>(),
        [1, 2, 4, 7]
    );
    assert_eq!(
        db.index_prefix::<OptionalIndex>(
            "parent,alias,code",
            &[1_u64.into(), "".into(), IndexValue::Absent],
            32
        )
        .unwrap()[0]
            .id,
        3
    );
    assert_eq!(
        db.index_prefix::<OptionalIndex>("alias,code", &["".into(), 0_i64.into()], 32)
            .unwrap()[0]
            .id,
        5
    );
    assert!(matches!(
        db.index_prefix::<OptionalIndex>("alias,code", &[IndexValue::Absent], 32),
        Err(Error::InvalidOperand)
    ));
    assert!(matches!(
        db.index_prefix::<OptionalIndex>("parent,alias,code", &[IndexValue::Absent], 32),
        Err(Error::InvalidOperand)
    ));
    assert_eq!(
        db.index_prefix::<OptionalIndex>(
            "active,principal",
            &[false.into(), IndexValue::Absent],
            32
        )
        .unwrap()[0]
            .id,
        5
    );
    assert_eq!(
        db.index_prefix::<OptionalIndex>(
            "active,principal",
            &[false.into(), Principal::management_canister().into()],
            32
        )
        .unwrap()[0]
            .id,
        2
    );
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch
        .insert(&OptionalIndex {
            id: 8,
            ..db.get::<OptionalIndex>(5).unwrap().unwrap()
        })
        .unwrap();
    assert!(matches!(db.commit(batch), Err(Error::UniqueViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut left = db.get::<OptionalIndex>(5).unwrap().unwrap();
    let mut right = db.get::<OptionalIndex>(6).unwrap().unwrap();
    // A changed owner retaining its unique key still blocks a second claimant
    // in either command order. Its unrelated index change must roll back too.
    left.active = Some(true);
    let claimant = OptionalIndex {
        id: 8,
        ..left.clone()
    };
    for replace_first in [false, true] {
        let mut batch = Batch::new();
        if replace_first {
            batch.replace(&left).unwrap();
            batch.insert(&claimant).unwrap();
        } else {
            batch.insert(&claimant).unwrap();
            batch.replace(&left).unwrap();
        }
        assert!(matches!(db.commit(batch), Err(Error::UniqueViolation)));
        assert_eq!(*backing.borrow(), before);
    }
    std::mem::swap(&mut left.alias, &mut right.alias);
    std::mem::swap(&mut left.code, &mut right.code);
    let mut batch = Batch::new();
    batch.replace(&left).unwrap();
    batch.replace(&right).unwrap();
    db.commit(batch).unwrap();
    let replacement = OptionalIndex {
        id: 8,
        ..left.clone()
    };
    left.code = None;
    let mut batch = Batch::new();
    batch.replace(&left).unwrap();
    batch.insert(&replacement).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(
        db.index_prefix::<OptionalIndex>("alias,code", &["a".into()], 32)
            .unwrap()[0]
            .id,
        8
    );
    drop(db);
    let raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    let unique_prefix = vec![3, 55, 73, 182, 239, 202, 184, 185, 40];
    assert_eq!(
        raw.range(unique_prefix.clone()..)
            .take_while(|entry| entry.key().starts_with(&unique_prefix))
            .count(),
        2
    );
    // Literal optional unique key: present empty text, present signed zero.
    let unique = vec![
        3, 55, 73, 182, 239, 202, 184, 185, 40, 1, 0, 0, 1, 128, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(raw.get(&unique), Some(6_u64.to_be_bytes().to_vec()));
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.delete::<Entry>(1).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    // Omitting a unique entry must not omit the row's required reference.
    let mut batch = Batch::new();
    batch
        .insert(&OptionalIndex {
            id: 99,
            parent: 99,
            alias: None,
            code: None,
            active: None,
            principal: None,
        })
        .unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    for row in db
        .index_prefix::<OptionalIndex>("parent,alias,code", &[1_u64.into()], 32)
        .unwrap()
    {
        batch.delete::<OptionalIndex>(row.id).unwrap();
    }
    batch.delete::<Entry>(1).unwrap();
    db.commit(batch).unwrap();
    assert!(
        db.index_prefix::<OptionalIndex>("parent,alias,code", &[1_u64.into()], 32)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn optional_membership_admission_and_corrupt_presence_fail_closed() {
    let contract = || Schema::new(&[Entry::SCHEMA, OptionalIndex::SCHEMA]).unwrap();
    let altered = |index: &str, unique: bool, present_only: bool| {
        let indexes: Vec<_> = OptionalIndex::SCHEMA
            .indexes
            .iter()
            .map(|value| ic_db::IndexSchema {
                id: value.id,
                name: value.name,
                fields: value.fields,
                unique: if value.name == index {
                    unique
                } else {
                    value.unique
                },
                present_only: if value.name == index {
                    present_only
                } else {
                    value.present_only
                },
            })
            .collect();

        Box::leak(Box::new(ic_db::EntitySchema {
            indexes: Box::leak(indexes.into_boxed_slice()),
            ..*OptionalIndex::SCHEMA
        }))
    };
    assert!(matches!(
        Schema::new(&[Entry::SCHEMA, altered("alias,code", true, false)]),
        Err(Error::InvalidSchema)
    ));
    assert!(matches!(
        Schema::new(&[Entry::SCHEMA, altered("parent,alias,code", true, true)]),
        Err(Error::InvalidSchema)
    ));
    let changed = Schema::new(&[Entry::SCHEMA, altered("alias,code", false, false)]).unwrap();
    assert_ne!(changed.fingerprint(), contract().fingerprint());
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [1; 16]).unwrap();
    let row = OptionalIndex {
        id: 1,
        parent: 1,
        alias: None,
        code: None,
        active: None,
        principal: None,
    };
    let mut batch = Batch::new();
    batch
        .insert(&Entry {
            id: 1,
            category: 0,
            amount: 0,
        })
        .unwrap();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let before = backing.borrow().clone();
    assert!(matches!(
        Database::open(runtime.open_memory(&key).unwrap(), changed),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    let row_key = vec![2, 55, 73, 182, 239, 0, 0, 0, 0, 0, 0, 0, 1];
    let mut bytes = raw.get(&row_key).unwrap();
    bytes[0] = 2; // invalid active presence; this row has no unique-index entry.
    raw.insert(row_key, bytes);
    drop(raw);
    let before = backing.borrow().clone();
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert!(matches!(db.get::<OptionalIndex>(1), Err(Error::Corrupt)));
    let mut batch = Batch::new();
    batch.delete::<OptionalIndex>(1).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
    let mut batch = Batch::new();
    batch
        .replace(&OptionalIndex {
            alias: Some("changed".into()),
            ..row
        })
        .unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn optional_references_distinguish_absence_from_zero_and_validate_final_state() {
    use ic_db::IndexValue;
    let contract = || Schema::new(&[OptionalLink::SCHEMA]).unwrap();
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [9; 16]).unwrap();
    let link = |id, next, alternate| OptionalLink {
        id,
        next,
        alternate,
    };
    let mut batch = Batch::new();
    batch.insert(&link(1, None, None)).unwrap();
    db.commit(batch).unwrap(); // No target, even though key zero does not exist.
    let before = backing.borrow().clone();
    for row in [link(1, Some(0), None), link(1, None, Some(0))] {
        let mut batch = Batch::new();
        batch.replace(&row).unwrap();
        assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
        assert_eq!(*backing.borrow(), before);
    }
    let mut batch = Batch::new();
    batch.replace(&link(1, Some(0), None)).unwrap();
    batch.insert(&link(0, None, None)).unwrap(); // Target can come last.
    db.commit(batch).unwrap();
    assert_eq!(
        db.index_prefix::<OptionalLink>("next,id", &[0_u64.into()], 32)
            .unwrap(),
        vec![link(1, Some(0), None)]
    );
    assert_eq!(
        db.index_prefix::<OptionalLink>("next,id", &[IndexValue::Absent], 32)
            .unwrap(),
        vec![link(0, None, None)]
    );
    let revision = db.revision::<OptionalLink>().unwrap();
    let mut batch = Batch::new();
    batch.replace(&link(1, Some(0), None)).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.revision::<OptionalLink>().unwrap(), revision);
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.delete::<OptionalLink>(0).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    // Unlinking one reference cannot hide a new link to the deleted target.
    let mut batch = Batch::new();
    batch.delete::<OptionalLink>(0).unwrap();
    batch.replace(&link(1, None, Some(0))).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::ReferenceViolation)));
    assert_eq!(*backing.borrow(), before);
    let mut batch = Batch::new();
    batch.delete::<OptionalLink>(0).unwrap();
    batch.replace(&link(1, None, None)).unwrap();
    db.commit(batch).unwrap();
    assert!(
        db.index_prefix::<OptionalLink>("next,id", &[0_u64.into()], 32)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.index_prefix::<OptionalLink>("next,id", &[IndexValue::Absent], 32)
            .unwrap(),
        vec![link(1, None, None)]
    );
    // Repoint a surviving edge while removing its old target; self edges and cycles work.
    let mut batch = Batch::new();
    batch.insert(&link(0, Some(u64::MAX), None)).unwrap();
    batch
        .insert(&link(u64::MAX, Some(0), Some(u64::MAX)))
        .unwrap();
    db.commit(batch).unwrap();
    let mut batch = Batch::new();
    batch.delete::<OptionalLink>(0).unwrap();
    batch
        .replace(&link(u64::MAX, Some(1), Some(u64::MAX)))
        .unwrap();
    db.commit(batch).unwrap();
    let mut batch = Batch::new();
    batch.delete::<OptionalLink>(u64::MAX).unwrap();
    db.commit(batch).unwrap(); // A self-reference disappears with its row.
    // Maximum unlink batch: 31 old edges and their target, with compound reverse keys.
    let mut batch = Batch::new();
    for id in 2..=32 {
        batch.insert(&link(id, Some(1), None)).unwrap();
    }
    db.commit(batch).unwrap();
    let mut batch = Batch::new();
    batch.delete::<OptionalLink>(1).unwrap();
    for id in 2..=32 {
        batch.replace(&link(id, None, None)).unwrap();
    }
    db.commit(batch).unwrap();
    assert!(
        db.index_prefix::<OptionalLink>("next,id", &[1_u64.into()], 32)
            .unwrap()
            .is_empty()
    );
    let expected: Vec<_> = (2..=32).map(|id| link(id, None, None)).collect();
    assert_eq!(
        db.index_prefix::<OptionalLink>("next,id", &[IndexValue::Absent], 32)
            .unwrap(),
        expected
    );
    drop(db);
    let before = backing.borrow().clone();
    let db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(db.page::<OptionalLink>(None, 32).unwrap().rows, expected);
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn optional_reference_keys_and_corruption_are_checked_without_mutation() {
    let contract = || Schema::new(&[OptionalLink::SCHEMA]).unwrap();
    // Independently assembled with Node byte framing and SHA-256.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "f65b2dea1070b84fd62a941de81bd497fd9b266fc4fdaf28c451bdd05c0351b0"
    );
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [9; 16]).unwrap();
    let mut fields = OptionalLink::SCHEMA.fields.to_vec();
    fields[2].optional = false;
    let changed = Box::leak(Box::new(ic_db::EntitySchema {
        fields: Box::leak(fields.into_boxed_slice()),
        ..*OptionalLink::SCHEMA
    }));
    let changed_contract = Schema::new(&[changed]).unwrap();
    assert_ne!(contract().fingerprint(), changed_contract.fingerprint());
    let mut batch = Batch::new();
    batch
        .insert(&OptionalLink {
            id: 0,
            next: None,
            alternate: None,
        })
        .unwrap();
    batch
        .insert(&OptionalLink {
            id: 1,
            next: Some(0),
            alternate: None,
        })
        .unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let before = backing.borrow().clone();
    assert!(matches!(
        Database::open(runtime.open_memory(&key).unwrap(), changed_contract),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    // Literal independent reverse keys: OptionalLink/next,id handles, presence, target,
    // explicit id component, then nonunique primary-key tie-breaker.
    let absent = vec![
        3, 16, 29, 126, 205, 119, 143, 133, 129, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let present = vec![
        3, 16, 29, 126, 205, 119, 143, 133, 129, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
        0, 0, 0, 0, 0, 0, 0, 1,
    ];
    assert_eq!(raw.get(&absent), Some(vec![0; 8]));
    assert_eq!(raw.get(&present), Some(vec![0, 0, 0, 0, 0, 0, 0, 1]));
    let row_key = vec![2, 16, 29, 126, 205, 0, 0, 0, 0, 0, 0, 0, 1];
    let original = raw.get(&row_key).unwrap();
    // Stored order is alternate, id, next. An invalid tag cannot hide an edge.
    let mut corrupt = original.clone();
    corrupt[9] = 2;
    raw.insert(row_key.clone(), corrupt);
    drop(raw);
    let before = backing.borrow().clone();
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert!(matches!(db.get::<OptionalLink>(1), Err(Error::Corrupt)));
    for id in [0, 1] {
        let mut batch = Batch::new();
        batch.delete::<OptionalLink>(id).unwrap();
        assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
        assert_eq!(*backing.borrow(), before);
    }
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    raw.insert(row_key, original);
    raw.insert(present, vec![0; 8]); // Valid owner bytes pointing at an unrelated row.
    drop(raw);
    let before = backing.borrow().clone();
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let mut batch = Batch::new();
    batch.delete::<OptionalLink>(0).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn logical_names_remain_authoritative_when_compact_handles_collide() {
    ///
    /// First
    /// Exercises an alternate `costarring` declaration under the same logical identity.
    ///

    #[entity(
        store = ic_db::testing::FixtureStore,
        name = "costarring",
        pk(field = "id"),
        fields(field(name = "id", value(item(prim = "Nat64"))))
    )]
    struct First {}

    ///
    /// Second
    /// Exercises an alternate `liquid` declaration under the same logical identity.
    ///

    #[entity(
        store = ic_db::testing::FixtureStore,
        name = "liquid",
        pk(field = "id"),
        fields(field(name = "id", value(item(prim = "Nat64"))))
    )]
    struct Second {}

    ///
    /// Renamed
    /// Exercises an alternate `costarring` declaration under the same logical identity.
    ///

    #[entity(
        store = ic_db::testing::FixtureStore,
        name = "costarring",
        pk(field = "id"),
        fields(field(name = "id", value(item(prim = "Nat64"))))
    )]
    struct Renamed {}

    ///
    /// CollidingIndexes
    /// Exercises a row containing only its `id` primary key.
    ///

    #[entity(
        store = ic_db::testing::FixtureStore,
        pk(field = "id"),
        index(name = "costarring", field = "id"),
        index(name = "liquid", field = "id", unique),
        fields(field(name = "id", value(item(prim = "Nat64"))))
    )]
    struct CollidingIndexes {}

    ///
    /// Dependent
    /// Exercises references to `Second` on rows keyed by `id`.
    ///

    #[entity(
        store = ic_db::testing::FixtureStore,
        pk(field = "id"),
        index(field = "target"),
        fields(
            field(name = "id", value(item(prim = "Nat64"))),
            field(
                name = "target",
                value(item(prim = "Nat64")),
                references(entity = "Second")
            )
        )
    )]
    struct Dependent {}

    // Independent known FNV-1a/32 collision; a storage handle is not authority.
    assert_eq!(First::SCHEMA.id, 1_582_148_253);
    assert_eq!(Second::SCHEMA.id, 1_582_148_253);
    for entities in [
        vec![First::SCHEMA, Second::SCHEMA],
        vec![CollidingIndexes::SCHEMA],
        vec![First::SCHEMA, Dependent::SCHEMA],
    ] {
        assert!(matches!(Schema::new(&entities), Err(Error::InvalidSchema)));
    }
    assert!(Schema::new(&[Second::SCHEMA, Dependent::SCHEMA]).is_ok());
    let contract = || Schema::new(&[First::SCHEMA]).unwrap();
    assert_ne!(
        contract().fingerprint(),
        Schema::new(&[Second::SCHEMA]).unwrap().fingerprint()
    );
    assert_eq!(
        contract().fingerprint(),
        Schema::new(&[Renamed::SCHEMA]).unwrap().fingerprint()
    );

    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [8; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&First { id: 7 }).unwrap();
    db.commit(batch).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(db.get::<Second>(7), Err(Error::UnknownEntity)));
    assert!(matches!(
        db.page::<Second>(None, 1),
        Err(Error::UnknownEntity)
    ));
    assert!(matches!(
        db.index_prefix::<Second>("missing", &[7_u64.into()], 1),
        Err(Error::UnknownEntity)
    ));
    assert!(matches!(db.revision::<Second>(), Err(Error::UnknownEntity)));
    let mut batch = Batch::new();
    batch.delete::<Second>(7).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::UnknownEntity)));
    assert_eq!(db.get::<Renamed>(7).unwrap(), Some(Renamed { id: 7 }));
    assert_eq!(
        db.page::<Renamed>(None, 2).unwrap().rows,
        [Renamed { id: 7 }]
    );
    drop(db);
    assert!(matches!(
        Database::open(
            runtime.open_memory(&key).unwrap(),
            Schema::new(&[Second::SCHEMA]).unwrap()
        ),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
}

mod enum_values {
    use ic_db::design::prelude::*;

    ///
    /// Lifecycle
    /// Distinguishes the `Ready`, `Archived` and `Draft` variants.
    ///

    #[enum_(
        variant(name = "Ready"),
        variant(name = "Archived"),
        variant(name = "Draft")
    )]
    pub struct Lifecycle {}

    ///
    /// Reordered
    /// Distinguishes the `Draft`, `Ready` and `Archived` variants.
    ///

    #[enum_(
        name = "Lifecycle",
        variant(name = "Draft"),
        variant(name = "Ready"),
        variant(name = "Archived")
    )]
    pub struct Reordered {}

    ///
    /// Changed
    /// Distinguishes the `Ready`, `Archived` and `Editing` variants.
    ///

    #[enum_(
        name = "Lifecycle",
        variant(name = "Ready"),
        variant(name = "Archived"),
        variant(name = "Editing")
    )]
    pub struct Changed {}
}

///
/// EnumRow
/// Stores `state` and `previous` under the `id` primary key.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "state", value(item(is = "State"))),
        field(name = "previous", value(opt, item(is = "enum_values::Lifecycle")))
    )
)]
struct EnumRow {}

///
/// EnumReordered
/// Exercises an alternate `EnumRow` declaration under the same logical identity.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "EnumRow",
    pk(field = "id"),
    fields(
        field(name = "state", value(item(is = "enum_values::Reordered"))),
        field(name = "previous", value(opt, item(is = "enum_values::Reordered"))),
        field(name = "id", value(item(prim = "Nat64")))
    )
)]
struct EnumReordered {}

///
/// EnumChanged
/// Exercises an alternate `EnumRow` declaration under the same logical identity.
///

#[entity(
    store = ic_db::testing::FixtureStore,
    name = "EnumRow",
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "state", value(item(is = "enum_values::Changed"))),
        field(name = "previous", value(opt, item(is = "enum_values::Changed")))
    )
)]
struct EnumChanged {}

#[test]
fn named_enums_use_canonical_ordinals_and_exact_admission() {
    use ic_db::Value;
    let ic_db::TypeBody::Enum(variants) = &State::definition().body else {
        panic!("enum");
    };
    assert_eq!(
        variants.iter().map(|v| v.name).collect::<Vec<_>>(),
        ["Archived", "Draft", "Ready"]
    );
    let row = EnumRow {
        id: 7,
        previous: Some(State::Draft),
        state: State::Ready,
    };
    // Independent bytes: id, optional presence, Draft ordinal 1, Ready ordinal 2.
    let golden = [7, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2];
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(EnumRow::decode(&golden).unwrap(), row);
    let reordered = EnumReordered::decode(&golden).unwrap();
    assert_eq!(reordered.state, enum_values::Reordered::Ready);
    assert_eq!(reordered.encode().unwrap(), golden);
    let absent = EnumRow {
        id: 7,
        previous: None,
        state: State::Archived,
    };
    assert_eq!(absent.encode().unwrap(), [7, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    for end in 0..golden.len() {
        assert!(matches!(
            EnumRow::decode(&golden[..end]),
            Err(Error::Corrupt)
        ));
    }
    let mut trailing = golden.to_vec();
    trailing.push(0);
    assert!(matches!(EnumRow::decode(&trailing), Err(Error::Corrupt)));
    for (position, bad) in [(8, 2), (9, 3), (10, 3), (10, 255)] {
        let mut bytes = golden;
        bytes[position] = bad;
        assert!(matches!(EnumRow::decode(&bytes), Err(Error::Corrupt)));
    }
    let contract = || Schema::new(&[EnumRow::SCHEMA]).unwrap();
    // Independent literal framing and Node SHA-256; shared enum definition occurs once.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|byte| format!("{byte:02x}"))
            .concat(),
        "20e6f1d4b2c5b8a34fa2b3032c7686c1872302d22934dd9b70a428f373ea0e3e"
    );
    assert_eq!(
        contract().fingerprint(),
        Schema::new(&[EnumReordered::SCHEMA]).unwrap().fingerprint()
    );
    assert_ne!(
        contract().fingerprint(),
        Schema::new(&[EnumChanged::SCHEMA]).unwrap().fingerprint()
    );
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [9; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&row).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.get::<EnumReordered>(7).unwrap(), Some(reordered));
    assert!(matches!(
        db.get::<EnumChanged>(7),
        Err(Error::UnknownEntity)
    ));
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(*backing.borrow(), before);
    drop(db);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(db.get::<EnumRow>(7).unwrap(), Some(row));
    let mut batch = Batch::new();
    batch.replace(&absent).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let before = backing.borrow().clone();
    assert!(matches!(
        Database::open(
            runtime.open_memory(&key).unwrap(),
            Schema::new(&[EnumChanged::SCHEMA]).unwrap()
        ),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
    // Corrupt an unindexed enum; shared preparation must inspect it before deletion.
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    let mut row_key = vec![2];
    row_key.extend(EnumRow::SCHEMA.id.to_be_bytes());
    row_key.extend(7_u64.to_be_bytes());
    let mut bytes = absent.encode().unwrap();
    *bytes.last_mut().unwrap() = 255;
    raw.insert(row_key, bytes);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let before = backing.borrow().clone();
    let mut batch = Batch::new();
    batch.delete::<EnumRow>(7).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
}

#[test]
fn enum_definitions_and_unsupported_operations_fail_closed() {
    use ic_db::{EntitySchema, FieldType, TypeBody, TypeLink, TypeSchema, VariantSchema};
    fn invalid() -> &'static TypeSchema {
        &TypeSchema {
            name: "Lifecycle",
            body: TypeBody::Enum(&[
                VariantSchema {
                    name: "Ready",
                    payload: None,
                },
                VariantSchema {
                    name: "Draft",
                    payload: None,
                },
            ]),
        }
    }
    let mut fields = EnumRow::SCHEMA.fields.to_vec();
    for field in &mut fields {
        if matches!(field.value, FieldType::Named(_)) {
            field.value = FieldType::Named(TypeLink {
                name: "Lifecycle",
                definition: invalid,
            });
        }
    }
    let altered = Box::leak(Box::new(EntitySchema {
        fields: Box::leak(fields.into_boxed_slice()),
        ..*EnumRow::SCHEMA
    }));
    assert!(matches!(Schema::new(&[altered]), Err(Error::InvalidSchema)));
    // One logical type name cannot carry two conflicting definitions, even across fields.
    let mut fields = EnumRow::SCHEMA.fields.to_vec();
    fields
        .iter_mut()
        .find(|field| field.name == "state")
        .unwrap()
        .value = EnumChanged::SCHEMA
        .fields
        .iter()
        .find(|field| field.name == "state")
        .unwrap()
        .value;
    let altered = Box::leak(Box::new(EntitySchema {
        fields: Box::leak(fields.into_boxed_slice()),
        ..*EnumRow::SCHEMA
    }));
    assert!(matches!(Schema::new(&[altered]), Err(Error::InvalidSchema)));
}

#[test]
fn earlier_unit_enum_contract_is_refused_without_touching_its_bytes() {
    let backing = VectorMemory::default();
    let runtime = host(backing.clone());
    let key = StableKey::parse("test.db.v1").unwrap();
    let contract = || Schema::new(&[EnumRow::SCHEMA]).unwrap();
    let mut db = Database::create(runtime.open_memory(&key).unwrap(), contract(), [9; 16]).unwrap();
    let mut batch = Batch::new();
    batch
        .insert(&EnumRow {
            id: 7,
            previous: Some(State::Draft),
            state: State::Ready,
        })
        .unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let mut raw = StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap());
    let mut manifest = raw.get(&vec![0]).unwrap();
    let old = "3fc6bb4b60668c7a04518cbad5659ce829b313eb2e35cda7d30a38ec3d73a715";
    for i in 0..32 {
        manifest[8 + i] = u8::from_str_radix(&old[i * 2..i * 2 + 2], 16).unwrap();
    }
    raw.insert(vec![0], manifest);
    drop(raw);
    let before = backing.borrow().clone();
    assert!(matches!(
        Database::open(runtime.open_memory(&key).unwrap(), contract()),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), before);
}

#[cfg(feature = "test-support")]
#[test]
fn native_fixture_reopens_application_storage_after_refusals() {
    use ic_db::testing::Fixture;
    let backing = VectorMemory::default();
    let retained = backing.clone();
    let fixture = Fixture::with_allocation(move || {
        host(retained.clone())
            .open_memory(&StableKey::parse("test.db.v1").unwrap())
            .unwrap()
    });
    let schema = Schema::new(&[TokenRow::SCHEMA]).unwrap();
    let before = backing.borrow().clone();
    assert!(matches!(fixture.open(schema.clone()), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
    assert!(matches!(
        fixture.create(schema.clone(), [0; 16]),
        Err(Error::InvalidSchema)
    ));
    let mut db = fixture.create(schema.clone(), [42; 16]).unwrap();
    let mut batch = Batch::new();
    batch.insert(&TokenRow { id: 1, token: 7 }).unwrap();
    db.commit(batch).unwrap();
    let mut batch = Batch::new();
    batch.insert(&TokenRow { id: 2, token: 7 }).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::UniqueViolation)));
    drop(db);

    let populated = backing.borrow().clone();
    assert!(matches!(
        fixture.create(schema.clone(), [43; 16]),
        Err(Error::Exists)
    ));
    assert!(matches!(
        fixture.open(Schema::new(&[Document::SCHEMA]).unwrap()),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(*backing.borrow(), populated);
    let db = fixture.open(schema).unwrap();
    assert_eq!(db.get::<TokenRow>(1).unwrap().unwrap().token, 7);
    assert!(db.get::<TokenRow>(2).unwrap().is_none());
}
