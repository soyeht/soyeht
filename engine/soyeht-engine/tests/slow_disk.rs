//! A slow disk must not stall the engine (ADR 0079, ADR 0095). Terminal output is flushed to
//! the pane's segment file before the supervisor may drop it. On a busy machine one flush takes
//! seconds (a full flush of the drive on macOS), and the tick used to wait for it with the Core
//! lock held, so output of every pane and every request on every connection waited too.
//! The editor's autosave writes, flushes and renames a file the same way, and used to do it
//! with the lock held too. The test engine pauses before each flush and each save
//! (`SOYEHT_TEST_SLOW_DISK_MS`).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::{Value, json};
use std::{
    fs, thread,
    time::{Duration, Instant},
};
use support::{Client, Rig, Ticker, longest_gap};

/// How long each flush takes. The old tick waited this long for every chunk of output.
const FLUSH: &str = "1500";
/// How long the engine is watched.
const WINDOW: Duration = Duration::from_secs(5);
/// The longest stretch a watcher may get no terminal frame, or a request no reply. With the
/// flush in the tick both are as long as a flush; without it, a scheduling delay.
const STALL_CEILING: Duration = Duration::from_millis(700);
/// The autosave starts this long after an edit (500 ms) and then waits for the disk; a request
/// sent this long after the edit meets the save in flight.
const DURING_THE_SAVE: Duration = Duration::from_millis(900);

/// Watches the engine for `WINDOW` while `work` runs, and returns the longest stretch a watcher
/// of a terminal got no frame, the slowest reply to a request, and what `work` returned.
fn stalls<T>(rig: &Rig, work: impl FnOnce() -> T) -> (Duration, Duration, T) {
    let mut client = rig.client();
    let (_, pane) = client.create(rig);
    let ticker = Ticker::start(rig, &mut client, &pane);

    let started = Instant::now();
    let work = work();
    let mut slowest_request = Duration::ZERO;
    let mut prober = rig.client();
    while started.elapsed() < WINDOW {
        let asked = Instant::now();
        prober.call("engine.status", json!({}));
        slowest_request = slowest_request.max(asked.elapsed());
        thread::sleep(Duration::from_millis(100));
    }
    let ended = Instant::now();
    let arrivals = ticker.finish();
    let silence = longest_gap(&arrivals, started, ended);
    eprintln!(
        "{} terminal frames in {WINDOW:?}; longest silence {silence:?}; \
         slowest engine.status {slowest_request:?}",
        arrivals.len()
    );
    (silence, slowest_request, work)
}

fn assert_flowing(silence: Duration, slowest_request: Duration, what: &str) {
    assert!(
        silence < STALL_CEILING,
        "no terminal frame for {silence:?}: the engine waited for {what}"
    );
    assert!(
        slowest_request < STALL_CEILING,
        "a request waited {slowest_request:?}: the Core lock was held across {what}"
    );
}

/// A folder with `note.txt` holding "one\n", opened as a workspace by `editor`.
fn open_note(rig: &Rig, editor: &mut Client, name: &str) -> (std::path::PathBuf, Value) {
    let folder = rig.path.join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("note.txt"), "one\n").unwrap();
    let workspace = editor.add_workspace(&folder, name);
    (folder, editor.open_file(&workspace, "note.txt"))
}

/// Types `text` at the end of the first line; returns the reply, whose revision the next
/// edit needs.
fn type_after(editor: &mut Client, opened: &Value, column: u32, text: &str) -> Value {
    editor.call(
        "file.write",
        json!({
            "fileId":opened["file"]["fileId"],"baseRevision":opened["file"]["revision"],
            "author":{"actor":{"role":"human_local","client":"runtime-test"},"name":"you"},
            "edits":[{"range":{"start":{"line":0,"column":column},"end":{"line":0,"column":column}},"text":text}],
            "idempotencyKey":format!("slow-{column}")
        }),
    )
}

#[test]
fn a_slow_disk_does_not_stall_terminal_output_or_requests() {
    let rig = Rig::with_environment(&[("SOYEHT_TEST_SLOW_DISK_MS", FLUSH)]);
    let (silence, slowest_request, ()) = stalls(&rig, || {});
    assert_flowing(silence, slowest_request, "the disk");
}

/// The autosave of an edited file takes `FLUSH` to reach the disk. The tick used to wait for it
/// with the Core lock held (ADR 0097).
#[test]
fn a_slow_autosave_does_not_stall_terminal_output_or_requests() {
    let rig = Rig::with_environment(&[("SOYEHT_TEST_SLOW_DISK_MS", FLUSH)]);
    let mut editor = rig.client();
    let (folder, opened) = open_note(&rig, &mut editor, "autosave-folder");

    let (silence, slowest_request, ()) = stalls(&rig, || {
        type_after(&mut editor, &opened, 3, " two");
    });

    assert_eq!(
        fs::read_to_string(folder.join("note.txt")).unwrap(),
        "one two\n",
        "the autosave still reached the disk"
    );
    assert_flowing(silence, slowest_request, "the save");
}

/// A rename that meets a save in flight waits for it, but off the lock: the person who renamed
/// waits, nobody else does (ADR 0097). It used to sleep under the Core lock.
#[test]
fn a_rename_during_a_slow_save_waits_without_stalling_the_engine() {
    let rig = Rig::with_environment(&[("SOYEHT_TEST_SLOW_DISK_MS", FLUSH)]);
    let mut editor = rig.client();
    let (folder, opened) = open_note(&rig, &mut editor, "rename-folder");
    let workspace = opened["file"]["workspaceId"].clone();

    let (silence, slowest_request, renamed) = stalls(&rig, || {
        type_after(&mut editor, &opened, 3, " two");
        thread::sleep(DURING_THE_SAVE);
        let id = editor.send(
            "project.entry.rename",
            json!({"workspaceId":workspace,"from":"note.txt","to":"moved.txt","idempotencyKey":"rename"}),
        );
        (id, Instant::now())
    });
    let (id, sent) = renamed;
    let reply = editor.wait(id);

    assert!(reply.get("error").is_none(), "{reply}");
    assert!(
        sent.elapsed() < Duration::from_secs(30),
        "the rename was answered"
    );
    assert!(!folder.join("note.txt").exists(), "nothing was recreated");
    assert_eq!(
        fs::read_to_string(folder.join("moved.txt")).unwrap(),
        "one two\n",
        "the save landed before the file moved"
    );
    assert_flowing(silence, slowest_request, "a rename");
}

/// A tab closed while its save is in flight is finished by a second save, which also takes
/// `FLUSH`. That save used to run under the lock too.
#[test]
fn closing_a_tab_during_a_slow_save_finishes_it_without_stalling_the_engine() {
    let rig = Rig::with_environment(&[("SOYEHT_TEST_SLOW_DISK_MS", FLUSH)]);
    let mut editor = rig.client();
    let (folder, opened) = open_note(&rig, &mut editor, "close-folder");
    let pane = opened["file"]["paneId"].clone();

    let (silence, slowest_request, ()) = stalls(&rig, || {
        let first = type_after(&mut editor, &opened, 3, " two");
        thread::sleep(DURING_THE_SAVE);
        // Typed after the save began, then the tab is closed.
        type_after(&mut editor, &first, 7, " three");
        editor.call(
            "pane.close",
            json!({"paneId":pane,"idempotencyKey":"close-during-save"}),
        );
    });

    // Both saves are slow and the second starts when the first ends: give it time to land.
    let note = folder.join("note.txt");
    let deadline = Instant::now() + Duration::from_secs(10);
    while fs::read_to_string(&note).unwrap() != "one two three\n" {
        assert!(Instant::now() < deadline, "the closed file was never saved");
        thread::sleep(Duration::from_millis(100));
    }
    assert_flowing(silence, slowest_request, "the save of a closed tab");
}
