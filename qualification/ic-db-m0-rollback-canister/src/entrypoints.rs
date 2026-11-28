//! Standalone Platform hooks with bounded Wire codecs, without Canic.
//! Reply failure probes distinguish the current message from an earlier call.

use crate::transport::fixture_wire;
use ic_db_qualification_transport::fixture_wire::Limits;
use ic_db_qualification_transport::fixture_wire::{ByteBuf, Direction, Encoding};
use ic_platform_runtime::call::{Call, Outcome, Wait};

// endpoint_sync
// Exercise synchronous writes with a normal or oversized reply.
// guard  : validates the request frame for method 8.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "endpoint_sync",
    guard = crate::transport::admit::<8>,
    decode = crate::transport::decode::<_, 8>,
    encode = crate::transport::encode::<_, 8>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn endpoint_sync(oversized: bool) -> ByteBuf {
    write(1);
    write(2);

    reply(oversized)
}

// endpoint_ack
// Acknowledge the self-call that separates two message executions.
// guard  : validates the request frame for method 9.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "endpoint_ack",
    guard = crate::transport::admit::<9>,
    decode = crate::transport::decode::<_, 9>,
    encode = crate::transport::encode::<_, 9>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn endpoint_ack((): ()) -> u8 {
    1
}

// endpoint_async
// Write on each side of a self-call to test rollback at reply time.
// guard  : validates the request frame for method 10.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "endpoint_async",
    guard = crate::transport::admit::<10>,
    decode = crate::transport::decode::<_, 10>,
    encode = crate::transport::encode::<_, 10>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
async fn endpoint_async(oversized: bool) -> ByteBuf {
    write(1);
    // An outgoing call ends the first execution; its writes survive callback traps.
    // Construct a request frame explicitly, not an empty unframed argument buffer.
    let limits = Limits {
        bytes: 64,
        allocation: 1024,
        nodes: 32,
        depth: 4,
    };
    let request = fixture_wire::encode(
        &(),
        Encoding::Frame {
            method: 9,
            direction: Direction::Request,
        },
        limits,
    )
    .unwrap();
    let response = Call::new(
        ic_platform_runtime::system::canister_self(),
        "endpoint_ack",
        Wait::Unbounded,
    )
    .args(&request)
    .execute()
    .await
    .unwrap();
    assert_eq!(
        fixture_wire::decode::<u8>(
            match response.outcome() {
                Outcome::Reply(bytes) => bytes,
                other => panic!("unexpected callback: {other:?}"),
            },
            Encoding::Frame {
                method: 9,
                direction: Direction::Response
            },
            limits
        )
        .unwrap(),
        1
    );
    write(2);

    reply(oversized)
}

// endpoint_denied
// Exercise rejection before the guarded handler runs.
// guard  : denies every request before decoding.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "endpoint_denied",
    guard = deny,
    decode = crate::transport::decode::<_, 11>,
    encode = crate::transport::encode::<_, 11>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn endpoint_denied((): ()) -> u8 {
    write(1);

    1
}

fn write(id: u64) {
    super::mutate((0, id, id + 100)).unwrap();
}

fn reply(oversized: bool) -> ByteBuf {
    ByteBuf::from(vec![7; if oversized { 65_536 } else { 1 }])
}

/// Reject every caller before the denial probe can mutate state.
fn deny() -> Result<(), String> {
    Err("M0 standalone guard refusal".to_owned())
}
