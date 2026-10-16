#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;
use soyeht_ptyd::{
    client::Client,
    wire::{Request, Response},
};
use soyeht_store::test_support::DatabaseProbe;
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// A binary built next to the `soyeht` one under test.
fn sibling_binary(name: &str) -> PathBuf {
    soyeht_platform::testing::sibling_binary(env!("CARGO_BIN_EXE_soyeht"), name)
}

// Positive asynchronous assertions may use the full failure budget; passing
// runs stop as soon as the expected signal arrives.
const ASYNC_ASSERTION_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs each program the tests start once before any deadline runs: the first run of a new
/// binary is assessed by the system, which takes seconds under load.
fn warm_up_binaries() {
    for binary in [
        sibling_binary("soyeht-ptyd"),
        sibling_binary("soyeht-engine"),
        sibling_binary("soyeht-fake-acp"),
        PathBuf::from(env!("CARGO_BIN_EXE_soyeht")),
    ] {
        let took = soyeht_platform::testing::warm_up(&binary);
        if took > Duration::from_secs(1) {
            eprintln!("first run of {} took {took:?}", binary.display());
        }
    }
}

struct Rig {
    path: PathBuf,
    engine: Child,
    ptyd: Child,
}
impl Rig {
    fn start() -> Self {
        warm_up_binaries();
        // A Unix socket path is limited to about a hundred bytes; the temporary folder of macOS is long.
        let root = if cfg!(unix) {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        };
        let path = root.join(format!("sn-cli-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        let path = soyeht_platform::fs::canonicalize(&path).unwrap();
        let ptyd = Command::new(sibling_binary("soyeht-ptyd"))
            .arg("--data-dir")
            .arg(&path)
            .stdout(Stdio::null())
            .spawn()
            .expect("build workspace binaries first");
        let engine = Command::new(sibling_binary("soyeht-engine"))
            .args(["--listen", "127.0.0.1:0", "--data-dir"])
            .arg(&path)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let rig = Self { path, engine, ptyd };
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        while soyeht_cli::Client::connect(&rig.path).is_err() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        while rig.command(&["status"])["ptyd"]["connected"] != true {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        rig
    }
    fn command(&self, args: &[&str]) -> Value {
        self.command_with_binding(args, None)
    }
    fn command_with_binding(&self, args: &[&str], binding: Option<&str>) -> Value {
        let output = self.run(args, binding);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn run(&self, args: &[&str], binding: Option<&str>) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_soyeht"));
        command
            .args(["--profile", "dev", "--data-dir"])
            .arg(&self.path);
        if binding.is_some() {
            command.arg("--bound");
        }
        command
            .args(args)
            .env_remove("SOYEHT_MCP_BINDING")
            .envs(binding.map(|value| ("SOYEHT_MCP_BINDING", value)))
            .current_dir(&self.path)
            .output()
            .unwrap()
    }
}

fn create_task(rig: &Rig, workspace: &str) -> Value {
    rig.command(&[
        "task",
        "create",
        workspace,
        "Document the flow",
        "--description",
        "Keep the operator informed",
        "--how-to-verify",
        "Run the focused test",
        "--must-not-change",
        "Keep terminal panes available",
        "--priority",
        "high",
        "--create-worktree",
        "false",
        "--idempotency-key",
        "task-create",
    ])
}

fn assert_bound_loop_disabled(rig: &Rig, pane: &str, binding: &str) {
    assert_eq!(
        rig.command_with_binding(
            &[
                "loop",
                "set",
                pane,
                "Pull the next task",
                "30",
                "10",
                "explicit_off",
                "false",
                "--idempotency-key",
                "loop-off",
            ],
            Some(binding),
        )["config"]["enabled"],
        false
    );
}

fn wait_for_agent_options(rig: &Rig) -> Value {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let list = rig.command(&["pane", "list"]);
        let current = &list["items"][0];
        let ready = current["agent"]["options"]
            .as_array()
            .is_some_and(|options| {
                options
                    .iter()
                    .any(|o| o["id"] == "model" && o["currentValue"] == "model-b")
                    && options
                        .iter()
                        .any(|o| o["id"] == "effort" && o["currentValue"] == "high")
            });
        if ready {
            return current.clone();
        }
        assert!(
            Instant::now() < deadline,
            "initial CLI selectors not applied"
        );
        thread::sleep(Duration::from_millis(20));
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.engine.kill();
        let _ = self.engine.wait();
        if let Ok(mut client) = Client::connect(&self.path.join("ptyd.sock"), u64::MAX)
            && let Ok(Response::Instances(instances)) = client.request(Request::List)
        {
            for instance in instances {
                let _ = client.request(Request::Close {
                    instance_id: instance.instance_id,
                });
            }
        }
        let _ = self.ptyd.kill();
        let _ = self.ptyd.wait();
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn cli_creates_lists_and_closes_a_real_shell_and_starts_pairing() {
    let rig = Rig::start();
    assert_eq!(rig.command(&["status"])["ptyd"]["connected"], true);
    let first = rig.command(&[
        "pane",
        "create",
        "--shell",
        "--idempotency-key",
        "cli-test-create",
    ]);
    assert_eq!(
        first,
        rig.command(&[
            "pane",
            "create",
            "--shell",
            "--idempotency-key",
            "cli-test-create"
        ])
    );
    let list = rig.command(&["pane", "list"]);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["cwd"], rig.path.to_str().unwrap());
    let pairing = rig.command(&["device", "pair-start"]);
    assert!(
        pairing["uri"]
            .as_str()
            .unwrap()
            .starts_with("soyeht-next://pair?")
    );
    assert!(!pairing["code"].as_str().unwrap().is_empty());
    assert!(
        rig.command(&["device", "list"])["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    rig.command(&["pane", "close", first["paneId"].as_str().unwrap()]);
    assert!(
        rig.command(&["pane", "list"])["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn cli_opens_edits_searches_and_reads_history_from_a_real_editor_pane() {
    let rig = Rig::start();
    fs::write(rig.path.join("note.txt"), "one\n").unwrap();
    let created = rig.command(&[
        "pane",
        "create",
        "--editor",
        "note.txt",
        "--idempotency-key",
        "editor-pane",
    ]);
    let panes = rig.command(&["pane", "list"]);
    let pane = &panes["items"][0];
    assert_eq!(pane["paneId"], created["paneId"]);
    assert_eq!(pane["mode"], "editor");
    let workspace = pane["workspaceId"].as_str().unwrap();
    let opened = rig.command(&[
        "file",
        "open",
        workspace,
        "note.txt",
        "--line",
        "0",
        "--column",
        "0",
        "--idempotency-key",
        "file-open",
    ]);
    let file = opened["file"]["fileId"].as_str().unwrap();
    assert_eq!(
        rig.command(&["file", "read", file, "--lines", "1"])["lines"][0]["text"],
        "one"
    );
    let written = rig.command(&[
        "file",
        "write",
        file,
        "1",
        "0",
        "0",
        "0",
        "0",
        "two ",
        "--idempotency-key",
        "file-write",
    ]);
    assert_eq!(written["file"]["revision"], "2");
    assert_eq!(
        rig.command(&["file", "search", workspace, "two"])["items"][0]["path"],
        "note.txt"
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    let history = loop {
        let history = rig.command(&["file", "history", file]);
        if !history["items"].as_array().unwrap().is_empty() {
            break history;
        }
        assert!(
            Instant::now() < deadline,
            "editor autosave did not reach history"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let version = history["items"][0]["versionId"].as_str().unwrap();
    assert_eq!(
        rig.command(&["file", "revert", file, version, "2"])["file"]["revision"],
        "3"
    );
    assert_eq!(
        rig.command(&["usage", "list"])["providers"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn another_profile_is_refused_before_connecting() {
    let output = Command::new(env!("CARGO_BIN_EXE_soyeht"))
        .args(["--profile", "other", "status"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("only the new dev profile"));
    assert!(output.stdout.is_empty());
}

#[test]
fn bound_mode_requires_the_inherited_session_binding() {
    let output = Command::new(env!("CARGO_BIN_EXE_soyeht"))
        .args(["--bound", "status"])
        .env_remove("SOYEHT_MCP_BINDING")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("SOYEHT_MCP_BINDING"));
    assert!(output.stdout.is_empty());
}

#[test]
fn cli_task_commands_mutate_and_query_the_real_store() {
    let rig = Rig::start();
    let pane = rig.command(&[
        "pane",
        "create",
        "--shell",
        "--idempotency-key",
        "task-workspace",
    ]);
    let panes = rig.command(&["pane", "list"]);
    let workspace = panes["items"][0]["workspaceId"].as_str().unwrap();
    let created = create_task(&rig, workspace);
    assert_eq!(created["task"]["taskId"], "WS-1");
    assert_eq!(created["task"]["createdBy"], "you");
    assert_eq!(created, create_task(&rig, workspace));
    let task = created["task"]["taskId"].as_str().unwrap();
    assert_eq!(
        rig.command(&["task", "list", workspace, "--state", "todo"])["items"][0]["taskId"],
        task
    );
    let updated = rig.command(&[
        "task",
        "update",
        workspace,
        task,
        "1",
        "--title",
        "Document the complete flow",
        "--priority",
        "urgent",
        "--idempotency-key",
        "task-update",
    ]);
    assert_eq!(updated["task"]["version"], 2);
    assert_eq!(updated["task"]["priority"], "urgent");
    assert_eq!(
        rig.command(&[
            "task",
            "comment",
            workspace,
            task,
            "First milestone",
            "--idempotency-key",
            "task-comment",
        ])["comment"]["author"],
        "you"
    );
    let subitem = rig.command(&[
        "task",
        "subitem",
        "add",
        workspace,
        task,
        "Run the focused test",
        "--idempotency-key",
        "subitem-add",
    ]);
    let subitem_id = subitem["subitem"]["subitemId"].as_str().unwrap();
    assert_eq!(
        rig.command(&[
            "task",
            "subitem",
            "done",
            workspace,
            task,
            subitem_id,
            "--idempotency-key",
            "subitem-done",
        ])["subitem"]["state"],
        "done"
    );
    let shown = rig.command(&["task", "show", workspace, task]);
    assert_eq!(shown["comments"][0]["text"], "First milestone");
    assert_eq!(shown["subitems"][0]["state"], "done");
    assert_eq!(
        rig.command(&[
            "task",
            "move",
            workspace,
            task,
            "cancelled",
            "No longer needed",
            "--idempotency-key",
            "task-move",
        ])["task"]["state"],
        "cancelled"
    );
    let clean = rig.command(&[
        "task",
        "worktree-clean",
        workspace,
        task,
        "--idempotency-key",
        "task-clean",
    ]);
    assert_eq!(clean["cleaned"], false);
    rig.command(&["pane", "close", pane["paneId"].as_str().unwrap()]);
}

#[test]
fn client_reconnects_after_engine_restart() {
    let mut rig = Rig::start();
    let mut client = soyeht_cli::Client::connect(&rig.path).unwrap();
    let before: soyeht_protocol::EngineStatusResult = client
        .request("engine.status", serde_json::json!({}))
        .unwrap();
    rig.engine.kill().unwrap();
    rig.engine.wait().unwrap();
    rig.engine = Command::new(sibling_binary("soyeht-engine"))
        .args(["--listen", "127.0.0.1:0", "--data-dir"])
        .arg(&rig.path)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    // Wait on a separate connection so a slow engine start on a busy host does not
    // exhaust the client's reconnect budget; `client` still has to reconnect itself.
    let started = Instant::now();
    let deadline = started + ASYNC_ASSERTION_TIMEOUT;
    while soyeht_cli::Client::connect(&rig.path).is_err() {
        // An engine that ended will never accept: say how it ended instead of waiting.
        let ended = rig.engine.try_wait().unwrap();
        assert!(
            ended.is_none(),
            "the restarted engine ended with {ended:?} after {:?}",
            started.elapsed()
        );
        assert!(
            Instant::now() < deadline,
            "restarted engine never accepted a connection in {:?}",
            started.elapsed()
        );
        thread::sleep(Duration::from_millis(10));
    }
    let after: soyeht_protocol::EngineStatusResult = client
        .request("engine.status", serde_json::json!({}))
        .unwrap();
    assert!(after.generation > before.generation);
    assert_eq!(after.db.id, before.db.id);
}

#[test]
fn cli_agent_controls_use_the_engine_session_and_native_permission_options() {
    let rig = Rig::start();
    let binary = sibling_binary("soyeht-fake-acp");
    fs::write(
        rig.path.join("harnesses.json"),
        serde_json::to_vec(&serde_json::json!([{
            "id":"test-agent","name":"Test agent","command":binary,
            "args":["--state-dir",rig.path.join("native")],"version":"1"
        }]))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        rig.command(&["harness", "list"])["items"][0]["id"],
        "test-agent"
    );
    let created = rig.command(&[
        "pane",
        "create",
        "--agent",
        "test-agent",
        "--model",
        "model-b",
        "--effort",
        "high",
        "--permission-mode",
        "ask",
        "--idempotency-key",
        "agent-create",
    ]);
    let pane = created["paneId"].as_str().unwrap();
    let before = wait_for_agent_options(&rig);
    assert_eq!(
        rig.command(&["model", "list", "test-agent", "--pane", pane])["source"],
        "native_session"
    );
    assert_eq!(
        rig.command(&["pane", "config", pane, "effort", "low"])["effective"],
        "low"
    );
    assert_eq!(
        rig.command(&["pane", "config", pane, "fast", "true"])["effective"],
        true
    );
    let prompt = rig.command(&[
        "pane",
        "prompt",
        pane,
        "permission",
        "--idempotency-key",
        "ask",
    ]);
    assert_eq!(
        prompt,
        rig.command(&[
            "pane",
            "prompt",
            pane,
            "permission",
            "--idempotency-key",
            "ask"
        ])
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while rig.command(&["pane", "list"])["items"][0]["state"] != "waiting_for_you" {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        rig.command(&["pane", "permission", pane, "allow"])["optionId"],
        "once"
    );
    while rig.command(&["pane", "list"])["items"][0]["state"] != "idle" {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        rig.command(&["pane", "mode", pane, "accept_edits"])["effective"],
        "accept_edits"
    );
    let binding = DatabaseProbe::open(&rig.path)
        .unwrap()
        .agent_binding(pane)
        .unwrap();
    assert_bound_loop_disabled(&rig, pane, &binding);
    let transcript = rig.command(&["pane", "transcript", pane, "--limit", "1"]);
    assert_eq!(transcript["items"].as_array().unwrap().len(), 1);
    let cursor = transcript["nextCursor"]
        .as_str()
        .expect("older transcript page");
    let older = rig.command(&[
        "pane",
        "transcript",
        pane,
        "--before",
        cursor,
        "--limit",
        "1",
    ]);
    assert_ne!(older["items"][0]["seq"], transcript["items"][0]["seq"]);
    let after = rig.command(&["pane", "list"])["items"][0].clone();
    assert_eq!(after["pid"], before["pid"]);
    assert_eq!(after["agent"]["sessionId"], before["agent"]["sessionId"]);
    rig.command(&["pane", "close", pane]);
}

#[test]
fn an_editor_pane_opens_a_file_named_by_an_absolute_path_inside_its_directory() {
    let rig = Rig::start();
    let project = rig.path.join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    let note = project.join("src/note.txt");
    fs::write(&note, "one\n").unwrap();
    let (cwd, note) = (project.to_str().unwrap(), note.to_str().unwrap());
    // No workspace has this directory yet: as for a terminal, creating the pane creates it.
    assert!(
        rig.command(&["pane", "list"])["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let created = rig.command(&["pane", "create", "--editor", note, "--cwd", cwd]);
    let listed = rig.command(&["pane", "list"]);
    let items = listed["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        (&items[0]["paneId"], &items[0]["mode"], &items[0]["cwd"]),
        (
            &created["paneId"],
            &Value::from("editor"),
            &Value::from(cwd)
        )
    );
    // The relative form names the same file, so it is the same pane.
    let again = rig.command(&["pane", "create", "--editor", "src/note.txt", "--cwd", cwd]);
    assert_eq!(again["paneId"], created["paneId"]);
}

#[test]
fn an_editor_pane_refuses_a_file_outside_its_directory() {
    let rig = Rig::start();
    let project = rig.path.join("project");
    fs::create_dir(&project).unwrap();
    let outside = rig.path.join("outside.txt");
    fs::write(&outside, "secret\n").unwrap();
    let output = rig.run(
        &[
            "pane",
            "create",
            "--editor",
            outside.to_str().unwrap(),
            "--cwd",
            project.to_str().unwrap(),
        ],
        None,
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("escapes the workspace"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        rig.command(&["pane", "list"])["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
