#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_protocol::{UsageProvider, UsageSource, UsageState, UsageUnavailableReason};
use soyeht_usage::{ReaderRegistry, SourceRead, UsageSourcePort};
use std::collections::VecDeque;

#[derive(Default)]
struct ScriptedSource {
    script: VecDeque<(UsageProvider, UsageSource, SourceRead)>,
    calls: Vec<(UsageProvider, UsageSource)>,
}

impl ScriptedSource {
    fn with(script: Vec<(UsageProvider, UsageSource, SourceRead)>) -> Self {
        Self {
            script: script.into(),
            calls: Vec::new(),
        }
    }
}

impl UsageSourcePort for ScriptedSource {
    fn read(&mut self, provider: UsageProvider, source: UsageSource) -> SourceRead {
        self.calls.push((provider.clone(), source.clone()));
        let (expected_provider, expected_source, response) = self.script.pop_front().unwrap();
        assert_eq!(provider, expected_provider);
        assert_eq!(source, expected_source);
        response
    }
}

fn refresh(
    provider: UsageProvider,
    script: Vec<(UsageProvider, UsageSource, SourceRead)>,
) -> (soyeht_usage::RefreshOutcome, ScriptedSource) {
    let registry = ReaderRegistry::new();
    let mut source = ScriptedSource::with(script);
    let outcome = registry
        .refresh(provider, &mut source, "2026-09-12T10:00:00Z", 1_000)
        .unwrap();
    (outcome, source)
}

#[test]
fn registry_and_source_orders_are_closed() {
    let registry = ReaderRegistry::new();
    assert_eq!(
        registry.providers(),
        vec![
            UsageProvider::Claude,
            UsageProvider::Codex,
            UsageProvider::Cursor
        ]
    );
    assert_eq!(
        registry.sources(UsageProvider::Claude),
        vec![UsageSource::ClaudeDesktopCache]
    );
    assert_eq!(
        registry.sources(UsageProvider::Codex),
        vec![
            UsageSource::CodexAppServer,
            UsageSource::CodexAccountEndpoint
        ]
    );
    assert_eq!(
        registry.sources(UsageProvider::Cursor),
        vec![UsageSource::CursorCurrentPeriod]
    );
}

#[test]
fn first_reader_normalizes_only_the_cached_usage_key() {
    let payload = br#"{"accountUuid":"must-not-escape","unrelated":{"private":true},"cachedUsageUtilization":{"accountUuid":"must-not-escape","fetchedAtMs":1789729200000,"utilization":{"five_hour":{"utilization":25,"resets_at":"2026-09-18T14:40:00Z"},"seven_day":{"utilization":75,"resets_at":"2026-09-23T16:00:00Z"}}}}"#.to_vec();
    let (outcome, source) = refresh(
        UsageProvider::Claude,
        vec![(
            UsageProvider::Claude,
            UsageSource::ClaudeDesktopCache,
            SourceRead::Payload(payload),
        )],
    );
    assert_eq!(source.calls.len(), 1);
    assert_eq!(outcome.reading.state, UsageState::Ok);
    assert_eq!(outcome.reading.primary.unwrap().used, 0.25);
    let weekly = outcome.reading.secondary.unwrap();
    assert_eq!(weekly.used, 0.75);
    assert_eq!(weekly.label, "Weekly");
    assert_eq!(outcome.reading.observed_at_sec, 1_789_729_200);
    assert_eq!(outcome.reading.fetched_at, "2026-09-18T11:00:00.000Z");
}

#[test]
fn second_reader_normalizes_the_verified_app_server_shape() {
    let payload = br#"{"result":{"rateLimits":{"primary":{"usedPercent":40,"resetsAt":1800000000,"windowDurationMins":300},"secondary":{"usedPercent":60,"resetsAt":1800100000,"windowDurationMins":10080}}}}"#.to_vec();
    let (outcome, _) = refresh(
        UsageProvider::Codex,
        vec![(
            UsageProvider::Codex,
            UsageSource::CodexAppServer,
            SourceRead::Payload(payload),
        )],
    );
    assert_eq!(outcome.reading.state, UsageState::Ok);
    let current = outcome.reading.primary.unwrap();
    assert_eq!(current.used, 0.4);
    assert_eq!(current.label, "Current window");
    let weekly = outcome.reading.secondary.unwrap();
    assert_eq!(weekly.used, 0.6);
    assert_eq!(weekly.label, "Weekly");
}

#[test]
fn second_reader_names_a_primary_weekly_window() {
    let payload = br#"{"result":{"rateLimits":{"primary":{"usedPercent":40,"resetsAt":1800000000,"windowDurationMins":10080},"secondary":null}}}"#.to_vec();
    let (outcome, _) = refresh(
        UsageProvider::Codex,
        vec![(
            UsageProvider::Codex,
            UsageSource::CodexAppServer,
            SourceRead::Payload(payload),
        )],
    );
    assert_eq!(outcome.reading.primary.unwrap().label, "Weekly");
    assert!(outcome.reading.secondary.is_none());
}

#[test]
fn third_reader_normalizes_the_verified_current_period_shape() {
    let payload =
        br#"{"billingCycleEnd":"2026-10-01T00:00:00Z","planUsage":{"totalPercentUsed":52.5}}"#
            .to_vec();
    let (outcome, _) = refresh(
        UsageProvider::Cursor,
        vec![(
            UsageProvider::Cursor,
            UsageSource::CursorCurrentPeriod,
            SourceRead::Payload(payload),
        )],
    );
    assert_eq!(outcome.reading.state, UsageState::Ok);
    assert_eq!(outcome.reading.primary.unwrap().used, 0.525);
    assert!(outcome.reading.secondary.is_none());
}

#[test]
fn missing_or_invalid_cached_usage_is_unavailable() {
    let cases = [
        br#"{"other":true}"#.as_slice(),
        br#"{"cachedUsageUtilization":{"fetchedAtMs":"wrong","utilization":{}}}"#.as_slice(),
        br#"{"cachedUsageUtilization":{"fetchedAtMs":1789729200000,"utilization":{"five_hour":{"utilization":"wrong","resets_at":"later"},"seven_day":{"utilization":20,"resets_at":"later"}}}}"#.as_slice(),
    ];
    for payload in cases {
        let (outcome, source) = refresh(
            UsageProvider::Claude,
            vec![(
                UsageProvider::Claude,
                UsageSource::ClaudeDesktopCache,
                SourceRead::Payload(payload.to_vec()),
            )],
        );
        assert_eq!(source.calls.len(), 1);
        assert_eq!(outcome.reading.state, UsageState::Unavailable);
        assert_eq!(outcome.reading.source, UsageSource::ClaudeDesktopCache);
    }
}

#[test]
fn cached_timestamp_drives_fresh_and_stale_presentation_without_losing_values() {
    let payload = |fetched_at_ms| {
        format!(
            "{{\"cachedUsageUtilization\":{{\"fetchedAtMs\":{fetched_at_ms},\"utilization\":{{\"five_hour\":{{\"utilization\":10,\"resets_at\":\"later\"}},\"seven_day\":{{\"utilization\":20,\"resets_at\":\"later\"}}}}}}}}"
        )
        .into_bytes()
    };
    for (fetched_at_ms, now_sec, expected) in [
        (1_900_000, 2_000, UsageState::Ok),
        (1_000_000, 2_000, UsageState::Stale),
    ] {
        let (outcome, _) = refresh(
            UsageProvider::Claude,
            vec![(
                UsageProvider::Claude,
                UsageSource::ClaudeDesktopCache,
                SourceRead::Payload(payload(fetched_at_ms)),
            )],
        );
        let displayed = outcome.reading.for_display(now_sec);
        assert_eq!(displayed.state, expected);
        assert_eq!(displayed.primary.unwrap().used, 0.1);
        assert_eq!(displayed.secondary.unwrap().used, 0.2);
    }
}

#[test]
fn scripted_sources_cover_all_five_public_states() {
    let ok_payload = br#"{"billingCycleEnd":"later","planUsage":{"totalPercentUsed":1}}"#.to_vec();
    let cases = [
        (SourceRead::Payload(ok_payload), UsageState::Ok),
        (SourceRead::LoginNeeded, UsageState::LoginNeeded),
        (SourceRead::NoAccess, UsageState::NoAccess),
        (SourceRead::Unavailable(None), UsageState::Unavailable),
    ];
    for (response, state) in cases {
        let (outcome, _) = refresh(
            UsageProvider::Cursor,
            vec![(
                UsageProvider::Cursor,
                UsageSource::CursorCurrentPeriod,
                response,
            )],
        );
        assert_eq!(outcome.reading.state, state);
    }
    let (outcome, _) = refresh(
        UsageProvider::Cursor,
        vec![(
            UsageProvider::Cursor,
            UsageSource::CursorCurrentPeriod,
            SourceRead::Payload(
                br#"{"billingCycleEnd":"later","planUsage":{"totalPercentUsed":1}}"#.to_vec(),
            ),
        )],
    );
    assert_eq!(outcome.reading.for_display(1_901).state, UsageState::Stale);
}

#[test]
fn no_access_has_precedence_over_other_failed_fallbacks() {
    let script = vec![
        (
            UsageProvider::Codex,
            UsageSource::CodexAppServer,
            SourceRead::LoginNeeded,
        ),
        (
            UsageProvider::Codex,
            UsageSource::CodexAccountEndpoint,
            SourceRead::NoAccess,
        ),
    ];
    let (outcome, _) = refresh(UsageProvider::Codex, script);
    assert_eq!(outcome.reading.state, UsageState::NoAccess);
    assert_eq!(outcome.reading.source, UsageSource::CodexAccountEndpoint);
}

#[test]
fn codex_missing_tool_reason_survives_an_unimplemented_fallback() {
    let (outcome, _) = refresh(
        UsageProvider::Codex,
        vec![
            (
                UsageProvider::Codex,
                UsageSource::CodexAppServer,
                SourceRead::Unavailable(Some(UsageUnavailableReason::ToolNotFound)),
            ),
            (
                UsageProvider::Codex,
                UsageSource::CodexAccountEndpoint,
                SourceRead::Unavailable(None),
            ),
        ],
    );
    assert_eq!(outcome.reading.state, UsageState::Unavailable);
    assert_eq!(outcome.reading.source, UsageSource::CodexAppServer);
    assert_eq!(
        outcome.reading.unavailable_reason,
        Some(UsageUnavailableReason::ToolNotFound)
    );
}

#[test]
fn a_transient_fallback_failure_drives_retry_even_after_plain_unavailability() {
    let script = vec![
        (
            UsageProvider::Codex,
            UsageSource::CodexAppServer,
            SourceRead::Unavailable(None),
        ),
        (
            UsageProvider::Codex,
            UsageSource::CodexAccountEndpoint,
            SourceRead::NetworkError,
        ),
    ];
    let (outcome, _) = refresh(UsageProvider::Codex, script);
    assert_eq!(
        outcome.transient,
        Some(soyeht_usage::TransientFailure::Network)
    );
    assert_eq!(outcome.reading.source, UsageSource::CodexAccountEndpoint);
}
