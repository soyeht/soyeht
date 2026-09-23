use crate::providers::{ProviderReader, readers};
use crate::{Reading, SourceRead, UsageSourcePort};
use soyeht_protocol::{UsageProvider, UsageSource, UsageState, UsageUnavailableReason};

#[derive(Debug, Clone, PartialEq)]
pub enum TransientFailure {
    Network,
    RateLimited { retry_after_sec: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefreshOutcome {
    pub reading: Reading,
    pub transient: Option<TransientFailure>,
}

struct ReadFailure {
    source: UsageSource,
    state: UsageState,
    transient: Option<TransientFailure>,
    reason: Option<UsageUnavailableReason>,
}

pub struct ReaderRegistry {
    readers: Vec<Box<dyn ProviderReader>>,
}

impl Default for ReaderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ReaderRegistry {
    pub fn new() -> Self {
        Self { readers: readers() }
    }

    pub fn providers(&self) -> Vec<UsageProvider> {
        self.readers
            .iter()
            .map(|reader| reader.provider())
            .collect()
    }

    pub fn sources(&self, provider: UsageProvider) -> Vec<UsageSource> {
        self.reader(provider)
            .map(|reader| reader.sources().to_vec())
            .unwrap_or_default()
    }

    pub fn refresh(
        &self,
        provider: UsageProvider,
        port: &mut dyn UsageSourcePort,
        fetched_at: impl Into<String>,
        observed_at_sec: u64,
    ) -> crate::Result<RefreshOutcome> {
        let fetched_at = fetched_at.into();
        let reader = self
            .reader(provider.clone())
            .ok_or(crate::Error::ReaderMissing { provider })?;
        refresh_reader(reader, port, fetched_at, observed_at_sec)
    }

    fn reader(&self, provider: UsageProvider) -> Option<&dyn ProviderReader> {
        self.readers
            .iter()
            .find(|reader| reader.provider() == provider)
            .map(Box::as_ref)
    }
}

fn refresh_reader(
    reader: &dyn ProviderReader,
    port: &mut dyn UsageSourcePort,
    fetched_at: String,
    observed_at_sec: u64,
) -> crate::Result<RefreshOutcome> {
    let first_source =
        reader
            .sources()
            .first()
            .cloned()
            .ok_or_else(|| crate::Error::ReaderHasNoSource {
                provider: reader.provider(),
            })?;
    let mut failure = None;
    for source in reader.sources() {
        match port.read(reader.provider(), source.clone()) {
            SourceRead::Payload(payload) => match reader.parse(source.clone(), &payload) {
                Ok(parsed) => {
                    let fetched_at = parsed.fetched_at.unwrap_or(fetched_at);
                    let observed_at_sec = parsed.observed_at_sec.unwrap_or(observed_at_sec);
                    return Ok(RefreshOutcome {
                        reading: Reading::available(
                            reader.provider(),
                            parsed.primary,
                            parsed.secondary,
                            source.clone(),
                            fetched_at,
                            observed_at_sec,
                        ),
                        transient: None,
                    });
                }
                Err(_) => choose_failure(
                    &mut failure,
                    source.clone(),
                    SourceRead::Unavailable(Some(UsageUnavailableReason::InvalidResponse)),
                ),
            },
            outcome => choose_failure(&mut failure, source.clone(), outcome),
        }
    }
    let failure = failure.unwrap_or(ReadFailure {
        source: first_source,
        state: UsageState::Unavailable,
        transient: None,
        reason: None,
    });
    Ok(RefreshOutcome {
        reading: Reading::unavailable(
            reader.provider(),
            failure.state,
            failure.reason,
            failure.source,
            fetched_at,
            observed_at_sec,
        ),
        transient: failure.transient,
    })
}

fn choose_failure(selected: &mut Option<ReadFailure>, source: UsageSource, outcome: SourceRead) {
    let candidate = failure_candidate(source, outcome);
    let replace = selected
        .as_ref()
        .is_none_or(|current| failure_rank(&candidate) > failure_rank(current));
    if replace {
        *selected = Some(candidate);
    }
}

fn failure_candidate(source: UsageSource, outcome: SourceRead) -> ReadFailure {
    let (state, transient, reason) = match outcome {
        SourceRead::NoAccess => (UsageState::NoAccess, None, None),
        SourceRead::LoginNeeded => (UsageState::LoginNeeded, None, None),
        SourceRead::RateLimited { retry_after_sec } => (
            UsageState::Unavailable,
            Some(TransientFailure::RateLimited { retry_after_sec }),
            None,
        ),
        SourceRead::NetworkError => (
            UsageState::Unavailable,
            Some(TransientFailure::Network),
            None,
        ),
        SourceRead::Unavailable(reason) => (UsageState::Unavailable, None, reason),
        SourceRead::Payload(_) => (UsageState::Unavailable, None, None),
    };
    ReadFailure {
        source,
        state,
        transient,
        reason,
    }
}

fn failure_rank(failure: &ReadFailure) -> (u8, u8, u8) {
    let state = match &failure.state {
        UsageState::NoAccess => 3,
        UsageState::LoginNeeded => 2,
        UsageState::Unavailable | UsageState::Stale | UsageState::Ok => 1,
    };
    let transient = match &failure.transient {
        Some(TransientFailure::RateLimited { .. }) => 2,
        Some(TransientFailure::Network) => 1,
        None => 0,
    };
    (state, transient, u8::from(failure.reason.is_some()))
}
