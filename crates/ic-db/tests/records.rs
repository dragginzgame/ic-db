//! First public journey: real generated records, governed storage and final-state batches.
use ic_db::{Batch, Database, Entity, Error, Schema, entity};
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
fn generated_codec_and_identity_ignore_declaration_order_but_not_names() {
    let row = Entry {
        id: 7,
        category: 2,
        amount: 42,
    };
    assert_eq!(row.encode().unwrap(), [42, 2, 7]);
    assert_eq!(Entry::decode(&[42, 2, 7]).unwrap(), row);
    assert!(matches!(Entry::decode(&[42, 2, 7, 0]), Err(Error::Corrupt)));
    let one = Schema::new(&[Entry::SCHEMA]).unwrap();
    // Independent literal grammar assembled with Perl pack(V)/UTF-8 lengths;
    // digest verified by OpenSSL, not the product's schema serializer.
    assert_eq!(
        one.fingerprint().map(|b| format!("{b:02x}")).concat(),
        "cf31e2057b1a4137cc59547765556c77ad8aaad97bf235b66d962da134e9d4aa"
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
    // Independent Wire vectors: canonical delta/id/previous tuple, zigzag i64
    // followed by unsigned LEB128; optional presence is a separate 0/1 byte.
    assert_eq!(row.encode().unwrap(), [1, 7, 1, 0]);
    let mut minimum = vec![0xff; 9];
    minimum.extend([1, 7, 0]);
    let min_row = SignedRow {
        id: 7,
        delta: i64::MIN,
        previous: None,
    };
    assert_eq!(min_row.encode().unwrap(), minimum);
    assert_eq!(SignedRow::decode(&minimum).unwrap(), min_row);
    let mut maximum = vec![0xfe];
    maximum.extend([0xff; 8]);
    maximum.extend([1, 7, 0]);
    let max_row = SignedRow {
        id: 7,
        delta: i64::MAX,
        previous: None,
    };
    assert_eq!(max_row.encode().unwrap(), maximum);
    assert_eq!(SignedRow::decode(&maximum).unwrap(), max_row);
    minimum[9] = 2; // payload exceeds 64 bits
    assert!(matches!(SignedRow::decode(&minimum), Err(Error::Corrupt)));
    assert!(matches!(SignedRow::decode(&[1, 7, 2]), Err(Error::Corrupt)));
    assert!(matches!(SignedRow::decode(&[0x80]), Err(Error::Corrupt)));

    let contract = || Schema::new(&[SignedRow::SCHEMA]).unwrap();
    // Independent Perl pack(V/C) and OpenSSL schema vector.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "d9710d2d315461b90416054bd54e3dae42a7380a9fac53b23a0556d270bbbb78"
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
fn wide_integers_preserve_all_bits_and_reject_overflowing_encodings() {
    let minimum = WideRow {
        id: 7,
        signed: i128::MIN,
        unsigned: u128::MAX,
        optional_signed: None,
        optional_unsigned: None,
    };
    // Independent literal unsigned LEB128 (19 bytes at 128 bits), with signed
    // zigzag mapping: i128::MIN maps to u128::MAX. Canonical field-name order.
    let mut bytes = vec![7, 0, 0];
    bytes.extend([0xff; 18]);
    bytes.push(3);
    bytes.extend([0xff; 18]);
    bytes.push(3);
    assert_eq!(minimum.encode().unwrap(), bytes);
    assert_eq!(WideRow::decode(&bytes).unwrap(), minimum);
    let mut maximum = minimum;
    maximum.signed = i128::MAX;
    bytes[3] = 0xfe;
    assert_eq!(maximum.encode().unwrap(), bytes);
    assert_eq!(WideRow::decode(&bytes).unwrap(), maximum);
    for position in [21, 40] {
        let mut malformed = bytes.clone();
        malformed[position] = 4; // would require 129 bits
        assert!(matches!(WideRow::decode(&malformed), Err(Error::Corrupt)));
    }
    assert!(matches!(WideRow::decode(&bytes[..40]), Err(Error::Corrupt)));
    let zero = WideRow {
        id: 7,
        signed: 0,
        unsigned: 0,
        optional_signed: Some(0),
        optional_unsigned: Some(0),
    };
    assert_eq!(zero.encode().unwrap(), [7, 1, 0, 1, 0, 0, 0]);
    let absent = WideRow {
        optional_signed: None,
        optional_unsigned: None,
        ..zero
    };
    assert_eq!(absent.encode().unwrap(), [7, 0, 0, 0, 0]);
    assert_ne!(zero, absent);

    let contract = || Schema::new(&[WideRow::SCHEMA]).unwrap();
    // Independent Perl pack(V/C) + OpenSSL contract vector.
    assert_eq!(
        contract()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "4295cb7f6bdd45ea72700f93d6b31f40683af7ac0c9064e2916dd607f9a479a5"
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
    let golden = [1, 0, 1, 0, 0, 2, 0xc3, 0xa9]; // id, note, priority, published, title
    assert_eq!(row.encode().unwrap(), golden);
    assert_eq!(Document::decode(&golden).unwrap(), row);
    for malformed in [vec![1, 2], vec![1, 0, 0, 2, 0], vec![1, 0, 0, 0, 1, 0xff]] {
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
    let bytes = ic_db::__private::native::encode(
        &(
            1_u64,
            Option::<String>::None,
            Some(0_u64),
            false,
            &changed.title,
        ),
        ic_db::__private::ROW_LIMITS,
    )
    .unwrap();
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
        "80625391c9330ef8b4379a91b5e93663acd40384608a1a74197813267f2b2b3a"
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
    assert_eq!(raw.get(&row), Some(vec![11, 9, 1]));
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
    // Independent Perl pack(V/C) + OpenSSL vector, including the reference tag,
    // target identity and reverse-index identity. Existing scalar vectors stay fixed.
    assert_eq!(
        Schema::new(&[Parent::SCHEMA, Child::SCHEMA])
            .unwrap()
            .fingerprint()
            .map(|b| format!("{b:02x}"))
            .concat(),
        "e7d4d90bdd9875157123c064930a6af8012d9a3b8e8fdcd4e08e0a75dd41a2ef"
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
