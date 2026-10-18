//! What an editor pane has to show before its text is on screen, as the Mac's `EditorPaneLoad`:
//! loading, ready, or failed with the reason the last read gave. A pane is never left waiting:
//! every request that could bring its first page ends in a page or in a reason, and the pane's
//! Retry asks again.

use super::{Editors, commands::PageRequest, editor_panes};
use crate::{Command, Effect, editor::EditorCommand};
use soyeht_protocol::{Pane, PaneMode, client_policy::EDITOR_NO_FILE};
use std::collections::HashMap;

/// Numbers the asks that bring a pane its first page, as the Mac's background load generation
/// does: an answer to an ask that a later one superseded (a Retry while it was on its way) must
/// not show a failure over the later ask.
#[derive(Default)]
pub(super) struct Attempts {
    /// The latest ask for the first page of each file.
    reads: HashMap<String, u64>,
    /// The latest ask for the tabs of each workspace.
    tabs: HashMap<String, u64>,
}

fn next(latest: &mut HashMap<String, u64>, key: &str) -> u64 {
    let attempt = latest.entry(key.to_owned()).or_default();
    *attempt += 1;
    *attempt
}

impl Attempts {
    fn next_read(&mut self, file_id: &str) -> u64 {
        next(&mut self.reads, file_id)
    }

    pub(super) fn next_tabs(&mut self, workspace_id: &str) -> u64 {
        next(&mut self.tabs, workspace_id)
    }

    /// Whether a failed read of `file_id` answers an ask that a later one replaced. Attempt 0
    /// is a read that is not an ask for the first page: it is never superseded.
    pub(super) fn read_superseded(&self, file_id: &str, attempt: u64) -> bool {
        attempt != 0 && self.reads.get(file_id) != Some(&attempt)
    }

    pub(super) fn tabs_superseded(&self, workspace_id: &str, attempt: u64) -> bool {
        self.tabs.get(workspace_id) != Some(&attempt)
    }

    pub(super) fn forget_file(&mut self, file_id: &str) {
        self.reads.remove(file_id);
    }

    pub(super) fn forget_workspace(&mut self, workspace_id: &str) {
        self.tabs.remove(workspace_id);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneLoad {
    /// The first page is on its way.
    Loading,
    /// A page is held.
    Ready,
    /// The last attempt could not get a page, and says why.
    Failed(String),
}

impl Editors {
    /// The state of a pane's first page: ready once a page is held, failed once the last attempt
    /// said why it could not, loading while one is on its way.
    pub fn pane_load(&self, pane_id: &str) -> PaneLoad {
        if self
            .file_of_pane(pane_id)
            .is_some_and(|file| file.page.is_some())
        {
            return PaneLoad::Ready;
        }
        self.failures
            .get(pane_id)
            .map_or(PaneLoad::Loading, |reason| PaneLoad::Failed(reason.clone()))
    }

    /// Asks again for the first page of a pane that failed to get one.
    pub fn retry(&mut self, pane: &Pane) -> Vec<Effect> {
        self.failures.remove(&pane.pane_id);
        // A pane that holds no file finds it in the engine's tab list again, as on the Mac: the
        // file the list named before may be gone.
        self.fetch(pane, true)
    }

    /// What an editor pane without a page needs: the page of the file this client holds, else
    /// the file the engine lists for the pane, else the engine's tab list to find it in.
    /// `refresh` asks for the list even when this client holds one that names the pane.
    pub(super) fn fetch(&mut self, pane: &Pane, refresh: bool) -> Vec<Effect> {
        if pane.mode != PaneMode::Editor {
            return Vec::new();
        }
        if let Some(file) = self.file_of_pane_mut(&pane.pane_id) {
            if file.page.is_some() {
                return Vec::new();
            }
            let effects = file.read_first_page();
            return self.stamped(effects);
        }
        let listed = self
            .tabs
            .get(&pane.workspace_id)
            .and_then(|tabs| tabs.iter().find(|tab| tab.pane_id == pane.pane_id));
        if let Some(tab) = listed.filter(|_| !refresh) {
            let file_id = tab.file_id.clone();
            return self.read_unknown(&file_id);
        }
        // The list may be a stale answer, or asked before this pane existed: ask again.
        self.asked.remove(&pane.workspace_id);
        self.ask_tabs(&pane.workspace_id)
    }

    /// The first page of a file the engine lists that this client has not read: an ask of its own.
    pub(super) fn read_unknown(&mut self, file_id: &str) -> Vec<Effect> {
        let attempt = self.attempts.next_read(file_id);
        vec![Effect::Send(Command::Editor(EditorCommand::Read(
            PageRequest {
                file_id: file_id.to_owned(),
                from_line: 0,
                lines: super::PAGE_LINES,
                attempt,
            },
        )))]
    }

    /// Numbers the reads of a first page among `effects` as asks of their own.
    fn stamped(&mut self, effects: Vec<Effect>) -> Vec<Effect> {
        effects
            .into_iter()
            .map(|effect| match effect {
                Effect::Send(Command::Editor(EditorCommand::Read(mut page)))
                    if page.from_line == 0 =>
                {
                    page.attempt = self.attempts.next_read(&page.file_id);
                    Effect::Send(Command::Editor(EditorCommand::Read(page)))
                }
                other => other,
            })
            .collect()
    }

    /// The panes of a workspace that hold no file and that `tabs` does not list have none to
    /// wait for.
    pub(super) fn unlisted(&mut self, workspace_id: &str, panes: &[Pane]) {
        for pane in editor_panes(panes, workspace_id) {
            let listed = self
                .tabs
                .get(workspace_id)
                .is_some_and(|tabs| tabs.iter().any(|tab| tab.pane_id == pane.pane_id));
            if !listed && self.file_of_pane(&pane.pane_id).is_none() {
                self.failures
                    .insert(pane.pane_id.clone(), EDITOR_NO_FILE.to_owned());
            }
        }
    }

    /// The tab list could not be read: the panes that wait on it for their file have none.
    pub(super) fn tabs_failed(&mut self, workspace_id: &str, reason: &str, panes: &[Pane]) {
        self.asked.remove(workspace_id);
        for pane in editor_panes(panes, workspace_id) {
            if self.file_of_pane(&pane.pane_id).is_none() {
                self.failures
                    .insert(pane.pane_id.clone(), reason.to_owned());
            }
        }
    }

    /// A read of `file_id` failed: a pane that holds no page for it says why. A pane that closed
    /// meanwhile is not told anything, and one that holds a page keeps it.
    pub(super) fn page_failed(&mut self, file_id: &str, reason: &str, panes: &[Pane]) {
        let pane_id = match self.files.get(file_id) {
            Some(file) if file.page.is_some() => return,
            Some(file) => file.doc.pane_id.clone(),
            None => match self
                .tabs
                .values()
                .flatten()
                .find(|tab| tab.file_id == file_id)
            {
                Some(tab) => tab.pane_id.clone(),
                None => return,
            },
        };
        if panes.iter().any(|pane| pane.pane_id == pane_id) {
            self.failures.insert(pane_id, reason.to_owned());
        }
    }
}
