//! Canonical-contract sensitivity witness, not a schema registry or fingerprint format.
//! This fixed contract orders fields by name; positional codecs need different rules.

use sha2::{Digest, Sha256};

const FINGERPRINT_DOMAIN: &[u8] = b"ic-db:schema:v1\0";

#[derive(Clone)]
struct Semantics {
    row_codec: u32,
    key_codec: u32,
    comparator: u32,
    normalization: u32,
    null_order: u32,
    uniqueness: u32,
    references: u32,
    defaults_timestamps: u32,
    supplier_format: u32,
}

impl Semantics {
    fn versions(&self) -> [u32; 9] {
        [
            self.row_codec,
            self.key_codec,
            self.comparator,
            self.normalization,
            self.null_order,
            self.uniqueness,
            self.references,
            self.defaults_timestamps,
            self.supplier_format,
        ]
    }
}

#[derive(Clone)]
struct Field {
    name: &'static str,
    kind: u8,
    bound: u32,
}
#[derive(Clone)]
struct Variant {
    name: &'static str,
    payload: u8,
}
#[derive(Clone)]
struct Index {
    id: u32,
    unique: bool,
    fields: Vec<(&'static str, u8)>,
    source_name: &'static str,
}
#[derive(Clone)]
struct Contract {
    entity_id: u32,
    source_name: &'static str,
    documentation: &'static str,
    semantics: Semantics,
    fields: Vec<Field>,
    primary_key: Vec<&'static str>,
    variants: Vec<Variant>,
    indexes: Vec<Index>,
}

fn canonical(contract: &Contract) -> Result<Vec<u8>, &'static str> {
    if contract.entity_id == 0
        || contract.fields.is_empty()
        || contract.fields.len() > 8
        || contract.variants.len() > 8
        || contract.indexes.len() > 8
        || contract.primary_key.is_empty()
        || contract.primary_key.len() > 4
        || contract.semantics.versions().contains(&0)
    {
        return Err("contract bounds");
    }
    let mut fields: Vec<_> = contract.fields.iter().collect();
    let mut variants: Vec<_> = contract.variants.iter().collect();
    let mut indexes: Vec<_> = contract.indexes.iter().collect();
    fields.sort_by_key(|field| field.name);
    variants.sort_by_key(|variant| variant.name);
    indexes.sort_by_key(|index| index.id);
    if fields.windows(2).any(|pair| pair[0].name == pair[1].name)
        || variants.windows(2).any(|pair| pair[0].name == pair[1].name)
        || indexes.windows(2).any(|pair| pair[0].id == pair[1].id)
    {
        return Err("duplicate identity");
    }
    let mut bytes = b"ICDBM0C4".to_vec();
    bytes.extend(contract.entity_id.to_le_bytes());
    for version in contract.semantics.versions() {
        bytes.extend(version.to_le_bytes());
    }
    bytes.push(fields.len() as u8);
    for field in &fields {
        if field.name.is_empty()
            || field.name.len() > 64
            || !(1..=3).contains(&field.kind)
            || !(1..=1024).contains(&field.bound)
        {
            return Err("field contract");
        }
        bytes.push(field.name.len() as u8);
        bytes.extend(field.name.as_bytes());
        bytes.push(field.kind);
        bytes.extend(field.bound.to_le_bytes());
    }
    bytes.push(contract.primary_key.len() as u8);
    for (position, &name) in contract.primary_key.iter().enumerate() {
        if !fields.iter().any(|field| field.name == name)
            || contract.primary_key[..position].contains(&name)
        {
            return Err("primary key field");
        }
        bytes.push(name.len() as u8);
        bytes.extend(name.as_bytes());
    }
    bytes.push(variants.len() as u8);
    for variant in variants {
        if variant.name.is_empty() || variant.name.len() > 64 || variant.payload > 3 {
            return Err("variant contract");
        }
        bytes.push(variant.name.len() as u8);
        bytes.extend(variant.name.as_bytes());
        bytes.push(variant.payload);
    }
    bytes.push(indexes.len() as u8);
    for index in indexes {
        if index.id == 0 || index.fields.is_empty() || index.fields.len() > 8 {
            return Err("index contract");
        }
        bytes.extend(index.id.to_le_bytes());
        bytes.push(u8::from(index.unique));
        bytes.push(index.fields.len() as u8);
        // Index column order is semantic; unlike declaration order it must not be sorted away.
        for &(field, direction) in &index.fields {
            if !fields.iter().any(|candidate| candidate.name == field) || direction > 1 {
                return Err("index field");
            }
            bytes.push(field.len() as u8);
            bytes.extend(field.as_bytes());
            bytes.push(direction);
        }
    }
    Ok(bytes)
}

fn fingerprint(contract: &Contract) -> Result<[u8; 32], &'static str> {
    let canonical = canonical(contract)?;
    let mut hash = Sha256::new();
    hash.update(FINGERPRINT_DOMAIN);
    hash.update(canonical);
    Ok(hash.finalize().into())
}

fn source() -> Contract {
    Contract {
        entity_id: 42,
        source_name: "Entry",
        documentation: "Example",
        semantics: Semantics {
            row_codec: 1,
            key_codec: 2,
            comparator: 3,
            normalization: 4,
            null_order: 5,
            uniqueness: 6,
            references: 7,
            defaults_timestamps: 8,
            supplier_format: 9,
        },
        fields: vec![
            Field {
                name: "amount",
                kind: 1,
                bound: 17,
            },
            Field {
                name: "id",
                kind: 2,
                bound: 8,
            },
        ],
        primary_key: vec!["id"],
        variants: vec![
            Variant {
                name: "Present",
                payload: 1,
            },
            Variant {
                name: "Absent",
                payload: 0,
            },
        ],
        indexes: vec![Index {
            id: 5,
            unique: true,
            fields: vec![("amount", 0), ("id", 0)],
            source_name: "by_amount",
        }],
    }
}

#[test]
fn canonical_bytes_match_independent_fixed_contract() {
    let mut expected = b"ICDBM0C4".to_vec();
    // Literal format witness: entity, semantic versions, fields, variants, index columns.
    expected.extend([
        42, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0, 7, 0,
        0, 0, 8, 0, 0, 0, 9, 0, 0, 0, 2,
    ]);
    expected.extend(b"\x06amount\x01\x11\0\0\0\x02id\x02\x08\0\0\0");
    expected.extend(b"\x01\x02id");
    expected.extend(b"\x02\x06Absent\0\x07Present\x01");
    expected.extend([1, 5, 0, 0, 0, 1, 2]);
    expected.extend(b"\x06amount\0\x02id\0");
    assert_eq!(canonical(&source()).unwrap(), expected);
}

#[test]
fn display_names_and_declaration_order_preserve_identity() {
    let source = source();
    let mut renamed = source.clone();
    renamed.source_name = "RenamedEntry";
    renamed.documentation = "Revised prose";
    renamed.indexes[0].source_name = "new_label";
    assert_ne!(source.source_name, renamed.source_name);
    assert_ne!(source.documentation, renamed.documentation);
    assert_ne!(
        source.indexes[0].source_name,
        renamed.indexes[0].source_name
    );
    renamed.fields.reverse();
    renamed.variants.reverse();
    assert_eq!(canonical(&source), canonical(&renamed));
    assert_eq!(fingerprint(&source), fingerprint(&renamed));
}

#[test]
fn behavior_and_layout_changes_change_contract_identity() {
    let original = source();
    let bytes = canonical(&original).unwrap();
    let changes: [fn(&mut Contract); 19] = [
        |c| c.semantics.row_codec += 1,
        |c| c.semantics.key_codec += 1,
        |c| c.semantics.comparator += 1,
        |c| c.semantics.normalization += 1,
        |c| c.semantics.null_order += 1,
        |c| c.semantics.uniqueness += 1,
        |c| c.semantics.references += 1,
        |c| c.semantics.defaults_timestamps += 1,
        |c| c.semantics.supplier_format += 1,
        |c| c.entity_id += 1,
        |c| c.primary_key = vec!["amount"],
        |c| c.fields[0].kind = 3,
        |c| c.fields[0].bound += 1,
        |c| c.variants[0].name = "SomeValue",
        |c| c.variants[0].payload = 2,
        |c| c.indexes[0].unique = false,
        |c| c.indexes[0].fields.reverse(),
        |c| c.indexes[0].fields[0].1 = 1,
        |c| {
            c.fields[0].name = "total";
            c.indexes[0].fields[0].0 = "total";
        },
    ];
    for (case, change) in changes.into_iter().enumerate() {
        let mut changed = original.clone();
        change(&mut changed);
        assert_ne!(
            canonical(&changed).unwrap(),
            bytes,
            "sensitivity case {case}"
        );
        assert_ne!(fingerprint(&changed), fingerprint(&original));
    }
}

#[test]
fn invalid_identity_graphs_refuse_instead_of_normalizing_away_errors() {
    let mut invalid = source();
    invalid.fields.push(invalid.fields[0].clone());
    assert!(canonical(&invalid).is_err());
    let mut invalid = source();
    invalid.variants.push(invalid.variants[0].clone());
    assert!(canonical(&invalid).is_err());
    let mut invalid = source();
    invalid.variants[0].name = "";
    assert!(canonical(&invalid).is_err());
    let mut invalid = source();
    invalid.indexes.push(invalid.indexes[0].clone());
    assert!(canonical(&invalid).is_err());
    let mut invalid = source();
    invalid.indexes[0].fields[0].0 = "missing";
    assert!(canonical(&invalid).is_err());
    let mut invalid = source();
    invalid.fields[0].bound = 0;
    assert!(canonical(&invalid).is_err());
    let mut invalid = source();
    invalid.semantics.supplier_format = 0;
    assert!(canonical(&invalid).is_err());
    let mut invalid = source();
    invalid.fields[0].name = "total";
    // A rename must also update its referencing index; no inferred identity mapping.
    assert!(canonical(&invalid).is_err());
    let mut invalid = source();
    invalid.fields[0].name = "";
    assert!(canonical(&invalid).is_err());
    let mut invalid = source();
    invalid.fields[0].name = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklm";
    assert!(canonical(&invalid).is_err());
    for key in [vec![], vec!["missing"], vec!["id", "id"]] {
        let mut invalid = source();
        invalid.primary_key = key;
        assert!(fingerprint(&invalid).is_err());
    }
}

#[test]
fn composite_primary_key_order_is_semantic() {
    let mut first = source();
    first.primary_key = vec!["amount", "id"];
    let mut second = first.clone();
    second.primary_key.reverse();
    assert_ne!(fingerprint(&first), fingerprint(&second));
}

#[test]
fn fingerprint_matches_independent_sha256_vector() {
    // OpenSSL SHA-256 over the literal canonical vector above, prefixed by
    // b"ic-db:schema:v1\0". No schema parser or Rust hash implementation in the oracle.
    let expected = [
        0xef, 0xa3, 0x0e, 0x0b, 0x12, 0xd9, 0x26, 0x06, 0x3a, 0x80, 0x59, 0x67, 0xb3, 0x7e, 0xef,
        0x01, 0xad, 0xcb, 0x80, 0x38, 0xf0, 0xf4, 0x2f, 0x4e, 0x69, 0xc4, 0xbf, 0x4c, 0x22, 0x56,
        0x78, 0xd3,
    ];
    assert_eq!(fingerprint(&source()).unwrap(), expected);
    // Omitting the domain must not produce the admitted identity.
    let untagged: [u8; 32] = Sha256::digest(canonical(&source()).unwrap()).into();
    assert_ne!(untagged, expected);
}
