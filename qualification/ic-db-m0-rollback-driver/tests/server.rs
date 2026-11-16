//! Endpoint admission runs in isolated processes without altering shared test env.
use ic_db_m0_rollback_driver::instance;
use std::process::Command;

#[tokio::test]
async fn explicitly_releases_borrowed_instance() {
    let owned = instance().await.unwrap();
    let canister = owned.create_canister().await;
    assert!(owned.get_subnet(canister).await.is_some());
    owned.drop().await;
    // Independent server inventory is observed by the qualification command.
}

#[tokio::test]
async fn endpoint_admission_never_falls_back_to_a_binary() {
    if let Ok(expected) = std::env::var("IC_DB_ENDPOINT_CHILD") {
        let error = instance().await.err().expect("endpoint must refuse");
        assert!(error.contains(&expected), "{error}");
        return;
    }
    for endpoint in [None, Some("not a URL")] {
        let expected = if endpoint.is_some() {
            "invalid PocketIC endpoint"
        } else {
            "IC_TESTKIT_POCKET_IC_URL"
        };
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", "endpoint_admission_never_falls_back_to_a_binary"])
            .env("IC_DB_ENDPOINT_CHILD", expected)
            .env("POCKET_IC_BIN", "/unavailable/no-implicit-server")
            .env_remove("IC_TESTKIT_POCKET_IC_URL");
        if let Some(endpoint) = endpoint {
            child.env("IC_TESTKIT_POCKET_IC_URL", endpoint);
        }
        let result = child.output().unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
