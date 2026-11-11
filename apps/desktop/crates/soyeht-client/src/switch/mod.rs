//! Switching an agent pane to another harness: what the pane says about a switch, and when one
//! may start. The rules are the Mac's `HarnessSwitchModel`.

use soyeht_protocol::{AgentSession, HarnessInfo};

mod draft;

pub use draft::Draft;

pub use soyeht_protocol::client_policy::{
    HARNESS_SWITCH_HANDOVER as HANDOVER_EXPLANATION, HARNESS_SWITCH_TITLE as TITLE,
};

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

/// Whether a pane running `running` may be switched to `target`: the engine lists the harness as
/// able to start, and it is not the one the pane already runs.
pub fn startable(harnesses: &[HarnessInfo], running: Option<&str>, target: &str) -> bool {
    Some(target) != running
        && harnesses
            .iter()
            .any(|harness| harness.id == target && harness.available)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{agent_session, cases, text};
    use serde_json::json;

    /// The cases of `contracts/vectors/harness-switch.json`, which the Mac client passes too.
    #[test]
    fn a_switch_follows_the_shared_vector() {
        for case in cases("harness-switch", "canSwitch") {
            let agent = match text(&case["agent"]) {
                "none" => None,
                "idle" => Some(agent_session(json!({}))),
                "configuring" => Some(agent_session(json!({"operation": {
                    "operationId": "o", "configId": "model", "value": "m",
                    "startedAt": "2026-10-01T00:00:00Z",
                }}))),
                other => panic!("unknown agent {other}"),
            };
            let view = case["switch"].as_str().map(|phase| HarnessSwitchView {
                from: "a".into(),
                to: "b".into(),
                phase: match phase {
                    "switching" => Phase::Switching,
                    "restarted" => Phase::Restarted,
                    "failed" => Phase::Failed,
                    other => panic!("unknown switch {other}"),
                },
                reason: None,
            });
            assert_eq!(
                can_switch(
                    case["connected"].as_bool().unwrap(),
                    agent.as_ref(),
                    view.as_ref()
                ),
                case["canSwitch"].as_bool().unwrap(),
                "{}",
                text(&case["name"])
            );
        }
    }

    #[test]
    fn only_a_listed_harness_that_can_start_and_is_not_running_is_startable() {
        let harness = crate::testing::harness;
        let listed = [harness("a", true), harness("b", true), harness("c", false)];
        assert!(startable(&listed, Some("a"), "b"));
        assert!(startable(&listed, None, "a"));
        assert!(!startable(&listed, Some("a"), "a"), "already running");
        assert!(!startable(&listed, Some("a"), "c"), "cannot start");
        assert!(!startable(&listed, Some("a"), "unknown"), "not listed");
    }
}
