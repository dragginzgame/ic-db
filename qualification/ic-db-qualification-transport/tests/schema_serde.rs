//! Public schema value representations at the Wire and JSON boundaries.
use ic_db::{Date, Decimal, Duration, Float32, Float64, Timestamp, U256, Ulid};
use ic_wire::{Encoding, Limits};
use serde::{Serialize, de::DeserializeOwned};

const LIMITS: Limits = Limits {
    bytes: 1024,
    allocation: 32 * 1024,
    nodes: 256,
    depth: 16,
};

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    ic_wire::encode(value, Encoding::Payload, LIMITS).unwrap()
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ic_wire::Error> {
    ic_wire::decode(bytes, Encoding::Payload, LIMITS)
}

#[test]
fn bare_ulid_accepts_the_existing_fixed_array_endpoint_payload() {
    let bytes = core::array::from_fn(|i| (i * 17) as u8);
    let id = Ulid::from_bytes(bytes);
    assert_eq!(encode(&id), bytes);
    assert_eq!(decode::<Ulid>(&encode(&bytes)).unwrap(), id);
    assert_eq!(decode::<[u8; 16]>(&encode(&id)).unwrap(), bytes);
    assert!(decode::<Ulid>(&bytes[..15]).is_err());
    assert_eq!(serde_json::to_value(id).unwrap(), id.to_string());
    assert_eq!(
        serde_json::from_value::<Ulid>(id.to_string().into()).unwrap(),
        id
    );
    assert!(serde_json::from_str::<Ulid>(r#""invalid""#).is_err());
}

///
/// Scalars
/// Exercises endpoint serialization for database scalar values.
///

#[ic_db::record(fields(
    field(name = "date", value(item(prim = "Date"))),
    field(name = "decimal", value(item(prim = "Decimal", scale = 2))),
    field(name = "wide", value(item(prim = "U256"))),
    field(name = "timestamp", value(item(prim = "Timestamp"))),
    field(name = "duration", value(item(prim = "Duration"))),
    field(name = "single", value(item(prim = "Float32"))),
    field(name = "double", value(item(prim = "Float64")))
))]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Scalars {}

#[test]
fn schema_values_round_trip_and_validate_external_inputs() {
    let values = Scalars {
        date: Date::MIN,
        decimal: Decimal::MIN,
        wide: U256::MAX,
        timestamp: Timestamp::from_millis(i64::MIN),
        duration: Duration::from_millis(u64::MAX),
        single: Float32::try_new(-1.5).unwrap(),
        double: Float64::try_new(f64::MAX).unwrap(),
    };
    assert_eq!(decode::<Scalars>(&encode(&values)).unwrap(), values);
    let json = serde_json::to_value(&values).unwrap();
    assert_eq!(json["wide"], format!("{:#x}", U256::MAX));
    assert_eq!(json["decimal"], i128::MIN.to_string());
    assert_eq!(serde_json::from_value::<Scalars>(json).unwrap(), values);
    assert!(decode::<Date>(&encode(&i32::MAX)).is_err());
    assert!(decode::<Float32>(&encode(&f32::INFINITY)).is_err());
    assert!(decode::<Float64>(&encode(&f64::NAN)).is_err());
    assert_eq!(
        decode::<Float64>(&encode(&-0.0f64))
            .unwrap()
            .get()
            .to_bits(),
        0
    );
    assert!(serde_json::from_str::<Decimal>(r#""0.00000000000000000000000000001""#).is_err());
}

#[test]
fn wire_and_row_codecs_choose_their_own_timestamp_bytes() {
    ///
    /// Event
    /// Provides one timestamp for comparison across Wire and row encodings.
    ///

    #[ic_db::entity(
        pk(field = "id"),
        fields(
            field(name = "id", value(item(prim = "Nat8"))),
            field(name = "time", value(item(prim = "Timestamp")))
        )
    )]
    struct Event {}

    use ic_db::Entity;
    let event = Event {
        id: 7,
        time: Timestamp::from_millis(1),
    };
    let stored = [7, 1, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(encode(&event), [7, 2]); // Wire signed varint.
    assert_eq!(event.encode().unwrap(), stored);
    assert_eq!(decode::<Event>(&[7, 2]).unwrap(), event);
    assert_eq!(Event::decode(&stored).unwrap(), event);
}
