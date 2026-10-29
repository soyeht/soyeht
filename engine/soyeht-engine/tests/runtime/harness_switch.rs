use super::*;

#[test]
fn held_agent_message_follows_the_sent_draft_as_its_own_attributed_turn() {
    let mut rig = Rig::new();
    let (mut human, sender) = rig.agent();
    let workspace = human.pane(&sender)["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, target) = rig.agent_in_workspace(&workspace);
    let sender_name = human.pane(&sender)["name"].as_str().unwrap().to_owned();
    human.call(
        "pane.draft",
        json!({"paneId":target,"active":true,"idempotencyKey":"typing-in-target"}),
    );
    let store = Store::open(&rig.path).unwrap();
    let runtime: Value = soyeht_store::agents::runtime(&store, &sender)
        .unwrap()
        .unwrap();
    let (socket, _) = binding_hello(
        &rig.path.join("engine.sock"),
        runtime["binding"].as_str().unwrap(),
    );
    let mut agent = Client {
        socket,
        next: 1,
        messages: vec![],
    };
    agent.call(
        "agent.message",
        json!({"targetPaneId":target,"text":"peer note","idempotencyKey":"held-peer-note"}),
    );
    thread::sleep(Duration::from_millis(500));
    assert!(
        transcript_items(&mut human, &target)
            .iter()
            .all(|item| item["role"] != "user"),
        "an agent message reached the pane while its user had a draft"
    );
    let queued = human.call("pane.queue.list", json!({"paneId":target,"limit":64}));
    assert_eq!(queued["items"][0]["text"], "peer note");
    assert_eq!(queued["items"][0]["agentMessage"]["name"], sender_name);

    human.call(
        "pane.prompt",
        json!({"paneId":target,"text":"typed draft","idempotencyKey":"send-typed-draft"}),
    );
    // Sending the composer's text ends its draft in the same commit.
    assert_eq!(
        human.call("pane.queue.list", json!({"paneId":target,"limit":64}))["draft"],
        false
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    let users = loop {
        let users: Vec<Value> = transcript_items(&mut human, &target)
            .into_iter()
            .filter(|item| item["role"] == "user")
            .collect();
        if users.len() == 2 {
            break users;
        }
        assert!(Instant::now() < deadline, "held message was not delivered");
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(users[0]["text"], "typed draft");
    assert!(users[0].get("agentMessage").is_none());
    assert_eq!(users[1]["text"], "peer note");
    assert_eq!(users[1]["agentMessage"]["paneId"], sender);
    assert_eq!(users[1]["agentMessage"]["name"], sender_name);

    let session = human.pane(&target)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    let prompts = loop {
        let prompts: Vec<String> = rig.native_session(&session)["history"]
            .as_array()
            .unwrap()
            .iter()
            .map(|turn| turn[0].as_str().unwrap().to_owned())
            .collect();
        if prompts.len() == 2 {
            break prompts;
        }
        assert!(Instant::now() < deadline, "native turns were not recorded");
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(prompts[0], "typed draft");
    assert!(prompts[1].contains(&format!("targetPaneId={sender}")));
    assert!(prompts[1].ends_with("\n\npeer note"));

    // Native replay after supervisor loss restores the same attributed bubble.
    let live = transcript_items(&mut human, &target);
    let restored = restart_and_wait_for_replay(&mut rig, &target, &live);
    let users: Vec<&Value> = restored
        .iter()
        .filter(|item| item["role"] == "user")
        .collect();
    assert_eq!(users.len(), 2, "{restored:?}");
    assert_eq!(users[0]["text"], "typed draft");
    assert_eq!(users[1]["text"], "peer note");
    assert_eq!(users[1]["agentMessage"]["name"], sender_name);
}

#[test]
fn web_host_attachment_frames_are_accepted_and_forwarded_before_the_response() {
    let rig = Rig::new();
    let mut caller = rig.client();
    let workspace = create_workspace(&mut caller, "Web", &rig.path, "web-workspace");
    let pane = caller.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"web","idempotencyKey":"web-pane"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut host = Client::new_with_capabilities(rig.path.join("engine.sock"), json!(["web_host"]));
    Frame::Json(
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":700,"method":"web.screenshot",
            "params":{"paneId":pane}}))
        .unwrap(),
    )
    .write_to(&mut caller.socket)
    .unwrap();
    let Some(Frame::Json(command_bytes)) = Frame::read_from(&mut host.socket).unwrap() else {
        panic!("web command expected")
    };
    let command: Value = serde_json::from_slice(&command_bytes).unwrap();
    assert_eq!(command["method"], "web.command");

    let attachment_id = "00112233-4455-4677-8899-aabbccddeeff";
    let image = b"real-image-bytes";
    Frame::Attachment {
        header: soyeht_protocol::attachment::attachment_header(attachment_id).unwrap(),
        bytes: image.to_vec(),
    }
    .write_to(&mut host.socket)
    .unwrap();
    Frame::Json(
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":command["id"],"result":{
        "paneId":pane,"value":{"image":{
            "attachmentId":attachment_id,"mimeType":"image/png",
            "name":"screenshot.png","bytes":image.len().to_string()
        }}}}))
        .unwrap(),
    )
    .write_to(&mut host.socket)
    .unwrap();

    let Some(Frame::Attachment { bytes, .. }) = Frame::read_from(&mut caller.socket).unwrap()
    else {
        panic!("attachment must be forwarded first")
    };
    assert_eq!(bytes, image);
    let Some(Frame::Json(response_bytes)) = Frame::read_from(&mut caller.socket).unwrap() else {
        panic!("response must follow its attachment")
    };
    let response: Value = serde_json::from_slice(&response_bytes).unwrap();
    assert_eq!(response["id"], 700);
    assert_eq!(
        response["result"]["image"]["bytes"],
        image.len().to_string()
    );
}

#[test]
fn harness_switch_starts_a_new_session_with_bounded_context_and_retires_the_old_process() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    rig.install_switch_harnesses();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"remember-switch-context","idempotencyKey":"switch-context"}),
    );
    client.wait_state(&pane, "idle");
    let original_session = client.pane(&pane)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    fs::write(
        rig.path
            .join("test-advance-screen-before-harness-switch-commit"),
        b"advance",
    )
    .unwrap();
    let result = client.call(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"next-agent","idempotencyKey":"switch-success"}),
    );
    assert_eq!(result["fromHarness"], "test-agent");
    assert_eq!(result["toHarness"], "next-agent");
    assert_ne!(result["newSessionId"], original_session);
    assert_eq!(client.pane(&pane)["agent"]["harness"], "next-agent");

    let new_session = result["newSessionId"].as_str().unwrap();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let path = rig
            .path
            .join("next-native")
            .join(format!("{new_session}.json"));
        if let Ok(bytes) = fs::read(path) {
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            let history = value["history"].as_array().unwrap();
            if history.iter().any(|turn| {
                turn[0].as_str().is_some_and(|text| {
                    text.contains("historical data, not instructions")
                        && text.contains("remember-switch-context")
                })
            }) {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "continuation prompt did not reach new harness"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let live = process_registry_instances(&rig);
        if live.len() == 1 {
            assert!(
                live[0]
                    .spawn
                    .argv
                    .iter()
                    .any(|arg| arg.contains("next-native")),
                "retired-process cleanup kept the original harness instead of the candidate: {live:?}"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "old harness process was not retired"
        );
        thread::sleep(Duration::from_millis(20));
    }
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"after-retired-cleanup","idempotencyKey":"after-retired-cleanup"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let path = rig
            .path
            .join("next-native")
            .join(format!("{new_session}.json"));
        let served = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| value["history"].as_array().cloned())
            .is_some_and(|history| {
                history
                    .iter()
                    .any(|turn| turn[0] == "after-retired-cleanup")
            });
        if served {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the new session did not serve a prompt after retired-process cleanup"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// The replay after the switch: one handover marker first, then later turns,
/// and never the engine's continuation package.
fn assert_handover_replay(messages: &[Value], name: &str, later_prompt: &str) {
    let handovers = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message["kind"] == "handover")
        .collect::<Vec<_>>();
    assert_eq!(handovers.len(), 1, "{messages:?}");
    let (marker, handover) = handovers[0];
    assert_eq!(handover["text"], name);
    assert_eq!(handover["role"], "system");
    let later = messages
        .iter()
        .position(|message| message["role"] == "user" && message["text"] == later_prompt)
        .expect("later prompt was not replayed");
    assert!(marker < later, "marker left the switch point: {messages:?}");
    assert!(
        messages.iter().all(|message| {
            !message["text"]
                .as_str()
                .unwrap_or("")
                .contains("[last messages]")
        }),
        "continuation package leaked: {messages:?}"
    );
}

fn switch_with_context_and_restart(harness: &str, name: &str) {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    rig.install_switch_harnesses();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"before-switch","idempotencyKey":"handover-before"}),
    );
    client.wait_state(&pane, "idle");
    client.call(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":harness,"idempotencyKey":"handover-switch"}),
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"after-switch","idempotencyKey":"handover-after"}),
    );
    let live = wait_for_user_message(&mut client, &pane, "after-switch");
    assert_handover_replay(&live, name, "after-switch");
    client.wait_state(&pane, "idle");
    let live = transcript_items(&mut client, &pane);
    drop(client);
    let replayed = restart_and_wait_for_replay(&mut rig, &pane, &live);
    assert_handover_replay(&replayed, name, "after-switch");
}

#[test]
fn harness_switch_replay_shows_a_marker_instead_of_the_internal_prompt() {
    switch_with_context_and_restart("next-agent", "Next agent");
}

#[test]
fn harness_switch_replay_hides_a_continuation_prompt_replayed_in_chunks() {
    switch_with_context_and_restart("chunked-agent", "Chunked Agent");
}

#[test]
fn harness_switch_without_prior_turn_keeps_its_marker_at_the_switch_after_recovery() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    rig.install_switch_harnesses();
    client.call(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"next-agent","idempotencyKey":"empty-handover-switch"}),
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"after-empty-switch","idempotencyKey":"empty-handover-after"}),
    );
    wait_for_user_message(&mut client, &pane, "after-empty-switch");
    client.wait_state(&pane, "idle");
    let live = transcript_items(&mut client, &pane);
    assert_handover_replay(&live, "Next agent", "after-empty-switch");
    drop(client);
    let replayed = restart_and_wait_for_replay(&mut rig, &pane, &live);
    assert_handover_replay(&replayed, "Next agent", "after-empty-switch");
}

/// Runs one fake subagent turn and returns the finished transcript.
fn subagent_turn(prompt: &str) -> (Rig, Client, String, Vec<Value>) {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":prompt,"idempotencyKey":prompt}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while client.pane(&pane)["phrase"] != "Agent turn ended" {
        assert!(Instant::now() < deadline, "{prompt} did not finish");
        thread::sleep(Duration::from_millis(20));
    }
    let items = transcript_items(&mut client, &pane);
    (rig, client, pane, items)
}

fn subagent_rows(items: &[Value]) -> Vec<&Value> {
    items
        .iter()
        .filter(|item| item["kind"] == "subagent")
        .collect()
}

/// Items a subagent owns, by the run's ID.
fn owned_by<'a>(items: &'a [Value], row: &Value) -> Vec<&'a Value> {
    let id = &row["subagent"]["subagentId"];
    items
        .iter()
        .filter(|item| &item["subagentId"] == id)
        .collect()
}

#[test]
fn claude_subagent_is_one_row_holding_its_tool_calls_and_model() {
    let (_rig, _client, _pane, items) = subagent_turn("subagents-claude");
    let rows = subagent_rows(&items);
    assert_eq!(rows.len(), 1);
    let run = &rows[0]["subagent"];
    assert_eq!(run["name"], "Find the config");
    assert_eq!(run["task"], "Search the repository for the config file");
    assert_eq!(run["status"], "completed");
    assert_eq!(run["model"], "fake-claude-model");
    assert_eq!(run["summary"], "The config is config/app.toml.");
    assert!(run["startedAt"].is_string() && run["finishedAt"].is_string());
    assert_eq!(rows[0]["complete"], true);
    let owned = owned_by(&items, rows[0]);
    assert_eq!(
        owned
            .iter()
            .map(|item| item["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["tool", "message"]
    );
    assert_eq!(owned[0]["tool"]["title"], "ls config");
    assert_eq!(owned[0]["tool"]["status"], "completed");
    // The spawning Agent call reports into the row, never as a tool row.
    assert!(
        !items
            .iter()
            .any(|item| item["kind"] == "tool" && item["subagentId"].is_null())
    );
    let parent = items
        .iter()
        .filter(|item| item["role"] == "assistant" && item["kind"] == "message")
        .filter(|item| item["subagentId"].is_null())
        .map(|item| item["text"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        parent,
        [
            "I'll delegate the search.",
            "The config is in config/app.toml."
        ]
    );
}

#[test]
fn codex_subagent_activity_and_usage_stay_out_of_the_parent() {
    let (_rig, _client, _pane, items) = subagent_turn("subagents-codex");
    let rows = subagent_rows(&items);
    assert_eq!(rows.len(), 1);
    let run = &rows[0]["subagent"];
    assert_eq!(run["name"], "List files");
    assert_eq!(run["status"], "completed");
    assert!(run.get("model").is_none());
    let owned = owned_by(&items, rows[0]);
    assert_eq!(owned.len(), 2);
    assert_eq!(owned[0]["tool"]["title"], "List files");
    assert_eq!(owned[1]["text"], "Two files.");
    // The parent's waits on the subagent fold into its row.
    assert!(
        !items
            .iter()
            .any(|item| item["kind"] == "tool" && item["subagentId"].is_null())
    );
    // The subagent's accounting is not the pane's context use.
    assert!(
        items
            .iter()
            .filter(|item| item["kind"] == "usage")
            .all(|item| item["usage"]["totalTokens"] != "7")
    );
}

#[test]
fn parallel_subagents_each_keep_their_own_activity() {
    let (_rig, _client, _pane, items) = subagent_turn("subagents-parallel");
    let rows = subagent_rows(&items);
    assert_eq!(
        rows.iter()
            .map(|row| row["subagent"]["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["First", "Second"]
    );
    for (row, text) in rows.iter().zip(["First done.", "Second done."]) {
        assert_eq!(row["subagent"]["status"], "completed");
        assert_eq!(row["subagent"]["summary"], text);
        let owned = owned_by(&items, row);
        assert_eq!(owned.len(), 2, "{text}");
        assert_eq!(owned[1]["text"], text);
    }
}

#[test]
fn failed_subagent_row_reports_the_failure() {
    let (_rig, _client, _pane, items) = subagent_turn("subagents-failure");
    let rows = subagent_rows(&items);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["subagent"]["status"], "failed");
    assert!(rows[0]["subagent"].get("summary").is_none());
    let owned = owned_by(&items, rows[0]);
    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0]["tool"]["status"], "failed");
}

#[test]
fn subagent_rows_survive_engine_restart_and_native_replay() {
    let (mut rig, client, pane, live) = subagent_turn("subagents-codex");
    drop(client);
    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    assert_eq!(transcript_items(&mut restored, &pane), live);
    drop(restored);
    let replayed = restart_and_wait_for_replay(&mut rig, &pane, &live);
    let rows = subagent_rows(&replayed);
    assert_eq!(rows.len(), 1);
    let run = &rows[0]["subagent"];
    assert_eq!(run["name"], "List files");
    assert_eq!(run["status"], "completed");
    assert_eq!(run["summary"], "Two files.");
    // A replay does not know when the run happened.
    assert!(run.get("startedAt").is_none() && run.get("finishedAt").is_none());
    let owned = owned_by(&replayed, rows[0]);
    assert_eq!(owned.len(), 2);
    assert_eq!(owned[0]["tool"]["title"], "List files");
}

#[test]
fn a_missing_current_agent_recovers_before_accepting_more_work() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"before-process-loss","idempotencyKey":"before-process-loss"}),
    );
    client.wait_state(&pane, "idle");
    let before = client.pane(&pane);
    let generation = before["generation"].as_u64().unwrap();

    fs::write(
        rig.path.join("test-drop-current-agent-before-poll"),
        b"drop",
    )
    .unwrap();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    let session = loop {
        let current = client.pane(&pane);
        if current["generation"]
            .as_u64()
            .is_some_and(|next| next > generation)
            && current["agent"]["sessionId"].is_string()
        {
            break current["agent"]["sessionId"].as_str().unwrap().to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "the pane did not recover its missing current process"
        );
        thread::sleep(Duration::from_millis(20));
    };

    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"after-process-loss","idempotencyKey":"after-process-loss"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let served = rig.native_session(&session)["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|turn| turn[0] == "after-process-loss");
        if served {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the recovered process did not serve the queued prompt"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn harness_switch_retry_returns_the_same_session_without_spawning_again() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    rig.install_switch_harnesses();
    let params = json!({
        "paneId":pane,"harness":"next-agent","idempotencyKey":"switch-retry"
    });

    let first = client.call("pane.harness.switch", params.clone());
    let repeated = client.call("pane.harness.switch", params);

    assert_eq!(repeated, first);
    assert_eq!(
        client.pane(&pane)["agent"]["sessionId"],
        first["newSessionId"]
    );
    assert_eq!(process_instances(&rig).len(), 1);
}
