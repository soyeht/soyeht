//! The part of the replica that is the folder's trust: which workspace the window asks about, and
//! the question put off. The rules are in `crate::trust`.

use super::{Effect, State, problem};
use crate::Event;

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

    /// The note the window shows beside the selected workspace while its folder is restricted or
    /// undecided, with the way to ask the question again; nothing for a trusted folder.
    pub fn trust_marker(&self) -> Option<&'static str> {
        self.selected_workspace()
            .filter(|workspace| self.trust.is_restricted(&workspace.workspace_id))
            .map(|_| soyeht_protocol::client_policy::PROJECT_TRUST_RESTRICTED_NOTE)
    }

    /// The person asks to be asked again about the selected workspace's folder, which they put
    /// off or kept restricted.
    pub fn reopen_trust_prompt(&mut self) {
        if let Some(workspace_id) = self.selected_workspace().map(|w| w.workspace_id.clone()) {
            self.trust.reopen(&workspace_id);
        }
    }

    /// The engine's answer to the request to trust, or restrict, a folder, or the failure of it.
    pub(super) fn trust_event(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::ProjectTrust {
                workspace_ids,
                trust,
            } => self.trust.changed(workspace_ids, trust),
            _ => self.report(
                problem::kind::PROJECT_TRUST_NOT_SAVED,
                soyeht_protocol::client_policy::PROJECT_TRUST_FAILED,
            ),
        }
        Vec::new()
    }
}
