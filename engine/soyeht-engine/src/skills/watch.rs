//! Follows the user's skill folder and the workspace skill folders clients
//! list, from operating-system file events (the `notify` crate, as the
//! project panel does). A burst is read once it settles, and a folder is
//! announced only when its listing changed. A folder that does not exist yet,
//! or that no event stream could follow, is looked at every two seconds.
//!
//! Starting an event stream asks the system's file-event service, which can take
//! seconds on a busy machine. It happens on its own thread, so neither engine
//! start nor a request waits for it: the folder is looked at every two seconds
//! until the stream is live, and read once more when it goes live.

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use soyeht_protocol::Skill;
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

/// A burst is read once no event arrived for this long ...
const QUIET_MS: i64 = 75;
/// ... or once it has lasted this long.
const BURST_LIMIT_MS: i64 = 500;
const PROBE_INTERVAL_MS: i64 = 2_000;

/// `None` is the user's folder, `Some(id)` a workspace's.
type Key = Option<String>;

struct Events {
    receiver: Receiver<notify::Result<Event>>,
    // Dropping the watcher stops the stream. Tests feed `receiver` directly.
    _watcher: Option<RecommendedWatcher>,
}

struct Folder {
    root: PathBuf,
    events: Option<Events>,
    /// The thread that is starting the event stream; it answers once.
    starting: Option<Receiver<Option<Events>>>,
    fingerprint: u64,
    /// The listing the fingerprint comes from was read before the event
    /// stream started, so one more read is due: a change in between is in no
    /// event and in no fingerprint.
    catch_up: bool,
    polling_only: bool,
}

#[derive(Default)]
pub struct Watches {
    user: Option<Folder>,
    /// Workspace folders with the connections that listed them.
    workspaces: BTreeMap<String, (Folder, BTreeSet<String>)>,
    dirty: BTreeSet<Key>,
    burst_started_ms: Option<i64>,
    last_event_ms: i64,
    probe_due_ms: i64,
    /// Never start event streams: every folder is looked at on the probe.
    polling_only: bool,
}

impl Watches {
    /// Replaces the event stream of a followed folder with a channel the
    /// test feeds, so event handling runs without the operating system.
    #[cfg(test)]
    pub(super) fn inject_events(
        &mut self,
        workspace_id: Option<&str>,
    ) -> mpsc::Sender<notify::Result<Event>> {
        let (sender, receiver) = mpsc::channel();
        if let Some(folder) = self.folder_mut(workspace_id) {
            folder.events = Some(Events {
                receiver,
                _watcher: None,
            });
        }
        sender
    }

    /// Follows folders by looking at them every probe instead of by events.
    pub(super) fn poll_only(&mut self) {
        self.polling_only = true;
    }

    pub(super) fn follow_user(&mut self, root: &Path, listed: &[Skill]) {
        self.user = Some(Folder::new(root, listed, self.polling_only));
    }

    pub(super) fn show(
        &mut self,
        connection: &str,
        workspace_id: &str,
        root: &Path,
        listed: &[Skill],
    ) {
        let polling_only = self.polling_only;
        let (folder, viewers) = self
            .workspaces
            .entry(workspace_id.into())
            .or_insert_with(|| (Folder::new(root, listed, polling_only), BTreeSet::new()));
        if folder.root != root {
            *folder = Folder::new(root, listed, polling_only);
        }
        viewers.insert(connection.into());
    }

    /// Marks a folder the engine itself changed, so it is read with the next
    /// burst even before its file events arrive.
    pub(super) fn touch(&mut self, workspace_id: Option<&str>, now: i64) {
        self.dirty.insert(workspace_id.map(Into::into));
        self.burst_started_ms.get_or_insert(now);
        self.last_event_ms = now;
    }

    /// The folders to list again, once their burst of events settled.
    pub(super) fn settled(&mut self, now: i64, live: impl Fn(&str) -> bool) -> Vec<(Key, PathBuf)> {
        self.workspaces.retain(|_, (_, viewers)| {
            viewers.retain(|connection| live(connection));
            !viewers.is_empty()
        });
        let probe = now >= self.probe_due_ms;
        if probe {
            self.probe_due_ms = now.saturating_add(PROBE_INTERVAL_MS);
        }
        let mut arrived = false;
        let user = self.user.iter_mut().map(|folder| (None, folder));
        let workspaces = self
            .workspaces
            .iter_mut()
            .map(|(id, (folder, _))| (Some(id.clone()), folder));
        for (key, folder) in user.chain(workspaces) {
            if folder.drain() | (probe && folder.reattach()) | folder.take_catch_up() {
                self.dirty.insert(key);
                arrived = true;
            }
        }
        if arrived {
            self.burst_started_ms.get_or_insert(now);
            self.last_event_ms = now;
        }
        let settled = self.burst_started_ms.is_some_and(|started| {
            now - self.last_event_ms >= QUIET_MS || now - started >= BURST_LIMIT_MS
        });
        if !settled {
            return Vec::new();
        }
        self.burst_started_ms = None;
        std::mem::take(&mut self.dirty)
            .into_iter()
            .filter_map(|key| {
                let folder = match &key {
                    None => self.user.as_ref(),
                    Some(id) => self.workspaces.get(id).map(|(folder, _)| folder),
                }?;
                let root = folder.root.clone();
                Some((key, root))
            })
            .collect()
    }

    fn folder_mut(&mut self, workspace_id: Option<&str>) -> Option<&mut Folder> {
        match workspace_id {
            None => self.user.as_mut(),
            Some(id) => self.workspaces.get_mut(id).map(|(folder, _)| folder),
        }
    }

    /// Keeps a folder's new listing; true when it differs from the last one.
    pub(super) fn record(&mut self, workspace_id: Option<&str>, listed: &[Skill]) -> bool {
        let Some(folder) = self.folder_mut(workspace_id) else {
            return false;
        };
        let current = fingerprint(listed);
        let changed = current != folder.fingerprint;
        folder.fingerprint = current;
        changed
    }
}

impl Folder {
    fn new(root: &Path, listed: &[Skill], polling_only: bool) -> Self {
        let mut folder = Self {
            root: root.into(),
            events: None,
            starting: None,
            fingerprint: fingerprint(listed),
            catch_up: true,
            polling_only,
        };
        folder.reattach();
        folder
    }

    /// True once after the folder was first listed.
    fn take_catch_up(&mut self) -> bool {
        std::mem::take(&mut self.catch_up)
    }

    /// Follows the folder while it exists. True when it appeared, vanished,
    /// or cannot be followed by events and must be read again.
    fn reattach(&mut self) -> bool {
        self.reattach_with(event_source)
    }

    fn reattach_with(
        &mut self,
        start: impl FnOnce(&Path) -> Option<Events> + Send + 'static,
    ) -> bool {
        match (self.root.is_dir(), self.events.is_some()) {
            (true, false) => {
                if !self.polling_only && self.starting.is_none() {
                    self.starting = start_in_background(&self.root, start);
                }
                true
            }
            (false, true) => {
                self.events = None;
                true
            }
            _ => false,
        }
    }

    /// Takes the event stream once its thread has started it. True when the
    /// thread answered: a change since the first listing is in no event, so the
    /// folder is read again.
    fn take_started(&mut self) -> bool {
        let Some(starting) = &self.starting else {
            return false;
        };
        match starting.try_recv() {
            Ok(events) => {
                self.starting = None;
                self.events = events;
                self.catch_up = true;
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.starting = None;
                false
            }
        }
    }

    /// Empties the event queue; true when anything arrived.
    fn drain(&mut self) -> bool {
        let started = self.take_started();
        let Some(events) = &self.events else {
            return started;
        };
        let mut arrived = started;
        loop {
            match events.receiver.try_recv() {
                Ok(_) => arrived = true,
                Err(TryRecvError::Empty) => return arrived,
                Err(TryRecvError::Disconnected) => {
                    self.events = None;
                    return true;
                }
            }
        }
    }
}

fn start_in_background(
    root: &Path,
    start: impl FnOnce(&Path) -> Option<Events> + Send + 'static,
) -> Option<Receiver<Option<Events>>> {
    let (sender, receiver) = mpsc::channel();
    let root = root.to_owned();
    thread::Builder::new()
        .name("skill-watch-start".into())
        .spawn(move || {
            // The receiver is gone when the folder was dropped meanwhile; nobody wants the
            // stream then.
            sender.send(start(&root)).unwrap_or_default();
        })
        .ok()?;
    Some(receiver)
}

fn event_source(root: &Path) -> Option<Events> {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(sender).ok()?;
    watcher.watch(root, RecursiveMode::Recursive).ok()?;
    Some(Events {
        receiver,
        _watcher: Some(watcher),
    })
}

fn fingerprint(listed: &[Skill]) -> u64 {
    let mut hasher = DefaultHasher::new();
    serde_json::to_string(listed)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::SkillScope;

    fn skill(name: &str) -> Skill {
        Skill {
            name: name.into(),
            scope: SkillScope::User,
            workspace_id: None,
            path: format!("/skills/{name}"),
            description: Some("d".into()),
            allowed_tools: None,
            revision: None,
            problem: None,
        }
    }

    const WORKSPACE: &str = "workspace";

    fn temp_folder() -> PathBuf {
        std::env::temp_dir().join(format!("soyeht-skill-watch-{}", uuid::Uuid::new_v4()))
    }

    fn workspace_key() -> Option<String> {
        Some(WORKSPACE.into())
    }

    /// Folders to list again: the turn that takes events, then the turn after
    /// the quiet period.
    fn settle(watches: &mut Watches, now: i64) -> Vec<(Key, PathBuf)> {
        assert!(
            watches.settled(now, |_| true).is_empty(),
            "a burst settles only after its quiet period"
        );
        watches.settled(now + QUIET_MS, |_| true)
    }

    /// A watched folder whose first read is already done.
    fn followed(root: &Path, polling_only: bool) -> Watches {
        let mut watches = Watches::default();
        if polling_only {
            watches.poll_only();
        }
        watches.show("connection", WORKSPACE, root, &[]);
        assert_eq!(settle(&mut watches, 0).len(), 1, "the catch-up read");
        watches
    }

    /// An existing watched folder fed by the returned channel.
    fn injected() -> (PathBuf, Watches, mpsc::Sender<notify::Result<Event>>) {
        let root = temp_folder();
        std::fs::create_dir_all(&root).unwrap();
        let mut watches = followed(&root, false);
        let events = watches.inject_events(Some(WORKSPACE));
        (root, watches, events)
    }

    #[test]
    fn a_missing_folder_is_probed_and_listings_are_announced_only_when_changed() {
        let root = temp_folder();
        let mut watches = followed(&root, true);
        assert!(!watches.record(workspace_key().as_deref(), &[]));
        std::fs::create_dir_all(&root).unwrap();
        assert!(settle(&mut watches, PROBE_INTERVAL_MS - 200).is_empty());
        let settled = settle(&mut watches, PROBE_INTERVAL_MS);
        assert_eq!(settled, [(workspace_key(), root.clone())]);
        assert!(!watches.record(Some(WORKSPACE), &[]));
        assert!(watches.record(Some(WORKSPACE), &[skill("new")]));
        assert!(!watches.record(Some(WORKSPACE), &[skill("new")]));
        watches.settled(PROBE_INTERVAL_MS, |_| false);
        assert!(!watches.record(Some(WORKSPACE), &[]));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_change_between_the_first_listing_and_the_event_stream_is_still_read() {
        let root = temp_folder();
        std::fs::create_dir_all(&root).unwrap();
        let mut watches = Watches::default();
        // The caller listed the folder (empty), then the stream started.
        watches.show("connection", WORKSPACE, &root, &[]);
        let folders = watches.settled(0, |_| true);
        assert!(folders.is_empty());
        // No event ever arrives for the skill written in between, yet the
        // catch-up read finds it.
        let folders = watches.settled(QUIET_MS, |_| true);
        assert_eq!(folders, [(workspace_key(), root.clone())]);
        assert!(watches.record(Some(WORKSPACE), &[skill("early")]));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_burst_of_events_is_read_once_after_it_settles() {
        let (root, mut watches, events) = injected();
        for _ in 0..50 {
            events.send(Ok(Event::default())).unwrap();
        }
        assert_eq!(
            settle(&mut watches, 1_000),
            [(workspace_key(), root.clone())]
        );
        assert!(settle(&mut watches, 2_000).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_steady_stream_is_read_before_it_stops_and_errors_count_as_changes() {
        let (root, mut watches, events) = injected();
        let mut read_at = None;
        // One event every 50 ms never leaves a quiet period of 75 ms.
        for step in 0..40 {
            let now = 1_000 + step * 50;
            events.send(Err(notify::Error::generic("dropped"))).unwrap();
            if !watches.settled(now, |_| true).is_empty() {
                read_at = Some(now);
                break;
            }
        }
        let read_at = read_at.expect("the stream is read while it runs");
        assert!(read_at - 1_000 >= BURST_LIMIT_MS && read_at - 1_000 < BURST_LIMIT_MS + 100);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_lost_event_stream_is_read_again_and_followed_again_on_the_probe() {
        let (root, mut watches, events) = injected();
        // The watcher died: the sender is gone, the receiver disconnected.
        drop(events);
        assert_eq!(
            settle(&mut watches, 1_000),
            [(workspace_key(), root.clone())]
        );
        assert!(watches.workspaces[WORKSPACE].0.events.is_none());
        assert_eq!(
            settle(&mut watches, 1_000 + PROBE_INTERVAL_MS),
            [(workspace_key(), root.clone())]
        );
        // The probe asks for a new stream on its own thread. Whether the system's file-event
        // service answers is not this rule (a busy machine may take minutes or refuse), and a
        // stream that arrives is taken by `a_slow_event_stream_start_..._caught_up`.
        let folder = &watches.workspaces[WORKSPACE].0;
        assert!(
            folder.starting.is_some() || folder.events.is_some(),
            "the stream was asked for again"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The system's file-event service can take seconds to start a stream. The
    /// folder returns at once, is read by polling until the stream is live, and
    /// is read once more when it goes live.
    #[test]
    fn a_slow_event_stream_start_does_not_block_and_is_caught_up_when_it_arrives() {
        let root = temp_folder();
        std::fs::create_dir_all(&root).unwrap();
        let mut folder = Folder::new(&root, &[], true);
        folder.polling_only = false;
        assert!(folder.take_catch_up(), "the first listing is read");
        let (release, gate) = mpsc::channel::<()>();
        let (_sender, receiver) = mpsc::channel();
        assert!(folder.reattach_with(move |_| {
            gate.recv().ok();
            Some(Events {
                receiver,
                _watcher: None,
            })
        }));
        assert!(folder.events.is_none() && folder.starting.is_some());
        assert!(!folder.drain() && !folder.take_catch_up(), "still starting");
        assert!(
            folder.reattach_with(|_| None),
            "a second probe starts nothing"
        );
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
        while !folder.drain() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(folder.events.is_some() && folder.starting.is_none());
        assert!(folder.take_catch_up(), "a change while starting is read");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The one test that waits for the operating system: a skill written
    /// after the folder is followed is announced. The system delivers events
    /// late under load, so it keeps writing and waits for the condition.
    #[test]
    fn real_file_events_reach_the_watcher() {
        let root = temp_folder();
        std::fs::create_dir_all(&root).unwrap();
        let mut watches = followed(&root, false);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
        let started = std::time::Instant::now();
        let mut written = 0;
        let mut found = false;
        while !found && std::time::Instant::now() < deadline {
            if started.elapsed().as_secs() / 3 >= written {
                std::fs::create_dir_all(root.join(format!("skill{written}"))).unwrap();
                written += 1;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
            let now = 1_000 + started.elapsed().as_millis() as i64;
            found = !watches.settled(now, |_| true).is_empty();
        }
        assert!(found, "no file event reached the watcher");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
