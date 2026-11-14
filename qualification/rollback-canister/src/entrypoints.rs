//! Standalone Platform hooks with bounded Wire codecs, without Canic.
//! Reply failure probes distinguish the current message from an earlier call.

use ic_platform_runtime::{
    call::{Call, WaitPolicy},
    cdk,
};
use ic_wire::ByteBuf;

fn write(id: u64) {
    super::mutate((0, id, id + 100)).unwrap();
}

fn reply(oversized: bool) -> ByteBuf {
    ByteBuf::from(vec![7; if oversized { 65_536 } else { 1 }])
}

#[cdk::update(
    guard = "crate::transport::admit::<8>",
    decode_with = "crate::transport::decode::<_, 8>",
    encode_with = "crate::transport::encode::<_, 8>"
)]
fn endpoint_sync(oversized: bool) -> ByteBuf {
    write(1);
    write(2);
    reply(oversized)
}

#[cdk::update(
    guard = "crate::transport::admit::<9>",
    decode_with = "crate::transport::decode::<_, 9>",
    encode_with = "crate::transport::encode::<_, 9>"
)]
fn endpoint_ack() -> u8 {
    1
}

#[cdk::update(
    guard = "crate::transport::admit::<10>",
    decode_with = "crate::transport::decode::<_, 10>",
    encode_with = "crate::transport::encode::<_, 10>"
)]
async fn endpoint_async(oversized: bool) -> ByteBuf {
    write(1);
    // An outgoing call ends the first execution; its writes survive callback traps.
    // Construct a request frame explicitly, not an empty unframed argument buffer.
    let limits = ic_wire::Limits {
        bytes: 64,
        allocation: 1024,
        nodes: 8,
        depth: 4,
    };
    let request = ic_wire::native::encode_request(9, &(), limits).unwrap();
    let response = Call::new(
        cdk::api::canister_self(),
        "endpoint_ack",
        WaitPolicy::Unbounded,
    )
    .with_raw_args(&request)
    .execute()
    .await
    .unwrap();
    assert_eq!(
        ic_wire::native::decode_response::<u8>(9, &response.into_bytes(), limits).unwrap(),
        1
    );
    write(2);
    reply(oversized)
}

fn deny() -> Result<(), String> {
    Err("M0 standalone guard refusal".to_owned())
}

#[cdk::update(
    guard = "deny",
    guard = "crate::transport::admit::<11>",
    decode_with = "crate::transport::decode::<_, 11>",
    encode_with = "crate::transport::encode::<_, 11>"
)]
fn endpoint_denied() -> u8 {
    write(1);
    1
}
