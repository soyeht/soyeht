//! Walks each provider's source chain in order until one source produces a reading.
//!
//! The rules come from CodexBar's `ProviderFetchPipeline` and the provider descriptors: a source
//! with nothing to read is skipped, a failure continues to the next source only when that
//! source's fallback rule allows it, and when every source fails the error shown is the last one
//! that was actually attempted. Cooldowns and the CLI reuse window protect automatic refreshes
//! only; a manual refresh always tries every source, as a click does in CodexBar.

use crate::{
    Reading,
    host::Host,
    sources::{self, Failure, ParsedReading, ReadContext, Source, Trigger},
};
use chrono::DateTime;
use soyeht_protocol::{UsageProvider, UsageSource, UsageState, UsageUnavailableReason};
use std::sync::{Arc, Mutex, PoisonError};

pub const PROVIDERS: [UsageProvider; 3] = [
    UsageProvider::Claude,
    UsageProvider::Codex,
    UsageProvider::Cursor,
];

/// A refresh failure that should make the provider's schedule retry sooner or later.
#[derive(Debug, Clone, PartialEq)]
pub enum TransientFailure {
    Network,
    RateLimited { retry_after_sec: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefreshOutcome {
    /// A fresh reading, or the failure of the chain (state other than `ok`, no windows).
    pub reading: Reading,
    pub transient: Option<TransientFailure>,
}

struct Cooldown {
    source: UsageSource,
    until_sec: u64,
    failure: Failure,
}

struct Reuse {
    source: UsageSource,
    read_at_sec: u64,
    reading: ParsedReading,
}

#[derive(Default)]
struct Memory {
    cooldowns: Vec<Cooldown>,
    reuse: Vec<Reuse>,
}

/// The source chains of every provider over one [`Host`].
pub struct Planner {
    host: Arc<dyn Host>,
    chains: Vec<(UsageProvider, Vec<Box<dyn Source>>)>,
    memory: Mutex<Memory>,
}

impl Planner {
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self {
            host,
            chains: PROVIDERS
                .iter()
                .map(|provider| (provider.clone(), sources::chain(provider)))
                .collect(),
            memory: Mutex::default(),
        }
    }

    pub fn providers(&self) -> Vec<UsageProvider> {
        self.chains
            .iter()
            .map(|(provider, _)| provider.clone())
            .collect()
    }

    /// The provider's sources in the order they are tried.
    pub fn sources(&self, provider: &UsageProvider) -> Vec<UsageSource> {
        self.chain(provider)
            .map(|chain| chain.iter().map(|source| source.id()).collect())
            .unwrap_or_default()
    }

    fn chain(&self, provider: &UsageProvider) -> Option<&[Box<dyn Source>]> {
        self.chains
            .iter()
            .find(|(candidate, _)| candidate == provider)
            .map(|(_, chain)| chain.as_slice())
    }

    /// Reads `provider` once. Runs without any engine lock: it may wait on the network, the
    /// keychain or a provider process for tens of seconds.
    pub fn refresh(
        &self,
        provider: &UsageProvider,
        trigger: Trigger,
        now_sec: u64,
        fetched_at: &str,
    ) -> RefreshOutcome {
        let chain = self.chain(provider).unwrap_or_default();
        let context = ReadContext { now_sec };
        let mut last: Option<(UsageSource, Failure)> = None;
        for source in chain {
            let id = source.id();
            let attempt = match self.held(&id, trigger, now_sec) {
                Some(Held::Reused(reading, read_at_sec)) => {
                    return available(provider, id, reading, read_at_sec);
                }
                Some(Held::CoolingDown(failure)) => Err(failure),
                None => source.read(self.host.as_ref(), context),
            };
            match attempt {
                Ok(reading) => {
                    self.remember_success(source.as_ref(), &reading, now_sec);
                    return RefreshOutcome {
                        reading: stamped(provider, id, reading, fetched_at, now_sec),
                        transient: None,
                    };
                }
                Err(failure) => {
                    self.remember_failure(source.as_ref(), &failure, trigger, now_sec);
                    let continues = source.falls_back(&failure);
                    if failure.attempted() {
                        last = Some((id, failure));
                    }
                    if !continues {
                        break;
                    }
                }
            }
        }
        let (source, failure) = last.unwrap_or_else(|| {
            let first = chain
                .first()
                .map_or(UsageSource::ClaudeOauthEndpoint, |source| source.id());
            (first, Failure::NotConfigured)
        });
        failed(provider, source, failure, fetched_at, now_sec)
    }

    fn held(&self, source: &UsageSource, trigger: Trigger, now_sec: u64) -> Option<Held> {
        if trigger == Trigger::Manual {
            return None;
        }
        let memory = self.memory.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(cooldown) = memory
            .cooldowns
            .iter()
            .find(|cooldown| &cooldown.source == source && cooldown.until_sec > now_sec)
        {
            return Some(Held::CoolingDown(cooldown.failure.clone()));
        }
        let chain_source = self
            .chains
            .iter()
            .flat_map(|(_, chain)| chain.iter())
            .find(|candidate| &candidate.id() == source)?;
        let window = chain_source.automatic_reuse_sec()?;
        memory
            .reuse
            .iter()
            .find(|reuse| &reuse.source == source)
            .filter(|reuse| now_sec.saturating_sub(reuse.read_at_sec) < window)
            .filter(|reuse| !reset_passed(&reuse.reading, now_sec))
            .map(|reuse| Held::Reused(reuse.reading.clone(), reuse.read_at_sec))
    }

    fn remember_success(&self, source: &dyn Source, reading: &ParsedReading, now_sec: u64) {
        let id = source.id();
        let mut memory = self.memory.lock().unwrap_or_else(PoisonError::into_inner);
        memory.cooldowns.retain(|cooldown| cooldown.source != id);
        memory.reuse.retain(|reuse| reuse.source != id);
        if source.automatic_reuse_sec().is_some() {
            memory.reuse.push(Reuse {
                source: id,
                read_at_sec: now_sec,
                reading: reading.clone(),
            });
        }
    }

    fn remember_failure(
        &self,
        source: &dyn Source,
        failure: &Failure,
        trigger: Trigger,
        now_sec: u64,
    ) {
        let id = source.id();
        let mut memory = self.memory.lock().unwrap_or_else(PoisonError::into_inner);
        // A cooldown that held this attempt back stays as it was.
        if trigger == Trigger::Automatic
            && memory
                .cooldowns
                .iter()
                .any(|cooldown| cooldown.source == id && cooldown.until_sec > now_sec)
        {
            return;
        }
        memory.cooldowns.retain(|cooldown| cooldown.source != id);
        if let Some(seconds) = source.automatic_cooldown_sec(failure) {
            memory.cooldowns.push(Cooldown {
                source: id,
                until_sec: now_sec.saturating_add(seconds),
                failure: failure.clone(),
            });
        }
    }
}

enum Held {
    Reused(ParsedReading, u64),
    CoolingDown(Failure),
}

/// A reused reading is stale as soon as one of its windows has reset.
fn reset_passed(reading: &ParsedReading, now_sec: u64) -> bool {
    [Some(&reading.primary), reading.secondary.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|window| DateTime::parse_from_rfc3339(&window.resets_at).ok())
        .any(|reset| reset.timestamp() <= i64::try_from(now_sec).unwrap_or(i64::MAX))
}

fn stamped(
    provider: &UsageProvider,
    source: UsageSource,
    reading: ParsedReading,
    fetched_at: &str,
    observed_at_sec: u64,
) -> Reading {
    Reading {
        provider: provider.clone(),
        state: UsageState::Ok,
        unavailable_reason: None,
        primary: Some(reading.primary),
        secondary: reading.secondary,
        source,
        fetched_at: fetched_at.to_owned(),
        observed_at_sec,
    }
}

fn available(
    provider: &UsageProvider,
    source: UsageSource,
    reading: ParsedReading,
    read_at_sec: u64,
) -> RefreshOutcome {
    let fetched_at =
        sources::epoch_text(i64::try_from(read_at_sec).unwrap_or(0)).unwrap_or_default();
    RefreshOutcome {
        reading: stamped(provider, source, reading, &fetched_at, read_at_sec),
        transient: None,
    }
}

fn failed(
    provider: &UsageProvider,
    source: UsageSource,
    failure: Failure,
    fetched_at: &str,
    observed_at_sec: u64,
) -> RefreshOutcome {
    let (state, reason, transient) = public_failure(&source, failure);
    RefreshOutcome {
        reading: Reading {
            provider: provider.clone(),
            state,
            unavailable_reason: reason,
            primary: None,
            secondary: None,
            source,
            fetched_at: fetched_at.to_owned(),
            observed_at_sec,
        },
        transient,
    }
}

fn public_failure(
    source: &UsageSource,
    failure: Failure,
) -> (
    UsageState,
    Option<UsageUnavailableReason>,
    Option<TransientFailure>,
) {
    use UsageUnavailableReason as Reason;
    match failure {
        Failure::NotConfigured | Failure::LoginNeeded => {
            (UsageState::LoginNeeded, Some(Reason::LoginNeeded), None)
        }
        Failure::NoAccess => (UsageState::NoAccess, Some(Reason::NoAccess), None),
        Failure::ToolNotFound => (UsageState::Unavailable, Some(Reason::ToolNotFound), None),
        Failure::RateLimited { retry_after_sec } => (
            UsageState::Unavailable,
            Some(Reason::RateLimited),
            Some(TransientFailure::RateLimited {
                retry_after_sec: retry_after_sec.unwrap_or(0),
            }),
        ),
        Failure::Network => (
            UsageState::Unavailable,
            Some(Reason::NetworkError),
            Some(TransientFailure::Network),
        ),
        Failure::InvalidResponse => (UsageState::Unavailable, Some(Reason::InvalidResponse), None),
        Failure::Failed if *source == UsageSource::CodexAppServer => {
            (UsageState::Unavailable, Some(Reason::AppServerFailed), None)
        }
        Failure::Failed => (UsageState::Unavailable, Some(Reason::SourceFailed), None),
    }
}
