#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use soyeht_protocol::{UsageProvider, UsageSource, UsageState};
use soyeht_usage::{
    Planner, Trigger,
    host::{Cookie, CookieJar, Platform, Secret, SecretRead},
};
use std::{path::Path, sync::Arc};
use support::{Call, FakeHost, HOME};

const NOW: u64 = 1_790_000_000;
const SUMMARY: &str = "https://cursor.com/api/usage-summary";

fn app_token() -> String {
    let claims = URL_SAFE_NO_PAD.encode(br#"{"sub":"auth0|user_fixture","exp":1890000000}"#);
    format!("eyJhbGciOiJIUzI1NiJ9.{claims}.signature")
}

fn summary() -> &'static [u8] {
    include_bytes!("fixtures/cursor-usage-summary.json")
}

fn secret_header(host: &FakeHost) -> Vec<String> {
    host.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Http { secret_values, .. } => secret_values.first().cloned(),
            _ => None,
        })
        .collect()
}

#[test]
fn app_login_is_sent_as_the_session_cookie() {
    let mut host = FakeHost::new(Platform::MacOs).respond(SUMMARY, 200, summary());
    host.state_db = SecretRead::Found(Secret::new(app_token()));
    let host = Arc::new(host);
    let reading = Planner::new(host.clone())
        .refresh(&UsageProvider::Cursor, Trigger::Automatic, NOW, "now")
        .reading;
    assert_eq!(reading.source, UsageSource::CursorAppLogin);
    let primary = reading.primary.unwrap();
    assert_eq!(primary.used, 0.62);
    assert_eq!(primary.resets_at, "2026-10-20T00:00:00Z");
    assert_eq!(
        secret_header(&host),
        vec![format!(
            "WorkosCursorSessionToken=user_fixture%3A%3A{}",
            app_token()
        )]
    );
    assert_eq!(
        host.calls()[0],
        Call::StateDb(
            Path::new(HOME)
                .join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
        )
    );
}

#[test]
fn rejected_app_login_falls_back_to_browser_cookies_on_macos() {
    let mut host = FakeHost::new(Platform::MacOs)
        .respond(SUMMARY, 401, b"{}")
        .respond(SUMMARY, 200, summary());
    host.state_db = SecretRead::Found(Secret::new(app_token()));
    host.cookies = vec![(
        "cursor.com".into(),
        CookieJar {
            cookies: vec![Cookie {
                name: "WorkosCursorSessionToken".into(),
                value: Secret::new("browser-session"),
            }],
        },
    )];
    let host = Arc::new(host);
    let reading = Planner::new(host.clone())
        .refresh(&UsageProvider::Cursor, Trigger::Automatic, NOW, "now")
        .reading;
    assert_eq!(reading.source, UsageSource::CursorBrowserCookies);
    assert_eq!(
        secret_header(&host).last().unwrap(),
        "WorkosCursorSessionToken=browser-session"
    );
}

#[test]
fn linux_uses_the_xdg_app_login_and_never_a_browser() {
    let mut host = FakeHost::new(Platform::Linux);
    let config = std::env::temp_dir().join("xdg");
    host.env = vec![("XDG_CONFIG_HOME".into(), config.to_str().unwrap().into())];
    let host = Arc::new(host);
    let reading = Planner::new(host.clone())
        .refresh(&UsageProvider::Cursor, Trigger::Automatic, NOW, "now")
        .reading;
    assert_eq!(reading.state, UsageState::LoginNeeded);
    assert_eq!(
        host.calls(),
        vec![Call::StateDb(
            config
                .join("Cursor")
                .join("User")
                .join("globalStorage")
                .join("state.vscdb")
        )]
    );
}
