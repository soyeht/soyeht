//! A save that cannot reach the disk is told to the clients with its reason, keeps the draft,
//! backs off, and is retried at once when the person asks to save again.

use super::{
    FileSystemPort, SystemFiles,
    interfered_files::{InterferedFiles, Interference},
    test_fixture::EditorFixture,
};
use serde_json::{Value, json};
use std::{
    fs, io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const TICK_MS: i64 = 250;
const TEN_MINUTES_MS: i64 = 600_000;
const EXPECTED_ATTEMPTS_IN_TEN_MINUTES: u32 = 15;

struct Rig {
    fixture: EditorFixture,
    file_id: String,
    now: i64,
}

impl Rig {
    async fn new() -> Self {
        Self::over(Box::new(SystemFiles)).await
    }

    async fn over(port: Box<dyn FileSystemPort>) -> Self {
        let mut fixture = EditorFixture::over(&[("main.rs", "fn main() {}\n")], port).await;
        fixture.call("events.subscribe", json!({"since":"0"}));
        let file_id = fixture.open("main.rs", false)["file"]["fileId"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            fixture,
            file_id,
            now: 1_000,
        }
    }

    /// The person deletes the file from a shell and types: the autosave fails for `ms`.
    fn lose_the_file_and_type(&mut self, ms: i64) {
        fs::remove_file(self.path()).unwrap();
        self.type_text("// ");
        self.tick_for(ms);
    }

    /// The file is on the disk with the draft, and nothing is left to save or to tell.
    fn assert_created_with_the_draft(&mut self) {
        assert_eq!(
            fs::read_to_string(self.path()).unwrap(),
            "// fn main() {}\n"
        );
        assert_eq!(self.save_failure(), Value::Null);
        assert!(!self.dirty());
    }

    /// The owner makes the file read-only, or writable again.
    fn set_read_only(&self, read_only: bool) {
        let mut permissions = fs::metadata(self.path()).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(read_only);
        fs::set_permissions(self.path(), permissions).unwrap();
    }

    fn path(&self) -> std::path::PathBuf {
        self.fixture.project().join("main.rs")
    }

    fn type_text(&mut self, text: &str) {
        let file_id = self.file_id.clone();
        self.fixture.type_at(&file_id, 0, 0, text);
    }

    /// Runs the ticks of `ms` milliseconds.
    fn tick_for(&mut self, ms: i64) {
        let until = self.now + ms;
        while self.now < until {
            self.now += TICK_MS;
            self.fixture.at(self.now);
            self.fixture.core.advance_editor_effects();
        }
    }

    fn save_failure(&mut self) -> Value {
        let file_id = self.file_id.clone();
        self.fixture.read(&file_id)["saveFailure"].clone()
    }

    fn dirty(&mut self) -> bool {
        let file_id = self.file_id.clone();
        self.fixture.read(&file_id)["file"]["dirty"] == true
    }

    fn save_again(&mut self) -> Value {
        let file_id = self.file_id.clone();
        self.fixture.call("file.save", json!({"fileId":file_id}))
    }

    /// Runs the ticks, backoff included, until `attempts` saves have failed in a row.
    fn fail_until(&mut self, attempts: u32) {
        let until = self.now + TEN_MINUTES_MS;
        while self.attempts() < attempts && self.now < until {
            self.tick_for(TICK_MS);
        }
        assert_eq!(self.attempts(), attempts);
    }

    fn attempts(&self) -> u32 {
        self.fixture
            .core
            .editor
            .effect_attempts(&self.file_id, super::Effect::Save)
    }

    /// The reasons of the `file.save_failed` events sent since the last call.
    async fn reasons_told(&mut self) -> Vec<Value> {
        self.failures_told()
            .await
            .into_iter()
            .map(|params| params["reason"].clone())
            .collect()
    }

    async fn failures_told(&mut self) -> Vec<Value> {
        self.fixture
            .notifications()
            .await
            .into_iter()
            .filter(|frame| frame["method"] == "file.save_failed")
            .map(|frame| frame["params"].clone())
            .collect()
    }
}

#[tokio::test]
async fn a_file_deleted_under_an_open_tab_is_told_as_missing_and_not_brought_back_by_autosave() {
    let mut rig = Rig::new().await;
    rig.lose_the_file_and_type(2_000);

    assert_eq!(rig.save_failure(), "missing");
    assert!(rig.dirty(), "the draft is kept");
    assert!(!rig.path().exists(), "autosave never recreates a file");
    let told = rig.failures_told().await;
    assert!(!told.is_empty());
    assert_eq!(told[0], json!({"fileId":rig.file_id,"reason":"missing"}));
}

#[tokio::test]
async fn a_failing_save_backs_off_and_does_not_stop_so_a_restored_file_is_saved() {
    let mut rig = Rig::new().await;
    let original = fs::read_to_string(rig.path()).unwrap();
    rig.lose_the_file_and_type(TEN_MINUTES_MS);

    // Ten minutes of ticks is 2400. The wait doubles from 1 s (1, 2, 4, 8, 16, 32 s) and stops at
    // the 60 s cap, so the attempts are 15: a cap of 30 s would make it more than 20, and no
    // backoff 2400.
    let attempts = rig
        .fixture
        .core
        .editor
        .effect_attempts(&rig.file_id, super::Effect::Save);
    assert_eq!(attempts, EXPECTED_ATTEMPTS_IN_TEN_MINUTES);
    assert_eq!(rig.save_failure(), "missing");

    // The file comes back (a restored mount, a checkout): the next attempt saves the draft.
    fs::write(rig.path(), original).unwrap();
    rig.tick_for(61_000);
    assert_eq!(rig.save_failure(), Value::Null);
    assert!(!rig.dirty());
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "// fn main() {}\n");
}

#[tokio::test]
async fn saving_again_creates_a_deleted_file_with_the_draft_at_once() {
    let mut rig = Rig::new().await;
    rig.lose_the_file_and_type(60_000);
    assert_eq!(rig.save_failure(), "missing");

    let answer = rig.save_again();
    assert_eq!(answer["file"]["dirty"], true);
    // One tick, however long the backoff had grown.
    rig.tick_for(TICK_MS);

    rig.assert_created_with_the_draft();
}

#[tokio::test]
async fn saving_again_with_nothing_to_save_changes_nothing() {
    let mut rig = Rig::new().await;
    let answer = rig.save_again();
    assert_eq!(answer["file"]["dirty"], false);
    assert!(
        rig.fixture
            .try_call("file.save", json!({"fileId":"missing"}))
            .is_err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_folder_without_write_permission_is_told_as_denied_and_saves_once_it_is_fixed() {
    use std::os::unix::fs::PermissionsExt;
    let mut rig = Rig::new().await;
    let project = rig.fixture.project();
    fs::set_permissions(&project, fs::Permissions::from_mode(0o555)).unwrap();
    // Running as the owner of a folder that refuses writes: skip where the system lets it.
    if fs::File::create(project.join(".probe")).is_ok() {
        fs::set_permissions(&project, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    rig.type_text("// ");
    rig.tick_for(2_000);
    let reason = rig.save_failure();
    fs::set_permissions(&project, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(reason, "denied");

    rig.save_again();
    rig.tick_for(TICK_MS);
    assert_eq!(rig.save_failure(), Value::Null);
    assert!(!rig.dirty());
}

#[tokio::test]
async fn a_read_only_file_is_told_as_denied_and_is_not_replaced() {
    let mut rig = Rig::new().await;
    rig.set_read_only(true);
    rig.type_text("// ");
    rig.tick_for(2_000);

    assert_eq!(rig.save_failure(), "denied");
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "fn main() {}\n");
    let told = rig.failures_told().await;
    assert_eq!(told[0]["reason"], "denied");
    // A tab switch or a click in the tree opens the file that is already open: its page says so.
    let opened = rig.fixture.open("main.rs", false);
    assert_eq!(opened["viewport"]["saveFailure"], "denied");

    rig.set_read_only(false);
    rig.save_again();
    rig.tick_for(TICK_MS);
    assert_eq!(rig.save_failure(), Value::Null);
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "// fn main() {}\n");
    let opened = rig.fixture.open("main.rs", false);
    assert_eq!(opened["viewport"]["saveFailure"], Value::Null);
}

/// Fails the first write or creation of a file with a full disk, as the disk does when a
/// person deletes things to make room and saves again.
struct FullOnce(AtomicBool);

impl Interference for FullOnce {
    fn before_write(&self, _path: &Path) -> io::Result<()> {
        if self.0.swap(true, Ordering::SeqCst) {
            Ok(())
        } else {
            Err(io::ErrorKind::StorageFull.into())
        }
    }
}

#[tokio::test]
async fn a_failed_recreate_leaves_no_file_and_never_turns_the_draft_into_a_conflict() {
    let port = InterferedFiles(FullOnce(AtomicBool::new(false)));
    let mut rig = Rig::over(Box::new(port)).await;
    rig.lose_the_file_and_type(2_000);
    assert_eq!(rig.save_failure(), "missing");

    // The creation fails with a full disk: nothing is on the disk, not even an empty file.
    rig.save_again();
    rig.tick_for(TICK_MS);
    assert_eq!(rig.save_failure(), "full");
    assert!(!rig.path().exists(), "no empty file is left behind");

    // Later attempts, however many, find the file still gone: never a change by someone else.
    rig.tick_for(120_000);
    let file_id = rig.file_id.clone();
    let read = rig.fixture.read(&file_id);
    assert_eq!(read["file"]["conflicted"], false);
    assert_eq!(read["file"]["dirty"], true);
    assert_eq!(read["saveFailure"], "missing");

    // And the next save again creates it with the draft.
    rig.save_again();
    rig.tick_for(TICK_MS);
    rig.assert_created_with_the_draft();
}

#[tokio::test]
async fn a_project_replace_still_replaces_a_read_only_file_as_it_always_did() {
    let mut fixture = EditorFixture::new(&[("a.rs", "needle\n"), ("b.rs", "needle\n")]).await;
    let b = fixture.project().join("b.rs");
    let mut permissions = fs::metadata(&b).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&b, permissions).unwrap();
    let workspace_id = fixture.workspace_id.clone();
    let replaced = fixture.try_call(
        "file.search",
        json!({"workspaceId":workspace_id,"query":"needle","replacement":"safe",
            "caseSensitive":true,"limit":10,"idempotencyKey":"replace-1"}),
    );
    let mut permissions = fs::metadata(&b).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&b, permissions).unwrap();

    assert!(replaced.is_ok(), "{replaced:?}");
    assert_eq!(
        fs::read_to_string(fixture.project().join("a.rs")).unwrap(),
        "safe\n"
    );
    assert_eq!(fs::read_to_string(&b).unwrap(), "safe\n");
}

#[tokio::test]
async fn a_conflict_ends_the_failure_and_saving_again_does_not_touch_it() {
    let mut rig = Rig::new().await;
    rig.lose_the_file_and_type(2_000);
    assert_eq!(rig.save_failure(), "missing");

    // Someone else writes the file again: the draft and the disk differ, which is a conflict,
    // and a conflict is not a failed save.
    fs::write(rig.path(), "external\n").unwrap();
    rig.tick_for(61_000);
    let file_id = rig.file_id.clone();
    let read = rig.fixture.read(&file_id);
    assert_eq!(read["file"]["conflicted"], true);
    assert_eq!(read["saveFailure"], Value::Null);

    rig.save_again();
    rig.tick_for(TICK_MS);
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "external\n");
}

#[tokio::test]
async fn a_closed_tab_stops_retrying_and_keeps_its_draft() {
    let mut rig = Rig::new().await;
    rig.lose_the_file_and_type(2_000);
    assert_eq!(rig.save_failure(), "missing");

    let pane_id = rig.fixture.read(&rig.file_id.clone())["file"]["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    rig.fixture.call(
        "pane.close",
        json!({"paneId":pane_id,"idempotencyKey":"close-1"}),
    );
    rig.tick_for(TEN_MINUTES_MS);

    assert_eq!(
        rig.fixture
            .core
            .editor
            .effect_attempts(&rig.file_id, super::Effect::Save),
        0,
        "a closed tab has no autosave"
    );
    assert!(!rig.path().exists());
    let stored = rig
        .fixture
        .core
        .store
        .editor_file(&rig.file_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.draft_text.as_deref(), Some("// fn main() {}\n"));
}

#[tokio::test]
async fn a_client_that_never_saw_the_event_learns_the_failure_by_reading() {
    let mut rig = Rig::new().await;
    rig.lose_the_file_and_type(2_000);
    // The event went to the clients that were connected; this one reads afterwards.
    assert!(!rig.failures_told().await.is_empty());
    assert!(rig.failures_told().await.is_empty());
    assert_eq!(rig.save_failure(), "missing");
}

/// A disk whose write fails with the kind set by the test, or that the owner made read-only
/// without the disk being touched.
#[derive(Default)]
struct Scripted {
    failing: Mutex<Option<io::ErrorKind>>,
    read_only: AtomicBool,
}

impl Scripted {
    fn fail_with(&self, kind: Option<io::ErrorKind>) {
        *self.failing.lock().unwrap() = kind;
    }
}

impl Interference for Arc<Scripted> {
    fn before_write(&self, _path: &Path) -> io::Result<()> {
        match *self.failing.lock().unwrap() {
            Some(kind) => Err(kind.into()),
            None => Ok(()),
        }
    }

    fn read_only(&self, _path: &Path) -> bool {
        self.read_only.load(Ordering::SeqCst)
    }
}

async fn scripted_rig() -> (Rig, Arc<Scripted>) {
    let disk = Arc::new(Scripted::default());
    let rig = Rig::over(Box::new(InterferedFiles(Arc::clone(&disk)))).await;
    (rig, disk)
}

#[tokio::test]
async fn a_port_that_reports_a_read_only_file_refuses_the_save_without_touching_the_disk() {
    let (mut rig, disk) = scripted_rig().await;
    disk.read_only.store(true, Ordering::SeqCst);
    rig.type_text("// ");
    rig.tick_for(2_000);

    assert_eq!(rig.save_failure(), "denied");
    assert!(rig.dirty(), "the draft is kept");
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "fn main() {}\n");
    assert_eq!(rig.reasons_told().await[0], "denied");
    let real = fs::metadata(rig.path()).unwrap().permissions();
    assert!(!real.readonly(), "the file on the disk was never changed");
}

#[tokio::test]
async fn a_failure_that_repeats_is_told_when_the_streak_says_not_on_every_retry() {
    let (mut rig, disk) = scripted_rig().await;
    disk.fail_with(Some(io::ErrorKind::PermissionDenied));
    rig.type_text("// ");

    // The streak reports the first failure and attempts 2, 4, 8, so the third and fifth are
    // retried without a new announcement.
    let mut told = Vec::new();
    for attempts in 1..=5 {
        rig.fail_until(attempts);
        told.push(rig.reasons_told().await.len());
    }
    assert_eq!(told, [1, 1, 0, 1, 0]);
}

#[tokio::test]
async fn a_new_kind_of_failure_is_told_again_and_a_recovery_starts_the_count_over() {
    let (mut rig, disk) = scripted_rig().await;
    disk.fail_with(Some(io::ErrorKind::PermissionDenied));
    rig.type_text("// ");
    rig.fail_until(2);
    assert_eq!(rig.reasons_told().await, ["denied", "denied"]);

    // Attempt 3 is no power of two: only the change of kind makes it worth telling.
    disk.fail_with(Some(io::ErrorKind::StorageFull));
    rig.fail_until(3);
    assert_eq!(rig.reasons_told().await, ["full"]);

    // Attempt 4 is told and 5 is not; a recovery ends the streak, so the next failure is a first.
    rig.fail_until(5);
    assert_eq!(rig.reasons_told().await, ["full"]);
    disk.fail_with(None);
    rig.save_again();
    rig.tick_for(TICK_MS);
    assert_eq!(rig.save_failure(), Value::Null);
    rig.reasons_told().await;

    disk.fail_with(Some(io::ErrorKind::PermissionDenied));
    rig.type_text("// ");
    rig.fail_until(1);
    assert_eq!(rig.reasons_told().await, ["denied"]);
}
