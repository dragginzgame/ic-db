//! Fixed three-field row codec candidate, not generated schemas or a product format.
//! IC DB owns schema admission and field semantics; Wire owns bounded traversal.

use ic_wire::{Encoding, Limits};

// Canonical field-name order: amount, id, note. No field names in each row.
type Row = (u64, u64, Option<String>);
const CONTRACT: [u8; 32] = [7; 32];
const MAGIC: &[u8; 8] = b"ICDBM0R1";
const HEADER: usize = 12;
const LIMITS: Limits = Limits {
    bytes: 64,
    allocation: 2048,
    nodes: 32,
    depth: 8,
};

#[derive(Debug, PartialEq)]
enum Refusal {
    Schema,
    Frame,
    Codec,
    FieldBound,
}

fn validate(row: &Row) -> Result<(), Refusal> {
    if row.2.as_ref().is_some_and(|note| note.len() > 8) {
        return Err(Refusal::FieldBound);
    }

    Ok(())
}

fn encode(row: &Row) -> Result<Vec<u8>, Refusal> {
    validate(row)?;
    let payload = ic_wire::encode(row, Encoding::Payload, LIMITS).map_err(|_| Refusal::Codec)?;
    let mut frame = MAGIC.to_vec();
    frame.extend((payload.len() as u32).to_le_bytes());
    frame.extend(payload);

    Ok(frame)
}

fn decode(stored_contract: &[u8; 32], bytes: &[u8], limits: Limits) -> Result<Row, Refusal> {
    // In the product the manifest admits the collection before any row is opened.
    // The stand-in identity is external to the row, not copied into every record.
    if *stored_contract != CONTRACT {
        return Err(Refusal::Schema);
    }
    if bytes.len() < HEADER || bytes.len() > HEADER + LIMITS.bytes || &bytes[..8] != MAGIC {
        return Err(Refusal::Frame);
    }
    let declared = u32::from_le_bytes(bytes[8..HEADER].try_into().unwrap()) as usize;
    if declared != bytes.len() - HEADER {
        return Err(Refusal::Frame);
    }
    let row =
        ic_wire::decode(&bytes[HEADER..], Encoding::Payload, limits).map_err(|_| Refusal::Codec)?;
    validate(&row)?;

    Ok(row)
}

#[test]
fn named_contract_uses_compact_positional_payload() {
    let row = (42, 7, Some("oak".into()));
    let expected = b"ICDBM0R1\x07\0\0\0\x2a\x07\x01\x03oak";
    assert_eq!(encode(&row).unwrap(), expected);
    assert_eq!(decode(&CONTRACT, expected, LIMITS).unwrap(), row);
    for note in [None, Some(String::new()), Some("éééé".into())] {
        let row = (u64::MAX, 0, note);
        assert_eq!(
            decode(&CONTRACT, &encode(&row).unwrap(), LIMITS).unwrap(),
            row
        );
    }
    assert_ne!(encode(&(0, 0, None)), encode(&(0, 0, Some(String::new()))));
}

#[test]
fn schema_gate_and_frame_checks_precede_payload_decode() {
    let valid = encode(&(42, 7, Some("oak".into()))).unwrap();
    assert_eq!(decode(&[8; 32], &[], LIMITS), Err(Refusal::Schema));
    for length in 0..valid.len() {
        assert!(decode(&CONTRACT, &valid[..length], LIMITS).is_err());
    }
    let mut unknown = valid.clone();
    unknown[7] = b'2';
    assert_eq!(decode(&CONTRACT, &unknown, LIMITS), Err(Refusal::Frame));
    let mut trailing = valid.clone();
    trailing.push(0);
    assert_eq!(decode(&CONTRACT, &trailing, LIMITS), Err(Refusal::Frame));
    // Even a corrected length cannot hide an extra payload byte from Wire.
    trailing[8..HEADER].copy_from_slice(&8_u32.to_le_bytes());
    assert_eq!(decode(&CONTRACT, &trailing, LIMITS), Err(Refusal::Codec));
    let mut corrupt = valid;
    corrupt[HEADER + 2] = 2; // Invalid Option discriminator.
    assert_eq!(decode(&CONTRACT, &corrupt, LIMITS), Err(Refusal::Codec));
}

#[test]
fn wire_work_limits_and_field_byte_bounds_are_separate() {
    assert_eq!(
        encode(&(0, 0, Some("ééééx".into()))),
        Err(Refusal::FieldBound)
    );
    let valid = encode(&(42, 7, Some("oak".into()))).unwrap();
    let limits = [
        Limits { bytes: 1, ..LIMITS },
        Limits {
            allocation: 0,
            ..LIMITS
        },
        Limits { nodes: 1, ..LIMITS },
        Limits { depth: 1, ..LIMITS },
    ];
    for limit in limits {
        assert_eq!(decode(&CONTRACT, &valid, limit), Err(Refusal::Codec));
    }
    // This is a valid Wire string but violates the database field's eight-byte bound.
    let too_long = b"ICDBM0R1\x0d\0\0\0\x00\x00\x01\x09abcdefghi";
    assert_eq!(
        decode(&CONTRACT, too_long, LIMITS),
        Err(Refusal::FieldBound)
    );
}
