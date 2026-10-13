use super::Changes;
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_acp::client;
use soyeht_protocol::{
    AgentMessageSource, PaneMode, PaneState, PromptOrigin, event, limits, method,
};
use soyeht_store::agents;

pub(crate) fn handles(method: &str) -> bool {
    matches!(
        method,
        method::PANE_PROMPT
            | method::PANE_QUEUE_LIST
            | method::PANE_QUEUE_REMOVE
            | method::PANE_QUEUE_RESUME
            | method::PANE_CANCEL
            | method::PANE_CONFIG_SET
            | method::PANE_TRANSCRIPT
            | method::PANE_DRAFT
    )
}

pub(crate) fn queued(pane: &str, state: &super::state::State, now_ms: i64) -> (String, Value) {
    (
        event::PANE_QUEUED.into(),
        json!({"paneId":pane,"n":state.queue.len(),
        "promptIds":state.queue.iter().map(|p| &p.prompt_id).collect::<Vec<_>>(),"draft":state.draft_until_ms > now_ms}),
    )
}

fn message_retry_after(times: &[i64], now_ms: i64) -> Option<u32> {
    if times.len() < limits::AGENT_MESSAGES_PER_WINDOW {
        return None;
    }
    let window_ms = (limits::AGENT_MESSAGE_WINDOW_SEC as i64) * 1000;
    let wait_ms = times[0]
        .saturating_add(window_ms)
        .saturating_sub(now_ms)
        .max(1);
    Some((wait_ms.saturating_add(999) / 1000).min(u32::MAX as i64) as u32)
}

fn read_agent_state(
    store: &impl StorePort,
    method: &str,
    params: &Value,
    pane: &str,
    state: &super::state::State,
    now_ms: i64,
) -> Result<Option<Value>> {
    if method == method::PANE_QUEUE_LIST {
        return Ok(Some(json!(state.queue_page(
            params["cursor"].as_str(),
            params["limit"].as_u64().unwrap_or(64) as usize,
            now_ms
        )?)));
    }
    if method == method::PANE_TRANSCRIPT {
        let before = params["before"]
            .as_str()
            .map(str::parse::<u64>)
            .transpose()
            .map_err(|_| errors::invalid("Transcript cursor is invalid"))?;
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

fn prompt_origin(params: &Value) -> Result<PromptOrigin> {
    params
        .get("origin")
        .map(|value| {
            serde_json::from_value(value.clone())
                .map_err(|_| errors::invalid("Prompt origin is invalid"))
        })
        .transpose()
        .map(|origin| origin.unwrap_or(PromptOrigin::Human))
}

pub(crate) fn cancel(
    store: &impl StorePort,
    pane: &str,
    state: &mut super::state::State,
    saved: &mut crate::model::StoredPane,
    changes: &mut Changes,
    now_ms: i64,
    timestamp: &str,
) -> Result<Value> {
    if !state.queue_paused {
        state.queue_paused = true;
        changes.events.push((
            event::PANE_QUEUE_PAUSED.into(),
            json!({"paneId":pane,"paused":true,"reason":"interrupted"}),
        ));
    }
    let mut result = json!({"paneId":pane,"cancelling":state.active.is_some()});
    let Some(prompt) = &state.active else {
        return Ok(result);
    };
    result["promptId"] = json!(prompt.prompt_id);
    if state.cancel_at_ms.is_some() {
        return Ok(result);
    }
    state.cancel_at_ms = Some(now_ms);
    saved.pane.state = PaneState::Cancelling;
    saved.pane.phrase = "Cancelling agent turn".into();
    changes.outgoing.push(client::cancel(errors::invariant(
        state.session_id.as_deref(),
        "Active prompt has no native session",
    )?));
    for request in store.agent_open_requests(pane).map_err(errors::storage)? {
        super::questions::decision(
            state,
            &request,
            super::questions::Decision {
                option: None,
                fields: None,
                by: "system",
                expired: false,
                timestamp,
            },
            changes,
        )?;
    }
    Ok(result)
}

impl Core {
    pub fn agent_peer_method(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        let sender_id = self
            .clients
            .get(connection)
            .and_then(|client| client.actor.as_ref())
            .and_then(|actor| actor.pane_id.as_deref())
            .ok_or_else(|| errors::forbidden_role("Agent binding required"))?
            .to_owned();
        let sender = self
            .panes
            .get(&sender_id)
            .filter(|pane| !pane.stored.closed && pane.stored.pane.mode == PaneMode::Agent)
            .ok_or_else(|| errors::not_found("Sender agent pane not found"))?
            .stored
            .pane
            .clone();
        if method == method::AGENT_LIST {
            let items: Vec<AgentMessageSource> = self
                .panes
                .values()
                .filter(|pane| {
                    !pane.stored.closed
                        && pane.stored.pane.workspace_id == sender.workspace_id
                        && pane.stored.pane.mode == PaneMode::Agent
                        && pane.stored.pane.pane_id != sender_id
                        && self.agents.contains_key(&pane.stored.pane.pane_id)
                })
                .filter_map(|pane| {
                    self.agents
                        .get(&pane.stored.pane.pane_id)
                        .map(|running| AgentMessageSource {
                            pane_id: pane.stored.pane.pane_id.clone(),
                            name: pane.stored.pane.name.clone(),
                            harness: running.state.launch.name.clone(),
                        })
                })
                .collect();
            return Ok(json!({"items":items}));
        }
        let target_id = errors::param_str(params, "targetPaneId")?;
        let target = self
            .panes
            .get(target_id)
            .filter(|pane| {
                !pane.stored.closed
                    && pane.stored.pane.workspace_id == sender.workspace_id
                    && pane.stored.pane.mode == PaneMode::Agent
                    && pane.stored.pane.pane_id != sender_id
            })
            .ok_or_else(|| errors::not_found("Target agent pane not found in this workspace"))?
            .stored
            .clone();
        let mut state = self.agent_state(target_id)?;
        if state.harness_switch.is_some() {
            return Err(errors::pane_busy("Target agent is switching harness"));
        }
        let now_ms = self.now_ms();
        let window_ms = (limits::AGENT_MESSAGE_WINDOW_SEC as i64) * 1000;
        let since_ms = now_ms.saturating_sub(window_ms);
        let times = agents::message_times(&self.store, &sender_id, target_id, since_ms)
            .map_err(errors::storage)?;
        if let Some(retry_after_sec) = message_retry_after(&times, now_ms) {
            return Err(errors::rate_limited(
                "Agent message rate limit reached for this recipient",
                retry_after_sec,
            ));
        }
        let text = errors::param_str(params, "text")?;
        let harness = self
            .agents
            .get(&sender_id)
            .map(|running| running.state.launch.name.clone())
            .ok_or_else(|| errors::invalid("Sender ACP session required"))?;
        let source = AgentMessageSource {
            pane_id: sender_id.clone(),
            name: sender.name,
            harness,
        };
        let prompt = state.enqueue(text.into(), PromptOrigin::Automatic, self.timestamp())?;
        let item = state
            .queue
            .iter_mut()
            .find(|item| item.prompt_id == prompt.prompt_id)
            .ok_or_else(|| errors::internal("Accepted agent message disappeared from queue"))?;
        item.agent_message = Some(source);
        let mut changes = Changes {
            message_rate: Some((sender_id, target_id.to_owned(), now_ms, since_ms)),
            ..Changes::default()
        };
        changes.events.push(queued(target_id, &state, now_ms));
        let result = json!({"promptId":prompt.prompt_id,"queued":true});
        Ok(self
            .commit_agent(
                target,
                state,
                changes,
                Some((connection, method, params)),
                result,
            )?
            .result)
    }
    pub fn close_agent(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let pane = errors::param_str(params, "paneId")?;
        if self.agent_state(pane)?.harness_switch.is_some() {
            return Err(errors::pane_busy("Harness switch is still pending"));
        }
        let task_owner_loss = self.task_owner_loss(pane, "returned: the pane closed")?;
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
            task_owner_loss,
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
        super::transcript::close(&self.store, pane, &mut state, &mut changes)?;
        if let Some(user) = state.operation_user.take() {
            changes.completed_user = Some(super::pending::Completion {
                user,
                result: Err(errors::wire(errors::harness_unavailable(
                    "Agent pane was closed before confirmation",
                ))),
            });
        }
        state.operation = None;
        if let Some(prompt) = state.active.take() {
            changes.events.push((
                event::PANE_TURN_ENDED.into(),
                json!({"paneId":pane,"promptId":prompt.prompt_id,"stopReason":"cancelled"}),
            ));
        }
        state.phase = super::state::Phase::Faulted;
        saved.pane.state = PaneState::Dead;
        saved.pane.phrase = "Agent pane closed".into();
        changes.workspace_layout = self.prepare_pane_close(&mut saved, &mut changes.events)?;
        let result = self
            .commit_agent(
                saved,
                state,
                changes,
                Some((connection, method::PANE_CLOSE, params)),
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
        prompt_attachments: Vec<super::attachments::PreparedAttachment>,
    ) -> Result<Value> {
        let pane = errors::param_str(params, "paneId")?;
        let mut state = self.agent_state(pane)?;
        let mut saved = self.panes[pane].stored.clone();
        let mut changes = Changes::default();
        let now_ms = self.now_ms();
        let timestamp = self.timestamp();
        if let Some(result) = read_agent_state(&self.store, method, params, pane, &state, now_ms)? {
            return Ok(result);
        }
        if state.harness_switch.is_some() {
            return Err(errors::pane_busy("Harness switch is still pending"));
        }
        let result = match method {
            method::PANE_PROMPT => {
                changes.clear_attention = true;
                let origin = prompt_origin(params)?;
                if origin == PromptOrigin::Human {
                    // Sending the composer's text ends its draft in the same
                    // commit, so held agent messages follow it, never precede it.
                    state.draft_until_ms = 0;
                }
                let attachments = (!prompt_attachments.is_empty()).then(|| {
                    prompt_attachments
                        .iter()
                        .map(|prepared| prepared.attachment.clone())
                        .collect()
                });
                let prompt = state.enqueue_with_attachments(
                    errors::param_str(params, "text")?.into(),
                    origin,
                    timestamp.clone(),
                    attachments,
                )?;
                // Staged bytes are written only once the queue accepted the
                // prompt, and before the commit that names their paths.
                for prepared in &prompt_attachments {
                    prepared.publish()?;
                }
                changes.events.push(queued(pane, &state, now_ms));
                json!({"promptId":prompt.prompt_id,"queued":true})
            }
            method::PANE_QUEUE_REMOVE => {
                let prompt = errors::param_str(params, "promptId")?;
                let removed = state.remove(prompt)?;
                if removed {
                    changes.events.push(queued(pane, &state, now_ms));
                }
                json!({"promptId":prompt,"removed":removed})
            }
            method::PANE_DRAFT => {
                let active = errors::param_bool(params, "active")?;
                state.draft_until_ms = if active { now_ms + 10_000 } else { 0 };
                changes.events.push(queued(pane, &state, now_ms));
                json!({"active":active})
            }
            method::PANE_CANCEL => cancel(
                &self.store,
                pane,
                &mut state,
                &mut saved,
                &mut changes,
                now_ms,
                &timestamp,
            )?,
            method::PANE_QUEUE_RESUME => {
                if state.queue_paused {
                    state.queue_paused = false;
                    changes.events.push((
                        event::PANE_QUEUE_RESUMED.into(),
                        json!({"paneId":pane,"paused":false}),
                    ));
                }
                json!({"paneId":pane,"paused":false})
            }
            method::PANE_CONFIG_SET => {
                let config_id = errors::param_str(params, "configId")?;
                let value: soyeht_protocol::ConfigValue =
                    serde_json::from_value(params["value"].clone())
                        .map_err(|_| errors::invalid("Configuration value is invalid"))?;
                let user = super::pending::UserOperation {
                    scope: self.clients[connection].scope.clone(),
                    key: errors::param_str(params, "idempotencyKey")?.into(),
                };
                match super::configuration::begin(
                    &mut state,
                    pane,
                    super::configuration::ChangeRequest {
                        config: config_id,
                        value: value.clone(),
                        user: Some(user),
                        started_at: timestamp,
                        now_ms,
                    },
                    &mut changes,
                )? {
                    Some(operation_id) => json!({"_nativePending":operation_id}),
                    None => super::configuration::applied(&value),
                }
            }
            _ => {
                return Err(errors::invalid("Agent method unavailable"));
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

#[cfg(test)]
mod message_tests {
    use super::*;
    use crate::ports::{Clock, testing::FixedClock};
    use std::collections::BTreeMap;

    #[test]
    fn seventh_message_is_refused_without_consuming_a_slot_and_recovers_after_window() {
        let mut pairs: BTreeMap<(&str, &str), Vec<i64>> = BTreeMap::new();
        let clock = FixedClock(1_000);
        for _ in 0..limits::AGENT_MESSAGES_PER_WINDOW {
            let sent = pairs.entry(("a", "b")).or_default();
            assert_eq!(message_retry_after(sent, clock.now_ms()), None);
            sent.push(clock.now_ms());
        }
        let sent = pairs.get(&("a", "b")).unwrap();
        assert_eq!(message_retry_after(sent, clock.now_ms()), Some(60));
        assert_eq!(sent.len(), limits::AGENT_MESSAGES_PER_WINDOW);
        assert_eq!(
            message_retry_after(pairs.entry(("b", "a")).or_default(), clock.now_ms()),
            None
        );
        let later = FixedClock(clock.now_ms() + (limits::AGENT_MESSAGE_WINDOW_SEC as i64) * 1_000);
        let cutoff = later.now_ms() - (limits::AGENT_MESSAGE_WINDOW_SEC as i64) * 1_000;
        let sent = pairs.get_mut(&("a", "b")).unwrap();
        sent.retain(|time| *time > cutoff);
        assert_eq!(message_retry_after(sent, later.now_ms()), None);
    }
}
