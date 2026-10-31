use super::*;

#[test]
fn no_output_is_visible_and_cancel_clears_durable_wait_before_switching() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let initial = client.pane(&pane);
    let pid = initial["pid"].clone();
    let session = initial["agent"]["sessionId"].as_str().unwrap().to_owned();
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
        json!({"paneId":pane,"text":"silent-no-output","idempotencyKey":"silent"}),
    );
    client.wait_state(&pane, "working");
    wait_for_silent_prompt_receipt(&rig, &pane, &session);
    let waiting = client.pane(&pane);
    assert_eq!(waiting["state"], "working");
    assert!(waiting["agent"]["silentSince"].is_string());
    rig.kill_engine();
    rig.age_agent(&pane, false);
    rig.start();
    let mut client = rig.client();
    let silent = client.wait_state(&pane, "no_signal");
    assert_eq!(silent["pid"], pid);
    assert_eq!(silent["agent"]["activePromptId"], prompt["promptId"]);
    assert_eq!(silent["agent"]["silentSince"], "1970-01-01T00:00:00.000Z");
    assert_eq!(client.section("attention").len(), 1);
    for _ in 0..10 {
        assert_eq!(client.pane(&pane)["state"], "no_signal");
        assert_eq!(client.section("attention").len(), 1);
        thread::sleep(Duration::from_millis(20));
    }
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
    assert_eq!(process_registry_instances(&rig).len(), 1);
    assert_eq!(
        rig.database_probe().agent_silence(&pane).unwrap(),
        (true, None)
    );
    assert!(
        client.call("pane.transcript", json!({"paneId":pane}))["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["text"] == "Turn cancelled by the user.")
    );
    rig.install_switch_harnesses();
    let switched = client.call(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"next-agent","idempotencyKey":"switch-after-visible-cancel"}),
    );
    assert_eq!(switched["fromHarness"], "test-agent");
    assert_eq!(switched["toHarness"], "next-agent");
    assert_eq!(client.pane(&pane)["agent"]["harness"], "next-agent");
    rig.kill_engine();
    assert_eq!(process_instances(&rig).len(), 1);
    rig.start();
    let mut client = rig.client();
    assert!(client.pane(&pane)["agent"]["silentSince"].is_null());
    assert!(
        client.call("pane.transcript", json!({"paneId":pane}))["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["text"] == "Turn cancelled by the user.")
    );
}

#[test]
fn unconfirmed_cancel_stops_the_process_and_returns_owned_work() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let workspace = client.pane(&pane)["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let created = client.call(
        "task.create",
        json!({"workspaceId":workspace,"title":"Return work after lost process",
            "mustNotChange":"Keep recovery atomic","idempotencyKey":"cancel-task"}),
    );
    let task = reserve_task(
        &rig,
        &workspace,
        &pane,
        created["task"]["taskId"].as_str().unwrap(),
    );
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
fn a_turn_that_emits_then_pauses_and_resumes_never_becomes_a_no_output_warning() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"quiet-then-resume","idempotencyKey":"stream-started"}),
    );
    let session = client.pane(&pane)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let thought_arrived = client.call("pane.transcript", json!({"paneId":pane}))["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["kind"] == "thought");
        let outbox_empty = rig.database_probe().agent_outbox_empty(&pane).unwrap();
        let harness_received = rig
            .path
            .join("native")
            .join(format!("quiet-then-resume-{session}.started"))
            .exists();
        if thought_arrived && outbox_empty && harness_received {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "healthy quiet turn lacked a real delivery receipt"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !rig.path
            .join("native")
            .join(format!("silent-no-output-{session}.started"))
            .exists()
    );
    rig.kill_engine();
    rig.age_agent(&pane, false);
    rig.start();
    let mut client = rig.client();
    for _ in 0..20 {
        let current = client.pane(&pane);
        assert_eq!(current["state"], "working");
        assert!(current["agent"]["silentSince"].is_null());
        thread::sleep(Duration::from_millis(20));
    }
    assert!(client.section("attention").is_empty());
    fs::write(
        rig.path
            .join("native")
            .join(format!("quiet-then-resume-{session}.release")),
        b"resume",
    )
    .unwrap();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let transcript = client.call("pane.transcript", json!({"paneId":pane}));
        let resumed = transcript["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| {
                message["role"] == "assistant"
                    && message["text"] == "Resumed after the quiet interval"
            });
        if resumed && client.pane(&pane)["state"] == "idle" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "healthy turn did not resume after its quiet interval"
        );
        assert!(client.section("attention").is_empty());
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn silent_prompt_start_is_durable_and_scoped_per_pane() {
    let mut rig = Rig::new();
    let (mut client, first) = rig.agent();
    let workspace = client.pane(&first)["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, second) = rig.agent_in_workspace(&workspace);
    let first_session = client.pane(&first)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let second_session = client.pane(&second)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    for (pane, key) in [(&first, "silent-first"), (&second, "silent-second")] {
        client.call(
            "pane.prompt",
            json!({"paneId":pane,"text":"silent-no-output","idempotencyKey":key}),
        );
    }
    for (pane, session) in [(&first, &first_session), (&second, &second_session)] {
        wait_for_silent_prompt_receipt(&rig, pane, session);
    }
    rig.kill_engine();
    rig.set_agent_silent_since(&first, 0);
    rig.set_agent_silent_since(&second, 1_000);
    rig.start();
    let mut client = rig.client();
    let first_view = client.wait_state(&first, "no_signal");
    let second_view = client.wait_state(&second, "no_signal");
    assert_eq!(
        first_view["agent"]["silentSince"],
        "1970-01-01T00:00:00.000Z"
    );
    assert_eq!(
        second_view["agent"]["silentSince"],
        "1970-01-01T00:00:01.000Z"
    );
    client.call(
        "pane.cancel",
        json!({"paneId":first,"idempotencyKey":"cancel-first-silent"}),
    );
    client.wait_state(&first, "idle");
    assert_eq!(client.pane(&second)["state"], "no_signal");
    assert!(client.pane(&second)["agent"]["activePromptId"].is_string());
    client.call(
        "pane.cancel",
        json!({"paneId":second,"idempotencyKey":"cancel-second-silent"}),
    );
    client.wait_state(&second, "idle");
    assert_eq!(process_instances(&rig).len(), 2);
}

fn wait_for_silent_prompt_receipt(rig: &Rig, pane: &str, session: &str) {
    let marker = rig
        .path
        .join("native")
        .join(format!("silent-no-output-{session}.started"));
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let bytes_reached_process = rig.database_probe().agent_outbox_empty(pane).unwrap();
        let harness_received_prompt = marker.exists();
        if bytes_reached_process && harness_received_prompt {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "prompt has no real delivery receipt: outbox_empty={bytes_reached_process}, harness_marker={harness_received_prompt}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn terminal_sign_in_uses_a_separate_pane_then_retries_the_agent() {
    let rig = Rig::new();
    let binary = sibling_binary("soyeht-fake-acp");
    fs::write(rig.path.join("harnesses.json"),serde_json::to_vec(&json!([{"id":"auth-test","name":"Auth test","command":binary,"args":["--state-dir",rig.path.join("native"),"--require-login"],"version":"test"}])).unwrap()).unwrap();
    let mut client = rig.client();
    let registry = client.call("harness.list", json!({}));
    assert_eq!(registry["items"][0]["id"], "auth-test");
    assert_eq!(registry["items"][0]["available"], true);
    assert_eq!(
        client.call("model.list", json!({"harness":"auth-test"}))["source"],
        "not_probed"
    );
    let workspace = create_workspace(&mut client, "Auth test", &rig.path, "auth-workspace");
    let pane = client.call("pane.create",json!({"workspaceId":workspace,"mode":"agent","harness":"auth-test","idempotencyKey":"auth-pane"}))["paneId"].as_str().unwrap().to_owned();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while client.pane(&pane)["phrase"] != "Sign-in required" {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    let signed_out = client.call("harness.list", json!({}));
    assert_eq!(signed_out["items"][0]["setupState"], "sign_in");
    assert_eq!(
        signed_out["items"][0]["setupReason"],
        "authentication_required"
    );
    let reconnect = rig.client().pane(&pane);
    assert_eq!(reconnect["agent"]["authMethods"][0]["id"], "login");
    assert_eq!(reconnect["agent"]["authMethods"][0]["terminal"], true);
    let mut observer = rig.client();
    let (snapshot, _) = observer.snapshot();
    observer.call("events.subscribe", json!({"since":snapshot["cursor"]}));
    observer.messages.clear();
    let params = json!({"paneId":pane,"authMethodId":"login","idempotencyKey":"auth-start"});
    let start = client.call("pane.auth.start", params.clone());
    assert_eq!(start, client.call("pane.auth.start", params));
    let terminal = start["terminalPaneId"].as_str().unwrap();
    assert_ne!(terminal, pane);
    assert_eq!(client.pane(terminal)["mode"], "terminal");
    assert_eq!(client.pane(terminal)["record"], false);
    let auth_launch: Spawn = Store::open(&rig.path)
        .unwrap()
        .pane_launch(terminal)
        .unwrap()
        .unwrap();
    let expected_auth_argv = vec![
        binary.to_string_lossy().into_owned(),
        "--state-dir".into(),
        rig.path.join("native").to_string_lossy().into_owned(),
        "--login".into(),
    ];
    assert_eq!(auth_launch.argv, expected_auth_argv);
    assert!(
        !auth_launch
            .argv
            .iter()
            .any(|value| value == "--require-login")
    );
    wait_until("sign-in did not unlock session creation", || {
        client.pane(&pane)["agent"]["sessionId"].is_string()
    });
    assert!(rig.path.join("native/login.ok").is_file());
    let finished = wait_for_pane_event(&mut observer, "pane.auth_finished", &pane);
    assert_eq!(finished["ok"], true, "{finished}");
    assert!(
        client
            .section("panes")
            .iter()
            .all(|item| item["paneId"] != terminal),
        "confirmed sign-in must close its temporary terminal with the success event"
    );
    let signed_in = client.call("harness.list", json!({}));
    assert_eq!(signed_in["items"][0]["setupState"], "ready");
    assert!(signed_in["items"][0]["lastReadyAtMs"].is_string());
    let resumed = client.pane(&pane);
    assert_eq!(resumed["paneId"], pane);
    assert_eq!(resumed["state"], "idle");
    assert_eq!(resumed["agent"]["authMethods"], json!([]));
    let models = client.call("model.list", json!({"harness":"auth-test","paneId":pane}));
    assert_eq!(models["source"], "native_session");
    assert_eq!(models["items"].as_array().unwrap().len(), 5);
    let _: soyeht_protocol::ModelListResult = serde_json::from_value(models).unwrap();
    // A signed-in pane offers the same sign-in as an account switch.
    assert_eq!(resumed["agent"]["reauthMethods"][0]["id"], "login");
}

#[derive(Clone, Copy, Debug)]
enum SignInEnding {
    Failed,
    Interrupted,
    ExitedWithoutSigningIn,
}

fn wait_for_pane_event(observer: &mut Client, method: &str, pane: &str) -> Value {
    observer.wait_for_notification(method, |params| params["paneId"] == pane)
}

fn sign_in_output_is_readable(client: &mut Client, terminal: &str) -> bool {
    let (_, frames) = client.tail(terminal);
    let marker = b"Test sign-in waiting.";
    frames
        .iter()
        .filter_map(|frame| match frame {
            Frame::Terminal { bytes, .. } => Some(bytes.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect::<Vec<u8>>()
        .windows(marker.len())
        .any(|window| window == marker)
}

#[test]
fn sign_in_that_is_not_confirmed_keeps_its_terminal_and_the_agent_signed_out() {
    for ending in [
        SignInEnding::Failed,
        SignInEnding::Interrupted,
        SignInEnding::ExitedWithoutSigningIn,
    ] {
        let rig = Rig::new();
        let binary = sibling_binary("soyeht-fake-acp");
        fs::write(
            rig.path.join("harnesses.json"),
            serde_json::to_vec(&json!([{
                "id":"auth-test","name":"Auth test","command":binary,
                "args":["--state-dir",rig.path.join("native"),"--require-login"],
                "version":"test"
            }]))
            .unwrap(),
        )
        .unwrap();
        let (fixture, reason) = match ending {
            SignInEnding::Failed => ("login.fail", "Sign-in failed or was interrupted"),
            SignInEnding::Interrupted => ("login.wait", "Sign-in failed or was interrupted"),
            SignInEnding::ExitedWithoutSigningIn => ("login.cancel", "Sign-in did not complete"),
        };
        fs::create_dir_all(rig.path.join("native")).unwrap();
        fs::write(rig.path.join("native").join(fixture), b"fixture").unwrap();
        let mut client = rig.client();
        let workspace = create_workspace(
            &mut client,
            "Sign-in ending",
            &rig.path,
            "unconfirmed-auth-workspace",
        );
        let pane = client.call(
            "pane.create",
            json!({"workspaceId":workspace,"mode":"agent","harness":"auth-test",
                "idempotencyKey":"unconfirmed-auth-pane"}),
        )["paneId"]
            .as_str()
            .unwrap()
            .to_owned();
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        while client.pane(&pane)["phrase"] != "Sign-in required" {
            assert!(Instant::now() < deadline, "agent did not request sign-in");
            thread::sleep(Duration::from_millis(20));
        }
        let mut observer = rig.client();
        let (snapshot, _) = observer.snapshot();
        observer.call("events.subscribe", json!({"since":snapshot["cursor"]}));
        observer.messages.clear();
        let terminal = client.call(
            "pane.auth.start",
            json!({"paneId":pane,"authMethodId":"login",
                "idempotencyKey":"unconfirmed-auth-start"}),
        )["terminalPaneId"]
            .as_str()
            .unwrap()
            .to_owned();
        while !sign_in_output_is_readable(&mut client, &terminal) {
            assert!(
                Instant::now() < deadline,
                "{ending:?}: login fixture did not start"
            );
            thread::sleep(Duration::from_millis(20));
        }
        if matches!(ending, SignInEnding::Interrupted) {
            client.call(
                "pane.input",
                json!({"paneId":terminal,"bytes":STANDARD.encode([0x03]),
                    "idempotencyKey":"interrupt-auth"}),
            );
        } else {
            fs::write(rig.path.join("native/login.release"), b"release").unwrap();
        }

        let finished = wait_for_pane_event(&mut observer, "pane.auth_finished", &pane);
        assert_eq!(finished["ok"], false, "{ending:?}: {finished}");
        assert!(
            finished["reason"].as_str().unwrap().starts_with(reason),
            "{ending:?}: {finished}"
        );
        let kept = client.pane(&terminal);
        assert_eq!(kept["mode"], "terminal", "{ending:?}");
        assert_eq!(kept["state"], "dead", "{ending:?}");
        assert!(
            sign_in_output_is_readable(&mut client, &terminal),
            "{ending:?}: sign-in output is not readable"
        );
        assert!(!rig.path.join("native/login.ok").exists());
        let signed_out = client.pane(&pane);
        assert!(signed_out["agent"]["sessionId"].is_null(), "{ending:?}");
        assert_eq!(signed_out["agent"]["authMethods"][0]["id"], "login");
        assert_eq!(
            client.call("harness.list", json!({}))["items"][0]["setupState"],
            "sign_in",
            "{ending:?}"
        );
        // A second attempt opens a fresh terminal beside the kept one.
        let retry = client.call(
            "pane.auth.start",
            json!({"paneId":pane,"authMethodId":"login",
                "idempotencyKey":"unconfirmed-auth-retry"}),
        );
        assert_ne!(retry["terminalPaneId"], terminal.as_str(), "{ending:?}");
        assert_eq!(client.pane(&terminal)["state"], "dead", "{ending:?}");
    }
}

#[test]
fn prompt_auth_required_downgrades_a_previously_ready_harness_without_guessing_from_text() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let ready = client.call("harness.list", json!({}));
    assert_eq!(ready["items"][0]["setupState"], "ready");
    let last_ready = ready["items"][0]["lastReadyAtMs"].clone();

    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"auth-expired","idempotencyKey":"expired-auth"}),
    );
    wait_until("auth_required did not change the pane phase", || {
        client.pane(&pane)["phrase"] == "Sign-in required"
    });
    let signed_out = client.call("harness.list", json!({}));
    assert_eq!(signed_out["items"][0]["setupState"], "sign_in");
    assert_eq!(
        signed_out["items"][0]["setupReason"],
        "authentication_required"
    );
    assert_eq!(signed_out["items"][0]["lastReadyAtMs"], last_ready);
    assert!(
        !client.pane(&pane)["agent"]["authMethods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    drop(client);
    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    assert_eq!(
        restored.call("harness.list", json!({}))["items"][0]["setupState"],
        "sign_in"
    );
    assert!(
        !restored.pane(&pane)["agent"]["authMethods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn acp_message_ids_keep_distinct_assistant_blocks_separate() {
    fn assistant_texts(client: &mut Client, pane: &str) -> Vec<String> {
        let transcript = client.call("pane.transcript", json!({"paneId":pane}));
        transcript["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["role"] == "assistant" && item["kind"] == "message")
            .map(|item| item["text"].as_str().unwrap().to_owned())
            .collect()
    }
    let expected = [
        "Failed to authenticate: OAuth session expired",
        "No response requested.",
    ];
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"separate-message-ids","idempotencyKey":"distinct-message-ids"}),
    );
    wait_until("agent turn did not finish", || {
        client.pane(&pane)["phrase"] == "Agent turn ended"
    });
    assert_eq!(assistant_texts(&mut client, &pane), expected);
    drop(client);
    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    assert_eq!(assistant_texts(&mut restored, &pane), expected);
}

#[test]
fn typed_auth_error_already_streamed_by_adapter_appears_once() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"auth-expired-with-chunk","idempotencyKey":"auth-streamed-once"}),
    );
    wait_until("auth error did not reach the pane", || {
        client.pane(&pane)["phrase"] == "Sign-in required"
    });
    let transcript = client.call("pane.transcript", json!({"paneId":pane}));
    let errors = transcript["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains("Failed to authenticate"))
        })
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["role"], "assistant");
    assert_eq!(
        client.call("harness.list", json!({}))["items"][0]["setupState"],
        "sign_in"
    );
}

#[test]
fn untyped_internal_error_with_auth_words_does_not_downgrade_ready() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    assert_eq!(
        client.call("harness.list", json!({}))["items"][0]["setupState"],
        "ready"
    );

    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"auth-expired-untagged","idempotencyKey":"untyped-auth"}),
    );
    wait_until("untagged error did not end the turn", || {
        client.pane(&pane)["phrase"] == "Agent turn ended"
    });
    assert_eq!(
        client.call("harness.list", json!({}))["items"][0]["setupState"],
        "ready"
    );
}

#[test]
fn preferred_harness_setting_is_durable_and_rejects_non_string_values() {
    let mut rig = Rig::new();
    let mut client = rig.client();
    let key = soyeht_protocol::setting::HARNESS_PREFERRED_ID;
    assert_eq!(client.call("settings.get", json!({"key":key}))["value"], "");
    let set = client.call(
        "settings.set",
        json!({"key":key,"value":"test-agent","idempotencyKey":"preferred-test-agent"}),
    );
    assert_eq!(set["value"], "test-agent");
    assert_eq!(
        client.request(
            "settings.set",
            json!({"key":key,"value":7,"idempotencyKey":"invalid-preference"})
        )["error"]["data"]["kind"],
        "invalid_params"
    );
    drop(client);
    rig.kill_engine();
    rig.start();
    assert_eq!(
        rig.client().call("settings.get", json!({"key":key}))["value"],
        "test-agent"
    );
}

#[test]
fn empty_profile_lists_the_pinned_catalogue_and_accepts_an_explicit_rescan() {
    let rig = Rig::new();
    assert!(!rig.path.join("harnesses.json").exists());
    let mut client = rig.client();
    let initial = client.call("harness.list", json!({"limit":64}));
    let unverified = initial["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "registry.amp-acp")
        .unwrap();
    assert_eq!(
        unverified["setupState"],
        if unverified["available"] == true {
            "not_checked"
        } else {
            "not_installed"
        }
    );
    assert_eq!(
        initial["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "registry.amp-acp")
            .unwrap()["installed"],
        false
    );

    rig.install_stub_executable("amp-acp");

    let registry = client.call("harness.list", json!({"limit":64,"rescan":true}));
    let items = registry["items"].as_array().unwrap();
    assert_eq!(items.len(), 41);
    for id in [
        "registry.claude-acp",
        "registry.codex-acp",
        "registry.cursor",
        "registry.opencode",
        "registry.gemini",
    ] {
        let item = items.iter().find(|item| item["id"] == id).unwrap();
        assert_eq!(item["source"], "built_in");
        assert!(matches!(
            item["setupState"].as_str(),
            Some("ready" | "not_checked" | "not_installed" | "check_setup")
        ));
    }
    let installed = items
        .iter()
        .find(|item| item["id"] == "registry.amp-acp")
        .unwrap();
    assert_eq!(installed["available"], true);
    assert_eq!(installed["installed"], true);
    assert_eq!(installed["setupState"], "not_checked");
    assert!(registry["nextCursor"].is_null());
}

#[test]
fn a_cancel_the_agent_dropped_is_sent_again_until_the_turn_ends() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let marker = rig.path.join("native/drop-first-cancel");
    fs::write(&marker, b"drop").unwrap();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"silent","idempotencyKey":"cancel-dropped"}),
    );
    client.wait_state(&pane, "working");
    client.call(
        "pane.cancel",
        json!({"paneId":pane,"idempotencyKey":"cancel-once"}),
    );
    client.wait_state(&pane, "idle");
    assert!(!marker.exists(), "the first cancel was not dropped");
    assert!(
        client.call("pane.transcript", json!({"paneId":pane}))["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["text"] == "Turn cancelled by the user.")
    );
}
