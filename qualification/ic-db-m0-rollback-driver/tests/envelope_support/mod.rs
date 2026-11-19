//! Independent row/index bytes shared by write and migration qualification.

use std::collections::BTreeMap;

const ENTITY: u32 = 931_470_443;
// Independently calculated name handles and declared component orders. The
// oracle imports neither product schemas nor its row/key encoders.
const INDEXES: [(u32, &str); 8] = [
    (3_678_505_679, "abcd"),
    (2_036_686_271, "abdc"),
    (3_668_886_527, "acbd"),
    (127_123_839, "acdb"),
    (1_383_128_007, "adbc"),
    (2_688_715_767, "adcb"),
    (4_254_775_335, "bacd"),
    (2_618_178_807, "badc"),
];
pub fn expected(generation: u8, count: u64) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let mut map = BTreeMap::new();
    for id in 1_u64..=count {
        let mut key = vec![2];
        key.extend(ENTITY.to_be_bytes());
        key.extend(id.to_be_bytes());
        let mut row = Vec::new();
        for field in b"abcd" {
            row.extend((if *field == b'a' { 67_u32 } else { 60 }).to_le_bytes());
            if *field == b'a' {
                row.extend(b"abcdefg");
            }
            row.push(if *field == b'a' { generation } else { 0 });
            row.extend([0; 59]);
        }
        row.extend(id.to_le_bytes());
        row.extend(3821_u32.to_le_bytes());
        row.extend([0; 3821]);
        assert_eq!(row.len(), 4096);
        map.insert(key, row);
        for (index, order) in INDEXES {
            let mut key = vec![3];
            key.extend(ENTITY.to_be_bytes());
            key.extend(index.to_be_bytes());
            for field in order.bytes() {
                if field == b'a' {
                    key.extend(b"abcdefg");
                }
                if field == b'a' && generation == 1 {
                    key.push(1);
                } else {
                    key.extend([0, 255]);
                }
                for _ in 0..59 {
                    key.extend([0, 255]);
                }
                key.extend([0, 0]);
            }
            key.extend(id.to_be_bytes());
            assert_eq!(key.len(), if generation == 0 { 512 } else { 511 });
            map.insert(key, id.to_be_bytes().to_vec());
        }
    }

    map
}
