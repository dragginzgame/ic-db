use super::*;

#[test]
fn literals_use_exact_declared_scalar_bytes() {
    for (literal, kind, expected) in [
        (LiteralDefault::Nat(65535), FieldType::Nat16, vec![255, 255]),
        (LiteralDefault::Int(-128), FieldType::Int8, vec![128]),
        (
            LiteralDefault::Int(i128::MIN),
            FieldType::Int128,
            [vec![0; 15], vec![128]].concat(),
        ),
        (
            LiteralDefault::Nat(u128::MAX),
            FieldType::Nat128,
            vec![255; 16],
        ),
        (LiteralDefault::Bool(false), FieldType::Bool, vec![0]),
        (
            LiteralDefault::Text("é"),
            FieldType::Text { max_bytes: 2 },
            vec![2, 0, 0, 0, 0xc3, 0xa9],
        ),
        (
            LiteralDefault::Text(""),
            FieldType::Text { max_bytes: 0 },
            vec![0, 0, 0, 0],
        ),
    ] {
        assert_eq!(literal.encode(kind).unwrap(), expected);
    }
}
