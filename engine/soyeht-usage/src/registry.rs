use crate::providers::{ProviderReader, readers};
use crate::{Reading, SourceRead, UsageSourcePort};
use soyeht_protocol::{UsageProvider, UsageSource, UsageState};

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
                Err(_) => choose_failure(&mut failure, source.clone(), SourceRead::Unavailable),
            },
            outcome => choose_failure(&mut failure, source.clone(), outcome),
        }
    }
    let (source, state, transient) =
        failure.unwrap_or((first_source, UsageState::Unavailable, None));
    Ok(unavailable(
        reader.provider(),
        source,
        state,
        fetched_at,
        observed_at_sec,
        transient,
    ))
}

fn choose_failure(
    selected: &mut Option<(UsageSource, UsageState, Option<TransientFailure>)>,
    source: UsageSource,
    outcome: SourceRead,
) {
    let candidate = failure_candidate(source, outcome);
    let replace = selected
        .as_ref()
        .is_none_or(|current| failure_rank(&candidate) > failure_rank(current));
    if replace {
        *selected = Some(candidate);
    }
}

fn failure_candidate(
    source: UsageSource,
    outcome: SourceRead,
) -> (UsageSource, UsageState, Option<TransientFailure>) {
    match outcome {
        SourceRead::NoAccess => (source, UsageState::NoAccess, None),
        SourceRead::LoginNeeded => (source, UsageState::LoginNeeded, None),
        SourceRead::RateLimited { retry_after_sec } => (
            source,
            UsageState::Unavailable,
            Some(TransientFailure::RateLimited { retry_after_sec }),
        ),
        SourceRead::NetworkError => (
            source,
            UsageState::Unavailable,
            Some(TransientFailure::Network),
        ),
        SourceRead::Payload(_) | SourceRead::Unavailable => (source, UsageState::Unavailable, None),
    }
}

fn failure_rank(failure: &(UsageSource, UsageState, Option<TransientFailure>)) -> (u8, u8) {
    let state = match &failure.1 {
        UsageState::NoAccess => 3,
        UsageState::LoginNeeded => 2,
        UsageState::Unavailable | UsageState::Stale | UsageState::Ok => 1,
    };
    let transient = match &failure.2 {
        Some(TransientFailure::RateLimited { .. }) => 2,
        Some(TransientFailure::Network) => 1,
        None => 0,
    };
    (state, transient)
}

fn unavailable(
    provider: UsageProvider,
    source: UsageSource,
    state: UsageState,
    fetched_at: String,
    observed_at_sec: u64,
    transient: Option<TransientFailure>,
) -> RefreshOutcome {
    RefreshOutcome {
        reading: Reading::unavailable(provider, state, source, fetched_at, observed_at_sec),
        transient,
    }
}
