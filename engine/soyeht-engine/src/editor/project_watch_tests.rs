//! The project panel follows disk changes from file events, and an idle
//! workspace costs the engine no folder reads.
//!
//! The event handling is tested through the channel the watcher feeds
//! (`Watches::inject_events`): the test makes the change on disk and sends the
//! event the operating system would, so ordering, coalescing and quiet periods
//! do not depend on how fast a loaded machine delivers file events. One test
//! at the end waits for real events.

use super::{
    project_paths::ProjectRoot,
    project_watch::{Plan, Source, prepare_watch},
    test_fixture::EditorFixture,
};
use notify::{
    Event, EventKind,
    event::{CreateKind, DataChange, Flag, ModifyKind, RemoveKind, RenameMode},
};
use serde_json::{Value, json};
use std::{
    fs,
    sync::mpsc::{self, Sender},
    time::{Duration, Instant},
};

/// Simulated time per engine turn: past the quiet period, so a burst that has
/// arrived is read on the next turn.
const TURN_MS: i64 = 100;

fn changed(workspace_id: &str, paths: &[&str]) -> Vec<Value> {
    vec![json!({"jsonrpc":"2.0","method":"project.tree_changed",
        "params":{"workspaceId":workspace_id,"paths":paths}})]
}

const CREATED: EventKind = EventKind::Create(CreateKind::File);
const REMOVED: EventKind = EventKind::Remove(RemoveKind::File);
const RENAMED: EventKind = EventKind::Modify(ModifyKind::Name(RenameMode::Any));
const WRITTEN: EventKind = EventKind::Modify(ModifyKind::Data(DataChange::Content));

fn watch(fixture: &mut EditorFixture, paths: &[&str]) {
    let workspace_id = fixture.workspace_id.clone();
    fixture.call(
        "project.tree.watch",
        json!({"workspaceId":workspace_id,"paths":paths}),
    );
}

/// A fixture without file events: its workspace is polled.
async fn polling(paths: &[&str]) -> EditorFixture {
    let mut fixture = EditorFixture::new(&[("src/lib.rs", "lib\n")]).await;
    fixture.core.project_watches.force_polling = true;
    watch(&mut fixture, paths);
    fixture
}

/// A fixture whose workspace takes its file events from the test.
pub(super) struct Followed {
    pub(super) fixture: EditorFixture,
    pub(super) workspace_id: String,
    pub(super) events: Sender<notify::Result<Event>>,
}

impl Followed {
    pub(super) fn attach(mut fixture: EditorFixture) -> Self {
        let workspace_id = fixture.workspace_id.clone();
        let root = fixture.core.project_root(&workspace_id).unwrap();
        let events = fixture
            .core
            .project_watches
            .inject_events(&workspace_id, root.root());
        Self {
            fixture,
            workspace_id,
            events,
        }
    }

    /// A fixture with the default files, watching `paths`.
    async fn watching(paths: &[&str]) -> Self {
        let fixture = EditorFixture::new(&[("src/lib.rs", "lib\n"), ("README.md", "r\n")]).await;
        let mut followed = Self::attach(fixture);
        followed.watch(paths).await;
        followed
    }

    pub(super) async fn watch(&mut self, paths: &[&str]) {
        watch(&mut self.fixture, paths);
        self.fixture.notifications().await;
    }

    /// Sends one event for `relative` paths of the project, as the operating
    /// system reports them (under the canonical root).
    pub(super) fn emit(&self, kind: EventKind, relative: &[&str]) {
        let root = self
            .fixture
            .core
            .project_root(&self.workspace_id)
            .unwrap()
            .root()
            .to_owned();
        let mut event = Event::new(kind);
        for path in relative {
            event = event.add_path(root.join(path));
        }
        self.events.send(Ok(event)).unwrap();
    }

    /// One engine turn `advance_ms` of simulated time after the last one.
    fn step(&mut self, advance_ms: i64) {
        let now = self.fixture.core.now_ms() + advance_ms;
        self.fixture.at(now);
        self.fixture.core.advance_project_tree().unwrap();
    }

    /// Runs the turns that read what has been sent and returns what the
    /// connection was told: the first turn takes the events, the second comes
    /// after the quiet period.
    pub(super) async fn changes(&mut self) -> Vec<Value> {
        self.step(0);
        self.step(TURN_MS);
        self.fixture.notifications().await
    }
}

#[tokio::test]
async fn create_rename_and_delete_on_disk_are_reported_from_events() {
    let mut followed = Followed::watching(&["", "src"]).await;
    let workspace_id = followed.workspace_id.clone();
    assert!(
        followed
            .fixture
            .core
            .project_watches
            .uses_events(&workspace_id)
    );
    let src = followed.fixture.project().join("src");

    fs::write(src.join("new.rs"), "").unwrap();
    followed.emit(CREATED, &["src/new.rs"]);
    assert_eq!(followed.changes().await, changed(&workspace_id, &["src"]));

    // A rename reports both the old and the new path, as FSEvents does.
    fs::rename(src.join("new.rs"), src.join("renamed.rs")).unwrap();
    followed.emit(RENAMED, &["src/new.rs"]);
    followed.emit(RENAMED, &["src/renamed.rs"]);
    assert_eq!(followed.changes().await, changed(&workspace_id, &["src"]));

    fs::remove_file(src.join("renamed.rs")).unwrap();
    followed.emit(REMOVED, &["src/renamed.rs"]);
    assert_eq!(followed.changes().await, changed(&workspace_id, &["src"]));

    fs::create_dir(followed.fixture.project().join("docs")).unwrap();
    followed.emit(EventKind::Create(CreateKind::Folder), &["docs"]);
    assert_eq!(followed.changes().await, changed(&workspace_id, &[""]));
}

#[tokio::test]
async fn writing_a_file_in_place_leaves_every_listing_alone() {
    let mut followed = Followed::watching(&["", "src"]).await;
    let reads = followed.fixture.core.project_watches.reads();
    fs::write(followed.fixture.project().join("README.md"), "changed\n").unwrap();
    followed.emit(WRITTEN, &["README.md"]);
    assert!(followed.changes().await.is_empty());
    // The event cost one read of the folder that holds the file.
    assert_eq!(followed.fixture.core.project_watches.reads(), reads + 1);
}

#[tokio::test]
async fn a_burst_of_changes_becomes_one_notification() {
    let mut followed = Followed::watching(&["src"]).await;
    let workspace_id = followed.workspace_id.clone();
    for index in 0..50 {
        let name = format!("src/f{index}.rs");
        fs::write(followed.fixture.project().join(&name), "").unwrap();
        followed.emit(CREATED, &[&name]);
        // Each event comes inside the quiet period of the one before.
        followed.step(1);
    }
    assert!(followed.fixture.notifications().await.is_empty());
    assert_eq!(followed.changes().await, changed(&workspace_id, &["src"]));
    assert!(followed.changes().await.is_empty());
}

#[tokio::test]
async fn a_steady_stream_is_read_before_it_stops() {
    let mut followed = Followed::watching(&["src"]).await;
    let workspace_id = followed.workspace_id.clone();
    let mut told = Vec::new();
    // One event every 50 ms never leaves a quiet period of 75 ms.
    for index in 0..30 {
        let name = format!("src/s{index}.rs");
        fs::write(followed.fixture.project().join(&name), "").unwrap();
        followed.emit(CREATED, &[&name]);
        followed.step(50);
        told.extend(followed.fixture.notifications().await);
    }
    assert!(!told.is_empty());
    let expected = changed(&workspace_id, &["src"]).remove(0);
    assert!(told.iter().all(|sent| *sent == expected));
}

#[tokio::test]
async fn a_file_created_and_deleted_inside_one_burst_is_not_reported() {
    let mut followed = Followed::watching(&["src"]).await;
    fs::write(followed.fixture.project().join("src/tmp.rs"), "").unwrap();
    followed.emit(CREATED, &["src/tmp.rs"]);
    fs::remove_file(followed.fixture.project().join("src/tmp.rs")).unwrap();
    followed.emit(REMOVED, &["src/tmp.rs"]);
    // The listing is what it was, so nothing is sent.
    assert!(followed.changes().await.is_empty());
}

#[tokio::test]
async fn a_rescan_request_or_a_watcher_error_reads_every_watched_folder() {
    let mut followed = Followed::watching(&["", "src"]).await;
    let workspace_id = followed.workspace_id.clone();
    fs::write(followed.fixture.project().join("src/a.rs"), "").unwrap();
    fs::write(followed.fixture.project().join("b.rs"), "").unwrap();
    let rescan = Event::new(EventKind::Other).set_flag(Flag::Rescan);
    followed.events.send(Ok(rescan)).unwrap();
    assert_eq!(
        followed.changes().await,
        changed(&workspace_id, &["", "src"])
    );

    fs::write(followed.fixture.project().join("src/c.rs"), "").unwrap();
    fs::write(followed.fixture.project().join("d.rs"), "").unwrap();
    followed
        .events
        .send(Err(notify::Error::generic("kernel dropped events")))
        .unwrap();
    assert_eq!(
        followed.changes().await,
        changed(&workspace_id, &["", "src"])
    );
}

#[tokio::test]
async fn events_outside_the_workspace_read_nothing() {
    let mut followed = Followed::watching(&["", "src"]).await;
    let reads = followed.fixture.core.project_watches.reads();
    let elsewhere = Event::new(CREATED).add_path(followed.fixture.root.join("elsewhere.rs"));
    followed.events.send(Ok(elsewhere)).unwrap();
    assert!(followed.changes().await.is_empty());
    assert_eq!(followed.fixture.core.project_watches.reads(), reads);
}

#[tokio::test]
async fn a_lost_event_stream_falls_back_to_polling_and_catches_up() {
    let mut followed = Followed::watching(&["src"]).await;
    let workspace_id = followed.workspace_id.clone();
    fs::write(followed.fixture.project().join("src/missed.rs"), "").unwrap();
    // The watcher died: the sender is gone, so the receiver is disconnected.
    followed.events = mpsc::channel().0;
    assert_eq!(followed.changes().await, changed(&workspace_id, &["src"]));
    assert!(
        !followed
            .fixture
            .core
            .project_watches
            .uses_events(&workspace_id)
    );
}

#[tokio::test]
async fn removing_or_renaming_a_folder_reports_the_watched_folders_inside_it() {
    let fixture = EditorFixture::new(&[("a/deep/x.rs", "x\n"), ("keep/y.rs", "y\n")]).await;
    let mut followed = Followed::attach(fixture);
    let workspace_id = followed.workspace_id.clone();
    followed.watch(&["", "a", "a/deep", "keep"]).await;
    let project = followed.fixture.project();

    // The operating system reports the renamed folder, not what is inside it.
    fs::rename(project.join("a"), project.join("b")).unwrap();
    followed.emit(RENAMED, &["a"]);
    followed.emit(RENAMED, &["b"]);
    assert_eq!(
        followed.changes().await,
        changed(&workspace_id, &["", "a", "a/deep"])
    );
}

#[tokio::test]
async fn an_idle_workspace_costs_no_folder_reads() {
    let mut followed = Followed::watching(&["", "src"]).await;
    let reads = followed.fixture.core.project_watches.reads();
    for _ in 0..20 {
        followed.step(1_000);
    }
    assert_eq!(followed.fixture.core.project_watches.reads(), reads);
    assert!(followed.fixture.notifications().await.is_empty());
}

#[tokio::test]
async fn the_event_source_starts_before_the_first_listing_is_read() {
    let fixture = EditorFixture::new(&[("src/lib.rs", "lib\n")]).await;
    let root = fixture.project();
    let project = ProjectRoot::open(&root).unwrap();
    let before = project.fingerprint("src");
    // A source that changes the folder while it starts: a listing read before the start
    // would miss the change, and no event would report it.
    let plan = Plan {
        starter: Some(|root| {
            fs::write(root.join("src/during_start.rs"), "").unwrap();
            Source::Polling
        }),
        epoch: 0,
    };
    let prepared = prepare_watch(&root, &["src".to_owned()], plan).unwrap();
    assert_ne!(prepared.fingerprints[0].1, before);
    assert_eq!(prepared.fingerprints[0].1, project.fingerprint("src"));
}

#[tokio::test]
async fn a_change_while_a_watch_lists_its_folders_is_not_lost_when_a_tick_consumed_it() {
    let mut followed = Followed::watching(&[""]).await;
    let workspace_id = followed.workspace_id.clone();
    let plan = followed.fixture.core.project_watches.plan(&workspace_id);
    assert!(plan.starter.is_none(), "the workspace has a live source");
    // The request lists its folder off the lock ...
    let root = followed.fixture.project();
    let prepared = prepare_watch(&root, &["src".to_owned()], plan).unwrap();
    // ... a file appears, the live source reports it and a tick consumes the burst before the
    // request is applied: the event reaches nobody and the listing is stale.
    fs::write(root.join("src/new.rs"), "").unwrap();
    followed.emit(CREATED, &["src/new.rs"]);
    followed.step(0);
    followed.step(TURN_MS);
    followed.fixture.notifications().await;
    let key = (followed.fixture.connection.clone(), workspace_id.clone());
    followed
        .fixture
        .core
        .apply_project_watch(key, prepared)
        .unwrap();
    let mut told = Vec::new();
    for _ in 0..5 {
        followed.step(TURN_MS);
        told.extend(followed.fixture.notifications().await);
    }
    assert_eq!(told, changed(&workspace_id, &["src"]));
}

#[tokio::test]
async fn a_workspace_closed_while_a_watch_lists_its_folders_is_not_found_and_keeps_nothing() {
    let mut fixture = EditorFixture::new(&[("src/lib.rs", "lib\n")]).await;
    let workspace_id = fixture.workspace_id.clone();
    let plan = fixture.core.project_watches.plan(&workspace_id);
    let prepared = prepare_watch(&fixture.project(), &["src".to_owned()], plan).unwrap();
    fixture.call(
        "workspace.close",
        json!({"workspaceId":workspace_id,"idempotencyKey":"close"}),
    );
    let key = (fixture.connection.clone(), workspace_id.clone());
    assert!(
        fixture
            .core
            .apply_project_watch(key.clone(), prepared)
            .is_err()
    );
    assert!(!fixture.core.project_watches.folders.contains_key(&key));
    assert!(!fixture.core.project_watches.uses_events(&workspace_id));
}

#[tokio::test]
async fn without_events_the_watched_folders_are_polled_once_a_second() {
    let mut fixture = polling(&["src"]).await;
    let workspace_id = fixture.workspace_id.clone();
    fixture.notifications().await;
    assert!(!fixture.core.project_watches.uses_events(&workspace_id));
    let start = fixture.core.now_ms();
    fixture.core.advance_project_tree().unwrap();
    let reads = fixture.core.project_watches.reads();
    fixture.at(start + 500);
    fixture.core.advance_project_tree().unwrap();
    assert_eq!(fixture.core.project_watches.reads(), reads);

    fs::write(fixture.project().join("src/new.rs"), "").unwrap();
    fixture.at(start + 1_000);
    fixture.core.advance_project_tree().unwrap();
    assert_eq!(
        fixture.notifications().await,
        changed(&workspace_id, &["src"])
    );
}

/// The one test that waits for the operating system: files created after
/// `project.tree.watch` returns are reported. Under load the system delivers
/// file events late, in batches, and sometimes only after a later write, so
/// the test keeps creating a file every few seconds and waits for the report
/// itself, with a deadline far past any load instead of a fixed time.
#[tokio::test]
async fn real_file_events_reach_the_engine() {
    let mut fixture = EditorFixture::new(&[("src/lib.rs", "lib\n")]).await;
    let workspace_id = fixture.workspace_id.clone();
    watch(&mut fixture, &["src"]);
    assert!(fixture.core.project_watches.uses_events(&workspace_id));

    let deadline = Instant::now() + Duration::from_secs(300);
    let mut created = 0;
    let mut next_file = Instant::now();
    let mut told = Vec::new();
    while told.is_empty() && Instant::now() < deadline {
        if Instant::now() >= next_file {
            fs::write(fixture.project().join(format!("src/new{created}.rs")), "").unwrap();
            created += 1;
            next_file += Duration::from_secs(3);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        let now = fixture.core.now_ms() + TURN_MS;
        fixture.at(now);
        fixture.core.advance_project_tree().unwrap();
        told = fixture.notifications().await;
    }
    assert_eq!(told, changed(&workspace_id, &["src"]));
}
