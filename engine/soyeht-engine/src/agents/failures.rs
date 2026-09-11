use super::{Changes, state::Phase, transcript};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::json;
use soyeht_protocol::{
    AttentionItem, PaneState, StateSource, TranscriptKind, TranscriptRole, limits,
};
use soyeht_ptyd::wire::Request;

impl Core {
    pub fn close_agent_processes(&mut self, pane: &str) -> Result<()> {
        let saved = &self.panes[pane].stored;
        let current = self.ptyd.as_ref().map(|p| p.supervisor_id());
        let mut ids = Vec::new();
        if saved.supervisor_id.as_deref() == current
            && let Some(id) = &saved.instance_id
        {
            ids.push(id.clone());
        }
        for terminal in self.agents[pane].state.terminals.values() {
            if terminal.supervisor_id.as_deref() == current
                && let Some(id) = &terminal.instance_id
            {
                ids.push(id.clone());
            }
        }
        for instance_id in ids {
            self.pty_request(Request::Close { instance_id })?;
        }
        Ok(())
    }

    pub fn fail_agent(&mut self, pane: &str, reason: &str) -> Result<()> {
        let mut state = self.agent_state(pane)?;
        if state.phase == Phase::Faulted {
            return self.close_agent_processes(pane);
        }
        let mut reason_end = reason.len().min(512);
        while !reason.is_char_boundary(reason_end) {
            reason_end -= 1;
        }
        let reason = &reason[..reason_end];
        let mut saved = self.panes[pane].stored.clone();
        let mut changes = Changes {
            retire_instance: saved.instance_id.clone().zip(saved.supervisor_id.clone()),
            clear_replay: true,
            ..Changes::default()
        };
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
                    timestamp: &self.timestamp(),
                },
                &mut changes,
            )?;
        }
        changes.outgoing.clear();
        if state.phase != Phase::Loading {
            transcript::complete(&self.store, pane, &mut state, &mut changes)?;
        }
        state.open_messages.clear();
        changes.messages.push(transcript::new_message(
            &mut state,
            saved.pane.generation,
            TranscriptRole::System,
            TranscriptKind::System,
            reason.into(),
            true,
        )?);
        if let Some(prompt) = state.active.take() {
            changes.events.push((
                "pane.turn_ended".into(),
                json!({"paneId":pane,"promptId":prompt.prompt_id,"stopReason":"error"}),
            ));
        }
        if let Some(operation) = state.operation.take() {
            changes.events.push(("pane.config_failed".into(),json!({"paneId":pane,"configId":operation.config_id,"value":operation.value,"reason":reason})));
        }
        if let Some(user) = state.operation_user.take() {
            changes.completed_user = Some(super::pending::Completion {
                user,
                result: Err(errors::wire(errors::harness_unavailable(reason))),
            });
        }
        state.pending_policy = None;
        state.client = Default::default();
        state.cancel_at_ms = None;
        state.phase = Phase::Faulted;
        saved.pane.state = PaneState::Dead;
        saved.pane.source = StateSource::Protocol;
        saved.pane.confidence = 1.0;
        saved.pane.phrase = reason.into();
        changes.attention.push(AttentionItem {
            attention_id: format!("{pane}:stopped"),
            pane_id: pane.into(),
            request_id: None,
            message: format!("Agent stopped: {reason}"),
            created_at: self.timestamp(),
        });
        self.commit_agent(saved, state, changes, None, json!({}))?;
        self.close_agent_processes(pane)
    }

    pub fn check_agent_deadlines(&mut self, pane: &str) -> Result<bool> {
        let mut state = self.agent_state(pane)?;
        let now = self.now_ms();
        if state
            .cancel_at_ms
            .is_some_and(|at| now - at >= limits::CANCEL_TIMEOUT_SEC as i64 * 1000)
        {
            self.fail_agent(pane, "Agent did not confirm cancellation within 10 seconds")?;
            return Ok(false);
        }
        if let Some(operation) = state.operation.clone()
            && !state.operation_timed_out
            && state.client.pending.values().any(|request| {
                request.context == operation.operation_id
                    && now - request.sent_at_ms as i64 >= limits::NO_SIGNAL_SEC as i64 * 1000
            })
        {
            let reason = "Configuration confirmation timed out; the session remains open and the pending request is retained";
            state.operation_timed_out = true;
            let mut changes = Changes::default();
            changes.events.push(("pane.config_failed".into(), json!({
                "paneId":pane,"configId":operation.config_id,"value":operation.value,"reason":reason
            })));
            if let Some(user) = state.operation_user.take() {
                changes.completed_user = Some(super::pending::Completion {
                    user,
                    result: Err(errors::wire(errors::harness_unavailable(reason))),
                });
            }
            self.commit_agent(
                self.panes[pane].stored.clone(),
                state.clone(),
                changes,
                None,
                json!({}),
            )?;
        }
        let saved = &self.panes[pane].stored;
        let waiting_for_reply = state.active.is_some()
            || state.operation.is_some()
            || matches!(
                state.phase,
                Phase::Initializing | Phase::Opening | Phase::Loading
            );
        let last = state.last_update_ms.max(
            state
                .client
                .pending
                .values()
                .map(|c| c.sent_at_ms as i64)
                .min()
                .unwrap_or(0),
        );
        let human_question = self
            .store
            .agent_open_requests(pane)
            .map_err(errors::storage)?
            .iter()
            .any(|r| {
                matches!(
                    r.method.as_str(),
                    "session/request_permission" | "elicitation/create"
                )
            });
        if waiting_for_reply
            && !human_question
            && now - last >= limits::NO_SIGNAL_SEC as i64 * 1000
            && saved.pane.state != PaneState::NoSignal
        {
            let mut saved = saved.clone();
            saved.pane.state = PaneState::NoSignal;
            saved.pane.source = StateSource::Protocol;
            saved.pane.confidence = 0.0;
            saved.pane.phrase =
                "No protocol signal for 90 seconds; process state is unconfirmed".into();
            let changes = Changes {
                attention: vec![AttentionItem {
                    attention_id: format!("{pane}:no-signal"),
                    pane_id: pane.into(),
                    request_id: None,
                    message: "No protocol signal for 90 seconds. The current turn remains active."
                        .into(),
                    created_at: self.timestamp(),
                }],
                ..Changes::default()
            };
            self.commit_agent(saved, state, changes, None, json!({}))?;
        }
        Ok(true)
    }
}
