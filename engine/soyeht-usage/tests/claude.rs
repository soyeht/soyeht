#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use soyeht_protocol::{UsageProvider, UsageSource, UsageState, UsageUnavailableReason};
use soyeht_usage::{
    Planner, RefreshOutcome, Trigger,
    host::{Cookie, CookieJar, HttpResult, Platform, Secret, SecretRead},
};
use std::{path::Path, sync::Arc};
use support::{Call, FakeHost, HOME, TEMP};

const NOW: u64 = 1_790_000_000;
const USAGE: &str = "https://api.anthropic.com/api/oauth/usage";
const ORGANIZATIONS: &str = "https://claude.ai/api/organizations";
const TOKEN: &str = "sk-ant-oat01-recorded-fixture-token";

fn login(expires_ms: u64) -> Vec<u8> {
    format!(
        r#"{{"claudeAiOauth":{{"accessToken":"{TOKEN}","refreshToken":"sk-ant-ort01-unused","expiresAt":{expires_ms},"scopes":["user:inference","user:profile"],"subscriptionType":"max"}}}}"#
    )
    .into_bytes()
}

fn valid_login() -> Vec<u8> {
    login((NOW + 3_600) * 1000)
}

fn usage_body() -> &'static [u8] {
    include_bytes!("fixtures/claude-oauth-usage.json")
}

/// A real prompt of Claude Code, and the same screen with the command typed.
const REPL: &str = include_str!("fixtures/claude-cli-repl.txt");

fn typed() -> String {
    REPL.replace("Try \"create a util logging.py that...\"", "/usage")
}

/// The screens of a session that reaches the panel: the prompt, the typed command, the panel.
fn screens_ending_in(last: &str) -> Vec<String> {
    vec![
        REPL.into(),
        REPL.into(),
        REPL.into(),
        REPL.into(),
        REPL.into(),
        typed(),
        last.into(),
    ]
}

fn panel() -> Vec<String> {
    screens_ending_in(include_str!("fixtures/claude-cli-usage.txt"))
}

fn refresh(host: &Arc<FakeHost>, trigger: Trigger, now: u64) -> RefreshOutcome {
    Planner::new(host.clone()).refresh(&UsageProvider::Claude, trigger, now, "fetched")
}

#[test]
fn chain_order_is_oauth_then_cli_then_web() {
    let planner = Planner::new(Arc::new(FakeHost::new(Platform::MacOs)));
    assert_eq!(
        planner.sources(&UsageProvider::Claude),
        vec![
            UsageSource::ClaudeOauthEndpoint,
            UsageSource::ClaudeCliUsage,
            UsageSource::ClaudeWeb
        ]
    );
}

#[test]
fn oauth_reads_the_credentials_file_with_codexbar_headers() {
    let host = Arc::new(
        FakeHost::new(Platform::MacOs)
            .file(".claude/.credentials.json", &valid_login())
            .respond(USAGE, 200, usage_body()),
    );
    let outcome = refresh(&host, Trigger::Automatic, NOW);
    let reading = outcome.reading;
    assert_eq!(reading.state, UsageState::Ok);
    assert_eq!(reading.source, UsageSource::ClaudeOauthEndpoint);
    assert_eq!(reading.primary.as_ref().unwrap().used, 0.23);
    assert_eq!(
        reading.primary.as_ref().unwrap().resets_at,
        "2026-10-03T21:00:00Z"
    );
    assert_eq!(reading.secondary.as_ref().unwrap().used, 0.41);
    let calls = host.calls();
    assert_eq!(calls.len(), 1, "a file login needs no keychain prompt");
    let Call::Http {
        headers,
        secret_names,
        secret_values,
        ..
    } = &calls[0]
    else {
        panic!("expected one request");
    };
    let header = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    assert_eq!(header("anthropic-beta"), Some("oauth-2025-04-20"));
    assert_eq!(header("User-Agent"), Some("claude-code/2.1.0"));
    assert_eq!(header("Accept"), Some("application/json"));
    assert_eq!(secret_names, &vec!["Authorization".to_owned()]);
    assert_eq!(secret_values, &vec![format!("Bearer {TOKEN}")]);
    assert!(headers.iter().all(|(_, value)| !value.contains(TOKEN)));
    assert!(!format!("{reading:?}").contains(TOKEN));
}

#[test]
fn macos_keychain_is_read_when_the_file_is_absent_or_expired() {
    for file in [None, Some(login(NOW * 1000 - 1))] {
        let mut host = FakeHost::new(Platform::MacOs).respond(USAGE, 200, usage_body());
        if let Some(file) = &file {
            host = host.file(".claude/.credentials.json", file);
        }
        host.keychain = SecretRead::Found(Secret::new(String::from_utf8(valid_login()).unwrap()));
        let host = Arc::new(host);
        let outcome = refresh(&host, Trigger::Automatic, NOW);
        assert_eq!(outcome.reading.source, UsageSource::ClaudeOauthEndpoint);
        assert_eq!(
            host.calls()[0],
            Call::Keychain("Claude Code-credentials".into())
        );
    }
}

#[test]
fn rejected_oauth_falls_back_to_a_hidden_cli_session_that_leaves_nothing_behind() {
    let mut host = FakeHost::new(Platform::MacOs)
        .file(".claude/.credentials.json", &valid_login())
        .respond(USAGE, 401, b"{}");
    host.screens = Some(panel());
    let host = Arc::new(host);
    let reading = refresh(&host, Trigger::Automatic, NOW).reading;
    assert_eq!(reading.source, UsageSource::ClaudeCliUsage);
    assert_eq!(reading.primary.as_ref().unwrap().used, 0.19);
    assert_eq!(reading.secondary.as_ref().unwrap().used, 0.39);
    assert!(!reading.secondary.as_ref().unwrap().resets_at.is_empty());
    let terminal = host
        .calls()
        .into_iter()
        .find_map(|call| match call {
            Call::Terminal {
                program,
                args,
                workdir,
                transcript,
                sent,
            } => Some((program, args, workdir, transcript, sent)),
            _ => None,
        })
        .unwrap();
    let (program, args, workdir, transcript, sent) = terminal;
    assert_eq!(program, "claude");
    assert!(args.contains(&"--strict-mcp-config".to_owned()));
    assert!(args.contains(&"--allowed-tools".to_owned()));
    assert_eq!(workdir, Path::new(TEMP).join("soyeht-usage-claude"));
    let session = &args[args.iter().position(|arg| arg == "--session-id").unwrap() + 1];
    assert_eq!(
        transcript.unwrap(),
        Path::new(HOME)
            .join(".claude/projects/-tmp-scratch-soyeht-usage-claude")
            .join(format!("{session}.jsonl"))
    );
    assert_eq!(sent.first().unwrap(), b"/usage");
}

#[test]
fn web_session_is_the_last_macos_fallback() {
    let mut host = FakeHost::new(Platform::MacOs)
        .respond(
            ORGANIZATIONS,
            200,
            include_bytes!("fixtures/claude-web-organizations.json"),
        )
        .respond(
            &format!("{ORGANIZATIONS}/0f0e0d0c-0000-4000-8000-00000000c002/usage?cedar_ember=1"),
            200,
            usage_body(),
        );
    host.cookies = vec![(
        "claude.ai".into(),
        CookieJar {
            cookies: vec![Cookie {
                name: "sessionKey".into(),
                value: Secret::new("sk-ant-sid01-recorded-cookie"),
            }],
        },
    )];
    let host = Arc::new(host);
    let reading = refresh(&host, Trigger::Automatic, NOW).reading;
    assert_eq!(reading.source, UsageSource::ClaudeWeb);
    assert_eq!(reading.primary.unwrap().used, 0.23);
    let cookie_header = host
        .calls()
        .into_iter()
        .find_map(|call| match call {
            Call::Http { secret_values, .. } => secret_values.first().cloned(),
            _ => None,
        })
        .unwrap();
    assert_eq!(cookie_header, "sessionKey=sk-ant-sid01-recorded-cookie");
}

/// What a signed-out Claude Code shows: a menu whose next key opens the sign-in page.
const LOGIN_MENU: &str = "Welcome to Claude Code\n\n Select login method:\n\n ❯ 1. Claude account with subscription · Pro, Max, Team, or Enterprise\n   2. Anthropic Console account · API usage billing\n";

/// No terminal run of the host received a key.
fn no_key_sent(host: &FakeHost) -> bool {
    host.calls().iter().all(|call| match call {
        Call::Terminal { sent, .. } => sent.is_empty(),
        _ => true,
    })
}

#[test]
fn without_a_stored_login_no_session_starts_and_the_ring_says_sign_in() {
    for platform in [Platform::Linux, Platform::Windows, Platform::MacOs] {
        let mut host = FakeHost::new(platform);
        host.screens = Some(vec![LOGIN_MENU.into()]);
        let host = Arc::new(host);
        let outcome = refresh(&host, Trigger::Automatic, NOW);
        assert_eq!(
            outcome.reading.state,
            UsageState::LoginNeeded,
            "{platform:?}"
        );
        assert_eq!(outcome.reading.source, UsageSource::ClaudeCliUsage);
        assert_eq!(host.terminal_runs(), 0, "{platform:?} started Claude Code");
        assert!(
            host.calls()
                .iter()
                .all(|call| !matches!(call, Call::Terminal { .. } | Call::Http { .. })),
            "{platform:?}: {:?}",
            host.calls()
        );
    }
}

#[test]
fn a_login_file_without_a_token_is_not_a_login() {
    for file in [
        &b"{}"[..],
        br#"{"claudeAiOauth":{"accessToken":" "}}"#,
        b"not json",
    ] {
        let mut host = FakeHost::new(Platform::Windows).file(".claude/.credentials.json", file);
        host.screens = Some(vec![LOGIN_MENU.into()]);
        let host = Arc::new(host);
        let outcome = refresh(&host, Trigger::Automatic, NOW);
        assert_eq!(outcome.reading.state, UsageState::LoginNeeded);
        assert_eq!(host.terminal_runs(), 0);
    }
}

#[test]
fn macos_without_a_keychain_login_or_with_it_declined_starts_no_session() {
    for (keychain, state) in [
        (SecretRead::Missing, UsageState::LoginNeeded),
        (SecretRead::Denied, UsageState::NoAccess),
    ] {
        let mut host = FakeHost::new(Platform::MacOs);
        host.keychain = keychain;
        host.screens = Some(vec![LOGIN_MENU.into()]);
        let host = Arc::new(host);
        let outcome = refresh(&host, Trigger::Automatic, NOW);
        assert_eq!(outcome.reading.state, state);
        assert_eq!(host.terminal_runs(), 0);
    }
}

#[test]
fn an_expired_login_still_starts_the_session_because_claude_code_refreshes_it() {
    // The endpoint is unreachable too, so the chain goes on to the session.
    let host = Arc::new(FakeHost {
        screens: Some(panel()),
        ..FakeHost::new(Platform::Linux).file(".claude/.credentials.json", &login(NOW * 1000 - 1))
    });
    let outcome = refresh(&host, Trigger::Automatic, NOW);
    assert_eq!(outcome.reading.source, UsageSource::ClaudeCliUsage);
    assert_eq!(host.terminal_runs(), 1);
}

#[test]
fn a_session_that_meets_the_login_menu_presses_no_key_and_is_not_started_again_soon() {
    let mut host = FakeHost::new(Platform::Windows)
        .file(".claude/.credentials.json", &login(NOW * 1000 - 1))
        .respond(USAGE, 401, b"{}");
    host.screens = Some(vec![LOGIN_MENU.into()]);
    let host = Arc::new(host);
    let planner = Planner::new(host.clone());
    let first = planner.refresh(&UsageProvider::Claude, Trigger::Automatic, NOW, "a");
    assert_eq!(first.reading.state, UsageState::LoginNeeded);
    assert_eq!(first.reading.source, UsageSource::ClaudeCliUsage);
    let sent: Vec<Vec<Vec<u8>>> = host
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Terminal { sent, .. } => Some(sent),
            _ => None,
        })
        .collect();
    assert_eq!(sent, vec![Vec::<Vec<u8>>::new()], "a key reached the menu");
    // Inside the cooldown the session is not started, from the cooldown's own answer.
    for later in [60, 29 * 60] {
        let again = planner.refresh(&UsageProvider::Claude, Trigger::Automatic, NOW + later, "b");
        assert_eq!(again.reading.state, UsageState::LoginNeeded);
    }
    assert_eq!(host.terminal_runs(), 1, "the session came back too soon");
    planner.refresh(
        &UsageProvider::Claude,
        Trigger::Automatic,
        NOW + 30 * 60,
        "c",
    );
    assert_eq!(host.terminal_runs(), 2);
    // A click always tries again.
    planner.refresh(
        &UsageProvider::Claude,
        Trigger::Manual,
        NOW + 30 * 60 + 1,
        "d",
    );
    assert_eq!(host.terminal_runs(), 3);
}

#[test]
fn a_screen_with_new_wording_gets_no_key_and_the_read_fails_without_a_browser() {
    for screens in [
        vec![
            "Sign in to continue\n\n ❯ 1. Use your Claude subscription\n   2. Use an API key\n"
                .to_owned(),
        ],
        vec![
            String::new(),
            String::new(),
            "Sign in to continue\n ❯ 1. Use your Claude subscription\n".to_owned(),
        ],
    ] {
        let mut host = FakeHost::new(Platform::Windows)
            .file(".claude/.credentials.json", &login(NOW * 1000 - 1));
        host.screens = Some(screens);
        let host = Arc::new(host);
        let outcome = refresh(&host, Trigger::Manual, NOW);
        assert_eq!(outcome.reading.state, UsageState::Unavailable);
        assert_eq!(
            outcome.reading.unavailable_reason,
            Some(UsageUnavailableReason::SourceFailed)
        );
        assert!(no_key_sent(&host));
        assert_eq!(host.terminal_runs(), 1);
    }
}

#[test]
fn a_screen_that_stays_unknown_is_not_tried_again_by_the_background_for_the_longest_backoff() {
    let mut host =
        FakeHost::new(Platform::Windows).file(".claude/.credentials.json", &login(NOW * 1000 - 1));
    host.screens = Some(vec![
        "Teach auto mode about your environment?\n\n ❯ 1. Yes\n   2. No\n".to_owned(),
    ]);
    let host = Arc::new(host);
    let planner = Planner::new(host.clone());
    let at = |now: u64, trigger: Trigger| {
        planner.refresh(&UsageProvider::Claude, trigger, now, "t");
        host.terminal_runs()
    };
    assert_eq!(at(NOW, Trigger::Automatic), 1);
    // Every five minutes the schedule asks again; the session is not started.
    for later in [300, 600, 1799] {
        assert_eq!(at(NOW + later, Trigger::Automatic), 1, "{later} s");
    }
    assert_eq!(at(NOW + 1800, Trigger::Automatic), 2);
    // A click always tries.
    assert_eq!(at(NOW + 1801, Trigger::Manual), 3);
    assert!(no_key_sent(&host));
}

#[test]
fn the_keychain_is_asked_once_per_refresh_whatever_sources_need_it() {
    // An expired login in the keychain: the endpoint source reads it, and the session source
    // looks for a stored login in the same item.
    let mut host = FakeHost::new(Platform::MacOs);
    host.keychain = SecretRead::Found(Secret::new(
        String::from_utf8(login(NOW * 1000 - 1)).unwrap(),
    ));
    host.screens = Some(panel());
    let host = Arc::new(host);
    let outcome = refresh(&host, Trigger::Manual, NOW);
    assert_eq!(outcome.reading.source, UsageSource::ClaudeCliUsage);
    assert_eq!(host.terminal_runs(), 1);
    let asked = host
        .calls()
        .iter()
        .filter(|call| matches!(call, Call::Keychain(_)))
        .count();
    assert_eq!(asked, 1, "one refresh must show at most one consent prompt");
}

#[test]
fn first_run_setup_screens_wait_for_a_person_and_get_no_key() {
    let mut host = FakeHost::new(Platform::Linux).file(".claude/.credentials.json", &valid_login());
    host.screens = Some(vec!["Let's get started.\n\nChoose the text style that looks best with your terminal\n ❯ 1. Dark mode\n".into()]);
    let host = Arc::new(host);
    let reading = refresh(&host, Trigger::Manual, NOW).reading;
    assert_eq!(reading.state, UsageState::LoginNeeded);
    assert!(no_key_sent(&host));
}

#[test]
fn linux_reads_the_file_login_and_the_cli_but_no_keychain_or_browser() {
    let host =
        Arc::new(FakeHost::new(Platform::Linux).file(".claude/.credentials.json", &valid_login()));
    let outcome = refresh(&host, Trigger::Automatic, NOW);
    assert_eq!(outcome.reading.state, UsageState::Unavailable);
    assert_eq!(
        outcome.reading.unavailable_reason,
        Some(UsageUnavailableReason::ToolNotFound)
    );
    assert_eq!(outcome.reading.source, UsageSource::ClaudeCliUsage);
    assert!(
        host.calls()
            .iter()
            .all(|call| matches!(call, Call::Terminal { .. } | Call::Http { .. })),
        "Linux touched a keychain or browser: {:?}",
        host.calls()
    );
}

#[test]
fn rate_limit_cools_the_oauth_source_and_reuses_the_cli_reading_until_a_click() {
    let mut host = FakeHost::new(Platform::MacOs)
        .file(".claude/.credentials.json", &valid_login())
        .respond_with(
            USAGE,
            HttpResult::Response {
                status: 429,
                retry_after_sec: Some(120),
                body: Vec::new(),
            },
        )
        .respond(USAGE, 200, usage_body());
    host.screens = Some(panel());
    let host = Arc::new(host);
    let planner = Planner::new(host.clone());
    let first = planner.refresh(&UsageProvider::Claude, Trigger::Automatic, NOW, "a");
    assert_eq!(first.reading.source, UsageSource::ClaudeCliUsage);
    let second = planner.refresh(&UsageProvider::Claude, Trigger::Automatic, NOW + 60, "b");
    assert_eq!(second.reading.source, UsageSource::ClaudeCliUsage);
    assert_eq!(
        host.http_urls().len(),
        1,
        "the cooled source was asked again"
    );
    assert_eq!(
        host.terminal_runs(),
        1,
        "the CLI started again inside its reuse window"
    );
    let clicked = planner.refresh(&UsageProvider::Claude, Trigger::Manual, NOW + 61, "c");
    assert_eq!(clicked.reading.source, UsageSource::ClaudeOauthEndpoint);
    assert_eq!(host.http_urls().len(), 2);
}

#[test]
fn every_source_failing_reports_the_last_attempt_and_no_values() {
    let mut host = FakeHost::new(Platform::MacOs)
        .file(".claude/.credentials.json", &valid_login())
        .respond(USAGE, 401, b"{}");
    host.screens = Some(screens_ending_in("Invalid API key · Please run /login"));
    let host = Arc::new(host);
    let outcome = refresh(&host, Trigger::Manual, NOW);
    assert_eq!(outcome.reading.state, UsageState::LoginNeeded);
    assert_eq!(outcome.reading.source, UsageSource::ClaudeCliUsage);
    assert!(outcome.reading.primary.is_none());
}

#[test]
fn a_declined_browser_key_is_no_access_and_not_asked_again_for_six_hours() {
    let mut host = FakeHost::new(Platform::MacOs);
    host.cookies_denied = true;
    let host = Arc::new(host);
    let planner = Planner::new(host.clone());
    let first = planner.refresh(&UsageProvider::Claude, Trigger::Automatic, NOW, "a");
    assert_eq!(first.reading.state, UsageState::NoAccess);
    assert_eq!(first.reading.source, UsageSource::ClaudeWeb);
    let asked = |host: &FakeHost| {
        host.calls()
            .iter()
            .filter(|call| matches!(call, Call::Cookies(_)))
            .count()
    };
    planner.refresh(
        &UsageProvider::Claude,
        Trigger::Automatic,
        NOW + 6 * 3600 - 1,
        "b",
    );
    assert_eq!(asked(&host), 1);
    planner.refresh(
        &UsageProvider::Claude,
        Trigger::Automatic,
        NOW + 6 * 3600,
        "c",
    );
    assert_eq!(asked(&host), 2);
}

fn empty_answer_host(body: &[u8]) -> Arc<FakeHost> {
    Arc::new(
        FakeHost::new(Platform::Linux)
            .file(".claude/.credentials.json", &valid_login())
            .respond(USAGE, 200, body),
    )
}

#[test]
fn a_missing_cache_block_is_no_data_even_when_the_later_sources_fail() {
    // No tool is installed, so the screen source fails after the OAuth answer.
    let outcome = refresh(&empty_answer_host(b"{}"), Trigger::Manual, NOW);
    assert_eq!(outcome.reading.state, UsageState::Unavailable);
    assert_eq!(
        outcome.reading.unavailable_reason,
        Some(UsageUnavailableReason::NoData)
    );
    assert_eq!(outcome.reading.source, UsageSource::ClaudeOauthEndpoint);
    assert!(outcome.reading.primary.is_none() && outcome.reading.secondary.is_none());
}

#[test]
fn empty_windows_are_no_data_not_invalid_data() {
    for body in [
        &br#"{"five_hour": null, "seven_day": null}"#[..],
        br#"{"five_hour": {"resets_at": null}, "extra_usage": {"utilization": null}}"#,
    ] {
        let outcome = refresh(&empty_answer_host(body), Trigger::Manual, NOW);
        assert_eq!(
            outcome.reading.unavailable_reason,
            Some(UsageUnavailableReason::NoData),
            "{}",
            String::from_utf8_lossy(body)
        );
    }
}

#[test]
fn a_populated_answer_is_a_reading() {
    let outcome = refresh(&empty_answer_host(usage_body()), Trigger::Manual, NOW);
    assert_eq!(outcome.reading.state, UsageState::Ok);
    assert!(outcome.reading.unavailable_reason.is_none());
    assert_eq!(outcome.reading.primary.as_ref().unwrap().used, 0.23);
}
