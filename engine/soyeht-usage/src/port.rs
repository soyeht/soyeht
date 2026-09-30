use soyeht_protocol::{UsageProvider, UsageSource, UsageUnavailableReason};

#[derive(Clone, PartialEq)]
pub enum SourceRead {
    Payload(Vec<u8>),
    LoginNeeded,
    NoAccess,
    Unavailable(Option<UsageUnavailableReason>),
    NetworkError,
    RateLimited { retry_after_sec: u32 },
}

pub trait UsageSourcePort: Send {
    fn read(&mut self, provider: UsageProvider, source: UsageSource) -> SourceRead;
}
