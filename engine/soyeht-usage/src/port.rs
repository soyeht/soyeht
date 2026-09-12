use soyeht_protocol::{UsageProvider, UsageSource};

#[derive(Clone, PartialEq)]
pub enum SourceRead {
    Payload(Vec<u8>),
    LoginNeeded,
    NoAccess,
    Unavailable,
    NetworkError,
    RateLimited { retry_after_sec: u32 },
}

pub trait UsageSourcePort {
    fn read(&mut self, provider: UsageProvider, source: UsageSource) -> SourceRead;
}
