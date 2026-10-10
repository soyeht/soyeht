//! Session-scoped harness hooks for experiment ground-truth labels.
//!
//! The capability closes the convenient accidental-write path. It is not a
//! security boundary against another process running as the same OS user: the
//! configured hook arguments remain discoverable to a determined local peer.

use super::state::State;
use crate::{
    core::Core,
    errors::{self, Result},
};
use serde::Deserialize;
use serde_json::{Value, json};
use soyeht_protocol::{PaneState, Role};
use std::path::Path;

pub(crate) const CLAUDE_HARNESS_ID: &str = "registry.claude-acp";
const HOOK_CLIENT: &str = "soyeht-hook";
const HOOK_TIMEOUT_SECONDS: u64 = 5;

#[derive(Debug, Clone, Copy, Deserialize)]
enum HookEvent {
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit,
    #[serde(rename = "PermissionRequest")]
    PermissionRequest,
    #[serde(rename = "PostToolUse")]
    PostToolUse,
    #[serde(rename = "PostToolUseFailure")]
    PostToolUseFailure,
    #[serde(rename = "Stop")]
    Stop,
}

impl HookEvent {
    fn state(self) -> PaneState {
        match self {
            Self::UserPromptSubmit | Self::PostToolUse | Self::PostToolUseFailure => {
                PaneState::Working
            }
            Self::PermissionRequest => PaneState::WaitingForYou,
            Self::Stop => PaneState::Idle,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct HookLabelParams {
    event: HookEvent,
    capability: String,
    #[serde(rename = "idempotencyKey")]
    _idempotency_key: String,
}

pub(crate) fn claude_hook_parameters(
    data_directory: &Path,
    state: &State,
    mut params: Value,
) -> Result<Value> {
    if state.launch.id != CLAUDE_HARNESS_ID {
        return Ok(params);
    }
    params["_meta"] = json!({
        "claudeCode":{
            "options":{
                "settings":terminal_hook_settings(data_directory, &state.hook_binding)?
            }
        }
    });
    Ok(params)
}

pub(crate) fn terminal_hook_settings(
    data_directory: &Path,
    binding: &super::state::Binding,
) -> Result<Value> {
    let executable = super::mcp::sibling_executable("soyeht")?;
    let handler = json!({
        "type":"command",
        "command":path_text(&executable)?,
        "args":[
            "--data-dir",
            path_text(data_directory)?,
            "__experiment-hook",
            binding.expose()
        ],
        "timeout":HOOK_TIMEOUT_SECONDS
    });
    let group = |handler: &Value| json!([{"hooks":[handler]}]);
    Ok(json!({
        "hooks":{
            "UserPromptSubmit":group(&handler),
            "PermissionRequest":group(&handler),
            "PostToolUse":group(&handler),
            "PostToolUseFailure":group(&handler),
            "Stop":group(&handler)
        }
    }))
}

fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| errors::internal("Hook path must be UTF-8"))
}

impl Core {
    pub(crate) fn record_hook_event(&mut self, connection: &str, raw: &Value) -> Result<Value> {
        let params: HookLabelParams = serde_json::from_value(raw.clone())
            .map_err(|_| errors::invalid("Invalid experiment hook parameters"))?;
        // An exact transport retry may outlive this native session's capability.
        // Return its durable receipt before resolving the current capability: the
        // original label remains attributed to its original generation, and the
        // retry never records a second label against the replacement session.
        if let Some(result) = self.repeated_mutation(connection, "pane.experiment.hook", raw)? {
            return Ok(result);
        }
        let client = self
            .clients
            .get(connection)
            .filter(|client| client.local)
            .and_then(|client| client.actor.as_ref())
            .filter(|actor| actor.role == Role::HumanLocal && actor.client == HOOK_CLIENT)
            .ok_or_else(|| {
                errors::forbidden_role("Experiment hook requires its local hook channel")
            })?;
        debug_assert_eq!(client.client, HOOK_CLIENT);

        let mut matched = self
            .agents
            .iter()
            .filter(|(_, running)| running.state.hook_binding.matches(&params.capability))
            .map(|(pane_id, running)| {
                (
                    pane_id.clone(),
                    running.state.launch.id.clone(),
                    running.state.hook_binding.sha256(),
                    false,
                )
            })
            .chain(
                self.terminal_harnesses
                    .iter()
                    .filter(|(_, session)| session.hook_binding.matches(&params.capability))
                    .map(|(pane_id, session)| {
                        (
                            pane_id.clone(),
                            session.harness.id().unwrap_or("").to_owned(),
                            session.hook_binding.sha256(),
                            true,
                        )
                    }),
            );
        let Some((pane_id, harness_id, credential_sha256, terminal)) = matched.next() else {
            return Err(errors::unauthorized("Unknown experiment hook capability"));
        };
        if matched.next().is_some() {
            return Err(errors::internal("Experiment hook capability is not unique"));
        }
        if harness_id != CLAUDE_HARNESS_ID {
            return Err(errors::unauthorized(
                "Experiment hook capability has no supported harness",
            ));
        }
        let pane = self
            .panes
            .get(&pane_id)
            .filter(|pane| !pane.stored.closed)
            .ok_or_else(|| errors::unauthorized("Experiment hook session is closed"))?;
        if terminal
            && self.terminal_harnesses.get(&pane_id).is_none_or(|session| {
                session.spawn.instance_key != pane.stored.instance_key
                    || session.spawn.generation != pane.stored.pane.generation
                    || pane.stored.instance_id.is_none()
            })
        {
            return Err(errors::unauthorized(
                "Experiment hook terminal session is no longer active",
            ));
        }
        let active_capture = pane
            .stored
            .pane
            .experiment_capture
            .as_ref()
            .is_some_and(|capture| capture.on && capture.failure.is_none())
            && if terminal {
                self.terminal_captures.contains_key(&pane_id)
            } else {
                self.agent_captures.contains_key(&pane_id)
            };
        if !active_capture {
            // Hooks are installed when the session opens so later consent can
            // begin recording without restarting the conversation. An inactive
            // capture is therefore an authenticated no-op, never a disk write.
            let result = json!({"recorded":false});
            return Ok(self
                .mutate(connection, "pane.experiment.hook", raw, |_| {
                    Ok((result, Vec::new()))
                })?
                .result);
        }
        if terminal {
            self.record_terminal_hook_label(&pane_id, params.event.state(), credential_sha256)?;
        } else {
            self.record_agent_hook_label(&pane_id, params.event.state(), credential_sha256)?;
        }
        let result = json!({"recorded":true});
        Ok(self
            .mutate(connection, "pane.experiment.hook", raw, |_| {
                Ok((result, Vec::new()))
            })?
            .result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::HarnessLaunch;

    fn state(id: &str) -> State {
        State::new(
            HarnessLaunch {
                id: id.into(),
                name: "Harness".into(),
                command: "/bin/false".into(),
                args: Vec::new(),
                version: "1".into(),
                environment: None,
            },
            "digest".into(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn params_reject_claimed_provenance_instead_of_ignoring_it() {
        assert!(
            serde_json::from_value::<HookLabelParams>(json!({
                "event":"Stop",
                "capability":"secret",
                "idempotencyKey":"hook-1",
                "source":"hook"
            }))
            .is_err()
        );
    }

    #[test]
    fn claude_session_gets_exec_hooks_but_other_harnesses_do_not() {
        let claude = state(CLAUDE_HARNESS_ID);
        let params =
            claude_hook_parameters(Path::new("/profile"), &claude, json!({"cwd":"/tmp"})).unwrap();
        let hooks = &params["_meta"]["claudeCode"]["options"]["settings"]["hooks"];
        assert_eq!(hooks.as_object().unwrap().len(), 5);
        let handler = &hooks["Stop"][0]["hooks"][0];
        assert_eq!(handler["type"], "command");
        assert_eq!(handler["args"][3], claude.hook_binding.expose());
        assert!(params.to_string().contains(claude.hook_binding.expose()));
        assert!(
            !json!({"role":"user","content":"prompt"})
                .to_string()
                .contains(claude.hook_binding.expose())
        );

        let other = state("registry.codex-acp");
        let plain =
            claude_hook_parameters(Path::new("/profile"), &other, json!({"cwd":"/tmp"})).unwrap();
        assert!(plain.get("_meta").is_none());
    }

    #[test]
    fn hook_events_map_to_states_inside_the_engine() {
        for (event, expected) in [
            ("UserPromptSubmit", PaneState::Working),
            ("PermissionRequest", PaneState::WaitingForYou),
            ("PostToolUse", PaneState::Working),
            ("PostToolUseFailure", PaneState::Working),
            ("Stop", PaneState::Idle),
        ] {
            let parsed: HookLabelParams = serde_json::from_value(json!({
                "event":event,
                "capability":"secret",
                "idempotencyKey":"hook-1"
            }))
            .unwrap();
            assert_eq!(parsed.event.state(), expected);
        }
    }
}
