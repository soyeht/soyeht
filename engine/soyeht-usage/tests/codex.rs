#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;
use soyeht_protocol::{UsageProvider, UsageSource, UsageState, UsageUnavailableReason};
use soyeht_usage::{
    Planner, TransientFailure, Trigger,
    host::{HttpResult, Platform, RpcRun},
};
use std::sync::Arc;
use support::{Call, FakeHost};

const NOW: u64 = 1_790_000_000;
const USAGE: &str = "https://chatgpt.com/backend-api/wham/usage";
const WHOAMI: &str = "https://auth.openai.com/api/accounts/v1/user-auth-credential/whoami";

fn access_token(expires: u64) -> String {
    let claims = URL_SAFE_NO_PAD.encode(
        json!({"exp": expires, "https://api.openai.com/auth": {"chatgpt_account_id": "acct-fixture"}})
            .to_string(),
    );
    format!("eyJhbGciOiJSUzI1NiJ9.{claims}.signature")
}

fn login(expires: u64) -> Vec<u8> {
    json!({"tokens": {"access_token": access_token(expires), "refresh_token": "rt-unused", "id_token": "x.e30.y"}, "last_refresh": "2026-09-30T00:00:00Z"})
        .to_string()
        .into_bytes()
}

fn rate_limits() -> serde_json::Value {
    serde_json::from_slice(include_bytes!("fixtures/codex-app-server-rate-limits.json")).unwrap()
}

fn header<'a>(call: &'a Call, name: &str) -> Option<&'a str> {
    let Call::Http { headers, .. } = call else {
        return None;
    };
    headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

#[test]
fn chain_order_is_access_token_then_login_then_app_server() {
    let planner = Planner::new(Arc::new(FakeHost::new(Platform::Linux)));
    assert_eq!(
        planner.sources(&UsageProvider::Codex),
        vec![
            UsageSource::CodexAccessToken,
            UsageSource::CodexAccountEndpoint,
            UsageSource::CodexAppServer
        ]
    );
}

#[test]
fn login_file_reads_the_chatgpt_usage_endpoint_with_the_account_header() {
    let host = Arc::new(
        FakeHost::new(Platform::Linux)
            .file(".codex/auth.json", &login(NOW + 86_400))
            .respond(USAGE, 200, include_bytes!("fixtures/codex-wham-usage.json")),
    );
    let reading = Planner::new(host.clone())
        .refresh(&UsageProvider::Codex, Trigger::Automatic, NOW, "now")
        .reading;
    assert_eq!(reading.source, UsageSource::CodexAccountEndpoint);
    let primary = reading.primary.unwrap();
    assert_eq!(
        (primary.used, primary.label.as_str()),
        (0.08, "5-hour window")
    );
    assert_eq!(primary.resets_at, "2026-10-03T21:20:00Z");
    assert_eq!(reading.secondary.unwrap().used, 0.27);
    let calls = host.calls();
    assert_eq!(
        header(&calls[0], "ChatGPT-Account-Id"),
        Some("acct-fixture")
    );
    assert_eq!(header(&calls[0], "Accept"), Some("application/json"));
    assert_eq!(host.rpc_runs(), 0);
}

#[test]
fn personal_access_token_asks_whoami_first() {
    let host = Arc::new(
        FakeHost::new(Platform::MacOs)
            .file(
                ".codex/auth.json",
                br#"{"personal_access_token":"pat-fixture"}"#,
            )
            .respond(
                WHOAMI,
                200,
                br#"{"chatgpt_account_id":"acct-pat","email":"x"}"#,
            )
            .respond(USAGE, 200, include_bytes!("fixtures/codex-wham-usage.json")),
    );
    let reading = Planner::new(host.clone())
        .refresh(&UsageProvider::Codex, Trigger::Automatic, NOW, "now")
        .reading;
    assert_eq!(reading.source, UsageSource::CodexAccessToken);
    let calls = host.calls();
    assert_eq!(host.http_urls(), vec![WHOAMI.to_owned(), USAGE.to_owned()]);
    assert_eq!(header(&calls[0], "originator"), Some("codex_cli_rs"));
    assert_eq!(header(&calls[1], "ChatGPT-Account-Id"), Some("acct-pat"));
}

#[test]
fn a_login_codex_must_refresh_goes_to_the_app_server() {
    let (host, reading) = app_server_reading(vec![
        json!({}),
        json!({"account": null, "requiresOpenaiAuth": true}),
        rate_limits(),
    ]);
    assert_eq!(reading.source, UsageSource::CodexAppServer);
    assert_eq!(reading.primary.unwrap().used, 0.08);
    assert!(host.http_urls().is_empty(), "an expiring token was sent");
    let Some(Call::Rpc {
        program,
        args,
        methods,
        ..
    }) = host.calls().pop()
    else {
        panic!("no app server");
    };
    assert_eq!(program, "codex");
    assert_eq!(args, ["-s", "read-only", "-a", "never", "app-server"]);
    assert_eq!(
        methods,
        [
            "initialize",
            "initialized",
            "account/read",
            "account/rateLimits/read"
        ]
    );
}

/// The Codex reading after an expiring login sent the planner to the app server, which answers
/// with `results`; the host, to see what was asked.
fn app_server_reading(results: Vec<serde_json::Value>) -> (Arc<FakeHost>, soyeht_usage::Reading) {
    let host = FakeHost::new(Platform::Linux).file(".codex/auth.json", &login(NOW + 60));
    host.rpc.lock().unwrap().push_back(RpcRun::Results(results));
    let host = Arc::new(host);
    let reading = Planner::new(host.clone())
        .refresh(&UsageProvider::Codex, Trigger::Automatic, NOW, "now")
        .reading;
    (host, reading)
}

/// The limits are asked for only after the app server was told to refresh its login, since a
/// token the usage endpoint refused is renewed by the refresh and not by the read.
#[test]
fn the_app_server_refreshes_its_login_before_reading_the_limits() {
    let (host, _) = app_server_reading(vec![json!({}), json!({}), rate_limits()]);
    let calls = host.rpc_calls();
    assert_eq!(calls[2].method, "account/read");
    assert_eq!(calls[2].params, json!({"refreshToken": true}));
    assert_eq!(calls[3].method, "account/rateLimits/read");
    assert!(calls[2].id < calls[3].id, "the refresh is answered first");
}

/// A token still refused after the refresh (`null` is how an error answer reaches the source)
/// gives no limits: the reading says the app server failed.
#[test]
fn an_expired_token_answer_to_the_limits_is_an_app_server_failure() {
    let null = serde_json::Value::Null;
    let (_, reading) = app_server_reading(vec![json!({}), null.clone(), null]);
    assert_eq!(reading.state, UsageState::Unavailable);
    assert_eq!(
        reading.unavailable_reason,
        Some(UsageUnavailableReason::AppServerFailed)
    );
}

/// A refused refresh does not stop the limits from being read.
#[test]
fn a_refused_refresh_still_reads_the_limits() {
    let (_, reading) = app_server_reading(vec![json!({}), serde_json::Value::Null, rate_limits()]);
    assert_eq!(reading.state, UsageState::Ok);
    assert_eq!(reading.source, UsageSource::CodexAppServer);
}

#[test]
fn a_network_error_does_not_start_an_app_server() {
    let host = Arc::new(
        FakeHost::new(Platform::Linux)
            .file(".codex/auth.json", &login(NOW + 86_400))
            .respond_with(USAGE, HttpResult::TransportFailed),
    );
    let outcome =
        Planner::new(host.clone()).refresh(&UsageProvider::Codex, Trigger::Automatic, NOW, "now");
    assert_eq!(outcome.reading.state, UsageState::Unavailable);
    assert_eq!(
        outcome.reading.unavailable_reason,
        Some(UsageUnavailableReason::NetworkError)
    );
    assert_eq!(outcome.transient, Some(TransientFailure::Network));
    assert_eq!(host.rpc_runs(), 0);
}

#[test]
fn a_failed_app_server_is_not_restarted_by_the_background_for_thirty_minutes() {
    let host = FakeHost::new(Platform::Linux);
    host.rpc
        .lock()
        .unwrap()
        .extend([RpcRun::Failed, RpcRun::Failed]);
    let host = Arc::new(host);
    let planner = Planner::new(host.clone());
    let first = planner.refresh(&UsageProvider::Codex, Trigger::Automatic, NOW, "a");
    assert_eq!(
        first.reading.unavailable_reason,
        Some(UsageUnavailableReason::AppServerFailed)
    );
    planner.refresh(&UsageProvider::Codex, Trigger::Automatic, NOW + 1_799, "b");
    assert_eq!(host.rpc_runs(), 1);
    planner.refresh(&UsageProvider::Codex, Trigger::Manual, NOW + 1_800, "c");
    assert_eq!(host.rpc_runs(), 2);
}
