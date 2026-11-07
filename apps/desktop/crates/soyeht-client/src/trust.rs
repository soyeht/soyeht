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
    /// Whether the engine has shown that it keeps the state on this connection: its snapshot
    /// listed a workspace, or it announced a change. An engine that never does is asked nothing,
    /// and once it has, a workspace with no entry (one created after the snapshot, whose
    /// announcement has not come yet) counts as undecided.
    announced: bool,
    /// The workspaces whose prompt the person dismissed without answering. It is kept for the
    /// session, so a reconnect does not ask again, and an answer clears it.
    deferred: HashSet<String>,
    /// The workspaces whose folder is restricted and whose question the person asked to see again.
    /// An answer clears it.
    reopened: HashSet<String>,
}

impl Trusts {
    /// The engine's list on a new connection replaces whatever was known.
    pub fn adopt(&mut self, entries: Vec<ProjectTrustEntry>) {
        self.announced = !entries.is_empty();
        self.known = entries
            .into_iter()
            .map(|entry| (entry.workspace_id, entry.trust))
            .collect();
        self.deferred.retain(|id| self.known.contains_key(id));
        self.reopened.retain(|id| self.known.contains_key(id));
    }

    /// The engine changed the trust of the folder that `workspace_ids` share. It names workspaces
    /// the list did not (one created since), and each gets an entry.
    pub fn changed(&mut self, workspace_ids: Vec<String>, trust: ProjectTrust) {
        self.announced = true;
        for id in workspace_ids {
            if trust != ProjectTrust::Undecided {
                self.deferred.remove(&id);
            }
            self.reopened.remove(&id);
            self.known.insert(id, trust.clone());
        }
    }

    /// The workspace is gone.
    pub fn forget(&mut self, workspace_id: &str) {
        self.known.remove(workspace_id);
        self.deferred.remove(workspace_id);
        self.reopened.remove(workspace_id);
    }

    /// The connection ended: nothing the engine said holds any more. What the person dismissed
    /// stays dismissed until the application restarts.
    pub fn connection_lost(&mut self) {
        self.known.clear();
        self.announced = false;
    }

    /// The folder is not trusted: the engine said it is undecided or restricted, or has not yet
    /// said anything of a workspace that exists on an engine that keeps the state.
    pub fn is_restricted(&self, workspace_id: &str) -> bool {
        match self.known.get(workspace_id) {
            Some(trust) => *trust != ProjectTrust::Trusted,
            None => self.announced,
        }
    }

    /// Nobody has answered for this workspace's folder.
    fn is_undecided(&self, workspace_id: &str) -> bool {
        match self.known.get(workspace_id) {
            Some(trust) => *trust == ProjectTrust::Undecided,
            None => self.announced,
        }
    }

    /// The window asks about this workspace: the engine has no answer yet for its folder and the
    /// person has not put the question off, or the person asked to see the question again.
    pub fn needs_decision(&self, workspace_id: &str) -> bool {
        (self.is_undecided(workspace_id) && !self.deferred.contains(workspace_id))
            || (self.reopened.contains(workspace_id) && self.is_restricted(workspace_id))
    }

    /// Puts the question off until the application restarts.
    pub fn defer(&mut self, workspace_id: &str) {
        self.reopened.remove(workspace_id);
        self.deferred.insert(workspace_id.to_owned());
    }

    /// The person asks to see the question again, whatever became of it before.
    pub fn reopen(&mut self, workspace_id: &str) {
        self.deferred.remove(workspace_id);
        self.reopened.insert(workspace_id.to_owned());
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
            trusts.needs_decision("unknown"),
            "the engine keeps the state and has no entry: undecided"
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

    #[test]
    fn a_workspace_the_engine_announces_after_the_snapshot_is_asked_about() {
        let mut trusts = loaded();
        trusts.changed(vec!["new".into()], ProjectTrust::Undecided);
        assert!(trusts.needs_decision("new"));
        assert!(trusts.is_restricted("new"));
        trusts.changed(vec!["new".into()], ProjectTrust::Trusted);
        assert!(!trusts.needs_decision("new"));
        assert!(!trusts.is_restricted("new"));
    }

    #[test]
    fn a_workspace_with_no_entry_is_undecided_once_the_engine_keeps_the_state() {
        let mut trusts = Trusts::default();
        trusts.changed(Vec::new(), ProjectTrust::Trusted);
        assert!(trusts.needs_decision("no-entry"));
        assert!(trusts.is_restricted("no-entry"));
        trusts.defer("no-entry");
        assert!(!trusts.needs_decision("no-entry"));
    }

    #[test]
    fn an_engine_with_no_trust_section_asks_nothing() {
        let mut trusts = Trusts::default();
        trusts.adopt(Vec::new());
        assert!(!trusts.needs_decision("a"));
        assert!(!trusts.is_restricted("a"));
        trusts.connection_lost();
        assert!(!trusts.needs_decision("a"));
        assert!(!trusts.is_restricted("a"));
    }

    #[test]
    fn restricted_and_undecided_folders_are_marked_and_a_trusted_one_is_not() {
        let trusts = loaded();
        assert!(trusts.is_restricted("a"));
        assert!(!trusts.is_restricted("b"));
        assert!(trusts.is_restricted("c"));
    }

    #[test]
    fn reopening_asks_again_about_a_deferred_or_a_restricted_folder() {
        let mut trusts = loaded();
        trusts.defer("a");
        trusts.reopen("a");
        assert!(trusts.needs_decision("a"));
        assert!(!trusts.needs_decision("c"));
        trusts.reopen("c");
        assert!(trusts.needs_decision("c"));
        trusts.defer("c");
        assert!(!trusts.needs_decision("c"), "dismissed again");
        trusts.reopen("c");
        trusts.changed(vec!["c".into()], ProjectTrust::Restricted);
        assert!(!trusts.needs_decision("c"), "an answer ends the question");
        trusts.reopen("b");
        assert!(!trusts.needs_decision("b"), "a trusted folder is not asked");
    }
}
