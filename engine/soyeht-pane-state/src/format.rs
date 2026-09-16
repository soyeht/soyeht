use crate::{
    Error, GroundTruthLabel, HarnessIdentity, ObservationFrame, ObservationGap,
    ObservationPaneKind, RecordedRow, Result,
    model::{bounded_nonempty, valid_timestamp},
};
use serde::{Deserialize, Serialize};

pub const DATASET_SCHEMA_VERSION: u32 = 6;
pub const MAX_DATASET_ROW_BYTES: usize = 256 * 1024;
pub const MAX_SEGMENT_BYTES: usize = 1024 * 1024;
pub const MAX_RETAINED_SEGMENTS: usize = 8;
const MAX_ENGINE_VERSION_BYTES: usize = 128;
const SHA256_HEX_BYTES: usize = 64;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SegmentSchemaProbe {
    kind: String,
    schema_version: u32,
}

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
    pub pane_kind: ObservationPaneKind,
    pub harness: HarnessIdentity,
    pub engine: EngineBuild,
    pub segment_sequence: u64,
    pub started_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SegmentEnd {
    pub pane_id: String,
    pub pane_kind: ObservationPaneKind,
    pub harness: HarnessIdentity,
    pub ended_at_ms: i64,
    pub last_sequence: u64,
}

impl SegmentEnd {
    pub fn validate(&self) -> Result<()> {
        bounded_nonempty(&self.pane_id, crate::MAX_PANE_ID_BYTES, "pane id")?;
        self.harness.validate()?;
        valid_timestamp(self.ended_at_ms)
    }
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
    SegmentEnd(SegmentEnd),
}

impl DatasetRow {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Metadata(metadata) => metadata.validate(),
            Self::Observation(frame) => frame.validate(),
            Self::Label(label) => label.validate(),
            Self::Gap(gap) => gap.validate(),
            Self::SegmentEnd(end) => end.validate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplayedSegment {
    pub metadata: SegmentMetadata,
    pub rows: Vec<RecordedRow>,
    pub clean_close: Option<SegmentEnd>,
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
    let mut clean_close = None;
    for line in lines {
        let row = decode_line(line)?;
        if clean_close.is_some() {
            return Err(Error::InvalidSegmentEnd);
        }
        let (pane_id, pane_kind, harness, sequence, recorded) = match row {
            DatasetRow::Metadata(_) => return Err(Error::InvalidHeader),
            DatasetRow::Observation(frame) => (
                frame.pane_id.clone(),
                frame.pane_kind,
                frame.harness.clone(),
                frame.sequence,
                RecordedRow::Observation(frame),
            ),
            DatasetRow::Label(label) => (
                label.pane_id.clone(),
                label.pane_kind,
                label.harness.clone(),
                label.sequence,
                RecordedRow::Label(label),
            ),
            DatasetRow::Gap(gap) => (
                gap.pane_id.clone(),
                gap.pane_kind,
                gap.harness.clone(),
                gap.last_sequence,
                RecordedRow::Gap(gap),
            ),
            DatasetRow::SegmentEnd(end) => {
                end.validate()?;
                if end.pane_id != metadata.pane_id
                    || end.pane_kind != metadata.pane_kind
                    || end.harness != metadata.harness
                    || last_sequence != Some(end.last_sequence)
                {
                    return Err(Error::InvalidSegmentEnd);
                }
                clean_close = Some(end);
                continue;
            }
        };
        recorded.validate()?;
        if pane_id != metadata.pane_id
            || pane_kind != metadata.pane_kind
            || harness != metadata.harness
        {
            return Err(Error::MismatchedProvenance);
        }
        if last_sequence.is_some_and(|previous| sequence <= previous) {
            return Err(Error::InvalidSequence);
        }
        last_sequence = Some(sequence);
        rows.push(recorded);
    }
    Ok(ReplayedSegment {
        metadata,
        rows,
        clean_close,
    })
}

pub fn segment_schema_version(bytes: &[u8]) -> Result<u32> {
    if bytes.len() > MAX_SEGMENT_BYTES {
        return Err(Error::Limit("dataset segment"));
    }
    let newline = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or(Error::IncompleteRecord)?;
    if newline == 0 {
        return Err(Error::InvalidHeader);
    }
    let first_line = &bytes[..newline];
    if first_line.len().saturating_add(1) > MAX_DATASET_ROW_BYTES {
        return Err(Error::Limit("dataset row"));
    }
    let probe: SegmentSchemaProbe = serde_json::from_slice(first_line).map_err(|_| Error::Json)?;
    if probe.kind != "metadata" || probe.schema_version == 0 {
        return Err(Error::InvalidHeader);
    }
    Ok(probe.schema_version)
}

fn decode_line(bytes: &[u8]) -> Result<DatasetRow> {
    if bytes.is_empty() || bytes.len() + 1 > MAX_DATASET_ROW_BYTES {
        return Err(Error::Limit("dataset row"));
    }
    serde_json::from_slice(bytes).map_err(|_| Error::Json)
}
