use super::{
    Changes,
    state::{Phase, RetiredAgentProcess, State},
    terminals::Terminal,
    transcript,
};
use crate::{
    core::Core,
    errors::{self, Result},
    model::{config_failed, turn_ended},
    ports::StorePort,
};
use serde_json::json;
use soyeht_platform::log;
use soyeht_protocol::{
    AttentionItem, PaneState, StateSource, TranscriptKind, TranscriptRole, TurnStopReason, limits,
};

/// How long a cancel that the agent has not answered waits before it is sent again.
const CANCEL_RESEND_MS: i64 = 1000;

impl Core {
    pub fn close_agent_processes(&mut self, pane: &str) -> Result<()> {
        let saved = &self.panes[pane].stored;
        let current = self.ptyd.as_ref().map(|p| p.supervisor_id());
        let mut processes = Vec::new();
        if saved.supervisor_id.as_deref() == current
            && let (Some(instance_id), Some(supervisor_id)) =
                (&saved.instance_id, &saved.supervisor_id)
        {
            processes.push(RetiredAgentProcess {
                instance_key: saved.instance_key.clone(),
                instance_id: instance_id.clone(),
                supervisor_id: supervisor_id.clone(),
            });
        }
        let terminals = self.agents[pane].state.terminals.values();
        processes.extend(
            terminals
                .filter(|terminal| terminal.supervisor_id.as_deref() == current)
                .filter_map(Terminal::retirement),
        );
        // A faulted agent is visited on every tick; each process is handed over once. The ledger
        // closes it, again and again until the supervisor has, and it survives a lost link.
        processes.retain(|process| !self.agents[pane].idle.already_closed(&process.instance_id));
        let handed: Vec<_> = processes
            .iter()
            .map(|process| process.instance_id.clone())
            .collect();
        self.retire_processes(pane, processes)?;
        for instance_id in handed {
            errors::invariant(self.agents.get_mut(pane), "Agent runtime disappeared")?
                .idle
                .mark_closed(instance_id);
        }
        Ok(())
    }

    pub fn fail_agent(&mut self, pane: &str, reason: &str) -> Result<()> {
        let mut state = self.agent_state(pane)?;
        if state.phase == Phase::Faulted {
            return self.close_agent_processes(pane);
        }
        let task_owner_loss = self.task_owner_loss(pane, "returned: the pane closed")?;
        let mut reason_end = reason.len().min(512);
        while !reason.is_char_boundary(reason_end) {
            reason_end -= 1;
        }
        let reason = &reason[..reason_end];
        let mut saved = self.panes[pane].stored.clone();
        let mut changes = Changes {
            retire_instance: saved.instance_id.clone().zip(saved.supervisor_id.clone()),
            clear_replay: true,
            task_owner_loss,
            ..Changes::default()
        };
        super::questions::decide_open_as_system(
            &self.store,
            pane,
            &mut state,
            &mut changes,
            &self.timestamp(),
        )?;
        changes.outgoing.clear();
        transcript::close(&self.store, pane, &mut state, &mut changes)?;
        changes.messages.push(transcript::new_message(
            &mut state,
            saved.pane.generation,
            TranscriptRole::System,
            TranscriptKind::System,
            reason.into(),
            true,
        )?);
        fail_work_in_flight(&mut state, &mut changes, pane, reason)?;
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
        if let Some(at) = state.cancel_at_ms
            && now - state.cancel_sent_ms.unwrap_or(at) >= CANCEL_RESEND_MS
            && let Some(session) = state.session_id.clone()
        {
            // An agent that was still starting the turn when the cancel reached it drops the
            // cancel and runs the turn to its end. The notification is idempotent, so it is sent
            // again until the turn answers or the deadline above passes.
            if state.cancel_sent_ms == Some(at) {
                // Once a turn, so the log shows an adapter that dropped a cancel and not each resend.
                log::event("engine.agent_cancel_resent", &[("paneId", pane)]);
            }
            state.cancel_sent_ms = Some(now);
            let mut changes = Changes::default();
            changes.outgoing.push(soyeht_acp::client::cancel(&session));
            self.commit_agent_state(pane, &state, changes)?;
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
            changes.events.push(config_failed(
                pane,
                operation.config_id.clone(),
                operation.value.clone(),
                reason,
            )?);
            if let Some(user) = state.operation_user.take() {
                changes.completed_user = Some(super::pending::Completion {
                    user,
                    result: Err(errors::wire(errors::harness_unavailable(reason))),
                });
            }
            self.commit_agent_state(pane, &state, changes)?;
        }
        self.warn_if_progress_is_unconfirmed(pane, state, now)?;
        Ok(true)
    }

    /// Stores `state` and `changes` for the pane as it is, with no user request behind them.
    fn commit_agent_state(&mut self, pane: &str, state: &State, changes: Changes) -> Result<()> {
        self.commit_agent(
            self.panes[pane].stored.clone(),
            state.clone(),
            changes,
            None,
            json!({}),
        )?;
        Ok(())
    }

    fn warn_if_progress_is_unconfirmed(
        &mut self,
        pane: &str,
        state: super::state::State,
        now: i64,
    ) -> Result<()> {
        let saved = &self.panes[pane].stored;
        let waiting_for_non_prompt_reply = state.active.is_none()
            && (state.operation.is_some()
                || matches!(
                    state.phase,
                    Phase::Initializing | Phase::Opening | Phase::Loading
                ));
        let last = state.last_update_ms.max(
            state
                .client
                .pending
                .values()
                .map(|c| c.sent_at_ms as i64)
                .min()
                .unwrap_or(0),
        );
        let silent_since = if state.active.is_some() && !state.active_output_observed {
            state.active_silent_since_ms
        } else if waiting_for_non_prompt_reply {
            Some(last)
        } else {
            None
        };
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
        // Parsed protocol state decides whether the turn has produced output.
        // Elapsed time only escalates a non-destructive presentation; it never
        // infers activity, cancels, restarts, or otherwise acts on the turn.
        if silent_since
            .is_some_and(|at| now.saturating_sub(at) >= limits::NO_SIGNAL_SEC as i64 * 1000)
            && !human_question
            && saved.pane.state != PaneState::NoSignal
        {
            let mut saved = saved.clone();
            saved.pane.state = PaneState::NoSignal;
            saved.pane.source = StateSource::Protocol;
            saved.pane.confidence = 0.0;
            let prompt_is_silent = state.active.is_some();
            saved.pane.phrase = if prompt_is_silent {
                format!(
                    "The agent has produced no output for {} seconds; the turn is still active",
                    limits::NO_SIGNAL_SEC
                )
            } else {
                format!(
                    "No protocol signal for {} seconds; process state is unconfirmed",
                    limits::NO_SIGNAL_SEC
                )
            };
            let changes = Changes {
                attention: vec![AttentionItem {
                    attention_id: format!("{pane}:no-signal"),
                    pane_id: pane.into(),
                    request_id: None,
                    message: if prompt_is_silent {
                        format!(
                            "The agent has produced no output for {} seconds. The current turn remains active.",
                            limits::NO_SIGNAL_SEC
                        )
                    } else {
                        format!(
                            "No protocol signal for {} seconds. The current operation remains active.",
                            limits::NO_SIGNAL_SEC
                        )
                    },
                    created_at: self.timestamp(),
                }],
                ..Changes::default()
            };
            self.commit_agent(saved, state, changes, None, json!({}))?;
        }
        Ok(())
    }
}

/// The turn, the configuration change and the caller waiting on it end together, with `reason`,
/// when the process that held them is gone.
pub(super) fn fail_work_in_flight(
    state: &mut State,
    changes: &mut Changes,
    pane: &str,
    reason: &str,
) -> Result<()> {
    if let Some(prompt) = state.active.take() {
        changes
            .events
            .push(turn_ended(pane, prompt.prompt_id, TurnStopReason::Error)?);
    }
    if let Some(operation) = state.operation.take() {
        changes.events.push(config_failed(
            pane,
            operation.config_id,
            operation.value,
            reason,
        )?);
    }
    if let Some(user) = state.operation_user.take() {
        changes.completed_user = Some(super::pending::Completion {
            user,
            result: Err(errors::wire(errors::harness_unavailable(reason))),
        });
    }
    Ok(())
}
