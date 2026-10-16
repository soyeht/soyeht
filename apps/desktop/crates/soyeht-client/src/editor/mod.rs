//! The client's replica of the editor: the open files with their pages and carets, the tabs of
//! each workspace, and the project tree. It is a plain value that applies what the engine says and
//! what the person does, and answers with the commands to send, so every rule here is testable
//! without a window or a connection.
//!
//! The engine keeps the files, their history and their drafts; the replica holds only the page of
//! lines in view (`viewport`), so a file of any size costs the same.

pub(crate) mod commands;
pub mod editing;
mod file;
pub mod keys;
pub mod page;
pub mod selection;
pub mod tabs;
pub mod text;
pub mod tree;
mod trees;
mod viewport;

pub use commands::{EditorCommand, EditorEvent, Failure};
pub use file::EditorFile;
pub use trees::ProjectTrees;
pub use viewport::PAGE_LINES;

use crate::Effect;
use page::{Page, position};
use selection::{Motion, Selection};
use serde_json::Value;
use soyeht_protocol::{
    EditorTab, EventName, FileChangedOnDiskEvent, FileConflictEvent, FileDocument, FileSavedEvent,
    Pane, PaneMode, ProjectEntryRenamedEvent, ProjectTreeChangedEvent, TextPosition, TextRange,
    limits::EDITOR_EDIT_BYTES,
};
use std::collections::{HashMap, HashSet};

/// Whether revision `a` is at least as new as `b`: revisions are decimal numbers.
pub(crate) fn at_least(a: &str, b: &str) -> bool {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(a), Ok(b)) => a >= b,
        _ => a == b,
    }
}

#[derive(Default)]
pub struct Editors {
    files: HashMap<String, EditorFile>,
    /// The engine's order of the tabs of each workspace.
    tabs: HashMap<String, Vec<EditorTab>>,
    /// The panes this client activated in each workspace, newest first.
    history: HashMap<String, Vec<String>>,
    /// The workspaces whose tabs were asked for on this connection.
    asked: HashSet<String>,
    pub trees: ProjectTrees,
}

fn editor_panes<'a>(panes: &'a [Pane], workspace_id: &'a str) -> impl Iterator<Item = &'a Pane> {
    panes
        .iter()
        .filter(move |pane| pane.mode == PaneMode::Editor && pane.workspace_id == workspace_id)
}

impl Editors {
    pub fn file_of_pane(&self, pane_id: &str) -> Option<&EditorFile> {
        self.files.values().find(|file| file.doc.pane_id == pane_id)
    }

    pub fn file_of_pane_mut(&mut self, pane_id: &str) -> Option<&mut EditorFile> {
        self.files
            .values_mut()
            .find(|file| file.doc.pane_id == pane_id)
    }

    pub fn file(&self, file_id: &str) -> Option<&EditorFile> {
        self.files.get(file_id)
    }

    /// The tabs of a workspace in the engine's order. An editor pane the engine has not listed
    /// yet follows at the end.
    pub fn tab_list(&self, workspace_id: &str, panes: &[Pane]) -> Vec<EditorTab> {
        let mut tabs: Vec<EditorTab> = self
            .tabs
            .get(workspace_id)
            .into_iter()
            .flatten()
            .filter(|tab| editor_panes(panes, workspace_id).any(|pane| pane.pane_id == tab.pane_id))
            .cloned()
            .collect();
        for pane in editor_panes(panes, workspace_id) {
            if tabs.iter().all(|tab| tab.pane_id != pane.pane_id) {
                let file = self.file_of_pane(&pane.pane_id).map(|file| &file.doc);
                tabs.push(EditorTab {
                    pane_id: pane.pane_id.clone(),
                    file_id: file.map(|doc| doc.file_id.clone()).unwrap_or_default(),
                    path: file.map_or_else(|| pane.name.clone(), |doc| doc.path.clone()),
                    preview: false,
                    preview_revision: None,
                });
            }
        }
        tabs
    }

    fn tab_ids(&self, workspace_id: &str, panes: &[Pane]) -> Vec<String> {
        self.tab_list(workspace_id, panes)
            .into_iter()
            .map(|tab| tab.pane_id)
            .collect()
    }

    fn history_of(&self, workspace_id: &str) -> &[String] {
        self.history.get(workspace_id).map_or(&[], Vec::as_slice)
    }

    /// The editor pane a workspace shows: the selected one when it is a tab, else the one
    /// activated last.
    pub fn active_pane(
        &self,
        workspace_id: &str,
        selected: Option<&str>,
        panes: &[Pane],
    ) -> Option<String> {
        tabs::active(
            selected,
            &self.tab_ids(workspace_id, panes),
            self.history_of(workspace_id),
        )
    }

    /// Whether the layout draws `pane`: every editor pane of a workspace shares one slot, which
    /// shows the active tab.
    pub fn shows(&self, pane: &Pane, selected: Option<&str>, panes: &[Pane]) -> bool {
        pane.mode != PaneMode::Editor
            || self
                .active_pane(&pane.workspace_id, selected, panes)
                .is_none_or(|active| active == pane.pane_id)
    }

    /// Remembers that the person went to this pane.
    pub fn activated(&mut self, pane: &Pane) {
        if pane.mode != PaneMode::Editor {
            return;
        }
        let history = self.history.entry(pane.workspace_id.clone()).or_default();
        *history = tabs::recorded(&pane.pane_id, history);
    }

    /// The tab after (or before) the active one.
    pub fn cycled(
        &self,
        workspace_id: &str,
        selected: Option<&str>,
        panes: &[Pane],
        forward: bool,
    ) -> Option<String> {
        let active = self.active_pane(workspace_id, selected, panes);
        tabs::cycled(
            active.as_deref(),
            &self.tab_ids(workspace_id, panes),
            forward,
        )
    }

    /// Closes a tab once its edits have reached the engine, and names the tab that takes over
    /// when it was the one showing.
    pub fn close_tab(
        &mut self,
        pane: &Pane,
        selected: Option<&str>,
        panes: &[Pane],
    ) -> (Vec<Effect>, Option<String>) {
        let ids = self.tab_ids(&pane.workspace_id, panes);
        let showing = self
            .active_pane(&pane.workspace_id, selected, panes)
            .as_deref()
            == Some(&pane.pane_id);
        let next = showing
            .then(|| tabs::successor(&pane.pane_id, &ids, self.history_of(&pane.workspace_id)))
            .flatten();
        if let Some(history) = self.history.get_mut(&pane.workspace_id) {
            history.retain(|other| *other != pane.pane_id);
        }
        let effects = match self.file_of_pane_mut(&pane.pane_id) {
            Some(file) => file.close(),
            None => vec![Effect::Send(crate::Command::ClosePane {
                pane_id: pane.pane_id.clone(),
            })],
        };
        (effects, next)
    }

    fn ask_tabs(&mut self, workspace_id: &str) -> Vec<Effect> {
        if !self.asked.insert(workspace_id.to_owned()) {
            return Vec::new();
        }
        vec![Effect::Send(crate::Command::Editor(EditorCommand::Tabs {
            workspace_id: workspace_id.to_owned(),
        }))]
    }

    /// A pane arrived: an editor pane brings the tabs of its workspace.
    pub fn pane_arrived(&mut self, pane: &Pane) -> Vec<Effect> {
        if pane.mode == PaneMode::Editor {
            self.ask_tabs(&pane.workspace_id)
        } else {
            Vec::new()
        }
    }

    /// A pane closed with its file.
    pub fn pane_closed(&mut self, pane_id: &str) {
        self.files.retain(|_, file| file.doc.pane_id != pane_id);
        for history in self.history.values_mut() {
            history.retain(|other| other != pane_id);
        }
    }

    /// A workspace closed with its tabs and its tree.
    pub fn workspace_closed(&mut self, workspace_id: &str) {
        self.tabs.remove(workspace_id);
        self.history.remove(workspace_id);
        self.asked.remove(workspace_id);
        self.trees.forget(workspace_id);
    }

    /// A fresh connection: the files the engine lists are read again, and the tabs and trees
    /// are asked for again, because the engine's watches belong to the connection.
    pub fn adopt(&mut self, docs: Vec<FileDocument>, panes: &[Pane]) -> Vec<Effect> {
        let mut effects = Vec::new();
        let mut known = std::mem::take(&mut self.files);
        for doc in docs {
            let file = match known.remove(&doc.file_id) {
                Some(mut file) => {
                    effects.extend(file.reconnect(doc));
                    file
                }
                None => {
                    let mut file = EditorFile::new(doc);
                    effects.extend(file.show(0, 0));
                    file
                }
            };
            self.files.insert(file.doc.file_id.clone(), file);
        }
        self.asked.clear();
        let workspaces: HashSet<&str> = panes
            .iter()
            .filter(|pane| pane.mode == PaneMode::Editor)
            .map(|pane| pane.workspace_id.as_str())
            .collect();
        for workspace in workspaces {
            effects.extend(self.ask_tabs(workspace));
        }
        effects.extend(self.trees.reconnect());
        effects
    }

    /// Something the engine told the replica.
    pub fn apply(&mut self, event: EditorEvent, panes: &[Pane]) -> Vec<Effect> {
        let live = |doc: &FileDocument| panes.iter().any(|pane| pane.pane_id == doc.pane_id);
        match event {
            EditorEvent::Opened(opened) => {
                if !live(&opened.file) {
                    return Vec::new();
                }
                let file = self
                    .files
                    .entry(opened.file.file_id.clone())
                    .or_insert_with(|| EditorFile::new(opened.file.clone()));
                file.opened(&opened.file, &opened.viewport)
            }
            EditorEvent::Page(read) => {
                if !live(&read.file) {
                    return Vec::new();
                }
                self.files
                    .entry(read.file.file_id.clone())
                    .or_insert_with(|| EditorFile::new(read.file.clone()))
                    .install(&read, PAGE_LINES)
            }
            EditorEvent::Written { file_id, result } => self
                .files
                .get_mut(&file_id)
                .map(|file| file.written(*result))
                .unwrap_or_default(),
            EditorEvent::Replayed { file_id, result } => self
                .files
                .get_mut(&file_id)
                .map(|file| file.replayed(*result))
                .unwrap_or_default(),
            EditorEvent::Tabs(listed) => self.set_tabs(listed.workspace_id, listed.tabs),
            EditorEvent::Listed(listing) => self.trees.listed(&listing),
            EditorEvent::Found {
                seq,
                file_id,
                result,
            } => self
                .files
                .get_mut(&file_id)
                .map(|file| file.found(seq, result))
                .unwrap_or_default(),
            EditorEvent::Failed(failure) => self.failed(failure),
        }
    }

    /// The engine's order of a workspace's tabs; the files it lists that are not known yet are
    /// read.
    fn set_tabs(&mut self, workspace_id: String, tabs: Vec<EditorTab>) -> Vec<Effect> {
        let unknown: Vec<String> = tabs
            .iter()
            .filter(|tab| !self.files.contains_key(&tab.file_id))
            .map(|tab| tab.file_id.clone())
            .collect();
        self.tabs.insert(workspace_id, tabs);
        unknown
            .iter()
            .flat_map(|id| self.read_unknown(id))
            .collect()
    }

    /// A file the engine lists that this client has not read: its first page is asked for, and
    /// the file joins the replica when it arrives.
    fn read_unknown(&self, file_id: &str) -> Vec<Effect> {
        vec![Effect::Send(crate::Command::Editor(EditorCommand::Read(
            commands::PageRequest {
                file_id: file_id.to_owned(),
                from_line: 0,
                lines: PAGE_LINES,
            },
        )))]
    }

    fn failed(&mut self, failure: Failure) -> Vec<Effect> {
        match failure {
            Failure::Edit(file_id) => self
                .files
                .get_mut(&file_id)
                .map(EditorFile::edit_failed)
                .unwrap_or_default(),
            Failure::Read(file_id) => self
                .files
                .get_mut(&file_id)
                .map(EditorFile::read_failed)
                .unwrap_or_default(),
            Failure::List { workspace_id, path } => self.trees.list_failed(&workspace_id, &path),
        }
    }

    /// Whether a notification of the engine is about files, tabs or the project tree.
    pub fn handles(name: &EventName) -> bool {
        matches!(
            name,
            EventName::FileChangedOnDisk
                | EventName::FileSaved
                | EventName::FileConflict
                | EventName::EditorTabsUpdated
                | EventName::ProjectTreeChanged
                | EventName::ProjectEntryRenamed
        )
    }

    /// What a notification of the engine about files, tabs or the project tree changes.
    pub fn notification(
        &mut self,
        name: &EventName,
        params: Value,
    ) -> serde_json::Result<Vec<Effect>> {
        Ok(match name {
            EventName::FileChangedOnDisk => {
                let event: FileChangedOnDiskEvent = serde_json::from_value(params)?;
                match self.files.get_mut(&event.file_id) {
                    Some(file) => {
                        file.doc.disk_revision = event.disk_revision;
                        file.changed_lines = event.changed_lines;
                        file.reload()
                    }
                    None => Vec::new(),
                }
            }
            EventName::FileSaved => {
                let event: FileSavedEvent = serde_json::from_value(params)?;
                match self.files.get_mut(&event.file.file_id) {
                    Some(file) => {
                        file.saved_by = Some(event.by);
                        file.changed_lines.clear();
                        file.changed_elsewhere(event.file)
                    }
                    None => Vec::new(),
                }
            }
            EventName::FileConflict => {
                let event: FileConflictEvent = serde_json::from_value(params)?;
                if let Some(file) = self.files.get_mut(&event.file_id) {
                    file.doc.revision = event.draft_revision;
                    file.doc.disk_revision = event.disk_revision;
                    file.doc.conflicted = true;
                }
                Vec::new()
            }
            EventName::EditorTabsUpdated => {
                let event: soyeht_protocol::EditorTabsUpdatedEvent =
                    serde_json::from_value(params)?;
                self.set_tabs(event.workspace_id, event.tabs)
            }
            EventName::ProjectTreeChanged => {
                let event: ProjectTreeChangedEvent = serde_json::from_value(params)?;
                self.trees.changed(&event.workspace_id, &event.paths)
            }
            EventName::ProjectEntryRenamed => {
                let event: ProjectEntryRenamedEvent = serde_json::from_value(params)?;
                for doc in event.files {
                    if let Some(file) = self.files.get_mut(&doc.file_id) {
                        file.doc = doc;
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        })
    }

    // What the person does in an editor pane.

    /// Runs `act` on the file of `pane_id`.
    fn on_file(
        &mut self,
        pane_id: &str,
        act: impl FnOnce(&mut EditorFile) -> Vec<Effect>,
    ) -> Vec<Effect> {
        self.file_of_pane_mut(pane_id).map(act).unwrap_or_default()
    }

    /// Makes the plan `plan` draws up for the selection of the file of `pane_id`.
    fn change(
        &mut self,
        pane_id: &str,
        plan: impl FnOnce(&Page, &[TextRange]) -> Option<editing::Plan>,
    ) -> Vec<Effect> {
        self.on_file(pane_id, |file| {
            let Some(range) = file.range_to_edit() else {
                return Vec::new();
            };
            let made = file.page.as_ref().and_then(|page| plan(page, &[range]));
            made.map(|made| file.edit(&made)).unwrap_or_default()
        })
    }

    /// Types `text` over the selection: a character, a line break or a tab.
    pub fn insert(&mut self, pane_id: &str, text: &str) -> Vec<Effect> {
        self.change(pane_id, |page, ranges| {
            editing::insertion(page, ranges, text)
        })
    }

    /// Backspace (`backward`) or Delete.
    pub fn delete(&mut self, pane_id: &str, backward: bool) -> Vec<Effect> {
        self.change(pane_id, |page, ranges| {
            editing::deletion(page, ranges, backward)
        })
    }

    /// Backspace or Delete over the word before or after the caret; over a selection, or at the
    /// edge of a line, it is the plain key.
    pub fn delete_word(&mut self, pane_id: &str, backward: bool) -> Vec<Effect> {
        self.change(pane_id, |page, ranges| {
            let [range] = ranges else {
                return editing::deletion(page, ranges, backward);
            };
            let at = &range.start;
            let text = page.text(at.line);
            let at_edge = if backward {
                at.column == 0
            } else {
                at.column >= page.line_end(at.line)
            };
            if *at != range.end || at_edge {
                return editing::deletion(page, ranges, backward);
            }
            let edge = if backward {
                text::word_before(text, at.column)
            } else {
                text::word_after(text, at.column)
            };
            let (from, to) = (edge.min(at.column), edge.max(at.column));
            let span = TextRange {
                start: position(at.line, from),
                end: position(at.line, to),
            };
            editing::deletion(page, &[span], backward)
        })
    }

    /// Pastes `text` as it is, line breaks of any system made one. A paste the engine would
    /// refuse for its size is refused here, with the reason.
    pub fn paste(&mut self, pane_id: &str, text: &str) -> Result<Vec<Effect>, &'static str> {
        // A file that breaks its lines with `\r\n` takes the paste's lines the same way.
        let ending = self
            .file_of_pane(pane_id)
            .map_or(page::LF, EditorFile::line_ending);
        let text = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace(page::LF, ending);
        if text.len() > EDITOR_EDIT_BYTES {
            return Err("The text is too large to paste into the editor.");
        }
        let strings = [text];
        Ok(self.change(pane_id, |page, ranges| {
            editing::verbatim(page, ranges, &strings)
        }))
    }

    /// The selected text, for copying.
    pub fn copy(&self, pane_id: &str) -> Option<String> {
        self.file_of_pane(pane_id)?.selected_text()
    }

    /// The selected text, which Cut also deletes.
    pub fn cut(&mut self, pane_id: &str) -> (Option<String>, Vec<Effect>) {
        let text = self.copy(pane_id);
        let effects = if text.is_some() {
            self.delete(pane_id, true)
        } else {
            Vec::new()
        };
        (text, effects)
    }

    /// Moves the caret, or grows the selection when `extend`.
    pub fn motion(&mut self, pane_id: &str, motion: Motion, extend: bool) -> Vec<Effect> {
        self.on_file(pane_id, |file| file.motion(motion, extend))
    }

    /// Selects the page: the part of the document that is loaded.
    pub fn select_all(&mut self, pane_id: &str) -> Vec<Effect> {
        self.on_file(pane_id, EditorFile::select_all)
    }

    /// A click (or a drag, with `extend`) at `at`.
    pub fn click(&mut self, pane_id: &str, at: TextPosition, extend: bool) -> Vec<Effect> {
        self.on_file(pane_id, |file| {
            let target = file.selection.moved_to(at, extend);
            file.place(target)
        })
    }

    /// A double click selects the word at `at`; a triple click, its line.
    pub fn select_around(&mut self, pane_id: &str, at: &TextPosition, line: bool) -> Vec<Effect> {
        self.on_file(pane_id, |file| {
            let Some(page) = file.page.as_ref().filter(|page| page.holds(at.line)) else {
                return Vec::new();
            };
            let (start, end) = if line {
                (
                    position(at.line, 0),
                    position(at.line, page.line_end(at.line)),
                )
            } else {
                let (from, to) = text::word_at(page.text(at.line), at.column);
                (position(at.line, from), position(at.line, to))
            };
            file.place(Selection::selecting(start, end))
        })
    }

    pub fn save(&mut self, pane_id: &str) -> Vec<Effect> {
        self.on_file(pane_id, EditorFile::save)
    }

    pub fn replay(&mut self, pane_id: &str, redo: bool) -> Vec<Effect> {
        self.on_file(pane_id, |file| file.replay(redo))
    }

    /// What the view shows: the first line and how many, so that the page that holds them is
    /// read in time.
    pub fn show(&mut self, pane_id: &str, first_line: u32, rows: u32) -> Vec<Effect> {
        self.on_file(pane_id, |file| file.show(first_line, rows))
    }
}

#[cfg(test)]
mod tests;
