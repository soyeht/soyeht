//! The part of the replica that is the folder's trust: which workspace the window asks about, and
//! the question put off. The rules are in `crate::trust`.

use super::State;

impl State {
    /// The workspace the window asks "Trust this folder?" about: the selected one, once the engine
    /// has said its folder is undecided, until the person answers or puts the question off.
    pub fn trust_prompt(&self) -> Option<&soyeht_protocol::Workspace> {
        self.selected_workspace()
            .filter(|workspace| self.trust.needs_decision(&workspace.workspace_id))
    }

    /// The person closed the question without answering: it does not come back until the
    /// application restarts, and the folder stays restricted.
    pub fn defer_trust_prompt(&mut self) {
        if let Some(workspace_id) = self.trust_prompt().map(|w| w.workspace_id.clone()) {
            self.trust.defer(&workspace_id);
        }
    }
}
