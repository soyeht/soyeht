use crate::{
    core::Core,
    errors::{self, Result},
    pane_observations::{experiment_root, replay_pane},
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_pane_state::{
    DATASET_SCHEMA_VERSION, DatasetRow, EngineBuild, HarnessIdentity, RecordedRow, encode_row,
};
use soyeht_protocol::limits;
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use uuid::Uuid;

const DIRECTORY_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;
const EXPORT_FORMAT_VERSION: u32 = 1;
const EXPORT_METHOD: &str = "pane.experiment.export";
const STALE_EXPORT_TEMP_AGE: Duration = Duration::from_secs(24 * 60 * 60);

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
    harness: HarnessIdentity,
    engine: EngineBuild,
    segment_sequence: u64,
    first_sequence: u64,
    last_sequence: u64,
    clean_close: bool,
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
}

struct SelectedRow {
    timestamp_ms: i64,
    pane_id: String,
    sequence: u64,
    row: RecordedRow,
}

struct ExportReceipt {
    path: PathBuf,
    rows: usize,
    bytes: usize,
    sha256: String,
    actual_from_ms: Option<i64>,
    actual_to_ms: Option<i64>,
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
        if pane_ids
            .iter()
            .any(|pane| self.terminal_captures.contains_key(pane))
        {
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
    let mut selected = Vec::new();
    let mut sources = Vec::new();
    for pane_id in pane_ids {
        for segment in replay_pane(&root, pane_id)? {
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
    selected.sort_by(|left, right| {
        (left.timestamp_ms, &left.pane_id, left.sequence).cmp(&(
            right.timestamp_ms,
            &right.pane_id,
            right.sequence,
        ))
    });
    let actual_from_ms = selected.iter().map(|row| row_interval(&row.row).0).min();
    let actual_to_ms = selected.iter().map(|row| row_interval(&row.row).1).max();
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
        sources,
    };
    let mut bytes = serde_json::to_vec(&header)
        .map_err(|_| errors::internal("Experiment export header serialization failed"))?;
    bytes.push(b'\n');
    for row in &selected {
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
        rows: selected.len(),
        bytes: bytes.len(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        actual_from_ms,
        actual_to_ms,
        stale_temporary_cleanup,
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
                && metadata.permissions().mode() & 0o077 == 0 =>
        {
            Ok(())
        }
        Ok(_) => Err(errors::internal(
            "Experiment export directory is not private",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::DirBuilder::new().mode(DIRECTORY_MODE).create(path)?;
            File::open(
                path.parent()
                    .ok_or_else(|| errors::internal("Export root is missing"))?,
            )?
            .sync_all()?;
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
        File::open(directory)
            .map_err(|error| temporary_cleanup_error(receipt, error.into()))?
            .sync_all()
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
    let mut existing = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = existing.metadata()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
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
        .mode(FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::hard_link(&temporary, path)?;
        fs::remove_file(&temporary)?;
        File::open(directory)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dispatch::Outcome, outbox, pane_observations::SegmentStore};
    use soyeht_pane_state::{
        GroundTruthLabel, ObservationFrame, ObservationGap, ObservationReason, RecordedRow,
    };
    use soyeht_protocol::{PaneState, StateSource};

    struct Fixture {
        profile: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let profile = std::env::temp_dir().join(format!("sn-export-{}", Uuid::new_v4()));
            fs::DirBuilder::new()
                .mode(DIRECTORY_MODE)
                .create(&profile)
                .unwrap();
            Self { profile }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.profile);
        }
    }

    fn harness(id: &str) -> HarnessIdentity {
        HarnessIdentity::known(id, Some("1.2.3".into())).unwrap()
    }

    fn engine(digest: char) -> EngineBuild {
        EngineBuild {
            package_version: "0.1.0".into(),
            executable_sha256: digest.to_string().repeat(64),
        }
    }

    fn observation(
        pane_id: &str,
        harness: &HarnessIdentity,
        at: i64,
        sequence: u64,
    ) -> RecordedRow {
        RecordedRow::Observation(ObservationFrame {
            pane_id: pane_id.into(),
            harness: harness.clone(),
            timestamp_ms: at,
            sequence,
            reason: ObservationReason::VisualChange,
            osc_title: None,
            lines: vec![format!("frame {sequence}")],
            cursor: None,
            foreground_process: Some("test".into()),
            bytes_per_second: 10,
        })
    }

    fn reply(outcome: Outcome) -> Value {
        let Outcome::Reply(value) = outcome else {
            panic!("expected reply")
        };
        value
    }

    fn write_private(path: &Path, bytes: &[u8]) {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(FILE_MODE)
            .open(path)
            .unwrap();
        file.write_all(bytes).unwrap();
        file.sync_all().unwrap();
    }

    #[test]
    fn export_temporary_match_is_anchored_to_one_canonical_uuid_name() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        assert!(is_canonical_export_temporary_name(OsStr::new(&format!(
            ".export-{uuid}.tmp"
        ))));
        for name in [
            format!("export-{uuid}.tmp"),
            format!(".export-{uuid}"),
            format!("prefix.export-{uuid}.tmp"),
            format!(".export-{uuid}.tmp.tail"),
            format!(".export-{}.tmp", uuid.to_uppercase()),
            format!("export-{uuid}.jsonl"),
        ] {
            assert!(
                !is_canonical_export_temporary_name(OsStr::new(&name)),
                "matched {name}"
            );
        }
    }

    #[test]
    fn cleanup_preserves_a_canonical_temporary_younger_than_the_declared_age() {
        let fixture = Fixture::new();
        let exports = experiment_root(&fixture.profile).unwrap().join("exports");
        ensure_private_directory(&exports).unwrap();
        let temporary = exports.join(".export-550e8400-e29b-41d4-a716-446655440000.tmp");
        write_private(&temporary, b"still live");
        let modified = fs::symlink_metadata(&temporary)
            .unwrap()
            .modified()
            .unwrap();

        let receipt = cleanup_stale_export_temporaries(&exports, modified).unwrap();

        assert_eq!(receipt.files, 0);
        assert_eq!(receipt.bytes, 0);
        assert!(temporary.exists());
    }

    #[test]
    fn cleanup_treats_a_future_modification_time_as_age_zero() {
        let fixture = Fixture::new();
        let exports = experiment_root(&fixture.profile).unwrap().join("exports");
        ensure_private_directory(&exports).unwrap();
        let temporary = exports.join(".export-550e8400-e29b-41d4-a716-446655440000.tmp");
        write_private(&temporary, b"clock moved backwards");
        let modified = fs::symlink_metadata(&temporary)
            .unwrap()
            .modified()
            .unwrap();
        let now = modified - Duration::from_secs(1);

        let receipt = cleanup_stale_export_temporaries(&exports, now).unwrap();

        assert_eq!(receipt.files, 0);
        assert_eq!(receipt.bytes, 0);
        assert!(temporary.exists());
    }

    #[test]
    fn engine_start_removes_an_old_orphan_without_waiting_for_another_export() {
        let fixture = Fixture::new();
        let exports = experiment_root(&fixture.profile).unwrap().join("exports");
        ensure_private_directory(&exports).unwrap();
        let temporary = exports.join(".export-550e8400-e29b-41d4-a716-446655440000.tmp");
        write_private(&temporary, b"abandoned raw terminal");
        let old = SystemTime::now() - STALE_EXPORT_TEMP_AGE - Duration::from_secs(1);
        File::options()
            .write(true)
            .open(&temporary)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();

        let _core = Core::open(&fixture.profile).unwrap();

        assert!(!temporary.exists());
    }

    #[test]
    fn startup_cleanup_failure_is_visible_but_does_not_block_the_engine() {
        let fixture = Fixture::new();
        let exports = experiment_root(&fixture.profile).unwrap().join("exports");
        ensure_private_directory(&exports).unwrap();
        let invalid = exports.join(".export-550e8400-e29b-41d4-a716-446655440000.tmp");
        fs::DirBuilder::new()
            .mode(DIRECTORY_MODE)
            .create(&invalid)
            .unwrap();

        let core = Core::open(&fixture.profile);

        assert!(core.is_ok(), "startup cleanup failure blocked the engine");
        assert!(invalid.is_dir());
    }

    #[test]
    fn a_real_export_removes_only_an_old_canonical_temporary_and_reports_it() {
        let fixture = Fixture::new();
        let exports = experiment_root(&fixture.profile).unwrap().join("exports");
        ensure_private_directory(&exports).unwrap();
        let temporary = exports.join(".export-550e8400-e29b-41d4-a716-446655440000.tmp");
        let neighbour = exports.join(".export-not-a-uuid.tmp");
        let final_export = exports.join(format!("export-{}.jsonl", "a".repeat(64)));
        write_private(&temporary, b"raw terminal");
        write_private(&neighbour, b"neighbour");
        write_private(&final_export, b"final\n");
        let modified = fs::symlink_metadata(&temporary)
            .unwrap()
            .modified()
            .unwrap();
        let now = modified + STALE_EXPORT_TEMP_AGE + Duration::from_secs(1);

        let receipt = build_export_at(&fixture.profile, &[], 0, 0, "cleanup", now).unwrap();

        assert_eq!(receipt.stale_temporary_cleanup.files, 1);
        assert_eq!(
            receipt.stale_temporary_cleanup.bytes,
            b"raw terminal".len() as u64
        );
        assert!(!temporary.exists());
        assert!(neighbour.exists());
        assert!(final_export.exists());
        assert!(receipt.path.exists());
    }

    #[test]
    fn a_failure_after_real_cleanup_still_reports_exactly_what_was_removed() {
        let fixture = Fixture::new();
        let exports = experiment_root(&fixture.profile).unwrap().join("exports");
        ensure_private_directory(&exports).unwrap();
        let temporary = exports.join(".export-550e8400-e29b-41d4-a716-446655440000.tmp");
        write_private(&temporary, b"raw terminal");
        write_private(&exports.join("export-after-cleanup.jsonl"), b"conflict\n");
        let modified = fs::symlink_metadata(&temporary)
            .unwrap()
            .modified()
            .unwrap();
        let now = modified + STALE_EXPORT_TEMP_AGE + Duration::from_secs(1);

        let error = build_export_at(&fixture.profile, &[], 0, 0, "after-cleanup", now)
            .err()
            .expect("conflicting destination must fail")
            .to_string();

        assert!(!temporary.exists());
        assert!(error.contains("removed 1 files (12 bytes)"));
        assert!(error.contains("but the experiment export failed"));
    }

    #[test]
    fn receipt_commit_failure_keeps_the_cleanup_receipt_visible() {
        let receipt = ExportReceipt {
            path: PathBuf::from("/profile/experiments/pane-state/exports/export-test.jsonl"),
            rows: 0,
            bytes: 1,
            sha256: "a".repeat(64),
            actual_from_ms: None,
            actual_to_ms: None,
            stale_temporary_cleanup: TemporaryCleanupReceipt {
                files: 2,
                bytes: 144,
            },
        };

        let error =
            export_receipt_commit_error(&receipt, errors::internal("receipt store unavailable"))
                .to_string();

        assert!(error.contains("removed 2 files (144 bytes)"));
        assert!(error.contains("durable receipt was not saved"));
    }

    #[test]
    fn export_is_private_atomic_idempotent_and_carries_exact_provenance() {
        let fixture = Fixture::new();
        let root = experiment_root(&fixture.profile).unwrap();
        let first_harness = harness("registry.first");
        let mut first =
            SegmentStore::open(&root, "pane-b", first_harness.clone(), engine('a')).unwrap();
        first
            .append(observation("pane-b", &first_harness, 100, 0))
            .unwrap();
        first
            .append(observation("pane-b", &first_harness, 200, 1))
            .unwrap();
        first.close().unwrap();

        let second_harness = harness("registry.second");
        let mut second =
            SegmentStore::open(&root, "pane-a", second_harness.clone(), engine('b')).unwrap();
        second
            .append(observation("pane-a", &second_harness, 150, 0))
            .unwrap();
        second
            .append(RecordedRow::Label(GroundTruthLabel {
                pane_id: "pane-a".into(),
                harness: second_harness,
                timestamp_ms: 175,
                sequence: 1,
                state: PaneState::Working,
                source: StateSource::Protocol,
            }))
            .unwrap();
        second
            .append(RecordedRow::Gap(ObservationGap {
                pane_id: "pane-a".into(),
                harness: harness("registry.second"),
                first_timestamp_ms: 180,
                last_timestamp_ms: 190,
                first_sequence: 2,
                last_sequence: 4,
                dropped_observations: None,
                dropped_bytes: Some(512),
            }))
            .unwrap();
        second.close().unwrap();

        let panes = vec!["pane-b".into(), "pane-a".into()];
        let first_receipt = build_export(&fixture.profile, &panes, 120, 210, "same").unwrap();
        let repeated = build_export(&fixture.profile, &panes, 120, 210, "same").unwrap();
        assert_eq!(first_receipt.sha256, repeated.sha256);
        assert_eq!(first_receipt.rows, 4);
        assert_eq!(first_receipt.actual_from_ms, Some(150));
        assert_eq!(first_receipt.actual_to_ms, Some(200));
        assert_eq!(
            fs::metadata(&first_receipt.path)
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
        let text = fs::read_to_string(&first_receipt.path).unwrap();
        assert!(text.ends_with('\n'));
        let rows: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0]["kind"], "export_metadata");
        assert_eq!(rows[0]["requestedWindow"]["fromMs"], "120");
        assert_eq!(rows[0]["requestedWindow"]["toMs"], "210");
        assert_eq!(rows[0]["actualWindow"]["fromMs"], "150");
        assert_eq!(rows[0]["actualWindow"]["toMs"], "200");
        assert_eq!(rows[0]["sources"].as_array().unwrap().len(), 2);
        assert!(rows[0]["sources"].as_array().unwrap().iter().all(|source| {
            source["cleanClose"] == true
                && source["engine"]["executableSha256"]
                    .as_str()
                    .is_some_and(|digest| digest.len() == 64)
        }));
        let second_source = rows[0]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|source| source["paneId"] == "pane-a")
            .unwrap();
        assert_eq!(second_source["firstSequence"], 0);
        assert_eq!(second_source["lastSequence"], 4);
        assert_eq!(rows[1]["pane_id"], "pane-a");
        assert_eq!(rows[2]["kind"], "label");
        assert_eq!(rows[3]["kind"], "gap");
        assert_eq!(rows[4]["pane_id"], "pane-b");
    }

    #[test]
    fn export_parameter_validation_rejects_reversed_duplicate_and_unbounded_requests() {
        assert!(parse_milliseconds(&json!({"fromMs":"-1"}), "fromMs").is_err());
        assert!(validate_window(2, 1).is_err());
        assert!(parse_pane_ids(&json!({"paneIds":["same","same"]})).is_err());
        let too_many: Vec<_> = (0..=limits::EXPERIMENT_EXPORT_PANES)
            .map(|index| format!("pane-{index}"))
            .collect();
        assert!(parse_pane_ids(&json!({"paneIds":too_many})).is_err());
        let error = enforce_row_limit(limits::EXPERIMENT_EXPORT_ROWS + 1, "pane-overflow")
            .unwrap_err()
            .to_string();
        assert!(error.contains(&format!(
            "more than {} rows",
            limits::EXPERIMENT_EXPORT_ROWS
        )));
        assert!(!error.contains(&(limits::EXPERIMENT_EXPORT_ROWS + 1).to_string()));
        assert!(error.contains("pane-overflow"));
        assert!(error.contains("shorter fromMs/toMs window or fewer panes"));
    }

    #[test]
    fn export_refuses_symlink_destination_without_changing_its_target() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let root = experiment_root(&fixture.profile).unwrap();
        let exports = root.join("exports");
        ensure_private_directory(&exports).unwrap();
        let outside = fixture.profile.join("outside");
        fs::write(&outside, b"keep").unwrap();
        let destination = exports.join("export-link.jsonl");
        symlink(&outside, &destination).unwrap();
        let error = write_atomic(&exports, &destination, b"replace\n").unwrap_err();
        assert_eq!(
            errors::kind(&error),
            soyeht_protocol::ErrorKind::InternalError
        );
        assert_eq!(fs::read(&outside).unwrap(), b"keep");
    }

    #[test]
    fn signed_ui_command_exports_once_and_returns_its_durable_receipt() {
        let fixture = Fixture::new();
        let root = experiment_root(&fixture.profile).unwrap();
        let identity = harness("registry.command");
        let mut store =
            SegmentStore::open(&root, "pane-command", identity.clone(), engine('c')).unwrap();
        store
            .append(observation("pane-command", &identity, 150, 0))
            .unwrap();
        store.close().unwrap();

        let mut core = Core::open(&fixture.profile).unwrap();
        let (sender, _receiver) = outbox::channel();
        let connection = core.connect(true, true, sender).unwrap();
        reply(
            core.dispatch(
                &connection,
                json!({
                    "jsonrpc":"2.0","id":"hello","method":"hello",
                    "params":{"client":"export-test","clientVersion":"1","protocolVersion":1}
                }),
            )
            .unwrap(),
        );
        let request = json!({
            "jsonrpc":"2.0","id":"export","method":EXPORT_METHOD,
            "params":{
                "paneIds":["pane-command"],"fromMs":"100","toMs":"200",
                "idempotencyKey":"export-once"
            }
        });
        let first = reply(core.dispatch(&connection, request.clone()).unwrap());
        let repeated = reply(core.dispatch(&connection, request).unwrap());
        assert_eq!(repeated, first);
        assert_eq!(first["rows"], 1);
        assert_eq!(first["actualFromMs"], "150");
        assert_eq!(first["actualToMs"], "150");
        assert_eq!(first["staleTemporaryFilesRemoved"], 0);
        assert_eq!(first["staleTemporaryBytesRemoved"], "0");
        let path = PathBuf::from(first["path"].as_str().unwrap());
        assert!(path.is_file());
        assert_eq!(
            fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1,
            "idempotent retry wrote another export"
        );

        let empty = reply(
            core.dispatch(
                &connection,
                json!({
                    "jsonrpc":"2.0","id":"empty","method":EXPORT_METHOD,
                    "params":{
                        "paneIds":["pane-without-data"],"fromMs":"100","toMs":"200",
                        "idempotencyKey":"export-empty"
                    }
                }),
            )
            .unwrap(),
        );
        assert_eq!(empty["rows"], 0);
        assert!(empty.get("actualFromMs").is_none());
        assert!(empty.get("actualToMs").is_none());
    }
}
