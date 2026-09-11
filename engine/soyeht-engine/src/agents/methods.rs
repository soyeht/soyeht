use super::{Changes, now_ms, timestamp};
use crate::{
    core::Core,
    errors::{self, Result},
};
use serde_json::{Value, json};
use soyeht_acp::client;
use soyeht_protocol::{ErrorKind, PaneState, PromptOrigin};
use soyeht_store::agents;

fn queued(pane: &str, state: &super::state::State) -> (String, Value) {
    (
        "pane.queued".into(),
        json!({"paneId":pane,"n":state.queue.len(),
        "promptIds":state.queue.iter().map(|p| &p.prompt_id).collect::<Vec<_>>(),"draft":state.draft_until_ms > now_ms()}),
    )
}

impl Core {
    pub fn close_agent(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let pane = params["paneId"].as_str().expect("validated pane");
        let mut state = self.agent_state(pane)?;
        let mut saved = self.panes[pane].stored.clone();
        let instance = saved.instance_id.clone();
        let child_instances: Vec<_> = state
            .terminals
            .values()
            .filter_map(|t| t.instance_id.clone())
            .collect();
        let mut changes = Changes {
            clear_attention: true,
            retire_instance: instance.clone().zip(saved.supervisor_id.clone()),
            clear_replay: true,
            ..Changes::default()
        };
        for question in agents::open_requests(&self.store, pane).map_err(errors::storage)? {
            super::questions::decision(
                &mut state,
                &question,
                None,
                None,
                "system",
                false,
                &mut changes,
            )?;
        }
        changes.outgoing.clear();
        if state.phase != super::state::Phase::Loading {
            super::transcript::complete(&self.store, pane, &mut state, &mut changes)?;
        }
        state.open_messages.clear();
        if let Some(user) = state.operation_user.take() {
            changes.completed_user = Some(super::pending::Completion {
                user,
                result: Err(errors::error(
                    ErrorKind::HarnessUnavailable,
                    "Agent pane was closed before confirmation",
                )),
            });
        }
        state.operation = None;
        state.pending_policy = None;
        if let Some(prompt) = state.active.take() {
            changes.events.push((
                "pane.turn_ended".into(),
                json!({"paneId":pane,"promptId":prompt.prompt_id,"stopReason":"cancelled"}),
            ));
        }
        state.phase = super::state::Phase::Faulted;
        saved.closed = true;
        saved.pane.state = PaneState::Dead;
        saved.pane.phrase = "Agent pane closed".into();
        changes.events.push((
            "pane.closed".into(),
            json!({"paneId":pane,"workspaceId":saved.pane.workspace_id}),
        ));
        let result = self
            .commit_agent(
                saved,
                state,
                changes,
                Some((connection, "pane.close", params)),
                json!({"paneId":pane}),
            )?
            .result;
        if let Some(instance_id) = instance {
            // Closure intent is durable; reconciliation completes it if this
            // supervisor request fails or the engine exits before sending it.
            let _ = self.pty_request(soyeht_ptyd::wire::Request::Close { instance_id });
        }
        for instance_id in child_instances {
            let _ = self.pty_request(soyeht_ptyd::wire::Request::Close { instance_id });
        }
        Ok(result)
    }

    pub fn agent_method(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        let pane = params["paneId"].as_str().expect("validated pane");
        let mut state = self.agent_state(pane)?;
        let mut saved = self.panes[pane].stored.clone();
        let mut changes = Changes::default();
        let result = match method {
            "pane.queue.list" => {
                return Ok(json!(state.queue_page(
                    params["cursor"].as_str(),
                    params["limit"].as_u64().unwrap_or(64) as usize,
                    now_ms()
                )?));
            }
            "pane.transcript" => {
                let before = params["before"]
                    .as_str()
                    .map(|s| s.parse::<u64>().expect("validated cursor"));
                return Ok(json!(
                    agents::transcript(
                        &self.store,
                        pane,
                        before,
                        params["limit"].as_u64().unwrap_or(100) as usize
                    )
                    .map_err(errors::storage)?
                ));
            }
            "pane.prompt" => {
                changes.clear_attention = true;
                if params["attachments"]
                    .as_array()
                    .is_some_and(|items| !items.is_empty())
                {
                    return Err(errors::error(
                        ErrorKind::InvalidParams,
                        "Attachment upload is not available in this slice",
                    ));
                }
                let origin = params
                    .get("origin")
                    .map(|v| serde_json::from_value(v.clone()).expect("validated origin"))
                    .unwrap_or(PromptOrigin::Human);
                let prompt = state.enqueue(
                    params["text"].as_str().expect("validated text").into(),
                    origin,
                    timestamp(),
                )?;
                changes.events.push(queued(pane, &state));
                json!({"promptId":prompt.prompt_id,"queued":true})
            }
            "pane.queue.remove" => {
                let prompt = params["promptId"].as_str().expect("validated prompt");
                let removed = state.remove(prompt)?;
                if removed {
                    changes.events.push(queued(pane, &state));
                }
                json!({"promptId":prompt,"removed":removed})
            }
            "pane.draft" => {
                let active = params["active"].as_bool().expect("validated draft");
                state.draft_until_ms = if active { now_ms() + 10_000 } else { 0 };
                changes.events.push(queued(pane, &state));
                json!({"active":active})
            }
            "pane.cancel" => {
                if !state.queue_paused {
                    state.queue_paused = true;
                    changes.events.push((
                        "pane.queue.paused".into(),
                        json!({"paneId":pane,"paused":true,"reason":"interrupted"}),
                    ));
                }
                let mut result = json!({"paneId":pane,"cancelling":state.active.is_some()});
                if let Some(prompt) = &state.active {
                    result["promptId"] = json!(prompt.prompt_id);
                    if state.cancel_at_ms.is_none() {
                        state.cancel_at_ms = Some(now_ms());
                        saved.pane.state = PaneState::Cancelling;
                        saved.pane.phrase = "Cancelling agent turn".into();
                        changes.outgoing.push(client::cancel(
                            state.session_id.as_deref().expect("active session"),
                        ));
                        for request in
                            agents::open_requests(&self.store, pane).map_err(errors::storage)?
                        {
                            super::questions::decision(
                                &mut state,
                                &request,
                                None,
                                None,
                                "system",
                                false,
                                &mut changes,
                            )?;
                        }
                    }
                }
                result
            }
            "pane.queue.resume" => {
                if state.queue_paused {
                    state.queue_paused = false;
                    changes.events.push((
                        "pane.queue.resumed".into(),
                        json!({"paneId":pane,"paused":false}),
                    ));
                }
                json!({"paneId":pane,"paused":false})
            }
            "pane.config.set" => {
                let config_id = params["configId"].as_str().expect("validated config ID");
                let value = serde_json::from_value(params["value"].clone())
                    .expect("validated config value");
                let user = super::pending::UserOperation {
                    scope: self.clients[connection].scope.clone(),
                    key: params["idempotencyKey"]
                        .as_str()
                        .expect("validated key")
                        .into(),
                };
                let operation_id = super::configuration::begin(
                    &mut state,
                    pane,
                    config_id,
                    value,
                    Some(user),
                    soyeht_protocol::ConfigOperationKind::Config,
                    &mut changes,
                )?;
                json!({"_nativePending":operation_id})
            }
            _ => {
                return Err(errors::error(
                    ErrorKind::InvalidParams,
                    "Agent method unavailable",
                ));
            }
        };
        Ok(self
            .commit_agent(
                saved,
                state,
                changes,
                Some((connection, method, params)),
                result,
            )?
            .result)
    }
}
