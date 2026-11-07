use super::*;
use crate::{
    agents::mode_ceiling::ModeCeiling,
    outbox,
    ports::StorePort,
    tasks::rules::tests::task_fixture,
    test_panes::{core_with_workspace, insert_agent_pane, ready_agent_state},
};
use serde_json::json;
use soyeht_acp::config::Options;
use soyeht_protocol::{ConfigValue, ErrorKind, Role, TaskState, method};

const WORKSPACE: &str = "workspace-1";

/// The modes the Claude adapter announces, with the kind each carries.
const CLAUDE: [(&str, &str); 5] = [
    ("default", "standard"),
    ("acceptEdits", "standard"),
    ("plan", "plan"),
    ("auto", "auto_review"),
    ("bypassPermissions", "full_access"),
];

/// The modes the Codex adapter announces: it has no plan mode, and its `read-only` is the
/// standard one that asks before editing outside the workspace.
const CODEX: [(&str, &str); 3] = [
    ("read-only", "standard"),
    ("agent", "auto_review"),
    ("agent-full-access", "full_access"),
];

fn modes(harness: &str) -> &'static [(&'static str, &'static str)] {
    if harness == "claude" { &CLAUDE } else { &CODEX }
}

fn options(current: &str, choices: &[(&str, &str)]) -> Options {
    let choices: Vec<_> = choices
        .iter()
        .map(|(value, kind)| json!({"value":value,"name":value,"_meta":{"kind":kind}}))
        .collect();
    Options::from_response(&json!({"configOptions":[{"id":"mode","name":"Mode",
        "category":"mode","type":"select","currentValue":current,"options":choices}]}))
    .unwrap()
}

/// The state of a ready agent of `harness` in `current`.
fn agent(harness: &str, current: &str) -> crate::agents::state::State {
    let mut state = ready_agent_state();
    state.launch.id = harness.into();
    state.options = options(current, modes(harness));
    state
}

fn announce(core: &mut Core, directory: &std::path::Path, pane: &str, harness: &str, mode: &str) {
    insert_agent_pane(core, directory, pane, WORKSPACE);
    if let Some(running) = core.agents.get_mut(pane) {
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
    rig.core.agents.get_mut("chief").unwrap().state.options = options(
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
    let ceiling = ModeCeiling::of(&rig.core.agents["chief"].state, false);
    let directory = rig.directory.clone();
    announce(&mut rig.core, &directory, "executor", harness, current);
    let state = &mut rig.core.agents.get_mut("executor").unwrap().state;
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
    let state = &mut rig.core.agents.get_mut("executor").unwrap().state;
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
    let state = &mut rig.core.agents.get_mut("executor").unwrap().state;
    state.requested_options.clear();
    assert_eq!(requested_mode(&mut rig), None);
}

#[test]
fn a_pane_with_no_mode_within_the_ceiling_is_stopped() {
    use crate::agents::state::Phase;
    // Codex has no mode below a plan ceiling, and no plan mode to drop into.
    let mut rig = started_executor("codex", "agent", "plan", "plan");
    rig.core.configure_initial_agent("executor").unwrap();
    assert_eq!(rig.core.agents["executor"].state.phase, Phase::Faulted);
}

#[test]
fn a_restart_for_a_task_keeps_the_ceiling_and_checks_the_mode_again() {
    let rig = started_executor("claude", "default", "default", "plan");
    let mut old = rig.core.agents["executor"].state.clone();
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
