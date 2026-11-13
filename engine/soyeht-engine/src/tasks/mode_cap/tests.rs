use super::*;
use crate::{
    agents::mode_ceiling::ModeCeiling,
    outbox,
    ports::StorePort,
    tasks::rules::tests::task_fixture,
    test_panes::{
        CLAUDE_MODES, CODEX_MODES, agent_with_modes, core_with_workspace, insert_agent_pane,
    },
};
use serde_json::json;
use soyeht_protocol::{ConfigValue, ErrorKind, Role, TaskState, method};

const WORKSPACE: &str = "workspace-1";

fn modes(harness: &str) -> &'static [(&'static str, &'static str)] {
    if harness == "claude" {
        &CLAUDE_MODES
    } else {
        &CODEX_MODES
    }
}

/// The state of a ready agent of `harness` in `current`.
fn agent(harness: &str, current: &str) -> crate::agents::state::State {
    agent_with_modes(harness, current, modes(harness))
}

fn executor_state(rig: &Rig) -> &crate::agents::state::State {
    &rig.core.agents.running["executor"].state
}

fn announce(core: &mut Core, directory: &std::path::Path, pane: &str, harness: &str, mode: &str) {
    insert_agent_pane(core, directory, pane, WORKSPACE);
    if let Some(running) = core.agents.running.get_mut(pane) {
        running.state = agent(harness, mode);
    }
}

const FIXED_PANES: [&str; 3] = ["chief", "claude-pane", "codex-pane"];

struct Rig {
    core: Core,
    directory: std::path::PathBuf,
    _outbox: Vec<outbox::Receiver>,
}

impl Rig {
    /// A core with a task to dispatch, an agent pane `chief` of `harness` in `mode`, and a
    /// running pane of each harness, which is how the engine knows the modes a harness offers.
    fn new(harness: &str, mode: &str) -> Self {
        let (mut core, directory) = core_with_workspace(WORKSPACE);
        let mut task = task_fixture(TaskState::Todo);
        task.owner_pane_id = None;
        task.task_id = "SEED-1".into();
        StorePort::mutate(&mut core.store, "test", "seed", "seed", |change| {
            change.insert_task(&task)?;
            Ok((json!({}), Vec::new()))
        })
        .unwrap();
        announce(&mut core, &directory, "chief", harness, mode);
        announce(&mut core, &directory, "claude-pane", "claude", "default");
        announce(&mut core, &directory, "codex-pane", "codex", "read-only");
        let entry =
            |id: &str| json!({"id":id,"name":id,"command":"/bin/ls","args":[],"version":"1"});
        std::fs::write(
            directory.join("harnesses.json"),
            json!([entry("claude"), entry("codex"), entry("stub")]).to_string(),
        )
        .unwrap();
        Self {
            core,
            directory,
            _outbox: Vec::new(),
        }
    }

    fn actor(role: Role, pane: Option<&str>) -> Actor {
        Actor {
            role,
            client: "mode-cap-test".into(),
            device_id: None,
            pane_id: pane.map(Into::into),
        }
    }

    fn connect(&mut self, role: Role, pane: Option<&str>) -> String {
        let (sender, receiver) = outbox::channel();
        self._outbox.push(receiver);
        let connection = self.core.connect(true, false, sender).unwrap();
        // The hello gives the connection its idempotency scope; the actor is then the test's.
        let hello = json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{
            "client":"mode-cap-test","clientVersion":"1","protocolVersion":1}});
        self.core.dispatch(&connection, hello).unwrap();
        self.core.clients.get_mut(&connection).unwrap().actor = Some(Self::actor(role, pane));
        connection
    }

    /// Whether the chief may start an executor of `harness` in `mode`.
    fn allows(&self, harness: &str, mode: &str) -> bool {
        let mut preset = TaskExecutor {
            workspace_default: false,
            harness: Some(harness.into()),
            model: None,
            effort: None,
            permission_mode: Some(mode.into()),
        };
        let chief = Self::actor(Role::Chief, Some("chief"));
        self.core.cap_preset_mode(&chief, &mut preset).is_ok()
    }

    /// The one executor pane the core holds besides the fixed ones.
    fn executor(&self) -> Option<&crate::agents::state::State> {
        let (_, running) = self
            .core
            .agents
            .running
            .iter()
            .find(|(pane, _)| !FIXED_PANES.contains(&pane.as_str()))?;
        Some(&running.state)
    }

    fn dispatch_to(
        &mut self,
        harness: &str,
        connection: &str,
        mode: Option<&str>,
        key: &str,
    ) -> Option<ErrorKind> {
        let mut preset = json!({"workspaceDefault":false,"harness":harness});
        if let Some(mode) = mode {
            preset["permissionMode"] = json!(mode);
        }
        let raw = json!({"workspaceId":WORKSPACE,"taskId":"SEED-1","preset":preset,
            "idempotencyKey":key});
        self.core
            .task_dispatch(connection, &raw)
            .err()
            .map(|error| errors::kind(&error))
    }

    fn create(
        &mut self,
        connection: &str,
        executor: serde_json::Value,
        key: &str,
    ) -> Option<ErrorKind> {
        let mut raw = json!({"workspaceId":WORKSPACE,"title":"New","mustNotChange":"Keep it",
            "executor":executor,"idempotencyKey":key});
        if let Some(dispatch) = raw.as_object_mut() {
            dispatch.insert(
                "dispatch".into(),
                json!(executor["dispatch"].as_bool().unwrap_or(false)),
            );
        }
        self.core
            .task_author(connection, method::TASK_CREATE, &raw)
            .err()
            .map(|error| errors::kind(&error))
    }

    fn task_count(&self) -> usize {
        self.core
            .store
            .list_tasks(WORKSPACE, None, 100, 0)
            .unwrap()
            .len()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        // Best effort: a leftover temporary folder does not change a result.
        std::fs::remove_dir_all(&self.directory).unwrap_or_default();
    }
}

#[test]
fn an_executor_is_below_its_dispatchers_rank_or_the_very_same_mode_on_its_harness() {
    // (the dispatcher's harness and mode, the modes it may start on each harness)
    let cases: [(&str, &str, &[&str], &[&str]); 5] = [
        ("claude", "plan", &["plan"], &[]),
        ("claude", "default", &["plan", "default"], &[]),
        // `acceptEdits` and `default` share a kind and are different modes.
        ("claude", "acceptEdits", &["plan", "acceptEdits"], &[]),
        (
            "claude",
            "auto",
            &["plan", "default", "acceptEdits", "auto"],
            &["read-only"],
        ),
        ("codex", "read-only", &["plan"], &["read-only"]),
    ];
    for (harness, mode, claude, codex) in cases {
        let rig = Rig::new(harness, mode);
        for (target, allowed) in [("claude", claude), ("codex", codex)] {
            for (candidate, _) in modes(target) {
                assert_eq!(
                    rig.allows(target, candidate),
                    allowed.contains(candidate),
                    "a {harness} chief in {mode} starting {candidate} on {target}"
                );
            }
        }
    }
}

#[test]
fn a_mode_the_target_harness_does_not_announce_is_refused_whatever_another_harness_does() {
    // Only Claude announces `plan`; Codex would drop it and start in its own default.
    let rig = Rig::new("claude", "plan");
    assert!(rig.allows("claude", "plan"));
    assert!(!rig.allows("codex", "plan"));
    let rig = Rig::new("claude", "auto");
    assert!(!rig.allows("codex", "plan"));
    // A harness no running agent belongs to announces nothing.
    assert!(!rig.allows("stub", "plan"));
}

#[test]
fn a_dispatcher_whose_mode_cannot_be_ranked_is_the_lowest() {
    let mut rig = Rig::new("claude", "plan");
    rig.core.agents.running.get_mut("chief").unwrap().state = agent_with_modes(
        "claude",
        "mystery",
        &[
            ("mystery", "unknown"),
            ("plan", "plan"),
            ("default", "standard"),
        ],
    );
    assert!(!rig.allows("claude", "default"));
    assert!(!rig.allows("claude", "mystery"));
    // The pane is not an agent here, so it has no mode.
    let missing = Rig::actor(Role::Chief, Some("missing"));
    let mut preset = TaskExecutor {
        workspace_default: false,
        harness: Some("claude".into()),
        model: None,
        effort: None,
        permission_mode: Some("default".into()),
    };
    assert!(rig.core.cap_preset_mode(&missing, &mut preset).is_err());
}

#[test]
fn a_person_is_never_capped() {
    let rig = Rig::new("claude", "plan");
    let mut preset = TaskExecutor {
        workspace_default: false,
        harness: Some("codex".into()),
        model: None,
        effort: None,
        permission_mode: Some("agent".into()),
    };
    for role in [Role::HumanLocal, Role::HumanDevice] {
        let person = Rig::actor(role, None);
        assert_eq!(
            rig.core.cap_preset_mode(&person, &mut preset).unwrap(),
            None
        );
    }
}

#[tokio::test]
async fn a_dispatch_above_the_chief_reserves_nothing_and_one_within_it_keeps_the_ceiling() {
    let mut rig = Rig::new("claude", "default");
    let chief = rig.connect(Role::Chief, Some("chief"));
    for (n, (harness, mode)) in [
        ("claude", "auto"),
        ("codex", "read-only"),
        ("codex", "plan"),
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            rig.dispatch_to(harness, &chief, Some(mode), &format!("above-{n}")),
            Some(ErrorKind::ForbiddenRole),
            "{harness} {mode}"
        );
    }
    assert!(rig.executor().is_none(), "no executor pane was reserved");
    assert_eq!(
        rig.dispatch_to("claude", &chief, Some("plan"), "within"),
        None
    );
    let executor = rig.executor().unwrap();
    assert_eq!(executor.mode.as_deref(), Some("plan"));
    assert_eq!(
        executor.mode_ceiling,
        Some(ModeCeiling::of(&agent("claude", "default"), false))
    );
}

#[tokio::test]
async fn a_chief_cannot_create_a_task_that_starts_above_its_mode() {
    let mut rig = Rig::new("claude", "default");
    let chief = rig.connect(Role::Chief, Some("chief"));
    let executor = |harness: Option<&str>, mode: &str| {
        let mut executor = json!({"workspaceDefault":true,"permissionMode":mode});
        if let Some(harness) = harness {
            executor["harness"] = json!(harness);
        }
        executor
    };
    assert_eq!(
        rig.create(&chief, executor(Some("claude"), "auto"), "create-above"),
        Some(ErrorKind::ForbiddenRole)
    );
    // Without a harness no announcement can show the mode to stay under the ceiling.
    assert_eq!(
        rig.create(&chief, executor(None, "plan"), "create-unbound"),
        Some(ErrorKind::ForbiddenRole)
    );
    assert_eq!(rig.task_count(), 1, "only the seeded task");
    assert_eq!(
        rig.create(&chief, executor(Some("claude"), "plan"), "create-within"),
        None
    );
    assert_eq!(rig.task_count(), 2);
}

#[tokio::test]
async fn a_create_that_dispatches_is_checked_before_the_task_is_stored() {
    let mut rig = Rig::new("claude", "default");
    let chief = rig.connect(Role::Chief, Some("chief"));
    let above = json!({"workspaceDefault":false,"harness":"claude","permissionMode":"auto",
        "dispatch":true});
    assert_eq!(
        rig.create(&chief, above, "create-dispatch-above"),
        Some(ErrorKind::ForbiddenRole)
    );
    assert_eq!(rig.task_count(), 1, "no `todo` task was left behind");
    // No mode and no harness default, on a harness other than the chief's: refused as well.
    let unnamed = json!({"workspaceDefault":false,"harness":"codex","dispatch":true});
    assert_eq!(
        rig.create(&chief, unnamed, "create-dispatch-unnamed"),
        Some(ErrorKind::ForbiddenRole)
    );
    assert_eq!(rig.task_count(), 1);
}

#[tokio::test]
async fn a_persons_dispatch_is_not_capped_by_any_chief() {
    let mut rig = Rig::new("claude", "plan");
    let person = rig.connect(Role::HumanLocal, None);
    assert_eq!(
        rig.dispatch_to("codex", &person, Some("agent"), "person"),
        None
    );
    assert_eq!(rig.executor().unwrap().mode_ceiling, None);
}

#[tokio::test]
async fn full_access_stays_refused_for_a_chief_whatever_its_own_mode() {
    let mut rig = Rig::new("claude", "bypassPermissions");
    let chief = rig.connect(Role::Chief, Some("chief"));
    for harness in ["claude", "codex"] {
        let mode = modes(harness)[modes(harness).len() - 1].0;
        assert_eq!(
            rig.dispatch_to(harness, &chief, Some(mode), &format!("full-{harness}")),
            Some(ErrorKind::ForbiddenRole)
        );
    }
    // A chief in full access starts an executor in any other mode of its harness.
    assert_eq!(
        rig.dispatch_to("claude", &chief, Some("auto"), "below"),
        None
    );
}

#[tokio::test]
async fn the_default_mode_of_the_harness_counts_when_the_dispatch_names_none() {
    let mut rig = Rig::new("claude", "plan");
    StorePort::mutate(
        &mut rig.core.store,
        "test",
        "default-mode",
        "seed",
        |change| {
            change.set_setting(
                &soyeht_protocol::setting::harness_default_mode_key("codex"),
                &json!("agent"),
            )?;
            Ok((json!({}), Vec::new()))
        },
    )
    .unwrap();
    let chief = rig.connect(Role::Chief, Some("chief"));
    assert_eq!(
        rig.dispatch_to("codex", &chief, None, "default-above"),
        Some(ErrorKind::ForbiddenRole)
    );
    let person = rig.connect(Role::HumanLocal, None);
    assert_eq!(
        rig.dispatch_to("codex", &person, None, "default-person"),
        None
    );
}

#[tokio::test]
async fn an_unnamed_mode_is_the_dispatchers_own_on_its_harness_and_refused_on_another() {
    let mut rig = Rig::new("claude", "default");
    let chief = rig.connect(Role::Chief, Some("chief"));
    // The executor's harness cannot announce its modes before it starts.
    assert_eq!(
        rig.dispatch_to("codex", &chief, None, "other-harness"),
        Some(ErrorKind::ForbiddenRole)
    );
    assert!(rig.executor().is_none(), "nothing was reserved");
    assert_eq!(
        rig.dispatch_to("claude", &chief, None, "same-harness"),
        None
    );
    assert_eq!(rig.executor().unwrap().mode.as_deref(), Some("default"));
}

#[tokio::test]
async fn a_chief_in_full_access_cannot_pass_it_on_by_leaving_the_mode_out() {
    let mut rig = Rig::new("claude", "bypassPermissions");
    let chief = rig.connect(Role::Chief, Some("chief"));
    assert_eq!(
        rig.dispatch_to("claude", &chief, None, "pinned-full"),
        Some(ErrorKind::ForbiddenRole)
    );
    assert!(rig.executor().is_none());
}

/// An executor pane of `harness` in `current`, whose chief is a Claude agent in `chief_mode`,
/// asked to start in `requested`.
fn started_executor(harness: &str, current: &str, chief_mode: &str, requested: &str) -> Rig {
    let mut rig = Rig::new("claude", chief_mode);
    let ceiling = ModeCeiling::of(&rig.core.agents.running["chief"].state, false);
    let directory = rig.directory.clone();
    announce(&mut rig.core, &directory, "executor", harness, current);
    let state = &mut rig.core.agents.running.get_mut("executor").unwrap().state;
    state.session_id = Some("session".into());
    state.mode_ceiling = Some(ceiling);
    state.mode = Some(requested.into());
    state
        .requested_options
        .insert("mode".into(), ConfigValue::V0(requested.into()));
    rig
}

/// What the pane `executor` asks its harness for once its session opened: the mode it would
/// request, or `None` when the pane is to stop.
fn requested_mode(rig: &mut Rig) -> Option<ConfigValue> {
    let state = &mut rig.core.agents.running.get_mut("executor").unwrap().state;
    let mut changes = crate::agents::Changes::default();
    crate::agents::configuration::hold_mode_ceiling(state, 1, &mut changes)
        .unwrap()
        .then(|| state.requested_options["mode"].clone())
}

#[test]
fn a_mode_the_harness_drops_does_not_leave_the_executor_in_the_harness_default() {
    // The chief asked for Codex's default `agent` (auto review) or for a mode Codex lacks;
    // Codex stays in `agent` unless the pane asks for the highest mode within the ceiling.
    for requested in ["plan", "agent"] {
        let mut rig = started_executor("codex", "agent", "auto", requested);
        assert_eq!(
            requested_mode(&mut rig),
            Some(ConfigValue::V0("read-only".into())),
            "{requested}"
        );
    }
}

#[test]
fn a_mode_within_the_ceiling_is_applied_as_asked() {
    let mut rig = started_executor("claude", "default", "auto", "plan");
    assert_eq!(
        requested_mode(&mut rig),
        Some(ConfigValue::V0("plan".into()))
    );
}

#[test]
fn a_refusal_of_the_lower_mode_stops_the_pane_instead_of_trying_again() {
    let mut rig = started_executor("codex", "agent", "auto", "agent");
    assert_eq!(
        requested_mode(&mut rig),
        Some(ConfigValue::V0("read-only".into()))
    );
    // The harness refused: it stays in `agent`, which is above the ceiling.
    let state = &mut rig.core.agents.running.get_mut("executor").unwrap().state;
    state.requested_options.clear();
    assert_eq!(requested_mode(&mut rig), None);
}

#[test]
fn a_pane_with_no_mode_within_the_ceiling_is_stopped() {
    use crate::agents::state::Phase;
    // Codex has no mode below a plan ceiling, and no plan mode to drop into.
    let mut rig = started_executor("codex", "agent", "plan", "plan");
    rig.core.configure_initial_agent("executor").unwrap();
    assert_eq!(executor_state(&rig).phase, Phase::Faulted);
}

#[test]
fn a_restart_for_a_task_keeps_the_ceiling_and_checks_the_mode_again() {
    let rig = started_executor("claude", "default", "default", "plan");
    let mut old = rig.core.agents.running["executor"].state.clone();
    // The first session was checked; the restarted one is a new session.
    old.mode_ceiling
        .as_mut()
        .unwrap()
        .verdict(&agent("claude", "plan"), None);
    assert!(!old.mode_ceiling.as_ref().unwrap().pending());
    let preset = TaskExecutor {
        workspace_default: false,
        harness: Some("claude".into()),
        model: None,
        effort: None,
        permission_mode: Some("auto".into()),
    };
    let restarted = old
        .restart_for_task(old.launch.clone(), old.binary_digest.clone(), &preset)
        .unwrap();
    let ceiling = restarted.mode_ceiling.as_ref().unwrap();
    assert!(ceiling.pending());
    assert!(restarted.needs_configuration());
    // The preset names a mode above the ceiling: it is replaced when the session opens.
    let mut opened = restarted.clone();
    opened.options = agent("claude", "plan").options;
    let mut changes = crate::agents::Changes::default();
    assert!(crate::agents::configuration::hold_mode_ceiling(&mut opened, 1, &mut changes).unwrap());
    assert_eq!(
        opened.requested_options["mode"],
        ConfigValue::V0("default".into())
    );
}

/// The pane `executor` has a process, so the requests it sends have somewhere to go.
fn adopt_executor(rig: &mut Rig) {
    let stored = &mut rig.core.panes.get_mut("executor").unwrap().stored;
    stored.instance_id = Some("executor-instance".into());
    stored.supervisor_id = Some("executor-supervisor".into());
}

fn set_mode_request(mode: &str) -> serde_json::Value {
    json!({"jsonrpc":"2.0","id":"set-mode","method":method::PANE_CONFIG_SET,"params":{
        "paneId":"executor","configId":"mode","value":mode,"idempotencyKey":"set-mode"}})
}

#[test]
fn a_person_who_picks_a_mode_above_the_ceiling_lifts_it_and_a_mode_within_it_does_not() {
    let mut rig = started_executor("claude", "default", "auto", "plan");
    adopt_executor(&mut rig);
    rig.core
        .agents
        .running
        .get_mut("executor")
        .unwrap()
        .state
        .requested_options
        .clear();
    // The chief is an agent binding: the method is not open to it, and the ceiling stays.
    let chief = rig.connect(Role::Chief, Some("chief"));
    let refused = rig
        .core
        .dispatch(&chief, set_mode_request("bypassPermissions"));
    assert_eq!(
        refused.err().map(|error| errors::kind(&error)),
        Some(ErrorKind::ForbiddenRole)
    );
    assert!(executor_state(&rig).mode_ceiling.is_some());
    // A person's mode within the ceiling leaves it in place: `plan` is below `auto`.
    let person = rig.connect(Role::HumanLocal, None);
    rig.core
        .dispatch(&person, set_mode_request("plan"))
        .unwrap();
    assert!(executor_state(&rig).mode_ceiling.is_some());
    // A person's choice above it is explicit: the pane has no ceiling afterwards.
    let again = json!({"jsonrpc":"2.0","id":"set-mode-2","method":method::PANE_CONFIG_SET,"params":{
        "paneId":"executor","configId":"mode","value":"bypassPermissions",
        "idempotencyKey":"set-mode-2"}});
    // The first change is still pending on the harness; the engine takes the next one once it
    // has been confirmed.
    rig.core
        .agents
        .running
        .get_mut("executor")
        .unwrap()
        .state
        .operation = None;
    rig.core.dispatch(&person, again).unwrap();
    assert!(executor_state(&rig).mode_ceiling.is_none());
}

/// The pane `executor`, started under the ceiling of a chief, has its session checked and works
/// on a prompt. Returns its session.
fn start_working(rig: &mut Rig) -> String {
    adopt_executor(rig);
    let state = &mut rig.core.agents.running.get_mut("executor").unwrap().state;
    state.requested_options.clear();
    let mut ceiling = state.mode_ceiling.take().unwrap();
    // The session was opened and checked: the mode it runs is within the ceiling.
    assert_eq!(
        ceiling.verdict(state, None),
        crate::agents::mode_ceiling::Verdict::Within
    );
    state.mode_ceiling = Some(ceiling);
    let prompt = state
        .enqueue(
            "work".into(),
            soyeht_protocol::PromptOrigin::Human,
            "t".into(),
        )
        .unwrap();
    state.active = Some(prompt);
    state.session_id.clone().unwrap()
}

/// The agent of the pane `executor` reports `reported` as its mode.
fn report_mode(rig: &mut Rig, session: &str, reported: &str) {
    let record = soyeht_acp::rpc::Record {
        from_seq: 0,
        to_seq: 1,
        message: soyeht_acp::rpc::Message::Notification {
            method: "session/update".into(),
            params: json!({"sessionId":session,"update":{
                "sessionUpdate":"current_mode_update","currentModeId":reported}}),
        },
    };
    rig.core.native_record("executor", record, false).unwrap();
}

/// The pane `executor` of `harness` working on a prompt, checked against a ceiling set by a
/// Claude chief in `chief_mode`, whose agent then reports `reported` as its mode.
fn executor_moving_itself(harness: &str, chief_mode: &str, current: &str, reported: &str) -> Rig {
    let mut rig = started_executor(harness, current, chief_mode, current);
    let session = start_working(&mut rig);
    report_mode(&mut rig, &session, reported);
    rig
}

/// What the transcript of the pane `executor` says when the harness ends its turn as cancelled,
/// and its state afterwards.
fn cancelled_turn(rig: &Rig) -> (Vec<String>, crate::agents::state::State) {
    let mut state = rig.core.agents.running["executor"].state.clone();
    let mut saved = rig.core.panes["executor"].stored.clone();
    let mut changes = crate::agents::Changes::default();
    let prompt_id = state.active.as_ref().unwrap().prompt_id.clone();
    crate::agents::responses::prompt_response(
        &rig.core.store,
        &mut state,
        &mut saved,
        &prompt_id,
        &mut changes,
        Ok(json!({"stopReason":"cancelled"})),
        "t",
    )
    .unwrap();
    let lines = changes.messages.into_iter().map(|m| m.text).collect();
    (lines, state)
}

#[test]
fn an_agent_that_moves_itself_above_the_ceiling_is_stopped_and_checked_again() {
    // The chief is in `auto`; the executor ran in `read-only` and moves to `agent`.
    let rig = executor_moving_itself("codex", "auto", "read-only", "agent");
    let state = &rig.core.agents.running["executor"].state;
    assert!(state.cancel_at_ms.is_some(), "the turn is cancelled");
    assert!(state.queue_paused, "the queue is held");
    let ceiling = state.mode_ceiling.as_ref().unwrap();
    assert!(ceiling.pending(), "the next advance checks the mode again");
    // The check runs once the cancelled turn has ended, not while it goes on.
    assert!(!state.needs_configuration());
    let mut ended = state.clone();
    ended.active = None;
    assert!(ended.needs_configuration());
    let mut changes = crate::agents::Changes::default();
    assert!(crate::agents::configuration::hold_mode_ceiling(&mut ended, 1, &mut changes).unwrap());
    assert_eq!(
        ended.requested_options["mode"],
        ConfigValue::V0("read-only".into())
    );
}

#[test]
fn a_move_within_the_ceiling_leaves_the_turn_alone() {
    let rig = executor_moving_itself("claude", "auto", "default", "plan");
    let state = &rig.core.agents.running["executor"].state;
    assert!(state.cancel_at_ms.is_none());
    assert!(!state.queue_paused);
    assert!(!state.mode_ceiling.as_ref().unwrap().pending());
}

#[test]
fn a_full_access_move_of_the_agent_is_above_any_ceiling() {
    let rig = executor_moving_itself("claude", "auto", "default", "bypassPermissions");
    let state = &rig.core.agents.running["executor"].state;
    assert!(state.cancel_at_ms.is_some());
    assert!(state.mode_ceiling.as_ref().unwrap().pending());
}

/// A task of the pane `executor`, in progress with one open attempt.
fn give_executor_a_task(rig: &mut Rig) {
    let mut task = task_fixture(TaskState::InProgress);
    task.task_id = "OWN-1".into();
    task.owner_pane_id = Some("executor".into());
    task.current_attempt = 1;
    let attempt = soyeht_protocol::Attempt {
        n: 1,
        pane_id: "executor".into(),
        harness: "claude".into(),
        model: "model".into(),
        start: "2026-09-11T00:00:00Z".into(),
        end: None,
        result: None,
        summary: None,
    };
    StorePort::mutate(&mut rig.core.store, "test", "own", "own", |change| {
        change.insert_task(&task)?;
        change.insert_attempt(&task.workspace_id, &task.task_id, &attempt)?;
        Ok((json!({}), Vec::new()))
    })
    .unwrap();
}

#[test]
fn a_task_whose_executor_raised_its_mode_goes_back_to_the_queue_with_the_reason() {
    use soyeht_protocol::AttemptResult;
    let mut rig = started_executor("codex", "read-only", "auto", "read-only");
    give_executor_a_task(&mut rig);
    let session = start_working(&mut rig);
    report_mode(&mut rig, &session, "agent");
    // The task is back in the queue, its attempt closed as returned for the ceiling.
    let task = rig
        .core
        .store
        .task("workspace-1", "OWN-1")
        .unwrap()
        .unwrap();
    assert_eq!(task.state, TaskState::Todo);
    assert_eq!(task.owner_pane_id, None);
    let attempts = rig
        .core
        .store
        .task_attempts("workspace-1", "OWN-1")
        .unwrap();
    assert_eq!(attempts[0].result, Some(AttemptResult::Returned));
    assert!(
        attempts[0]
            .summary
            .as_deref()
            .is_some_and(|reason| reason.contains("mode_ceiling"))
    );
    let comments = rig
        .core
        .store
        .task_comments("workspace-1", "OWN-1", None, 10)
        .unwrap();
    assert!(comments.iter().any(|c| c.text.contains("mode_ceiling")));
    // The turn is cancelled, and when the harness confirms, the transcript says why.
    assert!(executor_state(&rig).cancelled_for_ceiling);
    let (lines, state) = cancelled_turn(&rig);
    assert!(
        lines
            .iter()
            .any(|line| line == crate::agents::configuration::MODE_CEILING_CANCELLED),
        "{lines:?}"
    );
    assert!(!lines.iter().any(|line| line.contains("by the user")));
    assert!(!state.cancelled_for_ceiling);
}

#[test]
fn a_person_cancelling_a_turn_keeps_the_usual_transcript_line() {
    let mut rig = executor_moving_itself("claude", "auto", "default", "plan");
    rig.core
        .agents
        .running
        .get_mut("executor")
        .unwrap()
        .state
        .cancel_at_ms = Some(1);
    let (lines, _) = cancelled_turn(&rig);
    assert!(
        lines
            .iter()
            .any(|line| line == "Turn cancelled by the user.")
    );
}

mod message;
