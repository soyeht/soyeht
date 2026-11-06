//! The OpenCode meter follows the provider of the pane in use: when it is read, and what it
//! shows after the pane moves to another provider.

use super::{
    Service,
    tests::{ScriptedHost, usage_answer},
};
use soyeht_protocol::{UsageProvider, UsageState};
use soyeht_usage::{
    ReadHints,
    host::{LocalUsage, LocalUsageRead},
};
use std::{sync::Arc, time::Duration};

fn host() -> Arc<ScriptedHost> {
    let host = // Codex reads its usage whenever it is asked, so that it never falls due early.
    Arc::new(ScriptedHost::new((0..16).map(|_| usage_answer(10)).collect()));
    host.record_opencode(activity(10));
    host
}

fn activity(tokens: u64) -> LocalUsageRead {
    LocalUsageRead::Totals(LocalUsage {
        tokens,
        cost_usd: 0.0,
    })
}

fn asked(host: &ScriptedHost) -> Vec<Option<String>> {
    host.opencode_asked.lock().unwrap().clone()
}

fn pane_uses(model: &str) -> ReadHints {
    ReadHints {
        opencode_model: Some(model.to_owned()),
    }
}

/// One background pass at `now_sec`: the providers it read, if it read any.
fn pass(service: &mut Service, now_sec: u64, hints: ReadHints) -> Option<Vec<UsageProvider>> {
    let prepared = service.prepare_automatic(now_sec, "then", hints)?;
    let providers = prepared.providers().to_vec();
    service.commit(prepared.read());
    Some(providers)
}

#[test]
fn a_pane_that_moves_opencode_to_another_provider_is_read_ahead_of_the_interval() {
    let host = host();
    let mut service = Service::restore(Vec::new(), host.clone(), true);
    assert_eq!(
        pass(&mut service, 1_000, ReadHints::default())
            .unwrap()
            .len(),
        4
    );
    assert_eq!(asked(&host), [None]);

    let moved = pane_uses("opencode-go/glm-5.3-flash");
    // Not hurried past the limit of a click, and not read again while nothing changed.
    assert_eq!(pass(&mut service, 1_010, moved.clone()), None);
    assert_eq!(pass(&mut service, 1_100, ReadHints::default()), None);
    assert_eq!(
        pass(&mut service, 1_100, moved.clone()),
        Some(vec![UsageProvider::Opencode])
    );
    assert_eq!(asked(&host), [None, Some("opencode-go".to_owned())]);

    // Another model of the same provider follows nothing new; closing the pane does.
    let sibling = pass(&mut service, 1_200, pane_uses("opencode-go/glm-5.3"));
    assert_eq!(sibling, None);
    assert_eq!(
        pass(&mut service, 1_200, ReadHints::default()),
        Some(vec![UsageProvider::Opencode])
    );
}

#[test]
fn a_provider_that_has_not_been_read_yet_is_read_by_the_ordinary_schedule() {
    let host = host();
    let mut service = Service::restore(Vec::new(), host.clone(), true);
    let first = pass(&mut service, 1_000, pane_uses("openai/gpt-5")).unwrap();
    assert!(first.contains(&UsageProvider::Opencode));
    assert_eq!(asked(&host), [Some("openai".to_owned())]);
}

#[test]
fn a_failed_read_of_the_new_provider_does_not_keep_the_old_providers_reading() {
    let host = host();
    let mut service = Service::restore(Vec::new(), host.clone(), true);
    pass(&mut service, 1_000, pane_uses("openai/gpt-5")).unwrap();
    let before = service.shown(&UsageProvider::Opencode, 1_000);
    assert_eq!(before.state, UsageState::Ok);

    host.record_opencode(LocalUsageRead::NoHistory);
    pass(&mut service, 1_100, pane_uses("anthropic/claude")).unwrap();
    let after = service.shown(&UsageProvider::Opencode, 1_100);
    assert_eq!(after.state, UsageState::Unavailable);
    assert_eq!(after.activity, None);

    // Without a change the first failure after a good reading is still not shown.
    host.record_opencode(activity(5));
    pass(&mut service, 1_500, pane_uses("anthropic/claude")).unwrap();
    host.record_opencode(LocalUsageRead::NoHistory);
    pass(&mut service, 1_900, pane_uses("anthropic/claude")).unwrap();
    assert_eq!(
        service.shown(&UsageProvider::Opencode, 1_900).state,
        UsageState::Ok
    );
}

#[tokio::test]
async fn the_schedule_is_woken_and_not_otherwise() {
    let service = Service::restore(Vec::new(), host(), true);
    let woken = service.woken();
    let quiet = tokio::time::timeout(Duration::from_millis(50), woken.notified()).await;
    assert!(quiet.is_err());
    service.wake();
    let signalled = tokio::time::timeout(Duration::from_secs(5), woken.notified()).await;
    assert!(signalled.is_ok());
}

#[test]
fn an_abandoned_batch_records_nothing_so_the_next_refresh_still_follows_the_change() {
    let host = host();
    let mut service = Service::restore(Vec::new(), host.clone(), true);
    pass(&mut service, 1_000, ReadHints::default()).unwrap();

    // A batch is prepared after the provider changed, and never produces a batch.
    let moved = pane_uses("opencode-go/glm-5.3-flash");
    let prepared = service
        .prepare_automatic(1_100, "then", moved.clone())
        .unwrap();
    let providers = prepared.providers().to_vec();
    assert_eq!(providers, [UsageProvider::Opencode]);
    service.abandon(&providers);

    // The change was not consumed: the next pass reads the new provider, and a failed read of it
    // is not hidden behind the old provider's reading.
    host.record_opencode(LocalUsageRead::NoHistory);
    assert_eq!(pass(&mut service, 1_100, moved), Some(providers));
    assert_eq!(asked(&host), [None, Some("opencode-go".to_owned())]);
    let after = service.shown(&UsageProvider::Opencode, 1_100);
    assert_eq!(after.state, UsageState::Unavailable);
}
