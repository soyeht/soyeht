//! The autosave and the disk watch run off the Core lock, and their results are applied to the
//! file as it is when they finish (ADR 0097).

use super::{DiskJob, DiskResult, Effect, Snapshot, close_key, job_key};
use crate::{
    dispatch::Outcome,
    editor::{
        interfered_files::{InterferedFiles, Interference},
        test_fixture::EditorFixture,
    },
    offload::Background,
};
use serde_json::json;
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// A point a disk operation can be held at, as a slow disk holds it.
#[derive(Default)]
struct Gate {
    armed: AtomicBool,
    denied: AtomicBool,
    entered: AtomicUsize,
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    /// Makes the operations that reach the gate fail, as a disk that refuses the write.
    fn deny(&self) {
        self.denied.store(true, Ordering::SeqCst);
    }

    fn pass(&self) {
        if !self.armed.load(Ordering::SeqCst) {
            return;
        }
        self.entered.fetch_add(1, Ordering::SeqCst);
        let open = self.open.lock().unwrap();
        // A test that fails must not leave the worker waiting for good.
        let _ = self
            .changed
            .wait_timeout_while(open, Duration::from_secs(5), |open| !*open)
            .unwrap();
    }

    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }

    /// Holds the next operation again, after a `release`.
    fn hold_again(&self) {
        *self.open.lock().unwrap() = false;
    }

    fn wait_for_visitors(&self, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.entered.load(Ordering::SeqCst) < count {
            assert!(
                Instant::now() < deadline,
                "no disk operation reached the gate"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn visitors(&self) -> usize {
        self.entered.load(Ordering::SeqCst)
    }
}

/// A gate before every read and write, and an optional change of the file that lands just
/// before a write, as another program's would.
struct Gated {
    reads: Arc<Gate>,
    writes: Arc<Gate>,
    before_write: Mutex<Option<String>>,
}

impl Gated {
    fn new(reads: &Arc<Gate>, writes: &Arc<Gate>) -> InterferedFiles<Self> {
        InterferedFiles(Self {
            reads: Arc::clone(reads),
            writes: Arc::clone(writes),
            before_write: Mutex::new(None),
        })
    }
}

impl Interference for Gated {
    fn before_read(&self) {
        self.reads.pass();
    }

    fn before_write(&self, path: &Path) -> io::Result<()> {
        self.writes.pass();
        if self.writes.denied.load(Ordering::SeqCst) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        match self.before_write.lock().unwrap().take() {
            Some(external) => fs::write(path, external),
            None => Ok(()),
        }
    }
}

struct Rig {
    fixture: EditorFixture,
    file_id: String,
    reads: Arc<Gate>,
    writes: Arc<Gate>,
}

impl Rig {
    /// `main.rs` holding "alpha\n", open, over a disk whose gates are open.
    async fn new() -> Self {
        Self::over(Gated::new(
            &Arc::new(Gate::default()),
            &Arc::new(Gate::default()),
        ))
        .await
    }

    async fn over(port: InterferedFiles<Gated>) -> Self {
        let (reads, writes) = (Arc::clone(&port.0.reads), Arc::clone(&port.0.writes));
        let mut fixture = EditorFixture::over(&[("main.rs", "alpha\n")], Box::new(port)).await;
        let file_id = fixture.open("main.rs", false)["file"]["fileId"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            fixture,
            file_id,
            reads,
            writes,
        }
    }

    /// The draft "alpha beta" of `main.rs` being saved: its writes are held at a gate, and the
    /// job has started and waits for the disk. Also returns how long the tick took.
    async fn saving() -> (Self, Duration) {
        let mut rig = Self::typed_over_a_held_disk().await;
        rig.fixture.at(10_000);
        let started = Instant::now();
        rig.fixture.core.advance_editor_effects();
        let tick = started.elapsed();
        rig.writes.wait_for_visitors(1);
        (rig, tick)
    }

    fn file(&self) -> soyeht_store::editor::EditorFile {
        self.fixture
            .core
            .editor
            .stored(&self.file_id)
            .unwrap()
            .clone()
    }

    fn path(&self) -> PathBuf {
        self.fixture.project().join("main.rs")
    }

    fn type_at(&mut self, column: u32, text: &str) {
        let file_id = self.file_id.clone();
        self.fixture.type_at(&file_id, 0, column, text);
    }

    /// The save of the draft as it is now, copied under the lock but not yet run.
    fn prepared_save(&mut self) -> (DiskJob, Snapshot) {
        self.fixture
            .core
            .prepare_disk_job(&self.file_id, Effect::Save)
            .unwrap()
            .unwrap()
    }

    /// The save of the draft as it is now, run to the end of its disk work but not applied: the
    /// state of an autosave whose write has finished and whose result waits for the lock.
    fn finished_save(&mut self) -> (Snapshot, DiskResult) {
        let (job, snapshot) = self.prepared_save();
        (snapshot, job.run().unwrap())
    }

    fn apply(&mut self, snapshot: &Snapshot, result: DiskResult) {
        let file_id = self.file_id.clone();
        self.fixture
            .core
            .apply_disk_result(&file_id, snapshot, result, 5_000)
            .unwrap();
    }

    fn versions(&self) -> Vec<(String, String)> {
        self.fixture
            .core
            .store
            .file_history(&self.file_id, None, 50)
            .unwrap()
            .items
            .into_iter()
            .map(|version| (version.revision.to_string(), version.byte_count.to_string()))
            .collect()
    }

    /// Ticks at `now_ms`, so that what is due then runs.
    fn tick_at(&mut self, now_ms: i64) {
        self.fixture.at(now_ms);
        self.fixture.core.advance_editor().unwrap();
    }
}

#[tokio::test]
async fn a_draft_typed_while_the_write_ran_stays_dirty_and_is_saved_next() {
    let mut rig = Rig::new().await;
    rig.type_at(5, " beta");
    let (snapshot, written) = rig.finished_save();
    assert!(matches!(written, DiskResult::Written(_)));
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "alpha beta\n");

    // The person types before the result is applied.
    rig.type_at(10, " gamma");
    let typed = rig.file();
    rig.apply(&snapshot, written);

    let file = rig.file();
    assert_eq!(
        file.disk_text, "alpha beta\n",
        "the disk holds what was written"
    );
    assert_eq!(file.draft_text.as_deref(), Some("alpha beta gamma\n"));
    assert_eq!(
        file.revision, typed.revision,
        "the newer draft keeps its revision"
    );
    assert!(file.draft_author.is_some());
    assert_eq!(file.conflict_text, None);
    // The version is the one written, at the revision written, not the newer draft.
    let versions = rig.versions();
    assert!(
        versions.contains(&(
            snapshot.revision.to_string(),
            "alpha beta\n".len().to_string()
        )),
        "{versions:?}"
    );
    assert!(
        !versions
            .iter()
            .any(|(revision, _)| *revision == typed.revision.to_string()),
        "{versions:?}"
    );

    // The newer draft is saved by the next autosave.
    rig.tick_at(10_000);
    assert_eq!(
        fs::read_to_string(rig.path()).unwrap(),
        "alpha beta gamma\n"
    );
    let file = rig.file();
    assert_eq!(file.draft_text, None);
    assert_eq!(file.disk_text, "alpha beta gamma\n");
}

#[tokio::test]
async fn an_unchanged_draft_is_saved_and_cleared_at_its_own_revision() {
    let mut rig = Rig::new().await;
    rig.type_at(5, " beta");
    let (snapshot, written) = rig.finished_save();
    rig.apply(&snapshot, written);
    let file = rig.file();
    assert_eq!(file.draft_text, None);
    assert_eq!(file.draft_author, None);
    assert_eq!(file.disk_text, "alpha beta\n");
    assert_eq!(file.revision, snapshot.revision);
    assert_eq!(file.disk_revision, snapshot.basis.disk_revision + 1);
}

#[tokio::test]
async fn a_disk_change_found_before_the_write_is_a_conflict_with_the_newer_draft_and_loses_nothing()
{
    let mut rig = Rig::new().await;
    rig.type_at(5, " beta");
    let (job, snapshot) = rig.prepared_save();
    // Another program rewrites the file, then the person types more, then the job reads.
    fs::write(rig.path(), "external\n").unwrap();
    rig.type_at(10, " gamma");
    let result = job.run().unwrap();
    assert_eq!(result, DiskResult::Changed("external\n".into()));
    rig.apply(&snapshot, result);

    let file = rig.file();
    assert_eq!(file.conflict_text.as_deref(), Some("external\n"));
    assert_eq!(file.draft_text.as_deref(), Some("alpha beta gamma\n"));
    assert_eq!(
        fs::read_to_string(rig.path()).unwrap(),
        "external\n",
        "never overwritten"
    );
    let read = rig.fixture.read(&rig.file_id.clone());
    assert_eq!(read["file"]["conflicted"], true);
    assert_eq!(read["file"]["dirty"], true);

    // A conflict is not saved over, however long it waits.
    rig.tick_at(60_000);
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "external\n");
}

#[tokio::test]
async fn a_disk_change_that_equals_the_draft_finishes_the_save() {
    let mut rig = Rig::new().await;
    rig.type_at(5, " beta");
    let (job, snapshot) = rig.prepared_save();
    fs::write(rig.path(), "alpha beta\n").unwrap();
    let result = job.run().unwrap();
    assert_eq!(result, DiskResult::Changed("alpha beta\n".into()));
    rig.apply(&snapshot, result);
    let file = rig.file();
    assert_eq!((file.draft_text, file.conflict_text), (None, None));
    assert_eq!(file.disk_text, "alpha beta\n");
}

#[tokio::test]
async fn a_file_that_changes_between_the_read_and_the_write_is_reported_not_overwritten() {
    let port = Gated::new(&Arc::new(Gate::default()), &Arc::new(Gate::default()));
    *port.0.before_write.lock().unwrap() = Some("external\n".into());
    let mut rig = Rig::over(port).await;
    rig.type_at(5, " beta");
    let (_, result) = rig.finished_save();
    assert_eq!(result, DiskResult::Changed("external\n".into()));
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "external\n");
}

#[tokio::test]
async fn a_disk_change_after_the_write_is_found_by_the_next_check() {
    let mut rig = Rig::new().await;
    rig.type_at(5, " beta");
    let (snapshot, written) = rig.finished_save();
    // Another program rewrites the file after the rename, and the person types before the
    // result is applied.
    fs::write(rig.path(), "external\n").unwrap();
    rig.type_at(10, " gamma");
    rig.apply(&snapshot, written);
    assert_eq!(rig.file().disk_text, "alpha beta\n");

    rig.tick_at(10_000);
    let file = rig.file();
    assert_eq!(file.conflict_text.as_deref(), Some("external\n"));
    assert_eq!(file.draft_text.as_deref(), Some("alpha beta gamma\n"));
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "external\n");
    // What was written is in the history, so the person can restore it.
    assert!(rig.versions().contains(&(
        snapshot.revision.to_string(),
        "alpha beta\n".len().to_string()
    )));
}

#[tokio::test]
async fn a_result_for_a_file_closed_meanwhile_is_dropped() {
    let mut rig = Rig::new().await;
    rig.type_at(5, " beta");
    let (snapshot, written) = rig.finished_save();
    rig.fixture.core.editor.open.remove(&rig.file_id);
    rig.apply(&snapshot, written);
    assert!(rig.fixture.core.editor.stored(&rig.file_id).is_err());
}

#[tokio::test]
async fn a_save_waiting_for_the_disk_holds_neither_the_tick_nor_a_second_write() {
    let (mut rig, tick) = Rig::saving().await;
    assert!(
        tick < Duration::from_secs(1),
        "the tick waited {tick:?} for the write"
    );

    // Later ticks neither wait nor start a second write for the file, and neither does a
    // person's close-time save.
    for step in 1..=3 {
        rig.fixture.at(10_000 + step * 600);
        let started = Instant::now();
        rig.fixture.core.advance_editor_effects();
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    // A close-time save leaves the file to the job that is writing it.
    let file_id = rig.file_id.clone();
    rig.fixture.core.start_pending_save(&file_id);
    assert_eq!(rig.writes.visitors(), 1);
    assert!(
        rig.file().draft_text.is_some(),
        "still unsaved while the disk is slow"
    );

    rig.writes.release();
    let deadline = Instant::now() + Duration::from_secs(10);
    while rig.file().draft_text.is_some() {
        assert!(
            Instant::now() < deadline,
            "the finished save was never applied"
        );
        rig.fixture.core.apply_background();
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "alpha beta\n");
    assert_eq!(rig.writes.visitors(), 1);
}

#[tokio::test]
async fn a_disk_check_waiting_for_the_disk_does_not_hold_the_tick() {
    let mut rig = Rig::new().await;
    rig.fixture.core.background = Background::default();
    rig.reads.arm();
    fs::write(rig.path(), "external\n").unwrap();

    let started = Instant::now();
    rig.tick_at(10_000);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the tick waited {:?} for the read",
        started.elapsed()
    );
    rig.reads.wait_for_visitors(1);
    assert_eq!(
        rig.file().disk_text,
        "alpha\n",
        "not applied before the read ends"
    );

    rig.reads.release();
    let deadline = Instant::now() + Duration::from_secs(10);
    while rig.file().disk_text != "external\n" {
        assert!(
            Instant::now() < deadline,
            "the finished check was never applied"
        );
        rig.fixture.core.apply_background();
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(rig.file().draft_text, None);
}

/// Whether a disk job, or the job that saves a closed tab, is working on the file.
fn job_running(core: &crate::core::Core, file_id: &str) -> bool {
    core.background.is_active(&job_key(file_id)) || core.background.is_active(&close_key(file_id))
}

impl Rig {
    /// The draft "alpha beta" of `main.rs` typed, with the writes of the disk held at their gate and
    /// the jobs running on the workers.
    async fn typed_over_a_held_disk() -> Self {
        let mut rig = Self::new().await;
        rig.fixture.core.background = Background::default();
        rig.writes.arm();
        rig.type_at(5, " beta");
        rig
    }

    /// Applies finished jobs until `done`.
    fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(self) {
            assert!(Instant::now() < deadline, "{what}");
            self.fixture.core.apply_background();
            thread::sleep(Duration::from_millis(5));
        }
    }
}

#[tokio::test]
async fn closing_a_tab_with_a_draft_does_not_wait_for_the_disk() {
    let mut rig = Rig::typed_over_a_held_disk().await;
    let pane = rig.file().pane_id.unwrap();

    let started = Instant::now();
    rig.fixture.call(
        "pane.close",
        json!({"paneId":pane,"idempotencyKey":"close"}),
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the close waited {:?} for the write",
        started.elapsed()
    );
    // The save has started and is held at the disk: nothing is written, and the tab is closed.
    rig.writes.wait_for_visitors(1);
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "alpha\n");
    assert!(!rig.fixture.core.editor.is_open(&rig.file_id));

    rig.writes.release();
    rig.until("the draft of the closed tab was never saved", |rig| {
        fs::read_to_string(rig.path()).is_ok_and(|text| text == "alpha beta\n")
    });
    assert_eq!(rig.writes.visitors(), 1);
}

impl Rig {
    /// Asks to delete `path` for good, as a client does, and returns what dispatch gives back.
    fn delete(&mut self, path: &str) -> Outcome {
        let request = json!({
            "jsonrpc":"2.0","id":900,"method":"project.entry.delete",
            "params":{
                "workspaceId":self.fixture.workspace_id,"path":path,"permanent":true,
                "idempotencyKey":Uuid::new_v4().to_string()
            }
        });
        let connection = self.fixture.connection.clone();
        self.fixture.core.dispatch(&connection, request).unwrap()
    }

    /// Lets the request wait, as the server does, and checks that the delete was carried out.
    fn finish_deleting(&mut self, offload: crate::offload::Offload) {
        let Outcome::Reply(reply) = offload.run_inline(&mut self.fixture.core).unwrap() else {
            panic!("expected the reply to the delete");
        };
        assert_eq!(reply["outcome"], "deleted");
    }

    /// The request waiting for the saves it started, off the lock.
    fn waiting(outcome: Outcome) -> crate::offload::Offload {
        let Outcome::Offload(offload) = outcome else {
            panic!("the delete should wait for the save off the lock");
        };
        offload
    }
}

#[tokio::test]
async fn deleting_a_file_with_a_draft_does_not_wait_for_the_disk_and_then_deletes_it() {
    let mut rig = Rig::typed_over_a_held_disk().await;

    let started = Instant::now();
    let outcome = rig.delete("main.rs");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the delete waited {:?} for the write",
        started.elapsed()
    );
    // The request is answered once the save is done: it waits for the job off the lock.
    let offload = Rig::waiting(outcome);
    rig.writes.wait_for_visitors(1);
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "alpha\n");

    rig.writes.release();
    rig.finish_deleting(offload);
    assert!(!rig.path().exists());
    assert_eq!(rig.writes.visitors(), 1);
}

#[tokio::test]
async fn a_disk_check_running_when_a_file_is_deleted_does_not_keep_its_draft_from_being_saved() {
    let mut rig = Rig::new().await;
    rig.fixture.core.background = Background::default();
    rig.reads.arm();
    rig.type_at(5, " beta");
    // The disk check is due, the autosave is not yet: a check is running when the delete comes.
    rig.tick_at(1_100);
    rig.reads.wait_for_visitors(1);

    let offload = Rig::waiting(rig.delete("main.rs"));
    assert!(rig.path().exists());
    rig.reads.release();
    // The delete waits for the check, saves what the check kept from starting, waits for that
    // too, and then deletes.
    rig.finish_deleting(offload);
    assert!(!rig.path().exists());
    assert!(
        !rig.versions().is_empty(),
        "the draft was saved before the file went"
    );
}

#[tokio::test]
async fn text_typed_while_a_delete_waits_for_the_save_refuses_the_delete_and_keeps_the_file() {
    let mut rig = Rig::typed_over_a_held_disk().await;
    let offload = Rig::waiting(rig.delete("main.rs"));
    rig.writes.wait_for_visitors(1);
    rig.type_at(10, " gamma");

    rig.writes.release();
    let refused = offload.run_inline(&mut rig.fixture.core).err().unwrap();
    assert!(refused.to_string().contains("unsaved changes"), "{refused}");
    // What was saved is on the disk, what was typed after it is still a draft, nothing was removed.
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "alpha beta\n");
    assert_eq!(rig.file().draft_text.as_deref(), Some("alpha beta gamma\n"));
}

#[tokio::test]
async fn a_save_that_fails_refuses_the_delete_and_says_why() {
    let mut rig = Rig::new().await;
    rig.fixture.core.background = Background::default();
    rig.writes.deny();
    rig.type_at(5, " beta");

    let offload = Rig::waiting(rig.delete("main.rs"));
    let refused = offload.run_inline(&mut rig.fixture.core).err().unwrap();
    let message = refused.to_string();
    assert!(message.contains("unsaved changes"), "{message}");
    assert!(message.contains("permission was denied"), "{message}");
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "alpha\n");
    assert_eq!(rig.file().draft_text.as_deref(), Some("alpha beta\n"));
}

#[tokio::test]
async fn deleting_a_folder_saves_every_draft_under_it_and_then_deletes_it() {
    let files = [
        ("src/a.rs", "a\n"),
        ("src/b.rs", "b\n"),
        ("src/c.rs", "c\n"),
    ];
    let mut fixture = EditorFixture::new(&files).await;
    fixture.core.background = Background::default();
    let ids: Vec<String> = files
        .iter()
        .map(|(path, _)| {
            fixture.open(path, false)["file"]["fileId"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    for id in &ids {
        fixture.type_at(id, 0, 1, " typed");
    }
    let mut rig = Rig {
        fixture,
        file_id: ids[0].clone(),
        reads: Arc::new(Gate::default()),
        writes: Arc::new(Gate::default()),
    };

    let offload = Rig::waiting(rig.delete("src"));
    rig.finish_deleting(offload);
    assert!(!rig.fixture.project().join("src").exists());
    for id in &ids {
        let saved = rig.fixture.core.store.file_history(id, None, 50).unwrap();
        assert_eq!(saved.items.len(), 1, "the draft of {id} was saved first");
    }
}

mod races;

#[test]
fn the_folder_of_a_workspace_is_read_from_the_database_once() {
    let (mut core, root) = crate::test_panes::core_with_agent(&[]);
    let first = core.workspace_root("workspace").unwrap();
    assert_eq!(first, root);

    let statements = soyeht_store::test_support::statements_started();
    for _ in 0..100 {
        assert_eq!(core.workspace_root("workspace").unwrap(), first);
    }

    assert_eq!(
        soyeht_store::test_support::statements_started() - statements,
        0,
        "the disk watch asked the database for the folder again"
    );
    // A workspace that does not exist is not remembered.
    assert!(core.workspace_root("missing").is_err());
    assert!(!core.workspace_roots.contains_key("missing"));
}
