#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_protocol::{UsageProvider, UsageSource, UsageState};
use soyeht_usage::{ManualRefresh, Reading, RefreshPolicy, TransientFailure, Window};

const CODEX: UsageProvider = UsageProvider::Codex;

fn reading(resets_at: &str) -> Reading {
    Reading {
        provider: CODEX,
        state: UsageState::Ok,
        unavailable_reason: None,
        primary: Some(Window {
            used: 0.4,
            resets_at: resets_at.into(),
            label: "5-hour window".into(),
        }),
        secondary: None,
        source: UsageSource::CodexAppServer,
        fetched_at: "then".into(),
        observed_at_sec: 1_000,
    }
}

#[test]
fn every_provider_is_due_at_startup_and_then_every_five_minutes() {
    let mut policy = RefreshPolicy::default();
    assert!(policy.automatic_due(&CODEX, 1_000, None));
    policy.record(&CODEX, 1_000, None);
    assert!(!policy.automatic_due(&CODEX, 1_299, None));
    assert!(policy.automatic_due(&CODEX, 1_300, None));
}

#[test]
fn network_failures_retry_on_the_connectivity_ladder() {
    let mut policy = RefreshPolicy::default();
    let mut now = 1_000;
    for wait in [15, 45, 120, 300, 300] {
        policy.record(&CODEX, now, Some(&TransientFailure::Network));
        assert!(!policy.automatic_due(&CODEX, now + wait - 1, None));
        assert!(policy.automatic_due(&CODEX, now + wait, None));
        now += wait;
    }
    policy.record(&CODEX, now, None);
    assert!(!policy.automatic_due(&CODEX, now + 299, None));
}

#[test]
fn a_window_that_resets_makes_the_provider_due_at_once() {
    let mut policy = RefreshPolicy::default();
    policy.record(&CODEX, 1_000, None);
    let shown = reading("1970-01-01T00:20:00Z");
    assert!(!policy.automatic_due(&CODEX, 1_199, Some(&shown)));
    assert!(policy.automatic_due(&CODEX, 1_200, Some(&shown)));
}

#[test]
fn manual_refresh_is_limited_per_provider_for_thirty_seconds() {
    let mut policy = RefreshPolicy::default();
    assert_eq!(policy.manual(&CODEX, 100), ManualRefresh::Ready);
    policy.record(&CODEX, 90, None);
    assert_eq!(
        policy.manual(&CODEX, 100),
        ManualRefresh::Ready,
        "background counted"
    );
    policy.record_manual(&CODEX, 100);
    assert_eq!(
        policy.manual(&CODEX, 110),
        ManualRefresh::RateLimited {
            retry_after_sec: 20
        }
    );
    assert_eq!(
        policy.manual(&UsageProvider::Claude, 110),
        ManualRefresh::Ready
    );
    assert_eq!(policy.manual(&CODEX, 130), ManualRefresh::Ready);
}

#[test]
fn kept_and_restored_readings_are_last_known_with_their_values_and_age() {
    let kept = reading("later")
        .kept_after_failure(Some(soyeht_protocol::UsageUnavailableReason::NetworkError));
    assert_eq!(kept.state, UsageState::Stale);
    assert_eq!(kept.primary.as_ref().unwrap().used, 0.4);
    assert_eq!(kept.fetched_at, "then");
    assert_eq!(
        reading("later").restored_from_store().state,
        UsageState::Stale
    );
    assert_eq!(reading("later").for_display(1_900).state, UsageState::Ok);
    assert_eq!(reading("later").for_display(1_901).state, UsageState::Stale);
}
