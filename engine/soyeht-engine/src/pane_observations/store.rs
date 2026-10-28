use crate::errors::{self, Result};
use sha2::{Digest, Sha256};
use soyeht_pane_state::{
    DATASET_SCHEMA_VERSION, DatasetRow, EngineBuild, HarnessIdentity, MAX_RETAINED_SEGMENTS,
    MAX_SEGMENT_BYTES, ObservationPaneKind, RecordedRow, ReplayedSegment, SegmentEnd,
    SegmentMetadata, decode_segment, encode_row, segment_schema_version,
};
use soyeht_platform::fs::{
    OpenOptionsPrivateExt, create_private_dir, dir_is_owner_only, file_is_owner_only,
    sync_directory,
};
use std::{
    collections::VecDeque,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const SEGMENT_SUFFIX: &str = ".jsonl";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeletionReceipt {
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkippedSegment {
    pub pane_id: String,
    pub schema_version: u32,
}

pub(crate) struct PaneReplay {
    pub segments: Vec<ReplayedSegment>,
    pub skipped_segments: Vec<SkippedSegment>,
}

enum SegmentRead {
    Current(Box<ReplayedSegment>),
    UnsupportedSchema(u32),
}

#[derive(Debug)]
struct Segment {
    number: u64,
    size: usize,
    metadata: SegmentMetadata,
    appendable: bool,
    last_timestamp_ms: i64,
}

pub struct SegmentStore {
    root: PathBuf,
    directory: PathBuf,
    pub(super) pane_id: String,
    pub(super) pane_kind: ObservationPaneKind,
    pub(super) harness: HarnessIdentity,
    engine: EngineBuild,
    segments: VecDeque<Segment>,
    last_sequence: Option<u64>,
    next_segment_number: u64,
}

impl SegmentStore {
    pub fn open(
        root: &Path,
        pane_id: impl Into<String>,
        pane_kind: ObservationPaneKind,
        harness: HarnessIdentity,
        engine: EngineBuild,
    ) -> Result<Self> {
        let pane_id = pane_id.into();
        validate_identity(&pane_id, pane_kind, &harness, &engine)?;
        ensure_private_directory(root)?;
        let directory = root.join(pane_directory_name(&pane_id));
        ensure_private_directory(&directory)?;
        let (segments, last_sequence, next_segment_number) = load_segments(&directory, &pane_id)?;
        Ok(Self {
            root: root.into(),
            directory,
            pane_id,
            pane_kind,
            harness,
            engine,
            segments,
            last_sequence,
            next_segment_number,
        })
    }

    pub fn append(&mut self, row: RecordedRow) -> Result<()> {
        self.append_batch([row])
    }

    pub fn append_batch(&mut self, rows: impl IntoIterator<Item = RecordedRow>) -> Result<()> {
        let mut wrote = false;
        for row in rows {
            row.validate()?;
            let (pane_id, pane_kind, harness, timestamp_ms, sequence) = row_identity(&row);
            if pane_id != self.pane_id || pane_kind != self.pane_kind || harness != &self.harness {
                return Err(errors::invalid(
                    "Observation row provenance does not match its pane",
                ));
            }
            if self.last_sequence.is_some_and(|last| sequence <= last) {
                return Err(soyeht_pane_state::Error::InvalidSequence.into());
            }
            let encoded = encode_recorded_row(&row)?;
            let footer = self.encoded_segment_end(timestamp_ms, sequence)?;
            if self.needs_rotation(encoded.len(), footer.len()) {
                self.close_current()?;
                self.create_segment(timestamp_ms)?;
            }
            self.append_bytes(&encoded)?;
            let segment = self
                .segments
                .back_mut()
                .ok_or_else(|| errors::internal("Observation segment is missing"))?;
            segment.last_timestamp_ms = timestamp_ms;
            self.last_sequence = Some(sequence);
            wrote = true;
        }
        if wrote {
            self.sync_current()?;
            self.enforce_retention()?;
        }
        Ok(())
    }

    pub fn next_sequence(&self) -> Result<u64> {
        self.last_sequence
            .map(|sequence| {
                sequence
                    .checked_add(1)
                    .ok_or_else(|| errors::internal("Observation sequence exhausted"))
            })
            .transpose()
            .map(|next| next.unwrap_or(0))
    }

    pub fn replay(&self) -> Result<Vec<ReplayedSegment>> {
        self.segments
            .iter()
            .map(|segment| read_segment(&self.directory, segment.number))
            .collect()
    }

    pub fn delete(self) -> Result<DeletionReceipt> {
        let root = self.root.clone();
        let pane_id = self.pane_id.clone();
        drop(self);
        delete_pane(&root, &pane_id)
    }

    pub fn close(&mut self) -> Result<()> {
        self.close_current()
    }

    fn needs_rotation(&self, row_bytes: usize, footer_bytes: usize) -> bool {
        self.segments.back().is_none_or(|segment| {
            !segment.appendable
                || segment.metadata.pane_kind != self.pane_kind
                || segment.metadata.harness != self.harness
                || segment.metadata.engine != self.engine
                || segment
                    .size
                    .saturating_add(row_bytes)
                    .saturating_add(footer_bytes)
                    > MAX_SEGMENT_BYTES
        })
    }

    fn encoded_segment_end(&self, ended_at_ms: i64, last_sequence: u64) -> Result<Vec<u8>> {
        Ok(encode_row(&DatasetRow::SegmentEnd(SegmentEnd {
            pane_id: self.pane_id.clone(),
            pane_kind: self.pane_kind,
            harness: self.harness.clone(),
            ended_at_ms,
            last_sequence,
        }))?)
    }

    fn close_current(&mut self) -> Result<()> {
        let Some(segment) = self.segments.back() else {
            return Ok(());
        };
        if !segment.appendable {
            return Ok(());
        }
        let last_sequence = self
            .last_sequence
            .ok_or_else(|| errors::internal("Observation segment has no final sequence"))?;
        let footer = self.encoded_segment_end(segment.last_timestamp_ms, last_sequence)?;
        self.append_bytes(&footer)?;
        self.sync_current()?;
        self.segments
            .back_mut()
            .ok_or_else(|| errors::internal("Observation segment disappeared"))?
            .appendable = false;
        Ok(())
    }

    fn create_segment(&mut self, started_at_ms: i64) -> Result<()> {
        let number = self.next_segment_number;
        let next_segment_number = number
            .checked_add(1)
            .ok_or_else(|| errors::internal("Observation segment sequence exhausted"))?;
        let metadata = SegmentMetadata {
            schema_version: DATASET_SCHEMA_VERSION,
            pane_id: self.pane_id.clone(),
            pane_kind: self.pane_kind,
            harness: self.harness.clone(),
            engine: self.engine.clone(),
            segment_sequence: number,
            started_at_ms,
        };
        let header = encode_row(&DatasetRow::Metadata(metadata.clone()))?;
        if header.len() >= MAX_SEGMENT_BYTES {
            return Err(errors::internal(
                "Observation segment header exceeds its bound",
            ));
        }
        create_segment_file(&self.directory, number, &header)?;
        self.next_segment_number = next_segment_number;
        self.segments.push_back(Segment {
            number,
            size: header.len(),
            metadata,
            appendable: true,
            last_timestamp_ms: started_at_ms,
        });
        Ok(())
    }

    fn append_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let segment = self
            .segments
            .back_mut()
            .ok_or_else(|| errors::internal("Observation segment is missing"))?;
        let path = self.directory.join(segment_filename(segment.number));
        let mut file = OpenOptions::new().append(true).open_no_follow(&path)?;
        validate_private_file(&file)?;
        let actual = usize::try_from(file.metadata()?.len())
            .map_err(|_| errors::internal("Observation segment size is invalid"))?;
        if actual != segment.size || actual.saturating_add(bytes.len()) > MAX_SEGMENT_BYTES {
            return Err(errors::internal(
                "Observation segment changed outside the recorder",
            ));
        }
        file.write_all(bytes)?;
        segment.size = segment.size.saturating_add(bytes.len());
        Ok(())
    }

    fn sync_current(&self) -> Result<()> {
        let segment = self
            .segments
            .back()
            .ok_or_else(|| errors::internal("Observation segment is missing"))?;
        let path = self.directory.join(segment_filename(segment.number));
        let file = OpenOptions::new().append(true).open_no_follow(&path)?;
        validate_private_file(&file)?;
        let actual = usize::try_from(file.metadata()?.len())
            .map_err(|_| errors::internal("Observation segment size is invalid"))?;
        if actual != segment.size {
            return Err(errors::internal(
                "Observation segment changed outside the recorder",
            ));
        }
        file.sync_all()?;
        Ok(())
    }

    fn enforce_retention(&mut self) -> Result<()> {
        while self.segments.len() > MAX_RETAINED_SEGMENTS {
            let oldest = self
                .segments
                .pop_front()
                .ok_or_else(|| errors::internal("Observation retention lost its segment"))?;
            fs::remove_file(self.directory.join(segment_filename(oldest.number)))?;
            sync_directory(&self.directory)?;
        }
        Ok(())
    }
}

pub fn experiment_root(profile: &Path) -> Result<PathBuf> {
    let experiments = profile.join("experiments");
    ensure_private_directory(&experiments)?;
    let root = experiments.join("pane-state");
    ensure_private_directory(&root)?;
    Ok(root)
}

pub(crate) fn replay_pane(root: &Path, pane_id: &str) -> Result<PaneReplay> {
    if pane_id.is_empty() {
        return Err(errors::invalid(
            "Pane identity is required for observation replay",
        ));
    }
    validate_private_directory(root)?;
    let directory = root.join(pane_directory_name(pane_id));
    match fs::symlink_metadata(&directory) {
        Ok(_) => validate_private_directory(&directory)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PaneReplay {
                segments: Vec::new(),
                skipped_segments: Vec::new(),
            });
        }
        Err(error) => return Err(error.into()),
    }
    let mut numbers = segment_numbers(&directory)?;
    numbers.sort_unstable();
    let mut last_sequence = None;
    let mut segments = Vec::new();
    let mut skipped_segments = Vec::new();
    for number in numbers {
        match read_segment_entry(&directory, number)? {
            SegmentRead::Current(segment) => {
                let segment = *segment;
                validate_loaded_segment(&segment, pane_id, number, &mut last_sequence)?;
                segments.push(segment);
            }
            SegmentRead::UnsupportedSchema(schema_version) => {
                skipped_segments.push(SkippedSegment {
                    pane_id: pane_id.to_owned(),
                    schema_version,
                });
            }
        }
    }
    Ok(PaneReplay {
        segments,
        skipped_segments,
    })
}

pub fn delete_pane(root: &Path, pane_id: &str) -> Result<DeletionReceipt> {
    if pane_id.is_empty() {
        return Err(errors::invalid(
            "Pane identity is required for observation deletion",
        ));
    }
    validate_private_directory(root)?;
    let directory = root.join(pane_directory_name(pane_id));
    match fs::symlink_metadata(&directory) {
        Ok(_) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DeletionReceipt { files: 0, bytes: 0 });
        }
        Err(error) => return Err(error.into()),
    }
    validate_private_directory(&directory)?;
    let files = validated_segment_files(&directory)?;
    let receipt = deletion_receipt(&files)?;
    for path in files {
        fs::remove_file(path)?;
    }
    sync_directory(&directory)?;
    fs::remove_dir(&directory)?;
    sync_directory(root)?;
    Ok(receipt)
}

fn validate_identity(
    pane_id: &str,
    pane_kind: ObservationPaneKind,
    harness: &HarnessIdentity,
    engine: &EngineBuild,
) -> Result<()> {
    let metadata = SegmentMetadata {
        schema_version: DATASET_SCHEMA_VERSION,
        pane_id: pane_id.into(),
        pane_kind,
        harness: harness.clone(),
        engine: engine.clone(),
        segment_sequence: 0,
        started_at_ms: 0,
    };
    Ok(metadata.validate()?)
}

fn load_segments(directory: &Path, pane_id: &str) -> Result<(VecDeque<Segment>, Option<u64>, u64)> {
    let mut numbers = segment_numbers(directory)?;
    numbers.sort_unstable();
    let next_segment_number = numbers
        .last()
        .map(|number| {
            number
                .checked_add(1)
                .ok_or_else(|| errors::internal("Observation segment sequence exhausted"))
        })
        .transpose()?
        .unwrap_or(0);
    let mut segments = VecDeque::new();
    let mut last_sequence = None;
    for number in numbers {
        let SegmentRead::Current(replayed) = read_segment_entry(directory, number)? else {
            continue;
        };
        let replayed = *replayed;
        validate_loaded_segment(&replayed, pane_id, number, &mut last_sequence)?;
        let last_timestamp_ms = replayed
            .clean_close
            .as_ref()
            .map(|end| end.ended_at_ms)
            .or_else(|| replayed.rows.last().map(|row| row_identity(row).3))
            .unwrap_or(replayed.metadata.started_at_ms);
        let size = usize::try_from(fs::metadata(directory.join(segment_filename(number)))?.len())
            .map_err(|_| errors::internal("Observation segment size is invalid"))?;
        segments.push_back(Segment {
            number,
            size,
            metadata: replayed.metadata,
            appendable: false,
            last_timestamp_ms,
        });
    }
    Ok((segments, last_sequence, next_segment_number))
}

fn validate_loaded_segment(
    segment: &ReplayedSegment,
    pane_id: &str,
    number: u64,
    last_sequence: &mut Option<u64>,
) -> Result<()> {
    if segment.metadata.pane_id != pane_id || segment.metadata.segment_sequence != number {
        return Err(soyeht_pane_state::Error::MismatchedProvenance.into());
    }
    for row in &segment.rows {
        let sequence = row_identity(row).4;
        if last_sequence.is_some_and(|last| sequence <= last) {
            return Err(soyeht_pane_state::Error::InvalidSequence.into());
        }
        *last_sequence = Some(sequence);
    }
    Ok(())
}

fn read_segment(directory: &Path, number: u64) -> Result<ReplayedSegment> {
    match read_segment_entry(directory, number)? {
        SegmentRead::Current(segment) => Ok(*segment),
        SegmentRead::UnsupportedSchema(_) => Err(soyeht_pane_state::Error::SchemaVersion.into()),
    }
}

fn read_segment_entry(directory: &Path, number: u64) -> Result<SegmentRead> {
    let path = directory.join(segment_filename(number));
    let mut file = OpenOptions::new().read(true).open_no_follow(&path)?;
    validate_private_file(&file)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_SEGMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let schema_version = segment_schema_version(&bytes)?;
    if schema_version != DATASET_SCHEMA_VERSION {
        return Ok(SegmentRead::UnsupportedSchema(schema_version));
    }
    Ok(SegmentRead::Current(Box::new(decode_segment(&bytes)?)))
}

fn segment_numbers(directory: &Path) -> Result<Vec<u64>> {
    let mut numbers = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() || !entry.file_type()?.is_file() {
            return Err(errors::internal(
                "Observation directory contains an invalid entry",
            ));
        }
        let name = entry.file_name();
        let number = parse_segment_name(&name)
            .ok_or_else(|| errors::internal("Observation segment name is invalid"))?;
        numbers.push(number);
    }
    Ok(numbers)
}

pub(super) fn validated_segment_files(directory: &Path) -> Result<Vec<PathBuf>> {
    segment_numbers(directory).map(|numbers| {
        numbers
            .into_iter()
            .map(|number| directory.join(segment_filename(number)))
            .collect()
    })
}

fn deletion_receipt(paths: &[PathBuf]) -> Result<DeletionReceipt> {
    let mut bytes = 0_u64;
    for path in paths {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(errors::internal(
                "Observation deletion refuses a non-file entry",
            ));
        }
        bytes = bytes
            .checked_add(metadata.len())
            .ok_or_else(|| errors::internal("Observation deletion size overflow"))?;
    }
    Ok(DeletionReceipt {
        files: paths.len(),
        bytes,
    })
}

pub(super) fn create_segment_file(directory: &Path, number: u64, header: &[u8]) -> Result<()> {
    let path = directory.join(segment_filename(number));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .owner_only()
        .open_no_follow(&path)?;
    file.write_all(header)?;
    file.sync_all()?;
    sync_directory(directory)?;
    Ok(())
}

pub(super) fn ensure_private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => validate_private_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_private_dir(path)?;
            validate_private_directory(path)
        }
        Err(error) => Err(error.into()),
    }
}

fn validate_private_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || !dir_is_owner_only(path)? {
        return Err(errors::internal(
            "Observation directory is not private and symlink-free",
        ));
    }
    Ok(())
}

fn validate_private_file(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || !file_is_owner_only(file)? {
        return Err(errors::internal(
            "Observation segment is not a private regular file",
        ));
    }
    Ok(())
}

pub(super) fn pane_directory_name(pane_id: &str) -> String {
    format!("pane-{:x}", Sha256::digest(pane_id.as_bytes()))
}

fn segment_filename(number: u64) -> String {
    format!("{number:016x}{SEGMENT_SUFFIX}")
}

fn parse_segment_name(name: &OsStr) -> Option<u64> {
    let text = name.to_str()?;
    let stem = text.strip_suffix(SEGMENT_SUFFIX)?;
    (stem.len() == 16)
        .then(|| u64::from_str_radix(stem, 16).ok())
        .flatten()
}

/// Where a row sits: its pane, and the time and sequence of its first and last observation.
/// An observation or a label is one point; a gap spans several.
pub(super) struct RowSpan<'a> {
    pane_id: &'a str,
    pane_kind: ObservationPaneKind,
    harness: &'a HarnessIdentity,
    first: (i64, u64),
    last: (i64, u64),
}

pub(super) fn row_span(row: &RecordedRow) -> RowSpan<'_> {
    let (pane_id, pane_kind, harness, first, last) = match row {
        RecordedRow::Observation(frame) => {
            let point = (frame.timestamp_ms, frame.sequence);
            (
                &frame.pane_id,
                frame.pane_kind,
                &frame.harness,
                point,
                point,
            )
        }
        RecordedRow::Label(label) => {
            let point = (label.timestamp_ms, label.sequence);
            (
                &label.pane_id,
                label.pane_kind,
                &label.harness,
                point,
                point,
            )
        }
        RecordedRow::Gap(gap) => (
            &gap.pane_id,
            gap.pane_kind,
            &gap.harness,
            (gap.first_timestamp_ms, gap.first_sequence),
            (gap.last_timestamp_ms, gap.last_sequence),
        ),
    };
    RowSpan {
        pane_id,
        pane_kind,
        harness,
        first,
        last,
    }
}

/// The pane, then the time and sequence at which the row ends.
pub(super) fn row_identity(
    row: &RecordedRow,
) -> (&str, ObservationPaneKind, &HarnessIdentity, i64, u64) {
    let span = row_span(row);
    (
        span.pane_id,
        span.pane_kind,
        span.harness,
        span.last.0,
        span.last.1,
    )
}

/// The pane, then the time and sequence at which the row starts.
pub(super) fn row_start(
    row: &RecordedRow,
) -> (&str, ObservationPaneKind, &HarnessIdentity, i64, u64) {
    let span = row_span(row);
    (
        span.pane_id,
        span.pane_kind,
        span.harness,
        span.first.0,
        span.first.1,
    )
}

fn encode_recorded_row(row: &RecordedRow) -> Result<Vec<u8>> {
    match row {
        RecordedRow::Observation(frame) => Ok(encode_row(&DatasetRow::Observation(frame.clone()))?),
        RecordedRow::Label(label) => Ok(encode_row(&DatasetRow::Label(label.clone()))?),
        RecordedRow::Gap(gap) => Ok(encode_row(&DatasetRow::Gap(gap.clone()))?),
    }
}
