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
        create_private_dir(&profile).unwrap();
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

fn observation(pane_id: &str, harness: &HarnessIdentity, at: i64, sequence: u64) -> RecordedRow {
    RecordedRow::Observation(ObservationFrame {
        pane_kind: ObservationPaneKind::Terminal,
        pane_id: pane_id.into(),
        harness: harness.clone(),
        session_generation: 1,
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
        .owner_only()
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

fn write_schema_only_segment(root: &Path, pane_id: &str, number: u64, schema_version: u32) {
    let directory = root.join(format!("pane-{:x}", Sha256::digest(pane_id.as_bytes())));
    create_private_dir(&directory).unwrap();
    write_private(
        &directory.join(format!("{number:016x}.jsonl")),
        format!("{{\"kind\":\"metadata\",\"schemaVersion\":{schema_version}}}\n").as_bytes(),
    );
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
    fs::File::options()
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
    create_private_dir(&invalid).unwrap();

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
        skipped_sources: Vec::new(),
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
    let mut first = SegmentStore::open(
        &root,
        "pane-b",
        ObservationPaneKind::Terminal,
        first_harness.clone(),
        engine('a'),
    )
    .unwrap();
    first
        .append(observation("pane-b", &first_harness, 100, 0))
        .unwrap();
    first
        .append(observation("pane-b", &first_harness, 200, 1))
        .unwrap();
    first.close().unwrap();

    let second_harness = harness("registry.second");
    let mut second = SegmentStore::open(
        &root,
        "pane-a",
        ObservationPaneKind::Terminal,
        second_harness.clone(),
        engine('b'),
    )
    .unwrap();
    second
        .append(observation("pane-a", &second_harness, 150, 0))
        .unwrap();
    second
        .append(RecordedRow::Label(GroundTruthLabel {
            pane_id: "pane-a".into(),
            pane_kind: ObservationPaneKind::Terminal,
            harness: second_harness,
            timestamp_ms: 175,
            sequence: 1,
            state: PaneState::Working,
            source: StateSource::Protocol,
            provenance: soyeht_pane_state::GroundTruthProvenance::protocol(1),
        }))
        .unwrap();
    second
        .append(RecordedRow::Gap(ObservationGap {
            pane_id: "pane-a".into(),
            pane_kind: ObservationPaneKind::Terminal,
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
    assert!(file_is_owner_only(&fs::File::open(&first_receipt.path).unwrap()).unwrap());
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
        source["paneKind"] == "terminal"
            && source["cleanClose"] == true
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
fn export_skips_only_incompatible_segments_and_names_the_local_remedy() {
    let fixture = Fixture::new();
    let root = experiment_root(&fixture.profile).unwrap();
    write_schema_only_segment(&root, "pane-old", 0, DATASET_SCHEMA_VERSION - 1);

    let identity = harness("registry.current");
    let mut current = SegmentStore::open(
        &root,
        "pane-current",
        ObservationPaneKind::Terminal,
        identity.clone(),
        engine('c'),
    )
    .unwrap();
    current
        .append(observation("pane-current", &identity, 150, 0))
        .unwrap();
    current.close().unwrap();

    let panes = vec!["pane-old".into(), "pane-current".into()];
    let receipt = build_export(&fixture.profile, &panes, 100, 200, "mixed-schema")
        .expect("an old pane must not abort an unrelated current pane");

    assert_eq!(receipt.rows, 1);
    assert_eq!(
        receipt.skipped_sources,
        vec![SkippedExportSource {
            pane_id: "pane-old".into(),
            schema_version: DATASET_SCHEMA_VERSION - 1,
            segments: 1,
        }]
    );
    let header: Value = serde_json::from_str(
        fs::read_to_string(&receipt.path)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(header["sources"].as_array().unwrap().len(), 1);
    assert_eq!(header["sources"][0]["paneId"], "pane-current");
    assert_eq!(header["skippedSources"][0]["paneId"], "pane-old");
    assert_eq!(
        header["skippedSources"][0]["schemaVersion"],
        DATASET_SCHEMA_VERSION - 1
    );
    assert_eq!(header["skippedSources"][0]["segments"], 1);
    assert!(
        header["skippedSourceNotice"]
            .as_str()
            .unwrap()
            .contains("Delete the named pane's captured data")
    );
}

#[test]
fn export_still_fails_closed_for_a_malformed_current_schema_segment() {
    let fixture = Fixture::new();
    let root = experiment_root(&fixture.profile).unwrap();
    write_schema_only_segment(&root, "pane-broken", 0, DATASET_SCHEMA_VERSION);

    let error = build_export(
        &fixture.profile,
        &["pane-broken".into()],
        100,
        200,
        "broken-current",
    )
    .err()
    .expect("malformed current-schema data must fail closed");

    assert!(error.to_string().contains("valid closed JSON"));
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
    use soyeht_platform::fs::symlink;

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
    write_schema_only_segment(&root, "pane-old", 0, DATASET_SCHEMA_VERSION - 1);
    let identity = harness("registry.command");
    let mut store = SegmentStore::open(
        &root,
        "pane-command",
        ObservationPaneKind::Terminal,
        identity.clone(),
        engine('c'),
    )
    .unwrap();
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
            "paneIds":["pane-command","pane-old"],"fromMs":"100","toMs":"200",
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
    assert_eq!(first["skippedSources"][0]["paneId"], "pane-old");
    assert_eq!(
        first["skippedSources"][0]["schemaVersion"],
        DATASET_SCHEMA_VERSION - 1
    );
    assert_eq!(first["skippedSources"][0]["segments"], 1);
    assert!(
        first["skippedSourceNotice"]
            .as_str()
            .unwrap()
            .contains("Delete the named pane's captured data")
    );
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
