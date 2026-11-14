//! First public journey: real generated records, governed storage and final-state batches.
use ic_db::{Batch, Blob, Database, Entity, Error, Schema, entity};
use ic_storage::{
    AllocationGrants, GenericAllocationPolicy, MemoryAuthority, MemoryRequest, MemoryRuntime,
    SealedDeclarationSnapshot, StableKey, VectorMemory, adapter::StableBTreeMap,
};

#[entity(id=1, pk(field="id", source="external"), index(id=1, fields=["category", "id"]),
    fields(field(name="id", value(item(prim="Nat64"))),
        field(name="category", value(item(prim="Nat64"))),
        field(name="amount", value(item(prim="Nat64")))))]
struct Entry {}

#[entity(
    id = 2,
    pk(field = "id"),
    index(id = 1, field = "token", unique),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "token", value(item(prim = "Nat64")))
    )
)]
struct TokenRow {}

#[entity(id=1, pk(field="id"), index(id=1, fields=["category", "id"]),
    fields(field(name="value", value(item(prim="Nat64"))), field(name="id", value(item(prim="Nat64"))),
        field(name="category", value(item(prim="Nat64")))))]
struct Changed {}

#[entity(id=1, pk(field="id"), index(id=1, fields=["category", "id"]),
    fields(field(name="amount", value(item(prim="Nat64"))), field(name="category", value(item(prim="Nat64"))),
        field(name="id", value(item(prim="Nat64")))))]
struct Reordered {}

#[entity(
    id = 2,
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

#[entity(
    id = 3,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "enabled", value(opt, item(prim = "Bool"))),
        field(name = "text", value(item(prim = "Text", max_bytes = 4096)))
    )
)]
struct WideText {}

#[entity(
    id = 20,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "delta", value(item(prim = "Int64"))),
        field(name = "previous", value(opt, item(prim = "Int64")))
    )
)]
struct SignedRow {}

#[entity(
    id = 21,
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

#[entity(
    id = 22,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 1024))),
        field(name = "optional", value(opt, item(prim = "Blob", max_bytes = 32)))
    )
)]
struct BinaryRow {}
#[entity(
    id = 23,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 0)))
    )
)]
struct EmptyBlob {}
#[entity(
    id = 25,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(prim = "Text", max_bytes = 0)))
    )
)]
struct EmptyText {}
#[entity(
    id = 24,
    pk(field = "id"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "payload", value(item(prim = "Blob", max_bytes = 4096)))
    )
)]
struct WideBlob {}

#[entity(
    id = 26,
    pk(field = "reader"),
    fields(
        field(name = "reader", value(item(prim = "Nat64"))),
        field(name = "writer", value(opt, item(prim = "Text", max_bytes = 8))),
        field(name = "row", value(item(prim = "Bool")))
    )
)]
struct CodecNames {}

#[entity(
    id = 27,
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

#[entity(id=28, pk(field="id"), index(id=1,fields=["category","id"]), fields(
    field(name="a_blob",value(item(prim="Blob",max_bytes=2048))),
    field(name="a_note",value(opt,item(prim="Text",max_bytes=64))),
    field(name="category",value(item(prim="Nat64"))),
    field(name="id",value(item(prim="Nat64"))),
    field(name="z_flag",value(item(prim="Bool"))),
    field(name="z_text",value(item(prim="Text",max_bytes=64)))
))]
struct InspectionRow {}

#[entity(id=29,pk(field="id"),index(id=1,fields=["parent_id","id"]),fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="note",value(item(prim="Text",max_bytes=32))),
    field(name="parent_id",value(item(prim="Nat64")),references(entity=28,index=1))
))]
struct InspectionDependent {}

#[entity(
    id = 10,
    pk(field = "id"),
    fields(field(name = "id", value(item(prim = "Nat64"))))
)]
struct Parent {}
#[entity(id=11, pk(field="id"), index(id=1,fields=["parent_id","id"]), fields(
    field(name="id",value(item(prim="Nat64"))),
    field(name="parent_id",value(item(prim="Nat64")),references(entity=10,index=1))))]
struct Child {}
#[entity(
    id = 12,
    pk(field = "id"),
    index(id = 1, field = "next"),
    fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "next",
            value(item(prim = "Nat64")),
            references(entity = 12, index = 1)
        )
    )
)]
struct Node {}

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
    // Independent v1 (Wire) and v2 (owned varint) fingerprints and original bytes.
    // Admission must refuse before interpreting those bytes under the new codec.
    for old in [
        "cf31e2057b1a4137cc59547765556c77ad8aaad97bf235b66d962da134e9d4aa",
        "a1e8982196ded1c94a3625762e178927acc467051a8c37210a4e84c7c76ab851",
    ] {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap())
                .unwrap();
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
    // Independent literal grammar assembled with Perl pack(V)/UTF-8 lengths;
    // digest verified independently, not the product's schema serializer.
    assert_eq!(
        one.fingerprint().map(|b| format!("{b:02x}")).concat(),
        "daeb2925a2a9e739d630f56189a8956a63e88fe624984a25034c5600ca2e7b60"
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
    // Independent Perl pack(V/C) and SHA-256 schema vector.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "17daf74608b78e95125a9781e27dc415c460a9acc52ca18ca7e33ae02767d7e9"
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
    assert_eq!(db.revision(20).unwrap(), 2);
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
    // Independent Perl pack(V/C) + SHA-256 contract vector.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "b865abdc3a28a04bdff6728de591335214cdaecb8a582c67d55dc0ae52cbf450"
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
    assert_eq!(db.revision(21).unwrap(), 2);
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
    // Mixed-type contract assembled independently with Perl pack(V/C), UTF-8
    // name lengths and OpenSSL SHA-256, alongside the unchanged Nat64 vector.
    assert_eq!(
        fingerprint.map(|b| format!("{b:02x}")).concat(),
        "afd4c6c415fdf7749d20c1f39a5eee7509bf657aa634e6ede4b81a3245fb6893"
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
    assert_eq!(db.revision(2).unwrap(), 2);
    assert_eq!(db.revision(1).unwrap(), 0);
    drop(db);
    drop(runtime);
    let runtime = host(backing);
    let db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    assert_eq!(db.get::<Document>(1).unwrap(), Some(row));
    assert_eq!(db.revision(2).unwrap(), 2);
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
    assert_eq!(db.revision(1).unwrap(), 1);
    assert_eq!(db.revision(2).unwrap(), 1);
    assert_eq!(db.get::<Entry>(1).unwrap().unwrap().amount, 10);
    assert!(db.get::<Entry>(99).unwrap().is_none());
    assert!(matches!(db.get::<Changed>(1), Err(Error::UnknownEntity)));
    assert_eq!(
        db.index_prefix::<Entry>(1, &[2], 3)
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
    assert_eq!(db.revision(2).unwrap(), 2);
    assert_eq!(db.index_prefix::<TokenRow>(1, &[4], 3).unwrap()[0].id, 1);
    assert_eq!(db.index_prefix::<Entry>(1, &[2], 3).unwrap()[0].id, 2);
    assert_eq!(db.index_prefix::<Entry>(1, &[9], 3).unwrap()[0].amount, 11);
    let mut batch = Batch::new();
    batch.insert(&TokenRow { id: 3, token: 4 }).unwrap();
    batch.delete::<TokenRow>(1).unwrap();
    db.commit(batch).unwrap();
    assert_eq!(db.index_prefix::<TokenRow>(1, &[4], 3).unwrap()[0].id, 3);
    drop(db);
    drop(runtime);

    let runtime = host(backing.clone());
    let db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
    assert_eq!(db.incarnation(), [1; 16]);
    assert_eq!(db.get::<Entry>(1).unwrap().unwrap().amount, 11);
    assert_eq!(db.revision(2).unwrap(), 3);
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
        Err(Error::Storage(_))
    ));
    assert_eq!(*backing.borrow(), before);
    // Independent corruption control removes authority, never reconstructs it.
    let mut raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap()).unwrap();
    // Literal physical keys independently check row/index representation. A stale
    // index pointing at a decodable row must not silently return that wrong row.
    let row = vec![2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1];
    let original = vec![
        11, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(raw.get(&row), Some(original.clone()));
    let index = vec![
        3, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0,
        0, 0, 1,
    ];
    assert_eq!(raw.get(&index), Some(1_u64.to_be_bytes().to_vec()));
    raw.insert(index.clone(), 2_u64.to_be_bytes().to_vec());
    drop(raw);
    let db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
    assert!(matches!(
        db.index_prefix::<Entry>(1, &[9], 3),
        Err(Error::Corrupt)
    ));
    drop(db);
    let mut raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap()).unwrap();
    raw.insert(index, 1_u64.to_be_bytes().to_vec());
    drop(raw);
    // Each read boundary must still reject malformed bytes and a decoded key
    // that disagrees with its physical row key after redundant decodes are removed.
    let mut wrong_key = original.clone();
    wrong_key[16] = 2;
    for corrupt in [vec![0x80], wrong_key] {
        let mut raw =
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap())
                .unwrap();
        raw.insert(row.clone(), corrupt);
        drop(raw);
        let before = backing.borrow().clone();
        let db = Database::open(runtime.open_memory(&key).unwrap(), schema()).unwrap();
        assert!(matches!(db.get::<Entry>(1), Err(Error::Corrupt)));
        assert!(matches!(db.page::<Entry>(None, 1), Err(Error::Corrupt)));
        assert!(matches!(
            db.index_prefix::<Entry>(1, &[9], 1),
            Err(Error::Corrupt)
        ));
        assert_eq!(*backing.borrow(), before);
    }
    let mut raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap()).unwrap();
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
    // Independent Perl pack(V/C) + SHA-256 vector, including the reference tag,
    // target identity and reverse-index identity. Existing scalar vectors stay fixed.
    assert_eq!(
        Schema::new(&[Parent::SCHEMA, Child::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "f8db0c68762edcc35e35aea372a747534023a450e8153831f896c8ad3c81c3fd"
    );
    assert!(matches!(
        Schema::new(&[Child::SCHEMA]),
        Err(Error::InvalidSchema)
    ));
    let contract = || Schema::new(&[Parent::SCHEMA, Child::SCHEMA, Node::SCHEMA]).unwrap();
    for target in [None, Some(12)] {
        let mut fields = Child::SCHEMA.fields.to_vec();
        fields[1].reference = target.map(|entity| ic_db::ReferenceSchema { entity, index: 1 });
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
    assert!(db.index_prefix::<Child>(1, &[1], 32).unwrap().is_empty());
    assert_eq!(
        db.index_prefix::<Child>(1, &[2], 32).unwrap(),
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
    // The longest valid dependent removal batch: 31 skips and the parent delete.
    let mut batch = Batch::new();
    batch.insert(&Parent { id: 3 }).unwrap();
    for id in 100..131 {
        batch.insert(&Child { id, parent_id: 3 }).unwrap();
    }
    db.commit(batch).unwrap();
    let mut batch = Batch::new();
    batch.delete::<Parent>(3).unwrap();
    for id in 100..131 {
        batch.delete::<Child>(id).unwrap();
    }
    db.commit(batch).unwrap();
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
    assert!(db.page::<Child>(None, 32).unwrap().rows.is_empty());
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
        db.index_prefix::<InspectionRow>(1, &[2], 3)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.index_prefix::<InspectionRow>(1, &[3], 3).unwrap(),
        [changed]
    );
    let mut batch = Batch::new();
    batch.replace(&row).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    let physical_key = vec![2, 0, 0, 0, 28, 0, 0, 0, 0, 0, 0, 0, 1];
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
            StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap())
                .unwrap();
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
    let mut raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap()).unwrap();
    raw.insert(physical_key, original);
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let mut batch = Batch::new();
    batch.insert(&dependent).unwrap();
    db.commit(batch).unwrap();
    drop(db);
    // Reverse-reference inspection also rejects corruption outside its key fields.
    let dependent_key = vec![2, 0, 0, 0, 29, 0, 0, 0, 0, 0, 0, 0, 1];
    let mut invalid = vec![1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 255];
    invalid.extend([1, 0, 0, 0, 0, 0, 0, 0]);
    let mut raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap()).unwrap();
    raw.insert(dependent_key.clone(), invalid);
    drop(raw);
    let before = backing.borrow().clone();
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let mut batch = Batch::new();
    batch.delete::<InspectionRow>(1).unwrap();
    assert!(matches!(db.commit(batch), Err(Error::Corrupt)));
    assert_eq!(*backing.borrow(), before);
    drop(db);
    let mut raw =
        StableBTreeMap::<Vec<u8>, Vec<u8>, _>::load(runtime.open_memory(&key).unwrap()).unwrap();
    raw.insert(dependent_key, dependent.encode().unwrap());
    drop(raw);
    let mut db = Database::open(runtime.open_memory(&key).unwrap(), contract()).unwrap();
    let mut batch = Batch::new();
    batch.delete::<InspectionRow>(1).unwrap();
    batch.delete::<InspectionDependent>(1).unwrap();
    db.commit(batch).unwrap();
    assert!(
        db.index_prefix::<InspectionRow>(1, &[2], 3)
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
    // Independent Perl pack(V/C) + SHA-256 contract vector.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "f6354d993e0b2f35571ed07b24c8fb11a2d521a1a5ccdfa0b9c4ea94f1df01a1"
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
    assert_eq!(db.revision(27).unwrap(), 6);
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
    assert_eq!(db.revision(27).unwrap(), 6);
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
    // Independent Perl pack(V/C) + SHA-256 schema vector.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "109a99d350ab1d8ccb0e1c84f7b8765b36eac680f2c81bfcbc19e310df788f24"
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
    assert_eq!(db.revision(22).unwrap(), 2);
}
