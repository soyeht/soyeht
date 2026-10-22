//! How a folder becomes a workspace, which folders are offered again, and which workspace is
//! selected when one closes. The Mac and iOS keep the same rules (`WorkspaceFolderPolicy`,
//! `WorkspaceNaming`, `WorkspaceTabStripPolicy`), and `contracts/vectors/workspaces.json` holds
//! the cases all of them pass.

use super::{Effect, State};
use crate::{Command, new_agent::NewAgent};
use soyeht_protocol::Workspace;

/// Why a typed or chosen path cannot be a workspace's folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderPathProblem {
    Empty,
    NotAbsolute,
}

impl FolderPathProblem {
    /// What to tell the person.
    pub fn explanation(self) -> &'static str {
        match self {
            Self::Empty => "Choose a folder.",
            Self::NotAbsolute => {
                "Use the full path from the top of the disk, such as /home/you/project."
            }
        }
    }
}

/// What opening a folder does: select the workspace that has it, or make one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderOpening {
    /// A workspace already has this folder.
    Existing(String),
    /// No workspace has it: the engine is asked to make one with this name and path.
    New { name: String, path: String },
}

/// A folder to offer again: where a workspace is rooted, or a folder a pane runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentFolder {
    pub path: String,
    pub name: String,
    /// The workspace already rooted here, if any.
    pub workspace_id: Option<String>,
}

/// What starting an agent asks for: the workspace the window selects (the one the folder
/// already has) and the commands to carry out.
#[derive(Debug, Clone, PartialEq)]
pub struct Started {
    pub select: Option<String>,
    pub effects: Vec<Effect>,
}

fn is_separator(character: char) -> bool {
    character == '/' || character == '\\'
}

/// A path as the engine takes it: trimmed, absolute, without a trailing separator (a disk's root
/// keeps its own).
pub fn resolve_path(typed: &str) -> Result<String, FolderPathProblem> {
    let path = typed.trim();
    if path.is_empty() {
        return Err(FolderPathProblem::Empty);
    }
    if !is_absolute(path) {
        return Err(FolderPathProblem::NotAbsolute);
    }
    let mut kept = path;
    while kept.len() > 1
        && kept.ends_with(is_separator)
        && !kept.ends_with(":\\")
        && !kept.ends_with(":/")
    {
        kept = &kept[..kept.len() - 1];
    }
    Ok(kept.to_owned())
}

fn is_absolute(path: &str) -> bool {
    if path.starts_with('/') || path.starts_with("\\\\") {
        return true;
    }
    let mut characters = path.chars();
    matches!(
        (characters.next(), characters.next(), characters.next()),
        (Some(drive), Some(':'), Some(separator)) if drive.is_alphabetic() && is_separator(separator)
    )
}

/// The last component of a path, which names the workspace made for it.
pub fn name_of(path: &str) -> String {
    path.split(is_separator)
        .rfind(|part| !part.is_empty())
        .unwrap_or(path)
        .to_owned()
}

/// The name to save for a rename, or `None` while it is blank or unchanged.
pub fn submission(typed: &str, current: &str) -> Option<String> {
    let name = typed.trim();
    (!name.is_empty() && name != current).then(|| name.to_owned())
}

/// The workspace selected after `closed` goes: the one after it, or the one before it when it was
/// last, as in a browser. `None` when it was the only one, or is not listed.
pub(super) fn selection_after_closing(order: &[String], closed: &str) -> Option<String> {
    let index = order.iter().position(|id| id == closed)?;
    order
        .get(index + 1)
        .or_else(|| index.checked_sub(1).and_then(|before| order.get(before)))
        .cloned()
}

impl State {
    /// What opening the folder at `path` does, or why it cannot be opened.
    pub fn open_folder(&self, path: &str) -> Result<FolderOpening, FolderPathProblem> {
        let path = resolve_path(path)?;
        Ok(match self.workspaces.iter().find(|w| w.path == path) {
            Some(workspace) => FolderOpening::Existing(workspace.workspace_id.clone()),
            None => FolderOpening::New {
                name: name_of(&path),
                path,
            },
        })
    }

    /// The folders the person works in: the roots of the workspaces in their order, then the
    /// folders panes run in that are not a root, each once.
    pub fn recent_folders(&self) -> Vec<RecentFolder> {
        let mut folders: Vec<RecentFolder> = Vec::new();
        let roots = self
            .workspaces
            .iter()
            .map(|workspace| (&workspace.path, Some(workspace.workspace_id.clone())));
        let runs = self.panes.iter().map(|pane| (&pane.cwd, None));
        for (path, workspace_id) in roots.chain(runs) {
            if !path.is_empty() && !folders.iter().any(|folder| &folder.path == path) {
                folders.push(RecentFolder {
                    name: name_of(path),
                    path: path.clone(),
                    workspace_id,
                });
            }
        }
        folders
    }

    /// Starts an agent as the Mac's new-pane form does: in the workspace of the form's folder
    /// (made first when the folder has none), running its harness, and sent the first message
    /// once its pane is here. Says why it cannot start when the form does not allow it.
    pub fn start_agent(&mut self, ticket: u64, form: &NewAgent) -> Result<Started, &'static str> {
        if let Some(blocker) = form.blocker(&self.harnesses) {
            return Err(blocker.explanation());
        }
        let harness = form.harness.clone();
        match self
            .open_folder(&form.directory)
            .map_err(FolderPathProblem::explanation)?
        {
            FolderOpening::Existing(workspace_id) => {
                self.select_workspace(&workspace_id);
                self.expect_open(ticket, &workspace_id);
                if let Some(expected) = self.expected.last_mut() {
                    expected.start = Some(form.clone());
                }
                Ok(Started {
                    select: Some(workspace_id.clone()),
                    effects: vec![Effect::Send(Command::CreateAgent {
                        workspace_id,
                        harness,
                        cwd: None,
                        ticket,
                    })],
                })
            }
            FolderOpening::New { name, path } => {
                self.starts.insert(ticket, form.clone());
                Ok(Started {
                    select: None,
                    effects: vec![Effect::Send(Command::CreateWorkspace {
                        name,
                        path,
                        ticket,
                    })],
                })
            }
        }
    }

    /// The requests in flight are lost with the connection; an agent that was starting is kept
    /// for the window to take back to its form.
    pub(super) fn forget_requests(&mut self) {
        let waiting = self.starts.drain().map(|(_, form)| form);
        let opening = self
            .expected
            .drain(..)
            .filter_map(|expected| expected.start);
        if let Some(form) = waiting.chain(opening).last() {
            self.failed_start = Some(form);
        }
    }

    /// Request `ticket` to open a pane or a workspace failed: nothing waits for it any more.
    pub(super) fn fail_request(&mut self, ticket: u64) {
        let opening = self
            .expected
            .iter()
            .position(|expected| expected.ticket == ticket)
            .and_then(|at| self.expected.remove(at).start);
        if let Some(form) = opening.or_else(|| self.starts.remove(&ticket)) {
            self.failed_start = Some(form);
        }
    }

    /// The agent that could not start, with what the person typed, once.
    pub fn take_failed_start(&mut self) -> Option<NewAgent> {
        self.failed_start.take()
    }

    /// The engine made the workspace that request `ticket` asked for: it is selected and a
    /// terminal opens in its folder, as the Mac does when a folder is chosen for a pane, or the
    /// agent that was waiting for the workspace starts.
    pub(crate) fn workspace_opened(&mut self, ticket: u64, workspace: Workspace) -> Vec<Effect> {
        let workspace_id = workspace.workspace_id.clone();
        let cwd = Some(workspace.path.clone());
        self.upsert_workspace(workspace);
        self.select_workspace(&workspace_id);
        self.expect_open(ticket, &workspace_id);
        let command = match self.starts.remove(&ticket) {
            Some(start) => {
                let harness = start.harness.clone();
                if let Some(expected) = self.expected.last_mut() {
                    expected.start = Some(start);
                }
                Command::CreateAgent {
                    workspace_id,
                    harness,
                    cwd,
                    ticket,
                }
            }
            None => Command::CreateTerminal {
                workspace_id,
                cwd,
                ticket,
            },
        };
        vec![Effect::Send(command)]
    }
}

#[cfg(test)]
mod tests;
