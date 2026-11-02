#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use soyeht_protocol::{UsageProvider, UsageSource, UsageState, UsageUnavailableReason};
use soyeht_usage::{
    Planner, RefreshOutcome, Trigger, Window,
    host::{HttpResult, LocalUsage, LocalUsageRead, Platform, Secret, StoredCredential},
};
use std::{path::Path, sync::Arc};
use support::{Call, FakeHost, HOME};

/// 2026-09-21 14:13:20 UTC.
const NOW: u64 = 1_790_000_000;
const GO: &str = "https://opencode.ai/zen/go/v1/usage";
const ZAI: &str = "https://api.z.ai/api/monitor/usage/quota/limit";
const BIGMODEL: &str = "https://open.bigmodel.cn/api/monitor/usage/quota/limit";
const DATABASE: &str = "/home/tester/.local/share/opencode/opencode.db";
const ZAI_TOKEN: &str = "zai-fixture-plan-token";
const CONSOLE_TOKEN: &str = "oc-fixture-console-access";

fn go_usage() -> &'static [u8] {
    include_bytes!("fixtures/opencode-go-usage.json")
}

fn zai_quota() -> &'static [u8] {
    include_bytes!("fixtures/opencode-zai-quota.json")
}

fn config(model: &str) -> Vec<u8> {
    format!(r#"{{"model": "{model}"}}"#).into_bytes()
}

/// A machine whose OpenCode is set to `model` (`provider/model`), with its program installed.
fn machine(model: &str) -> FakeHost {
    let mut host =
        FakeHost::new(Platform::MacOs).file(".config/opencode/opencode.json", &config(model));
    host.programs = vec!["opencode".into()];
    host
}

fn with_auth(host: FakeHost) -> FakeHost {
    host.file(
        ".local/share/opencode/auth.json",
        include_bytes!("fixtures/opencode-auth.json"),
    )
}

fn with_console_login(mut host: FakeHost) -> FakeHost {
    let login = include_str!("fixtures/opencode-credential-go.json");
    host.credentials = vec![(
        "opencode".into(),
        StoredCredential::Found(Secret::new(login.trim())),
    )];
    host
}

fn activity(tokens: u64, cost_usd: f64) -> LocalUsageRead {
    LocalUsageRead::Totals(LocalUsage { tokens, cost_usd })
}

fn refresh(host: &Arc<FakeHost>) -> RefreshOutcome {
    Planner::new(host.clone()).refresh(&UsageProvider::Opencode, Trigger::Automatic, NOW, "fetched")
}

/// The one request the host saw, as the test reads it.
struct Sent {
    url: String,
    headers: Vec<(String, String)>,
    secret_names: Vec<String>,
    secret_values: Vec<String>,
}

fn only_http(host: &FakeHost) -> Sent {
    let calls = host.calls();
    let mut requests = calls.iter().filter_map(|call| match call {
        Call::Http {
            url,
            headers,
            secret_names,
            secret_values,
        } => Some(Sent {
            url: url.clone(),
            headers: headers.clone(),
            secret_names: secret_names.clone(),
            secret_values: secret_values.clone(),
        }),
        _ => None,
    });
    let sent = requests
        .next()
        .unwrap_or_else(|| panic!("no request in {calls:?}"));
    assert!(requests.next().is_none(), "{calls:?}");
    sent
}

/// A window as `(label, used, resets_at)`.
fn shown(window: &Option<Window>) -> (&str, f64, &str) {
    let window = window.as_ref().unwrap();
    (&window.label, window.used, &window.resets_at)
}

/// A machine with OpenCode installed and nothing else, whose database records `usage`.
fn bare(platform: Platform, usage: LocalUsageRead) -> FakeHost {
    let mut host = FakeHost::new(platform);
    host.programs = vec!["opencode".into()];
    host.local_usage = usage;
    host
}

fn usage_reads(host: &FakeHost) -> Vec<Call> {
    host.calls()
        .into_iter()
        .filter(|call| matches!(call, Call::OpencodeUsage { .. }))
        .collect()
}

// --- the Go allowance ---

#[test]
fn the_go_allowance_maps_the_five_hour_and_weekly_meters_with_the_consoles_login() {
    let host =
        Arc::new(with_console_login(machine("opencode/big-pickle")).respond(GO, 200, go_usage()));
    let outcome = refresh(&host);
    let reading = outcome.reading;
    assert_eq!(reading.source, UsageSource::OpencodeGoAllowance);
    assert_eq!(reading.state, UsageState::Ok);
    assert_eq!(reading.activity, None);
    assert_eq!(
        shown(&reading.primary),
        ("5-hour", 0.42, "2026-09-21T16:13:20Z")
    );
    assert_eq!(
        shown(&reading.secondary),
        ("Weekly", 0.185, "2026-09-24T14:13:20Z")
    );
    let Sent {
        url,
        headers,
        secret_names,
        secret_values,
    } = only_http(&host);
    assert_eq!(url, GO);
    assert!(headers.contains(&("x-opencode-org-id".into(), "wrk_FIXTURE01".into())));
    assert_eq!(secret_names, vec!["Authorization".to_owned()]);
    assert_eq!(secret_values, vec![format!("Bearer {CONSOLE_TOKEN}")]);
    assert!(
        headers
            .iter()
            .all(|(_, value)| !value.contains(CONSOLE_TOKEN))
    );
    assert!(!format!("{reading:?}").contains(CONSOLE_TOKEN));
    assert_eq!(
        host.calls()[0],
        Call::OpencodeCredential {
            database: DATABASE.into(),
            integration: "opencode".into()
        }
    );
}

#[test]
fn a_login_never_goes_to_a_console_that_is_not_opencode_ai() {
    let mut host = machine("opencode/big-pickle");
    host.credentials = vec![(
        "opencode".into(),
        StoredCredential::Found(Secret::new(
            r#"{"type":"key","key":"oc-fixture-key","metadata":{"orgID":"wrk_X","server":"https://opencode.ai.evil.example/console"}}"#,
        )),
    )];
    host.local_usage = activity(10, 0.0);
    let host = Arc::new(host.respond(GO, 200, go_usage()));
    let reading = refresh(&host).reading;
    assert!(host.http_urls().is_empty(), "{:?}", host.calls());
    assert_eq!(reading.source, UsageSource::OpencodeLocalHistory);
}

#[test]
fn the_retired_console_host_is_read_as_the_default_one() {
    let mut host = machine("opencode-go/glm-5");
    host.credentials = vec![(
        "opencode".into(),
        StoredCredential::Found(Secret::new(
            r#"{"type":"key","key":"oc-fixture-key","metadata":{"orgID":"wrk_X","server":"https://console.opencode.ai/console"}}"#,
        )),
    )];
    let host = Arc::new(host.respond(GO, 200, go_usage()));
    assert_eq!(
        refresh(&host).reading.source,
        UsageSource::OpencodeGoAllowance
    );
    assert_eq!(host.http_urls(), vec![GO.to_owned()]);
}

#[test]
fn an_account_without_go_has_nothing_to_show_and_the_device_activity_stays() {
    for status in [403, 401] {
        let mut host = with_console_login(machine("opencode/big-pickle"));
        host.local_usage = activity(5_000, 0.0);
        let host = Arc::new(host.respond(GO, status, b"{}"));
        let reading = refresh(&host).reading;
        assert_eq!(
            reading.source,
            UsageSource::OpencodeLocalHistory,
            "{status}"
        );
        assert_eq!(reading.state, UsageState::Ok);
        assert_eq!(reading.primary, None);
    }
}

#[test]
fn a_changed_go_reply_is_invalid_data_and_never_a_number() {
    for body in [
        br#"{"usage":{"rolling":{"percent":"soon"},"weekly":{"percent":1}}}"#.as_slice(),
        br#"{"limits":[{"percent":10}]}"#.as_slice(),
        br#"{"usage":{"rolling":{"status":"ok","percent":10}}}"#.as_slice(),
    ] {
        let mut host = with_console_login(machine("opencode/big-pickle"));
        host.local_usage = activity(5_000, 0.0);
        let host = Arc::new(host.respond(GO, 200, body));
        let outcome = refresh(&host);
        assert_eq!(outcome.reading.state, UsageState::Unavailable);
        assert_eq!(
            outcome.reading.unavailable_reason,
            Some(UsageUnavailableReason::InvalidResponse)
        );
        assert_eq!(outcome.reading.primary, None);
        assert!(usage_reads(&host).is_empty());
    }
}

#[test]
fn a_go_login_in_auth_json_has_no_workspace_and_is_not_sent() {
    let mut host = with_auth(machine("opencode/big-pickle"));
    host.local_usage = activity(1, 0.0);
    let host = Arc::new(host.respond(GO, 200, go_usage()));
    let reading = refresh(&host).reading;
    assert!(host.http_urls().is_empty());
    assert_eq!(reading.source, UsageSource::OpencodeLocalHistory);
}

#[test]
fn a_network_failure_of_go_is_retried_sooner_and_not_replaced_by_activity() {
    let mut host = with_console_login(machine("opencode/big-pickle"));
    host.local_usage = activity(1, 0.0);
    let host = Arc::new(host.respond_with(GO, HttpResult::TransportFailed));
    let outcome = refresh(&host);
    assert_eq!(
        outcome.reading.unavailable_reason,
        Some(UsageUnavailableReason::NetworkError)
    );
    assert!(outcome.transient.is_some());
    assert!(usage_reads(&host).is_empty());
}

// --- the GLM Coding Plan ---

#[test]
fn the_glm_plan_quota_maps_its_windows_and_asks_the_way_the_vendors_plugin_does() {
    let host =
        Arc::new(with_auth(machine("zai-coding-plan/glm-5.3")).respond(ZAI, 200, zai_quota()));
    let reading = refresh(&host).reading;
    assert_eq!(reading.source, UsageSource::OpencodeZaiPlan);
    assert_eq!(reading.state, UsageState::Ok);
    assert_eq!(
        shown(&reading.primary),
        ("5-hour", 0.37, "2026-09-21T16:13:20Z")
    );
    assert_eq!(
        shown(&reading.secondary),
        ("Weekly", 0.18, "2026-09-24T14:13:20Z")
    );
    let Sent {
        url,
        headers,
        secret_names,
        secret_values,
    } = only_http(&host);
    assert_eq!(url, ZAI);
    // The plan's token is the whole Authorization value, as the plugin sends it.
    assert_eq!(secret_names, vec!["Authorization".to_owned()]);
    assert_eq!(secret_values, vec![ZAI_TOKEN.to_owned()]);
    assert!(headers.contains(&("Accept-Language".into(), "en-US,en".into())));
    assert!(headers.iter().all(|(_, value)| !value.contains(ZAI_TOKEN)));
    assert!(!format!("{reading:?}").contains(ZAI_TOKEN));
}

#[test]
fn an_expired_plan_is_its_own_state_and_the_chain_ends_there() {
    let mut host = with_auth(machine("zai-coding-plan/glm-5.3"));
    host.local_usage = activity(115_000_000, 0.0);
    let host = Arc::new(host.respond(
        ZAI,
        200,
        include_bytes!("fixtures/opencode-zai-no-plan.json"),
    ));
    let outcome = refresh(&host);
    let reading = outcome.reading;
    assert_eq!(reading.state, UsageState::Unavailable);
    assert_eq!(
        reading.unavailable_reason,
        Some(UsageUnavailableReason::PlanExpired)
    );
    assert_eq!(reading.source, UsageSource::OpencodeZaiPlan);
    assert_eq!((reading.primary, reading.activity), (None, None));
    assert_eq!(outcome.transient, None);
    assert!(usage_reads(&host).is_empty());
}

#[test]
fn other_refusals_of_the_plan_endpoint_are_not_an_expired_plan() {
    for (status, body, reason) in [
        (
            200,
            br#"{"code":1001,"msg":"token invalid","success":false}"#.as_slice(),
            UsageUnavailableReason::SourceFailed,
        ),
        (401, b"{}".as_slice(), UsageUnavailableReason::LoginNeeded),
        (
            200,
            br#"{"code":200,"success":true,"data":{}}"#.as_slice(),
            UsageUnavailableReason::InvalidResponse,
        ),
        (
            200,
            br#"{"code":200,"success":true,"data":{"limits":[{"type":"TOKENS_LIMIT","unit":3}]}}"#
                .as_slice(),
            UsageUnavailableReason::InvalidResponse,
        ),
        (500, b"{}".as_slice(), UsageUnavailableReason::NetworkError),
    ] {
        let host =
            Arc::new(with_auth(machine("zai-coding-plan/glm-5.3")).respond(ZAI, status, body));
        let reading = refresh(&host).reading;
        assert_eq!(
            reading.unavailable_reason,
            Some(reason),
            "{status} {}",
            String::from_utf8_lossy(body)
        );
        assert_eq!(reading.primary, None);
    }
}

#[test]
fn a_plan_with_only_the_mcp_lane_has_no_data_and_not_a_made_up_window() {
    let body = br#"{"code":200,"success":true,"data":{"limits":[
        {"type":"TIME_LIMIT","unit":5,"number":1,"usage":1000,"currentValue":12,"remaining":988,"percentage":1}]}}"#;
    let host = Arc::new(with_auth(machine("zai-coding-plan/glm-5.3")).respond(ZAI, 200, body));
    let reading = refresh(&host).reading;
    assert_eq!(
        reading.unavailable_reason,
        Some(UsageUnavailableReason::NoData)
    );
    assert_eq!(reading.primary, None);
}

#[test]
fn counts_decide_the_percentage_and_an_impossible_five_hour_reset_is_dropped() {
    let body = br#"{"code":200,"success":true,"data":{"limits":[
        {"type":"TOKENS_LIMIT","unit":3,"number":5,"usage":200,"currentValue":50,"remaining":100,"percentage":1,"nextResetTime":1790100000000}]}}"#;
    let host = Arc::new(with_auth(machine("zai-coding-plan/glm-5.3")).respond(ZAI, 200, body));
    let reading = refresh(&host).reading;
    let only = reading.primary.clone().unwrap();
    // 200 allowed, 100 left: half of it is used, not the 1 % the reply also carries.
    assert_eq!((only.used, only.resets_at.as_str()), (0.5, ""));
    assert_eq!(reading.secondary, None);
}

#[test]
fn the_zhipu_plan_is_asked_on_the_mainland_host() {
    let mut host = machine("zhipuai-coding-plan/glm-5.3");
    host = host.file(
        ".local/share/opencode/auth.json",
        br#"{"zhipuai-coding-plan":{"type":"api","key":"zhipu-fixture-token"}}"#,
    );
    let host = Arc::new(host.respond(BIGMODEL, 200, zai_quota()));
    assert_eq!(refresh(&host).reading.source, UsageSource::OpencodeZaiPlan);
    assert_eq!(
        only_http(&host).secret_values,
        vec!["zhipu-fixture-token".to_owned()]
    );
}

#[test]
fn set_to_a_plan_with_no_login_asks_to_sign_in_and_sends_nothing() {
    let mut host = machine("zai-coding-plan/glm-5.3");
    host.local_usage = activity(1, 0.0);
    let host = Arc::new(host);
    let reading = refresh(&host).reading;
    assert_eq!(reading.state, UsageState::LoginNeeded);
    assert_eq!(
        reading.unavailable_reason,
        Some(UsageUnavailableReason::LoginNeeded)
    );
    assert!(host.http_urls().is_empty());
    assert!(usage_reads(&host).is_empty());
}

#[test]
fn a_credential_table_that_holds_credentials_is_the_answer_over_a_leftover_auth_json() {
    let mut host = with_auth(machine("zai-coding-plan/glm-5.3"));
    host.credentials = vec![("zai-coding-plan".into(), StoredCredential::NotInTable)];
    let host = Arc::new(host.respond(ZAI, 200, zai_quota()));
    assert_eq!(refresh(&host).reading.state, UsageState::LoginNeeded);
    assert!(host.http_urls().is_empty());

    let mut host = with_auth(machine("zai-coding-plan/glm-5.3"));
    host.credentials = vec![(
        "zai-coding-plan".into(),
        StoredCredential::Found(Secret::new(r#"{"type":"key","key":"zai-table-token"}"#)),
    )];
    let host = Arc::new(host.respond(ZAI, 200, zai_quota()));
    assert_eq!(refresh(&host).reading.state, UsageState::Ok);
    assert_eq!(
        only_http(&host).secret_values,
        vec!["zai-table-token".to_owned()]
    );
}

// --- activity on this device ---

#[test]
fn a_provider_without_a_limit_shows_what_this_device_recorded_and_no_percentage() {
    let mut host = with_auth(machine("openai/gpt-5.5"));
    host.local_usage = activity(1_234_567, 3.4);
    let host = Arc::new(host);
    let reading = refresh(&host).reading;
    assert_eq!(reading.source, UsageSource::OpencodeLocalHistory);
    assert_eq!(reading.state, UsageState::Ok);
    assert_eq!(
        (reading.primary.clone(), reading.secondary.clone()),
        (None, None)
    );
    let shown = reading.activity.unwrap();
    assert_eq!(shown.label, "Last 7 days on this device");
    assert_eq!((shown.tokens, shown.cost_usd), (1_234_567, Some(3.4)));
    assert!(host.http_urls().is_empty());
    assert_eq!(
        usage_reads(&host),
        vec![Call::OpencodeUsage {
            database: Path::new(DATABASE).to_owned(),
            since_ms: ((NOW - 7 * 86_400) * 1000) as i64,
            provider: Some("openai".into()),
        }]
    );
}

#[test]
fn a_plan_that_bills_nothing_per_token_shows_tokens_and_no_cost() {
    let mut host = machine("kimi-for-coding/k3");
    host.local_usage = activity(42, 0.0);
    let reading = refresh(&Arc::new(host)).reading;
    let shown = reading.activity.unwrap();
    assert_eq!((shown.tokens, shown.cost_usd), (42, None));
}

#[test]
fn the_model_used_last_decides_when_the_configuration_names_none_and_all_providers_when_neither() {
    let host = Arc::new(bare(Platform::Linux, activity(1, 0.0)).file(
        ".local/state/opencode/model.json",
        include_bytes!("fixtures/opencode-model-state.json"),
    ));
    refresh(&host);
    assert!(matches!(
        &usage_reads(&host)[0],
        Call::OpencodeUsage { provider: Some(provider), .. } if provider == "openai"
    ));

    let host = Arc::new(bare(Platform::Linux, activity(1, 0.0)));
    refresh(&host);
    assert!(matches!(
        &usage_reads(&host)[0],
        Call::OpencodeUsage { provider: None, .. }
    ));
}

#[test]
fn the_configuration_wins_over_the_model_used_last() {
    let mut host = FakeHost::new(Platform::Linux)
        .file(
            ".config/opencode/opencode.json",
            include_bytes!("fixtures/opencode-config.json"),
        )
        .file(
            ".local/state/opencode/model.json",
            include_bytes!("fixtures/opencode-model-state.json"),
        )
        .file(
            ".local/share/opencode/auth.json",
            include_bytes!("fixtures/opencode-auth.json"),
        );
    host.programs = vec!["opencode".into()];
    let host = Arc::new(host.respond(ZAI, 200, zai_quota()));
    assert_eq!(refresh(&host).reading.source, UsageSource::OpencodeZaiPlan);
}

#[test]
fn opencodes_own_variables_move_its_files() {
    let mut host = FakeHost::new(Platform::Linux);
    host.env = vec![
        ("XDG_DATA_HOME".into(), "/data".into()),
        ("XDG_CONFIG_HOME".into(), "/config".into()),
        ("OPENCODE_DB".into(), "history.db".into()),
    ];
    host.files = vec![
        (
            "/config/opencode/opencode.json".into(),
            soyeht_usage::host::FileRead::Bytes(config("openai/gpt-5.5")),
        ),
        (
            "/data/opencode/history.db".into(),
            soyeht_usage::host::FileRead::Unreadable,
        ),
    ];
    host.local_usage = activity(1, 0.0);
    let host = Arc::new(host);
    assert_eq!(refresh(&host).reading.state, UsageState::Ok);
    assert!(matches!(
        &usage_reads(&host)[0],
        Call::OpencodeUsage { database, provider: Some(provider), .. }
            if database == Path::new("/data/opencode/history.db") && provider == "openai"
    ));

    // An in-memory database has no file to read.
    let mut host = FakeHost::new(Platform::Linux);
    host.env = vec![("OPENCODE_DB".into(), ":memory:".into())];
    host.programs = vec!["opencode".into()];
    let host = Arc::new(host);
    assert_eq!(
        refresh(&host).reading.unavailable_reason,
        Some(UsageUnavailableReason::NoData)
    );
    assert!(usage_reads(&host).is_empty());
}

#[test]
fn opencode_missing_from_the_machine_is_not_found_and_reads_nothing() {
    let host = Arc::new(FakeHost::new(Platform::MacOs));
    let reading = refresh(&host).reading;
    assert_eq!(reading.state, UsageState::Unavailable);
    assert_eq!(
        reading.unavailable_reason,
        Some(UsageUnavailableReason::ToolNotFound)
    );
    assert!(usage_reads(&host).is_empty() && host.http_urls().is_empty());
}

#[test]
fn an_installed_opencode_with_no_history_or_an_unknown_layout_has_no_data() {
    for read in [LocalUsageRead::NoDatabase, LocalUsageRead::NoHistory] {
        let mut host = FakeHost::new(Platform::MacOs);
        host.programs = vec!["opencode".into()];
        host.local_usage = read;
        let reading = refresh(&Arc::new(host)).reading;
        assert_eq!(
            reading.unavailable_reason,
            Some(UsageUnavailableReason::NoData)
        );
    }
    let mut host = FakeHost::new(Platform::MacOs);
    host.programs = vec!["opencode".into()];
    host.local_usage = LocalUsageRead::Unreadable;
    let reading = refresh(&Arc::new(host)).reading;
    assert_eq!(
        reading.unavailable_reason,
        Some(UsageUnavailableReason::SourceFailed)
    );
}

#[test]
fn a_database_without_a_program_still_counts_as_installed() {
    let mut host = FakeHost::new(Platform::MacOs);
    host.files = vec![(
        Path::new(HOME).join(".local/share/opencode/opencode.db"),
        soyeht_usage::host::FileRead::Unreadable,
    )];
    host.local_usage = activity(0, 0.0);
    let reading = refresh(&Arc::new(host)).reading;
    assert_eq!(reading.state, UsageState::Ok);
    assert_eq!(reading.activity.unwrap().tokens, 0);
}

#[test]
fn the_chain_lists_the_three_sources_in_order() {
    let planner = Planner::new(Arc::new(FakeHost::new(Platform::MacOs)));
    assert_eq!(
        planner.sources(&UsageProvider::Opencode),
        vec![
            UsageSource::OpencodeGoAllowance,
            UsageSource::OpencodeZaiPlan,
            UsageSource::OpencodeLocalHistory
        ]
    );
    assert!(planner.providers().contains(&UsageProvider::Opencode));
}
