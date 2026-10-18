//! What an editor pane has to show before its text is on screen, as the Mac's `EditorPaneLoad`:
//! loading, ready, or failed with the reason the last read gave. A pane is never left waiting:
//! every request that could bring its first page ends in a page or in a reason, and the pane's
//! Retry asks again.

use super::{Editors, editor_panes};
use crate::Effect;
use soyeht_protocol::{Pane, PaneMode};

/// What the engine's tab list says of a pane it does not list.
const NO_FILE: &str = "The engine has no file for this editor tab.";

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
        self.fetch(pane)
    }

    /// What an editor pane without a page needs: the page of the file this client holds, else
    /// the file the engine lists for the pane, else the engine's tab list to find it in.
    pub(super) fn fetch(&mut self, pane: &Pane) -> Vec<Effect> {
        if pane.mode != PaneMode::Editor {
            return Vec::new();
        }
        if let Some(file) = self.file_of_pane_mut(&pane.pane_id) {
            return if file.page.is_none() {
                file.read_first_page()
            } else {
                Vec::new()
            };
        }
        let listed = self
            .tabs
            .get(&pane.workspace_id)
            .and_then(|tabs| tabs.iter().find(|tab| tab.pane_id == pane.pane_id));
        if let Some(tab) = listed {
            let file_id = tab.file_id.clone();
            return self.read_unknown(&file_id);
        }
        // The list may be a stale answer, or asked before this pane existed: ask again.
        self.asked.remove(&pane.workspace_id);
        self.ask_tabs(&pane.workspace_id)
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
                    .insert(pane.pane_id.clone(), NO_FILE.to_owned());
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
