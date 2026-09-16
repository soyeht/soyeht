use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use soyeht_protocol::{PaneState, StateSource};

pub const MAX_PANE_ID_BYTES: usize = 128;
pub const MAX_HARNESS_ID_BYTES: usize = 256;
pub const MAX_HARNESS_VERSION_BYTES: usize = 128;
pub const MAX_TITLE_BYTES: usize = 512;
pub const MAX_SCREEN_LINES: usize = 64;
pub const MAX_SCREEN_LINE_BYTES: usize = 2_048;
pub const MAX_FOREGROUND_PROCESS_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "identity", rename_all = "snake_case")]
pub enum HarnessIdentity {
    Known { id: String, version: Option<String> },
    Unknown,
}

impl HarnessIdentity {
    pub fn known(id: impl Into<String>, version: Option<String>) -> Result<Self> {
        let identity = Self::Known {
            id: id.into(),
            version,
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Known { id, .. } => Some(id),
            Self::Unknown => None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Known { id, version } => {
                bounded_nonempty(id, MAX_HARNESS_ID_BYTES, "harness id")?;
                if let Some(version) = version {
                    bounded_nonempty(version, MAX_HARNESS_VERSION_BYTES, "harness version")?;
                }
                Ok(())
            }
            Self::Unknown => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorPosition {
    pub row: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationReason {
    VisualChange,
    StaticActivity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationFrame {
    pub pane_id: String,
    pub harness: HarnessIdentity,
    pub timestamp_ms: i64,
    pub sequence: u64,
    pub reason: ObservationReason,
    pub osc_title: Option<String>,
    pub lines: Vec<String>,
    pub cursor: Option<CursorPosition>,
    pub foreground_process: Option<String>,
    pub bytes_per_second: u64,
}

impl ObservationFrame {
    pub fn validate(&self) -> Result<()> {
        bounded_nonempty(&self.pane_id, MAX_PANE_ID_BYTES, "pane id")?;
        self.harness.validate()?;
        valid_timestamp(self.timestamp_ms)?;
        bounded_optional(&self.osc_title, MAX_TITLE_BYTES, "OSC title")?;
        if self.lines.len() > MAX_SCREEN_LINES {
            return Err(Error::Limit("screen lines"));
        }
        if self
            .lines
            .iter()
            .any(|line| line.len() > MAX_SCREEN_LINE_BYTES)
        {
            return Err(Error::Limit("screen line"));
        }
        if self.lines.iter().any(|line| line.contains(['\r', '\n'])) {
            return Err(Error::InvalidScreenLine);
        }
        bounded_optional(
            &self.foreground_process,
            MAX_FOREGROUND_PROCESS_BYTES,
            "foreground process",
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroundTruthLabel {
    pub pane_id: String,
    pub harness: HarnessIdentity,
    pub timestamp_ms: i64,
    pub sequence: u64,
    pub state: PaneState,
    pub source: StateSource,
}

impl GroundTruthLabel {
    pub fn validate(&self) -> Result<()> {
        bounded_nonempty(&self.pane_id, MAX_PANE_ID_BYTES, "pane id")?;
        self.harness.validate()?;
        valid_timestamp(self.timestamp_ms)?;
        if !matches!(self.source, StateSource::Protocol | StateSource::Hook) {
            return Err(Error::InvalidLabelSource);
        }
        if self.state == PaneState::Unknown {
            return Err(Error::InvalidLabelState);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationGap {
    pub pane_id: String,
    pub harness: HarnessIdentity,
    pub first_timestamp_ms: i64,
    pub last_timestamp_ms: i64,
    pub first_sequence: u64,
    pub last_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped_observations: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped_bytes: Option<u64>,
}

impl ObservationGap {
    pub fn validate(&self) -> Result<()> {
        bounded_nonempty(&self.pane_id, MAX_PANE_ID_BYTES, "pane id")?;
        self.harness.validate()?;
        valid_timestamp(self.first_timestamp_ms)?;
        valid_timestamp(self.last_timestamp_ms)?;
        let sequence_span = self
            .last_sequence
            .checked_sub(self.first_sequence)
            .and_then(|span| span.checked_add(1));
        let valid_observation_loss = self
            .dropped_observations
            .is_none_or(|count| count > 0 && sequence_span.is_some_and(|span| count <= span));
        let valid_byte_loss = self.dropped_bytes.is_none_or(|count| count > 0);
        let has_loss = self.dropped_observations.is_some() || self.dropped_bytes.is_some();
        if self.last_sequence < self.first_sequence
            || !has_loss
            || !valid_observation_loss
            || !valid_byte_loss
        {
            return Err(Error::InvalidGap);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordedRow {
    Observation(ObservationFrame),
    Label(GroundTruthLabel),
    Gap(ObservationGap),
}

impl RecordedRow {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Observation(frame) => frame.validate(),
            Self::Label(label) => label.validate(),
            Self::Gap(gap) => gap.validate(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Confidence(u16);

impl Confidence {
    pub const UNKNOWN: Self = Self(0);

    pub fn from_basis_points(value: u16) -> Result<Self> {
        if value > 10_000 {
            return Err(Error::InvalidConfidence);
        }
        Ok(Self(value))
    }

    pub fn as_f64(self) -> f64 {
        f64::from(self.0) / 10_000.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub state: PaneState,
    pub source: StateSource,
    pub confidence: Confidence,
    pub rule: Option<&'static str>,
}

impl Detection {
    pub fn unknown() -> Self {
        Self {
            state: PaneState::Unknown,
            source: StateSource::None,
            confidence: Confidence::UNKNOWN,
            rule: None,
        }
    }
}

/// Replaceable pure boundary between an observation and one state claim.
pub trait Detector {
    fn state(&self, frame: &ObservationFrame) -> Detection;
}

pub(crate) fn bounded_nonempty(value: &str, limit: usize, field: &'static str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::Empty(field));
    }
    if value.len() > limit {
        return Err(Error::Limit(field));
    }
    Ok(())
}

fn bounded_optional(value: &Option<String>, limit: usize, field: &'static str) -> Result<()> {
    if let Some(value) = value {
        bounded_nonempty(value, limit, field)?;
    }
    Ok(())
}

pub(crate) fn valid_timestamp(value: i64) -> Result<()> {
    if value < 0 {
        return Err(Error::InvalidTimestamp);
    }
    Ok(())
}
