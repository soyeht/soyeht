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

use super::project_files::is_save_temporary;
use crate::{
    core::Core,
    errors::{self, Result},
    model,
};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use soyeht_protocol::{ProjectTreeChangedEvent, event};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

/// A burst is read once no event arrived for this long ...
const QUIET_MS: i64 = 75;
/// ... or once it has lasted this long, so a steady stream still shows up.
const BURST_LIMIT_MS: i64 = 500;
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
    burst_started_ms: Option<i64>,
    last_event_ms: i64,
    poll_due_ms: i64,
    /// Folder listings read since the engine started; tests assert idleness.
    reads: u64,
    #[cfg(test)]
    pub(super) force_polling: bool,
    /// Folder reads made before each source started; tests assert that the
    /// source is live before the first listing is fingerprinted.
    #[cfg(test)]
    pub(super) reads_at_source_start: BTreeMap<String, u64>,
}

enum Source {
    Events {
        root: PathBuf,
        events: Receiver<notify::Result<Event>>,
        // Dropping the watcher stops the stream. Tests feed `events` directly.
        _watcher: Option<RecommendedWatcher>,
    },
    Polling,
}

impl Watches {
    /// Starts the event source of `workspace_id` if it has none yet.
    pub(super) fn ensure_source(&mut self, workspace_id: &str, root: &Path) {
        if self.sources.contains_key(workspace_id) {
            return;
        }
        #[cfg(test)]
        if self.force_polling {
            self.insert_source(workspace_id, Source::Polling);
            return;
        }
        let source = event_source(root).unwrap_or(Source::Polling);
        self.insert_source(workspace_id, source);
    }

    fn insert_source(&mut self, workspace_id: &str, source: Source) {
        #[cfg(test)]
        self.reads_at_source_start
            .insert(workspace_id.to_owned(), self.reads);
        self.sources.insert(workspace_id.to_owned(), source);
    }

    /// Replaces the file-event stream of `workspace_id` with a channel the
    /// test feeds, so event handling runs without the operating system.
    #[cfg(test)]
    pub(super) fn inject_events(
        &mut self,
        workspace_id: &str,
        root: &Path,
    ) -> mpsc::Sender<notify::Result<Event>> {
        let (sender, events) = mpsc::channel();
        let source = Source::Events {
            root: root.to_owned(),
            events,
            _watcher: None,
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
        self.sources
            .retain(|workspace, _| watched.contains(workspace));
        self.dirty
            .retain(|workspace, _| watched.contains(workspace));
    }

    /// Moves queued events into `dirty`; true when any arrived.
    fn drain_events(&mut self, now: i64) -> bool {
        let mut arrived = false;
        let mut failed = Vec::new();
        for (workspace, source) in &self.sources {
            let Source::Events { root, events, .. } = source else {
                continue;
            };
            loop {
                match events.try_recv() {
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
            self.burst_started_ms.get_or_insert(now);
            self.last_event_ms = now;
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
        let settled = self.burst_started_ms.is_some_and(|started| {
            now - self.last_event_ms >= QUIET_MS || now - started >= BURST_LIMIT_MS
        });
        if !(settled || polled) || self.dirty.is_empty() {
            return BTreeMap::new();
        }
        self.burst_started_ms = None;
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

    /// Starts watching `workspace_id` when a connection first shows it.
    pub(super) fn start_project_source(&mut self, workspace_id: &str) -> Result<()> {
        let project = self.project_root(workspace_id)?;
        self.project_watches
            .ensure_source(workspace_id, project.root());
        Ok(())
    }
}

fn event_source(root: &Path) -> Option<Source> {
    let (sender, events) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(sender).ok()?;
    watcher.watch(root, RecursiveMode::Recursive).ok()?;
    Some(Source::Events {
        root: root.to_owned(),
        events,
        _watcher: Some(watcher),
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
    use super::Touched;
    use std::path::Path;

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
