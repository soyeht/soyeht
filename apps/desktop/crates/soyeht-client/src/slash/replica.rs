//! What the replica holds for the "/" list: the skills of each workspace the panes are in and
//! the commands of each agent pane, when to ask for them, and the catalogue a pane's list is
//! built from.

use super::Catalog;
use crate::state::{Effect, State};
use serde_json::Value;
use soyeht_protocol::{
    AgentCommand, EventName, Pane, PaneCommandsChangedEvent, PaneMode, SkillScope,
    SkillsChangedEvent, SkillsListResult,
};

impl State {
    /// What the "/" list of an agent pane offers: the skills of its workspace (the user's
    /// alone until that listing arrives) and the commands its harness advertised.
    pub fn slash_catalog(&self, pane_id: &str) -> Catalog<'_> {
        let workspace = self.pane(pane_id).map_or("", |pane| &pane.workspace_id);
        let listing = self.skills.get(workspace).or_else(|| self.skills.get(""));
        Catalog::new(
            listing.map_or(&[][..], |listing| listing.items.as_slice()),
            self.commands.get(pane_id).map_or(&[][..], Vec::as_slice),
        )
    }

    pub(crate) fn skills_listed(
        &mut self,
        workspace_id: Option<String>,
        listing: SkillsListResult,
    ) {
        // The listing of a workspace that closed while it was on its way is not kept.
        if workspace_id
            .as_deref()
            .is_some_and(|id| self.workspace(id).is_none())
        {
            return;
        }
        self.skills
            .insert(workspace_id.unwrap_or_default(), listing);
    }

    pub(crate) fn commands_listed(&mut self, pane_id: String, items: Vec<AgentCommand>) {
        if self.agents.contains_key(&pane_id) {
            self.commands.insert(pane_id, items);
        }
    }

    /// The loads a new agent pane needs: its commands, and its workspace's skills when they
    /// were not listed yet.
    pub(crate) fn slash_sources_for(&self, pane: &Pane) -> Vec<Effect> {
        let mut effects = vec![Effect::LoadCommands(pane.pane_id.clone())];
        if !self.skills.contains_key(&pane.workspace_id) {
            effects.push(Effect::LoadSkills(Some(pane.workspace_id.clone())));
        }
        effects
    }

    /// After a snapshot, what may have changed while the connection was gone: the commands of
    /// every agent pane and the skills of every workspace that has one.
    pub(crate) fn adopt_slash_sources(&mut self) -> Vec<Effect> {
        self.commands
            .retain(|id, _| self.agents.contains_key(id.as_str()));
        let mut effects = Vec::new();
        let mut workspaces: Vec<&str> = Vec::new();
        for pane in self
            .panes
            .iter()
            .filter(|pane| pane.mode == PaneMode::Agent)
        {
            effects.push(Effect::LoadCommands(pane.pane_id.clone()));
            if !workspaces.contains(&pane.workspace_id.as_str()) {
                workspaces.push(&pane.workspace_id);
                effects.push(Effect::LoadSkills(Some(pane.workspace_id.clone())));
            }
        }
        effects
    }

    /// Events about the skills and about a harness's commands: whatever was listed is listed
    /// again. What was never listed is not asked for.
    pub(crate) fn slash_notification(
        &mut self,
        name: EventName,
        params: Value,
    ) -> serde_json::Result<Vec<Effect>> {
        Ok(match name {
            EventName::SkillsChanged => {
                let event = serde_json::from_value::<SkillsChangedEvent>(params)?;
                // A user skill is part of every listing; a workspace skill of its own.
                let mut keys: Vec<&String> = self
                    .skills
                    .keys()
                    .filter(|key| {
                        event.scope == SkillScope::User || event.workspace_id.as_ref() == Some(*key)
                    })
                    .collect();
                keys.sort();
                keys.into_iter()
                    .map(|key| Effect::LoadSkills(Some(key.clone()).filter(|key| !key.is_empty())))
                    .collect()
            }
            EventName::PaneCommandsChanged => {
                let event = serde_json::from_value::<PaneCommandsChangedEvent>(params)?;
                if self.commands.contains_key(&event.pane_id) {
                    vec![Effect::LoadCommands(event.pane_id)]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        })
    }
}
