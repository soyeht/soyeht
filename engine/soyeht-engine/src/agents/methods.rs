use super::Changes;
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_acp::client;
use soyeht_protocol::{ErrorKind, PaneState, PromptOrigin};

fn queued(pane: &str, state: &super::state::State, now_ms: i64) -> (String, Value) {
    (
        "pane.queued".into(),
        json!({"paneId":pane,"n":state.queue.len(),
        "promptIds":state.queue.iter().map(|p| &p.prompt_id).collect::<Vec<_>>(),"draft":state.draft_until_ms > now_ms}),
    )
}

fn read_agent_state(
    store: &impl StorePort,
    method: &str,
    params: &Value,
    pane: &str,
    state: &super::state::State,
    now_ms: i64,
) -> Result<Option<Value>> {
    if method == "pane.queue.list" {
        return Ok(Some(json!(state.queue_page(
            params["cursor"].as_str(),
            params["limit"].as_u64().unwrap_or(64) as usize,
            now_ms
        )?)));
    }
    if method == "pane.transcript" {
        let before = params["before"]
            .as_str()
            .map(|value| value.parse::<u64>().expect("validated cursor"));
        return store
            .agent_transcript(
                pane,
                before,
                params["limit"].as_u64().unwrap_or(100) as usize,
            )
            .map(|value| json!(value))
            .map(Some)
            .map_err(errors::storage);
    }
    Ok(None)
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
        for question in self
            .store
            .agent_open_requests(pane)
            .map_err(errors::storage)?
        {
            super::questions::decision(
                &mut state,
                &question,
                super::questions::Decision {
                    option: None,
                    fields: None,
                    by: "system",
                    expired: false,
                    timestamp: &self.timestamp(),
                },
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
        let now_ms = self.now_ms();
        let timestamp = self.timestamp();
        if let Some(result) = read_agent_state(&self.store, method, params, pane, &state, now_ms)? {
            return Ok(result);
        }
        let result = match method {
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
                    timestamp.clone(),
                )?;
                changes.events.push(queued(pane, &state, now_ms));
                json!({"promptId":prompt.prompt_id,"queued":true})
            }
            "pane.queue.remove" => {
                let prompt = params["promptId"].as_str().expect("validated prompt");
                let removed = state.remove(prompt)?;
                if removed {
                    changes.events.push(queued(pane, &state, now_ms));
                }
                json!({"promptId":prompt,"removed":removed})
            }
            "pane.draft" => {
                let active = params["active"].as_bool().expect("validated draft");
                state.draft_until_ms = if active { now_ms + 10_000 } else { 0 };
                changes.events.push(queued(pane, &state, now_ms));
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
                        state.cancel_at_ms = Some(now_ms);
                        saved.pane.state = PaneState::Cancelling;
                        saved.pane.phrase = "Cancelling agent turn".into();
                        changes.outgoing.push(client::cancel(
                            state.session_id.as_deref().expect("active session"),
                        ));
                        for request in self
                            .store
                            .agent_open_requests(pane)
                            .map_err(errors::storage)?
                        {
                            super::questions::decision(
                                &mut state,
                                &request,
                                super::questions::Decision {
                                    option: None,
                                    fields: None,
                                    by: "system",
                                    expired: false,
                                    timestamp: &timestamp,
                                },
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
                    super::configuration::ChangeRequest {
                        config: config_id,
                        value,
                        user: Some(user),
                        kind: soyeht_protocol::ConfigOperationKind::Config,
                        started_at: timestamp,
                        now_ms,
                    },
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
