use soyeht_protocol::{UsageProvider, UsageSource, UsageState, UsageUnavailableReason};

/// The longest reset time or label the wire accepts.
const MAX_TEXT: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub used: f64,
    /// RFC 3339 in UTC, or empty when the provider did not say.
    pub resets_at: String,
    pub label: String,
}

impl Window {
    pub(crate) fn checked(
        used: f64,
        resets_at: impl Into<String>,
        label: impl Into<String>,
    ) -> Option<Self> {
        let resets_at = resets_at.into();
        let label = label.into();
        ((0.0..=1.0).contains(&used)
            && resets_at.len() <= MAX_TEXT
            && !label.is_empty()
            && label.len() <= MAX_TEXT)
            .then_some(Self {
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
    /// Why there is no reading, or, with a stale reading, why the latest refresh failed.
    pub unavailable_reason: Option<UsageUnavailableReason>,
    pub primary: Option<Window>,
    pub secondary: Option<Window>,
    pub source: UsageSource,
    pub fetched_at: String,
    pub observed_at_sec: u64,
}

impl Reading {
    pub fn has_windows(&self) -> bool {
        self.primary.is_some()
    }

    /// A reading older than the freshness window shows as last known.
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

    /// A reading restored after a restart is last known until it is read again.
    pub fn restored_from_store(&self) -> Self {
        let mut reading = self.clone();
        if reading.has_windows() {
            reading.state = UsageState::Stale;
        }
        reading
    }

    /// The last good reading kept, marked last known, after a refresh that failed with
    /// `reason`. Its values, source and age stay those of the last success.
    pub fn kept_after_failure(&self, reason: Option<UsageUnavailableReason>) -> Self {
        let mut reading = self.clone();
        reading.state = UsageState::Stale;
        reading.unavailable_reason = reason;
        reading
    }
}
