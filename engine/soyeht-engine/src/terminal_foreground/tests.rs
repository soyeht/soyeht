//! The scans, with a supervisor that says what each terminal runs.

use super::*;
use crate::{
    dispatch::Outcome,
    outbox,
    panes::persist_new_pane,
    ports::StorePort,
    supervisor_link::tests::Manual,
    test_panes::{
        Answering, Featured, core_with_workspace, insert_agent_pane, insert_terminal_pane,
    },
    tick_due::Next,
};
use serde_json::{Value, json};
use soyeht_protocol::{Actor, Role, method};
use soyeht_ptyd::wire::{FEATURE_FOREGROUND, Foreground};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, AtomicUsize, Ordering},
};

const WORKSPACE: &str = "workspace-1";

fn program(pid: u32, exe: &str, arguments: &[&str]) -> Program {
    harnesses::sample(pid, None, exe, arguments)
}

/// The programs of each terminal instance, in the order they were set.
type ByInstance = Vec<(String, Vec<Program>)>;

/// What the supervisor says each terminal runs; `asked` counts its `Foreground` requests.
#[derive(Clone, Default)]
struct Programs {
    by_instance: Arc<Mutex<ByInstance>>,
    asked: Arc<AtomicUsize>,
}

impl Programs {
    fn set(&self, instance: &str, programs: Vec<Program>) {
        let mut by_instance = self.by_instance.lock().unwrap();
        by_instance.retain(|(id, _)| id != instance);
        by_instance.push((instance.into(), programs));
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }

    /// A supervisor of this build, which lists the `foreground` feature.
    fn supervisor(&self) -> Box<dyn crate::ports::SupervisorPort> {
        Box::new(Featured(self.answering(), vec![FEATURE_FOREGROUND.into()]))
    }

    fn answering(&self) -> Box<dyn crate::ports::SupervisorPort> {
        let programs = self.clone();
        Box::new(Answering(move |request| match request {
            Request::Foreground { instance_ids } => {
                programs.asked.fetch_add(1, Ordering::SeqCst);
                let known = programs.by_instance.lock().unwrap();
                Ok(Response::Foregrounds(
                    instance_ids
                        .into_iter()
                        .filter_map(|instance_id| {
                            let programs = known
                                .iter()
                                .find(|(id, _)| *id == instance_id)
                                .map(|(_, programs)| programs.clone())?;
                            Some(Foreground {
                                instance_id,
                                programs,
                                truncated: false,
                            })
                        })
                        .collect(),
                ))
            }
            _ => Ok(Response::Ok),
        }))
    }
}

struct Rig {
    core: Core,
    directory: std::path::PathBuf,
    programs: Programs,
    clock: Arc<Manual>,
    _receivers: Vec<outbox::Receiver>,
}

impl Rig {
    fn new() -> Self {
        let (mut core, directory) = core_with_workspace(WORKSPACE);
        let programs = Programs::default();
        let clock = Arc::new(Manual(AtomicI64::new(1_000_000)));
        core.clock = clock.clone();
        core.link_supervisor(programs.supervisor());
        Self {
            core,
            directory,
            programs,
            clock,
            _receivers: Vec::new(),
        }
    }

    fn advance(&self, milliseconds: i64) {
        self.clock.0.fetch_add(milliseconds, Ordering::SeqCst);
    }

    fn terminal(&mut self, label: &str) -> (String, String) {
        let pane = insert_terminal_pane(&mut self.core, &self.directory, label, false);
        let Some(running) = self.core.panes.get_mut(&pane) else {
            panic!("the pane was just inserted");
        };
        running.stored.pane.workspace_id = WORKSPACE.into();
        let stored = running.stored.clone();
        StorePort::mutate(
            &mut self.core.store,
            "test",
            &pane,
            "seed-terminal",
            |change| {
                persist_new_pane(change, &stored, None)?;
                Ok((json!({}), Vec::new()))
            },
        )
        .unwrap();
        (pane, format!("{label}-instance"))
    }

    /// A rig whose app watches and whose one terminal runs `programs`, scanned once.
    fn scanned(programs: Vec<Program>) -> (Self, String, String) {
        let mut rig = Self::new();
        rig.watch();
        let (pane, instance) = rig.terminal("shell");
        rig.programs.set(&instance, programs);
        rig.core.foreground_mut().note_output(&pane);
        rig.core.advance_foreground().unwrap();
        (rig, pane, instance)
    }

    /// The pane printed again, and the next scan of the period comes.
    fn rescan(&mut self, pane: &str) {
        self.core.foreground_mut().note_output(pane);
        self.advance(INTERVAL_MS);
        self.core.advance_foreground().unwrap();
    }

    /// An app that has subscribed to events.
    fn watch(&mut self) {
        let (sender, receiver) = outbox::channel();
        self._receivers.push(receiver);
        let connection = self.core.connect(true, false, sender).unwrap();
        self.core.clients.get_mut(&connection).unwrap().events = true;
    }

    /// An agent binding of `pane`, as `soyeht-mcp` is.
    fn agent_binding(&mut self, pane: &str) -> String {
        insert_agent_pane(&mut self.core, &self.directory, pane, WORKSPACE);
        let (sender, receiver) = outbox::channel();
        self._receivers.push(receiver);
        let connection = self.core.connect(true, false, sender).unwrap();
        self.core.clients.get_mut(&connection).unwrap().actor = Some(Actor {
            role: Role::Agent,
            client: "test".into(),
            device_id: None,
            pane_id: Some(pane.into()),
        });
        connection
    }

    fn harness_of(&self, pane: &str) -> Option<TerminalHarness> {
        self.core.panes[pane].stored.pane.terminal_harness.clone()
    }

    fn agent_list(&mut self, connection: &str) -> Value {
        let outcome = self
            .core
            .agent_peer(connection, method::AGENT_LIST, &json!({}))
            .unwrap();
        let outcome = match outcome {
            Outcome::Offload(offload) => offload.run_inline(&mut self.core).unwrap(),
            outcome => outcome,
        };
        let Outcome::Reply(value) = outcome else {
            panic!("expected a reply");
        };
        value
    }

    /// `agent.list` names no pane and the supervisor was asked nothing.
    fn assert_nothing_listed(&mut self, sender: &str) {
        let listed = self.agent_list(sender);
        assert!(listed["items"].as_array().unwrap().is_empty());
        assert_eq!(self.programs.asked(), 0);
    }

    fn finish(self) {
        crate::test_paths::remove_after(self.core, self.directory);
    }
}

#[test]
fn a_terminal_that_runs_a_known_harness_is_recorded_with_its_version_and_cleared_when_it_leaves() {
    let (mut rig, pane, instance) = Rig::scanned(vec![program(
        40,
        "/home/x/.local/share/claude/versions/2.1.325",
        &["claude", "--resume"],
    )]);

    assert_eq!(
        rig.harness_of(&pane),
        Some(TerminalHarness {
            id: "registry.claude-acp".into(),
            name: "Claude Code".into(),
            version: Some("2.1.325".into()),
        })
    );
    // It is durable: the store holds the pane with its harness.
    let stored = rig.core.store.panes().unwrap();
    let saved = stored
        .iter()
        .find(|record| record.pane.pane_id == pane)
        .unwrap();
    assert_eq!(
        saved
            .pane
            .terminal_harness
            .as_ref()
            .map(|harness| harness.name.as_str()),
        Some("Claude Code")
    );

    // The person leaves the harness: the shell is in the foreground again.
    rig.programs
        .set(&instance, vec![program(41, "/bin/zsh", &["-zsh"])]);
    rig.rescan(&pane);
    assert_eq!(rig.harness_of(&pane), None);
    rig.finish();
}

#[test]
fn a_plain_shell_has_no_harness() {
    let (rig, pane, _) = Rig::scanned(vec![program(41, "/bin/zsh", &["-zsh"])]);
    assert_eq!(rig.programs.asked(), 1);
    assert_eq!(rig.harness_of(&pane), None);
    rig.finish();
}

#[test]
fn the_harness_whose_path_carries_no_version_is_recorded_without_one() {
    // Nothing is run to ask: the path is all there is, and it names none.
    let (rig, pane, _) = Rig::scanned(vec![program(
        12,
        "/nowhere/bin/opencode",
        &["opencode", "--standalone"],
    )]);
    assert_eq!(
        rig.harness_of(&pane),
        Some(TerminalHarness {
            id: "registry.opencode".into(),
            name: "OpenCode".into(),
            version: None,
        })
    );
    rig.finish();
}

#[test]
fn nothing_is_scanned_while_no_app_watches_or_no_terminal_is_open() {
    let mut rig = Rig::new();
    let (pane, instance) = rig.terminal("shell");
    rig.programs
        .set(&instance, vec![program(1, "/bin/zsh", &["zsh"])]);
    rig.core.foreground_mut().note_output(&pane);
    rig.core.advance_foreground().unwrap();
    assert_eq!(rig.programs.asked(), 0, "no app watches");
    rig.finish();

    let mut rig = Rig::new();
    rig.watch();
    rig.core.advance_foreground().unwrap();
    assert_eq!(rig.programs.asked(), 0, "no terminal is open");
    rig.finish();
}

#[test]
fn an_idle_shell_with_an_app_connected_causes_no_scan() {
    let mut rig = Rig::new();
    rig.watch();
    let (pane, instance) = rig.terminal("shell");
    rig.programs
        .set(&instance, vec![program(1, "/bin/zsh", &["zsh"])]);
    // Time goes by and the tick runs: the pane printed nothing, so nothing is read.
    for _ in 0..5 {
        rig.advance(INTERVAL_MS);
        rig.core.advance_foreground().unwrap();
    }
    assert_eq!(rig.programs.asked(), 0);
    let mut next = Next::default();
    rig.core.foreground_due(&mut next, rig.core.now_ms());
    assert!(
        next.soonest().is_none(),
        "the tick does not wake for an idle pane"
    );
    // It prints once: one scan, then idle again.
    rig.core.foreground_mut().note_output(&pane);
    rig.core.advance_foreground().unwrap();
    assert_eq!(rig.programs.asked(), 1);
    rig.advance(INTERVAL_MS);
    rig.core.advance_foreground().unwrap();
    assert_eq!(rig.programs.asked(), 1);
    rig.finish();
}

#[test]
fn a_scan_is_not_repeated_before_its_period_is_up() {
    let (mut rig, pane, _) = Rig::scanned(vec![program(1, "/bin/zsh", &["zsh"])]);
    // It printed again at once: the next read waits for the period.
    rig.core.foreground_mut().note_output(&pane);
    rig.core.advance_foreground().unwrap();
    assert_eq!(rig.programs.asked(), 1);
    rig.rescan(&pane);
    assert_eq!(rig.programs.asked(), 2);
    rig.finish();
}

#[test]
fn a_pane_the_supervisor_cannot_read_keeps_its_harness() {
    let (mut rig, pane, _) =
        Rig::scanned(vec![program(12, "/nowhere/bin/opencode", &["opencode"])]);
    assert!(rig.harness_of(&pane).is_some());
    // No answer for the instance: the supervisor could not read it.
    rig.programs.by_instance.lock().unwrap().clear();
    rig.rescan(&pane);
    assert!(rig.harness_of(&pane).is_some());
    rig.finish();
}

#[test]
fn a_pane_whose_process_ended_loses_the_harness_it_had() {
    let (mut rig, pane, _) =
        Rig::scanned(vec![program(12, "/nowhere/bin/opencode", &["opencode"])]);
    assert!(rig.harness_of(&pane).is_some());
    if let Some(running) = rig.core.panes.get_mut(&pane) {
        running.stored.pane.state = PaneState::Dead;
    }
    rig.rescan(&pane);
    assert_eq!(rig.harness_of(&pane), None);
    rig.finish();
}

#[test]
fn agent_list_names_the_terminals_that_run_a_harness_and_leaves_the_others_out() {
    let mut rig = Rig::new();
    let sender = rig.agent_binding("chief");
    insert_agent_pane(&mut rig.core, &rig.directory, "peer", WORKSPACE);
    let (running, running_instance) = rig.terminal("busy");
    let (_plain, plain_instance) = rig.terminal("plain");
    rig.programs.set(
        &running_instance,
        vec![program(
            13,
            "/h/.local/share/cursor-agent/versions/2026.09.02-c22c1a3/node",
            &[
                "/h/.local/bin/cursor-agent",
                "--use-system-ca",
                "/h/.local/share/cursor-agent/versions/2026.09.02-c22c1a3/index.js",
            ],
        )],
    );
    rig.programs
        .set(&plain_instance, vec![program(1, "/bin/zsh", &["zsh"])]);

    let listed = rig.agent_list(&sender);

    let items = listed["items"].as_array().unwrap();
    // The panes of one workspace are listed by their ids, so the order is not what is checked.
    let mut described: Vec<(&str, &str)> = items
        .iter()
        .map(|item| {
            (
                item["paneId"].as_str().unwrap(),
                item["kind"].as_str().unwrap(),
            )
        })
        .collect();
    described.sort_unstable();
    let mut expected = [(running.as_str(), "terminal"), ("peer", "agent")];
    expected.sort_unstable();
    assert_eq!(described, expected);
    let terminal = items.iter().find(|item| item["paneId"] == running).unwrap();
    assert_eq!(terminal["harness"], "Cursor Agent");
    assert_eq!(terminal["name"], "busy");
    assert_eq!(rig.programs.asked(), 1);

    // A second list in the same second answers from what was read.
    rig.agent_list(&sender);
    assert_eq!(rig.programs.asked(), 1);
    rig.finish();
}

#[test]
fn agent_list_names_the_terminals_of_every_workspace_with_the_workspace() {
    let mut rig = Rig::new();
    let sender = rig.agent_binding("chief");
    let (other, instance) = rig.terminal("elsewhere");
    if let Some(running) = rig.core.panes.get_mut(&other) {
        running.stored.pane.workspace_id = "another-workspace".into();
    }
    rig.programs.set(
        &instance,
        vec![program(12, "/nowhere/bin/opencode", &["opencode"])],
    );
    let listed = rig.agent_list(&sender);
    let items = listed["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["paneId"], other);
    assert_eq!(items[0]["kind"], "terminal");
    assert_eq!(items[0]["harness"], "OpenCode");
    assert!(items[0]["workspaceName"].is_string());
    assert_eq!(rig.programs.asked(), 1);
    rig.finish();
}

#[test]
fn a_supervisor_that_does_not_list_the_feature_is_never_asked() {
    let mut rig = Rig::new();
    rig.watch();
    let (pane, instance) = rig.terminal("shell");
    rig.programs.set(
        &instance,
        vec![program(12, "/nowhere/bin/opencode", &["opencode"])],
    );
    // A supervisor that was running before this engine: its welcome lists no features.
    rig.core.link_supervisor(rig.programs.answering());
    rig.core.foreground_mut().note_output(&pane);
    rig.core.advance_foreground().unwrap();
    assert_eq!(rig.programs.asked(), 0);
    assert_eq!(rig.harness_of(&pane), None);
    let sender = rig.agent_binding("chief");
    rig.assert_nothing_listed(&sender);
    rig.finish();
}

#[test]
fn agent_list_answers_with_what_is_known_while_the_supervisor_is_away() {
    let mut rig = Rig::new();
    let sender = rig.agent_binding("chief");
    insert_agent_pane(&mut rig.core, &rig.directory, "peer", WORKSPACE);
    let (shell, instance) = rig.terminal("busy");
    rig.programs.set(
        &instance,
        vec![program(12, "/nowhere/bin/opencode", &["opencode"])],
    );
    // The terminal was known to run a harness before the supervisor went away.
    rig.core
        .set_terminal_harness(
            &shell,
            Some(TerminalHarness {
                id: "registry.opencode".into(),
                name: "OpenCode".into(),
                version: None,
            }),
        )
        .unwrap();
    rig.core.ptyd = None;
    let listed = rig.agent_list(&sender);
    let mut ids: Vec<&str> = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["paneId"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    let mut expected = [shell.as_str(), "peer"];
    expected.sort_unstable();
    assert_eq!(ids, expected);
    assert_eq!(rig.programs.asked(), 0);
    rig.finish();
}

#[test]
fn a_sign_in_terminal_is_never_read_or_listed() {
    let mut rig = Rig::new();
    let sender = rig.agent_binding("chief");
    let (login, instance) = rig.terminal("Sign in — Codex");
    rig.programs.set(
        &instance,
        vec![program(12, "/nowhere/bin/codex", &["codex", "login"])],
    );
    if let Some(agent) = rig.core.agents.get_mut("chief") {
        agent.state.login_pane_id = Some(login.clone());
    }
    let listed = rig.agent_list(&sender);
    assert!(listed["items"].as_array().unwrap().is_empty());
    assert_eq!(rig.programs.asked(), 0);
    rig.finish();
}
