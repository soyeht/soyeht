use soyeht_protocol::{UsageProvider, UsageSource, UsageState};

#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub used: f64,
    pub resets_at: String,
    pub label: String,
}

impl Window {
    pub(crate) fn checked(
        used: f64,
        resets_at: impl Into<String>,
        label: impl Into<String>,
        source: UsageSource,
    ) -> crate::Result<Self> {
        let resets_at = resets_at.into();
        let label = label.into();
        if !(0.0..=1.0).contains(&used)
            || resets_at.is_empty()
            || resets_at.len() > 128
            || label.is_empty()
            || label.len() > 128
        {
            return Err(crate::Error::InvalidWindow { source });
        }
        Ok(Self {
            used,
            resets_at,
            label,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub provider: UsageProvider,
    pub state: UsageState,
    pub primary: Option<Window>,
    pub secondary: Option<Window>,
    pub source: UsageSource,
    pub fetched_at: String,
    pub observed_at_sec: u64,
}

impl Reading {
    pub(crate) fn available(
        provider: UsageProvider,
        primary: Window,
        secondary: Option<Window>,
        source: UsageSource,
        fetched_at: String,
        observed_at_sec: u64,
    ) -> Self {
        Self {
            provider,
            state: UsageState::Ok,
            primary: Some(primary),
            secondary,
            source,
            fetched_at,
            observed_at_sec,
        }
    }

    pub(crate) fn unavailable(
        provider: UsageProvider,
        state: UsageState,
        source: UsageSource,
        fetched_at: String,
        observed_at_sec: u64,
    ) -> Self {
        Self {
            provider,
            state,
            primary: None,
            secondary: None,
            source,
            fetched_at,
            observed_at_sec,
        }
    }

    pub fn for_display(&self, now_sec: u64) -> Self {
        let mut reading = self.clone();
        if self.state == UsageState::Ok
            && now_sec.saturating_sub(self.observed_at_sec)
                > soyeht_protocol::limits::USAGE_STALE_AFTER_SEC as u64
        {
            reading.state = UsageState::Stale;
        }
        reading
    }

    pub fn restored_from_store(&self) -> Self {
        let mut reading = self.clone();
        if reading.primary.is_some() {
            reading.state = UsageState::Stale;
        }
        reading
    }
}
