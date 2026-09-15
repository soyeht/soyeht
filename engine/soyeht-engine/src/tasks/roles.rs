//! Durable agent role assignment.

use super::api::parameters;
use crate::{
    agents::{Changes, state::Phase},
    core::Core,
    errors::{self, Result},
    outbox::CloseReason,
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::{AgentPaneRole, PaneRoleSetParams, PaneState, PromptOrigin, Role};
use soyeht_ptyd::wire::Request;

const LEAD_PROMPT: &str = "Coordinate; do not implement. Create small, self-contained tasks with how to verify. Never depend on another task's memory. Dispatch only when no free pane is available to claim.";

impl Core {
    pub(crate) fn pane_role_set(&mut self, connection: &str, raw: &Value) -> Result<Value> {
        let params: PaneRoleSetParams = parameters(raw)?;
        let actor = self.task_actor(connection)?;
        if !matches!(actor.role, Role::HumanLocal | Role::HumanDevice) {
            return Err(errors::forbidden_role(
                "Only a person may assign an agent role",
            ));
        }
        if self
            .store
            .active_task_for_pane(&params.pane_id)
            .map_err(errors::storage)?
            .is_some()
        {
            return Err(errors::pane_busy(
                "An agent role cannot change while the pane owns a task",
            ));
        }
        let mut state = self.agent_state(&params.pane_id)?;
        if state.harness_switch.is_some() {
            return Err(errors::pane_busy("Harness switch is still pending"));
        }
        let saved = self.panes[&params.pane_id].stored.clone();
        if saved.pane.state != PaneState::Idle
            || state.phase != Phase::Ready
            || state.active.is_some()
            || !self
                .store
                .agent_open_requests(&params.pane_id)
                .map_err(errors::storage)?
                .is_empty()
        {
            return Err(errors::pane_busy(
                "An agent role can change only while the pane is idle",
            ));
        }
        let role = match params.role {
            AgentPaneRole::Worker => Role::Agent,
            AgentPaneRole::Chief => Role::Chief,
        };
        let changed = state.mcp_role != role;
        if changed && role == Role::Chief {
            state.enqueue(
                LEAD_PROMPT.into(),
                PromptOrigin::Automatic,
                self.timestamp(),
            )?;
        }
        state.mcp_role = role;
        let result = json!({"paneId":params.pane_id,"role":params.role});
        let mutation = self
            .commit_agent(
                saved,
                state,
                Changes::default(),
                Some((connection, "pane.role.set", raw)),
                result,
            )?
            .result;
        if changed {
            self.restart_after_role_change(&params.pane_id);
        }
        Ok(mutation)
    }

    fn restart_after_role_change(&mut self, pane: &str) {
        for client in self.clients.values().filter(|client| {
            client
                .actor
                .as_ref()
                .and_then(|actor| actor.pane_id.as_deref())
                == Some(pane)
        }) {
            client.outbox.close(CloseReason::RoleChanged);
        }
        let instance = self
            .panes
            .get(pane)
            .and_then(|saved| saved.stored.instance_id.clone());
        if instance
            .is_some_and(|instance_id| self.pty_request(Request::Close { instance_id }).is_err())
            || self.recover_missing_agent(pane).is_err()
        {
            self.ptyd = None;
        }
    }
}
