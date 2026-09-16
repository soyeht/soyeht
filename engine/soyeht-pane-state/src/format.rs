use crate::{
    Error, GroundTruthLabel, HarnessIdentity, ObservationFrame, ObservationGap, RecordedRow,
    Result,
    model::{bounded_nonempty, valid_timestamp},
};
use serde::{Deserialize, Serialize};

pub const DATASET_SCHEMA_VERSION: u32 = 4;
pub const MAX_DATASET_ROW_BYTES: usize = 256 * 1024;
pub const MAX_SEGMENT_BYTES: usize = 1024 * 1024;
pub const MAX_RETAINED_SEGMENTS: usize = 8;
const MAX_ENGINE_VERSION_BYTES: usize = 128;
const SHA256_HEX_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EngineBuild {
    pub package_version: String,
    pub executable_sha256: String,
}

impl EngineBuild {
    pub fn validate(&self) -> Result<()> {
        bounded_nonempty(
            &self.package_version,
            MAX_ENGINE_VERSION_BYTES,
            "engine package version",
        )?;
        if self.executable_sha256.len() != SHA256_HEX_BYTES
            || !self
                .executable_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::InvalidDigest);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SegmentMetadata {
    pub schema_version: u32,
    pub pane_id: String,
    pub harness: HarnessIdentity,
    pub engine: EngineBuild,
    pub segment_sequence: u64,
    pub started_at_ms: i64,
}

impl SegmentMetadata {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != DATASET_SCHEMA_VERSION {
            return Err(Error::SchemaVersion);
        }
        bounded_nonempty(&self.pane_id, crate::MAX_PANE_ID_BYTES, "pane id")?;
        self.harness.validate()?;
        self.engine.validate()?;
        valid_timestamp(self.started_at_ms)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub enum DatasetRow {
    Metadata(SegmentMetadata),
    Observation(ObservationFrame),
    Label(GroundTruthLabel),
    Gap(ObservationGap),
}

impl DatasetRow {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Metadata(metadata) => metadata.validate(),
            Self::Observation(frame) => frame.validate(),
            Self::Label(label) => label.validate(),
            Self::Gap(gap) => gap.validate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplayedSegment {
    pub metadata: SegmentMetadata,
    pub rows: Vec<RecordedRow>,
}

pub fn encode_row(row: &DatasetRow) -> Result<Vec<u8>> {
    row.validate()?;
    let mut bytes = serde_json::to_vec(row).map_err(|_| Error::Json)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_DATASET_ROW_BYTES {
        return Err(Error::Limit("dataset row"));
    }
    Ok(bytes)
}

pub fn decode_segment(bytes: &[u8]) -> Result<ReplayedSegment> {
    if bytes.len() > MAX_SEGMENT_BYTES {
        return Err(Error::Limit("dataset segment"));
    }
    if bytes.is_empty() || !bytes.ends_with(b"\n") {
        return Err(Error::IncompleteRecord);
    }
    let mut lines = bytes[..bytes.len() - 1].split(|byte| *byte == b'\n');
    let header = decode_line(lines.next().ok_or(Error::InvalidHeader)?)?;
    let DatasetRow::Metadata(metadata) = header else {
        return Err(Error::InvalidHeader);
    };
    metadata.validate()?;
    let mut last_sequence = None;
    let mut rows = Vec::new();
    for line in lines {
        let row = decode_line(line)?;
        let (pane_id, harness, sequence, recorded) = match row {
            DatasetRow::Metadata(_) => return Err(Error::InvalidHeader),
            DatasetRow::Observation(frame) => (
                frame.pane_id.clone(),
                frame.harness.clone(),
                frame.sequence,
                RecordedRow::Observation(frame),
            ),
            DatasetRow::Label(label) => (
                label.pane_id.clone(),
                label.harness.clone(),
                label.sequence,
                RecordedRow::Label(label),
            ),
            DatasetRow::Gap(gap) => (
                gap.pane_id.clone(),
                gap.harness.clone(),
                gap.last_sequence,
                RecordedRow::Gap(gap),
            ),
        };
        recorded.validate()?;
        if pane_id != metadata.pane_id || harness != metadata.harness {
            return Err(Error::MismatchedProvenance);
        }
        if last_sequence.is_some_and(|previous| sequence <= previous) {
            return Err(Error::InvalidSequence);
        }
        last_sequence = Some(sequence);
        rows.push(recorded);
    }
    Ok(ReplayedSegment { metadata, rows })
}

fn decode_line(bytes: &[u8]) -> Result<DatasetRow> {
    if bytes.is_empty() || bytes.len() + 1 > MAX_DATASET_ROW_BYTES {
        return Err(Error::Limit("dataset row"));
    }
    serde_json::from_slice(bytes).map_err(|_| Error::Json)
}
