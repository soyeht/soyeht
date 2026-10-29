use super::*;

#[test]
fn observed_model_refusal_is_plain_durable_and_does_not_guess_from_generic_errors() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"refuse-disabled-model","idempotencyKey":"disabled-model"}),
    );
    let transcript =
        wait_for_transcript(&mut client, &pane, "refusal was not published", |message| {
            message["text"] == "The agent refused model \"model-a\": Model is disabled"
        });
    assert!(!transcript.to_string().contains("Internal error:"));
    let models = client.call(
        "model.list",
        json!({"harness":"test-agent","paneId":pane,"limit":64}),
    );
    let refused = models["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["id"] == "model-a")
        .unwrap();
    assert_eq!(refused["observedRefusal"]["reason"], "Model is disabled");

    client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"model","value":"model-b","idempotencyKey":"choose-model-b"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let pane_view = client.pane(&pane);
        let selected = pane_view["agent"]["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|option| option["id"] == "model")
            .map(|option| &option["currentValue"]);
        if selected == Some(&json!("model-b")) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "model did not change: {pane_view}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"refuse-generic-error","idempotencyKey":"generic-error"}),
    );
    let transcript = wait_for_transcript(
        &mut client,
        &pane,
        "generic error was not published",
        |message| message["text"] == "Agent error: Temporary upstream failure",
    );
    assert!(!transcript.to_string().contains("Internal error:"));
    let models = client.call(
        "model.list",
        json!({"harness":"test-agent","paneId":pane,"limit":64}),
    );
    assert!(
        models["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|model| model["id"] == "model-b")
            .unwrap()["observedRefusal"]
            .is_null()
    );

    drop(client);
    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    let models = restored.call(
        "model.list",
        json!({"harness":"test-agent","paneId":pane,"limit":64}),
    );
    assert_eq!(
        models["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|model| model["id"] == "model-a")
            .unwrap()["observedRefusal"]["reason"],
        "Model is disabled"
    );
}

#[test]
fn undisplayable_execute_permission_is_rejected_and_explained_in_the_transcript() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission-missing-command","idempotencyKey":"missing-command"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let transcript = client.call("pane.transcript", json!({"paneId":pane}));
        if let Some(message) = transcript["items"].as_array().unwrap().iter().find(|item| {
            item["role"] == "system"
                && item["kind"] == "system"
                && item["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("no command to display"))
        }) {
            assert!(
                message["text"]
                    .as_str()
                    .unwrap()
                    .contains("Nothing was approved")
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "permission rejection was not visible"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(client.section("requests").is_empty());
    assert_eq!(
        rig.database_probe()
            .event_count_by_field("pane.update", "kind", "system_message")
            .unwrap(),
        1
    );
}

/// Runs a fake MCP approval turn, `tracked-mcp:<tool>` or
/// `subagent-mcp:<tool>`, and returns the presented request.
fn mcp_permission(client: &mut Client, pane: &str, turn: &str, key: &str) -> Value {
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":format!("permission-{turn}"),"idempotencyKey":key}),
    );
    client.wait_state(pane, "waiting_for_you");
    client.section("requests")[0]["request"].clone()
}

#[test]
fn codex_mcp_approval_carrying_only_the_tool_call_id_is_presented_from_the_tracked_call() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let first = mcp_permission(&mut client, &pane, "tracked-mcp:list_agents", "list-agents");
    assert_eq!(first["title"], "Use soyeht-workspace · list_agents");
    assert_eq!(first["presentation"]["command"], Value::Null);
    assert_eq!(
        first["presentation"]["toolArguments"],
        json!([{"name":"text","value":"  first line\n  second line"}])
    );
    client.call("pane.permission.answer", json!({"paneId":pane,"requestId":first["requestId"],"optionId":"always","idempotencyKey":"always-list"}));
    client.wait_state(&pane, "idle");
    // A grant for one tool never covers another tool whose request looks
    // the same without its tracked call.
    let second = mcp_permission(
        &mut client,
        &pane,
        "tracked-mcp:message_agent",
        "message-agent",
    );
    assert_eq!(second["title"], "Use soyeht-workspace · message_agent");
    client.call("pane.permission.answer", json!({"paneId":pane,"requestId":second["requestId"],"optionId":"deny","idempotencyKey":"deny-message"}));
    client.wait_state(&pane, "idle");
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission-tracked-mcp:list_agents","idempotencyKey":"list-again"}),
    );
    wait_until("the granted tool was not approved by its rule", || {
        rig.database_probe()
            .event_count_by_field("pane.permission_answered", "by", "rule")
            .unwrap()
            != 0
    });
    client.wait_state(&pane, "idle");
    assert_eq!(
        rig.database_probe()
            .event_count("pane.permission_requested")
            .unwrap(),
        2
    );
}

#[test]
fn subagent_mcp_approval_is_presented_from_the_subagents_tracked_call() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let request = mcp_permission(
        &mut client,
        &pane,
        "subagent-mcp:message_agent",
        "subagent-message",
    );
    assert_eq!(request["title"], "Use soyeht-workspace · message_agent");
    assert_eq!(request["presentation"]["command"], Value::Null);
    assert_eq!(
        request["presentation"]["toolArguments"],
        json!([{"name":"text","value":"  first line\n  second line"}])
    );
    client.call("pane.permission.answer", json!({"paneId":pane,"requestId":request["requestId"],"optionId":"once","idempotencyKey":"once-subagent"}));
    client.wait_state(&pane, "idle");
    let items = transcript_items(&mut client, &pane);
    let row = items
        .iter()
        .find(|item| item["kind"] == "subagent")
        .expect("the asking subagent's row");
    let tool = items
        .iter()
        .find(|item| item["kind"] == "tool")
        .expect("the subagent's tool call");
    assert_eq!(tool["subagentId"], row["subagent"]["subagentId"]);
}

#[test]
fn a_parent_grant_never_approves_a_subagents_request_for_the_same_call() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let parent = mcp_permission(&mut client, &pane, "tracked-mcp:list_agents", "parent-list");
    client.call("pane.permission.answer", json!({"paneId":pane,"requestId":parent["requestId"],"optionId":"always","idempotencyKey":"always-parent"}));
    client.wait_state(&pane, "idle");
    // Same tool call id, tool and arguments, asked from a subagent session.
    let subagent = mcp_permission(
        &mut client,
        &pane,
        "subagent-mcp:list_agents",
        "subagent-list",
    );
    assert_eq!(subagent["title"], parent["title"]);
    assert_eq!(subagent["presentation"], parent["presentation"]);
    assert_eq!(
        rig.database_probe()
            .event_count_by_field("pane.permission_answered", "by", "rule")
            .unwrap(),
        0
    );
}

#[test]
fn approval_for_an_unreported_tool_call_without_input_stays_blocked() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission-untracked-mcp","idempotencyKey":"untracked"}),
    );
    wait_for_transcript(&mut client, &pane, "the approval was not blocked", |item| {
        item["role"] == "system"
            && item["text"].as_str().is_some_and(|text| {
                text.contains("no command to display") && text.contains("Nothing was approved")
            })
    });
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let transcript = client.call("pane.transcript", json!({"paneId":pane}));
        if transcript["items"].as_array().unwrap().iter().any(|item| {
            item["role"] == "system"
                && item["text"].as_str().is_some_and(|text| {
                    text.contains("no command to display") && text.contains("Nothing was approved")
                })
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "the approval was not blocked");
        thread::sleep(Duration::from_millis(20));
    }
    assert!(client.section("requests").is_empty());
    assert_eq!(
        rig.database_probe()
            .event_count("pane.permission_requested")
            .unwrap(),
        0
    );
}

fn wait_for_mode(client: &mut Client, pane: &str, mode: &str) {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while current_mode(client, pane) != mode {
        assert!(Instant::now() < deadline, "mode did not become {mode}");
        thread::sleep(Duration::from_millis(20));
    }
}

fn run_turn(client: &mut Client, pane: &str, text: &str) {
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":text,"idempotencyKey":Uuid::new_v4().to_string()}),
    );
    client.wait_state(pane, "idle");
}

#[test]
fn native_modes_change_only_on_confirmation_and_follow_the_agent() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let modes = mode_option(&mut client, &pane);
    let kinds: Vec<_> = modes["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|choice| (choice["value"].clone(), choice["kind"].clone()))
        .collect();
    assert_eq!(kinds[2], (json!("allow_all"), json!("full_access")));
    assert_eq!(modes["currentValue"], "ask");
    let applied = client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"accept_edits","idempotencyKey":"edits"}),
    );
    assert_eq!(applied["effective"], "accept_edits");
    let refused = client.request(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"locked","idempotencyKey":"locked"}),
    );
    assert_eq!(refused["error"]["data"]["kind"], "invalid_params");
    assert_eq!(current_mode(&mut client, &pane), "accept_edits");
    assert!(client.pane(&pane)["agent"]["operation"].is_null());
    run_turn(&mut client, &pane, "switch-mode:allow_all");
    wait_for_mode(&mut client, &pane, "allow_all");
    run_turn(&mut client, &pane, "permission");
    assert!(client.section("requests").is_empty());
    client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"ask","idempotencyKey":"ask"}),
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission","idempotencyKey":"asks"}),
    );
    client.wait_state(&pane, "waiting_for_you");
    assert_eq!(client.section("requests")[0]["kind"], "permission");
}

#[test]
fn legacy_session_modes_use_set_mode_and_follow_the_agent() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent_announcing("legacy");
    assert_eq!(
        mode_option(&mut client, &pane)["options"][2]["kind"],
        "full_access"
    );
    client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"allow_all","idempotencyKey":"all"}),
    );
    let session = client.pane(&pane)["agent"]["sessionId"].clone();
    assert_eq!(
        rig.native_session(session.as_str().unwrap())["values"]["mode"],
        "allow_all"
    );
    run_turn(&mut client, &pane, "switch-mode:accept_edits");
    wait_for_mode(&mut client, &pane, "accept_edits");
    let refused = client.request(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"locked","idempotencyKey":"locked"}),
    );
    assert_eq!(refused["error"]["data"]["kind"], "invalid_params");
    assert_eq!(current_mode(&mut client, &pane), "accept_edits");
}

#[test]
fn chosen_mode_returns_after_restart_and_harness_switch() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"accept_edits","idempotencyKey":"edits"}),
    );
    run_turn(&mut client, &pane, "before restart");
    let live = transcript_items(&mut client, &pane);
    restart_and_wait_for_replay(&mut rig, &pane, &live);
    let mut client = rig.client();
    // The fake reloads sessions in its default mode, as real adapters do.
    wait_for_mode(&mut client, &pane, "accept_edits");
    let session = client.pane(&pane)["agent"]["sessionId"].clone();
    assert_eq!(
        rig.native_session(session.as_str().unwrap())["values"]["mode"],
        "accept_edits"
    );
    rig.install_switch_harnesses();
    client.call(
        "settings.set",
        json!({"key":soyeht_protocol::setting::harness_default_mode_key("next-agent"),"value":"allow_all","idempotencyKey":"default"}),
    );
    client.call(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"next-agent","idempotencyKey":"switch"}),
    );
    wait_for_mode(&mut client, &pane, "accept_edits");
}

#[test]
fn new_panes_start_in_the_preferred_mode_only_when_offered() {
    let rig = Rig::new();
    let mut client = rig.client();
    client.call(
        "settings.set",
        json!({"key":soyeht_protocol::setting::harness_default_mode_key("test-agent"),"value":"accept_edits","idempotencyKey":"default"}),
    );
    let (mut client, pane) = rig.agent();
    wait_for_mode(&mut client, &pane, "accept_edits");
    let workspace = client.pane(&pane)["workspaceId"].clone();
    let unoffered = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"agent","harness":"test-agent",
            "permissionMode":"plan","idempotencyKey":"unoffered"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    run_turn(&mut client, &unoffered, "hello");
    assert_eq!(current_mode(&mut client, &unoffered), "ask");
}

#[test]
fn a_mode_the_agent_changes_during_a_model_change_is_kept() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"model","value":"mode-shifting-model","idempotencyKey":"model"}),
    );
    assert_eq!(current_mode(&mut client, &pane), "accept_edits");
    run_turn(&mut client, &pane, "before restart");
    let live = transcript_items(&mut client, &pane);
    restart_and_wait_for_replay(&mut rig, &pane, &live);
    let mut client = rig.client();
    // The fake reloads in its default mode; the pane restores the agent's choice.
    wait_for_mode(&mut client, &pane, "accept_edits");
}

#[test]
fn a_refused_mode_leaves_the_queue_running_in_the_agents_mode() {
    let mut rig = Rig::new();
    let mut client = rig.client();
    client.call(
        "settings.set",
        json!({"key":soyeht_protocol::setting::harness_default_mode_key("test-agent"),"value":"locked","idempotencyKey":"locked"}),
    );
    let (mut client, pane) = rig.agent();
    run_turn(&mut client, &pane, "hello");
    assert_eq!(current_mode(&mut client, &pane), "ask");
    let notice = transcript_items(&mut client, &pane);
    assert!(
        notice.iter().any(|item| item["role"] == "system"
            && item["text"] == "The agent did not enter Locked; it stays in Ask"),
        "{notice:?}"
    );
    let live = transcript_items(&mut client, &pane);
    restart_and_wait_for_replay(&mut rig, &pane, &live);
    let mut client = rig.client();
    run_turn(&mut client, &pane, "after restart");
    assert_eq!(current_mode(&mut client, &pane), "ask");
}

#[test]
fn a_mode_the_next_harness_lacks_is_not_restored_on_the_way_back() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let binary = sibling_binary("soyeht-fake-acp");
    fs::write(
        rig.path.join("harnesses.json"),
        serde_json::to_vec(&json!([
            {"id":"test-agent","name":"Test agent","command":binary,
                "args":["--state-dir",rig.path.join("native"),"--modes","config"],"version":"test"},
            {"id":"restricted-agent","name":"Restricted agent","command":binary,
                "args":["--state-dir",rig.path.join("restricted-native"),"--modes","restricted"],
                "version":"test"}
        ]))
        .unwrap(),
    )
    .unwrap();
    client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"allow_all","idempotencyKey":"all"}),
    );
    for (harness, key) in [("restricted-agent", "away"), ("test-agent", "back")] {
        client.call(
            "pane.harness.switch",
            json!({"paneId":pane,"harness":harness,"idempotencyKey":key}),
        );
        run_turn(&mut client, &pane, key);
        assert_eq!(current_mode(&mut client, &pane), "ask", "{harness}");
    }
}
