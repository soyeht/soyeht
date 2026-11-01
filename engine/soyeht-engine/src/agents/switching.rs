//! Atomic replacement of one live ACP harness with another.
//!
//! The original process and checkpoint remain authoritative while a candidate
//! initializes beside it. Only a candidate that has opened a native session,
//! accepted requested configuration, and accepted the bounded continuation
//! package may replace the pane's durable runtime.

use super::{
    Changes,
    harness_sign_in::{self, SignInChange},
    naming, native_error,
    pending::{Completion, UserOperation},
    state::{HarnessSwitchCheckpoint, InternalContinuation, Phase, RetiredAgentProcess, State},
    transcript,
};
use crate::supervisor_handle::Supervisor;
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
use soyeht_platform::log::{self, Failure as _};
use soyeht_protocol::{
    ConfigValue, PaneHarnessSwitchParams, PaneState, StateSource, TranscriptKind, TranscriptRole,
    event, limits, method::PANE_HARNESS_SWITCH, ndjson::Decoder,
};
use soyeht_ptyd::wire::{Instance, ProcessKind, Request, Response, Spawn};
use std::time::{Duration, Instant};
use uuid::Uuid;

mod candidate;
mod commit;

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
    /// The agent refused the sign-in remembered for its harness.
    sign_in_refused: bool,
    /// The request to write to its process in the next step, once.
    outgoing: Option<Message>,
}

/// A candidate whose process has not been started yet.
struct Draft {
    saved: crate::model::StoredPane,
    state: State,
    default_mode: Option<String>,
    initialize: Message,
}

/// The job of the step of the switch of `pane`.
fn step_job(pane: &str) -> String {
    format!("switch:{pane}")
}

enum CandidatePhase {
    Initializing,
    /// Answering `authenticate` with the method the person chose for this harness.
    Authenticating,
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
        if let Some(result) = self.repeated_mutation(connection, PANE_HARNESS_SWITCH, raw)? {
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
                event::PANE_QUEUE_PAUSED.into(),
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
            event::PANE_HARNESS_SWITCH_STARTED.into(),
            json!({"paneId":pane,"fromHarness":checkpoint.from_harness,
                "toHarness":checkpoint.to_harness}),
        ));
        let pending = json!({"_nativePending":checkpoint.candidate_instance_key});
        let result = self
            .commit_agent(
                saved,
                state,
                changes,
                Some((connection, PANE_HARNESS_SWITCH, raw)),
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

    /// The tick's part for the switches: starts the next step of each, which waits for the
    /// supervisor off the Core lock (ADR 0126) and is applied by a later tick. A switch whose step
    /// is on its way waits for it. A step that fails ends the switch and restores the original.
    pub(crate) fn advance_harness_switches(&mut self) -> Result<()> {
        let panes = self.harness_switches.keys().cloned().collect::<Vec<_>>();
        for pane in panes {
            if !self.background.accepts(&step_job(&pane)) {
                continue;
            }
            if let Err(error) = self.begin_switch_step(&pane) {
                self.fail_switch(&pane, error)?;
            }
        }
        Ok(())
    }

    fn begin_switch_step(&mut self, pane: &str) -> Result<()> {
        let runtime = errors::invariant(
            self.harness_switches.get(pane),
            "Harness switch runtime disappeared",
        )?;
        if Instant::now() >= runtime.deadline {
            return Err(errors::harness_unavailable(
                "The selected harness did not become ready in time",
            ));
        }
        if runtime.candidate.is_some() {
            return self.begin_candidate_read(pane);
        }
        if self.agent_state(pane)?.active.is_some() {
            return Ok(());
        }
        let (launch, digest) = (runtime.launch.clone(), runtime.binary_digest.clone());
        let (draft, spawn) = self.draft_switch_candidate(pane, &launch, &digest)?;
        self.begin_candidate_spawn(pane, draft, spawn)
    }

    /// Starts the candidate's process and writes its `initialize`, off the lock.
    fn begin_candidate_spawn(&mut self, pane: &str, draft: Draft, spawn: Spawn) -> Result<()> {
        let supervisor = self.supervisor()?;
        let used = supervisor.clone();
        let (owner, initialize) = (pane.to_owned(), draft.initialize.clone());
        self.spawn_prompt_background(
            step_job(pane),
            move || {
                let Response::Instance(instance) = supervisor.call(Request::Spawn(spawn))? else {
                    return Err(errors::ptyd_unavailable("Invalid candidate spawn response"));
                };
                let (keys, written) =
                    candidate::write_candidate(&supervisor, &instance.instance_id, &initialize);
                Ok((instance, keys, written))
            },
            move |core, started| {
                let result = core.finish_candidate_spawn(&owner, &used, draft, started);
                core.switch_step_ended(&owner, result)
            },
            |_| true,
        )
    }

    fn finish_candidate_spawn(
        &mut self,
        pane: &str,
        used: &Supervisor,
        draft: Draft,
        started: Result<(Instance, Vec<String>, Result<()>)>,
    ) -> Result<()> {
        let Some((instance, keys, written)) = self.settle_if_current(used, started)? else {
            return Ok(());
        };
        let Draft {
            saved,
            state,
            default_mode,
            ..
        } = draft;
        let candidate = Candidate {
            instance,
            saved,
            state,
            decoder: Decoder::new(0),
            raw_output: Vec::new(),
            phase: CandidatePhase::Initializing,
            default_mode,
            sign_in_refused: false,
            outgoing: None,
        };
        // The candidate is recorded first: a failure from here on retires its process.
        let runtime = errors::invariant(
            self.harness_switches.get_mut(pane),
            "Harness switch runtime disappeared",
        )?;
        runtime.candidate = Some(candidate);
        self.confirm_candidate_writes(keys);
        self.settle_supervisor(used, written)
    }

    /// Writes what the last step queued and reads the candidate's output, off the lock.
    fn begin_candidate_read(&mut self, pane: &str) -> Result<()> {
        let supervisor = self.supervisor()?;
        let used = supervisor.clone();
        let candidate = errors::invariant(
            self.harness_switches
                .get(pane)
                .and_then(|runtime| runtime.candidate.as_ref()),
            "Harness switch candidate disappeared",
        )?;
        let (instance_id, from) = (
            candidate.instance.instance_id.clone(),
            candidate.decoder.next_offset(),
        );
        let outgoing = candidate.outgoing.clone();
        let owner = pane.to_owned();
        self.spawn_prompt_background(
            step_job(pane),
            move || {
                let (keys, written) = match &outgoing {
                    Some(message) => candidate::write_candidate(&supervisor, &instance_id, message),
                    None => (Vec::new(), Ok(())),
                };
                let output = written
                    .and_then(|()| candidate::read_candidate(&supervisor, &instance_id, from));
                Ok((keys, output))
            },
            move |core, read| {
                let result = core.finish_candidate_read(&owner, &used, read);
                core.switch_step_ended(&owner, result)
            },
            |_| true,
        )
    }

    /// Takes the candidate's answer under the lock: decodes it and takes the next step of the
    /// handshake, or commits the switch.
    fn finish_candidate_read(
        &mut self,
        pane: &str,
        used: &Supervisor,
        read: Result<(Vec<String>, Result<soyeht_ptyd::wire::Output>)>,
    ) -> Result<()> {
        // A write that was started and then failed or timed out is not repeated: its pages may
        // have reached the candidate, so the switch ends instead.
        let (keys, output) = self.settle_supervisor(used, read)?;
        self.confirm_candidate_writes(keys);
        let output = self.settle_supervisor(used, output)?;
        let Some(mut runtime) = self.harness_switches.remove(pane) else {
            return Ok(());
        };
        let next_name = runtime.launch.name.clone();
        let params = runtime.params.clone();
        let advanced = match runtime.candidate.as_mut() {
            Some(candidate) => {
                candidate.outgoing = None;
                self.decode_candidate_output(candidate, output).and_then(
                    |response| match response {
                        None => Ok(false),
                        Some((call, response)) => self.advance_candidate_response(
                            pane, &next_name, &params, candidate, call, response,
                        ),
                    },
                )
            }
            None => Err(errors::internal("Harness switch candidate disappeared")),
        };
        match advanced {
            Ok(true) => Ok(()),
            Ok(false) => {
                self.harness_switches.insert(pane.into(), runtime);
                Ok(())
            }
            Err(error) => {
                self.harness_switches.insert(pane.into(), runtime);
                Err(error)
            }
        }
    }

    /// Ends the step of a switch: a step that failed ends the switch.
    fn switch_step_ended(&mut self, pane: &str, result: Result<()>) -> Result<()> {
        match result {
            Ok(()) => Ok(()),
            Err(error) => self.fail_switch(pane, error),
        }
    }

    /// Confirms the receipts of the writes to the candidate. Nobody waits for it: an unconfirmed
    /// receipt is reconciled when the supervisor returns.
    fn confirm_candidate_writes(&mut self, keys: Vec<String>) {
        for operation_key in keys {
            self.send_later(Request::ConfirmInput { operation_key });
        }
    }

    /// Ends the switch of `pane` because a step failed: its candidate is retired and the original
    /// session is restored.
    fn fail_switch(&mut self, pane: &str, error: errors::Error) -> Result<()> {
        let Some(mut runtime) = self.harness_switches.remove(pane) else {
            return Ok(());
        };
        let mut forget = None;
        if let Some(candidate) = runtime.candidate.take() {
            self.retire_failed_candidate(pane, &candidate.instance)?;
            if candidate.sign_in_refused {
                forget = Some(SignInChange::forget(&candidate.state.launch.id));
            }
        }
        let reason = bounded_reason(&error.to_string());
        self.restore_failed_switch(pane, &reason, forget)
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
            self.restore_failed_switch(
                &pane,
                "Engine restarted before the new harness committed",
                None,
            )?;
        }
        Ok(())
    }

    /// Closes the processes the agents retired and forgets them once closed. The closes wait for
    /// the supervisor off the Core lock, one job per agent (ADR 0119); a close that fails leaves
    /// the process in the ledger for the next tick.
    pub(crate) fn cleanup_retired_agent_processes(&mut self) -> Result<()> {
        let panes = self.panes_with_retired_processes();
        for pane in panes {
            let job = format!("retired:{pane}");
            let now = self.now_ms();
            // Not while the job runs, nor within the delay a refusal left.
            if !self.background.accepts(&job)
                || self
                    .agents
                    .get(&pane)
                    .is_none_or(|agent| !agent.idle.retired_close_due(now))
            {
                continue;
            }
            // Closed agent panes remain in the runtime map so their durable
            // process-retirement ledger can converge. `agent_state` rejects
            // closed panes by design, so cleanup must read this internal
            // state directly instead of turning one closed pane into a
            // poll-loop error that starves every pane after it.
            let Some(retired) = self
                .agents
                .get(&pane)
                .map(|agent| agent.state.retired_processes.clone())
            else {
                continue;
            };
            let current_supervisor = self
                .ptyd
                .as_ref()
                .map(|value| value.supervisor_id().to_owned());
            let (current, stale): (Vec<_>, Vec<_>) = retired
                .into_iter()
                .partition(|retired| current_supervisor.as_deref() == Some(&retired.supervisor_id));
            // A process of another supervisor is gone with it.
            let stale: Vec<String> = stale
                .into_iter()
                .map(|retired| retired.instance_id)
                .collect();
            if !stale.is_empty() {
                self.forget_retired(&pane, &stale)?;
            }
            let closing: Vec<String> = current
                .into_iter()
                .filter(|_| !self.hooks.fail_old_close())
                .map(|retired| retired.instance_id)
                .collect();
            if closing.is_empty() {
                continue;
            }
            let supervisor = self.supervisor()?;
            let used = supervisor.clone();
            let turn = self.turn(&closing.iter().map(String::as_str).collect::<Vec<_>>());
            self.spawn_background(
                job,
                move || {
                    turn.wait();
                    let mut closed = Vec::new();
                    let mut refused = Ok(());
                    for instance_id in closing {
                        match supervisor.call(Request::Close {
                            instance_id: instance_id.clone(),
                        }) {
                            Ok(_) => closed.push(instance_id),
                            Err(error) if errors::supervisor_link_lost(&error) => {
                                return Err(error);
                            }
                            // The process stays in the ledger; a later tick closes it again.
                            Err(error) => refused = refused.and(Err(error)),
                        }
                    }
                    Ok((closed, refused))
                },
                move |core, closed| {
                    let (closed, refused) = core.settle_supervisor(&used, closed)?;
                    core.note_retired_closes(&pane, refused);
                    core.forget_retired(&pane, &closed)
                },
            )?;
        }
        Ok(())
    }

    /// Settles what the supervisor said to the closes of `pane`'s retired processes: a refusal
    /// delays the next try, and is logged once per change of kind.
    fn note_retired_closes(&mut self, pane: &str, refused: Result<()>) {
        let now = self.now_ms();
        let Some(agent) = self.agents.get_mut(pane) else {
            return;
        };
        match refused {
            Ok(()) => agent.idle.retired_close_done(),
            Err(error) => {
                if agent.idle.retired_close_refused(now, &error.failure_kind()) {
                    log::failure(CLOSE_REFUSED, &error);
                }
            }
        }
    }

    /// Puts `processes` in the retirement ledger of `pane`, unless they are in it already.
    pub(crate) fn retire_processes(
        &mut self,
        pane: &str,
        processes: Vec<RetiredAgentProcess>,
    ) -> Result<()> {
        self.change_ledger(pane, |ledger| {
            for process in processes {
                if ledger
                    .iter()
                    .all(|retired| retired.instance_id != process.instance_id)
                {
                    ledger.push(process);
                }
            }
        })
    }

    /// Takes the processes `instance_ids` out of the retirement ledger of `pane`.
    fn forget_retired(&mut self, pane: &str, instance_ids: &[String]) -> Result<()> {
        self.change_ledger(pane, |ledger| {
            ledger.retain(|retired| !instance_ids.contains(&retired.instance_id));
        })
    }

    /// Applies `change` to the retirement ledger of `pane` and commits it if it grew or shrank.
    fn change_ledger(
        &mut self,
        pane: &str,
        change: impl FnOnce(&mut Vec<RetiredAgentProcess>),
    ) -> Result<()> {
        let Some(mut state) = self.agents.get(pane).map(|agent| agent.state.clone()) else {
            return Ok(());
        };
        let before = state.retired_processes.len();
        change(&mut state.retired_processes);
        if state.retired_processes.len() == before {
            return Ok(());
        }
        let saved = self.panes[pane].stored.clone();
        self.commit_agent(saved, state, Changes::default(), None, json!({}))
            .map(drop)
    }
}

/// The log event of a close of a retired process that the supervisor refused.
const CLOSE_REFUSED: &str = "engine.retired_close_failed";

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{pending::UserOperation, state::HarnessSwitchCheckpoint};
    use crate::{
        agents::{Running, state::State},
        model::{RunningPane, StoredPane},
        screen::Screen,
    };
    use soyeht_protocol::{HarnessLaunch, Pane, PaneMode, PaneState};
    use std::fs;

    #[test]
    fn a_switch_step_waits_for_the_supervisor_off_the_lock() {
        use crate::test_panes::{Answering, apply_until_done, core_with_agent, quiet_read};
        let (mut core, _directory) = core_with_agent(&[]);
        core.background = crate::offload::Background::default();
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = std::sync::Arc::clone(&asked);
        core.link_supervisor(Box::new(Answering(move |request| {
            Ok(match request {
                Request::Spawn(spawn) => {
                    record.lock().unwrap().push("spawn");
                    // A supervisor that takes its time to start the candidate.
                    std::thread::sleep(Duration::from_millis(400));
                    Response::Instance(Instance {
                        instance_id: "candidate".into(),
                        supervisor_id: "answering".into(),
                        spawn,
                        pid: 7,
                        closed: false,
                        exit_code: None,
                    })
                }
                Request::Input { bytes, .. } => {
                    record.lock().unwrap().push("initialize");
                    Response::Accepted {
                        bytes: STANDARD.decode(bytes).unwrap().len(),
                    }
                }
                Request::Drain { from_seq, .. } => quiet_read(from_seq),
                _ => Response::Ok,
            })
        })));
        let mut state = core.agents["agent"].state.clone();
        state.harness_switch = Some(HarnessSwitchCheckpoint {
            user: UserOperation {
                scope: "scope".into(),
                key: "key".into(),
            },
            from_harness: "test".into(),
            to_harness: "other".into(),
            to_name: "Other".into(),
            candidate_instance_key: "candidate-key".into(),
            previous_queue_paused: false,
        });
        core.agents.get_mut("agent").unwrap().state = state;
        core.harness_switches.insert(
            "agent".into(),
            HarnessSwitchRuntime {
                params: PaneHarnessSwitchParams {
                    pane_id: "agent".into(),
                    harness: "other".into(),
                    model: None,
                    effort: None,
                    idempotency_key: "key".into(),
                },
                launch: HarnessLaunch {
                    id: "other".into(),
                    name: "Other".into(),
                    command: "/other".into(),
                    args: Vec::new(),
                    version: "1".into(),
                    environment: None,
                },
                binary_digest: "digest".into(),
                candidate: None,
                deadline: Instant::now() + SWITCH_TIMEOUT,
            },
        );

        let began = Instant::now();
        core.advance_harness_switches().unwrap();
        // A step on its way is not started again.
        core.advance_harness_switches().unwrap();
        assert!(
            began.elapsed() < Duration::from_millis(250),
            "the switch waited for the supervisor: {:?}",
            began.elapsed()
        );
        assert!(core.harness_switches["agent"].candidate.is_none());

        apply_until_done(&mut core, "switch:agent");
        let candidate = core.harness_switches["agent"].candidate.as_ref().unwrap();
        assert_eq!(candidate.instance.instance_id, "candidate");
        assert_eq!(*asked.lock().unwrap(), ["spawn", "initialize"]);
    }

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
            name: "Closed agent".into(),
            state: PaneState::Dead,
            phrase: "Agent pane closed".into(),
            record: true,
            ..crate::test_panes::pane(
                &pane_id,
                "workspace",
                PaneMode::Agent,
                &root.display().to_string(),
            )
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
