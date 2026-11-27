//! Bounded request and response frames for installed fixture calls.

use ic_db_m0_rollback_driver::LIMITS;
use ic_wire::{Direction, Encoding};
use serde::{Serialize, de::DeserializeOwned};

pub(super) fn request<T: Serialize>(method: u64, value: &T) -> Vec<u8> {
    ic_wire::encode(
        value,
        Encoding::Frame {
            method,
            direction: Direction::Request,
        },
        LIMITS,
    )
    .unwrap()
}

pub(super) fn response<T: DeserializeOwned>(method: u64, bytes: &[u8]) -> T {
    ic_wire::decode(
        bytes,
        Encoding::Frame {
            method,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .unwrap()
}
