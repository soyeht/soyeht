//! Renaming a workspace (its tab) or a pane (its header) where its name is drawn: the name
//! becomes a one-line field, Return keeps the new name, Escape drops it, and moving the keyboard
//! elsewhere keeps it, as in a file manager. The rules for a name are `soyeht_client::clean_name`.

use crate::composer_field::{ComposerEvent, ComposerField, new_field};
use crate::workbench::Workbench;
use gpui::{Context, Entity, Focusable, Window};
use soyeht_client::{Command, state::submission};

/// What is being renamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenameTarget {
    Workspace(String),
    Pane(String),
}

/// The name being edited.
pub(crate) struct Rename {
    pub(crate) target: RenameTarget,
    pub(crate) field: Entity<ComposerField>,
    /// The field has had the keyboard: losing it afterwards ends the rename.
    had_focus: bool,
}

impl Workbench {
    /// The field of the name being edited, when it is `target`'s.
    pub(crate) fn rename_field(&self, target: &RenameTarget) -> Option<Entity<ComposerField>> {
        self.rename
            .as_ref()
            .filter(|rename| &rename.target == target)
            .map(|rename| rename.field.clone())
    }

    /// Turns the name of `target` into a field holding `current`, with the keyboard.
    pub(crate) fn begin_rename(
        &mut self,
        target: RenameTarget,
        current: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.rename_field(&target).is_some() {
            return;
        }
        self.commit_rename(cx);
        self.menus.close();
        let field = new_field(self.theme.palette(), cx);
        field.update(cx, |field, cx| field.insert(current, cx));
        cx.subscribe(&field, |this, _, event: &ComposerEvent, cx| match event {
            ComposerEvent::Submit => this.commit_rename(cx),
            ComposerEvent::Dismiss => this.cancel_rename(cx),
            ComposerEvent::Attach => {}
        })
        .detach();
        // The first click of a double click asked to focus a pane on the next frame; on a slow
        // frame that would take the keyboard from the field the second click opens.
        self.focus_request = None;
        window.focus(&field.focus_handle(cx), cx);
        self.rename = Some(Rename {
            target,
            field,
            had_focus: false,
        });
        cx.notify();
    }

    /// Sends the new name, when there is one and it differs from the name now.
    pub(crate) fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(rename) = self.rename.take() else {
            return;
        };
        // Shift+Return adds a line to the field; a name is one line.
        let typed = rename
            .field
            .read(cx)
            .draft()
            .text()
            .replace(['\n', '\r'], " ");
        let command = match &rename.target {
            RenameTarget::Workspace(id) => {
                let current = self.state.workspace(id).map(|w| w.name.as_str());
                submission(&typed, current.unwrap_or_default()).map(|name| {
                    Command::RenameWorkspace {
                        workspace_id: id.clone(),
                        name,
                    }
                })
            }
            RenameTarget::Pane(id) => {
                let current = self.state.pane(id).map(|pane| pane.name.as_str());
                submission(&typed, current.unwrap_or_default()).map(|name| Command::RenamePane {
                    pane_id: id.clone(),
                    name,
                })
            }
        };
        if let Some(command) = command {
            self.send(command);
        }
        cx.notify();
    }

    pub(crate) fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.rename.take().is_some() {
            cx.notify();
        }
    }

    /// Called at the start of a frame: a rename whose field lost the keyboard is over.
    pub(crate) fn settle_rename(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(rename) = self.rename.as_mut() else {
            return;
        };
        if rename.field.focus_handle(cx).is_focused(window) {
            rename.had_focus = true;
        } else if rename.had_focus {
            self.commit_rename(cx);
        }
    }
}
