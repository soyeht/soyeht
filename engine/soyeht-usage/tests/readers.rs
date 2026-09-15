#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_protocol::{UsageProvider, UsageSource, UsageState};
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
        vec![
            UsageSource::ClaudeCliUsage,
            UsageSource::ClaudeOauthEndpoint,
            UsageSource::ClaudeDesktopCache,
        ]
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
fn first_reader_normalizes_two_verified_windows() {
    let payload = r#"{"result":"Current session: 25% used · resets Sep 12 at 2:00pm\nCurrent week (all models): 75% used · resets Sep 16 at 1:00am"}"#
        .as_bytes()
        .to_vec();
    let (outcome, source) = refresh(
        UsageProvider::Claude,
        vec![(
            UsageProvider::Claude,
            UsageSource::ClaudeCliUsage,
            SourceRead::Payload(payload),
        )],
    );
    assert_eq!(source.calls.len(), 1);
    assert_eq!(outcome.reading.state, UsageState::Ok);
    assert_eq!(outcome.reading.primary.unwrap().used, 0.25);
    assert_eq!(outcome.reading.secondary.unwrap().used, 0.75);
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
    assert_eq!(outcome.reading.primary.unwrap().used, 0.4);
    let weekly = outcome.reading.secondary.unwrap();
    assert_eq!(weekly.used, 0.6);
    assert_eq!(weekly.label, "Weekly");
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
fn fallbacks_are_tried_in_order_and_unverified_payloads_fail_closed() {
    let script = vec![
        (
            UsageProvider::Claude,
            UsageSource::ClaudeCliUsage,
            SourceRead::Unavailable,
        ),
        (
            UsageProvider::Claude,
            UsageSource::ClaudeOauthEndpoint,
            SourceRead::Payload(br#"{"usage":25}"#.to_vec()),
        ),
        (
            UsageProvider::Claude,
            UsageSource::ClaudeDesktopCache,
            SourceRead::LoginNeeded,
        ),
    ];
    let (outcome, source) = refresh(UsageProvider::Claude, script);
    assert_eq!(source.calls.len(), 3);
    assert_eq!(outcome.reading.state, UsageState::LoginNeeded);
    assert_eq!(outcome.reading.source, UsageSource::ClaudeDesktopCache);
}

#[test]
fn scripted_sources_cover_all_five_public_states() {
    let ok_payload = br#"{"billingCycleEnd":"later","planUsage":{"totalPercentUsed":1}}"#.to_vec();
    let cases = [
        (SourceRead::Payload(ok_payload), UsageState::Ok),
        (SourceRead::LoginNeeded, UsageState::LoginNeeded),
        (SourceRead::NoAccess, UsageState::NoAccess),
        (SourceRead::Unavailable, UsageState::Unavailable),
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
fn a_transient_fallback_failure_drives_retry_even_after_plain_unavailability() {
    let script = vec![
        (
            UsageProvider::Codex,
            UsageSource::CodexAppServer,
            SourceRead::Unavailable,
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
