//! The part of the replica that is the editor's: what the engine said about files, and what the
//! person does in an editor pane. The rules are in `crate::editor`; this is where they meet the
//! panes, the focus and the problem line.

use super::{Effect, State};
use crate::{
    Command,
    editor::{EditorCommand, EditorEvent, Editors},
};
use soyeht_protocol::{EventName, PaneMode};

impl State {
    pub(super) fn editor_event(&mut self, event: EditorEvent) -> Vec<Effect> {
        let mut effects = Vec::new();
        if let EditorEvent::Opened(opened) = &event {
            // The file's pane may arrive before this answer, or after it.
            effects.extend(self.upsert_pane(opened.pane.clone()));
            effects.push(Effect::FocusPane(opened.pane.pane_id.clone()));
        }
        effects.extend(self.editors.apply(event, &self.panes));
        effects
    }

    pub(super) fn editor_notification(
        &mut self,
        name: &EventName,
        params: serde_json::Value,
    ) -> serde_json::Result<Vec<Effect>> {
        self.editors.notification(name, params, &self.panes)
    }

    pub(super) fn handles_editor_event(name: &EventName) -> bool {
        Editors::handles(name)
    }

    /// Asks again for the first page of an editor pane that could not get one.
    pub fn editor_retry(&mut self, pane_id: &str) -> Vec<Effect> {
        match self
            .pane(pane_id)
            .filter(|pane| pane.mode == PaneMode::Editor)
            .cloned()
        {
            Some(pane) => self.editors.retry(&pane),
            None => Vec::new(),
        }
    }

    /// Opens a file of the workspace in a tab: a preview that the next file replaces, or a
    /// permanent tab.
    pub fn editor_open(&mut self, workspace_id: &str, path: &str, preview: bool) -> Vec<Effect> {
        vec![Effect::Send(Command::Editor(EditorCommand::Open {
            workspace_id: workspace_id.to_owned(),
            path: path.to_owned(),
            preview,
        }))]
    }

    /// Goes to the next (or previous) tab of the workspace the focused pane is in.
    pub fn editor_cycle(&mut self, workspace_id: &str, forward: bool) {
        let selected = self.focused_pane().map(|pane| pane.pane_id.clone());
        if let Some(next) =
            self.editors
                .cycled(workspace_id, selected.as_deref(), &self.panes, forward)
        {
            self.focus_pane(&next);
        }
    }

    /// The editor place of a workspace, which the "Editor" entry of the new-pane menu goes to:
    /// the tab that shows takes the keyboard. A workspace with no tab has nothing to focus and
    /// the place is its empty one, the project panel to pick a file from.
    pub fn editor_place(&self, workspace_id: &str) -> Vec<Effect> {
        let selected = self.focused_pane().map(|pane| pane.pane_id.clone());
        self.editors
            .active_pane(workspace_id, selected.as_deref(), &self.panes)
            .map(Effect::FocusPane)
            .into_iter()
            .collect()
    }

    /// Closes a pane, as the close button of its header asks. An editor pane is a tab, which
    /// closes after its edits have reached the engine; any other pane closes at once.
    pub fn close_pane_effects(&mut self, pane_id: &str) -> Vec<Effect> {
        if self
            .pane(pane_id)
            .is_some_and(|pane| pane.mode == PaneMode::Editor)
        {
            return self.editor_close_tab(pane_id);
        }
        vec![Effect::Send(Command::ClosePane {
            pane_id: pane_id.to_owned(),
        })]
    }

    /// Closes a tab. The edits that have not reached the engine go first, and the tab that was
    /// active before it takes over.
    pub fn editor_close_tab(&mut self, pane_id: &str) -> Vec<Effect> {
        let Some(pane) = self
            .pane(pane_id)
            .filter(|pane| pane.mode == PaneMode::Editor)
            .cloned()
        else {
            return Vec::new();
        };
        let selected = self.focused_pane().map(|pane| pane.pane_id.clone());
        let (mut effects, next) = self
            .editors
            .close_tab(&pane, selected.as_deref(), &self.panes);
        effects.extend(next.map(Effect::FocusPane));
        effects
    }

    /// Pastes text into an editor pane; a text the engine would refuse is said on the problem
    /// line.
    pub fn editor_paste(&mut self, pane_id: &str, text: &str) -> Vec<Effect> {
        match self.editors.paste(pane_id, text) {
            Ok(effects) => effects,
            Err(reason) => {
                self.problem = Some(reason.to_owned());
                Vec::new()
            }
        }
    }
}
