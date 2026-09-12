use soyeht_protocol::{UsageProvider, UsageSource, UsageState};
use soyeht_usage::{
    AutomaticTrigger, ManualRefresh, Reading, RefreshPolicy, TransientFailure, Window,
};

fn reading() -> Reading {
    Reading {
        provider: UsageProvider::Claude,
        state: UsageState::Ok,
        primary: Some(Window {
            used: 0.4,
            resets_at: "later".into(),
            label: "Current".into(),
        }),
        secondary: None,
        source: UsageSource::ClaudeCliUsage,
        fetched_at: "now".into(),
        observed_at_sec: 1_000,
    }
}

#[test]
fn manual_refresh_is_limited_per_provider_for_thirty_seconds() {
    let mut policy = RefreshPolicy::default();
    assert_eq!(
        policy.manual(UsageProvider::Claude, 100),
        ManualRefresh::Ready
    );
    policy.record(UsageProvider::Claude, 100, None);
    assert_eq!(
        policy.manual(UsageProvider::Claude, 101),
        ManualRefresh::RateLimited {
            retry_after_sec: 29
        }
    );
    assert_eq!(
        policy.manual(UsageProvider::Claude, 130),
        ManualRefresh::Ready
    );
    assert_eq!(
        policy.manual(UsageProvider::Codex, 101),
        ManualRefresh::Ready
    );
}

#[test]
fn hover_requires_an_old_reading_and_never_queues_ticks() {
    let mut policy = RefreshPolicy::default();
    assert!(!policy.automatic_due(
        UsageProvider::Claude,
        AutomaticTrigger::Hover,
        100,
        Some(60)
    ));
    assert!(policy.automatic_due(
        UsageProvider::Claude,
        AutomaticTrigger::Hover,
        100,
        Some(61)
    ));
    policy.record(UsageProvider::Claude, 100, None);
    assert!(!policy.automatic_due(
        UsageProvider::Claude,
        AutomaticTrigger::Hover,
        129,
        Some(90)
    ));
    assert!(policy.automatic_due(
        UsageProvider::Claude,
        AutomaticTrigger::Hover,
        130,
        Some(90)
    ));
}

#[test]
fn rail_refreshes_every_five_minutes_and_transient_backoff_caps_at_thirty() {
    let mut policy = RefreshPolicy::default();
    for (attempt, expected_wait) in [(100, 300), (400, 600), (1_000, 1_200), (2_200, 1_800)] {
        policy.record(
            UsageProvider::Claude,
            attempt,
            Some(&TransientFailure::Network),
        );
        assert!(!policy.automatic_due(
            UsageProvider::Claude,
            AutomaticTrigger::RailOpen,
            attempt + expected_wait - 1,
            None
        ));
        assert!(policy.automatic_due(
            UsageProvider::Claude,
            AutomaticTrigger::RailOpen,
            attempt + expected_wait,
            None
        ));
    }
    policy.record(UsageProvider::Claude, 4_000, None);
    assert!(policy.automatic_due(
        UsageProvider::Claude,
        AutomaticTrigger::RailOpen,
        4_300,
        None
    ));
}

#[test]
fn provider_retry_after_affects_automatic_refresh_but_not_the_manual_gate() {
    let mut policy = RefreshPolicy::default();
    policy.record(
        UsageProvider::Cursor,
        100,
        Some(&TransientFailure::RateLimited {
            retry_after_sec: 9_000,
        }),
    );
    assert!(!policy.automatic_due(
        UsageProvider::Cursor,
        AutomaticTrigger::RailOpen,
        1_899,
        None
    ));
    assert!(policy.automatic_due(
        UsageProvider::Cursor,
        AutomaticTrigger::RailOpen,
        1_900,
        None
    ));
    assert_eq!(
        policy.manual(UsageProvider::Cursor, 130),
        ManualRefresh::Ready
    );
    policy.record(UsageProvider::Cursor, 2_000, None);
    assert_eq!(
        policy.manual(UsageProvider::Cursor, 2_001),
        ManualRefresh::RateLimited {
            retry_after_sec: 29
        }
    );
}

#[test]
fn stored_and_old_readings_are_stale_without_losing_values() {
    let reading = reading();
    let restored = reading.restored_from_store();
    assert_eq!(restored.state, UsageState::Stale);
    assert_eq!(restored.primary, reading.primary);
    assert_eq!(reading.for_display(1_900).state, UsageState::Ok);
    assert_eq!(reading.for_display(1_901).state, UsageState::Stale);
}
