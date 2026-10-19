use super::*;

#[test]
fn custom_harness_management_is_atomic_idempotent_and_protects_bound_panes() {
    let mut rig = Rig::new();
    let binary = sibling_binary("soyeht-fake-acp");
    let mut client = rig.client();
    let launch = json!({
        "id":"custom-managed",
        "name":"Managed agent",
        "command":binary,
        "args":["--state-dir",rig.path.join("managed-native")],
        "version":"1"
    });
    let create = json!({"harness":launch,"idempotencyKey":"custom-create"});
    let created = client.call("harness.custom.create", create.clone());
    assert_eq!(created, client.call("harness.custom.create", create));
    let conflicting = client.request(
        "harness.custom.create",
        json!({"harness":{
            "id":"custom-conflict","name":"Conflicting agent","command":binary,
            "args":["--state-dir",rig.path.join("conflicting-native")],"version":"1"
        },"idempotencyKey":"custom-create"}),
    );
    assert_eq!(conflicting["error"]["data"]["kind"], "idempotency_conflict");
    assert_eq!(
        client.call("harness.custom.list", json!({}))["items"][0]["id"],
        "custom-managed"
    );
    assert_eq!(
        client.call("harness.custom.list", json!({}))["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let workspace = client.call(
        "workspace.create",
        json!({"name":"Custom harness","path":rig.path,"idempotencyKey":"custom-workspace"}),
    )["workspace"]["workspaceId"]
        .clone();
    let pane = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"agent","harness":"custom-managed","idempotencyKey":"custom-pane"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let update = client.request(
        "harness.custom.update",
        json!({"harness":{
            "id":"custom-managed","name":"Renamed agent","command":binary,
            "args":["--state-dir",rig.path.join("managed-native")],"version":"1"
        },"idempotencyKey":"custom-update-bound"}),
    );
    assert_eq!(update["error"]["data"]["kind"], "pane_busy");
    let remove = client.request(
        "harness.custom.remove",
        json!({"harnessId":"custom-managed","idempotencyKey":"custom-remove-bound"}),
    );
    assert_eq!(remove["error"]["data"]["kind"], "pane_busy");

    client.call(
        "pane.close",
        json!({"paneId":pane,"idempotencyKey":"close-custom-pane"}),
    );
    let updated = client.call(
        "harness.custom.update",
        json!({"harness":{
            "id":"custom-managed","name":"Renamed agent","command":binary,
            "args":["--state-dir",rig.path.join("managed-native")],"version":"1"
        },"idempotencyKey":"custom-update"}),
    );
    assert_eq!(updated["harness"]["name"], "Renamed agent");
    let removed = client.call(
        "harness.custom.remove",
        json!({"harnessId":"custom-managed","idempotencyKey":"custom-remove"}),
    );
    assert_eq!(removed["removed"], true);
    assert!(
        client.call("harness.custom.list", json!({}))["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    rig.kill_engine();
    rig.start();
    assert!(
        rig.client().call("harness.custom.list", json!({}))["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

/// What a pane's bound MCP connection may do: the agent role only, and no wider than a device.
fn assert_bound_agent_is_limited(path: &std::path::Path, binding: &str) {
    let (mut bound_socket, welcome) = binding_hello(&path.join("engine.sock"), binding);
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
    // Device parity must not widen agent powers.
    for (id, method, params) in [
        (
            3,
            "pane.input",
            json!({"paneId":"any","bytes":STANDARD.encode(b"x"),"idempotencyKey":"agent-input"}),
        ),
        (
            4,
            "settings.set",
            json!({"key":soyeht_protocol::setting::HARNESS_PREFERRED_ID,"value":"agent","idempotencyKey":"agent-setting"}),
        ),
        (
            5,
            "device.pair.start",
            json!({"idempotencyKey":"agent-pair"}),
        ),
    ] {
        assert_eq!(
            binding_request(&mut bound_socket, id, method, params)["error"]["data"]["kind"],
            "forbidden_role",
            "agent {method}"
        );
    }
    let (_, unknown) = binding_hello(&path.join("engine.sock"), "unknown-binding");
    assert_eq!(unknown["error"]["data"]["kind"], "unauthorized");
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
        sibling_binary("soyeht-mcp").to_string_lossy().as_ref()
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

    assert_bound_agent_is_limited(&rig.path, &binding);

    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"hello","idempotencyKey":"mcp-restart-turn"}),
    );
    let before = client.wait_state(&pane, "idle");
    rig.ptyd.kill().unwrap();
    rig.ptyd.wait().unwrap();
    rig.ptyd = Command::new(sibling_binary("soyeht-ptyd"))
        .arg("--data-dir")
        .arg(&rig.path)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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
fn mcp_connections_outside_the_pane_process_tree_keep_each_pane_identity() {
    let rig = Rig::new();
    let (mut human, first) = rig.agent();
    let workspace = human.pane(&first)["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, second) = rig.agent_in_workspace(&workspace);

    // Both connections originate in this test process, outside either pane's
    // process tree, so no process ancestry or TTY lookup can identify them.
    let first_binding = binding_env(&mcp_server_for(&rig, &mut human, &first));
    let second_binding = binding_env(&mcp_server_for(&rig, &mut human, &second));
    assert_ne!(first_binding, second_binding);

    let (mut first_socket, first_hello) =
        binding_hello(&rig.path.join("engine.sock"), &first_binding);
    let (mut second_socket, second_hello) =
        binding_hello(&rig.path.join("engine.sock"), &second_binding);
    assert_eq!(first_hello["result"]["role"], "agent");
    assert_eq!(second_hello["result"]["role"], "agent");

    let first_peers = binding_request(&mut first_socket, 2, "agent.list", json!({}));
    let second_peers = binding_request(&mut second_socket, 2, "agent.list", json!({}));
    assert_eq!(first_peers["result"]["items"][0]["paneId"], second);
    assert_eq!(second_peers["result"]["items"][0]["paneId"], first);

    // A binding stops authenticating once its pane is closed.
    human.call(
        "pane.close",
        json!({"paneId":first,"idempotencyKey":"close-first-bound-pane"}),
    );
    let (_, closed) = binding_hello(&rig.path.join("engine.sock"), &first_binding);
    assert_eq!(closed["error"]["data"]["kind"], "unauthorized");
    let (_, still_open) = binding_hello(&rig.path.join("engine.sock"), &second_binding);
    assert_eq!(still_open["result"]["role"], "agent");
}

#[test]
fn mcp_process_launched_outside_the_pane_resolves_identity_from_its_binding() {
    let rig = Rig::new();
    let (mut human, first) = rig.agent();
    let workspace = human.pane(&first)["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, second) = rig.agent_in_workspace(&workspace);
    let server = mcp_server_for(&rig, &mut human, &first);

    // The test runner launches MCP directly, outside either pane's TTY
    // process tree, with the declared args and env.
    let replies = run_mcp(
        &server,
        &first,
        concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"list_agents\",\"arguments\":{}}}\n"
        ),
    );
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "Soyeht");
    assert_eq!(replies[1]["result"]["isError"], false);
    assert_eq!(
        replies[1]["result"]["structuredContent"]["items"][0]["paneId"],
        second
    );

    // Pane A's binding launched with pane B's --pane-id still acts as A: it
    // sees B as its peer and cannot configure B's loop.
    let crossed = run_mcp(
        &server,
        &second,
        concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"list_agents\",\"arguments\":{}}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"loop_set\",\"arguments\":{\"prompt\":\"impersonate\",\"interval\":60,\"maxRounds\":1,\"stopWhen\":\"round_limit\",\"enabled\":false}}}\n"
        ),
    );
    assert_eq!(crossed[1]["result"]["isError"], false);
    assert_eq!(
        crossed[1]["result"]["structuredContent"]["items"][0]["paneId"],
        second
    );
    assert_eq!(crossed[2]["result"]["isError"], true);
    assert_eq!(
        crossed[2]["result"]["structuredContent"]["kind"],
        "forbidden_role"
    );
}

fn mcp_server_for(rig: &Rig, human: &mut Client, pane: &str) -> Value {
    let session_id = human.pane(pane)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    rig.native_session(&session_id)["mcp_servers"][0].clone()
}

fn binding_env(server: &Value) -> String {
    server["env"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "SOYEHT_MCP_BINDING")
        .unwrap()["value"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Runs the declared MCP server with its env, overriding `--pane-id`, and
/// returns one parsed reply per request line.
fn run_mcp(server: &Value, pane_id: &str, requests: &str) -> Vec<Value> {
    let mut args: Vec<String> = server["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect();
    let pane_arg = args.iter().position(|value| value == "--pane-id").unwrap() + 1;
    args[pane_arg] = pane_id.to_owned();
    let mut command = Command::new(server["command"].as_str().unwrap());
    command.args(&args);
    for entry in server["env"].as_array().unwrap() {
        command.env(
            entry["name"].as_str().unwrap(),
            entry["value"].as_str().unwrap(),
        );
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(requests.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "soyeht-mcp failed: {stderr}");
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        replies.len(),
        requests.lines().count(),
        "missing replies; stderr: {stderr}"
    );
    replies
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

#[test]
fn persistent_per_pane_poll_failure_does_not_starve_the_next_agent() {
    let rig = Rig::new();
    let (mut client, first) = rig.agent();
    let workspace = client.pane(&first)["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, second) = rig.agent_in_workspace(&workspace);
    let (faulted, survivor) = if first < second {
        (&first, &second)
    } else {
        (&second, &first)
    };
    let survivor_session = client.pane(survivor)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let marker = rig.path.join("test-fail-agent-poll");
    fs::write(&marker, faulted).unwrap();
    client.call(
        "pane.prompt",
        json!({"paneId":survivor,"text":"later-pane-still-serves","idempotencyKey":"poll-isolation"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let served = rig.native_session(&survivor_session)["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|turn| turn[0] == "later-pane-still-serves");
        if served && client.pane(faulted)["state"] == "dead" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "a persistent earlier-pane failure starved the later pane"
        );
        thread::sleep(Duration::from_millis(20));
    }
    fs::remove_file(marker).unwrap();
    assert_eq!(process_instances(&rig).len(), 1);
    assert!(
        client.call("pane.transcript", json!({"paneId":faulted}))["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["text"] == "Injected per-pane polling failure")
    );
}
