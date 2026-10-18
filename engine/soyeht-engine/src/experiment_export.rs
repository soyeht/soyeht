use crate::{
    core::Core,
    errors::{self, Result},
    pane_observations::{experiment_root, replay_pane},
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_pane_state::{
    DATASET_SCHEMA_VERSION, DatasetRow, EngineBuild, HarnessIdentity, ObservationPaneKind,
    RecordedRow, encode_row,
};
use soyeht_platform::fs::{
    OpenOptionsPrivateExt, create_private_dir, dir_is_owner_only, file_is_owner_only,
    sync_directory,
};
use soyeht_protocol::{limits, method};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use uuid::Uuid;

const EXPORT_FORMAT_VERSION: u32 = 1;
const EXPORT_METHOD: &str = method::PANE_EXPERIMENT_EXPORT;
const STALE_EXPORT_TEMP_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const SKIPPED_SOURCE_NOTICE: &str = "Some selected experiment segments were excluded because their dataset schema is unsupported. Delete the named pane's captured data to remove those segments.";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportWindow {
    from_ms: String,
    to_ms: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportSource {
    pane_id: String,
    pane_kind: ObservationPaneKind,
    harness: HarnessIdentity,
    engine: EngineBuild,
    segment_sequence: u64,
    first_sequence: u64,
    last_sequence: u64,
    clean_close: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkippedExportSource {
    pane_id: String,
    schema_version: u32,
    segments: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportMetadata {
    kind: &'static str,
    export_format_version: u32,
    dataset_schema_version: u32,
    requested_window: ExportWindow,
    actual_window: Option<ExportWindow>,
    sources: Vec<ExportSource>,
    skipped_sources: Vec<SkippedExportSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped_source_notice: Option<&'static str>,
}

struct SelectedRow {
    timestamp_ms: i64,
    pane_id: String,
    sequence: u64,
    row: RecordedRow,
}

struct ExportSelection {
    rows: Vec<SelectedRow>,
    sources: Vec<ExportSource>,
    skipped_sources: Vec<SkippedExportSource>,
}

struct ExportReceipt {
    path: PathBuf,
    rows: usize,
    bytes: usize,
    sha256: String,
    actual_from_ms: Option<i64>,
    actual_to_ms: Option<i64>,
    skipped_sources: Vec<SkippedExportSource>,
    stale_temporary_cleanup: TemporaryCleanupReceipt,
}

#[derive(Clone, Copy, Default)]
struct TemporaryCleanupReceipt {
    files: usize,
    bytes: u64,
}

impl Core {
    pub(crate) fn export_experiment(&mut self, connection: &str, params: &Value) -> Result<Value> {
        if let Some(result) = self.repeated_mutation(connection, EXPORT_METHOD, params)? {
            return Ok(result);
        }
        let pane_ids = parse_pane_ids(params)?;
        if pane_ids.iter().any(|pane| {
            self.terminal_captures.contains_key(pane) || self.agent_captures.contains_key(pane)
        }) {
            return Err(errors::pane_busy(
                "Stop experiment capture before exporting its data",
            ));
        }
        let from_ms = parse_milliseconds(params, "fromMs")?;
        let to_ms = parse_milliseconds(params, "toMs")?;
        validate_window(from_ms, to_ms)?;
        let idempotency_key = errors::param_str(params, "idempotencyKey")?;
        let fingerprint = export_fingerprint(&pane_ids, from_ms, to_ms, idempotency_key);
        let receipt = build_export(&self.directory, &pane_ids, from_ms, to_ms, &fingerprint)?;
        let rows = u32::try_from(receipt.rows)
            .map_err(|_| errors::internal("Experiment export row count is invalid"))?;
        let stale_temporary_files_removed = u32::try_from(receipt.stale_temporary_cleanup.files)
            .map_err(|_| {
                errors::internal("Stale experiment export temporary file count is invalid")
            })?;
        let path = receipt
            .path
            .to_str()
            .ok_or_else(|| errors::internal("Experiment export path is not valid text"))?;
        let mut result = json!({
            "path":path,
            "rows":rows,
            "bytes":receipt.bytes.to_string(),
            "sha256":receipt.sha256,
            "staleTemporaryFilesRemoved":stale_temporary_files_removed,
            "staleTemporaryBytesRemoved":receipt.stale_temporary_cleanup.bytes.to_string()
        });
        if let Some(actual) = receipt.actual_from_ms {
            result["actualFromMs"] = json!(actual.to_string());
        }
        if let Some(actual) = receipt.actual_to_ms {
            result["actualToMs"] = json!(actual.to_string());
        }
        if !receipt.skipped_sources.is_empty() {
            result["skippedSources"] = json!(receipt.skipped_sources);
            result["skippedSourceNotice"] = json!(SKIPPED_SOURCE_NOTICE);
        }
        match self.mutate(connection, EXPORT_METHOD, params, |_tx| {
            Ok((result, vec![]))
        }) {
            Ok(mutation) => Ok(mutation.result),
            Err(error) => Err(export_receipt_commit_error(&receipt, error)),
        }
    }
}

fn parse_pane_ids(params: &Value) -> Result<Vec<String>> {
    let values = params
        .get("paneIds")
        .and_then(Value::as_array)
        .ok_or_else(|| errors::invalid("Experiment export pane IDs are required"))?;
    if values.is_empty() || values.len() > limits::EXPERIMENT_EXPORT_PANES {
        return Err(errors::invalid("Experiment export pane count is invalid"));
    }
    let mut unique = BTreeSet::new();
    for value in values {
        let pane = value
            .as_str()
            .filter(|pane| !pane.is_empty() && pane.len() <= 128)
            .ok_or_else(|| errors::invalid("Experiment export pane ID is invalid"))?;
        if !unique.insert(pane.to_owned()) {
            return Err(errors::invalid("Experiment export pane IDs must be unique"));
        }
    }
    Ok(unique.into_iter().collect())
}

fn parse_milliseconds(params: &Value, field: &str) -> Result<i64> {
    let value = params
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| errors::invalid(format!("Missing {field}")))?;
    let parsed = value
        .parse::<i64>()
        .map_err(|_| errors::invalid(format!("Invalid {field}")))?;
    if parsed < 0 {
        return Err(errors::invalid(format!("Invalid {field}")));
    }
    Ok(parsed)
}

fn validate_window(from_ms: i64, to_ms: i64) -> Result<()> {
    if from_ms > to_ms {
        return Err(errors::invalid("Experiment export window is reversed"));
    }
    Ok(())
}

fn export_fingerprint(
    pane_ids: &[String],
    from_ms: i64,
    to_ms: i64,
    idempotency_key: &str,
) -> String {
    let identity = json!({
        "paneIds":pane_ids,
        "fromMs":from_ms.to_string(),
        "toMs":to_ms.to_string(),
        "idempotencyKey":idempotency_key
    });
    format!("{:x}", Sha256::digest(identity.to_string().as_bytes()))
}

fn build_export(
    profile: &Path,
    pane_ids: &[String],
    from_ms: i64,
    to_ms: i64,
    fingerprint: &str,
) -> Result<ExportReceipt> {
    build_export_at(
        profile,
        pane_ids,
        from_ms,
        to_ms,
        fingerprint,
        SystemTime::now(),
    )
}

fn build_export_at(
    profile: &Path,
    pane_ids: &[String],
    from_ms: i64,
    to_ms: i64,
    fingerprint: &str,
    now: SystemTime,
) -> Result<ExportReceipt> {
    let root = experiment_root(profile)?;
    let mut selection = select_export_sources(&root, pane_ids, from_ms, to_ms)?;
    selection.rows.sort_by(|left, right| {
        (left.timestamp_ms, &left.pane_id, left.sequence).cmp(&(
            right.timestamp_ms,
            &right.pane_id,
            right.sequence,
        ))
    });
    let actual_from_ms = selection
        .rows
        .iter()
        .map(|row| row_interval(&row.row).0)
        .min();
    let actual_to_ms = selection
        .rows
        .iter()
        .map(|row| row_interval(&row.row).1)
        .max();
    let header = ExportMetadata {
        kind: "export_metadata",
        export_format_version: EXPORT_FORMAT_VERSION,
        dataset_schema_version: DATASET_SCHEMA_VERSION,
        requested_window: ExportWindow {
            from_ms: from_ms.to_string(),
            to_ms: to_ms.to_string(),
        },
        actual_window: actual_from_ms
            .zip(actual_to_ms)
            .map(|(from_ms, to_ms)| ExportWindow {
                from_ms: from_ms.to_string(),
                to_ms: to_ms.to_string(),
            }),
        sources: selection.sources,
        skipped_source_notice: (!selection.skipped_sources.is_empty())
            .then_some(SKIPPED_SOURCE_NOTICE),
        skipped_sources: selection.skipped_sources.clone(),
    };
    let mut bytes = serde_json::to_vec(&header)
        .map_err(|_| errors::internal("Experiment export header serialization failed"))?;
    bytes.push(b'\n');
    for row in &selection.rows {
        let dataset = match &row.row {
            RecordedRow::Observation(frame) => DatasetRow::Observation(frame.clone()),
            RecordedRow::Label(label) => DatasetRow::Label(label.clone()),
            RecordedRow::Gap(gap) => DatasetRow::Gap(gap.clone()),
        };
        bytes.extend(encode_row(&dataset)?);
        if bytes.len() > limits::EXPERIMENT_EXPORT_BYTES {
            return Err(errors::invalid("Experiment export byte limit reached"));
        }
    }
    let exports = root.join("exports");
    ensure_private_directory(&exports)?;
    let stale_temporary_cleanup = cleanup_stale_export_temporaries(&exports, now)?;
    let path = exports.join(format!("export-{fingerprint}.jsonl"));
    if let Err(error) = write_atomic(&exports, &path, &bytes) {
        return Err(export_after_cleanup_error(stale_temporary_cleanup, error));
    }
    Ok(ExportReceipt {
        path,
        rows: selection.rows.len(),
        bytes: bytes.len(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        actual_from_ms,
        actual_to_ms,
        skipped_sources: selection.skipped_sources,
        stale_temporary_cleanup,
    })
}

fn select_export_sources(
    root: &Path,
    pane_ids: &[String],
    from_ms: i64,
    to_ms: i64,
) -> Result<ExportSelection> {
    let mut selected = Vec::new();
    let mut sources = Vec::new();
    let mut skipped_source_counts = BTreeMap::<(String, u32), u32>::new();
    for pane_id in pane_ids {
        let replay = replay_pane(root, pane_id)?;
        for skipped in replay.skipped_segments {
            let count = skipped_source_counts
                .entry((skipped.pane_id, skipped.schema_version))
                .or_default();
            *count = count
                .checked_add(1)
                .ok_or_else(|| errors::internal("Skipped experiment segment count overflowed"))?;
        }
        for segment in replay.segments {
            let start = selected.len();
            for row in segment.rows {
                let (first, last, first_sequence, _) = row_interval(&row);
                if last < from_ms || first > to_ms {
                    continue;
                }
                enforce_row_limit(selected.len() + 1, pane_id)?;
                selected.push(SelectedRow {
                    timestamp_ms: first,
                    pane_id: pane_id.clone(),
                    sequence: first_sequence,
                    row,
                });
            }
            if selected.len() > start {
                let selected_rows = &selected[start..];
                sources.push(ExportSource {
                    pane_id: segment.metadata.pane_id,
                    pane_kind: segment.metadata.pane_kind,
                    harness: segment.metadata.harness,
                    engine: segment.metadata.engine,
                    segment_sequence: segment.metadata.segment_sequence,
                    first_sequence: selected_rows
                        .iter()
                        .map(|row| row_interval(&row.row).2)
                        .min()
                        .ok_or_else(|| errors::internal("Export source is empty"))?,
                    last_sequence: selected_rows
                        .iter()
                        .map(|row| row_interval(&row.row).3)
                        .max()
                        .ok_or_else(|| errors::internal("Export source is empty"))?,
                    clean_close: segment.clean_close.is_some(),
                });
            }
        }
    }
    let skipped_sources = skipped_source_counts
        .into_iter()
        .map(
            |((pane_id, schema_version), segments)| SkippedExportSource {
                pane_id,
                schema_version,
                segments,
            },
        )
        .collect::<Vec<_>>();
    Ok(ExportSelection {
        rows: selected,
        sources,
        skipped_sources,
    })
}

fn export_receipt_commit_error(receipt: &ExportReceipt, error: errors::Error) -> errors::Error {
    let cleanup = receipt.stale_temporary_cleanup;
    if cleanup.files == 0 {
        return errors::internal(format!(
            "The experiment export was written to {}, but its durable receipt was not saved: {error}",
            receipt.path.display()
        ));
    }
    errors::internal(format!(
        "The experiment export was written to {}, and stale temporary cleanup removed {} files ({} bytes), but its durable receipt was not saved: {error}",
        receipt.path.display(),
        cleanup.files,
        cleanup.bytes
    ))
}

fn export_after_cleanup_error(
    cleanup: TemporaryCleanupReceipt,
    error: errors::Error,
) -> errors::Error {
    if cleanup.files == 0 {
        return error;
    }
    errors::internal(format!(
        "Stale temporary cleanup removed {} files ({} bytes), but the experiment export failed: {error}",
        cleanup.files, cleanup.bytes
    ))
}

fn row_interval(row: &RecordedRow) -> (i64, i64, u64, u64) {
    match row {
        RecordedRow::Observation(frame) => (
            frame.timestamp_ms,
            frame.timestamp_ms,
            frame.sequence,
            frame.sequence,
        ),
        RecordedRow::Label(label) => (
            label.timestamp_ms,
            label.timestamp_ms,
            label.sequence,
            label.sequence,
        ),
        RecordedRow::Gap(gap) => (
            gap.first_timestamp_ms,
            gap.last_timestamp_ms,
            gap.first_sequence,
            gap.last_sequence,
        ),
    }
}

fn enforce_row_limit(rows: usize, pane_id: &str) -> Result<()> {
    if rows > limits::EXPERIMENT_EXPORT_ROWS {
        return Err(errors::invalid(format!(
            "Experiment export contains more than {} rows across the selected panes while reading pane {pane_id}; choose a shorter fromMs/toMs window or fewer panes",
            limits::EXPERIMENT_EXPORT_ROWS
        )));
    }
    Ok(())
}

fn ensure_private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if !metadata.file_type().is_symlink()
                && metadata.is_dir()
                && dir_is_owner_only(path)? =>
        {
            Ok(())
        }
        Ok(_) => Err(errors::internal(
            "Experiment export directory is not private",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_private_dir(path)?;
            sync_directory(
                path.parent()
                    .ok_or_else(|| errors::internal("Export root is missing"))?,
            )?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn cleanup_stale_export_temporaries(
    directory: &Path,
    now: SystemTime,
) -> Result<TemporaryCleanupReceipt> {
    let entries = fs::read_dir(directory).map_err(|error| {
        errors::internal(format!(
            "Could not inspect stale experiment export temporaries: {error}"
        ))
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| temporary_cleanup_error(Default::default(), error.into()))?
            .path();
        if path
            .file_name()
            .is_some_and(is_canonical_export_temporary_name)
        {
            paths.push(path);
        }
    }
    paths.sort();

    let mut receipt = TemporaryCleanupReceipt::default();
    for path in paths {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(temporary_cleanup_error(receipt, error.into())),
        };
        if !metadata.file_type().is_file() {
            return Err(temporary_cleanup_error(
                receipt,
                errors::internal("Experiment export temporary is not a regular file"),
            ));
        }
        let modified = metadata
            .modified()
            .map_err(|error| temporary_cleanup_error(receipt, error.into()))?;
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age < STALE_EXPORT_TEMP_AGE {
            continue;
        }
        let next_files = receipt.files.checked_add(1).ok_or_else(|| {
            temporary_cleanup_error(
                receipt,
                errors::internal("Stale temporary cleanup file count overflowed"),
            )
        })?;
        let next_bytes = receipt.bytes.checked_add(metadata.len()).ok_or_else(|| {
            temporary_cleanup_error(
                receipt,
                errors::internal("Stale temporary cleanup byte count overflowed"),
            )
        })?;
        fs::remove_file(&path).map_err(|error| temporary_cleanup_error(receipt, error.into()))?;
        receipt.files = next_files;
        receipt.bytes = next_bytes;
    }
    if receipt.files > 0 {
        sync_directory(directory)
            .map_err(|error| temporary_cleanup_error(receipt, error.into()))?;
    }
    Ok(receipt)
}

pub(crate) fn cleanup_stale_export_temporaries_on_start(profile: &Path) -> Result<(usize, u64)> {
    let expected = profile
        .join("experiments")
        .join("pane-state")
        .join("exports");
    match fs::symlink_metadata(&expected) {
        Ok(_) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(error) => return Err(error.into()),
    }
    let exports = experiment_root(profile)?.join("exports");
    ensure_private_directory(&exports)?;
    let receipt = cleanup_stale_export_temporaries(&exports, SystemTime::now())?;
    Ok((receipt.files, receipt.bytes))
}

fn is_canonical_export_temporary_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some(uuid) = name
        .strip_prefix(".export-")
        .and_then(|name| name.strip_suffix(".tmp"))
    else {
        return false;
    };
    Uuid::parse_str(uuid).is_ok_and(|parsed| parsed.hyphenated().to_string() == uuid)
}

fn temporary_cleanup_error(
    receipt: TemporaryCleanupReceipt,
    error: errors::Error,
) -> errors::Error {
    if receipt.files == 0 {
        return errors::internal(format!(
            "Stale experiment export temporary cleanup failed: {error}"
        ));
    }
    errors::internal(format!(
        "Stale experiment export temporary cleanup removed {} files ({} bytes) before it failed: {error}",
        receipt.files, receipt.bytes
    ))
}

fn existing_export_matches(path: &Path, bytes: &[u8]) -> Result<bool> {
    let mut existing = OpenOptions::new().read(true).open_no_follow(path)?;
    let metadata = existing.metadata()?;
    if !metadata.is_file() || !file_is_owner_only(&existing)? {
        return Err(errors::internal(
            "Existing experiment export is not private",
        ));
    }
    let mut prior = Vec::new();
    Read::by_ref(&mut existing)
        .take(limits::EXPERIMENT_EXPORT_BYTES as u64 + 1)
        .read_to_end(&mut prior)?;
    Ok(prior == bytes)
}

fn write_atomic(directory: &Path, path: &Path, bytes: &[u8]) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) if existing_export_matches(path, bytes)? => return Ok(()),
        Ok(_) => {
            return Err(errors::internal(
                "Existing experiment export differs from this retry",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    let temporary = directory.join(format!(".export-{}.tmp", Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .owner_only()
        .open_no_follow(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::hard_link(&temporary, path)?;
        fs::remove_file(&temporary)?;
        sync_directory(directory)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary); // Best effort: the write error is returned.
    }
    result
}

#[cfg(test)]
mod tests;
