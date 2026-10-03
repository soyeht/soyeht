//! The slash commands a harness advertises with ACP
//! `available_commands_update`. They stay with the agent checkpoint, so an
//! adopted process keeps them across an engine restart. Clients read them with
//! `pane.commands.list` and refresh on `pane.commands_changed`; `pane.updated`
//! stays small because a harness can list hundreds of skills.

use super::{Changes, state::State};
use crate::{
    core::Core,
    errors::{self, Result},
};
use serde_json::{Value, json};
use soyeht_protocol::{AgentCommand, PaneCommandsListResult};

pub(super) fn replace(
    pane: &str,
    state: &mut State,
    commands: Vec<AgentCommand>,
    changes: &mut Changes,
) {
    if state.commands == commands {
        return;
    }
    state.commands = commands;
    changes
        .events
        .push(("pane.commands_changed".into(), json!({"paneId":pane})));
}

impl Core {
    pub(crate) fn pane_commands_list(&self, params: &Value) -> Result<Value> {
        let pane = errors::param_str(params, "paneId")?;
        let items = self.agent_state(pane)?.commands;
        Ok(json!(PaneCommandsListResult {
            pane_id: pane.into(),
            items,
        }))
    }
}
