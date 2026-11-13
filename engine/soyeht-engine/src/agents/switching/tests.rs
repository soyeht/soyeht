use super::*;
use crate::agents::{pending::UserOperation, state::HarnessSwitchCheckpoint};
use crate::{
    agents::state::State,
    model::{RunningPane, StoredPane},
    screen::Screen,
};
use soyeht_protocol::{HarnessLaunch, Pane, PaneMode, PaneState};
use std::fs;

/// Records a switch of the test agent to "other" that must be ready by `deadline`.
fn arm_switch(core: &mut Core, deadline: Instant) {
    let mut state = core.agents.running["agent"].state.clone();
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
    core.agents.running.get_mut("agent").unwrap().state = state;
    core.agents.switches.insert(
        "agent".into(),
        HarnessSwitchRuntime {
            params: PaneHarnessSwitchParams {
                pane_id: "agent".into(),
                harness: "other".into(),
                model: None,
                effort: None,
                acknowledged_package: None,
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
            deadline,
        },
    );
}

/// A candidate process of `saved`, running the session `state` in `phase`.
fn candidate_with(saved: StoredPane, state: State, phase: CandidatePhase) -> Candidate {
    Candidate {
        instance: Instance {
            instance_id: "candidate".into(),
            supervisor_id: "answering".into(),
            spawn: Spawn {
                instance_key: "candidate-key".into(),
                generation: 2,
                kind: ProcessKind::PipesAcp,
                cwd: String::new(),
                argv: Vec::new(),
                env: Default::default(),
                cols: 80,
                rows: 24,
            },
            pid: 7,
            closed: false,
            exit_code: None,
        },
        saved,
        state,
        decoder: Decoder::new(0),
        raw_output: Vec::new(),
        phase,
        default_mode: None,
        sign_in_refused: false,
        outgoing: None,
    }
}

#[test]
fn a_switch_step_waits_for_the_supervisor_off_the_lock() {
    use crate::test_panes::{Answering, apply_until_done, core_with_agent, quiet_read};
    let (mut core, _directory) = core_with_agent(&[]);
    core.background = crate::offload::Background::default();
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = std::sync::Arc::clone(&asked);
    // The supervisor answers the spawn only once the test lets it. A step that waited for it
    // under the lock would never see the release, which no load on the host can imitate.
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let was_released = std::sync::Arc::clone(&released);
    core.link_supervisor(Box::new(Answering(move |request| {
        Ok(match request {
            Request::Spawn(spawn) => {
                record.lock().unwrap().push("spawn");
                was_released.store(
                    gate.recv_timeout(Duration::from_secs(30)).is_ok(),
                    std::sync::atomic::Ordering::SeqCst,
                );
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
    arm_switch(&mut core, Instant::now() + SWITCH_TIMEOUT);

    core.advance_harness_switches().unwrap();
    // A step on its way is not started again.
    core.advance_harness_switches().unwrap();
    // Both ticks returned while the supervisor was still holding the spawn.
    assert!(core.agents.switches["agent"].candidate.is_none());
    assert!(core.background.is_active("switch:agent"));

    release.send(()).unwrap();
    apply_until_done(&mut core, "switch:agent");
    assert!(
        released.load(std::sync::atomic::Ordering::SeqCst),
        "the switch waited for the supervisor"
    );
    let candidate = core.agents.switches["agent"].candidate.as_ref().unwrap();
    assert_eq!(candidate.instance.instance_id, "candidate");
    assert_eq!(*asked.lock().unwrap(), ["spawn", "initialize"]);
}

#[test]
fn a_switch_past_its_deadline_restores_the_pane_and_retires_the_candidate() {
    let (mut core, _directory) = crate::test_panes::core_with_agent(&[]);
    let past = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("the clock has run for a second");
    arm_switch(&mut core, past);
    // The request that started the switch is waiting for its result.
    StorePort::mutate(&mut core.store, "scope", "key", "switch", |_| {
        Ok((json!({"_nativePending": "candidate-key"}), Vec::new()))
    })
    .unwrap();
    let candidate = candidate_with(
        core.panes["agent"].stored.clone(),
        core.agents.running["agent"].state.clone(),
        CandidatePhase::Initializing,
    );
    core.agents.switches.get_mut("agent").unwrap().candidate = Some(candidate);

    core.advance_harness_switches().unwrap();

    assert!(core.agents.switches.is_empty());
    let state = &core.agents.running["agent"].state;
    assert!(state.harness_switch.is_none());
    let retired = &state.retired_processes;
    assert_eq!(retired.len(), 1);
    assert_eq!(retired[0].instance_id, "candidate");
    let pane = &core.panes["agent"].stored.pane;
    assert_eq!(pane.state, PaneState::Idle);
    assert_eq!(pane.phrase, "Original agent session restored");
    let failed = core
        .store
        .replay(0)
        .unwrap()
        .1
        .into_iter()
        .filter(|event| event.method == event::PANE_HARNESS_SWITCH_FAILED)
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].params["restored"], true);
    assert_eq!(failed[0].params["toHarness"], "other");
    assert!(
        failed[0].params["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("did not become ready in time"))
    );
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
    core.agents.adopt(pane_id, state);

    core.recover_harness_switches(&[]).unwrap();
    core.cleanup_retired_agent_processes().unwrap();
    crate::test_paths::remove_after(core, root);
}

/// A candidate session of `harness` in `current`, opened under the ceiling a Claude agent in
/// `chief_mode` puts on the pane.
fn candidate_under_ceiling(harness: &str, current: &str, chief_mode: &str) -> Candidate {
    use crate::test_panes::{CLAUDE_MODES, CODEX_MODES, agent_with_modes};
    let chief = agent_with_modes("claude", chief_mode, &CLAUDE_MODES);
    let mut state = agent_with_modes(harness, current, &CODEX_MODES);
    state.session_id = Some("candidate-session".into());
    state.mode_ceiling = Some(crate::agents::mode_ceiling::ModeCeiling::of(&chief, false));
    state.mode_ceiling.as_mut().unwrap().reopened();
    candidate_with(
        crate::model::StoredPane {
            pane: crate::test_panes::pane("agent", "workspace", PaneMode::Agent, "/"),
            closed: false,
            instance_key: "candidate-key".into(),
            instance_id: None,
            supervisor_id: None,
            sequence_base: 0,
        },
        state,
        CandidatePhase::Opening,
    )
}

#[test]
fn a_candidate_above_the_ceiling_is_moved_down_before_the_continuation_prompt() {
    let (mut core, _directory) = crate::test_panes::core_with_agent(&[]);
    // Codex opens in `agent`, which ranks with the chief's `auto`: not within the ceiling.
    let mut candidate = candidate_under_ceiling("codex", "agent", "auto");
    let sent = core
        .begin_continuation_or_commit("agent", "Codex", &mut candidate)
        .unwrap();
    assert!(!sent || candidate.outgoing.is_none());
    let Some(Message::Request { method, params, .. }) = &candidate.outgoing else {
        panic!("the candidate was not asked for a lower mode");
    };
    assert_ne!(method, "session/prompt", "no turn ran before the check");
    assert!(params.to_string().contains("read-only"), "{params}");
    assert!(matches!(
        candidate.phase,
        CandidatePhase::Configuring { .. }
    ));
}

#[test]
fn a_candidate_within_the_ceiling_goes_on_and_one_with_no_mode_within_it_ends_the_switch() {
    let (mut core, _directory) = crate::test_panes::core_with_agent(&[]);
    let mut within = candidate_under_ceiling("codex", "read-only", "auto");
    assert!(core.hold_candidate_ceiling(&mut within).unwrap());
    assert!(within.outgoing.is_none());
    // Codex has no plan mode and nothing below it: a plan ceiling leaves it no mode.
    let mut none = candidate_under_ceiling("codex", "agent", "plan");
    let error = core
        .begin_continuation_or_commit("agent", "Codex", &mut none)
        .unwrap_err();
    assert_eq!(
        errors::kind(&error),
        soyeht_protocol::ErrorKind::HarnessUnavailable
    );
    assert!(none.outgoing.is_none(), "nothing was sent to the harness");
}

#[test]
fn a_candidate_that_stays_above_the_ceiling_after_the_correction_ends_the_switch() {
    let (mut core, _directory) = crate::test_panes::core_with_agent(&[]);
    let mut candidate = candidate_under_ceiling("codex", "agent", "auto");
    assert!(!core.hold_candidate_ceiling(&mut candidate).unwrap());
    // The harness confirmed but still runs `agent`: the correction is not tried twice.
    assert!(core.hold_candidate_ceiling(&mut candidate).is_err());
}
