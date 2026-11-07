use super::*;
use crate::{
    outbox,
    ports::StorePort,
    tasks::rules::tests::task_fixture,
    test_panes::{core_with_workspace, insert_agent_pane},
};
use serde_json::json;
use soyeht_acp::config::Options;
use soyeht_protocol::{ErrorKind, TaskState, method};

const WORKSPACE: &str = "workspace-1";

/// The mode selector of a harness that offers every kind of mode.
fn options(current: &str, choices: &[(&str, Option<&str>)]) -> Options {
    let choices: Vec<_> = choices
        .iter()
        .map(|(value, kind)| match kind {
            Some(kind) => json!({"value":value,"name":value,"_meta":{"kind":kind}}),
            None => json!({"value":value,"name":value}),
        })
        .collect();
    Options::from_response(&json!({"configOptions":[{"id":"mode","name":"Mode",
        "category":"mode","type":"select","currentValue":current,"options":choices}]}))
    .unwrap()
}

const KINDS: [(&str, Option<&str>); 5] = [
    ("read-only", Some("plan")),
    ("ask", Some("standard")),
    ("review", Some("auto_review")),
    ("everything", Some("full_access")),
    ("mystery", None),
];

struct Rig {
    core: Core,
    directory: std::path::PathBuf,
    _outbox: Vec<outbox::Receiver>,
}

impl Rig {
    /// A core with a task to dispatch, an agent pane `chief` in the mode `chief_mode`, whose
    /// harness announces every kind of mode, and the stub harness `stub`.
    fn new(chief_mode: &str) -> Self {
        let (mut core, directory) = core_with_workspace(WORKSPACE);
        let mut task = task_fixture(TaskState::Todo);
        task.owner_pane_id = None;
        task.task_id = "SEED-1".into();
        StorePort::mutate(&mut core.store, "test", "seed", "seed", |change| {
            change.insert_task(&task)?;
            Ok((json!({}), Vec::new()))
        })
        .unwrap();
        insert_agent_pane(&mut core, &directory, "chief", WORKSPACE);
        if let Some(agent) = core.agents.get_mut("chief") {
            agent.state.options = options(chief_mode, &KINDS);
        }
        std::fs::write(
            directory.join("harnesses.json"),
            json!([
                {"id":"stub","name":"Stub","command":"/bin/ls","args":[],"version":"1"},
                // The harness of the pane `chief`.
                {"id":"test","name":"Test","command":"/bin/ls","args":[],"version":"1"}
            ])
            .to_string(),
        )
        .unwrap();
        Self {
            core,
            directory,
            _outbox: Vec::new(),
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
        self.core.clients.get_mut(&connection).unwrap().actor = Some(Actor {
            role,
            client: "mode-cap-test".into(),
            device_id: None,
            pane_id: pane.map(Into::into),
        });
        connection
    }

    fn dispatch(&mut self, connection: &str, mode: Option<&str>, key: &str) -> Option<ErrorKind> {
        self.dispatch_to("stub", connection, mode, key)
    }

    /// The mode requested for the one executor pane the core holds besides the chief.
    fn executor_mode(&self) -> Option<String> {
        let (_, agent) = self.core.agents.iter().find(|(pane, _)| *pane != "chief")?;
        agent.state.mode.clone()
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

    fn create(&mut self, connection: &str, executor_mode: &str, key: &str) -> Option<ErrorKind> {
        let raw = json!({"workspaceId":WORKSPACE,"title":"New","mustNotChange":"Keep it",
            "executor":{"workspaceDefault":true,"permissionMode":executor_mode},
            "idempotencyKey":key});
        self.core
            .task_author(connection, method::TASK_CREATE, &raw)
            .err()
            .map(|error| errors::kind(&error))
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        // Best effort: a leftover temporary folder does not change a result.
        std::fs::remove_dir_all(&self.directory).unwrap_or_default();
    }
}

fn refuses(core: &Core, role: Role, pane: &str, mode: &str) -> bool {
    let actor = Actor {
        role,
        client: "mode-cap-test".into(),
        device_id: None,
        pane_id: Some(pane.into()),
    };
    core.require_modes_within_dispatcher(&actor, [Some(mode)])
        .is_err()
}

#[test]
fn an_agent_may_start_work_only_up_to_its_own_mode_rank() {
    let ranks = ["read-only", "ask", "review", "everything"];
    for (cap, current) in ["read-only", "ask", "review"].iter().enumerate() {
        let rig = Rig::new(current);
        for (rank, mode) in ranks.iter().enumerate() {
            assert_eq!(
                refuses(&rig.core, Role::Chief, "chief", mode),
                rank > cap,
                "a chief in {current} starting {mode}"
            );
        }
    }
}

#[test]
fn a_mode_without_a_rank_counts_as_the_lowest_for_the_dispatcher_and_is_refused_as_a_request() {
    let rig = Rig::new("mystery");
    assert!(!refuses(&rig.core, Role::Chief, "chief", "read-only"));
    assert!(refuses(&rig.core, Role::Chief, "chief", "ask"));
    let rig = Rig::new("ask");
    // No agent announced this mode, so it cannot be shown to stay under the cap.
    assert!(refuses(&rig.core, Role::Chief, "chief", "never-announced"));
    // An announced mode with no kind has no rank either.
    assert!(refuses(&rig.core, Role::Chief, "chief", "mystery"));
    // A pane that is not an agent here has no mode: the lowest.
    assert!(refuses(&rig.core, Role::Chief, "missing", "ask"));
}

#[test]
fn a_person_is_never_capped() {
    let rig = Rig::new("read-only");
    for role in [Role::HumanLocal, Role::HumanDevice] {
        for mode in ["ask", "review", "everything", "never-announced"] {
            assert!(!refuses(&rig.core, role.clone(), "chief", mode));
        }
    }
}

#[tokio::test]
async fn a_chief_in_plan_mode_cannot_dispatch_an_executor_above_plan() {
    let mut rig = Rig::new("read-only");
    let chief = rig.connect(Role::Chief, Some("chief"));
    for (n, mode) in ["ask", "review"].iter().enumerate() {
        assert_eq!(
            rig.dispatch(&chief, Some(mode), &format!("above-{n}")),
            Some(ErrorKind::ForbiddenRole),
            "{mode}"
        );
    }
    assert_eq!(rig.core.panes.len(), 1, "no executor pane was reserved");
    assert_eq!(rig.dispatch(&chief, Some("read-only"), "plan"), None);
}

#[tokio::test]
async fn a_chief_cannot_create_a_task_that_starts_above_its_mode() {
    let mut rig = Rig::new("ask");
    let chief = rig.connect(Role::Chief, Some("chief"));
    assert_eq!(
        rig.create(&chief, "review", "create-above"),
        Some(ErrorKind::ForbiddenRole)
    );
    assert_eq!(rig.create(&chief, "ask", "create-within"), None);
}

#[tokio::test]
async fn a_persons_dispatch_is_not_capped_by_any_chief() {
    let mut rig = Rig::new("read-only");
    let person = rig.connect(Role::HumanLocal, None);
    assert_eq!(rig.dispatch(&person, Some("review"), "person"), None);
}

#[tokio::test]
async fn full_access_stays_refused_for_a_chief_whatever_its_own_mode() {
    let mut rig = Rig::new("everything");
    let chief = rig.connect(Role::Chief, Some("chief"));
    assert_eq!(
        rig.dispatch(&chief, Some("everything"), "full"),
        Some(ErrorKind::ForbiddenRole)
    );
}

#[tokio::test]
async fn the_default_mode_of_the_harness_counts_when_the_dispatch_names_none() {
    let mut rig = Rig::new("read-only");
    StorePort::mutate(
        &mut rig.core.store,
        "test",
        "default-mode",
        "seed",
        |change| {
            change.set_setting(
                &soyeht_protocol::setting::harness_default_mode_key("stub"),
                &json!("review"),
            )?;
            Ok((json!({}), Vec::new()))
        },
    )
    .unwrap();
    let chief = rig.connect(Role::Chief, Some("chief"));
    assert_eq!(
        rig.dispatch(&chief, None, "default-above"),
        Some(ErrorKind::ForbiddenRole)
    );
    let person = rig.connect(Role::HumanLocal, None);
    assert_eq!(rig.dispatch(&person, None, "default-person"), None);
}

#[tokio::test]
async fn an_unnamed_mode_is_the_dispatchers_own_on_its_harness_and_refused_on_another() {
    let mut rig = Rig::new("ask");
    let chief = rig.connect(Role::Chief, Some("chief"));
    // The executor's harness cannot announce its modes before it starts.
    assert_eq!(
        rig.dispatch_to("stub", &chief, None, "other-harness"),
        Some(ErrorKind::ForbiddenRole)
    );
    assert_eq!(rig.executor_mode(), None, "nothing was reserved");
    assert_eq!(rig.dispatch_to("test", &chief, None, "same-harness"), None);
    assert_eq!(rig.executor_mode().as_deref(), Some("ask"));
}

#[tokio::test]
async fn a_persons_unnamed_mode_is_left_to_the_harness() {
    let mut rig = Rig::new("read-only");
    let person = rig.connect(Role::HumanLocal, None);
    assert_eq!(rig.dispatch_to("stub", &person, None, "person"), None);
    assert_eq!(rig.executor_mode(), None);
}

#[tokio::test]
async fn a_chief_in_full_access_cannot_pass_it_on_by_leaving_the_mode_out() {
    let mut rig = Rig::new("everything");
    let chief = rig.connect(Role::Chief, Some("chief"));
    assert_eq!(
        rig.dispatch_to("test", &chief, None, "pinned-full"),
        Some(ErrorKind::ForbiddenRole)
    );
    assert_eq!(rig.executor_mode(), None);
}

#[tokio::test]
async fn a_chief_in_full_access_cannot_start_a_mode_no_harness_announced() {
    let mut rig = Rig::new("everything");
    let chief = rig.connect(Role::Chief, Some("chief"));
    // The id may be the harness's own name for full access: it is not known to be anything less.
    assert_eq!(
        rig.dispatch(&chief, Some("never-announced"), "unknown"),
        Some(ErrorKind::ForbiddenRole)
    );
    assert_eq!(
        rig.create(&chief, "never-announced", "unknown-create"),
        Some(ErrorKind::ForbiddenRole)
    );
    assert_eq!(rig.dispatch(&chief, Some("ask"), "known"), None);
}

/// A message from the pane `chief` to the pane `target`, whose mode is `target_mode`.
fn message_error(target_mode: &str) -> Option<ErrorKind> {
    let mut rig = Rig::new("ask");
    insert_agent_pane(&mut rig.core, &rig.directory, "target", WORKSPACE);
    if let Some(agent) = rig.core.agents.get_mut("target") {
        agent.state.options = options(target_mode, &KINDS);
    }
    let sender = rig.connect(Role::Agent, Some("chief"));
    let params = json!({"targetPaneId":"target","text":"hello","idempotencyKey":"message"});
    rig.core
        .agent_peer_method(&sender, method::AGENT_MESSAGE, &params)
        .err()
        .map(|error| errors::kind(&error))
}

#[tokio::test]
async fn an_agent_cannot_send_a_message_into_a_pane_in_full_access() {
    assert_eq!(message_error("everything"), Some(ErrorKind::ForbiddenRole));
    for mode in ["read-only", "ask", "review", "mystery"] {
        assert_eq!(message_error(mode), None, "{mode}");
    }
}
