//! Closing a workspace closes every document in it, so what was typed there reaches the engine
//! first, as it does for one pane (`EditorFile::close`).

use super::Editors;
use crate::{Command, Effect};

impl Editors {
    /// Closes the workspace now, or once the edits of its documents have reached the engine.
    pub fn close_workspace(&mut self, workspace_id: &str) -> Vec<Effect> {
        if self.is_settling(workspace_id) {
            self.closing.insert(workspace_id.to_owned());
            return Vec::new();
        }
        vec![close(workspace_id)]
    }

    /// The workspaces that were waiting for their edits and have none left to wait for now.
    pub fn settled_workspace_closes(&mut self) -> Vec<Effect> {
        let ready: Vec<String> = self
            .closing
            .iter()
            .filter(|workspace_id| !self.is_settling(workspace_id))
            .cloned()
            .collect();
        ready
            .iter()
            .map(|workspace_id| {
                self.closing.remove(workspace_id);
                close(workspace_id)
            })
            .collect()
    }

    fn is_settling(&self, workspace_id: &str) -> bool {
        self.files
            .values()
            .any(|file| file.doc.workspace_id == workspace_id && file.is_settling())
    }
}

fn close(workspace_id: &str) -> Effect {
    Effect::Send(Command::CloseWorkspace {
        workspace_id: workspace_id.to_owned(),
    })
}
