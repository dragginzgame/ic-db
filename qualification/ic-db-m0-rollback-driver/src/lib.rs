#![forbid(unsafe_code)]

//! Local M0 harness shared by executable probes and the development console.
//! Testkit owns server provisioning and lifecycle.

use ic_testkit::{
    pic::{PocketIcInstance, PocketIcStartupConfig, server_assets},
    pocket_ic::PocketIcBuilder,
};
use ic_wire::Limits;
use std::{path::PathBuf, time::Duration};

/// Upgrade a local fixture only after the IC has completed its stop request.
/// Platform's suspended futures are heap-local; they are never migrated.
/// Restart also follows a refused atomic upgrade so callers can inspect the
/// preserved predecessor. Testkit/PocketIC owns each management operation.
/// # Errors
/// Returns stop/start or upgrade refusal without retrying the upgrade.
pub async fn upgrade_stopped(
    pic: &ic_testkit::pocket_ic::nonblocking::PocketIc,
    canister: ic_testkit::pocket_ic::CanisterId,
    wasm: Vec<u8>,
    args: Vec<u8>,
    sender: Option<ic_testkit::pocket_ic::CanisterId>,
) -> Result<(), ic_testkit::pocket_ic::RejectResponse> {
    pic.stop_canister(canister, sender).await?;
    let result = pic.upgrade_canister(canister, wasm, args, sender).await;
    pic.start_canister(canister, sender).await?;
    result
}

/// Transport bounds for the fixed probe controls.
pub const LIMITS: Limits = Limits {
    bytes: 65_536,
    allocation: 8 * 1024 * 1024,
    nodes: 65_536,
    depth: 32,
};

/// The explicitly prepared server selection for this checkout.
///
/// # Panics
/// Panics if the compiled package path does not have its workspace ancestors.
#[must_use]
pub fn server_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".tools/m0-pocketic")
}

/// Open a local instance with the already prepared, authenticated server.
///
/// # Errors
/// Returns setup or startup failures without provisioning a server implicitly.
pub async fn instance() -> Result<PocketIcInstance, String> {
    PocketIcStartupConfig::Spawn {
        server_binary: server_assets::check(&server_directory())
            .map_err(|error| format!("run make m0-setup: {error}"))?,
        timeout: Duration::from_secs(30),
        server_hard_ttl: None,
        server_idle_ttl: None,
        server_output_files: None,
    }
    .build_instance(PocketIcBuilder::new().with_application_subnet())
    .await
    .map_err(|error| error.to_string())
}
