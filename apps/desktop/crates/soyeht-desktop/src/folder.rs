//! Opening a folder as a workspace: the platform's folder picker, then either the workspace the
//! folder already has or a new one. The rules (its name, the workspace it reuses) are
//! `soyeht_client::state::FolderOpening`, the ones the Mac applies to a folder chosen for a pane.

use crate::workbench::Workbench;
use gpui::{Context, PathPromptOptions, Window, actions};
use soyeht_client::{Command, Connection, state::FolderOpening};
use std::path::PathBuf;

actions!(soyeht, [OpenFolder]);

/// The keys of the "Open Folder" command, as the empty window shows them.
pub(crate) const SHORTCUT: &str = if cfg!(target_os = "macos") {
    "\u{2318}O"
} else {
    "Ctrl+Shift+O"
};

/// What the picker answered: the folders chosen, none when it was cancelled, or why it could not
/// open.
type Picked = Result<Option<Vec<PathBuf>>, String>;

impl Workbench {
    pub(crate) fn open_folder_action(
        &mut self,
        _: &OpenFolder,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.choose_folder(cx);
    }

    /// Shows the platform's folder picker and opens what it returns.
    pub(crate) fn choose_folder(&mut self, cx: &mut Context<Self>) {
        self.menus.close();
        if self.state.connection != Connection::Connected {
            self.state.problem = Some(soyeht_client::Error::Disconnected.to_string());
            cx.notify();
            return;
        }
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |this, cx| {
            // A picker dropped before it answered is a cancelled one.
            let answer: Picked = picker
                .await
                .map_or(Ok(None), |answer| answer.map_err(|error| error.to_string()));
            // The window closed while the picker was open: nothing is left to open it in.
            let Ok(()) = this.update(cx, |this, cx| this.folder_chosen(answer, cx)) else {
                return;
            };
        })
        .detach();
    }

    fn folder_chosen(&mut self, answer: Picked, cx: &mut Context<Self>) {
        match answer {
            Ok(Some(folders)) => {
                if let Some(folder) = folders.first() {
                    self.open_folder_path(folder, cx);
                }
            }
            Ok(None) => {}
            Err(reason) => {
                self.state.problem = Some(format!("Couldn't open the folder picker. {reason}"));
                cx.notify();
            }
        }
    }

    /// Opens `folder`: its workspace when it has one (with a terminal if that has no pane), else
    /// a new workspace, which gets a terminal once the engine has made it.
    pub(crate) fn open_folder_path(&mut self, folder: &std::path::Path, cx: &mut Context<Self>) {
        let Some(path) = folder.to_str() else {
            self.state.problem = Some("That folder's name is not text Soyeht can use.".to_owned());
            cx.notify();
            return;
        };
        self.menus.close();
        match self.state.open_folder(path) {
            Ok(FolderOpening::Existing(workspace_id)) => {
                self.select_workspace(&workspace_id, cx);
                if self.state.panes_of(&workspace_id).next().is_none() {
                    self.open_pane(None, cx);
                }
            }
            Ok(FolderOpening::New { name, path }) => {
                let ticket = self.next_ticket();
                self.send(Command::CreateWorkspace { name, path, ticket });
                cx.notify();
            }
            Err(problem) => {
                self.state.problem = Some(problem.explanation().to_owned());
                cx.notify();
            }
        }
    }
}
