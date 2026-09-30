use super::{Changes, native_error, questions, responses, state::Phase, transcript, updates};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::{Clock, StorePort},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_acp::{
    Message, RpcError, client,
    rpc::{Record as NativeRecord, decode},
};
use soyeht_protocol::{
    PaneState, ProbeCapability, ProbeResult, PromptItem, TranscriptKind, TranscriptRole,
    ndjson::Decoder,
};
use soyeht_ptyd::wire::{Request, Response};
use soyeht_store::segments::Record;
use uuid::Uuid;

fn next_prompt<S: StorePort, C: Clock + ?Sized>(
    store: &S,
    clock: &C,
    pane: &str,
    generation: u32,
    state: &mut super::state::State,
) -> Result<(Vec<soyeht_store::agents::NativeRequest>, Option<PromptItem>)> {
    let open = store.agent_open_requests(pane).map_err(errors::storage)?;
    let next = state.take_next(generation, clock.now_ms(), !open.is_empty())?;
    Ok((open, next))
}

fn prompt_payload(store: &impl StorePort, session: &str, prompt: &PromptItem) -> Result<Value> {
    let attachments =
        StorePort::agent_prompt_attachments(store, &prompt.prompt_id).map_err(errors::storage)?;
    let images: Vec<_> = attachments
        .iter()
        .map(|attachment| client::PromptImage {
            mime_type: &attachment.reference.mime_type,
            bytes: &attachment.bytes,
        })
        .collect();
    Ok(client::prompt_with_images(session, &prompt.text, &images))
}

struct NativeContext<'a> {
    pane: &'a str,
    source_seq: u64,
    now_ms: i64,
    timestamp: &'a str,
}

fn native_error_response(id: serde_json::Value, code: i32, message: String) -> Message {
    Message::Response {
        id,
        result: Err(RpcError {
            code,
            message,
            data: None,
        }),
    }
}

fn reject_question(
    pane: &str,
    state: &mut super::state::State,
    generation: u32,
    id: Value,
    method: &str,
    error: crate::errors::Error,
    changes: &mut Changes,
) -> Result<()> {
    let reason = match &error {
        errors::Error::InvalidParams(reason) => reason.as_str(),
        _ => "The request could not be displayed safely",
    };
    if method == "session/request_permission" {
        let notice =
            format!("Soyeht blocked this permission request: {reason}. Nothing was approved.");
        let message = transcript::new_message(
            state,
            generation,
            TranscriptRole::System,
            TranscriptKind::System,
            notice,
            true,
        )?;
        transcript::put(changes, pane, message, "system_message")?;
    }
    changes
        .outgoing
        .push(native_error_response(id, -32602, error.to_string()));
    Ok(())
}

impl Core {
    fn apply_native_message(
        &self,
        context: NativeContext<'_>,
        message: Message,
        state: &mut super::state::State,
        saved: &mut crate::model::StoredPane,
    ) -> Result<(Changes, bool)> {
        let NativeContext {
            pane,
            source_seq,
            now_ms,
            timestamp,
        } = context;
        let mut changes = Changes::default();
        let observed_turn_activity = match message {
            response @ Message::Response { .. } => {
                responses::response(
                    &self.store,
                    &responses::Runtime {
                        data_directory: &self.directory,
                        now_ms,
                        timestamp,
                    },
                    state,
                    saved,
                    response,
                    &mut changes,
                )?;
                true
            }
            Message::Notification { method, params } => {
                if method == "session/update" {
                    updates::update(
                        &self.store,
                        pane,
                        saved.pane.generation,
                        state,
                        &params,
                        &mut changes,
                    )?
                } else {
                    false
                }
            }
            Message::Request { id, method, params } => {
                if let Some(prior) = self
                    .store
                    .agent_request_at(pane, saved.pane.generation, source_seq)
                    .map_err(errors::storage)?
                {
                    if prior.native_id != id || prior.method != method || prior.params != params {
                        return Err(errors::resync_required(
                            "Native request replay changed identity",
                        ));
                    }
                    // Receipt and native outbox were committed together. The
                    // outbox resumes its byte offset instead of sending twice.
                } else if matches!(
                    method.as_str(),
                    "session/request_permission" | "elicitation/create"
                ) {
                    let mut question = Changes::default();
                    if let Err(error) = questions::receive(
                        state,
                        saved,
                        questions::Incoming {
                            source_seq,
                            native_id: id.clone(),
                            method: &method,
                            params,
                            now_ms,
                            timestamp,
                        },
                        &mut question,
                    ) {
                        reject_question(
                            pane,
                            state,
                            saved.pane.generation,
                            id,
                            &method,
                            error,
                            &mut changes,
                        )?;
                    } else {
                        changes = question;
                    }
                } else if matches!(
                    method.as_str(),
                    "fs/read_text_file"
                        | "fs/write_text_file"
                        | "terminal/create"
                        | "terminal/output"
                        | "terminal/wait_for_exit"
                        | "terminal/kill"
                        | "terminal/release"
                ) {
                    if let Err(error) = super::effects::require_session(state, &params) {
                        changes
                            .outgoing
                            .push(native_error_response(id, -32602, error.to_string()));
                    } else {
                        changes.received.push(super::effects::receive(
                            pane,
                            saved.pane.generation,
                            source_seq,
                            id,
                            method,
                            params,
                            now_ms,
                        ));
                    }
                } else {
                    changes.outgoing.push(native_error_response(
                        id,
                        -32601,
                        "Client method unavailable".into(),
                    ));
                }
                true
            }
        };
        Ok((changes, observed_turn_activity))
    }

    pub(crate) fn native_record(
        &mut self,
        pane: &str,
        record: NativeRecord,
        advances_stream: bool,
    ) -> Result<()> {
        let mut state = self.agent_state(pane)?;
        let mut saved = self.panes[pane].stored.clone();
        let was_no_signal = saved.pane.state == PaneState::NoSignal;
        let had_active_prompt = state.active.is_some();
        let now_ms = self.now_ms();
        let timestamp = self.timestamp();
        let (mut changes, observed_turn_activity) = self.apply_native_message(
            NativeContext {
                pane,
                source_seq: record.from_seq,
                now_ms,
                timestamp: &timestamp,
            },
            record.message,
            &mut state,
            &mut saved,
        )?;
        // Unknown native frames keep the durable elapsed wait visible. That is
        // deliberately conservative while the notice is non-destructive: the
        // observed wedge emits catalogue noise without producing turn output.
        // Never attach automatic cancellation or restart to this classification.
        if had_active_prompt && observed_turn_activity {
            state.active_output_observed = true;
            state.active_silent_since_ms = None;
        }
        if advances_stream {
            state.acknowledged_offset = record.to_seq;
        }
        state.last_update_ms = self.now_ms();
        if saved.pane.state == PaneState::NoSignal && (!had_active_prompt || observed_turn_activity)
        {
            saved.pane.state = if state.active.is_some() {
                PaneState::Working
            } else {
                PaneState::Starting
            };
            saved.pane.phrase = "Agent protocol resumed".into();
        }
        if was_no_signal && saved.pane.state != PaneState::NoSignal {
            changes.remove_attention.push(format!("{pane}:no-signal"));
        }
        saved.pane.source = soyeht_protocol::StateSource::Protocol;
        saved.pane.confidence = 1.0;
        self.commit_agent(saved, state, changes, None, json!({}))?;
        Ok(())
    }

    /// First consume any raw records synced before an engine crash. The public
    /// checkpoint can lag the raw log, but it never advances beyond durable data.
    fn decode_agent_log(&mut self, pane: &str) -> Result<()> {
        let base = self.panes[pane].stored.sequence_base;
        let offset = self.agents[pane].decoder.next_offset();
        let end = self.panes[pane].screen.next_offset();
        let mut from = base
            .checked_add(offset)
            .ok_or_else(|| errors::internal("Agent offset exhausted"))?;
        let first = self.panes[pane].screen.first_offset();
        if from < first {
            self.agent_output_gap(pane, first - base)?;
            from = first;
        }
        if from == end {
            return Ok(());
        }
        let records = self.panes[pane].screen.read(from, end, 64 * 1024)?;
        for record in records {
            let (mut offset, mut bytes) = match record {
                Record::Gap { to, .. } => {
                    self.agent_output_gap(pane, to - base)?;
                    continue;
                }
                Record::Data { offset, bytes } => (offset, bytes),
            };
            if self.agents[pane].state.discard_until_newline {
                let newline = bytes.iter().position(|byte| *byte == b'\n');
                let skipped = newline.map_or(bytes.len(), |at| at + 1);
                self.skip_interrupted_agent_record(
                    pane,
                    offset - base + skipped as u64,
                    newline.is_some(),
                )?;
                offset += skipped as u64;
                bytes.drain(..skipped);
                if bytes.is_empty() {
                    continue;
                }
            }
            let decoded = decode(
                &mut self
                    .agents
                    .get_mut(pane)
                    .ok_or_else(|| {
                        errors::internal("Agent runtime disappeared while decoding output")
                    })?
                    .decoder,
                offset - base,
                &bytes,
            );
            let decoded = match decoded {
                Ok(records) => records,
                Err(error) => {
                    let running = errors::invariant(
                        self.agents.get_mut(pane),
                        "Agent runtime disappeared after a decoder failure",
                    )?;
                    running.decoder = Decoder::new(running.state.acknowledged_offset);
                    return Err(native_error(error));
                }
            };
            for record in decoded {
                if let Err(error) = self.native_record(pane, record, true) {
                    let running = errors::invariant(
                        self.agents.get_mut(pane),
                        "Agent runtime disappeared after a native record failure",
                    )?;
                    running.decoder = Decoder::new(running.state.acknowledged_offset);
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    pub fn poll_agent(&mut self, pane: &str) -> Result<()> {
        if self.agents[pane].state.phase == Phase::Faulted {
            return self.close_agent_processes(pane);
        }
        inject_per_pane_poll_failure(self, pane)?;
        drop_current_agent_before_poll(self, pane)?;
        self.decode_agent_log(pane)?;
        let saved = self.panes[pane].stored.clone();
        let instance_id = errors::invariant(
            saved.instance_id.clone(),
            "Agent process has no adopted instance",
        )?;
        let seq = self.panes[pane].screen.next_offset() - saved.sequence_base;
        let Response::Output(output) = self.pty_request(Request::Drain {
            instance_id: instance_id.clone(),
            from_seq: seq,
        })?
        else {
            return Err(errors::ptyd_unavailable("Invalid native output response"));
        };
        if output.from_seq != seq {
            if output.from_seq < seq
                || output
                    .gap
                    .as_ref()
                    .is_none_or(|gap| gap.from_seq != seq || gap.to_seq != output.from_seq)
            {
                return Err(errors::ptyd_unavailable("Invalid native output gap"));
            }
            self.panes
                .get_mut(pane)
                .ok_or_else(|| errors::internal("Agent pane disappeared during output gap"))?
                .screen
                .append(Record::Gap {
                    from: saved.sequence_base + seq,
                    to: saved.sequence_base + output.from_seq,
                })?;
        }
        let bytes = STANDARD
            .decode(output.bytes)
            .map_err(|_| errors::ptyd_unavailable("Invalid native output encoding"))?;
        if output.next_seq != output.from_seq + bytes.len() as u64 {
            return Err(errors::ptyd_unavailable(
                "Native output offsets disagree with bytes",
            ));
        }
        if !bytes.is_empty() {
            self.panes
                .get_mut(pane)
                .ok_or_else(|| errors::internal("Agent pane disappeared during output append"))?
                .screen
                .append(Record::Data {
                    offset: saved.sequence_base + output.from_seq,
                    bytes,
                })?;
        }
        self.decode_agent_log(pane)?;
        self.pty_request(Request::Ack {
            instance_id,
            seq: self.agents[pane].state.acknowledged_offset,
        })?;
        if output.exited {
            // Exit does not imply that the last page has been consumed. Keep
            // draining before retiring the process or trying to write to it.
            if output.next_seq != output.end_seq
                || self.agents[pane].decoder.next_offset() != output.end_seq
            {
                return Ok(());
            }
            if self.agents[pane].state.phase != Phase::RestartAfterProbe {
                return self.fail_agent(pane, "Agent process exited");
            }
        }
        if !self.check_agent_deadlines(pane)? {
            return Ok(());
        }
        self.advance_agent(pane)?;
        self.flush_agent_outbox(pane)
    }

    pub(crate) fn restart_after_probe(&mut self, pane: &str) -> Result<()> {
        let saved = self.panes[pane].stored.clone();
        let retire_instance = saved.instance_id.clone().zip(saved.supervisor_id.clone());
        if let Some(instance_id) = saved.instance_id {
            self.pty_request(Request::Close { instance_id })?;
        }
        let mut saved = self.panes[pane].stored.clone();
        let mut state = self.agent_state(pane)?;
        saved.instance_key = Uuid::new_v4().to_string();
        saved.pane.generation = saved
            .pane
            .generation
            .checked_add(1)
            .ok_or_else(|| errors::internal("Pane generation exhausted"))?;
        saved.instance_id = None;
        saved.supervisor_id = None;
        saved.pane.pid = None;
        saved.sequence_base = self.panes[pane].screen.next_offset();
        state.client = Default::default();
        state.initialized = None;
        state.options = Default::default();
        state.probe_session_id = None;
        state.probe_started_ms = None;
        state.acknowledged_offset = 0;
        state.discard_until_newline = false;
        state.open_messages.clear();
        state.replay_role = None;
        state.phase = Phase::Initializing;
        self.commit_agent(
            saved,
            state,
            Changes {
                retire_instance,
                clear_replay: true,
                ..Changes::default()
            },
            None,
            json!({}),
        )?;
        errors::invariant(
            self.agents.get_mut(pane),
            "Agent runtime disappeared during restart",
        )?
        .decoder = Decoder::new(0);
        self.spawn_agent(pane)
    }

    fn advance_agent(&mut self, pane: &str) -> Result<()> {
        self.poll_agent_authentication(pane)?;
        self.run_native_files(pane)?;
        self.run_native_terminals(pane)?;
        let mut state = self.agent_state(pane)?;
        if state.phase == Phase::RestartAfterProbe {
            return self.restart_after_probe(pane);
        }
        if state.phase == Phase::Ready && !state.requested_options.is_empty() {
            return self.configure_initial_agent(pane);
        }
        if state
            .probe_started_ms
            .is_some_and(|start| self.now_ms() - start >= 10_000)
        {
            let mut changes = Changes::default();
            for method in responses::PROBED_METHODS {
                if self
                    .store
                    .agent_probe(&state.fingerprint, method)
                    .map_err(errors::storage)?
                    .is_none()
                {
                    changes.probes.push(ProbeCapability {
                        method: method.into(),
                        result: ProbeResult::Inconclusive,
                        checked_at: self.timestamp(),
                    });
                }
            }
            state.phase = Phase::RestartAfterProbe;
            self.commit_agent(
                self.panes[pane].stored.clone(),
                state,
                changes,
                None,
                json!({}),
            )?;
            return self.restart_after_probe(pane);
        }
        let mut open = self
            .store
            .agent_open_requests(pane)
            .map_err(errors::storage)?;
        for request in &open {
            let selected =
                questions::automatic(&state, request, &self.panes[pane].stored.pane.cwd)?;
            let expired = request.expires_ms <= self.now_ms();
            if !expired && selected.is_none() {
                continue;
            }
            let mut changes = Changes::default();
            questions::decision(
                &mut state,
                request,
                questions::Decision {
                    option: if expired { None } else { selected.as_deref() },
                    fields: None,
                    by: if expired { "system" } else { "rule" },
                    expired,
                    timestamp: &self.timestamp(),
                },
                &mut changes,
            )?;
            self.commit_agent(
                self.panes[pane].stored.clone(),
                state.clone(),
                changes,
                None,
                json!({}),
            )?;
        }
        let mut saved = self.panes[pane].stored.clone();
        let (next_open, next) = next_prompt(
            &self.store,
            self.clock.as_ref(),
            pane,
            saved.pane.generation,
            &mut state,
        )?;
        open = next_open;
        if open.is_empty() && saved.pane.state == PaneState::WaitingForYou && state.active.is_some()
        {
            saved.pane.state = PaneState::Working;
            saved.pane.phrase = "Agent is working".into();
            self.commit_agent(
                saved.clone(),
                state.clone(),
                Changes::default(),
                None,
                json!({}),
            )?;
        }
        let Some(prompt) = next else {
            return Ok(());
        };
        let session = state
            .session_id
            .clone()
            .ok_or_else(|| errors::internal("Ready agent has no session"))?;
        let mut changes = Changes::default();
        transcript::user_prompt(
            &mut state,
            saved.pane.generation,
            &prompt.text,
            &mut changes,
        )?;
        responses::call(
            &mut state,
            &mut changes,
            "session/prompt",
            prompt_payload(&self.store, &session, &prompt)?,
            &prompt.prompt_id,
            self.now_ms(),
        )?;
        saved.pane.state = PaneState::Working;
        saved.pane.phrase = "Agent is working".into();
        self.commit_agent(saved, state, changes, None, json!({}))?;
        Ok(())
    }
}

#[cfg(debug_assertions)]
fn inject_per_pane_poll_failure(core: &Core, pane: &str) -> Result<()> {
    let marker = core.directory.join("test-fail-agent-poll");
    let Ok(target) = std::fs::read_to_string(marker) else {
        return Ok(());
    };
    if target.trim() == pane {
        return Err(errors::not_found("Injected per-pane polling failure"));
    }
    Ok(())
}

#[cfg(not(debug_assertions))]
fn inject_per_pane_poll_failure(_core: &Core, _pane: &str) -> Result<()> {
    Ok(())
}

#[cfg(debug_assertions)]
fn drop_current_agent_before_poll(core: &mut Core, pane: &str) -> Result<()> {
    let marker = core.directory.join("test-drop-current-agent-before-poll");
    if !marker.exists() {
        return Ok(());
    }
    std::fs::remove_file(marker).map_err(errors::io)?;
    let instance_id = core.panes[pane]
        .stored
        .instance_id
        .clone()
        .ok_or_else(|| errors::internal("Agent process has no adopted instance"))?;
    core.pty_request(Request::Close { instance_id })?;
    Ok(())
}

#[cfg(not(debug_assertions))]
fn drop_current_agent_before_poll(_core: &mut Core, _pane: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::testing::{FixedClock, MemoryStore};
    use soyeht_protocol::{HarnessLaunch, PromptOrigin};

    #[test]
    fn rejected_permission_is_visible_without_echoing_native_payload() {
        let mut state = super::super::state::State::new(
            HarnessLaunch {
                id: "test".into(),
                name: "Test".into(),
                command: "/test".into(),
                args: Vec::new(),
                version: "1".into(),
                environment: None,
            },
            "digest".into(),
            None,
        )
        .unwrap();
        let mut changes = Changes::default();
        reject_question(
            "pane",
            &mut state,
            1,
            json!("native-id"),
            "session/request_permission",
            errors::invalid("Executable permission has no command to display"),
            &mut changes,
        )
        .unwrap();
        assert_eq!(changes.messages.len(), 1);
        assert_eq!(changes.messages[0].role, TranscriptRole::System);
        assert_eq!(changes.messages[0].kind, TranscriptKind::System);
        assert!(changes.messages[0].text.contains("no command to display"));
        assert!(changes.messages[0].text.contains("Nothing was approved"));
        assert_eq!(changes.events.len(), 1);
        assert_eq!(changes.events[0].0, "pane.update");
        assert_eq!(changes.events[0].1["kind"], "system_message");
        assert_eq!(changes.outgoing.len(), 1);
        assert!(matches!(
            &changes.outgoing[0],
            Message::Response { result: Err(error), .. } if error.code == -32602
        ));
    }

    #[test]
    fn agent_session_rule_reads_requests_and_time_through_ports() {
        let store = MemoryStore::default();
        let mut state = super::super::state::State::new(
            HarnessLaunch {
                id: "test".into(),
                name: "Test".into(),
                command: "/test".into(),
                args: Vec::new(),
                version: "1".into(),
                environment: None,
            },
            "digest".into(),
            None,
        )
        .unwrap();
        state.phase = Phase::Ready;
        state.session_id = Some("session".into());
        state.requested_options.clear();
        state
            .enqueue("do work".into(), PromptOrigin::Automatic, "fixed".into())
            .unwrap();
        state.draft_until_ms = 20_001;

        let (_, held) = next_prompt(&store, &FixedClock(20_000), "pane", 1, &mut state).unwrap();
        assert!(held.is_none());
        let (_, released) =
            next_prompt(&store, &FixedClock(20_001), "pane", 1, &mut state).unwrap();
        assert_eq!(released.unwrap().text, "do work");
    }
}
