#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_platform::{
    fs::symlink,
    local::{Stream, connect},
    testing::{allow_owner, deny_everyone},
};
use soyeht_protocol::{Task, frame::Frame};
use soyeht_ptyd::{
    client::Client as PtyClient,
    wire::{Request, Response, Spawn},
};
use soyeht_store::{
    database::Store,
    editor::{EditorFile, ProjectReplacement, ProjectReplacementItem},
    tasks::{ReviewStage, TaskReviewRecord},
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[path = "../support/mod.rs"]
mod support;
use support::*;

mod beat_board_and_messages;
mod cancel_and_agent_auth;
mod editor_agent_writes;
mod editor_drafts_and_web_panes;
mod harness_catalogue_and_mcp;
mod harness_switch;
mod harness_switch_names_and_failures;
mod language_and_project_search;
mod native_permissions_and_modes;
mod permissions_and_queue;
mod task_attempts;
mod task_dispatch;
mod terminal_hooks_and_phone;
mod terminal_replay_and_capture;
mod workspace_and_writing;
mod writing_documents;

fn experiment_rows(root: &std::path::Path) -> Vec<Value> {
    let Ok(panes) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut files = panes
        .flatten()
        .flat_map(|pane| fs::read_dir(pane.path()).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|suffix| suffix == "jsonl"))
        .collect::<Vec<_>>();
    files.sort();
    files
        .into_iter()
        .flat_map(|path| {
            fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

fn binding_hello(path: &std::path::Path, binding: &str) -> (Stream, Value) {
    let mut socket = connect(path).unwrap();
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

fn binding_request(socket: &mut Stream, id: u64, method: &str, params: Value) -> Value {
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
    executor: Stream,
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
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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

fn request_permission(client: &mut Client, pane: &str) -> Value {
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission-null","idempotencyKey":"permission-prompt"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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

fn mode_option(client: &mut Client, pane: &str) -> Value {
    client.pane(pane)["agent"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["category"] == "mode")
        .cloned()
        .expect("pane announces a mode selector")
}

fn current_mode(client: &mut Client, pane: &str) -> Value {
    mode_option(client, pane)["currentValue"].clone()
}

fn git(repository: &std::path::Path, arguments: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .status()
        .unwrap();
    assert!(status.success());
}

fn stage_interrupted_replacement(
    rig: &Rig,
    workspace: &str,
    params: &Value,
    before: &str,
    after: &str,
) -> String {
    let mut store = Store::open(&rig.path).unwrap();
    let file_id = Uuid::new_v4().to_string();
    let author = serde_json::from_value(local_file_author()).unwrap();
    let matcher = soyeht_editor::ContentMatcher::new("needle", true).unwrap();
    let found = matcher.search(before, Some("safe"), 10).unwrap();
    let fingerprint = STANDARD.encode(soyeht_engine::auth::token_hash(
        &json!(["file.search", params]).to_string(),
    ));
    let uid = soyeht_platform::user::id().unwrap();
    let item = ProjectReplacementItem {
        ordinal: 0,
        file_id: file_id.clone(),
        relative_path: "note.txt".into(),
        before_hash: test_content_hash(before),
        after_hash: test_content_hash(after),
        replaced: 1,
        applied: false,
    };
    let file = EditorFile {
        file_id: file_id.clone(),
        pane_id: None,
        workspace_id: workspace.into(),
        relative_path: "note.txt".into(),
        language: soyeht_protocol::EditorLanguage::PlainText,
        revision: 0,
        disk_revision: 0,
        disk_text: before.into(),
        draft_text: None,
        draft_author: None,
        conflict_text: None,
        watching: false,
        updated_ms: 1,
    };
    store
        .begin_file_replacement(
            &ProjectReplacement {
                operation_id: Uuid::new_v4().to_string(),
                scope: format!("local:{uid}:runtime-test"),
                idempotency_key: params["idempotencyKey"].as_str().unwrap().into(),
                fingerprint,
                workspace_id: workspace.into(),
                query: "needle".into(),
                replacement: "safe".into(),
                case_sensitive: true,
                author,
                matches: vec![soyeht_protocol::FileSearchMatch {
                    workspace_id: workspace.into(),
                    path: "note.txt".into(),
                    range: found.matches[0].range.clone(),
                    preview: found.matches[0].preview.clone(),
                }],
                total_replaced: 1,
                created_ms: 1,
                items: vec![item],
            },
            &[file],
        )
        .unwrap();
    file_id
}

fn test_content_hash(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn local_file_author() -> Value {
    json!({
        "actor":{"role":"human_local","client":"runtime-test"},
        "name":"you"
    })
}

fn wait_for_editor_file(
    client: &mut Client,
    file_id: &str,
    condition: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let file = client
            .section("files")
            .into_iter()
            .find(|file| file["fileId"] == file_id)
            .expect("editor file in snapshot");
        if condition(&file) {
            return file;
        }
        assert!(
            Instant::now() < deadline,
            "editor file did not advance: {file}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn process_instances(rig: &Rig) -> Vec<soyeht_ptyd::wire::Instance> {
    process_instances_at(rig, u64::MAX)
}

fn process_registry_instances(rig: &Rig) -> Vec<soyeht_ptyd::wire::Instance> {
    let bytes = fs::read(rig.path.join("ptyd-registry.json")).unwrap();
    let registry: Value = serde_json::from_slice(&bytes).unwrap();
    registry["instances"]
        .as_object()
        .unwrap()
        .values()
        .cloned()
        .map(|value| serde_json::from_value(value).unwrap())
        .filter(|instance: &soyeht_ptyd::wire::Instance| !instance.closed)
        .collect()
}

fn process_instances_at(rig: &Rig, generation: u64) -> Vec<soyeht_ptyd::wire::Instance> {
    let mut supervisor = PtyClient::connect(&rig.path.join("ptyd.sock"), generation).unwrap();
    let Response::Instances(instances) = supervisor.request(Request::List).unwrap() else {
        panic!("process registry")
    };
    instances
        .into_iter()
        .filter(|instance| !instance.closed)
        .collect()
}

fn native_prompt_count(rig: &Rig, session: &str, prompt: &str) -> usize {
    rig.native_session(session)["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|turn| turn[0] == prompt)
        .count()
}
