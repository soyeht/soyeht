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
                if self.hooks.fail_old_close() {
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
    use crate::{
        agents::{Running, state::State},
        model::{RunningPane, StoredPane},
        screen::Screen,
    };
    use soyeht_protocol::{HarnessLaunch, Pane, PaneMode, PaneState};
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
