//! Project trust: whether the person has said that the folder of a workspace may run what it
//! contains. The engine holds the answer (`trust` section of the snapshot, `project.trust_changed`
//! events); this replica adds only what the window needs to ask once per session. A folder that is
//! not trusted is restricted, and so is one that is undecided.

use soyeht_protocol::{ProjectTrust, ProjectTrustEntry};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default)]
pub struct Trusts {
    /// What the engine last said, by workspace.
    known: HashMap<String, ProjectTrust>,
    /// The workspaces whose prompt the person dismissed without answering. It is kept for the
    /// session, so a reconnect does not ask again, and an answer clears it.
    deferred: HashSet<String>,
}

impl Trusts {
    /// The engine's list on a new connection replaces whatever was known.
    pub fn adopt(&mut self, entries: Vec<ProjectTrustEntry>) {
        self.known = entries
            .into_iter()
            .map(|entry| (entry.workspace_id, entry.trust))
            .collect();
        self.deferred.retain(|id| self.known.contains_key(id));
    }

    /// The engine changed the trust of the folder that `workspace_ids` share.
    pub fn changed(&mut self, workspace_ids: Vec<String>, trust: ProjectTrust) {
        for id in workspace_ids {
            if trust != ProjectTrust::Undecided {
                self.deferred.remove(&id);
            }
            self.known.insert(id, trust.clone());
        }
    }

    /// The workspace is gone.
    pub fn forget(&mut self, workspace_id: &str) {
        self.known.remove(workspace_id);
        self.deferred.remove(workspace_id);
    }

    /// The connection ended: nothing the engine said holds any more. What the person dismissed
    /// stays dismissed until the application restarts.
    pub fn connection_lost(&mut self) {
        self.known.clear();
    }

    /// The engine has no answer yet for this workspace's folder, and the person has not put the
    /// question off.
    pub fn needs_decision(&self, workspace_id: &str) -> bool {
        self.known.get(workspace_id) == Some(&ProjectTrust::Undecided)
            && !self.deferred.contains(workspace_id)
    }

    /// Puts the question off until the application restarts.
    pub fn defer(&mut self, workspace_id: &str) {
        self.deferred.insert(workspace_id.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, trust: ProjectTrust) -> ProjectTrustEntry {
        ProjectTrustEntry {
            workspace_id: id.to_owned(),
            trust,
        }
    }

    fn loaded() -> Trusts {
        let mut trusts = Trusts::default();
        trusts.adopt(vec![
            entry("a", ProjectTrust::Undecided),
            entry("b", ProjectTrust::Trusted),
            entry("c", ProjectTrust::Restricted),
        ]);
        trusts
    }

    #[test]
    fn only_an_undecided_folder_needs_a_decision() {
        let trusts = loaded();
        assert!(trusts.needs_decision("a"));
        assert!(!trusts.needs_decision("b"));
        assert!(!trusts.needs_decision("c"));
        assert!(
            !trusts.needs_decision("unknown"),
            "no word from the engine, no question"
        );
    }

    #[test]
    fn dismissing_the_question_puts_it_off_through_a_reconnect() {
        let mut trusts = loaded();
        trusts.defer("a");
        assert!(!trusts.needs_decision("a"));
        trusts.connection_lost();
        trusts.adopt(vec![entry("a", ProjectTrust::Undecided)]);
        assert!(!trusts.needs_decision("a"), "not asked again this session");
    }

    #[test]
    fn an_answer_replaces_the_state_and_clears_the_deferral() {
        let mut trusts = loaded();
        trusts.defer("a");
        trusts.changed(vec!["a".into(), "d".into()], ProjectTrust::Trusted);
        assert!(!trusts.needs_decision("a"));
        assert!(!trusts.deferred.contains("a"));
        assert_eq!(trusts.known.get("d"), Some(&ProjectTrust::Trusted));
    }

    #[test]
    fn a_disconnect_forgets_what_the_engine_said() {
        let mut trusts = loaded();
        trusts.connection_lost();
        assert!(!trusts.needs_decision("a"));
        assert!(trusts.known.is_empty());
    }

    #[test]
    fn a_closed_workspace_takes_its_trust_with_it() {
        let mut trusts = loaded();
        trusts.defer("a");
        trusts.forget("a");
        assert!(!trusts.known.contains_key("a"));
        assert!(trusts.deferred.is_empty());
    }

    #[test]
    fn a_new_snapshot_drops_the_deferral_of_a_workspace_that_is_gone() {
        let mut trusts = loaded();
        trusts.defer("a");
        trusts.adopt(vec![entry("b", ProjectTrust::Trusted)]);
        assert!(trusts.deferred.is_empty());
    }
}
