//! Following the folders the project panel shows, from operating-system file
//! events.
//!
//! Each watched workspace gets one recursive watcher (FSEvents on macOS,
//! inotify on Linux, through the `notify` crate). Its callback only queues the
//! event; the engine loop drains the queue, maps each path to the folder that
//! lists it, and waits until the burst settles before it reads those folders.
//! A folder is announced only when its listing fingerprint changed, so an
//! event that left the listing alone (a file written in place) costs one
//! `read_dir` and sends nothing. Where no watcher can start, that workspace
//! falls back to reading its watched folders once a second. With events and no
//! changes on disk, the engine touches no folder at all.

use super::{
    project_files::is_save_temporary, project_paths::ProjectRoot, project_tree::project_error,
};
use crate::{
    core::Core,
    errors::{self, Result},
    file_events::{Burst, EventStream},
    model,
};
use soyeht_protocol::{ProjectTreeChangedEvent, event};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::mpsc::TryRecvError,
    thread,
};

/// The fallback reads every watched folder at this period.
const POLL_INTERVAL_MS: i64 = 1_000;

type Folders = BTreeMap<String, Option<u64>>;
type Dirty = BTreeMap<String, Option<Touched>>;

/// Folders each connection shows, with the fingerprint last seen, and the
/// event source of each watched workspace.
#[derive(Default)]
pub struct Watches {
    pub(super) folders: BTreeMap<(String, String), Folders>,
    sources: BTreeMap<String, Source>,
    /// Workspace → what events touched; `None` means every folder.
    dirty: Dirty,
    burst: Burst,
    poll_due_ms: i64,
    /// Folder listings read since the engine started; tests assert idleness.
    reads: u64,
    /// Counts the turns that took events out of `dirty` or dropped a source. A request that
    /// listed its folders off the lock while one happened may have missed an event.
    epoch: u64,
    #[cfg(test)]
    pub(super) force_polling: bool,
}

pub(super) enum Source {
    Events { root: PathBuf, stream: EventStream },
    Polling,
}

impl Watches {
    /// What a request needs before it leaves the lock: how to start the event source of
    /// `workspace_id` (`None` when it has one) and the epoch to hand back to
    /// [`Watches::adopt`].
    pub(super) fn plan(&self, workspace_id: &str) -> Plan {
        let starter = if self.sources.contains_key(workspace_id) {
            None
        } else {
            #[cfg(test)]
            let starter: Starter = if self.force_polling {
                |_| Source::Polling
            } else {
                start_source
            };
            #[cfg(not(test))]
            let starter: Starter = start_source;
            Some(starter)
        };
        Plan {
            starter,
            epoch: self.epoch,
        }
    }

    /// Keeps the source a request started, once it is back under the lock, and returns the
    /// one that is not needed so the caller drops it after the lock is released. The folders
    /// the request listed may be older than the source that stays, or than a turn that
    /// consumed events meanwhile: a change in between is in no event and in no fingerprint,
    /// so then every folder is read once more.
    fn adopt(
        &mut self,
        workspace_id: &str,
        started: Option<Source>,
        epoch: u64,
        now: i64,
    ) -> Option<Source> {
        let (reread, surplus) = match (self.sources.contains_key(workspace_id), started) {
            (false, Some(source)) => {
                self.insert_source(workspace_id, source);
                (false, None)
            }
            (false, None) => {
                self.insert_source(workspace_id, Source::Polling);
                (true, None)
            }
            (true, Some(source)) => (true, Some(source)),
            (true, None) => (self.epoch != epoch, None),
        };
        if reread {
            self.reread(workspace_id, now);
        }
        surplus
    }

    fn reread(&mut self, workspace_id: &str, now: i64) {
        self.dirty.insert(workspace_id.to_owned(), None);
        self.burst.note(now);
    }

    fn insert_source(&mut self, workspace_id: &str, source: Source) {
        self.sources.insert(workspace_id.to_owned(), source);
    }

    /// Replaces the file-event stream of `workspace_id` with a channel the
    /// test feeds, so event handling runs without the operating system.
    #[cfg(test)]
    pub(super) fn inject_events(
        &mut self,
        workspace_id: &str,
        root: &Path,
    ) -> std::sync::mpsc::Sender<notify::Result<notify::Event>> {
        let (sender, events) = std::sync::mpsc::channel();
        let source = Source::Events {
            root: root.to_owned(),
            stream: EventStream::fed(events),
        };
        self.insert_source(workspace_id, source);
        sender
    }

    pub(super) fn record_read(&mut self) {
        self.reads += 1;
    }

    #[cfg(test)]
    pub(super) fn reads(&self) -> u64 {
        self.reads
    }

    #[cfg(test)]
    pub(super) fn uses_events(&self, workspace_id: &str) -> bool {
        matches!(self.sources.get(workspace_id), Some(Source::Events { .. }))
    }

    /// Drops watches of closed connections and sources nobody watches.
    fn prune(&mut self, live: impl Fn(&str) -> bool) {
        self.folders.retain(|(connection, _), _| live(connection));
        let watched = self
            .folders
            .keys()
            .map(|(_, workspace)| workspace.clone())
            .collect::<BTreeSet<_>>();
        let sources = self.sources.len();
        self.sources
            .retain(|workspace, _| watched.contains(workspace));
        if self.sources.len() != sources {
            self.epoch += 1;
        }
        self.dirty
            .retain(|workspace, _| watched.contains(workspace));
    }

    /// Moves queued events into `dirty`; true when any arrived.
    fn drain_events(&mut self, now: i64) -> bool {
        let mut arrived = false;
        let mut failed = Vec::new();
        for (workspace, source) in &self.sources {
            let Source::Events { root, stream } = source else {
                continue;
            };
            loop {
                match stream.receiver.try_recv() {
                    Ok(Ok(event)) if !event.need_rescan() => {
                        arrived = true;
                        let entry = self.dirty.entry(workspace.clone());
                        let touched = entry.or_insert_with(|| Some(Touched::default()));
                        if let Some(touched) = touched {
                            for path in &event.paths {
                                touched.add(root, path);
                            }
                        }
                    }
                    Ok(_) => {
                        arrived = true;
                        self.dirty.insert(workspace.clone(), None);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        failed.push(workspace.clone());
                        break;
                    }
                }
            }
        }
        for workspace in failed {
            self.sources.insert(workspace.clone(), Source::Polling);
            self.dirty.insert(workspace, None);
            arrived = true;
        }
        if arrived {
            self.burst.note(now);
        }
        arrived
    }

    /// Marks every folder of polling workspaces dirty once a second.
    fn poll(&mut self, now: i64) -> bool {
        if now < self.poll_due_ms {
            return false;
        }
        let polling = self
            .sources
            .iter()
            .filter(|(_, source)| matches!(source, Source::Polling))
            .map(|(workspace, _)| workspace.clone())
            .collect::<Vec<_>>();
        if polling.is_empty() {
            return false;
        }
        self.poll_due_ms = now.saturating_add(POLL_INTERVAL_MS);
        for workspace in polling {
            self.dirty.insert(workspace, None);
        }
        true
    }

    /// The burst to read now, once events stopped or ran long enough.
    fn take_settled(&mut self, now: i64, polled: bool) -> Dirty {
        let settled = self.burst.settled(now);
        if !(settled || polled) || self.dirty.is_empty() {
            return BTreeMap::new();
        }
        self.burst.end();
        self.epoch += 1;
        std::mem::take(&mut self.dirty)
    }
}

impl Core {
    /// Reads the folders that changed since the last call and tells each
    /// connection that shows one of them. Without events it does nothing.
    pub(crate) fn advance_project_tree(&mut self) -> Result<()> {
        let now = self.now_ms();
        let clients = &self.clients;
        self.project_watches
            .prune(|connection| clients.contains_key(connection));
        self.project_watches.drain_events(now);
        let polled = self.project_watches.poll(now);
        let dirty = self.project_watches.take_settled(now, polled);
        if dirty.is_empty() {
            return Ok(());
        }
        let keys = self
            .project_watches
            .folders
            .keys()
            .filter(|(_, workspace)| dirty.contains_key(workspace))
            .cloned()
            .collect::<Vec<_>>();
        let mut fingerprints = BTreeMap::<(String, String), Option<u64>>::new();
        for (connection, workspace_id) in keys {
            let Ok(project) = self.project_root(&workspace_id) else {
                self.project_watches
                    .folders
                    .remove(&(connection, workspace_id));
                continue;
            };
            let touched = &dirty[&workspace_id];
            let key = (connection.clone(), workspace_id.clone());
            let paths = self.project_watches.folders[&key]
                .keys()
                .filter(|path| touched.as_ref().is_none_or(|touched| touched.reaches(path)))
                .cloned()
                .collect::<Vec<_>>();
            let mut changed = Vec::new();
            for path in paths {
                let current = match fingerprints.get(&(workspace_id.clone(), path.clone())) {
                    Some(current) => *current,
                    None => {
                        self.project_watches.record_read();
                        let current = project.fingerprint(&path);
                        fingerprints.insert((workspace_id.clone(), path.clone()), current);
                        current
                    }
                };
                let Some(seen) = self
                    .project_watches
                    .folders
                    .get_mut(&key)
                    .and_then(|folders| folders.get_mut(&path))
                else {
                    continue;
                };
                if current != *seen {
                    *seen = current;
                    changed.push(path);
                }
            }
            if changed.is_empty() {
                continue;
            }
            let event = serde_json::to_value(ProjectTreeChangedEvent {
                workspace_id,
                paths: changed,
            })
            .map_err(|_| errors::internal("Project change serialization failed"))?;
            if let Some(client) = self.clients.get(&connection) {
                client
                    .outbox
                    .send(model::notification(event::PROJECT_TREE_CHANGED, event)?);
            }
        }
        Ok(())
    }

    /// Records what [`prepare_watch`] made off the lock: the connection's folders and, when
    /// the workspace had no event source, the one just started. A workspace closed meanwhile
    /// is not found, and nothing is kept.
    pub(super) fn apply_project_watch(
        &mut self,
        key: (String, String),
        prepared: Prepared,
    ) -> Result<()> {
        if let Err(error) = model::workspace(&self.store, &key.1) {
            drop_off_lock(prepared.source);
            return Err(error);
        }
        let now = self.now_ms();
        let surplus = self
            .project_watches
            .adopt(&key.1, prepared.source, prepared.epoch, now);
        drop_off_lock(surplus);
        let mut folders = BTreeMap::new();
        for (path, fingerprint) in prepared.fingerprints {
            self.project_watches.record_read();
            folders.insert(path, fingerprint);
        }
        self.project_watches.folders.insert(key, folders);
        Ok(())
    }
}

/// Stops a watcher that is not kept on a thread of its own: dropping one can take
/// milliseconds, and this runs under the Core lock (ADR 0079).
fn drop_off_lock(source: Option<Source>) {
    if source.is_some() {
        thread::spawn(move || drop(source));
    }
}

/// Starts the event source of a workspace root; where no watcher can start, it is polled.
pub(super) type Starter = fn(&Path) -> Source;

fn start_source(root: &Path) -> Source {
    match EventStream::follow(root) {
        Some(stream) => Source::Events {
            root: root.to_owned(),
            stream,
        },
        None => Source::Polling,
    }
}

/// What a request takes off the lock, from [`Watches::plan`].
pub(super) struct Plan {
    pub(super) starter: Option<Starter>,
    pub(super) epoch: u64,
}

/// What a `project.tree.watch` request makes off the Core lock.
pub(super) struct Prepared {
    source: Option<Source>,
    pub(super) fingerprints: Vec<(String, Option<u64>)>,
    epoch: u64,
}

/// Checks the folders, starts the event source when the plan has a starter, then reads each
/// folder's first listing. The source goes live before the first listing is fingerprinted:
/// a change after that fingerprint is then always an event, never lost between the two.
pub(super) fn prepare_watch(root: &Path, paths: &[String], plan: Plan) -> Result<Prepared> {
    let project = ProjectRoot::open(root).map_err(errors::io)?;
    for path in paths {
        project.directory(path).map_err(project_error)?;
    }
    let source = plan.starter.map(|start| start(project.root()));
    let fingerprints = paths
        .iter()
        .map(|path| (path.clone(), project.fingerprint(path)))
        .collect();
    Ok(Prepared {
        source,
        fingerprints,
        epoch: plan.epoch,
    })
}

/// What file events touched in one workspace.
#[derive(Default)]
struct Touched {
    /// Folders whose listing a change can alter: the folder that holds the
    /// changed path, and the path itself when it is a watched folder that
    /// appeared, vanished or was replaced.
    folders: BTreeSet<String>,
    /// The changed paths. The system reports a folder that was renamed or
    /// removed, not what was inside it, so folders below these are touched too.
    paths: BTreeSet<String>,
}

impl Touched {
    fn add(&mut self, root: &Path, path: &Path) {
        let Ok(relative) = path.strip_prefix(root) else {
            return;
        };
        if relative
            .components()
            .any(|part| is_save_temporary(part.as_os_str()))
        {
            return;
        }
        let names = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>();
        if names.is_empty() {
            self.folders.insert(String::new());
            return;
        }
        self.folders.insert(names[..names.len() - 1].join("/"));
        let changed = names.join("/");
        self.folders.insert(changed.clone());
        self.paths.insert(changed);
    }

    fn reaches(&self, folder: &str) -> bool {
        self.folders.contains(folder)
            || self.paths.iter().any(|path| {
                folder
                    .strip_prefix(path.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{Source, Touched, Watches};
    use std::path::Path;

    fn reread(watches: &Watches) -> bool {
        watches.dirty.get("w").is_some_and(Option::is_none)
    }

    #[test]
    fn a_source_started_by_a_slower_request_is_dropped_and_the_workspace_read_again() {
        let mut watches = Watches::default();
        watches.adopt("w", Some(Source::Polling), 0, 0);
        assert!(watches.sources.contains_key("w"));
        assert!(!reread(&watches), "the first source needs no second read");
        // Two requests found no source and both started one: their listings may predate
        // the source that stays.
        let surplus = watches.adopt("w", Some(Source::Polling), 0, 5);
        assert!(surplus.is_some() && reread(&watches));
        assert!(watches.plan("w").starter.is_none());
    }

    #[test]
    fn a_source_dropped_while_a_request_listed_is_replaced_and_the_workspace_read_again() {
        let mut watches = Watches::default();
        watches.adopt("w", None, 0, 0);
        assert!(matches!(watches.sources.get("w"), Some(Source::Polling)));
        assert!(reread(&watches));
    }

    #[test]
    fn a_change_touches_its_parent_folder_itself_and_the_folders_below_it() {
        let mut touched = Touched::default();
        let root = Path::new("/w");
        touched.add(root, Path::new("/w/src/new.rs"));
        touched.add(root, Path::new("/w/top.rs"));
        touched.add(root, Path::new("/elsewhere/x"));
        touched.add(root, root);
        for reached in ["", "src", "src/new.rs", "top.rs", "src/new.rs/below"] {
            assert!(touched.reaches(reached), "{reached}");
        }
        // A sibling that merely shares a name prefix is not below the path.
        for untouched in ["other", "src/new.rsx", "tops.rs", "src/other"] {
            assert!(!touched.reaches(untouched), "{untouched}");
        }
    }
}
