use super::*;

fn start_agent_case(rig: &Rig) -> (Client, String, Value) {
    let binary = sibling_binary("soyeht-fake-acp");
    fs::write(rig.path.join("harnesses.json"), serde_json::to_vec(&json!([{
        "id":"test-agent","name":"Test agent","command":binary,"args":["--state-dir",rig.path.join("native")],"version":"test"
    }])).unwrap()).unwrap();
    let mut client = rig.client();
    let workspace = create_workspace(
        &mut client,
        "Agent test",
        &rig.path,
        &Uuid::new_v4().to_string(),
    );
    let pane = client.call("pane.create", json!({"workspaceId":workspace,"mode":"agent","harness":"test-agent","idempotencyKey":Uuid::new_v4().to_string()}))["paneId"].as_str().unwrap().to_owned();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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
    rig.ptyd = Command::new(sibling_binary("soyeht-ptyd"))
        .arg("--data-dir")
        .arg(&rig.path)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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

/// A full-access mode is the agent's own policy: it never answers a form.
fn exercise_full_access_mode_and_form(client: &mut Client, pane: &str) {
    let applied = client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"allow_all","idempotencyKey":"full-access"}),
    );
    assert_eq!(applied["effective"], "allow_all");
    assert_eq!(current_mode(client, pane), "allow_all");
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"elicitation","idempotencyKey":"form-prompt"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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
            "form was not presented in a full-access mode"
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
    client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"ask","idempotencyKey":"restore-ask"}),
    );
    assert_eq!(current_mode(client, pane), "ask");
}

fn exercise_cancel_and_tools(rig: &Rig, client: &mut Client, pane: &str) {
    let pid = client.pane(pane)["pid"].clone();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"slow","idempotencyKey":"slow-prompt"}),
    );
    wait_until("slow turn did not start", || {
        client.pane(pane)["state"] == "working"
    });
    client.call(
        "pane.cancel",
        json!({"paneId":pane,"idempotencyKey":"cancel-slow"}),
    );
    wait_until("healthy cancellation exceeded deadline", || {
        client.pane(pane)["state"] == "idle"
    });
    assert_eq!(client.pane(pane)["pid"], pid);
    client.call(
        "pane.queue.resume",
        json!({"paneId":pane,"idempotencyKey":"resume-tools"}),
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"tools","idempotencyKey":"native-tools"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let transcript = client.call("pane.transcript", json!({"paneId":pane}));
        let has_terminal = transcript["items"].as_array().unwrap().iter().any(|item| {
            item["kind"] == "terminal"
                && item["terminal"]["bytes"].as_str().is_some_and(|bytes| {
                    STANDARD
                        .decode(bytes)
                        .is_ok_and(|bytes| printed(&bytes, b"fake-terminal"))
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
    exercise_full_access_mode_and_form(&mut first, &pane);
    exercise_cancel_and_tools(&rig, &mut first, &pane);
}

#[test]
fn editor_open_owns_one_bounded_workspace_file_and_restores_it_in_the_snapshot() {
    let mut rig = Rig::new();
    let repository = rig.path.join("editor-repository");
    fs::create_dir_all(repository.join("src")).unwrap();
    fs::write(
        repository.join("src/main.rs"),
        "fn main() {\n    println!(\"ready\");\n}\n",
    )
    .unwrap();
    fs::write(repository.join("src/lib.rs"), "pub fn ready() {}\n").unwrap();
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Editor", &repository, "editor-workspace");
    let opened = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"src/main.rs","line":1,"column":4,"idempotencyKey":"open-main"}),
    );
    assert_eq!(opened["pane"]["mode"], "editor");
    assert_eq!(opened["pane"]["pid"], Value::Null);
    assert_eq!(opened["file"]["path"], "src/main.rs");
    assert_eq!(opened["viewport"]["fromLine"], 1);
    assert_eq!(opened["viewport"]["lines"][0]["line"], 1);
    let file_id = opened["file"]["fileId"].as_str().unwrap();
    let pane_id = opened["pane"]["paneId"].as_str().unwrap();
    let read = client.call(
        "file.read",
        json!({"fileId":file_id,"revision":"1","source":"buffer","fromLine":0,"lineCount":1}),
    );
    assert_eq!(read["lines"][0]["text"], "fn main() {");
    let invalid_version = client.request(
        "file.read",
        json!({"fileId":file_id,"source":"disk","versionId":"unused","fromLine":0,"lineCount":1}),
    );
    assert_eq!(invalid_version["error"]["data"]["kind"], "invalid_params");
    let invalid_position = client.request(
        "file.open",
        json!({"workspaceId":workspace,"path":"src/main.rs","line":0,"column":99,"idempotencyKey":"open-invalid-position"}),
    );
    assert_eq!(invalid_position["error"]["data"]["kind"], "invalid_params");
    let second = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"src/main.rs","idempotencyKey":"open-main-again"}),
    );
    assert_eq!(second["file"]["fileId"], file_id);
    assert_eq!(second["pane"]["paneId"], pane_id);
    client.call(
        "pane.close",
        json!({"paneId":pane_id,"idempotencyKey":"close-main-editor"}),
    );
    let reopened = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"src/main.rs","idempotencyKey":"reopen-main"}),
    );
    assert_eq!(reopened["file"]["fileId"], file_id);
    assert_ne!(reopened["pane"]["paneId"], pane_id);
    let files = client.section("files");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["fileId"], file_id);
    let editor_pane = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"editor","path":"src/lib.rs","idempotencyKey":"create-editor-pane"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(client.pane(&editor_pane)["mode"], "editor");
    assert_eq!(client.pane(&editor_pane)["pid"], Value::Null);

    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    let files = restored.section("files");
    assert_eq!(files.len(), 2);
    let read = restored.call(
        "file.read",
        json!({"fileId":file_id,"fromLine":1,"lineCount":1}),
    );
    assert_eq!(read["lines"][0]["text"], "    println!(\"ready\");");
}

#[test]
fn file_diff_uses_the_current_buffer_revalidates_scope_and_rejects_stale_revisions() {
    let rig = Rig::new();
    let repository = rig.path.join("diff-repository");
    fs::create_dir_all(repository.join("src")).unwrap();
    let source = repository.join("src/main.rs");
    fs::write(&source, "fn main() {\n    before();\n}\n").unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["add", "."]);
    git(
        &repository,
        &[
            "-c",
            "user.name=Writer",
            "-c",
            "user.email=writer@example.invalid",
            "commit",
            "-qm",
            "baseline",
        ],
    );
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Diff", &repository, "diff-workspace");
    let file_id = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"src/main.rs","idempotencyKey":"diff-open"}),
    )["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    let written = client.call(
        "file.write",
        json!({
            "fileId":file_id,"baseRevision":"1","author":local_file_author(),
            "edits":[{"range":{"start":{"line":1,"column":4},"end":{"line":1,"column":10}},"text":"after"}],
            "idempotencyKey":"diff-write"
        }),
    );
    assert_eq!(written["file"]["revision"], "2");
    let diff = client.call("file.diff", json!({"fileId":file_id,"revision":"2"}));
    let lines = diff["hunks"][0]["lines"].as_array().unwrap();
    assert!(
        lines
            .iter()
            .any(|line| line["kind"] == "removed"
                && line["text"].as_str().unwrap().contains("before"))
    );
    assert!(lines.iter().any(|line| line["kind"] == "added" && line["text"].as_str().unwrap().contains("after")));
    assert_eq!(diff["blame"].as_array().unwrap().len(), 2);
    assert!(
        diff["blame"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["author"] == "Writer")
    );
    assert!(serde_json::to_vec(&diff).unwrap().len() < soyeht_protocol::limits::JSON_BYTES);
    let stale = client.request("file.diff", json!({"fileId":file_id,"revision":"1"}));
    assert_eq!(stale["error"]["data"]["kind"], "stale_version");

    let outside = rig.path.join("outside-diff.rs");
    fs::write(&outside, "outside\n").unwrap();
    fs::remove_file(&source).unwrap();
    symlink(&outside, &source).unwrap();
    let escaped = client.request("file.diff", json!({"fileId":file_id,"revision":"2"}));
    assert_eq!(escaped["error"]["data"]["kind"], "invalid_params");
}

#[test]
fn agent_writes_are_attributed_and_each_review_hunk_resolves_once() {
    let mut rig = Rig::new();
    let repository = rig.path.join("agent-editor");
    fs::create_dir_all(&repository).unwrap();
    let path = repository.join("agent-review.txt");
    let baseline = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\n";
    fs::write(&path, baseline).unwrap();
    let mut client = rig.client();
    let workspace = create_workspace(
        &mut client,
        "Agent editor",
        &repository,
        "agent-editor-workspace",
    );
    let opened = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"agent-review.txt","idempotencyKey":"open-agent-review"}),
    );
    let file_id = opened["file"]["fileId"].as_str().unwrap().to_owned();
    let (mut client, pane) = rig.open_agent(client, &json!(workspace));
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"editor-review","idempotencyKey":"agent-editor-write"}),
    );
    client.wait_state(&pane, "idle");
    let agent_name = client.pane(&pane)["name"].as_str().unwrap().to_owned();

    let first = rig
        .database_probe()
        .latest_event("editor.review_requested")
        .unwrap()
        .unwrap();
    assert_eq!(first["paneId"], pane);
    assert_eq!(first["fileId"], file_id);
    assert_eq!(first["hunks"].as_array().unwrap().len(), 2);
    assert!(first["hunks"].as_array().unwrap().iter().all(|hunk| {
        hunk["lines"].as_array().unwrap().iter().any(|line| {
            line["kind"] != "context"
                && line["author"]["actor"]["paneId"] == pane
                && line["author"]["name"] == agent_name
        })
    }));
    let history = client.call("file.history", json!({"fileId":file_id,"limit":1}));
    assert_eq!(history["items"][0]["author"]["actor"]["paneId"], pane);
    assert_eq!(history["items"][0]["author"]["name"], agent_name);
    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    let revision = first["revision"].as_str().unwrap();
    let first_hunk = first["hunks"][0]["hunkId"].as_str().unwrap();
    let accept = json!({"fileId":file_id,"hunkId":first_hunk,"baseRevision":revision,"idempotencyKey":"accept-first-agent-hunk"});
    let accepted = client.call("editor.review.accept", accept.clone());
    assert_eq!(client.call("editor.review.accept", accept), accepted);
    let remaining = rig
        .database_probe()
        .latest_event("editor.review_requested")
        .unwrap()
        .unwrap();
    assert_eq!(remaining["hunks"].as_array().unwrap().len(), 1);
    let second_hunk = remaining["hunks"][0]["hunkId"].as_str().unwrap();
    let reject = json!({"fileId":file_id,"hunkId":second_hunk,"baseRevision":revision,"idempotencyKey":"reject-second-agent-hunk"});
    let rejected = client.call("editor.review.reject", reject.clone());
    assert_eq!(client.call("editor.review.reject", reject), rejected);
    assert_eq!(rejected["file"]["revision"], "3");
    let final_review = rig
        .database_probe()
        .latest_event("editor.review_requested")
        .unwrap()
        .unwrap();
    assert!(final_review["hunks"].as_array().unwrap().is_empty());
    let content = client.call(
        "file.read",
        json!({"fileId":file_id,"fromLine":0,"lineCount":20}),
    )["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| format!("{}\n", line["text"].as_str().unwrap()))
        .collect::<String>();
    assert_eq!(
        content,
        "one\nagent two\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\n\n"
    );
}

#[test]
fn editor_line_comment_is_one_durable_task_change_request() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Review agent edit", "reserve-editor-comment");
    let path = lifecycle.repository.join("agent-review.txt");
    fs::write(
        &path,
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\n",
    )
    .unwrap();
    let opened = lifecycle.human.call(
        "file.open",
        json!({"workspaceId":lifecycle.workspace,"path":"agent-review.txt","idempotencyKey":"open-comment-review"}),
    );
    let file_id = opened["file"]["fileId"].as_str().unwrap().to_owned();
    lifecycle.human.call(
        "pane.prompt",
        json!({"paneId":lifecycle.pane,"text":"editor-review","idempotencyKey":"write-comment-review"}),
    );
    lifecycle.human.wait_state(&lifecycle.pane, "idle");
    let request = json!({
        "fileId":file_id,
        "range":{"start":{"line":1,"column":0},"end":{"line":1,"column":9}},
        "body":"Keep the original name here.",
        "idempotencyKey":"editor-line-comment"
    });
    let result = lifecycle
        .human
        .call("editor.review.comment", request.clone());
    assert_eq!(
        lifecycle.human.call("editor.review.comment", request),
        result
    );
    assert_eq!(result["taskId"], task.task_id);
    assert_eq!(result["delivered"], true);

    let shown = lifecycle.human.call(
        "task.show",
        json!({"workspaceId":lifecycle.workspace,"taskId":task.task_id}),
    );
    assert_eq!(shown["task"]["reviewBadge"], "system_turn");
    assert_eq!(shown["task"]["phrase"], "review requested changes");
    let expected = "Change request at agent-review.txt:2:1\n\nKeep the original name here.";
    let store = Store::open(&lifecycle.rig.path).unwrap();
    let comments = store
        .task_comments(&lifecycle.workspace, &task.task_id, None, 10)
        .unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].comment_id, result["commentId"]);
    assert_eq!(comments[0].author, "you");
    assert_eq!(comments[0].text, expected);
    drop(store);

    let session = lifecycle.human.pane(&lifecycle.pane)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    wait_until("change request was not delivered", || {
        native_prompt_count(&lifecycle.rig, &session, expected) == 1
    });
    lifecycle.human.wait_state(&lifecycle.pane, "idle");
    assert_eq!(native_prompt_count(&lifecycle.rig, &session, expected), 1);
}

#[test]
fn agent_write_refuses_a_dirty_editor_file_without_touching_disk() {
    let rig = Rig::new();
    let repository = rig.path.join("agent-editor-dirty");
    fs::create_dir_all(&repository).unwrap();
    let path = repository.join("agent-review.txt");
    let baseline = "one\ntwo\nthree\n";
    fs::write(&path, baseline).unwrap();
    let mut client = rig.client();
    let workspace = create_workspace(
        &mut client,
        "Dirty editor",
        &repository,
        "dirty-editor-workspace",
    );
    let file_id = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"agent-review.txt","idempotencyKey":"open-dirty-review"}),
    )["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (mut client, pane) = rig.open_agent(client, &json!(workspace));
    client.call(
        "file.watch",
        json!({"fileId":file_id,"watch":false,"idempotencyKey":"dirty-review-watch-off"}),
    );
    fs::write(&path, "external\n").unwrap();
    client.call(
        "file.write",
        json!({"fileId":file_id,"baseRevision":"1","author":local_file_author(),"edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":3}},"text":"human"}],"idempotencyKey":"human-draft-before-agent"}),
    );
    client.call(
        "file.watch",
        json!({"fileId":file_id,"watch":true,"idempotencyKey":"dirty-review-watch-on"}),
    );
    wait_for_editor_file(&mut client, &file_id, |file| file["conflicted"] == true);
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"editor-review","idempotencyKey":"agent-write-refused"}),
    );
    client.wait_state(&pane, "idle");
    assert_eq!(fs::read_to_string(path).unwrap(), "external\n");
    let result = rig
        .database_probe()
        .latest_request_result("fs/write_text_file", &pane)
        .unwrap()
        .unwrap();
    assert!(
        result["native"]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("clean editor file")
    );
}
