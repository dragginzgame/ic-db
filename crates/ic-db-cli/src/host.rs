//! Local probes borrow an explicitly selected server; the launcher owns its process.

use canic::wire::Limits;
use ic_testkit::pocket_ic::{self, PocketIcBuilder, nonblocking::PocketIc};
use std::time::Duration;

/// Upgrade a local fixture only after the IC has completed its stop request.
/// Platform's suspended futures are heap-local; they are never migrated.
/// Restart also follows a refused atomic upgrade so callers can inspect the
/// preserved predecessor. PocketIC owns each management operation.
/// # Errors
/// Returns stop/start or upgrade refusal without retrying the upgrade.
pub async fn upgrade_stopped(
    pic: &PocketIc,
    canister: pocket_ic::CanisterId,
    wasm: Vec<u8>,
    args: Vec<u8>,
    sender: Option<pocket_ic::CanisterId>,
) -> Result<(), pocket_ic::RejectResponse> {
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

/// Create an instance on the explicitly supplied external server.
/// Never discovers/downloads a binary, starts a server or creates a Tokio runtime.
/// Call the upstream async `drop` explicitly to attempt deletion. The external
/// launcher owns shutdown, including after a test panic; deletion is best-effort.
///
/// # Errors
/// Refuses absent/invalid configuration or creation timeout. Timeout/cancellation
/// can leave an instance on the external server; no successful cleanup is implied.
/// # Panics
/// Requires the caller's Tokio runtime; upstream construction panics propagate.
pub async fn instance() -> Result<PocketIc, String> {
    let endpoint = std::env::var("IC_TESTKIT_POCKET_IC_URL").map_err(|_| {
        "run through scripts/with-pocketic.sh or supply IC_TESTKIT_POCKET_IC_URL".to_owned()
    })?;
    let url = endpoint
        .parse()
        .map_err(|_| "invalid PocketIC endpoint".to_owned())?;

    tokio::time::timeout(
        Duration::from_secs(30),
        PocketIcBuilder::new()
            .with_application_subnet()
            .with_server_url(url)
            .with_max_request_time_ms(Some(30_000))
            .build_async(),
    )
    .await
    .map_err(|_| {
        "PocketIC instance creation timed out; remote creation may have occurred".to_owned()
    })
}
