//! Follows the user's skill folder and the workspace skill folders clients
//! list, from operating-system file events (the `notify` crate, as the
//! project panel does). A burst is read once it settles, and a folder is
//! announced only when its listing changed. A folder that does not exist yet,
//! or that no event stream could follow, is looked at every two seconds.

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use soyeht_protocol::Skill;
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

/// A burst is read once no event arrived for this long ...
const QUIET_MS: i64 = 75;
/// ... or once it has lasted this long.
const BURST_LIMIT_MS: i64 = 500;
const PROBE_INTERVAL_MS: i64 = 2_000;

/// `None` is the user's folder, `Some(id)` a workspace's.
type Key = Option<String>;

struct Folder {
    root: PathBuf,
    events: Option<(Receiver<notify::Result<Event>>, RecommendedWatcher)>,
    fingerprint: u64,
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
}

impl Watches {
    pub(super) fn follow_user(&mut self, root: &Path, listed: &[Skill]) {
        self.user = Some(Folder::new(root, listed));
    }

    pub(super) fn show(
        &mut self,
        connection: &str,
        workspace_id: &str,
        root: &Path,
        listed: &[Skill],
    ) {
        let (folder, viewers) = self
            .workspaces
            .entry(workspace_id.into())
            .or_insert_with(|| (Folder::new(root, listed), BTreeSet::new()));
        if folder.root != root {
            *folder = Folder::new(root, listed);
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
            if folder.drain() | (probe && folder.reattach()) {
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

    /// Keeps a folder's new listing; true when it differs from the last one.
    pub(super) fn record(&mut self, workspace_id: Option<&str>, listed: &[Skill]) -> bool {
        let folder = match workspace_id {
            None => self.user.as_mut(),
            Some(id) => self.workspaces.get_mut(id).map(|(folder, _)| folder),
        };
        let Some(folder) = folder else {
            return false;
        };
        let current = fingerprint(listed);
        let changed = current != folder.fingerprint;
        folder.fingerprint = current;
        changed
    }
}

impl Folder {
    fn new(root: &Path, listed: &[Skill]) -> Self {
        let mut folder = Self {
            root: root.into(),
            events: None,
            fingerprint: fingerprint(listed),
        };
        folder.reattach();
        folder
    }

    /// Follows the folder while it exists. True when it appeared, vanished,
    /// or cannot be followed by events and must be read again.
    fn reattach(&mut self) -> bool {
        match (self.root.is_dir(), self.events.is_some()) {
            (true, false) => {
                self.events = event_source(&self.root);
                true
            }
            (false, true) => {
                self.events = None;
                true
            }
            _ => false,
        }
    }

    /// Empties the event queue; true when anything arrived.
    fn drain(&mut self) -> bool {
        let Some((events, _)) = &self.events else {
            return false;
        };
        let mut arrived = false;
        loop {
            match events.try_recv() {
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

fn event_source(root: &Path) -> Option<(Receiver<notify::Result<Event>>, RecommendedWatcher)> {
    let (sender, events) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(sender).ok()?;
    watcher.watch(root, RecursiveMode::Recursive).ok()?;
    Some((events, watcher))
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

    #[test]
    fn a_missing_folder_is_probed_and_listings_are_announced_only_when_changed() {
        let root =
            std::env::temp_dir().join(format!("soyeht-skill-watch-{}", uuid::Uuid::new_v4()));
        let mut watches = Watches::default();
        watches.show("connection", "workspace", &root, &[]);
        assert!(watches.settled(0, |_| true).is_empty());
        std::fs::create_dir_all(&root).unwrap();
        assert!(watches.settled(PROBE_INTERVAL_MS - 1, |_| true).is_empty());
        let start = PROBE_INTERVAL_MS;
        assert!(watches.settled(start, |_| true).is_empty());
        // File events from the new folder may extend the burst, never past its limit.
        let settled = watches.settled(start + BURST_LIMIT_MS, |_| true);
        assert_eq!(settled, [(Some("workspace".into()), root.clone())]);
        assert!(!watches.record(Some("workspace"), &[]));
        assert!(watches.record(Some("workspace"), &[skill("new")]));
        assert!(!watches.record(Some("workspace"), &[skill("new")]));
        watches.settled(start + BURST_LIMIT_MS, |_| false);
        assert!(!watches.record(Some("workspace"), &[]));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
