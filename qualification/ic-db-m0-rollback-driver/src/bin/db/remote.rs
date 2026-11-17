//! Shared native target, identity and verified query calls; no database semantics.
use clap::Args;
use ic_agent::{
    Agent, Identity,
    export::{Principal, reqwest},
    identity::{AnonymousIdentity, BasicIdentity, Secp256k1Identity},
};
use ic_host_text::{bordered_table, escape};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
#[derive(Args)]
pub(super) struct Target {
    /// Explicit HTTP(S) replica or boundary-node URL; never falls back to a fixture.
    #[arg(long)]
    provider: String,
    /// Explicit application canister principal.
    #[arg(long)]
    canister: String,
    /// Explicit anonymous caller (the application may refuse it).
    #[arg(
        long,
        required_unless_present = "identity",
        conflicts_with = "identity"
    )]
    anonymous: bool,
    /// Ed25519 or secp256k1 PEM identity; never falls back to anonymous.
    #[arg(long, value_name = "PEM")]
    identity: Option<PathBuf>,
    /// Trust the root key fetched from an explicitly selected loopback provider.
    #[arg(long)]
    local_root_key: bool,
}
pub(super) fn provider(value: &str, local_root_key: bool) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value).map_err(|_| "invalid provider URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("provider requires HTTP(S), without credentials, query or fragment".into());
    }
    let host = url.host_str().unwrap_or_default().trim_matches(['[', ']']);
    let local = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if local_root_key && !local {
        return Err("--local-root-key requires a loopback provider".into());
    }
    Ok(url)
}

pub(super) struct Connection {
    agent: Agent,
    canister: Principal,
    caller: Principal,
    provider: String,
}
impl Connection {
    pub(super) async fn connect(target: &Target, label: &str, json: bool) -> Result<Self, String> {
        let url = provider(&target.provider, target.local_root_key)?;
        let canister =
            Principal::from_text(&target.canister).map_err(|_| "invalid canister principal")?;
        let identity: Box<dyn Identity> = if let Some(path) = &target.identity {
            let pem = std::fs::read(path).map_err(|e| format!("cannot read identity: {e}"))?;
            if let Ok(identity) = BasicIdentity::from_pem(&pem) {
                Box::new(identity)
            } else {
                Box::new(
                    Secp256k1Identity::from_pem(&pem)
                        .map_err(|_| "identity must be an Ed25519 or secp256k1 PEM")?,
                )
            }
        } else if target.anonymous {
            Box::new(AnonymousIdentity)
        } else {
            return Err("select --anonymous or --identity explicitly".into());
        };
        let caller = identity
            .sender()
            .map_err(|e| format!("cannot derive caller: {e}"))?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        let agent = Agent::builder()
            .with_url(url.as_str())
            .with_boxed_identity(identity)
            .with_http_client(client)
            .with_max_tcp_error_retries(0)
            .with_max_response_body_size(2 * 1024 * 1024)
            .build()
            .map_err(|e| e.to_string())?;
        if !json {
            println!(
                "IC DB — {label}\n{}",
                bordered_table(
                    &["Target", "Value"],
                    &[
                        vec!["Provider".into(), escape(url.as_str()).into_owned()],
                        vec!["Canister".into(), canister.to_text()],
                        vec!["Caller".into(), caller.to_text()],
                    ]
                )
            );
        }
        if target.local_root_key {
            agent
                .fetch_root_key()
                .await
                .map_err(|e| format!("cannot fetch local root key: {e}"))?;
        }

        Ok(Self {
            agent,
            canister,
            caller,
            provider: url.to_string(),
        })
    }
    pub(super) async fn query(
        &self,
        method: &str,
        payload: Vec<u8>,
        statistics: &mut Vec<String>,
    ) -> Result<Vec<u8>, String> {
        let start = Instant::now();
        let result = self
            .agent
            .query(&self.canister, method)
            .with_arg(payload)
            .call()
            .await;
        statistics.push(format!(
            "query call ({method}): {:.2} sec, cycles unavailable",
            start.elapsed().as_secs_f64()
        ));
        result.map_err(|e| format!("{method} query failed: {}", escape(&e.to_string())))
    }
    pub(super) fn envelope(&self, key: &str, value: serde_json::Value) -> serde_json::Value {
        serde_json::Value::Object(serde_json::Map::from_iter([
            ("provider".into(), self.provider.clone().into()),
            ("canister".into(), self.canister.to_text().into()),
            ("caller".into(), self.caller.to_text().into()),
            (key.into(), value),
        ]))
    }
}
pub(super) fn report(statistics: Vec<String>, stderr: bool) {
    for line in statistics {
        if stderr {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    }
}
