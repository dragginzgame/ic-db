//! Canonical-contract sensitivity witness, not a schema registry or fingerprint format.
//! This fixed codec lays out fields by explicit ID; positional codecs need different rules.

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
    id: u32,
    kind: u8,
    bound: u32,
    source_name: &'static str,
}
#[derive(Clone)]
struct Variant {
    id: u32,
    payload: u8,
    source_name: &'static str,
}
#[derive(Clone)]
struct Index {
    id: u32,
    unique: bool,
    fields: Vec<(u32, u8)>,
    source_name: &'static str,
}
#[derive(Clone)]
struct Contract {
    entity_id: u32,
    source_name: &'static str,
    documentation: &'static str,
    semantics: Semantics,
    fields: Vec<Field>,
    variants: Vec<Variant>,
    indexes: Vec<Index>,
}

fn canonical(contract: &Contract) -> Result<Vec<u8>, &'static str> {
    if contract.entity_id == 0
        || contract.fields.is_empty()
        || contract.fields.len() > 8
        || contract.variants.len() > 8
        || contract.indexes.len() > 8
        || contract.semantics.versions().contains(&0)
    {
        return Err("contract bounds");
    }
    let mut fields: Vec<_> = contract.fields.iter().collect();
    let mut variants: Vec<_> = contract.variants.iter().collect();
    let mut indexes: Vec<_> = contract.indexes.iter().collect();
    fields.sort_by_key(|field| field.id);
    variants.sort_by_key(|variant| variant.id);
    indexes.sort_by_key(|index| index.id);
    if fields.windows(2).any(|pair| pair[0].id == pair[1].id)
        || variants.windows(2).any(|pair| pair[0].id == pair[1].id)
        || indexes.windows(2).any(|pair| pair[0].id == pair[1].id)
    {
        return Err("duplicate identity");
    }
    let mut bytes = b"ICDBM0C1".to_vec();
    bytes.extend(contract.entity_id.to_le_bytes());
    for version in contract.semantics.versions() {
        bytes.extend(version.to_le_bytes());
    }
    bytes.push(fields.len() as u8);
    for field in &fields {
        if field.id == 0 || !(1..=3).contains(&field.kind) || !(1..=1024).contains(&field.bound) {
            return Err("field contract");
        }
        bytes.extend(field.id.to_le_bytes());
        bytes.push(field.kind);
        bytes.extend(field.bound.to_le_bytes());
    }
    bytes.push(variants.len() as u8);
    for variant in variants {
        if variant.id == 0 || variant.payload > 3 {
            return Err("variant contract");
        }
        bytes.extend(variant.id.to_le_bytes());
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
            if !fields.iter().any(|candidate| candidate.id == field) || direction > 1 {
                return Err("index field");
            }
            bytes.extend(field.to_le_bytes());
            bytes.push(direction);
        }
    }
    Ok(bytes)
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
                id: 10,
                kind: 1,
                bound: 17,
                source_name: "amount",
            },
            Field {
                id: 20,
                kind: 2,
                bound: 8,
                source_name: "id",
            },
        ],
        variants: vec![
            Variant {
                id: 3,
                payload: 1,
                source_name: "Present",
            },
            Variant {
                id: 8,
                payload: 0,
                source_name: "Absent",
            },
        ],
        indexes: vec![Index {
            id: 5,
            unique: true,
            fields: vec![(10, 0), (20, 0)],
            source_name: "by_amount",
        }],
    }
}

#[test]
fn canonical_bytes_match_independent_fixed_contract() {
    let mut expected = b"ICDBM0C1".to_vec();
    // Literal format witness: entity, semantic versions, fields, variants, index columns.
    expected.extend([
        42, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0, 7, 0,
        0, 0, 8, 0, 0, 0, 9, 0, 0, 0, 2, 10, 0, 0, 0, 1, 17, 0, 0, 0, 20, 0, 0, 0, 2, 8, 0, 0, 0,
        2, 3, 0, 0, 0, 1, 8, 0, 0, 0, 0, 1, 5, 0, 0, 0, 1, 2, 10, 0, 0, 0, 0, 20, 0, 0, 0, 0,
    ]);
    assert_eq!(canonical(&source()).unwrap(), expected);
}

#[test]
fn renames_and_declaration_order_preserve_explicit_identity() {
    let source = source();
    let mut renamed = source.clone();
    renamed.source_name = "RenamedEntry";
    renamed.documentation = "Revised prose";
    renamed.fields[0].source_name = "renamed_amount";
    renamed.variants[0].source_name = "SomeValue";
    renamed.indexes[0].source_name = "new_label";
    assert_ne!(source.source_name, renamed.source_name);
    assert_ne!(source.documentation, renamed.documentation);
    assert_ne!(source.fields[0].source_name, renamed.fields[0].source_name);
    assert_ne!(
        source.variants[0].source_name,
        renamed.variants[0].source_name
    );
    assert_ne!(
        source.indexes[0].source_name,
        renamed.indexes[0].source_name
    );
    renamed.fields.reverse();
    renamed.variants.reverse();
    assert_eq!(canonical(&source), canonical(&renamed));
}

#[test]
fn behavior_and_layout_changes_change_contract_identity() {
    let original = source();
    let bytes = canonical(&original).unwrap();
    let changes: [fn(&mut Contract); 17] = [
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
        |c| c.fields[0].kind = 3,
        |c| c.fields[0].bound += 1,
        |c| c.variants[0].id = 4,
        |c| c.variants[0].payload = 2,
        |c| c.indexes[0].unique = false,
        |c| c.indexes[0].fields.reverse(),
        |c| c.indexes[0].fields[0].1 = 1,
    ];
    for (case, change) in changes.into_iter().enumerate() {
        let mut changed = original.clone();
        change(&mut changed);
        assert_ne!(
            canonical(&changed).unwrap(),
            bytes,
            "sensitivity case {case}"
        );
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
    invalid.indexes.push(invalid.indexes[0].clone());
    assert!(canonical(&invalid).is_err());
    let mut invalid = source();
    invalid.indexes[0].fields[0].0 = 99;
    assert!(canonical(&invalid).is_err());
    let mut invalid = source();
    invalid.fields[0].bound = 0;
    assert!(canonical(&invalid).is_err());
    let mut invalid = source();
    invalid.semantics.supplier_format = 0;
    assert!(canonical(&invalid).is_err());
}
