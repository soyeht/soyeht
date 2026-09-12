#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::{Task, frame::Frame};
use soyeht_ptyd::{
    client::Client as PtyClient,
    wire::{Request, Response},
};
use soyeht_store::{
    database::Store,
    tasks::{ReviewStage, TaskReviewRecord},
    test_support::DatabaseProbe,
};
use std::{
    fs,
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

struct Rig {
    path: PathBuf,
    engine: Option<Child>,
    ptyd: Child,
}
impl Rig {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sn-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        install_fake_worktrunk(&path);
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine"));
        let ptyd = Command::new(binary.with_file_name("soyeht-ptyd"))
            .arg("--data-dir")
            .arg(&path)
            .env("ZDOTDIR", &path)
            .stdout(Stdio::null())
            .spawn()
            .expect("build workspace binaries before engine integration tests");
        let mut rig = Self {
            path,
            engine: None,
            ptyd,
        };
        rig.start();
        rig
    }
    fn start(&mut self) {
        let path = std::env::join_paths(std::iter::once(self.path.join("bin")).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        self.engine = Some(
            Command::new(env!("CARGO_BIN_EXE_soyeht-engine"))
                .args(["--listen", "127.0.0.1:0", "--data-dir"])
                .arg(&self.path)
                .env("PATH", path)
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while UnixStream::connect(self.path.join("engine.sock")).is_err() {
            assert!(Instant::now() < deadline, "engine ready timeout");
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn install_fake_harness(&self) {
        let binary =
            PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine")).with_file_name("soyeht-fake-acp");
        fs::write(
            self.path.join("harnesses.json"),
            serde_json::to_vec(&json!([{
                "id":"test-agent","name":"Test agent","command":binary,
                "args":["--state-dir",self.path.join("native")],"version":"test"
            }]))
            .unwrap(),
        )
        .unwrap();
    }
    fn open_agent(&self, mut client: Client, workspace: &Value) -> (Client, String) {
        self.install_fake_harness();
        let pane = client.call(
            "pane.create",
            json!({"workspaceId":workspace,"mode":"agent",
            "harness":"test-agent","idempotencyKey":Uuid::new_v4().to_string()}),
        )["paneId"]
            .as_str()
            .unwrap()
            .to_owned();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !client.pane(&pane)["agent"]["sessionId"].is_string() {
            assert!(Instant::now() < deadline, "agent session did not open");
            thread::sleep(Duration::from_millis(20));
        }
        (client, pane)
    }
    fn agent(&self) -> (Client, String) {
        let mut client = self.client();
        let workspace = client.call(
            "workspace.create",
            json!({"name":"Test","path":self.path,
            "idempotencyKey":"agent-workspace"}),
        )["workspace"]["workspaceId"]
            .clone();
        self.open_agent(client, &workspace)
    }
    fn agent_in_workspace(&self, workspace: &str) -> (Client, String) {
        self.open_agent(self.client(), &json!(workspace))
    }
    fn database_probe(&self) -> DatabaseProbe {
        DatabaseProbe::open(&self.path).unwrap()
    }
    fn native_session(&self, session_id: &str) -> Value {
        serde_json::from_slice(
            &fs::read(self.path.join("native").join(format!("{session_id}.json"))).unwrap(),
        )
        .unwrap()
    }
    // Shift only a stopped test engine's persisted timestamps. There is no
    // runtime clock override or shortened deadline in the shipped executable.
    fn age_agent(&self, pane: &str, cancel: bool) {
        assert!(self.engine.is_none());
        self.database_probe().age_agent(pane, cancel).unwrap();
    }
    fn kill_engine(&mut self) {
        if let Some(mut child) = self.engine.take() {
            child.kill().unwrap();
            child.wait().unwrap();
        }
        let _ = fs::remove_file(self.path.join("engine.sock"));
    }
    fn client(&self) -> Client {
        Client::new(self.path.join("engine.sock"))
    }
}

fn install_fake_worktrunk(root: &std::path::Path) {
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let executable = bin.join("wt");
    fs::write(
        &executable,
        r#"#!/bin/sh
repository="$2"
command="$3"
if [ "$command" = "list" ]; then
  printf '[]\n'
  exit 0
fi
branch=""
for argument in "$@"; do branch="$argument"; done
safe_branch=$(printf '%s' "$branch" | tr '/' '-')
path="$repository/.test-worktrees/$safe_branch"
mkdir -p "$path"
printf '{"path":"%s","branch":"%s"}\n' "$path" "$branch"
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(executable, permissions).unwrap();
}

fn binding_hello(path: &std::path::Path, binding: &str) -> (UnixStream, Value) {
    let mut socket = UnixStream::connect(path).unwrap();
    let challenge = Frame::read_from(&mut socket).unwrap().unwrap();
    assert!(matches!(challenge, Frame::Json(_)));
    Frame::Json(
        serde_json::to_vec(&json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"hello",
            "params":{
                "client":"binding-test",
                "clientVersion":"1",
                "protocolVersion":1,
                "binding":binding
            }
        }))
        .unwrap(),
    )
    .write_to(&mut socket)
    .unwrap();
    let Some(Frame::Json(response)) = Frame::read_from(&mut socket).unwrap() else {
        panic!("binding hello response")
    };
    (socket, serde_json::from_slice(&response).unwrap())
}

fn binding_request(socket: &mut UnixStream, id: u64, method: &str, params: Value) -> Value {
    Frame::Json(
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .unwrap(),
    )
    .write_to(&mut *socket)
    .unwrap();
    loop {
        let Some(Frame::Json(response)) = Frame::read_from(&mut *socket).unwrap() else {
            panic!("binding response")
        };
        let response: Value = serde_json::from_slice(&response).unwrap();
        if response["id"] == id {
            return response;
        }
    }
}

fn reserve_task(rig: &Rig, workspace: &str, pane: &str, key: &str) -> Task {
    let mut store = Store::open(&rig.path).unwrap();
    let result = store
        .mutate("test", key, key, |change| {
            let task = change.reserve_next_task(
                workspace,
                pane,
                "test-agent",
                "test-model",
                "2026-09-12T00:00:00Z",
            )?;
            Ok((json!(task), vec![]))
        })
        .unwrap()
        .result;
    serde_json::from_value(result).unwrap()
}

struct TaskLifecycleRig {
    rig: Rig,
    human: Client,
    executor: UnixStream,
    workspace: String,
    pane: String,
    repository: PathBuf,
    generation: u32,
    next_id: u64,
}

impl TaskLifecycleRig {
    fn new() -> Self {
        let rig = Rig::new();
        let repository = initialize_review_repository(&rig.path);
        let mut human = rig.client();
        let workspace = human.call(
            "workspace.create",
            json!({"name":"Lifecycle","path":repository,"idempotencyKey":"lifecycle-workspace"}),
        )["workspace"]["workspaceId"]
            .as_str()
            .unwrap()
            .to_owned();
        let (agent, pane) = rig.agent_in_workspace(&workspace);
        drop(agent);
        let generation = human.pane(&pane)["generation"].as_u64().unwrap() as u32;
        let binding = rig.database_probe().agent_binding(&pane).unwrap();
        let (executor, welcome) = binding_hello(&rig.path.join("engine.sock"), &binding);
        assert_eq!(welcome["result"]["role"], "agent");
        Self {
            rig,
            human,
            executor,
            workspace,
            pane,
            repository,
            generation,
            next_id: 10,
        }
    }

    fn create_and_reserve(&mut self, title: &str, key: &str) -> Task {
        self.human.call(
            "task.create",
            json!({
                "workspaceId":self.workspace,"title":title,"mustNotChange":"Keep behavior",
                "idempotencyKey":format!("create-{key}")
            }),
        );
        let task = reserve_task(&self.rig, &self.workspace, &self.pane, key);
        fs::write(
            self.repository.join("src/lib.rs"),
            "pub fn calculate(value: u32) -> u32 {\n    value + 2\n}\n",
        )
        .unwrap();
        task
    }

    fn executor_request(&mut self, method: &str, task: &Task, fields: Value, key: &str) -> Value {
        let mut params = fields.as_object().unwrap().clone();
        params.insert("workspaceId".into(), json!(self.workspace));
        params.insert("taskId".into(), json!(task.task_id));
        params.insert("paneId".into(), json!(self.pane));
        params.insert("sessionGeneration".into(), json!(self.generation));
        params.insert("attempt".into(), json!(task.current_attempt));
        params.insert("idempotencyKey".into(), json!(key));
        self.next_id += 1;
        binding_request(
            &mut self.executor,
            self.next_id,
            method,
            Value::Object(params),
        )
    }

    fn wait_task_state(&mut self, task: &str, state: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let shown = self.human.call(
                "task.show",
                json!({"workspaceId":self.workspace,"taskId":task}),
            );
            if shown["task"]["state"] == state {
                return shown;
            }
            assert!(Instant::now() < deadline, "expected task {state}: {shown}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn review(&self, task: &Task) -> TaskReviewRecord<Value> {
        Store::open(&self.rig.path)
            .unwrap()
            .task_review(&self.workspace, &task.task_id, task.current_attempt)
            .unwrap()
            .unwrap()
    }

    fn wait_review_stage(&self, task: &Task, stage: ReviewStage) -> TaskReviewRecord<Value> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let review = self.review(task);
            if review.stage == stage {
                return review;
            }
            assert!(Instant::now() < deadline, "expected review stage {stage:?}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn reconnect_executor(&mut self) {
        let binding = self.rig.database_probe().agent_binding(&self.pane).unwrap();
        let (executor, welcome) = binding_hello(&self.rig.path.join("engine.sock"), &binding);
        assert_eq!(welcome["result"]["role"], "agent");
        self.executor = executor;
        self.generation = self.human.pane(&self.pane)["generation"].as_u64().unwrap() as u32;
    }
}

fn initialize_review_repository(root: &std::path::Path) -> PathBuf {
    let repository = root.join("repository");
    fs::create_dir_all(repository.join("src")).unwrap();
    let run = |arguments: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(arguments)
            .status()
            .unwrap();
        assert!(status.success());
    };
    run(&["init", "-b", "main"]);
    fs::write(
        repository.join("src/lib.rs"),
        "pub fn calculate(value: u32) -> u32 {\n    value + 1\n}\n",
    )
    .unwrap();
    run(&["add", "."]);
    let status = Command::new("git")
        .arg("-C")
        .arg(&repository)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "baseline",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    run(&["switch", "-c", "task/test"]);
    repository
}
impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(mut child) = self.engine.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Ok(mut client) = PtyClient::connect(&self.path.join("ptyd.sock"), u64::MAX)
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
struct Client {
    socket: UnixStream,
    next: u64,
    messages: Vec<Frame>,
}

#[test]
fn configuration_timeout_preserves_late_confirmation_without_retrying_or_restarting() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let before = client.pane(&pane);
    let params = json!({"paneId":pane,"configId":"model","value":"delayed-model","idempotencyKey":"delayed-config"});
    Frame::Json(
        serde_json::to_vec(
            &json!({"jsonrpc":"2.0","id":9000,"method":"pane.config.set","params":params}),
        )
        .unwrap(),
    )
    .write_to(&mut client.socket)
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !rig.path.join("native/config-waiting").exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    rig.kill_engine();
    rig.age_agent(&pane, false);
    rig.start();
    let mut client = rig.client();
    client.wait_state(&pane, "no_signal");
    let timeout = client.request("pane.config.set", params.clone());
    assert_eq!(timeout["error"]["data"]["kind"], "harness_unavailable");
    assert!(
        timeout["error"]["message"]
            .as_str()
            .unwrap()
            .contains("timed out")
    );
    let pending = client.pane(&pane);
    assert_eq!(pending["pid"], before["pid"]);
    assert_eq!(pending["agent"]["sessionId"], before["agent"]["sessionId"]);
    let conflicting = client.request("pane.config.set", json!({"paneId":pane,"configId":"model","value":"model-b","idempotencyKey":"conflicting-config"}));
    assert_eq!(conflicting["error"]["data"]["kind"], "pane_busy");
    fs::write(rig.path.join("native/config-release"), b"ready").unwrap();
    loop {
        let current = client.pane(&pane);
        if current["agent"]["options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["id"] == "model" && o["currentValue"] == "delayed-model")
        {
            assert_eq!(current["pid"], before["pid"]);
            assert_eq!(current["agent"]["sessionId"], before["agent"]["sessionId"]);
            break;
        }
        assert!(Instant::now() < deadline, "late confirmation was lost");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        client.request("pane.config.set", params)["error"],
        timeout["error"]
    );
    client.call("pane.config.set", json!({"paneId":pane,"configId":"model","value":"model-b","idempotencyKey":"after-timeout"}));
}

#[test]
fn cancellation_pauses_the_durable_queue_until_explicit_resume() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"silent","idempotencyKey":"first"}),
    );
    client.wait_state(&pane, "working");
    let waiting = client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"after-resume","idempotencyKey":"second"}),
    );
    client.call(
        "pane.cancel",
        json!({"paneId":pane,"idempotencyKey":"interrupt"}),
    );
    client.wait_state(&pane, "idle");
    let queue = client.call("pane.queue.list", json!({"paneId":pane}));
    assert_eq!(queue["paused"], true);
    assert_eq!(queue["items"].as_array().unwrap().len(), 1);
    assert_eq!(queue["items"][0]["promptId"], waiting["promptId"]);
    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    assert_eq!(client.pane(&pane)["agent"]["queuePaused"], true);
    let still_waiting = client.call("pane.queue.list", json!({"paneId":pane}));
    assert_eq!(still_waiting["items"], queue["items"]);
    let params = json!({"paneId":pane,"idempotencyKey":"resume"});
    assert_eq!(
        client.call("pane.queue.resume", params.clone()),
        client.call("pane.queue.resume", params)
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let ended = rig
            .database_probe()
            .event_count_by_field(
                "pane.turn_ended",
                "promptId",
                waiting["promptId"].as_str().unwrap(),
            )
            .unwrap();
        if ended == 1 {
            break;
        }
        assert!(Instant::now() < deadline, "resumed queue did not run");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(client.pane(&pane)["agent"]["queuePaused"], false);
    let events = rig
        .database_probe()
        .event_count("pane.queue.resumed")
        .unwrap();
    assert_eq!(events, 1);
}

#[test]
fn terminal_spawn_refusal_finishes_the_native_request_without_restarting_the_agent() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let before = client.pane(&pane);
    let prompt = client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"terminal-missing","idempotencyKey":"missing"}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let ended = rig
            .database_probe()
            .event_count_by_field(
                "pane.turn_ended",
                "promptId",
                prompt["promptId"].as_str().unwrap(),
            )
            .unwrap();
        if ended == 1 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native spawn error left the agent waiting"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(client.pane(&pane)["state"], "idle");
    assert_eq!(client.pane(&pane)["pid"], before["pid"]);
    assert_eq!(
        client.pane(&pane)["agent"]["sessionId"],
        before["agent"]["sessionId"]
    );
    let response = rig
        .database_probe()
        .request_result("terminal/create", &pane)
        .unwrap()
        .unwrap();
    assert_eq!(response["native"]["error"]["code"], -32603);
    let pending = rig.database_probe().open_request_count(&pane).unwrap();
    assert_eq!(pending, 0);
}

#[test]
fn a_lost_output_page_preserves_pending_permission_and_declares_the_gap() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let before = client.pane(&pane);
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission-overflow","idempotencyKey":"overflow"}),
    );
    client.wait_state(&pane, "working");
    rig.kill_engine();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !rig.path.join("native/overflow.done").exists() {
        assert!(
            Instant::now() < deadline,
            "fake did not fill output retention"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let generation = rig.database_probe().engine_generation().unwrap();
    let mut supervisor = PtyClient::connect(&rig.path.join("ptyd.sock"), generation).unwrap();
    let Response::Instances(instances) = supervisor.request(Request::List).unwrap() else {
        panic!("process registry");
    };
    let instance = instances
        .iter()
        .find(|i| json!(i.pid) == before["pid"])
        .unwrap();
    let Response::PendingRequests {
        items,
        recovery_gap,
    } = supervisor
        .request(Request::PendingRequests {
            instance_id: instance.instance_id.clone(),
        })
        .unwrap()
    else {
        panic!("pending registry");
    };
    assert!(!recovery_gap);
    let pending = items
        .iter()
        .find(|r| r.method == "session/request_permission")
        .unwrap();
    let Response::Output(output) = supervisor
        .request(Request::Drain {
            instance_id: instance.instance_id.clone(),
            from_seq: pending.from_seq,
        })
        .unwrap()
    else {
        panic!("retained output");
    };
    assert!(
        output
            .gap
            .as_ref()
            .is_some_and(|gap| gap.to_seq >= pending.to_seq),
        "permission was not actually lost from byte retention"
    );
    drop(supervisor);
    rig.start();
    let mut client = rig.client();
    client.wait_state(&pane, "waiting_for_you");
    let requests = client.section("requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(client.pane(&pane)["pid"], before["pid"]);
    let history = client.call("pane.transcript", json!({"paneId":pane}));
    assert!(history["items"].as_array().unwrap().iter().any(|m| {
        m["role"] == "system"
            && m["text"]
                .as_str()
                .is_some_and(|text| text.contains("missing section"))
    }));
    let request = requests[0]["request"]["requestId"].clone();
    client.call("pane.permission.answer", json!({"paneId":pane,"requestId":request,"optionId":"once","idempotencyKey":"answer-overflow"}));
    client.wait_state(&pane, "idle");
    assert!(client.section("requests").is_empty());
    let count = rig
        .database_probe()
        .event_count_by_field(
            "pane.permission_requested",
            "requestId",
            request.as_str().unwrap(),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn a_permission_created_during_engine_outage_is_recovered_once() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let before = client.pane(&pane);
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission-delayed","idempotencyKey":"during-outage"}),
    );
    client.wait_state(&pane, "working");
    assert!(client.section("requests").is_empty());
    rig.kill_engine();
    let generation = rig.database_probe().engine_generation().unwrap();
    let mut supervisor = PtyClient::connect(&rig.path.join("ptyd.sock"), generation).unwrap();
    let Response::Instances(instances) = supervisor.request(Request::List).unwrap() else {
        panic!("process registry");
    };
    let instance = instances
        .iter()
        .find(|i| json!(i.pid) == before["pid"])
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Response::PendingRequests {
            items,
            recovery_gap,
        } = supervisor
            .request(Request::PendingRequests {
                instance_id: instance.instance_id.clone(),
            })
            .unwrap()
        else {
            panic!("pending requests");
        };
        assert!(!recovery_gap);
        if items
            .iter()
            .any(|r| r.method == "session/request_permission")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "fake did not ask while engine was absent"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let persisted = rig
        .database_probe()
        .request_count("session/request_permission")
        .unwrap();
    assert_eq!(persisted, 0, "request predates engine outage");
    drop(supervisor);
    rig.start();
    let mut first = rig.client();
    let mut second = rig.client();
    first.wait_state(&pane, "waiting_for_you");
    let requests = first.section("requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests, second.section("requests"));
    assert_eq!(first.pane(&pane)["pid"], before["pid"]);
    assert_eq!(
        first.pane(&pane)["agent"]["sessionId"],
        before["agent"]["sessionId"]
    );
    let id = requests[0]["request"]["requestId"].clone();
    let result = first.call(
        "pane.permission.answer",
        json!({"paneId":pane,"requestId":id,"optionId":"once","idempotencyKey":"first"}),
    );
    let late = second.call(
        "pane.permission.answer",
        json!({"paneId":pane,"requestId":id,"optionId":"deny","idempotencyKey":"second"}),
    );
    assert_eq!(result, late);
    first.wait_state(&pane, "idle");
    let answered = rig
        .database_probe()
        .event_count_by_field(
            "pane.permission_answered",
            "requestId",
            id.as_str().unwrap(),
        )
        .unwrap();
    assert_eq!(answered, 1);
}

#[test]
fn automatic_permission_is_never_presented_and_expiry_survives_restart() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    client.call(
        "pane.yolo",
        json!({"paneId":pane,"on":true,"idempotencyKey":"on"}),
    );
    let prompt = client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission","idempotencyKey":"auto"}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            client.section("requests").is_empty(),
            "automatic permission became visible"
        );
        let done = rig
            .database_probe()
            .event_count_by_field(
                "pane.turn_ended",
                "promptId",
                prompt["promptId"].as_str().unwrap(),
            )
            .unwrap();
        if done == 1 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "automatic permission did not finish"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let requested = rig
        .database_probe()
        .event_count("pane.permission_requested")
        .unwrap();
    assert_eq!(
        requested, 0,
        "automatic permission was published to connected clients"
    );
    let by_rule = rig
        .database_probe()
        .event_count_by_field("pane.permission_answered", "by", "rule")
        .unwrap();
    assert_eq!(by_rule, 1);
    client.call(
        "pane.yolo",
        json!({"paneId":pane,"on":false,"idempotencyKey":"off"}),
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission","idempotencyKey":"expires"}),
    );
    client.wait_state(&pane, "waiting_for_you");
    let question = client.section("requests")[0]["request"].clone();
    rig.kill_engine();
    // Preserve the request and process; advance only this test deadline.
    rig.database_probe()
        .expire_request(question["requestId"].as_str().unwrap())
        .unwrap();
    rig.start();
    let mut client = rig.client();
    client.wait_state(&pane, "idle");
    assert!(client.section("requests").is_empty());
    let attention = client.section("attention");
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0]["attentionId"], question["requestId"]);
    assert!(
        attention[0]["message"]
            .as_str()
            .unwrap()
            .contains("expired")
    );
    assert!(attention[0]["requestId"].is_null());
    let late = client.call("pane.permission.answer", json!({"paneId":pane,"requestId":question["requestId"],"optionId":"once","idempotencyKey":"late"}));
    assert_eq!(late["result"], "expired");
    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    assert_eq!(client.section("attention"), attention);
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"hello","idempotencyKey":"resume"}),
    );
    assert!(client.section("attention").is_empty());
}

#[test]
fn silence_keeps_the_turn_active_but_unconfirmed_cancel_stops_the_process() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let initial = client.pane(&pane);
    let pid = initial["pid"].clone();
    let workspace = initial["workspaceId"].as_str().unwrap().to_owned();
    let created = client.call(
        "task.create",
        json!({"workspaceId":workspace,"title":"Keep the owner during silence",
            "mustNotChange":"Keep recovery atomic","idempotencyKey":"silence-task"}),
    );
    let task = reserve_task(
        &rig,
        &workspace,
        &pane,
        created["task"]["taskId"].as_str().unwrap(),
    );
    let prompt = client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"silent","idempotencyKey":"silent"}),
    );
    client.wait_state(&pane, "working");
    // Wait for the fake's initial thought so no buffered traffic resets the clock.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !client.call("pane.transcript", json!({"paneId":pane}))["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["kind"] == "thought")
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    rig.kill_engine();
    rig.age_agent(&pane, false);
    rig.start();
    let mut client = rig.client();
    let silent = client.wait_state(&pane, "no_signal");
    assert_eq!(silent["pid"], pid);
    assert_eq!(silent["agent"]["activePromptId"], prompt["promptId"]);
    assert_eq!(client.section("attention").len(), 1);
    let store = Store::open(&rig.path).unwrap();
    let still_owned = store.task(&workspace, &task.task_id).unwrap().unwrap();
    assert_eq!(still_owned.state, soyeht_protocol::TaskState::InProgress);
    assert_eq!(still_owned.owner_pane_id.as_deref(), Some(pane.as_str()));
    client.call(
        "pane.cancel",
        json!({"paneId":pane,"idempotencyKey":"cancel-silent"}),
    );
    client.wait_state(&pane, "idle");
    assert_eq!(client.pane(&pane)["pid"], pid);
    assert!(client.section("attention").is_empty());
    client.call(
        "pane.queue.resume",
        json!({"paneId":pane,"idempotencyKey":"resume-after-cancel"}),
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"ignore-cancel","idempotencyKey":"unresponsive"}),
    );
    client.wait_state(&pane, "working");
    client.call(
        "pane.cancel",
        json!({"paneId":pane,"idempotencyKey":"cancel-unresponsive"}),
    );
    client.wait_state(&pane, "cancelling");
    rig.kill_engine();
    rig.age_agent(&pane, true);
    rig.start();
    let mut client = rig.client();
    let dead = client.wait_state(&pane, "dead");
    assert!(dead["agent"]["activePromptId"].is_null());
    assert!(dead["phrase"].as_str().unwrap().contains("10 seconds"));
    assert_eq!(client.section("attention").len(), 1);
    let store = Store::open(&rig.path).unwrap();
    let returned = store.task(&workspace, &task.task_id).unwrap().unwrap();
    assert_eq!(returned.state, soyeht_protocol::TaskState::Todo);
    assert_eq!(returned.owner_pane_id, None);
    assert_eq!(
        store.task_attempts(&workspace, &task.task_id).unwrap()[0].result,
        Some(soyeht_protocol::AttemptResult::Returned)
    );
    rig.kill_engine();
    rig.start();
    assert_eq!(
        rig.client().pane(&pane)["state"],
        "dead",
        "faulted process was resurrected"
    );
}

#[test]
fn terminal_sign_in_uses_a_separate_pane_then_retries_the_agent() {
    let rig = Rig::new();
    let binary =
        PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine")).with_file_name("soyeht-fake-acp");
    fs::write(rig.path.join("harnesses.json"),serde_json::to_vec(&json!([{"id":"auth-test","name":"Auth test","command":binary,"args":["--state-dir",rig.path.join("native"),"--require-login"],"version":"test"}])).unwrap()).unwrap();
    let mut client = rig.client();
    let registry = client.call("harness.list", json!({}));
    assert_eq!(registry["items"][0]["id"], "auth-test");
    assert_eq!(registry["items"][0]["available"], true);
    assert_eq!(
        client.call("model.list", json!({"harness":"auth-test"}))["source"],
        "not_probed"
    );
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Auth test","path":rig.path,"idempotencyKey":"auth-workspace"}),
    )["workspace"]["workspaceId"]
        .clone();
    let pane = client.call("pane.create",json!({"workspaceId":workspace,"mode":"agent","harness":"auth-test","idempotencyKey":"auth-pane"}))["paneId"].as_str().unwrap().to_owned();
    let deadline = Instant::now() + Duration::from_secs(10);
    while client.pane(&pane)["phrase"] != "Sign-in required" {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    let reconnect = rig.client().pane(&pane);
    assert_eq!(reconnect["agent"]["authMethods"][0]["id"], "login");
    assert_eq!(reconnect["agent"]["authMethods"][0]["terminal"], true);
    let params = json!({"paneId":pane,"authMethodId":"login","idempotencyKey":"auth-start"});
    let start = client.call("pane.auth.start", params.clone());
    assert_eq!(start, client.call("pane.auth.start", params));
    let terminal = start["terminalPaneId"].as_str().unwrap();
    assert_ne!(terminal, pane);
    assert_eq!(client.pane(terminal)["mode"], "terminal");
    assert_eq!(client.pane(terminal)["record"], false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !client.pane(&pane)["agent"]["sessionId"].is_string() {
        assert!(
            Instant::now() < deadline,
            "sign-in did not unlock session creation"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(rig.path.join("native/login.ok").is_file());
    let models = client.call("model.list", json!({"harness":"auth-test","paneId":pane}));
    assert_eq!(models["source"], "native_session");
    assert_eq!(models["items"].as_array().unwrap().len(), 4);
    let _: soyeht_protocol::ModelListResult = serde_json::from_value(models).unwrap();
}

fn start_agent_case(rig: &Rig) -> (Client, String, Value) {
    let binary =
        PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine")).with_file_name("soyeht-fake-acp");
    fs::write(rig.path.join("harnesses.json"), serde_json::to_vec(&json!([{
        "id":"test-agent","name":"Test agent","command":binary,"args":["--state-dir",rig.path.join("native")],"version":"test"
    }])).unwrap()).unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Agent test","path":rig.path,"idempotencyKey":Uuid::new_v4().to_string()}),
    )["workspace"]["workspaceId"]
        .clone();
    let pane = client.call("pane.create", json!({"workspaceId":workspace,"mode":"agent","harness":"test-agent","idempotencyKey":Uuid::new_v4().to_string()}))["paneId"].as_str().unwrap().to_owned();
    let deadline = Instant::now() + Duration::from_secs(10);
    let ready = loop {
        let view = client.pane(&pane);
        if view["agent"]["sessionId"].is_string() {
            break view;
        }
        assert!(
            Instant::now() < deadline,
            "agent did not initialize: {view}"
        );
        thread::sleep(Duration::from_millis(20));
    };
    (client, pane, ready)
}

#[test]
fn production_sessions_receive_one_bound_mcp_server_and_probes_receive_none() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let ready = client.pane(&pane);
    let session_id = ready["agent"]["sessionId"].as_str().unwrap();
    let session = rig.native_session(session_id);
    let server = &session["mcp_servers"][0];
    let binding = server["env"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "SOYEHT_MCP_BINDING")
        .unwrap()["value"]
        .as_str()
        .unwrap()
        .to_owned();
    let args = server["args"].as_array().unwrap();
    assert_eq!(
        server["command"],
        PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine"))
            .with_file_name("soyeht-mcp")
            .to_string_lossy()
            .as_ref()
    );
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--data-dir", rig.path.to_str().unwrap()])
    );
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--pane-id", pane.as_str()])
    );
    assert!(!args.iter().any(|value| value == &binding));
    let (_, snapshot) = client.snapshot();
    assert!(!serde_json::to_string(&snapshot).unwrap().contains(&binding));

    let probe_sessions: Vec<Value> = fs::read_dir(rig.path.join("native"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|value| value == "json")
        })
        .filter(|entry| entry.file_name() != format!("{session_id}.json").as_str())
        .map(|entry| serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap())
        .collect();
    assert!(!probe_sessions.is_empty());
    assert!(
        probe_sessions
            .iter()
            .all(|session| session["mcp_servers"].as_array().unwrap().is_empty())
    );

    let (mut bound_socket, welcome) = binding_hello(&rig.path.join("engine.sock"), &binding);
    assert_eq!(welcome["result"]["role"], "agent");
    Frame::Json(
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":2,"method":"engine.status","params":{}}))
            .unwrap(),
    )
    .write_to(&mut bound_socket)
    .unwrap();
    let Some(Frame::Json(response)) = Frame::read_from(&mut bound_socket).unwrap() else {
        panic!("bound method response")
    };
    assert_eq!(
        serde_json::from_slice::<Value>(&response).unwrap()["error"]["data"]["kind"],
        "forbidden_role"
    );
    let (_, unknown) = binding_hello(&rig.path.join("engine.sock"), "unknown-binding");
    assert_eq!(unknown["error"]["data"]["kind"], "unauthorized");

    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"hello","idempotencyKey":"mcp-restart-turn"}),
    );
    let before = client.wait_state(&pane, "idle");
    rig.ptyd.kill().unwrap();
    rig.ptyd.wait().unwrap();
    rig.ptyd = Command::new(
        PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine")).with_file_name("soyeht-ptyd"),
    )
    .arg("--data-dir")
    .arg(&rig.path)
    .stdout(Stdio::null())
    .spawn()
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let restored = loop {
        let view = client.pane(&pane);
        if view["generation"].as_u64().unwrap() > before["generation"].as_u64().unwrap()
            && view["state"] == "idle"
        {
            break view;
        }
        assert!(Instant::now() < deadline, "agent did not reload: {view}");
        thread::sleep(Duration::from_millis(20));
    };
    let reloaded = rig.native_session(session_id);
    let reloaded_server = &reloaded["mcp_servers"][0];
    assert_eq!(reloaded_server["env"], server["env"]);
    let restored_generation = restored["generation"].as_u64().unwrap().to_string();
    assert!(
        reloaded_server["args"]
            .as_array()
            .unwrap()
            .windows(2)
            .any(|pair| pair == ["--session-generation", restored_generation.as_str()])
    );
}

#[test]
fn a_legacy_agent_checkpoint_gets_one_binding_that_survives_the_next_restart() {
    let mut rig = Rig::new();
    let (_, pane) = rig.agent();
    let original = rig.database_probe().agent_binding(&pane).unwrap();
    assert!(!original.is_empty());
    rig.kill_engine();
    rig.database_probe().remove_agent_binding(&pane).unwrap();
    rig.start();
    let repaired = rig.database_probe().agent_binding(&pane).unwrap();
    assert!(!repaired.is_empty());
    assert_ne!(repaired, original);
    rig.kill_engine();
    rig.start();
    assert_eq!(rig.database_probe().agent_binding(&pane).unwrap(), repaired);
}

fn configure_and_run_first_turn(
    client: &mut Client,
    pane: &str,
    native_pid: &Value,
    native_session: &Value,
) {
    let configured = client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"model","value":"model-b","idempotencyKey":"model-change"}),
    );
    let _: soyeht_protocol::PaneConfigSetResult =
        serde_json::from_value(configured.clone()).unwrap();
    assert_eq!(configured["effective"], "model-b");
    let invalid = json!({"paneId":pane,"configId":"model","value":"rejected-model","idempotencyKey":"model-rejected"});
    let refused = client.request("pane.config.set", invalid.clone());
    assert_eq!(refused["error"]["data"]["kind"], "invalid_params");
    assert_eq!(
        client.request("pane.config.set", invalid)["error"],
        refused["error"]
    );
    assert_eq!(client.pane(pane)["pid"], *native_pid);
    assert_eq!(client.pane(pane)["agent"]["sessionId"], *native_session);
    assert_eq!(
        client.pane(pane)["agent"]["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|option| option["id"] == "model")
            .unwrap()["currentValue"],
        "model-b"
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"hello","idempotencyKey":"first-prompt"}),
    );
    client.wait_state(pane, "idle");
    let transcript = client.call("pane.transcript", json!({"paneId":pane}));
    assert!(
        transcript["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "assistant"
                && message["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("hello")))
    );
}

fn request_permission(client: &mut Client, pane: &str) -> Value {
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission-null","idempotencyKey":"permission-prompt"}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (_, parts) = client.snapshot();
        if let Some(question) = parts
            .iter()
            .filter(|part| part["section"] == "requests")
            .flat_map(|part| part["data"].as_array().unwrap())
            .find(|request| request["kind"] == "permission")
        {
            return question["request"].clone();
        }
        assert!(Instant::now() < deadline, "no permission question");
        thread::sleep(Duration::from_millis(20));
    }
}

fn answer_after_engine_adoption(
    rig: &mut Rig,
    pane: &str,
    native_pid: &Value,
    native_session: &Value,
    question: &Value,
) -> Client {
    rig.kill_engine();
    rig.start();
    let mut first = rig.client();
    let mut second = rig.client();
    let restored = first.pane(pane);
    assert_eq!(restored["pid"], *native_pid);
    assert_eq!(restored["agent"]["sessionId"], *native_session);
    let selected = first.call("pane.permission.answer", json!({"paneId":pane,"requestId":question["requestId"],"optionId":"once","idempotencyKey":"answer-once"}));
    let repeated = second.call("pane.permission.answer", json!({"paneId":pane,"requestId":question["requestId"],"optionId":"deny","idempotencyKey":"late-answer"}));
    assert_eq!(selected, repeated);
    assert_eq!(selected["optionId"], "once");
    first.wait_state(pane, "idle");
    let transcript = first.call("pane.transcript", json!({"paneId":pane}));
    let decisions: Vec<_> = transcript["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| {
            message["text"]
                .as_str()
                .is_some_and(|text| text.starts_with("Permission result:"))
        })
        .collect();
    assert_eq!(decisions.len(), 1);
    assert!(decisions[0]["text"].as_str().unwrap().contains("once"));
    first
}

fn restart_supervisor_and_assert_load(
    rig: &mut Rig,
    client: &mut Client,
    pane: &str,
    ready_generation: u64,
    native_pid: &Value,
    native_session: &Value,
) {
    rig.ptyd.kill().unwrap();
    rig.ptyd.wait().unwrap();
    rig.ptyd = Command::new(
        PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine")).with_file_name("soyeht-ptyd"),
    )
    .arg("--data-dir")
    .arg(&rig.path)
    .stdout(Stdio::null())
    .spawn()
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let view = client.pane(pane);
        if view["generation"].as_u64().unwrap() > ready_generation && view["state"] == "idle" {
            assert_ne!(view["pid"], *native_pid);
            assert_eq!(view["agent"]["sessionId"], *native_session);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native load did not complete: {view}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let transcript = client.call("pane.transcript", json!({"paneId":pane}));
    let items = transcript["items"].as_array().unwrap();
    assert_eq!(
        items
            .iter()
            .filter(|message| message["role"] == "user")
            .count(),
        2
    );
    assert_eq!(
        items
            .iter()
            .filter(|message| message["role"] == "assistant")
            .count(),
        2
    );
    assert!(
        items
            .iter()
            .any(|message| { message["text"] == "hello" && message["role"] == "user" })
    );
    assert_eq!(
        items
            .iter()
            .filter(|message| message["text"]
                .as_str()
                .is_some_and(|text| text.starts_with("Permission result:")))
            .count(),
        1
    );
}

fn exercise_yolo_and_form(client: &mut Client, pane: &str) {
    let toggled = client.call(
        "pane.yolo",
        json!({"paneId":pane,"on":true,"idempotencyKey":"yolo-on"}),
    );
    assert_eq!(toggled["effectiveMode"], "allow_all");
    assert_eq!(toggled["on"], true);
    client.call("pane.config.set", json!({"paneId":pane,"configId":"mode","value":"accept_edits","idempotencyKey":"manual-mode"}));
    let manual = client.pane(pane);
    assert_eq!(manual["agent"]["permission"], "accept_edits");
    assert_eq!(manual["agent"]["yolo"], false);
    assert!(manual["agent"]["previousPermission"].is_null());
    client.call(
        "pane.yolo",
        json!({"paneId":pane,"on":true,"idempotencyKey":"yolo-after-manual"}),
    );
    let restored = client.call(
        "pane.yolo",
        json!({"paneId":pane,"on":false,"idempotencyKey":"restore-manual"}),
    );
    assert_eq!(restored["effectiveMode"], "accept_edits");
    client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"ask","idempotencyKey":"restore-ask"}),
    );
    client.call(
        "pane.yolo",
        json!({"paneId":pane,"on":true,"idempotencyKey":"yolo-before-form"}),
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"elicitation","idempotencyKey":"form-prompt"}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let form = loop {
        let (_, parts) = client.snapshot();
        if let Some(form) = parts
            .iter()
            .filter(|part| part["section"] == "requests")
            .flat_map(|part| part["data"].as_array().unwrap())
            .find(|request| request["kind"] == "elicitation")
        {
            break form["request"].clone();
        }
        assert!(
            Instant::now() < deadline,
            "form was not presented while yolo was enabled"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(client.pane(pane)["state"], "waiting_for_you");
    let invalid = client.request("pane.elicitation.answer", json!({"paneId":pane,"requestId":form["requestId"],"fields":{"answer":42},"idempotencyKey":"invalid-form"}));
    assert_eq!(invalid["error"]["data"]["kind"], "invalid_params");
    let answered = client.call("pane.elicitation.answer", json!({"paneId":pane,"requestId":form["requestId"],"fields":{"answer":"yes"},"idempotencyKey":"valid-form"}));
    assert_eq!(answered["result"], "submitted");
    while client.pane(pane)["state"] != "idle" {
        assert!(Instant::now() < deadline, "form turn did not end");
        thread::sleep(Duration::from_millis(20));
    }
    let off = client.call(
        "pane.yolo",
        json!({"paneId":pane,"on":false,"idempotencyKey":"yolo-off"}),
    );
    assert_eq!(off["effectiveMode"], "ask");
    assert_eq!(off["on"], false);
}

fn exercise_cancel_and_tools(rig: &Rig, client: &mut Client, pane: &str) {
    let pid = client.pane(pane)["pid"].clone();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"slow","idempotencyKey":"slow-prompt"}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while client.pane(pane)["state"] != "working" {
        assert!(Instant::now() < deadline, "slow turn did not start");
        thread::sleep(Duration::from_millis(20));
    }
    client.call(
        "pane.cancel",
        json!({"paneId":pane,"idempotencyKey":"cancel-slow"}),
    );
    while client.pane(pane)["state"] != "idle" {
        assert!(
            Instant::now() < deadline,
            "healthy cancellation exceeded deadline"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(client.pane(pane)["pid"], pid);
    client.call(
        "pane.queue.resume",
        json!({"paneId":pane,"idempotencyKey":"resume-tools"}),
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"tools","idempotencyKey":"native-tools"}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let transcript = client.call("pane.transcript", json!({"paneId":pane}));
        let has_terminal = transcript["items"].as_array().unwrap().iter().any(|item| {
            item["kind"] == "terminal"
                && item["terminal"]["bytes"].as_str().is_some_and(|bytes| {
                    STANDARD
                        .decode(bytes)
                        .is_ok_and(|bytes| bytes == b"fake-terminal")
                })
        });
        if client.pane(pane)["state"] == "idle" && has_terminal {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native filesystem/terminal operations did not complete: {transcript}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        fs::read_to_string(rig.path.join("fake-acp-note.txt")).unwrap(),
        "test data\n"
    );
}

#[test]
fn agent_turns_and_permission_survive_engine_adoption() {
    let mut rig = Rig::new();
    let (mut client, pane, ready) = start_agent_case(&rig);
    let native_session = ready["agent"]["sessionId"].clone();
    let native_pid = ready["pid"].clone();
    configure_and_run_first_turn(&mut client, &pane, &native_pid, &native_session);
    let question = request_permission(&mut client, &pane);
    let mut first =
        answer_after_engine_adoption(&mut rig, &pane, &native_pid, &native_session, &question);
    restart_supervisor_and_assert_load(
        &mut rig,
        &mut first,
        &pane,
        ready["generation"].as_u64().unwrap(),
        &native_pid,
        &native_session,
    );
    exercise_yolo_and_form(&mut first, &pane);
    exercise_cancel_and_tools(&rig, &mut first, &pane);
}

impl Client {
    fn wait_state(&mut self, pane: &str, state: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let view = self.pane(pane);
            if view["state"] == state {
                return view;
            }
            assert!(Instant::now() < deadline, "expected {state}: {view}");
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn section(&mut self, section: &str) -> Vec<Value> {
        self.snapshot()
            .1
            .into_iter()
            .filter(|p| p["section"] == section)
            .flat_map(|p| p["data"].as_array().unwrap().clone())
            .collect()
    }
    fn new(path: PathBuf) -> Self {
        let socket = UnixStream::connect(path).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut client = Self {
            socket,
            next: 0,
            messages: vec![],
        };
        let Some(Frame::Json(bytes)) = Frame::read_from(&mut client.socket).unwrap() else {
            panic!("challenge")
        };
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["method"],
            "auth.challenge"
        );
        let welcome = client.call(
            "hello",
            json!({"client":"runtime-test","clientVersion":"1","protocolVersion":1}),
        );
        assert_eq!(welcome["role"], "human_local");
        client
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        Frame::Json(
            serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
                .unwrap(),
        )
        .write_to(&mut self.socket)
        .unwrap();
        loop {
            let frame = Frame::read_from(&mut self.socket)
                .unwrap()
                .expect("server frame");
            if let Frame::Json(bytes) = &frame {
                let value: Value = serde_json::from_slice(bytes).unwrap();
                if value["id"] == id {
                    return value;
                }
            }
            self.messages.push(frame);
        }
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        let response = self.request(method, params);
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
    }
    fn snapshot(&mut self) -> (Value, Vec<Value>) {
        self.messages.clear();
        let result = self.call("state.snapshot", json!({}));
        let parts = self
            .messages
            .iter()
            .filter_map(|f| {
                if let Frame::Json(b) = f {
                    let v: Value = serde_json::from_slice(b).unwrap();
                    (v["method"] == "state.snapshot_part").then_some(v["params"].clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(parts.len(), result["parts"].as_u64().unwrap() as usize);
        (result, parts)
    }
    fn pane(&mut self, id: &str) -> Value {
        self.snapshot()
            .1
            .into_iter()
            .filter(|p| p["section"] == "panes")
            .flat_map(|p| p["data"].as_array().unwrap().clone())
            .find(|p| p["paneId"] == id)
            .expect("pane in snapshot")
    }
    fn create(&mut self, rig: &Rig) -> (String, String) {
        let workspace = self.call(
            "workspace.create",
            json!({"name":"Test","path":rig.path,"idempotencyKey":Uuid::new_v4().to_string()}),
        )["workspace"]["workspaceId"]
            .as_str()
            .unwrap()
            .to_owned();
        let pane = self.call(
            "pane.create",
            json!({"workspaceId":workspace,"mode":"terminal","idempotencyKey":"create-pane"}),
        )["paneId"]
            .as_str()
            .unwrap()
            .to_owned();
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.pane(&pane)["pid"].is_null() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        (workspace, pane)
    }
    fn tail(&mut self, pane: &str) -> (Value, Vec<Frame>) {
        self.messages.clear();
        let info = self.call(
            "pane.stream.subscribe",
            json!({"paneId":pane,"from":"tail"}),
        );
        let mut frames = std::mem::take(&mut self.messages);
        loop {
            let frame = Frame::read_from(&mut self.socket).unwrap().expect("replay");
            let done = if let Frame::Json(b) = &frame {
                serde_json::from_slice::<Value>(b).unwrap()["method"] == "stream.replay_done"
            } else {
                false
            };
            frames.push(frame);
            if done {
                break;
            }
        }
        frames.retain(|frame| match frame {
            Frame::Terminal { pane_ref, .. } => {
                Some(*pane_ref as u64) == info["binaryPaneRef"].as_u64()
            }
            _ => true,
        });
        (info, frames)
    }
}

#[test]
fn creation_retries_and_close_survive_engine_restart_without_resurrection() {
    let mut rig = Rig::new();
    let mut client = rig.client();
    let (workspace, pane) = client.create(&rig);
    let duplicate = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"terminal","idempotencyKey":"create-pane"}),
    );
    assert_eq!(duplicate["paneId"], pane);
    let pid = client.pane(&pane)["pid"].clone();
    drop(client);
    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    assert_eq!(client.pane(&pane)["pid"], pid);
    let close = json!({"paneId":pane,"idempotencyKey":"close-pane"});
    client.call("pane.close", close.clone());
    assert_eq!(client.call("pane.close", close.clone())["paneId"], pane);
    drop(client);
    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    assert_eq!(client.call("pane.close", close)["paneId"], pane);
    assert!(
        client
            .snapshot()
            .1
            .iter()
            .filter(|p| p["section"] == "panes")
            .all(|p| p["data"].as_array().unwrap().is_empty())
    );
    let changed=client.request("pane.create",json!({"workspaceId":workspace,"mode":"terminal","name":"Different","idempotencyKey":"create-pane"}));
    assert_eq!(changed["error"]["data"]["kind"], "idempotency_conflict");
}

#[test]
fn snapshot_cursor_replays_changes_exactly_once_before_live_events() {
    let rig = Rig::new();
    let mut observer = rig.client();
    let mut writer = rig.client();
    let (workspace, _) = writer.create(&rig);
    let (snapshot, _) = observer.snapshot();
    for n in 0..20 {
        writer.call("workspace.rename",json!({"workspaceId":workspace,"name":format!("Name {n}"),"idempotencyKey":Uuid::new_v4().to_string()}));
    }
    observer.messages.clear();
    observer.call("events.subscribe", json!({"since":snapshot["cursor"]}));
    let events: Vec<_> = observer
        .messages
        .iter()
        .filter_map(|f| {
            if let Frame::Json(bytes) = f {
                let v: Value = serde_json::from_slice(bytes).unwrap();
                (v["method"] == "workspace.renamed").then_some(v)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(events.len(), 20);
    let mut prior = snapshot["cursor"].as_str().unwrap().parse::<u64>().unwrap();
    for (n, event) in events.iter().enumerate() {
        let id = event["params"]["eventId"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!(id > prior);
        prior = id;
        assert_eq!(event["params"]["workspace"]["name"], format!("Name {n}"));
    }
    writer.call(
        "workspace.rename",
        json!({"workspaceId":workspace,"name":"Live","idempotencyKey":Uuid::new_v4().to_string()}),
    );
    let Some(Frame::Json(bytes)) = Frame::read_from(&mut observer.socket).unwrap() else {
        panic!("live event")
    };
    let event: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(event["params"]["workspace"]["name"], "Live");
    assert!(
        event["params"]["eventId"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > prior
    );
}

#[test]
fn backpressured_subscriber_survives_an_answer_from_a_second_subscriber() {
    let rig = Rig::new();
    let (mut answerer, pane) = rig.agent();
    let workspace = answerer.pane(&pane)["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut observer = rig.client();
    let (observer_snapshot, _) = observer.snapshot();
    observer.call(
        "events.subscribe",
        json!({"since":observer_snapshot["cursor"]}),
    );
    let (answerer_snapshot, _) = answerer.snapshot();
    answerer.call(
        "events.subscribe",
        json!({"since":answerer_snapshot["cursor"]}),
    );

    answerer.call(
        "workspace.rename",
        json!({
            "workspaceId":workspace,
            "name":"x".repeat(256 * 1024),
            "idempotencyKey":"backpressure-event",
        }),
    );
    answerer.call(
        "pane.prompt",
        json!({
            "paneId":pane,
            "text":"permission",
            "idempotencyKey":"backpressure-prompt",
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let request = loop {
        if let Some(entry) = answerer
            .section("requests")
            .into_iter()
            .find(|entry| entry["kind"] == "permission")
        {
            break entry["request"]["requestId"].clone();
        }
        assert!(Instant::now() < deadline, "permission request did not open");
        thread::sleep(Duration::from_millis(20));
    };
    answerer.call(
        "pane.permission.answer",
        json!({
            "paneId":pane,
            "requestId":request,
            "optionId":"once",
            "idempotencyKey":"backpressure-answer",
        }),
    );
    answerer.wait_state(&pane, "idle");

    // This exceeds the former per-write deadline while remaining far below
    // the contractual 8 MiB connection budget.
    thread::sleep(Duration::from_secs(6));
    observer.call("engine.status", json!({}));
    assert!(observer.messages.iter().any(|frame| {
        let Frame::Json(bytes) = frame else {
            return false;
        };
        serde_json::from_slice::<Value>(bytes)
            .is_ok_and(|value| value["method"] == "pane.permission_answered")
    }));
}

#[test]
fn reconnect_replays_screen_and_duplicate_input_is_not_typed_twice() {
    let rig = Rig::new();
    let mut client = rig.client();
    let (_, pane) = client.create(&rig);
    let input = json!({"paneId":pane,"bytes":STANDARD.encode(b"printf x >> once; printf 'UNIQUE_OUTPUT_42\\n'\n"),"idempotencyKey":"input-1"});
    let receipt = client.call("pane.input", input.clone());
    assert_eq!(client.call("pane.input", input), receipt);
    let deadline = Instant::now() + Duration::from_secs(10);
    let (info, frames) = loop {
        let replay = client.tail(&pane);
        let bytes: Vec<_> = replay
            .1
            .iter()
            .filter_map(|f| {
                if let Frame::Terminal { bytes, .. } = f {
                    Some(bytes.as_slice())
                } else {
                    None
                }
            })
            .flatten()
            .copied()
            .collect();
        if bytes
            .windows(16)
            .filter(|s| *s == b"UNIQUE_OUTPUT_42")
            .count()
            >= 2
            // A shell redraw can echo the marker twice before executing it.
            // Wait for the actual effect as well as its terminal output.
            && fs::read(rig.path.join("once")).ok().as_deref() == Some(b"x")
        {
            break replay;
        }
        assert!(
            Instant::now() < deadline,
            "output not recorded: {}",
            String::from_utf8_lossy(&bytes)
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(fs::read(rig.path.join("once")).unwrap(), b"x");
    let mut expected = 0;
    for frame in &frames {
        if let Frame::Json(bytes) = frame {
            let v: Value = serde_json::from_slice(bytes).unwrap();
            if v["method"] == "stream.replay_start" {
                expected = v["params"]["fromOffset"].as_str().unwrap().parse().unwrap();
            }
        }
        if let Frame::Terminal { offset, bytes, .. } = frame {
            assert_eq!(*offset, expected);
            expected += bytes.len() as u64;
        }
    }
    assert_eq!(
        expected,
        info["nextOffset"].as_str().unwrap().parse::<u64>().unwrap()
    );
    let pid = client.pane(&pane)["pid"].clone();
    drop(client);
    let mut client = rig.client();
    assert_eq!(client.pane(&pane)["pid"], pid);
    let (_, replayed) = client.tail(&pane);
    assert!(!replayed.is_empty());
}

struct Phone {
    socket: tokio_tungstenite::tungstenite::WebSocket<std::net::TcpStream>,
    challenge: Value,
    next: u64,
}
impl Phone {
    fn connect(address: &str) -> Self {
        let stream = std::net::TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let (socket, _) =
            tokio_tungstenite::tungstenite::client(format!("ws://{address}/"), stream).unwrap();
        let mut phone = Self {
            socket,
            challenge: Value::Null,
            next: 0,
        };
        let message = phone.read();
        assert_eq!(message["method"], "auth.challenge");
        phone.challenge = message["params"].clone();
        phone
    }
    fn read(&mut self) -> Value {
        loop {
            match self.socket.read().unwrap() {
                tokio_tungstenite::tungstenite::Message::Text(text) => {
                    return serde_json::from_str(&text).unwrap();
                }
                tokio_tungstenite::tungstenite::Message::Ping(_) => {
                    self.socket.flush().unwrap();
                }
                other => panic!("unexpected phone message {other:?}"),
            }
        }
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        loop {
            let response = self.read();
            if response["id"] == id {
                return response;
            }
        }
    }
    fn hello(&mut self, key: &ed25519_dalek::SigningKey, token: &str) -> Value {
        use ed25519_dalek::Signer;
        let nonce: [u8; 32] = STANDARD
            .decode(self.challenge["nonce"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let proof = STANDARD.encode(
            key.sign(&soyeht_engine::auth::hello_bytes(
                &nonce,
                self.challenge["engineId"].as_str().unwrap(),
                token,
            ))
            .to_bytes(),
        );
        self.request("hello",json!({"client":"phone-test","clientVersion":"1","protocolVersion":1,"deviceToken":token,"proof":proof}))
    }
}

#[test]
fn phone_pairing_requires_consent_signature_and_revocation_closes_access() {
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};
    let rig = Rig::new();
    let mut local = rig.client();
    let (_, pane) = local.create(&rig);
    let window = local.call("device.pair.start", json!({"idempotencyKey":"pair-window"}));
    let address = window["uri"]
        .as_str()
        .unwrap()
        .split("address=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap();
    let mut phone = Phone::connect(address);
    assert_eq!(
        phone.request("workspace.list", json!({}))["error"]["data"]["kind"],
        "unauthorized"
    );
    let key = ed25519_dalek::SigningKey::from_bytes(&[17; 32]);
    let begin=phone.request("pair.begin",json!({"code":window["code"],"deviceName":"Test phone","devicePublicKey":STANDARD.encode(key.verifying_key().to_bytes()),"idempotencyKey":"begin"}));
    assert!(begin.get("error").is_none(), "{begin}");
    let begin = &begin["result"];
    let derived = Sha256::digest(
        [
            window["code"].as_str().unwrap().as_bytes(),
            key.verifying_key().as_bytes(),
        ]
        .concat(),
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(&derived).unwrap();
    mac.update(
        &STANDARD
            .decode(begin["challenge"].as_str().unwrap())
            .unwrap(),
    );
    mac.update(b"Test phone");
    let prove=phone.request("pair.prove",json!({"requestId":begin["requestId"],"proof":STANDARD.encode(mac.finalize().into_bytes()),"idempotencyKey":"prove"}));
    assert_eq!(prove["result"]["status"], "pending");
    assert!(
        local.call("device.list", json!({}))["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    local.call(
        "device.pair.decide",
        json!({"requestId":begin["requestId"],"approve":true,"idempotencyKey":"approve"}),
    );
    let paired = phone.read();
    assert_eq!(paired["method"], "pair.result");
    assert_eq!(paired["params"]["approved"], true);
    let token = paired["params"]["deviceToken"].as_str().unwrap();
    let device = paired["params"]["deviceId"].as_str().unwrap();
    let mut reuse = Phone::connect(address);
    assert_eq!(
        reuse.request(
            "pair.begin",
            json!({
                "code": window["code"], "deviceName": "Another attempt",
                "devicePublicKey": STANDARD.encode(key.verifying_key().to_bytes()),
                "idempotencyKey": "used-code"
            })
        )["error"]["data"]["kind"],
        "unauthorized",
        "pairing code must be consumed by consent"
    );
    drop(reuse);
    let mut recover = Phone::connect(address);
    let recovered = recover.request("pair.begin",json!({"code":window["code"],"deviceName":"Test phone","devicePublicKey":STANDARD.encode(key.verifying_key().to_bytes()),"idempotencyKey":"begin"}));
    assert_eq!(
        recovered["result"], *begin,
        "original attempt remains recoverable after code consumption"
    );
    let mut recovery_mac = Hmac::<Sha256>::new_from_slice(&derived).unwrap();
    recovery_mac.update(
        &STANDARD
            .decode(begin["challenge"].as_str().unwrap())
            .unwrap(),
    );
    recovery_mac.update(b"Test phone");
    recover.socket.send(tokio_tungstenite::tungstenite::Message::Text(json!({
        "jsonrpc":"2.0","id":900,"method":"pair.prove","params":{
            "requestId":begin["requestId"],"proof":STANDARD.encode(recovery_mac.finalize().into_bytes()),"idempotencyKey":"prove"
        }
    }).to_string().into())).unwrap();
    let replayed = recover.read();
    assert_eq!(replayed["method"], "pair.result");
    assert_eq!(replayed["params"]["deviceToken"], token);
    assert_eq!(recover.read()["id"], 900);
    drop(recover);
    drop(phone);
    let mut phone = Phone::connect(address);
    assert_eq!(phone.hello(&key, token)["result"]["role"], "human_device");
    assert_eq!(phone.request("pane.input",json!({"paneId":pane,"bytes":STANDARD.encode(b"echo no\n"),"idempotencyKey":"forbidden"}))["error"]["data"]["kind"],"forbidden_role");
    assert_eq!(
        phone.request("workspace.list", json!({}))["result"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    local.call(
        "device.revoke",
        json!({"deviceId":device,"idempotencyKey":"revoke"}),
    );
    assert!(matches!(
        phone.socket.read(),
        Ok(tokio_tungstenite::tungstenite::Message::Close(_)) | Err(_)
    ));
    let mut phone = Phone::connect(address);
    assert_eq!(
        phone.hello(&key, token)["error"]["data"]["kind"],
        "unauthorized"
    );
}

fn create_task_fixture(rig: &Rig, client: &mut Client) -> (String, Value, Value) {
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Tasks","path":rig.path,"idempotencyKey":"task-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let invalid = client.request(
        "task.create",
        json!({"workspaceId":workspace,"title":"Invalid","mustNotChange":"","idempotencyKey":"invalid-task"}),
    );
    assert_eq!(invalid["error"]["data"]["kind"], "invalid_params");
    let first_params = json!({
        "workspaceId":workspace,"title":"First","description":"Build it",
        "mustNotChange":"Keep the boundary","idempotencyKey":"first-task"
    });
    let first = client.call("task.create", first_params.clone())["task"].clone();
    let repeated = client.call("task.create", first_params)["task"].clone();
    assert_eq!(repeated["taskId"], first["taskId"]);
    assert_eq!(first["taskId"], "WS-1");
    assert_eq!(first["createdBy"], "you");
    let second = client.call(
        "task.create",
        json!({
            "workspaceId":workspace,"title":"Second","mustNotChange":"Keep the first task",
            "priority":"urgent","dependencies":[first["taskId"]],"idempotencyKey":"second-task"
        }),
    )["task"]
        .clone();
    let page = client.call("task.list", json!({"workspaceId":workspace,"limit":1}));
    assert_eq!(page["items"][0]["taskId"], second["taskId"]);
    assert_eq!(page["nextCursor"], "1");
    let next = client.call(
        "task.list",
        json!({"workspaceId":workspace,"limit":1,"cursor":page["nextCursor"]}),
    );
    assert_eq!(next["items"][0]["taskId"], first["taskId"]);
    assert!(next["nextCursor"].is_null());
    (workspace, first, second)
}

#[test]
fn dispatch_opens_one_task_pane_and_delivers_the_complete_first_prompt() {
    let rig = Rig::new();
    rig.install_fake_harness();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Dispatch","path":rig.path,"idempotencyKey":"dispatch-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let task = client.call(
        "task.create",
        json!({
            "workspaceId":workspace,"title":"Build dispatch","description":"Wire the route",
            "howToVerify":"Run the task test","mustNotChange":"Keep prior methods",
            "idempotencyKey":"dispatch-task"
        }),
    )["task"]
        .clone();
    let params = json!({
        "workspaceId":workspace,"taskId":task["taskId"],
        "preset":{"workspaceDefault":false,"harness":"test-agent","permission":"ask"},
        "idempotencyKey":"dispatch-once"
    });
    let dispatched = client.call("task.dispatch", params.clone());
    assert_eq!(client.call("task.dispatch", params), dispatched);
    let pane = dispatched["paneId"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let shown = loop {
        let shown = client.call(
            "task.show",
            json!({"workspaceId":workspace,"taskId":task["taskId"]}),
        );
        if shown["task"]["worktree"]["path"].is_string()
            && client.pane(pane)["agent"]["sessionId"].is_string()
        {
            break shown;
        }
        assert!(
            Instant::now() < deadline,
            "dispatch did not finish: {shown}"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(shown["task"]["ownerPaneId"], pane);
    assert_eq!(shown["attempts"].as_array().unwrap().len(), 1);
    let transcript = loop {
        let transcript = client.call("pane.transcript", json!({"paneId":pane}));
        if transcript["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| {
                message["role"] == "user"
                    && message["text"]
                        .as_str()
                        .is_some_and(|text| text.contains("## Must not change\nKeep prior methods"))
            })
        {
            break transcript;
        }
        assert!(
            Instant::now() < deadline,
            "first prompt was not delivered: {transcript}"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let text = transcript["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "user")
        .unwrap()["text"]
        .as_str()
        .unwrap();
    for expected in [
        "Build dispatch",
        "Wire the route",
        "Keep prior methods",
        "Run the task test",
        ".test-worktrees/task-ws-1",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    let second = client.request(
        "task.dispatch",
        json!({
            "workspaceId":workspace,"taskId":task["taskId"],
            "preset":{"workspaceDefault":false,"harness":"test-agent"},
            "idempotencyKey":"dispatch-twice"
        }),
    );
    assert_eq!(second["error"]["data"]["kind"], "task_has_owner");
}

#[test]
fn task_next_restarts_the_same_pane_in_the_claimed_worktree() {
    let rig = Rig::new();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Pull","path":rig.path,"idempotencyKey":"pull-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (mut client, pane) = rig.open_agent(client, &json!(workspace));
    let before = client.pane(&pane);
    let task = client.call(
        "task.create",
        json!({
            "workspaceId":workspace,"title":"Pulled task","description":"Run in a new session",
            "mustNotChange":"Keep the pane","idempotencyKey":"pull-task"
        }),
    )["task"]
        .clone();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"task-claim","idempotencyKey":"pull-turn"}),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let shown = client.call(
            "task.show",
            json!({"workspaceId":workspace,"taskId":task["taskId"]}),
        );
        let current = client.pane(&pane);
        if shown["task"]["ownerPaneId"] == pane
            && shown["task"]["worktree"]["path"].is_string()
            && current["generation"].as_u64().unwrap() > before["generation"].as_u64().unwrap()
            && current["cwd"] == shown["task"]["worktree"]["path"]
            && current["agent"]["sessionId"].is_string()
            && current["agent"]["sessionId"] != before["agent"]["sessionId"]
        {
            assert_eq!(shown["attempts"].as_array().unwrap().len(), 1);
            assert_eq!(current["name"], "Pulled task");
            let transcript = client.call("pane.transcript", json!({"paneId":pane}));
            if transcript_contains(&transcript, "\"willRestartSession\":true")
                && transcript_contains(&transcript, "# Pulled task\n\nRun in a new session")
            {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "task.next did not restart the pane: task={shown} pane={current}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn two_idle_panes_claim_one_task_exactly_once() {
    let rig = Rig::new();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Race","path":rig.path,"idempotencyKey":"race-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (client, first_pane) = rig.open_agent(client, &json!(workspace));
    let (mut client, second_pane) = rig.open_agent(client, &json!(workspace));
    let task = client.call(
        "task.create",
        json!({
            "workspaceId":workspace,"title":"One owner","description":"Claim once",
            "mustNotChange":"Keep one attempt","idempotencyKey":"race-task"
        }),
    )["task"]
        .clone();
    for (pane, key) in [(&first_pane, "race-first"), (&second_pane, "race-second")] {
        client.call(
            "pane.prompt",
            json!({"paneId":pane,"text":"task-claim","idempotencyKey":key}),
        );
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let shown = client.call(
            "task.show",
            json!({"workspaceId":workspace,"taskId":task["taskId"]}),
        );
        let first = client.call("pane.transcript", json!({"paneId":first_pane}));
        let second = client.call("pane.transcript", json!({"paneId":second_pane}));
        let restarts = [&first, &second]
            .iter()
            .filter(|value| transcript_contains(value, "\"willRestartSession\":true"))
            .count();
        let empties = [&first, &second]
            .iter()
            .filter(|value| transcript_contains(value, "\"empty\":true"))
            .count();
        if restarts == 1 && empties == 1 {
            assert_eq!(shown["attempts"].as_array().unwrap().len(), 1);
            let owner = shown["task"]["ownerPaneId"].as_str().unwrap();
            assert!(owner == first_pane || owner == second_pane);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "task.next race did not settle once: task={shown}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn loop_for(client: &mut Client, pane: &str) -> Value {
    client
        .section("loops")
        .into_iter()
        .find(|item| item["paneId"] == pane)
        .expect("loop in snapshot")
}

fn native_prompt_count(rig: &Rig, session: &str, prompt: &str) -> usize {
    rig.native_session(session)["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|turn| turn[0] == prompt)
        .count()
}

const CHIEF_ROLE_PROMPT: &str = "Coordinate; do not implement. Create small, self-contained tasks with how to verify. Never depend on another task's memory. Dispatch only when no free pane is available to claim.";

fn appoint_chief(rig: &Rig, human: &mut Client, pane: &str) -> String {
    human.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"ready","idempotencyKey":"prime-lead-pane"}),
    );
    human.wait_state(pane, "idle");
    human.call(
        "pane.role.set",
        json!({"paneId":pane,"role":"chief","idempotencyKey":"appoint-lead"}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let session = loop {
        let current = human.pane(pane);
        let session = current["agent"]["sessionId"].as_str().unwrap_or_default();
        if current["state"] == "idle"
            && current["agent"]["role"] == "chief"
            && !session.is_empty()
            && native_prompt_count(rig, session, CHIEF_ROLE_PROMPT) == 1
        {
            break session.to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "chief role was not applied: {current}"
        );
        thread::sleep(Duration::from_millis(20));
    };
    human.call(
        "pane.role.set",
        json!({"paneId":pane,"role":"chief","idempotencyKey":"keep-lead"}),
    );
    thread::sleep(Duration::from_millis(100));
    assert_eq!(native_prompt_count(rig, &session, CHIEF_ROLE_PROMPT), 1);
    session
}

#[test]
fn pane_loop_stops_after_three_empty_task_results_and_explicit_off() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let session = client.pane(&pane)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"ready","idempotencyKey":"prime-loop-pane"}),
    );
    client.wait_state(&pane, "idle");
    client.call(
        "loop.set",
        json!({
            "paneId":pane,"prompt":"task-claim","interval":1,"maxRounds":10,
            "stopWhen":"three_empty","enabled":true,"idempotencyKey":"three-empty-loop"
        }),
    );

    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let saved = loop_for(&mut client, &pane);
        if saved["config"]["enabled"] == false
            && saved["config"]["emptyResults"] == 3
            && client.pane(&pane)["state"] == "idle"
        {
            break;
        }
        assert!(Instant::now() < deadline, "loop did not stop: {saved}");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(rig.database_probe().event_count("loop.tick").unwrap(), 3);
    assert_eq!(rig.database_probe().event_count("loop.stopped").unwrap(), 1);
    assert_eq!(native_prompt_count(&rig, &session, "task-claim"), 3);
    thread::sleep(Duration::from_millis(1200));
    assert_eq!(rig.database_probe().event_count("loop.tick").unwrap(), 3);

    client.call(
        "loop.set",
        json!({
            "paneId":pane,"prompt":"","interval":0,"maxRounds":0,
            "stopWhen":"explicit_off","enabled":false,"idempotencyKey":"explicit-off"
        }),
    );
    assert_eq!(rig.database_probe().event_count("loop.stopped").unwrap(), 2);
    assert_eq!(loop_for(&mut client, &pane)["config"]["enabled"], false);
}

#[test]
fn pane_loop_waits_while_busy_and_dispatches_only_one_due_round() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let session = client.pane(&pane)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"slow","idempotencyKey":"busy-turn"}),
    );
    client.wait_state(&pane, "working");
    client.call(
        "loop.set",
        json!({
            "paneId":pane,"prompt":"loop-after-idle","interval":1,"maxRounds":1,
            "stopWhen":"round_limit","enabled":true,"idempotencyKey":"busy-loop"
        }),
    );
    thread::sleep(Duration::from_millis(1200));
    assert_eq!(rig.database_probe().event_count("loop.tick").unwrap(), 0);

    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let saved = loop_for(&mut client, &pane);
        if saved["config"]["enabled"] == false
            && saved["config"]["rounds"] == 1
            && client.pane(&pane)["state"] == "idle"
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "due round did not finish: {saved}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(rig.database_probe().event_count("loop.tick").unwrap(), 1);
    assert_eq!(native_prompt_count(&rig, &session, "loop-after-idle"), 1);
}

#[test]
fn lead_role_prompt_and_task_authority_are_applied_to_the_live_session() {
    let mut rig = Rig::new();
    let (mut human, pane) = rig.agent();
    let workspace = human.pane(&pane)["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    appoint_chief(&rig, &mut human, &pane);
    rig.kill_engine();
    rig.start();
    human = rig.client();
    assert_eq!(human.pane(&pane)["agent"]["role"], "chief");

    let binding = rig.database_probe().agent_binding(&pane).unwrap();
    let (mut lead, welcome) = binding_hello(&rig.path.join("engine.sock"), &binding);
    assert_eq!(welcome["result"]["role"], "chief");
    let created = binding_request(
        &mut lead,
        2,
        "task.create",
        json!({
            "workspaceId":workspace,"title":"Lead-created task",
            "mustNotChange":"Keep the task contract","idempotencyKey":"lead-create"
        }),
    );
    let task = created["result"]["task"].clone();
    let listed = binding_request(
        &mut lead,
        3,
        "task.list",
        json!({"workspaceId":workspace,"limit":20}),
    );
    assert_eq!(listed["result"]["items"][0]["taskId"], task["taskId"]);
    let comment = binding_request(
        &mut lead,
        4,
        "task.comment",
        json!({
            "workspaceId":workspace,"taskId":task["taskId"],"text":"Ready to dispatch",
            "idempotencyKey":"lead-comment"
        }),
    );
    assert_eq!(comment["result"]["comment"]["text"], "Ready to dispatch");
    let dispatched = binding_request(
        &mut lead,
        5,
        "task.dispatch",
        json!({
            "workspaceId":workspace,"taskId":task["taskId"],
            "preset":{"workspaceDefault":true},"idempotencyKey":"lead-dispatch"
        }),
    );
    let worker_pane = dispatched["result"]["paneId"].as_str().unwrap();
    let busy_role = human.request(
        "pane.role.set",
        json!({"paneId":worker_pane,"role":"chief","idempotencyKey":"busy-role"}),
    );
    assert_eq!(busy_role["error"]["data"]["kind"], "pane_busy");
    let current = human.wait_state(worker_pane, "idle");
    let worker_binding = rig.database_probe().agent_binding(worker_pane).unwrap();
    let (mut worker, worker_welcome) =
        binding_hello(&rig.path.join("engine.sock"), &worker_binding);
    assert_eq!(worker_welcome["result"]["role"], "agent");
    let blocked = binding_request(
        &mut worker,
        6,
        "task.block",
        json!({
            "workspaceId":workspace,"taskId":task["taskId"],"kind":"missing_information",
            "reason":"Need a decision","paneId":worker_pane,
            "sessionGeneration":current["generation"],"attempt":dispatched["result"]["attempt"],
            "idempotencyKey":"worker-block"
        }),
    );
    assert_eq!(
        blocked["result"]["task"]["blocking"]["kind"],
        "missing_information"
    );
    let unblocked = binding_request(
        &mut lead,
        7,
        "task.unblock",
        json!({
            "workspaceId":workspace,"taskId":task["taskId"],"reason":"Decision supplied",
            "idempotencyKey":"lead-unblock"
        }),
    );
    assert!(unblocked["result"]["task"]["blocking"].is_null());
    let denied = binding_request(
        &mut lead,
        8,
        "task.complete",
        json!({
            "workspaceId":workspace,"taskId":task["taskId"],"summary":"Not mine",
            "contractChanges":[],"outOfScopeEdits":[],"idempotencyKey":"lead-complete"
        }),
    );
    assert_eq!(denied["error"]["data"]["kind"], "forbidden_role");
}

fn transcript_contains(transcript: &Value, needle: &str) -> bool {
    transcript["items"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains(needle))
        })
    })
}

fn assert_task_mutations_and_snapshot(
    client: &mut Client,
    workspace: &str,
    first: &Value,
    second: &Value,
) {
    let update = client.call(
        "task.update",
        json!({
            "workspaceId":workspace,"taskId":first["taskId"],"version":1,
            "title":"First revised","idempotencyKey":"update-first"
        }),
    );
    assert_eq!(update["task"]["version"], 2);
    let stale = client.request(
        "task.update",
        json!({
            "workspaceId":workspace,"taskId":first["taskId"],"version":1,
            "title":"Stale","idempotencyKey":"stale-first"
        }),
    );
    assert_eq!(stale["error"]["data"]["kind"], "stale_version");
    let cycle = client.request(
        "task.update",
        json!({
            "workspaceId":workspace,"taskId":first["taskId"],"version":2,
            "dependencies":[second["taskId"]],"idempotencyKey":"cycle-first"
        }),
    );
    assert_eq!(cycle["error"]["data"]["kind"], "dependency_cycle");

    let subitem = client.call(
        "task.subitem.add",
        json!({
            "workspaceId":workspace,"taskId":first["taskId"],"text":"Plan",
            "idempotencyKey":"subitem-add"
        }),
    )["subitem"]
        .clone();
    assert_eq!(subitem["subitemId"], "WS-1-T01");
    let done = client.call(
        "task.subitem.done",
        json!({
            "workspaceId":workspace,"taskId":first["taskId"],
            "subitemId":subitem["subitemId"],"idempotencyKey":"subitem-done"
        }),
    );
    assert_eq!(done["subitem"]["state"], "done");
    for (key, text) in [("comment-one", "Started"), ("comment-two", "Finished")] {
        client.call(
            "task.comment",
            json!({
                "workspaceId":workspace,"taskId":first["taskId"],"text":text,
                "idempotencyKey":key
            }),
        );
    }
    let comments = client.call(
        "task.comments",
        json!({"workspaceId":workspace,"taskId":first["taskId"],"limit":1}),
    );
    assert_eq!(comments["items"][0]["text"], "Finished");
    let older = client.call(
        "task.comments",
        json!({
            "workspaceId":workspace,"taskId":first["taskId"],"limit":1,
            "before":comments["nextCursor"]
        }),
    );
    assert_eq!(older["items"][0]["text"], "Started");

    let shown = client.call(
        "task.show",
        json!({"workspaceId":workspace,"taskId":first["taskId"]}),
    );
    assert_eq!(shown["task"]["title"], "First revised");
    assert_eq!(shown["subitems"].as_array().unwrap().len(), 1);
    assert_eq!(shown["comments"].as_array().unwrap().len(), 2);
    let (_, parts) = client.snapshot();
    assert_eq!(
        parts
            .iter()
            .filter(|part| part["section"] == "tasks")
            .count(),
        1
    );
    assert_eq!(
        parts
            .iter()
            .find(|part| part["section"] == "tasks")
            .unwrap()["data"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

fn assert_executor_cannot_read_unowned_tasks(rig: &Rig, workspace: &str, first: &Value) {
    let (_, pane) = rig.agent();
    let binding = rig.database_probe().agent_binding(&pane).unwrap();
    let (mut bound, welcome) = binding_hello(&rig.path.join("engine.sock"), &binding);
    assert_eq!(welcome["result"]["role"], "agent");
    let denied = binding_request(
        &mut bound,
        2,
        "task.show",
        json!({"workspaceId":workspace,"taskId":first["taskId"]}),
    );
    assert_eq!(denied["error"]["data"]["kind"], "forbidden_role");
    let denied = binding_request(
        &mut bound,
        3,
        "task.list",
        json!({"workspaceId":workspace,"limit":20}),
    );
    assert_eq!(denied["error"]["data"]["kind"], "forbidden_role");
}

#[test]
fn task_authoring_is_durable_paged_versioned_and_closed_by_role() {
    let rig = Rig::new();
    let mut client = rig.client();
    let (workspace, first, second) = create_task_fixture(&rig, &mut client);
    assert_task_mutations_and_snapshot(&mut client, &workspace, &first, &second);
    assert_executor_cannot_read_unowned_tasks(&rig, &workspace, &first);
}

#[test]
fn active_executor_blocks_requests_review_and_completes_one_durable_attempt() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Complete lifecycle", "reserve-complete");
    let blocked = lifecycle.executor_request(
        "task.block",
        &task,
        json!({"kind":"missing_information","reason":"Need an answer"}),
        "block",
    );
    assert_eq!(
        blocked["result"]["task"]["blocking"]["kind"],
        "missing_information"
    );
    let attention = lifecycle.human.section("attention");
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0]["paneId"], lifecycle.pane);
    assert!(
        attention[0]["message"]
            .as_str()
            .unwrap()
            .contains("Need an answer")
    );
    lifecycle.human.call(
        "task.unblock",
        json!({
            "workspaceId":lifecycle.workspace,"taskId":task.task_id,
            "reason":"Answer supplied","idempotencyKey":"unblock"
        }),
    );
    assert!(lifecycle.human.section("attention").is_empty());
    let added = lifecycle.executor_request(
        "task.subitem.add",
        &task,
        json!({"text":"Check the result"}),
        "add-step",
    );
    let refused = lifecycle.executor_request(
        "task.request_review",
        &task,
        json!({"reason":"Ready"}),
        "review-too-early",
    );
    assert_eq!(refused["error"]["data"]["kind"], "transition_refused");
    lifecycle.executor_request(
        "task.subitem.done",
        &task,
        json!({"subitemId":added["result"]["subitem"]["subitemId"]}),
        "finish-step",
    );
    let review = lifecycle.executor_request(
        "task.request_review",
        &task,
        json!({"reason":"Ready"}),
        "request-review",
    );
    assert_eq!(review["result"]["task"]["state"], "in_review");
    let review_task: Task = serde_json::from_value(review["result"]["task"].clone()).unwrap();
    let mut stale = review_task.clone();
    stale.current_attempt = 0;
    let refused = lifecycle.executor_request(
        "task.complete",
        &stale,
        json!({"summary":"Done","contractChanges":[],"outOfScopeEdits":[]}),
        "stale-complete",
    );
    assert_eq!(refused["error"]["data"]["kind"], "stale_attempt");
    let fields = json!({"summary":"Done","contractChanges":[],"outOfScopeEdits":[]});
    let completed =
        lifecycle.executor_request("task.complete", &review_task, fields.clone(), "complete");
    assert_eq!(completed["result"]["task"]["state"], "in_review");
    let shown = lifecycle.wait_task_state(&task.task_id, "completed");
    assert!(shown["task"]["ownerPaneId"].is_null());
    let repeated = lifecycle.executor_request("task.complete", &review_task, fields, "complete");
    assert_eq!(repeated["result"], completed["result"]);
    let store = Store::open(&lifecycle.rig.path).unwrap();
    let review: TaskReviewRecord<Value> = store
        .task_review(&lifecycle.workspace, &task.task_id, task.current_attempt)
        .unwrap()
        .unwrap();
    assert_eq!(review.stage, ReviewStage::Resolved);
    assert_eq!(review.payload["decision"]["approved"], true);
    let reviewer = lifecycle.human.wait_state(&review.reviewer_pane_id, "idle");
    let session = reviewer["agent"]["sessionId"].as_str().unwrap();
    assert!(
        lifecycle.rig.native_session(session)["mcp_servers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let attempts = store
        .task_attempts(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(
        attempts[0].result,
        Some(soyeht_protocol::AttemptResult::Completed)
    );
    assert_eq!(attempts[0].summary.as_deref(), Some("Done"));
    assert!(attempts[0].end.is_some());
    assert_eq!(
        store
            .task_moves(&lifecycle.workspace, &task.task_id)
            .unwrap()
            .len(),
        2
    );
}

fn replace_task_state(rig: &Rig, workspace: &str, task_id: &str, state: &str, key: &str) {
    let mut store = Store::open(&rig.path).unwrap();
    let mut task = store.task(workspace, task_id).unwrap().unwrap();
    task.state = serde_json::from_value(json!(state)).unwrap();
    task.version += 1;
    task.phrase = state.replace('_', " ");
    store
        .mutate("test", key, key, |change| {
            change.replace_task(&task, task.version - 1)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

fn completed_dependency_returns_task(lifecycle: &mut TaskLifecycleRig) -> Task {
    let task = lifecycle.create_and_reserve("Dependency target", "reserve-dependency-target");
    let dependency = lifecycle.human.call(
        "task.create",
        json!({
            "workspaceId":lifecycle.workspace,"title":"Dependency",
            "mustNotChange":"Keep behavior","idempotencyKey":"create-dependency"
        }),
    )["task"]
        .clone();
    lifecycle.executor_request(
        "task.block",
        &task,
        json!({"kind":"dependency","reason":"waiting","dependencyTaskId":dependency["taskId"]}),
        "block-dependency",
    );
    replace_task_state(
        &lifecycle.rig,
        &lifecycle.workspace,
        dependency["taskId"].as_str().unwrap(),
        "completed",
        "complete-dependency",
    );
    let returned = lifecycle.wait_task_state(&task.task_id, "todo");
    assert!(returned["task"]["blocking"].is_null());
    assert!(returned["task"]["ownerPaneId"].is_null());
    reserve_task(
        &lifecycle.rig,
        &lifecycle.workspace,
        &lifecycle.pane,
        "reserve-cancelled-dependency",
    )
}

fn cancelled_dependency_requires_a_decision(lifecycle: &mut TaskLifecycleRig, active: &Task) {
    let cancelled_dependency = lifecycle.human.call(
        "task.create",
        json!({
            "workspaceId":lifecycle.workspace,"title":"Cancelled dependency",
            "mustNotChange":"Keep behavior","idempotencyKey":"create-cancelled-dependency"
        }),
    )["task"]
        .clone();
    lifecycle.executor_request(
        "task.block",
        active,
        json!({"kind":"dependency","reason":"waiting","dependencyTaskId":cancelled_dependency["taskId"]}),
        "block-cancelled-dependency",
    );
    replace_task_state(
        &lifecycle.rig,
        &lifecycle.workspace,
        cancelled_dependency["taskId"].as_str().unwrap(),
        "cancelled",
        "cancel-dependency",
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let shown = lifecycle.human.call(
            "task.show",
            json!({"workspaceId":lifecycle.workspace,"taskId":active.task_id}),
        );
        if shown["task"]["phrase"] == "blocked: dependency cancelled" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "cancelled dependency was not reported: {shown}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    lifecycle.human.call(
        "task.unblock",
        json!({
            "workspaceId":lifecycle.workspace,"taskId":active.task_id,
            "reason":"Proceed without dependency","idempotencyKey":"unblock-cancelled-dependency"
        }),
    );
}

fn wait_for_retry(lifecycle: &mut TaskLifecycleRig, active: &Task, count: u32) {
    lifecycle
        .rig
        .database_probe()
        .make_task_retry_due(&lifecycle.workspace, &active.task_id)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let shown = lifecycle.human.call(
            "task.show",
            json!({"workspaceId":lifecycle.workspace,"taskId":active.task_id}),
        );
        if shown["task"]["blocking"]["retryCount"] == count {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "retry {count} did not run: {shown}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn external_error_exhausts_two_durable_retries(lifecycle: &mut TaskLifecycleRig, active: &Task) {
    lifecycle.executor_request(
        "task.block",
        active,
        json!({"kind":"external_error","reason":"service unavailable"}),
        "block-external",
    );
    for count in [1, 2] {
        wait_for_retry(lifecycle, active, count);
        lifecycle.executor_request(
            "task.block",
            active,
            json!({"kind":"external_error","reason":"service unavailable"}),
            &format!("external-still-blocked-{count}"),
        );
    }
    wait_for_external_escalation(lifecycle, active);
    assert_eq!(lifecycle.human.section("attention").len(), 1);
}

fn wait_for_external_escalation(lifecycle: &mut TaskLifecycleRig, active: &Task) {
    lifecycle
        .rig
        .database_probe()
        .make_task_retry_due(&lifecycle.workspace, &active.task_id)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let shown = lifecycle.human.call(
            "task.show",
            json!({"workspaceId":lifecycle.workspace,"taskId":active.task_id}),
        );
        if shown["task"]["blocking"]["kind"] == "missing_information" {
            assert!(
                shown["task"]["phrase"]
                    .as_str()
                    .unwrap()
                    .starts_with("blocked:")
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "external block did not escalate: {shown}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn typed_blocks_advance_from_real_engine_ticks_and_durable_deadlines() {
    let mut lifecycle = TaskLifecycleRig::new();
    let active = completed_dependency_returns_task(&mut lifecycle);
    cancelled_dependency_requires_a_decision(&mut lifecycle, &active);
    external_error_exhausts_two_durable_retries(&mut lifecycle, &active);
}

#[test]
fn undeclared_contract_drift_returns_the_same_attempt_with_a_review_prompt() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Return drift", "reserve-drift");
    fs::write(
        lifecycle.repository.join("src/lib.rs"),
        "pub fn calculate(value: Option<u32>) -> u32 {\n    value.unwrap_or_default()\n}\n",
    )
    .unwrap();
    let review = lifecycle.executor_request(
        "task.request_review",
        &task,
        json!({"reason":"Ready"}),
        "request-drift-review",
    );
    let review_task: Task = serde_json::from_value(review["result"]["task"].clone()).unwrap();
    lifecycle.executor_request(
        "task.complete",
        &review_task,
        json!({"summary":"Candidate","contractChanges":[],"outOfScopeEdits":[]}),
        "propose-drift-completion",
    );

    let shown = lifecycle.wait_task_state(&task.task_id, "in_progress");
    assert_eq!(shown["task"]["ownerPaneId"], lifecycle.pane);
    assert_eq!(shown["task"]["reviewBadge"], "your_turn");
    assert_eq!(shown["task"]["phrase"], "review requested changes");
    assert_eq!(shown["task"]["summary"], "Candidate");
    let store = Store::open(&lifecycle.rig.path).unwrap();
    let attempts = store
        .task_attempts(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(attempts.len(), 1);
    assert!(attempts[0].end.is_none());
    assert!(attempts[0].result.is_none());
    let comments = store
        .task_comments(&lifecycle.workspace, &task.task_id, None, 10)
        .unwrap();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].text.contains("silent_fallback"));
    assert!(comments[0].text.contains("returned:"));
    let review: TaskReviewRecord<Value> = store
        .task_review(&lifecycle.workspace, &task.task_id, task.current_attempt)
        .unwrap()
        .unwrap();
    assert_eq!(review.stage, ReviewStage::Resolved);
    assert_eq!(review.payload["decision"]["approved"], false);
    let transcript_or_queue = soyeht_store::agents::queue(&store, &lifecycle.pane)
        .unwrap()
        .iter()
        .any(|item| item.text.contains("silent_fallback"))
        || soyeht_store::agents::transcript(&store, &lifecycle.pane, None, 200)
            .unwrap()
            .items
            .iter()
            .any(|item| item.text.contains("silent_fallback"));
    assert!(transcript_or_queue);
    drop(store);

    lifecycle.human.wait_state(&lifecycle.pane, "idle");
    fs::write(
        lifecycle.repository.join("src/lib.rs"),
        "pub fn calculate(value: u32) -> u32 {\n    value + 3\n}\n",
    )
    .unwrap();
    let returned: Task = serde_json::from_value(shown["task"].clone()).unwrap();
    let second = lifecycle.executor_request(
        "task.request_review",
        &returned,
        json!({"reason":"Corrected"}),
        "request-corrected-review",
    );
    let second: Task = serde_json::from_value(second["result"]["task"].clone()).unwrap();
    lifecycle.executor_request(
        "task.complete",
        &second,
        json!({"summary":"Corrected","contractChanges":[],"outOfScopeEdits":[]}),
        "propose-corrected-completion",
    );
    let completed = lifecycle.wait_task_state(&task.task_id, "completed");
    assert_eq!(completed["task"]["summary"], "Corrected");
    assert!(lifecycle.review(&task).version >= 7);
}

#[test]
fn review_proposal_and_reviewer_turn_survive_an_engine_restart() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Restart review", "reserve-restart-review");
    let review = lifecycle.executor_request(
        "task.request_review",
        &task,
        json!({"reason":"Ready"}),
        "request-restart-review",
    );
    let review_task: Task = serde_json::from_value(review["result"]["task"].clone()).unwrap();
    lifecycle.executor_request(
        "task.complete",
        &review_task,
        json!({"summary":"Restart-safe","contractChanges":[],"outOfScopeEdits":[]}),
        "propose-before-restart",
    );
    let durable = lifecycle.review(&task);
    assert_eq!(durable.payload["completion"]["summary"], "Restart-safe");
    lifecycle.rig.kill_engine();
    lifecycle.rig.start();
    lifecycle.human = lifecycle.rig.client();

    let shown = lifecycle.wait_task_state(&task.task_id, "completed");
    assert_eq!(shown["task"]["summary"], "Restart-safe");
    let review = lifecycle.wait_review_stage(&task, ReviewStage::Resolved);
    assert_eq!(review.payload["decision"]["approved"], true);
    assert_eq!(
        Store::open(&lifecycle.rig.path)
            .unwrap()
            .task_attempts(&lifecycle.workspace, &task.task_id)
            .unwrap()[0]
            .result,
        Some(soyeht_protocol::AttemptResult::Completed)
    );
}

#[test]
fn reviewer_findings_resolve_after_completion_arrives_through_a_restarted_engine() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Findings first", "reserve-findings-first");
    let review = lifecycle.executor_request(
        "task.request_review",
        &task,
        json!({"reason":"Ready"}),
        "request-findings-first",
    );
    let review_task: Task = serde_json::from_value(review["result"]["task"].clone()).unwrap();
    let classified = lifecycle.wait_review_stage(&task, ReviewStage::Classified);
    assert_eq!(classified.payload["completion"], Value::Null);
    assert_eq!(classified.payload["findings"][0]["label"], "equivalent");

    lifecycle.rig.kill_engine();
    lifecycle.rig.start();
    lifecycle.human = lifecycle.rig.client();
    lifecycle.reconnect_executor();
    let proposed = lifecycle.executor_request(
        "task.complete",
        &review_task,
        json!({"summary":"Findings survived","contractChanges":[],"outOfScopeEdits":[]}),
        "complete-after-findings-restart",
    );
    assert_eq!(proposed["result"]["task"]["state"], "in_review");

    let shown = lifecycle.wait_task_state(&task.task_id, "completed");
    assert_eq!(shown["task"]["summary"], "Findings survived");
    let resolved = lifecycle.wait_review_stage(&task, ReviewStage::Resolved);
    assert_eq!(resolved.payload["decision"]["approved"], true);
}

#[test]
fn executor_release_is_idempotent_and_closes_the_attempt_as_returned() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Return lifecycle", "reserve-return");
    let fields = json!({"reason":"Needs another pass"});
    let released =
        lifecycle.executor_request("task.release", &task, fields.clone(), "release-task");
    assert_eq!(released["result"]["task"]["state"], "todo");
    assert!(released["result"]["task"]["ownerPaneId"].is_null());
    let repeated = lifecycle.executor_request("task.release", &task, fields, "release-task");
    assert_eq!(repeated["result"], released["result"]);
    let store = Store::open(&lifecycle.rig.path).unwrap();
    let attempts = store
        .task_attempts(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(
        attempts[0].result,
        Some(soyeht_protocol::AttemptResult::Returned)
    );
    assert_eq!(attempts[0].summary.as_deref(), Some("Needs another pass"));
    assert_eq!(
        store
            .task_moves(&lifecycle.workspace, &task.task_id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn dying_executor_returns_active_work_and_records_one_system_comment() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Recover after death", "reserve-death");
    lifecycle.human.call(
        "pane.prompt",
        json!({"paneId":lifecycle.pane,"text":"die","idempotencyKey":"die-turn"}),
    );
    lifecycle.human.wait_state(&lifecycle.pane, "dead");

    let store = Store::open(&lifecycle.rig.path).unwrap();
    let returned = store
        .task(&lifecycle.workspace, &task.task_id)
        .unwrap()
        .unwrap();
    assert_eq!(returned.state, soyeht_protocol::TaskState::Todo);
    assert_eq!(returned.owner_pane_id, None);
    let attempts = store
        .task_attempts(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(
        attempts[0].result,
        Some(soyeht_protocol::AttemptResult::Returned)
    );
    let comments = store
        .task_comments(&lifecycle.workspace, &task.task_id, None, 10)
        .unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].author, "system");
    assert_eq!(comments[0].text, "returned: the pane closed");
}

#[test]
fn closing_a_review_pane_unbinds_it_without_moving_the_card_back() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Preserve review", "reserve-review-close");
    let review = lifecycle.executor_request(
        "task.request_review",
        &task,
        json!({"reason":"Ready"}),
        "review-before-close",
    );
    assert_eq!(review["result"]["task"]["state"], "in_review");
    lifecycle.human.call(
        "pane.close",
        json!({"paneId":lifecycle.pane,"idempotencyKey":"close-review-pane"}),
    );

    let store = Store::open(&lifecycle.rig.path).unwrap();
    let waiting = store
        .task(&lifecycle.workspace, &task.task_id)
        .unwrap()
        .unwrap();
    assert_eq!(waiting.state, soyeht_protocol::TaskState::InReview);
    assert_eq!(waiting.owner_pane_id, None);
    assert_eq!(
        waiting.review_badge,
        Some(soyeht_protocol::TaskReviewBadge::YourTurn)
    );
    let attempts = store
        .task_attempts(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(
        attempts[0].result,
        Some(soyeht_protocol::AttemptResult::Returned)
    );
    assert_eq!(
        store
            .task_moves(&lifecycle.workspace, &task.task_id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn human_cancellation_stops_the_turn_and_closes_the_attempt_before_the_move() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Cancel active work", "reserve-cancel");
    lifecycle.human.call(
        "pane.prompt",
        json!({"paneId":lifecycle.pane,"text":"silent","idempotencyKey":"active-turn"}),
    );
    lifecycle.human.wait_state(&lifecycle.pane, "working");
    let params = json!({"workspaceId":lifecycle.workspace,"taskId":task.task_id,
        "state":"cancelled","reason":"Stopped by a person","idempotencyKey":"move-cancelled"});
    let cancelled = lifecycle.human.call("task.move", params.clone());
    assert_eq!(cancelled["task"]["state"], "cancelled");
    assert!(cancelled["task"]["ownerPaneId"].is_null());
    assert_eq!(lifecycle.human.call("task.move", params), cancelled);
    lifecycle.human.wait_state(&lifecycle.pane, "idle");

    let store = Store::open(&lifecycle.rig.path).unwrap();
    let attempts = store
        .task_attempts(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(
        attempts[0].result,
        Some(soyeht_protocol::AttemptResult::Cancelled)
    );
    assert_eq!(attempts[0].summary.as_deref(), Some("Stopped by a person"));
    let history = store
        .task_moves(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].actor, "you");
    assert_eq!(history[0].from, soyeht_protocol::TaskState::InProgress);
    assert_eq!(history[0].to, soyeht_protocol::TaskState::Cancelled);
    assert_eq!(history[0].reason, "Stopped by a person");
}

#[test]
fn human_drag_table_refuses_reserved_targets_and_records_accepted_moves() {
    let rig = Rig::new();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Moves","path":rig.path,"idempotencyKey":"move-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let task = client.call(
        "task.create",
        json!({"workspaceId":workspace,"title":"Move card","mustNotChange":"Keep history",
            "idempotencyKey":"move-task"}),
    )["task"]
        .clone();
    for (state, key) in [("in_progress", "move-active"), ("completed", "move-done")] {
        let refused = client.request(
            "task.move",
            json!({"workspaceId":workspace,"taskId":task["taskId"],"state":state,
                "reason":"Invalid drag","idempotencyKey":key}),
        );
        assert_eq!(refused["error"]["data"]["kind"], "transition_refused");
    }
    let cancelled = client.call(
        "task.move",
        json!({"workspaceId":workspace,"taskId":task["taskId"],"state":"cancelled",
            "reason":"No longer needed","idempotencyKey":"move-away"}),
    );
    assert_eq!(cancelled["task"]["state"], "cancelled");
    let restored = client.call(
        "task.move",
        json!({"workspaceId":workspace,"taskId":task["taskId"],"state":"todo",
            "reason":"Needed again","idempotencyKey":"move-back"}),
    );
    assert_eq!(restored["task"]["state"], "todo");
    let history = Store::open(&rig.path)
        .unwrap()
        .task_moves(&workspace, task["taskId"].as_str().unwrap())
        .unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].reason, "No longer needed");
    assert_eq!(history[1].reason, "Needed again");
}

#[test]
fn refused_move_enters_attention_and_a_valid_move_clears_it() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Move needs attention", "reserve-attention-move");
    let refused = lifecycle.human.request(
        "task.move",
        json!({
            "workspaceId":lifecycle.workspace,"taskId":task.task_id,"state":"in_progress",
            "reason":"Cannot reserve by dragging","idempotencyKey":"refused-attention-move"
        }),
    );
    assert_eq!(refused["error"]["data"]["kind"], "transition_refused");
    let attention = lifecycle.human.section("attention");
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0]["paneId"], lifecycle.pane);
    assert!(
        attention[0]["message"]
            .as_str()
            .unwrap()
            .contains("Move refused")
    );

    let moved = lifecycle.human.call(
        "task.move",
        json!({
            "workspaceId":lifecycle.workspace,"taskId":task.task_id,"state":"in_review",
            "reason":"Ready for review","idempotencyKey":"accepted-attention-move"
        }),
    );
    assert_eq!(moved["task"]["state"], "in_review");
    assert!(lifecycle.human.section("attention").is_empty());
}

#[test]
fn supervisor_overflow_during_engine_outage_produces_an_explicit_replay_gap() {
    let mut rig = Rig::new();
    let mut client = rig.client();
    let (_, pane) = client.create(&rig);
    let (before, _) = client.tail(&pane);
    client.call("pane.input",json!({"paneId":pane,"bytes":STANDARD.encode(b"sleep 0.2; head -c 9437184 /dev/zero; printf done > overflow.done\n"),"idempotencyKey":"overflow-input"}));
    drop(client);
    rig.kill_engine();
    let deadline = Instant::now() + Duration::from_secs(15);
    while fs::read(rig.path.join("overflow.done")).ok().as_deref() != Some(b"done") {
        assert!(
            Instant::now() < deadline,
            "supervisor blocked terminal output"
        );
        thread::sleep(Duration::from_millis(10));
    }
    rig.start();
    let mut client = rig.client();
    // Wait until the retained output has been durably ingested by the engine.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let (snapshot, _) = client.tail(&pane);
        if snapshot["nextOffset"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= 9437184
        {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    client.messages.clear();
    let after = client.call(
        "pane.stream.subscribe",
        json!({"paneId":pane,"from":{"streamId":before["streamId"],"offset":before["nextOffset"]}}),
    );
    assert_eq!(after["streamId"], before["streamId"]);
    let mut saw_gap = false;
    let mut next = before["nextOffset"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    loop {
        let frame = Frame::read_from(&mut client.socket)
            .unwrap()
            .expect("delta replay");
        match frame {
            Frame::Terminal { offset, bytes, .. } => {
                assert_eq!(offset, next);
                next += bytes.len() as u64;
            }
            Frame::Json(bytes) => {
                let event: Value = serde_json::from_slice(&bytes).unwrap();
                match event["method"].as_str() {
                    Some("stream.gap") => {
                        assert_eq!(
                            event["params"]["fromOffset"]
                                .as_str()
                                .unwrap()
                                .parse::<u64>()
                                .unwrap(),
                            next
                        );
                        next = event["params"]["toOffset"]
                            .as_str()
                            .unwrap()
                            .parse()
                            .unwrap();
                        saw_gap = true;
                    }
                    Some("stream.replay_done") => {
                        assert_eq!(
                            next,
                            event["params"]["nextOffset"]
                                .as_str()
                                .unwrap()
                                .parse::<u64>()
                                .unwrap()
                        );
                        break;
                    }
                    _ => (),
                }
            }
            _ => panic!("unexpected attachment"),
        }
    }
    assert!(
        saw_gap,
        "missing bytes must not be presented as a continuous stream"
    );
}
