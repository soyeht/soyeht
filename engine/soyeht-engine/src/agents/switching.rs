//! Atomic replacement of one live ACP harness with another.
//!
//! The original process and checkpoint remain authoritative while a candidate
//! initializes beside it. Only a candidate that has opened a native session,
//! accepted requested configuration, and accepted the bounded continuation
//! package may replace the pane's durable runtime.

use super::{
    Changes, naming, native_error,
    pending::{Completion, UserOperation},
    state::{HarnessSwitchCheckpoint, InternalContinuation, Phase, RetiredAgentProcess, State},
    transcript,
};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_acp::{
    Message,
    config::Options,
    continuation::{self, Fields, Message as ContinuationMessage, Role as ContinuationRole},
    rpc::decode,
    session,
};
use soyeht_protocol::{
    ConfigValue, PaneHarnessSwitchParams, PaneState, StateSource, TranscriptKind, TranscriptRole,
    limits, ndjson::Decoder,
};
use soyeht_ptyd::wire::{Instance, ProcessKind, Request, Response, Spawn};
use soyeht_store::segments::Record;
use std::time::{Duration, Instant};
use uuid::Uuid;

const SWITCH_TIMEOUT: Duration = Duration::from_secs(30);
const CONTINUATION_TEMPLATE: &str =
    include_str!("../../../../docs/contracts/prompts/harness-switch.en.md");

pub(crate) struct HarnessSwitchRuntime {
    params: PaneHarnessSwitchParams,
    launch: soyeht_protocol::HarnessLaunch,
    binary_digest: String,
    candidate: Option<Candidate>,
    deadline: Instant,
}

struct Candidate {
    instance: Instance,
    saved: crate::model::StoredPane,
    state: State,
    decoder: Decoder,
    raw_output: Vec<u8>,
    phase: CandidatePhase,
    /// The new harness's preferred mode, used when it does not offer the
    /// mode the pane kept from the previous harness.
    default_mode: Option<String>,
}

enum CandidatePhase {
    Initializing,
    Opening,
    Configuring {
        option_id: String,
        desired: ConfigValue,
        remaining: Vec<(String, String)>,
    },
    Continuing {
        incorporated_pending: Vec<String>,
    },
}

impl Core {
    pub fn agent_harness_switch(&mut self, connection: &str, raw: &Value) -> Result<Value> {
        let params: PaneHarnessSwitchParams = serde_json::from_value(raw.clone())
            .map_err(|_| errors::invalid("Invalid harness switch parameters"))?;
        if let Some(result) = self.repeated_mutation(connection, "pane.harness.switch", raw)? {
            return Ok(result);
        }
        let pane = params.pane_id.as_str();
        let mut state = self.agent_state(pane)?;
        if self.harness_switches.contains_key(pane)
            || state.harness_switch.is_some()
            || !state.retired_processes.is_empty()
            || state.operation.is_some()
            || matches!(
                state.phase,
                Phase::NeedsLogin | Phase::Faulted | Phase::Recovering
            )
        {
            return Err(errors::pane_busy("Another agent operation is pending"));
        }
        if state.launch.id == params.harness {
            return Err(errors::invalid("Choose a different harness"));
        }
        let (launch, binary_digest) = super::registry::prepare(&self.directory, &params.harness)?;
        let checkpoint = HarnessSwitchCheckpoint {
            user: UserOperation {
                scope: self.clients[connection].scope.clone(),
                key: params.idempotency_key.clone(),
            },
            from_harness: state.launch.id.clone(),
            to_harness: launch.id.clone(),
            to_name: launch.name.clone(),
            candidate_instance_key: Uuid::new_v4().to_string(),
            previous_queue_paused: state.queue_paused,
        };
        state.harness_switch = Some(checkpoint.clone());
        let mut saved = self.panes[pane].stored.clone();
        let mut changes = Changes::default();
        if !state.queue_paused {
            state.queue_paused = true;
            changes.events.push((
                "pane.queue.paused".into(),
                json!({"paneId":pane,"paused":true,"reason":"harness_switch"}),
            ));
        }
        if state.active.is_some() {
            super::methods::cancel(
                &self.store,
                pane,
                &mut state,
                &mut saved,
                &mut changes,
                self.now_ms(),
                &self.timestamp(),
            )?;
        }
        saved.pane.state = PaneState::Starting;
        saved.pane.phrase = format!("Switching to {}", launch.name);
        changes.events.push((
            "pane.harness_switch_started".into(),
            json!({"paneId":pane,"fromHarness":checkpoint.from_harness,
                "toHarness":checkpoint.to_harness}),
        ));
        let pending = json!({"_nativePending":checkpoint.candidate_instance_key});
        let result = self
            .commit_agent(
                saved,
                state,
                changes,
                Some((connection, "pane.harness.switch", raw)),
                pending,
            )?
            .result;
        self.harness_switches.insert(
            pane.into(),
            HarnessSwitchRuntime {
                params,
                launch,
                binary_digest,
                candidate: None,
                deadline: Instant::now() + SWITCH_TIMEOUT,
            },
        );
        Ok(result)
    }

    pub(crate) fn advance_harness_switches(&mut self) -> Result<()> {
        let panes = self.harness_switches.keys().cloned().collect::<Vec<_>>();
        for pane in panes {
            let runtime = errors::invariant(
                self.harness_switches.remove(&pane),
                "Harness switch runtime disappeared",
            )?;
            match self.advance_harness_switch(&pane, runtime) {
                Ok(Some(runtime)) => {
                    self.harness_switches.insert(pane, runtime);
                }
                Ok(None) => {}
                Err(failure) => {
                    let (mut runtime, error) = *failure;
                    if let Some(candidate) = runtime.candidate.take() {
                        self.retire_failed_candidate(&pane, &candidate.instance)?;
                    }
                    let reason = bounded_reason(&error.to_string());
                    self.restore_failed_switch(&pane, &reason)?;
                }
            }
        }
        Ok(())
    }

    fn advance_harness_switch(
        &mut self,
        pane: &str,
        mut runtime: HarnessSwitchRuntime,
    ) -> std::result::Result<Option<HarnessSwitchRuntime>, Box<(HarnessSwitchRuntime, errors::Error)>>
    {
        let result = (|| -> Result<bool> {
            if Instant::now() >= runtime.deadline {
                return Err(errors::harness_unavailable(
                    "The selected harness did not become ready in time",
                ));
            }
            if runtime.candidate.is_none() {
                if self.agent_state(pane)?.active.is_some() {
                    return Ok(false);
                }
                let candidate = self.spawn_switch_candidate(pane, &runtime)?;
                runtime.candidate = Some(candidate);
                if let Some(candidate) = runtime.candidate.as_mut() {
                    self.initialize_switch_candidate(candidate)?;
                }
                return Ok(false);
            }
            let next_name = runtime.launch.name.clone();
            let params = runtime.params.clone();
            let candidate = errors::invariant(
                runtime.candidate.as_mut(),
                "Harness switch candidate disappeared",
            )?;
            let Some((call, response)) = self.drain_candidate(candidate)? else {
                return Ok(false);
            };
            self.advance_candidate_response(pane, &next_name, &params, candidate, call, response)
        })();
        match result {
            Ok(true) => Ok(None),
            Ok(false) => Ok(Some(runtime)),
            Err(error) => Err(Box::new((runtime, error))),
        }
    }

    fn advance_candidate_response(
        &mut self,
        pane: &str,
        next_name: &str,
        params: &PaneHarnessSwitchParams,
        candidate: &mut Candidate,
        call: soyeht_acp::client::Call,
        response: std::result::Result<Value, soyeht_acp::RpcError>,
    ) -> Result<bool> {
        match &candidate.phase {
            CandidatePhase::Initializing if call.context == "switch:initialize" => {
                let value = response.map_err(candidate_rpc_error)?;
                let initialized = session::Initialization::parse(&value).map_err(native_error)?;
                candidate.state.fingerprint = session::fingerprint(
                    &candidate.state.binary_digest,
                    &initialized.version,
                    &json!(candidate.state.launch),
                )
                .map_err(native_error)?;
                candidate.state.initialized = Some(initialized);
                let servers = if candidate.state.mcp_enabled {
                    vec![super::mcp::declaration(
                        &self.store,
                        &self.directory,
                        &candidate.state,
                        &candidate.saved,
                    )?]
                } else {
                    Vec::new()
                };
                let request = candidate
                    .state
                    .client
                    .request(
                        "session/new",
                        super::opening::parameters(
                            &self.directory,
                            &candidate.state,
                            soyeht_acp::client::new_session_with_mcp(
                                &candidate.saved.pane.cwd,
                                &servers,
                            ),
                        )?,
                        "switch:open",
                        self.now_ms() as u64,
                    )
                    .map_err(native_error)?;
                self.write_candidate(&candidate.instance.instance_id, request)?;
                candidate.phase = CandidatePhase::Opening;
                Ok(false)
            }
            CandidatePhase::Opening if call.context == "switch:open" => {
                let value = response.map_err(candidate_rpc_error)?;
                candidate.state.session_id =
                    Some(session::session_id(&value).map_err(native_error)?);
                let options = Options::from_response(&value).map_err(native_error)?;
                if let Some(mode) = candidate.default_mode.take().filter(|mode| {
                    options.offers_mode(mode)
                        && !candidate
                            .state
                            .mode
                            .as_deref()
                            .is_some_and(|kept| options.offers_mode(kept))
                }) {
                    candidate.state.mode = Some(mode);
                }
                super::configuration::open_session(&mut candidate.state, options)?;
                let requested = requested_options(params, &candidate.state);
                if requested.is_empty() {
                    return self.begin_continuation_or_commit(pane, next_name, candidate);
                }
                self.send_next_candidate_option(candidate, requested)?;
                Ok(false)
            }
            CandidatePhase::Configuring {
                option_id,
                desired,
                remaining,
            } if call.context == "switch:config" => {
                let value = response.map_err(candidate_rpc_error)?;
                let options = candidate
                    .state
                    .options
                    .confirmed(option_id, desired, &value)
                    .map_err(native_error)?;
                super::configuration::confirmed(&mut candidate.state, options, option_id);
                if remaining.is_empty() {
                    return self.begin_continuation_or_commit(pane, next_name, candidate);
                }
                self.send_next_candidate_option(candidate, remaining.clone())?;
                Ok(false)
            }
            CandidatePhase::Continuing {
                incorporated_pending,
            } if call.context == "switch:continuation" => {
                response.map_err(candidate_rpc_error)?;
                let incorporated_pending = incorporated_pending.clone();
                self.commit_switch_candidate(pane, next_name, candidate, &incorporated_pending)?;
                Ok(true)
            }
            _ => Err(errors::harness_unavailable(
                "The selected harness answered out of order",
            )),
        }
    }

    fn spawn_switch_candidate(
        &mut self,
        pane: &str,
        runtime: &HarnessSwitchRuntime,
    ) -> Result<Candidate> {
        let original = self.agent_state(pane)?;
        let checkpoint = errors::invariant(
            original.harness_switch.clone(),
            "Harness switch checkpoint disappeared",
        )?;
        let old_saved = self.panes[pane].stored.clone();
        let new_generation = old_saved
            .pane
            .generation
            .checked_add(1)
            .ok_or_else(|| errors::internal("Pane generation exhausted"))?;
        let mut saved = old_saved.clone();
        saved.pane.generation = new_generation;
        saved.instance_key = checkpoint.candidate_instance_key.clone();
        saved.instance_id = None;
        saved.supervisor_id = None;
        saved.pane.pid = None;
        saved.sequence_base = self.panes[pane].screen.next_offset();
        let mut state = State::new(runtime.launch.clone(), runtime.binary_digest.clone(), None)?;
        state.mode = original.mode.clone();
        state.internal_continuation = Some(InternalContinuation::default());
        state.binding = original.binding.clone();
        state.mcp_role = original.mcp_role.clone();
        state.mcp_enabled = original.mcp_enabled;
        state.permission_rules = original.permission_rules.clone();
        state.queue = original.queue.clone();
        state.queue_revision = original.queue_revision;
        state.queue_paused = checkpoint.previous_queue_paused;
        state.next_message_seq = original.next_message_seq;
        state.has_finished_turn = original.has_finished_turn;
        let spawn = Spawn {
            instance_key: checkpoint.candidate_instance_key,
            generation: new_generation,
            kind: ProcessKind::PipesAcp,
            cwd: saved.pane.cwd.clone(),
            argv: std::iter::once(runtime.launch.command.clone())
                .chain(runtime.launch.args.clone())
                .collect(),
            env: super::environment::launch_environment_with(runtime.launch.environment.as_deref()),
            cols: saved.pane.cols as u16,
            rows: saved.pane.rows as u16,
        };
        let Response::Instance(instance) = self.pty_request(Request::Spawn(spawn))? else {
            return Err(errors::ptyd_unavailable("Invalid candidate spawn response"));
        };
        Ok(Candidate {
            instance,
            saved,
            state,
            decoder: Decoder::new(0),
            raw_output: Vec::new(),
            phase: CandidatePhase::Initializing,
            default_mode: crate::settings::default_agent_mode(&self.store, &runtime.launch.id)?,
        })
    }

    fn initialize_switch_candidate(&mut self, candidate: &mut Candidate) -> Result<()> {
        let initialize = candidate
            .state
            .client
            .request(
                "initialize",
                soyeht_acp::client::initialize_params(),
                "switch:initialize",
                self.now_ms() as u64,
            )
            .map_err(native_error)?;
        self.write_candidate(&candidate.instance.instance_id, initialize)
    }

    fn send_next_candidate_option(
        &mut self,
        candidate: &mut Candidate,
        mut requested: Vec<(String, String)>,
    ) -> Result<()> {
        let (public_name, requested_value) = requested.remove(0);
        let option = super::configuration::requested_option(&candidate.state.options, &public_name)
            .cloned()
            .ok_or_else(|| {
                errors::harness_unavailable(format!(
                    "The selected harness does not offer {public_name}"
                ))
            })?;
        let desired = ConfigValue::V0(requested_value);
        let session_id = errors::invariant(
            candidate.state.session_id.as_deref(),
            "Candidate configuration has no session",
        )?;
        let (method, params) = candidate
            .state
            .options
            .request(session_id, &option.id, &desired)
            .map_err(native_error)?
            .ok_or_else(|| errors::internal("A requested switch option is not native"))?;
        let request = candidate
            .state
            .client
            .request(method, params, "switch:config", self.now_ms() as u64)
            .map_err(native_error)?;
        self.write_candidate(&candidate.instance.instance_id, request)?;
        candidate.phase = CandidatePhase::Configuring {
            option_id: option.id,
            desired,
            remaining: requested,
        };
        Ok(())
    }

    fn drain_candidate(
        &mut self,
        candidate: &mut Candidate,
    ) -> Result<
        Option<(
            soyeht_acp::client::Call,
            std::result::Result<Value, soyeht_acp::RpcError>,
        )>,
    > {
        let from = candidate.decoder.next_offset();
        let Response::Output(output) = self.pty_request(Request::Drain {
            instance_id: candidate.instance.instance_id.clone(),
            from_seq: from,
        })?
        else {
            return Err(errors::ptyd_unavailable(
                "Invalid candidate output response",
            ));
        };
        if output.from_seq != from || output.gap.is_some() {
            return Err(errors::ptyd_unavailable(
                "Candidate output is discontinuous",
            ));
        }
        let bytes = STANDARD
            .decode(output.bytes)
            .map_err(|_| errors::ptyd_unavailable("Invalid candidate output encoding"))?;
        if output.next_seq != output.from_seq + bytes.len() as u64 {
            return Err(errors::ptyd_unavailable(
                "Candidate output offsets disagree",
            ));
        }
        let mut response = None;
        if !bytes.is_empty() {
            candidate.raw_output.extend_from_slice(&bytes);
            for record in
                decode(&mut candidate.decoder, output.from_seq, &bytes).map_err(native_error)?
            {
                match record.message {
                    message @ Message::Response { .. } if response.is_none() => {
                        response = Some(
                            candidate
                                .state
                                .client
                                .response(message)
                                .map_err(native_error)?,
                        );
                    }
                    Message::Notification { .. } => {}
                    Message::Request { .. } | Message::Response { .. } => {
                        return Err(errors::harness_unavailable(
                            "The selected harness sent unexpected startup traffic",
                        ));
                    }
                }
            }
        }
        if output.exited {
            return Err(errors::harness_unavailable(
                "The selected harness exited during startup",
            ));
        }
        Ok(response)
    }

    fn write_candidate(&mut self, instance_id: &str, message: Message) -> Result<()> {
        let encoded = message.encode().map_err(native_error)?;
        let mut offset = 0;
        while offset < encoded.len() {
            let end = (offset + limits::TERMINAL_PAYLOAD_BYTES).min(encoded.len());
            let bytes = &encoded[offset..end];
            let operation_key = format!("switch-input:{}", Uuid::new_v4());
            let Response::Accepted { bytes: accepted } = self.pty_request(Request::Input {
                instance_id: instance_id.into(),
                bytes: STANDARD.encode(bytes),
                operation_key: operation_key.clone(),
                fingerprint: format!("{:x}", Sha256::digest(bytes)),
            })?
            else {
                return Err(errors::ptyd_unavailable("Invalid candidate input receipt"));
            };
            self.pty_request(Request::ConfirmInput { operation_key })?;
            if accepted == 0 || accepted > bytes.len() {
                return Err(errors::ptyd_unavailable(
                    "Candidate did not accept its complete input",
                ));
            }
            offset += accepted;
        }
        Ok(())
    }

    fn begin_continuation_or_commit(
        &mut self,
        pane: &str,
        next_name: &str,
        candidate: &mut Candidate,
    ) -> Result<bool> {
        let original = self.agent_state(pane)?;
        let Some((package, included_messages)) =
            self.continuation_package(pane, &original, next_name)?
        else {
            self.commit_switch_candidate(pane, next_name, candidate, &[])?;
            return Ok(true);
        };
        if let Some(continuation) = candidate.state.internal_continuation.as_mut() {
            continuation.record_prompt(&package);
        }
        let session_id = errors::invariant(
            candidate.state.session_id.as_deref(),
            "Candidate continuation has no session",
        )?;
        let request = candidate
            .state
            .client
            .request(
                "session/prompt",
                soyeht_acp::client::prompt(session_id, &package),
                "switch:continuation",
                self.now_ms() as u64,
            )
            .map_err(native_error)?;
        self.write_candidate(&candidate.instance.instance_id, request)?;
        let incorporated_pending = original
            .queue
            .iter()
            .rev()
            .take(included_messages.min(original.queue.len()))
            .map(|prompt| prompt.prompt_id.clone())
            .collect();
        candidate.phase = CandidatePhase::Continuing {
            incorporated_pending,
        };
        Ok(false)
    }

    fn commit_switch_candidate(
        &mut self,
        pane: &str,
        next_name: &str,
        candidate: &mut Candidate,
        incorporated_pending: &[String],
    ) -> Result<()> {
        let original = self.agent_state(pane)?;
        let checkpoint = errors::invariant(
            original.harness_switch.clone(),
            "Harness switch checkpoint disappeared before commit",
        )?;
        let old_saved = self.panes[pane].stored.clone();
        let session_id = errors::invariant(
            candidate.state.session_id.clone(),
            "Candidate session did not open",
        )?;
        candidate.state.queue = original.queue.clone();
        candidate.state.queue_revision = original.queue_revision;
        for prompt_id in incorporated_pending {
            candidate.state.remove(prompt_id)?;
        }
        candidate.state.harness_switch = None;
        candidate.state.requested_options.clear();
        candidate.state.acknowledged_offset = candidate.decoder.next_offset();
        candidate.state.phase = Phase::Ready;
        candidate.saved.instance_id = Some(candidate.instance.instance_id.clone());
        candidate.saved.supervisor_id = Some(candidate.instance.supervisor_id.clone());
        candidate.saved.pane.pid = Some(candidate.instance.pid);
        candidate.saved.pane.source = StateSource::Protocol;
        candidate.saved.pane.confidence = 1.0;
        candidate.saved.pane.state = PaneState::Idle;
        candidate.saved.pane.phrase = format!("Restarted with {next_name}");
        // Keep a name the user chose, including one set while the switch ran;
        // a default name follows the harness.
        candidate.saved.pane.name =
            if naming::is_default_name(&old_saved.pane.name, &original.launch) {
                self.default_agent_name(
                    &old_saved.pane.workspace_id,
                    naming::harness_label(&candidate.state.launch),
                    Some(pane),
                )
            } else {
                old_saved.pane.name.clone()
            };
        if let Some(retired) = old_saved
            .instance_id
            .clone()
            .zip(old_saved.supervisor_id.clone())
            .map(|(instance_id, supervisor_id)| RetiredAgentProcess {
                instance_key: old_saved.instance_key.clone(),
                instance_id,
                supervisor_id,
            })
        {
            candidate.state.retired_processes.push(retired);
        }
        let marker_message =
            transcript::handover(&mut candidate.state, candidate.saved.pane.generation)?;
        let marker_seq = marker_message.seq.clone();
        let result = json!({
            "paneId":pane,"fromHarness":checkpoint.from_harness,
            "toHarness":checkpoint.to_harness,"newSessionId":session_id,"markerSeq":marker_seq,
        });
        let mut changes = Changes {
            completed_user: Some(Completion {
                user: checkpoint.user,
                result: Ok(result.clone()),
            }),
            retire_instance: old_saved
                .instance_id
                .clone()
                .zip(old_saved.supervisor_id.clone()),
            ..Changes::default()
        };
        // The generic pane.update event cannot carry TranscriptKind::Handover.
        // The client reloads the typed transcript on pane.harness_switch_done.
        changes.messages.push(marker_message);
        changes.events.push((
            "pane.harness_switch_done".into(),
            json!({"paneId":pane,"fromHarness":checkpoint.from_harness,
                "toHarness":checkpoint.to_harness,"newSessionId":session_id,"markerSeq":marker_seq}),
        ));
        inject_screen_advance_before_switch_commit(
            &self.directory,
            self.panes
                .get_mut(pane)
                .ok_or_else(|| errors::internal("Candidate pane disappeared before commit"))?,
        )?;
        // The original process can emit output while its replacement starts.
        // Anchor the candidate generation only after that output is ingested;
        // the base captured at spawn is necessarily stale in that case.
        candidate.saved.sequence_base = self.panes[pane].screen.next_offset();
        let candidate_base = candidate.saved.sequence_base;
        self.commit_agent(
            candidate.saved.clone(),
            candidate.state.clone(),
            changes,
            None,
            json!({}),
        )?;
        self.agents
            .get_mut(pane)
            .ok_or_else(|| errors::internal("Candidate runtime disappeared after commit"))?
            .decoder = std::mem::replace(&mut candidate.decoder, Decoder::new(0));
        if !candidate.raw_output.is_empty() {
            self.panes
                .get_mut(pane)
                .ok_or_else(|| errors::internal("Candidate pane disappeared after commit"))?
                .screen
                .append_data(candidate_base, std::mem::take(&mut candidate.raw_output))?;
        }
        let _ = self.pty_request(Request::Ack {
            instance_id: candidate.instance.instance_id.clone(),
            seq: self.agents[pane].state.acknowledged_offset,
        }); // Flow control only: a missed ack keeps output retained until the next one.
        Ok(())
    }

    fn continuation_package(
        &self,
        pane: &str,
        state: &State,
        next_name: &str,
    ) -> Result<Option<(String, usize)>> {
        let page = self
            .store
            .agent_transcript(pane, None, limits::TRANSCRIPT_PAGE_MESSAGES)
            .map_err(errors::storage)?;
        let mut owned: Vec<(ContinuationRole, continuation::State, String)> = page
            .items
            .into_iter()
            .filter(|message| message.complete && message.kind == TranscriptKind::Message)
            .filter_map(|message| match message.role {
                TranscriptRole::User => Some((
                    ContinuationRole::User,
                    continuation::State::Delivered,
                    message.text,
                )),
                TranscriptRole::Assistant => Some((
                    ContinuationRole::Assistant,
                    continuation::State::Delivered,
                    message.text,
                )),
                TranscriptRole::System => None,
            })
            .collect();
        owned.extend(state.queue.iter().map(|prompt| {
            (
                ContinuationRole::User,
                continuation::State::Pending,
                prompt.text.clone(),
            )
        }));
        if owned.is_empty() {
            return Ok(None);
        }
        let messages = owned
            .iter()
            .map(|(role, status, text)| ContinuationMessage {
                role: *role,
                state: *status,
                text,
            })
            .collect::<Vec<_>>();
        let task = crate::tasks::ports::TaskStorePort::active_task_for_pane(&self.store, pane)
            .map_err(errors::storage)?;
        continuation::build_with_receipt(
            CONTINUATION_TEMPLATE,
            Fields {
                previous_harness: &state.launch.name,
                new_harness: next_name,
                task_title: task.as_ref().map(|task| task.title.as_str()),
                worktree_path: Some(&self.panes[pane].stored.pane.cwd),
                branch: None,
                transcript_path: None,
                messages: &messages,
            },
        )
        .map(|built| Some((built.text, built.included_messages)))
        .map_err(native_error)
    }

    fn restore_failed_switch(&mut self, pane: &str, reason: &str) -> Result<()> {
        let mut state = self.agent_state(pane)?;
        let checkpoint = errors::invariant(
            state.harness_switch.take(),
            "Failed harness switch has no checkpoint",
        )?;
        state.queue_paused = checkpoint.previous_queue_paused;
        let mut saved = self.panes[pane].stored.clone();
        saved.pane.state = if state.active.is_some() {
            PaneState::Working
        } else {
            PaneState::Idle
        };
        saved.pane.phrase = "Original agent session restored".into();
        let target = if checkpoint.to_name.is_empty() {
            &checkpoint.to_harness
        } else {
            &checkpoint.to_name
        };
        let marker_message = transcript::new_message(
            &mut state,
            saved.pane.generation,
            TranscriptRole::System,
            TranscriptKind::System,
            format!("The switch to {target} failed. The original agent session is still running."),
            true,
        )?;
        let mut changes = Changes {
            completed_user: Some(Completion {
                user: checkpoint.user,
                result: Err(errors::wire(errors::harness_switch_failed(reason))),
            }),
            ..Changes::default()
        };
        transcript::put(&mut changes, pane, marker_message, "agent_message_chunk")?;
        changes.events.push((
            "pane.harness_switch_failed".into(),
            json!({"paneId":pane,"fromHarness":checkpoint.from_harness,
                "toHarness":checkpoint.to_harness,"reason":reason,"restored":true}),
        ));
        self.commit_agent(saved, state, changes, None, json!({}))?;
        Ok(())
    }

    fn retire_failed_candidate(&mut self, pane: &str, instance: &Instance) -> Result<()> {
        if self
            .pty_request(Request::Close {
                instance_id: instance.instance_id.clone(),
            })
            .is_err()
        {
            let mut state = self.agent_state(pane)?;
            state.retired_processes.push(RetiredAgentProcess {
                instance_key: instance.spawn.instance_key.clone(),
                instance_id: instance.instance_id.clone(),
                supervisor_id: instance.supervisor_id.clone(),
            });
            self.commit_agent(
                self.panes[pane].stored.clone(),
                state,
                Changes::default(),
                None,
                json!({}),
            )?;
        }
        Ok(())
    }

    pub(crate) fn recover_harness_switches(&mut self, instances: &[Instance]) -> Result<()> {
        let panes = self.agents.keys().cloned().collect::<Vec<_>>();
        for pane in panes {
            // Closing an agent is forbidden while a switch is pending, so a
            // closed pane has no switch to recover. Keep it out of the
            // live-pane accessor so one historical pane cannot abort startup
            // reconciliation for every live process.
            if self.panes.get(&pane).is_some_and(|pane| pane.stored.closed) {
                continue;
            }
            let state = self.agent_state(&pane)?;
            let Some(checkpoint) = state.harness_switch.clone() else {
                continue;
            };
            if let Some(candidate) = instances
                .iter()
                .find(|instance| {
                    instance.spawn.instance_key == checkpoint.candidate_instance_key
                        && !instance.closed
                })
                .cloned()
            {
                self.retire_failed_candidate(&pane, &candidate)?;
            }
            self.restore_failed_switch(&pane, "Engine restarted before the new harness committed")?;
        }
        Ok(())
    }

    pub(crate) fn cleanup_retired_agent_processes(&mut self) -> Result<()> {
        let panes = self.panes_with_retired_processes();
        for pane in panes {
            // Closed agent panes remain in the runtime map so their durable
            // process-retirement ledger can converge. `agent_state` rejects
            // closed panes by design, so cleanup must read this internal
            // state directly instead of turning one closed pane into a
            // poll-loop error that starves every pane after it.
            let Some(mut state) = self.agents.get(&pane).map(|agent| agent.state.clone()) else {
                continue;
            };
            let current_supervisor = self
                .ptyd
                .as_ref()
                .map(|value| value.supervisor_id().to_owned());
            let mut keep = Vec::new();
            for retired in &state.retired_processes {
                if current_supervisor.as_deref() != Some(&retired.supervisor_id) {
                    continue;
                }
                if should_fail_old_close(&self.directory) {
                    keep.push(retired.clone());
                    continue;
                }
                if self
                    .pty_request(Request::Close {
                        instance_id: retired.instance_id.clone(),
                    })
                    .is_err()
                {
                    keep.push(retired.clone());
                }
            }
            if keep.len() != state.retired_processes.len() {
                state.retired_processes = keep;
                self.commit_agent(
                    self.panes[&pane].stored.clone(),
                    state,
                    Changes::default(),
                    None,
                    json!({}),
                )?;
            }
        }
        Ok(())
    }
}

/// Model and effort the switch asked for, then the mode the pane keeps when
/// the new harness offers it.
fn requested_options(params: &PaneHarnessSwitchParams, state: &State) -> Vec<(String, String)> {
    [
        ("model", params.model.as_ref()),
        ("effort", params.effort.as_ref()),
        (
            super::configuration::MODE_KEY,
            state
                .requested_options
                .get(super::configuration::MODE_KEY)
                .and_then(|value| match value {
                    ConfigValue::V0(mode) => Some(mode),
                    ConfigValue::V1(_) => None,
                }),
        ),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.map(|value| (name.into(), value.clone())))
    .collect()
}

#[cfg(debug_assertions)]
fn inject_screen_advance_before_switch_commit(
    directory: &std::path::Path,
    pane: &mut crate::model::RunningPane,
) -> Result<()> {
    if directory
        .join("test-advance-screen-before-harness-switch-commit")
        .exists()
    {
        let from = pane.screen.next_offset();
        pane.screen.append(Record::Gap {
            from,
            to: from.saturating_add(1),
        })?;
    }
    Ok(())
}

#[cfg(not(debug_assertions))]
fn inject_screen_advance_before_switch_commit(
    _directory: &std::path::Path,
    _pane: &mut crate::model::RunningPane,
) -> Result<()> {
    Ok(())
}

fn candidate_rpc_error(error: soyeht_acp::RpcError) -> errors::Error {
    if soyeht_acp::rpc::is_auth_required(&error) {
        errors::harness_unavailable("The selected harness requires sign-in")
    } else {
        errors::harness_unavailable(error.message)
    }
}

fn bounded_reason(reason: &str) -> String {
    let mut end = reason.len().min(512);
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    reason[..end].to_owned()
}

#[cfg(debug_assertions)]
fn should_fail_old_close(directory: &std::path::Path) -> bool {
    directory
        .join("test-fail-harness-switch-old-close")
        .is_file()
}

#[cfg(not(debug_assertions))]
fn should_fail_old_close(_: &std::path::Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agents::{Running, state::State},
        model::{RunningPane, StoredPane},
        screen::Screen,
    };
    use soyeht_protocol::{HarnessLaunch, Pane, PaneMode, PaneState, StateSource};
    use std::fs;

    #[test]
    fn closed_agent_does_not_abort_switch_recovery_or_retired_cleanup() {
        let root = std::env::temp_dir().join(format!("soyeht-closed-agent-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut core = Core::open(&root).unwrap();
        let pane_id = "closed-agent".to_owned();
        let state = State::new(
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
        let pane = Pane {
            pane_id: pane_id.clone(),
            workspace_id: "workspace".into(),
            name: "Closed agent".into(),
            cwd: root.display().to_string(),
            mode: PaneMode::Agent,
            state: PaneState::Dead,
            source: StateSource::Protocol,
            confidence: 1.0,
            phrase: "Agent pane closed".into(),
            generation: 1,
            cols: 80,
            rows: 24,
            record: true,
            experiment_capture: None,
            pid: None,
            stream_id: None,
            agent: None,
            url: None,
            title: None,
            viewport: None,
            incognito: None,
            web_session: None,
            writing_kind: None,
            writing_section: None,
        };
        let screen = Screen::open(&root.join("sessions").join(&pane_id), true).unwrap();
        core.panes.insert(
            pane_id.clone(),
            RunningPane {
                stored: StoredPane {
                    pane,
                    closed: true,
                    instance_key: "closed-instance".into(),
                    instance_id: None,
                    supervisor_id: None,
                    sequence_base: 0,
                },
                screen,
            },
        );
        core.agents.insert(pane_id, Running::new(state));

        core.recover_harness_switches(&[]).unwrap();
        core.cleanup_retired_agent_processes().unwrap();
        crate::test_paths::remove_after(core, root);
    }
}
