//! Bounded Wire update calls shared by installed qualification tests.

use ic_db_m0_rollback_driver::LIMITS;
use ic_testkit::pocket_ic::{CanisterId, nonblocking::PocketIc};
use ic_wire::{Direction, Encoding};
use serde::{Serialize, de::DeserializeOwned};

pub(super) async fn call<T: DeserializeOwned>(
    pic: &PocketIc,
    canister: CanisterId,
    name: &str,
    method: u64,
    value: &impl Serialize,
) -> T {
    let bytes = ic_wire::encode(
        value,
        Encoding::Frame {
            method,
            direction: Direction::Request,
        },
        LIMITS,
    )
    .unwrap();
    let reply = pic
        .update_call(canister, canister, name, bytes)
        .await
        .unwrap();

    ic_wire::decode(
        &reply,
        Encoding::Frame {
            method,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap()
}
