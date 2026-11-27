//! Bounded Wire update calls shared by installed qualification tests.

#[path = "../wire_frame/mod.rs"]
mod wire_frame;
use ic_testkit::pocket_ic::{CanisterId, nonblocking::PocketIc};
use serde::{Serialize, de::DeserializeOwned};

pub(super) async fn call<T: DeserializeOwned>(
    pic: &PocketIc,
    canister: CanisterId,
    name: &str,
    method: u64,
    value: &impl Serialize,
) -> T {
    let bytes = wire_frame::request(method, value);
    let reply = pic
        .update_call(canister, canister, name, bytes)
        .await
        .unwrap();

    wire_frame::response(method, &reply)
}
