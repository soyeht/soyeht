//! Switching an agent pane to another harness: what the pane says about a switch, and when one
//! may start. The rules are the Mac's `HarnessSwitchModel`.

use soyeht_protocol::AgentSession;

pub const TITLE: &str = "Switch agent";
pub const HANDOVER_EXPLANATION: &str = "This starts a new agent session. Recent messages and pending prompts are handed over as historical data.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Switching,
    Restarted,
    Failed,
}

/// A pane's last switch, as the engine's events told it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessSwitchView {
    pub from: String,
    pub to: String,
    pub phase: Phase,
    pub reason: Option<String>,
}

impl HarnessSwitchView {
    pub fn is_switching(view: Option<&Self>) -> bool {
        view.is_some_and(|view| view.phase == Phase::Switching)
    }
}

/// A pane can switch while connected, with no configuration change in flight and no switch
/// already running.
pub fn can_switch(
    connected: bool,
    agent: Option<&AgentSession>,
    view: Option<&HarnessSwitchView>,
) -> bool {
    connected
        && agent.is_some_and(|agent| agent.operation.is_none())
        && !HarnessSwitchView::is_switching(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::agent_session;
    use serde_json::json;

    fn agent(operation: bool) -> AgentSession {
        agent_session(if operation {
            json!({"operation": {
                "operationId": "o", "configId": "model", "value": "m",
                "startedAt": "2026-10-01T00:00:00Z",
            }})
        } else {
            json!({})
        })
    }
    fn view(phase: Phase) -> HarnessSwitchView {
        HarnessSwitchView {
            from: "a".into(),
            to: "b".into(),
            phase,
            reason: None,
        }
    }

    #[test]
    fn a_switch_is_blocked_while_disconnected_configuring_or_already_switching() {
        let idle = agent(false);
        assert!(can_switch(true, Some(&idle), None));
        assert!(can_switch(true, Some(&idle), Some(&view(Phase::Restarted))));
        assert!(can_switch(true, Some(&idle), Some(&view(Phase::Failed))));
        assert!(!can_switch(false, Some(&idle), None));
        assert!(!can_switch(true, Some(&agent(true)), None));
        assert!(!can_switch(
            true,
            Some(&idle),
            Some(&view(Phase::Switching))
        ));
        assert!(!can_switch(true, None, None));
    }
}
