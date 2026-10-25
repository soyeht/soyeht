//! A disk job runs while the person acts: a rename, a delete, a closed tab, a second workspace
//! on the same folder. Each of these once lost an edit or wrote over another one (ADR 0097).

use super::{Gate, Rig};
use crate::{dispatch::Outcome, editor::disk_effects::job_key, errors::Error, offload::Background};
use serde_json::{Value, json};
use std::{
    fs,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

fn key() -> String {
    Uuid::new_v4().to_string()
}

/// Closes the tab of the rig's file, as the person does.
fn close_tab(rig: &mut Rig) {
    let pane_id = rig.file().pane_id.unwrap();
    rig.fixture.call(
        "pane.close",
        json!({"paneId":pane_id,"idempotencyKey":key()}),
    );
}

/// Lets the save in flight reach the disk, but leaves its result unapplied and holds the next
/// write at the gate again.
fn finish_first_save_and_hold_the_disk(rig: &Rig) {
    let first = rig
        .fixture
        .core
        .background
        .done_of(&job_key(&rig.file_id))
        .unwrap();
    rig.writes.release();
    assert!(first.wait(Duration::from_secs(5)));
    rig.writes.hold_again();
}

fn release_later(gate: &Arc<Gate>, after: Duration) {
    let gate = Arc::clone(gate);
    thread::spawn(move || {
        thread::sleep(after);
        gate.release();
    });
}

/// Applies finished jobs the way the tick does, until `done` holds.
fn tick_until(rig: &mut Rig, what: &str, done: impl Fn(&mut Rig) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(rig) {
        assert!(Instant::now() < deadline, "{what}");
        rig.fixture.core.apply_background();
        thread::sleep(Duration::from_millis(5));
    }
}

fn rename(rig: &mut Rig, from: &str, to: &str) -> crate::errors::Result<Value> {
    let workspace_id = rig.fixture.workspace_id.clone();
    rig.fixture.try_call(
        "project.entry.rename",
        json!({"workspaceId":workspace_id,"from":from,"to":to,"idempotencyKey":key()}),
    )
}

#[tokio::test]
async fn a_rename_waits_for_the_save_in_flight_instead_of_losing_the_edit() {
    let mut rig = Rig::saving().await.0;
    release_later(&rig.writes, Duration::from_millis(300));

    rename(&mut rig, "main.rs", "moved.rs").unwrap();

    // The write reached the file before it moved; nothing was recreated at the old name.
    let project = rig.fixture.project();
    assert!(!project.join("main.rs").exists());
    assert_eq!(
        fs::read_to_string(project.join("moved.rs")).unwrap(),
        "alpha beta\n"
    );
    let file = rig.file();
    assert_eq!(file.relative_path, "moved.rs");
    assert_eq!(file.draft_text, None);
    assert_eq!(file.disk_text, "alpha beta\n");
}

#[tokio::test]
async fn a_rename_or_delete_that_cannot_wait_is_refused_and_moves_nothing() {
    let mut rig = Rig::saving().await.0;

    let refused = rename(&mut rig, "main.rs", "moved.rs");
    assert!(
        matches!(refused, Err(Error::StaleVersion(_))),
        "{refused:?}"
    );
    let workspace_id = rig.fixture.workspace_id.clone();
    let deleted = rig.fixture.try_call(
        "project.entry.delete",
        json!({"workspaceId":workspace_id,"path":"main.rs","permanent":true,"idempotencyKey":key()}),
    );
    assert!(
        matches!(deleted, Err(Error::StaleVersion(_))),
        "{deleted:?}"
    );
    let project = rig.fixture.project();
    assert_eq!(
        fs::read_to_string(project.join("main.rs")).unwrap(),
        "alpha\n"
    );
    assert!(!project.join("moved.rs").exists());

    // Once the disk answers, the save completes at the old place and the person can retry.
    rig.writes.release();
    tick_until(&mut rig, "the save was never applied", |rig| {
        rig.file().draft_text.is_none()
    });
    rename(&mut rig, "main.rs", "moved.rs").unwrap();
    assert_eq!(
        fs::read_to_string(project.join("moved.rs")).unwrap(),
        "alpha beta\n"
    );
}

#[tokio::test]
async fn a_result_for_a_file_that_moved_never_marks_it_saved() {
    let mut rig = Rig::new().await;
    rig.type_at(5, " beta");
    let (snapshot, written) = rig.finished_save();
    rename(&mut rig, "main.rs", "moved.rs").unwrap();
    rig.apply(&snapshot, written);
    let file = rig.file();
    assert_eq!(file.draft_text.as_deref(), Some("alpha beta\n"));
    assert_eq!(file.disk_text, "alpha\n");
}

#[tokio::test]
async fn a_tab_closed_during_the_write_still_gets_the_text_typed_after_it() {
    let mut rig = Rig::saving().await.0;
    // The person types more while the write waits, then closes the tab.
    rig.type_at(10, " gamma");
    close_tab(&mut rig);
    assert!(!rig.fixture.core.editor.is_open(&rig.file_id));
    rig.writes.release();

    let stored = |rig: &Rig| {
        rig.fixture
            .core
            .store
            .editor_file(&rig.file_id)
            .unwrap()
            .unwrap()
    };
    tick_until(&mut rig, "the closed file was never finished", |rig| {
        let row = stored(rig);
        row.draft_text.is_none() && row.disk_text == "alpha beta gamma\n"
    });
    assert_eq!(
        fs::read_to_string(rig.path()).unwrap(),
        "alpha beta gamma\n"
    );
    let row = stored(&rig);
    assert_eq!(row.draft_text, None);
    assert_eq!(row.conflict_text, None);
    assert_eq!(row.disk_text, "alpha beta gamma\n");

    // Opening it again shows the text, with no conflict with its own write.
    let reopened = rig.fixture.open("main.rs", false);
    assert_eq!(reopened["file"]["conflicted"], false);
    assert_eq!(reopened["file"]["dirty"], false);
}

#[tokio::test]
async fn two_workspaces_on_one_folder_saving_at_once_leave_one_conflict_not_an_overwrite() {
    let mut rig = Rig::new().await;
    rig.fixture.core.background = Background::default();
    let project = rig.fixture.project();
    let second_workspace = rig.fixture.call(
        "workspace.create",
        json!({"name":"Second","path":project,"idempotencyKey":key()}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let second = rig.fixture.call(
        "file.open",
        json!({"workspaceId":second_workspace,"path":"main.rs","preview":false,"idempotencyKey":key()}),
    )["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = rig.file_id.clone();
    rig.writes.arm();
    rig.fixture.type_at(&first, 0, 5, " one");
    rig.fixture.type_at(&second, 0, 5, " two");

    // Both jobs read "alpha" and wait for the disk, then both write at once.
    rig.fixture.at(10_000);
    rig.fixture.core.advance_editor_effects();
    rig.writes.wait_for_visitors(2);
    rig.writes.release();
    let editor = |rig: &Rig, id: &str| rig.fixture.core.editor.stored(id).unwrap().clone();
    tick_until(&mut rig, "the saves were never applied", |rig| {
        !rig.fixture.core.disk_job_running(&first) && !rig.fixture.core.disk_job_running(&second)
    });

    let disk = fs::read_to_string(project.join("main.rs")).unwrap();
    let (a, b) = (editor(&rig, &first), editor(&rig, &second));
    let (winner, loser) = if a.draft_text.is_none() {
        (a, b)
    } else {
        (b, a)
    };
    assert_eq!(winner.draft_text, None, "exactly one save succeeded");
    assert_eq!(winner.disk_text, disk, "the disk holds the winner's text");
    assert!(
        disk == "alpha one\n" || disk == "alpha two\n",
        "unexpected text {disk:?}"
    );
    assert_eq!(loser.conflict_text.as_deref(), Some(disk.as_str()));
    assert!(loser.draft_text.is_some(), "the loser's text is kept");
}

/// The first save ends and the person's newer text starts a second one before a rename's apply
/// step runs. The apply step cannot offload again (an offload is resolved once), so it refuses.
#[tokio::test]
async fn a_rename_that_finds_another_save_started_is_refused_not_offloaded_again() {
    let mut rig = Rig::saving().await.0;
    rig.type_at(10, " gamma");
    let workspace_id = rig.fixture.workspace_id.clone();
    let connection = rig.fixture.connection.clone();
    let request = json!({
        "jsonrpc":"2.0","id":9001,"method":"project.entry.rename",
        "params":{"workspaceId":workspace_id,"from":"main.rs","to":"moved.rs","idempotencyKey":key()}
    });
    let Outcome::Offload(offload) = rig.fixture.core.dispatch(&connection, request).unwrap() else {
        panic!("a rename that meets a save waits off the lock")
    };

    finish_first_save_and_hold_the_disk(&rig);
    rig.fixture.core.apply_background();
    rig.fixture.at(20_000);
    rig.fixture.core.advance_editor_effects();
    rig.writes.wait_for_visitors(2);

    let refused = offload.run_inline(&mut rig.fixture.core);
    assert!(
        matches!(refused, Err(Error::StaleVersion(_))),
        "{:?}",
        refused.map(|_| ())
    );
    let project = rig.fixture.project();
    assert!(project.join("main.rs").exists());
    assert!(!project.join("moved.rs").exists());
    rig.writes.release();
}

/// The tab closed while its save was in flight: the job that saves what it had left is writing
/// when the rename arrives, and the rename waits for it too.
#[tokio::test]
async fn a_rename_waits_for_the_job_that_saves_a_closed_tab() {
    let mut rig = Rig::saving().await.0;
    rig.type_at(10, " gamma");
    close_tab(&mut rig);
    finish_first_save_and_hold_the_disk(&rig);
    // The first job is applied; the job for the rest of the closed tab starts and waits.
    rig.fixture.core.apply_background();
    rig.writes.wait_for_visitors(2);
    assert!(rig.fixture.core.disk_job_running(&rig.file_id));
    release_later(&rig.writes, Duration::from_millis(300));

    rename(&mut rig, "main.rs", "moved.rs").unwrap();

    let project = rig.fixture.project();
    assert!(
        !project.join("main.rs").exists(),
        "no stray file at the old name"
    );
    assert_eq!(
        fs::read_to_string(project.join("moved.rs")).unwrap(),
        "alpha beta gamma\n"
    );
}

#[tokio::test]
async fn saving_again_while_an_autosave_is_in_flight_still_creates_the_deleted_file() {
    let mut rig = Rig::saving().await.0;
    // The file goes away while the autosave waits for the disk, and the person saves again.
    fs::remove_file(rig.path()).unwrap();
    rig.fixture
        .call("file.save", json!({"fileId":rig.file_id.clone()}));
    rig.writes.release();
    let file_id = rig.file_id.clone();
    tick_until(&mut rig, "the autosave in flight is applied", |rig| {
        rig.fixture.core.editor.save_failure(&file_id).is_some()
    });
    assert!(!rig.path().exists(), "the job in flight never recreates");

    // The request to save again was made before that job failed: it still holds for the next.
    rig.tick_at(100_000);
    let path = rig.path();
    tick_until(&mut rig, "the next save creates the file", |rig| {
        rig.fixture.core.editor.save_failure(&file_id).is_none() && path.exists()
    });
    assert_eq!(fs::read_to_string(rig.path()).unwrap(), "alpha beta\n");
}
