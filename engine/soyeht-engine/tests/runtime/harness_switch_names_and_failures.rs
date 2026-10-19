use super::*;

#[test]
fn unnamed_agent_panes_take_unique_harness_names_in_their_workspace() {
    let rig = Rig::new();
    let (mut client, first) = rig.agent();
    let workspace = client.pane(&first)["workspaceId"].clone();
    let second = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"agent","harness":"test-agent",
            "idempotencyKey":"second-agent"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let named = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"agent","harness":"test-agent",
            "name":"Reviewer","idempotencyKey":"named-agent"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();

    assert_eq!(client.pane(&first)["name"], "Test agent");
    assert_eq!(client.pane(&second)["name"], "Test agent 2");
    assert_eq!(client.pane(&named)["name"], "Reviewer");
}

#[test]
fn harness_switch_renames_a_default_named_pane_to_the_new_harness() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    rig.install_switch_harnesses();
    assert_eq!(client.pane(&pane)["name"], "Test agent");

    client.call(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"chunked-agent","idempotencyKey":"switch-default-name"}),
    );
    assert_eq!(client.pane(&pane)["name"], "Chunked");
}

#[test]
fn harness_switch_keeps_a_name_the_user_chose() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    rig.install_switch_harnesses();
    client.call(
        "pane.rename",
        json!({"paneId":pane,"name":"Planner","idempotencyKey":"rename-planner"}),
    );

    client.call(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"next-agent","idempotencyKey":"switch-user-name"}),
    );

    assert_eq!(client.pane(&pane)["name"], "Planner");
}

#[test]
fn harness_switch_carries_a_frozen_pending_prompt_once_and_removes_it_from_the_queue() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    rig.install_switch_harnesses();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"slow","idempotencyKey":"switch-active"}),
    );
    client.wait_state(&pane, "working");
    let queued = client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"pending-during-switch","idempotencyKey":"switch-pending"}),
    );
    assert_eq!(queued["queued"], true);

    let result = client.call(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"next-agent","idempotencyKey":"switch-with-pending"}),
    );
    let new_session = result["newSessionId"].as_str().unwrap();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let path = rig
            .path
            .join("next-native")
            .join(format!("{new_session}.json"));
        let carried = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| value["history"].as_array().cloned())
            .is_some_and(|history| {
                history.iter().any(|turn| {
                    turn[0].as_str().is_some_and(|text| {
                        text.matches("pending-during-switch").count() == 1
                            && text.contains("\"state\":\"pending\"")
                    })
                })
            });
        if carried {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the pending prompt did not reach the continuation package exactly once"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        soyeht_store::agents::queue(&Store::open(&rig.path).unwrap(), &pane)
            .unwrap()
            .is_empty(),
        "an incorporated pending prompt must not be delivered again"
    );
}

#[test]
fn induced_candidate_failure_leaves_the_original_session_serving_prompts() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    rig.install_switch_harnesses();
    let original_session = client.pane(&pane)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let failed = client.request(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"broken-agent","idempotencyKey":"switch-failure"}),
    );
    assert_eq!(failed["error"]["data"]["kind"], "harness_switch_failed");
    let pane_after_failure = client.pane(&pane);
    assert_eq!(pane_after_failure["agent"]["harness"], "test-agent");
    assert_eq!(pane_after_failure["agent"]["sessionId"], original_session);

    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"original-still-live","idempotencyKey":"original-after-failure"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        if rig.native_session(&original_session)["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|turn| turn[0] == "original-still-live")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the original session did not serve a prompt after candidate failure"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(process_instances(&rig).len(), 1);
}

#[test]
fn continuation_build_failure_leaves_the_original_session_serving_prompts() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    rig.install_switch_harnesses();
    let original_session = client.pane(&pane)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"slow","idempotencyKey":"continuation-active"}),
    );
    client.wait_state(&pane, "working");
    let oversized = client.call(
        "pane.prompt",
        json!({
            "paneId":pane,"text":"x".repeat(20_000),
            "idempotencyKey":"continuation-too-large"
        }),
    );
    let failed = client.request(
        "pane.harness.switch",
        json!({
            "paneId":pane,"harness":"next-agent",
            "idempotencyKey":"switch-continuation-failure"
        }),
    );
    assert_eq!(failed["error"]["data"]["kind"], "harness_switch_failed");
    assert_eq!(client.pane(&pane)["agent"]["harness"], "test-agent");
    assert_eq!(client.pane(&pane)["agent"]["sessionId"], original_session);
    client.call(
        "pane.queue.remove",
        json!({
            "paneId":pane,"promptId":oversized["promptId"],
            "idempotencyKey":"remove-oversized-continuation"
        }),
    );
    client.call(
        "pane.prompt",
        json!({
            "paneId":pane,"text":"original-after-package-failure",
            "idempotencyKey":"prompt-after-package-failure"
        }),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        if rig.native_session(&original_session)["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|turn| turn[0] == "original-after-package-failure")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the original session did not serve a prompt after continuation failure"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(process_instances(&rig).len(), 1);
    drop(client);
    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    let transcript = restored.call("pane.transcript", json!({"paneId":pane}));
    assert!(transcript["items"].as_array().unwrap().iter().any(|message| {
        message["role"] == "system"
            && message["text"].as_str().is_some_and(|text| {
                text == "The switch to Next agent failed. The original agent session is still running."
            })
    }));
}

#[test]
fn old_process_close_failure_is_durable_and_retried_after_the_new_session_commits() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    rig.install_switch_harnesses();
    fs::write(rig.path.join("test-fail-harness-switch-old-close"), b"once").unwrap();
    let result = client.call(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"next-agent","idempotencyKey":"switch-close-retry"}),
    );
    assert_eq!(result["toHarness"], "next-agent");
    assert_eq!(client.pane(&pane)["agent"]["harness"], "next-agent");
    let new_session = result["newSessionId"].as_str().unwrap().to_owned();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"new-session-live","idempotencyKey":"new-after-close-failure"}),
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
            .is_some_and(|history| history.iter().any(|turn| turn[0] == "new-session-live"));
        if served {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the committed session did not serve a prompt after retired-process cleanup"
        );
        thread::sleep(Duration::from_millis(20));
    }
    drop(client);
    rig.kill_engine();
    let generation = rig.database_probe().engine_generation().unwrap();
    assert_eq!(
        process_instances_at(&rig, generation).len(),
        2,
        "the failed close must remain durably managed while the new session serves"
    );
    fs::remove_file(rig.path.join("test-fail-harness-switch-old-close")).unwrap();
    rig.start();
    let mut restored = rig.client();
    for _ in 0..20 {
        let _ = restored.pane(&pane);
        thread::sleep(Duration::from_millis(20));
    }
    drop(restored);
    rig.kill_engine();
    let generation = rig.database_probe().engine_generation().unwrap();
    assert_eq!(
        process_instances_at(&rig, generation).len(),
        1,
        "retired process cleanup did not converge"
    );
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
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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
