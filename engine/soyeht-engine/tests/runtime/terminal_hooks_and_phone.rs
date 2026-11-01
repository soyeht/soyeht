use super::*;

#[cfg(unix)]
fn open_claude_hook_session(rig: &mut Rig, client: &mut Client) -> (String, String, u64) {
    let workspace = create_workspace(client, "Hook labels", &rig.path, "hook-label-workspace");
    let pane = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"agent","harness":"registry.claude-acp",
            "idempotencyKey":"hook-label-pane"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    let (session_id, generation) = loop {
        let view = client.pane(&pane);
        if let (Some(session_id), Some(generation)) = (
            view["agent"]["sessionId"].as_str(),
            view["generation"].as_u64(),
        ) {
            break (session_id.to_owned(), generation);
        }
        assert!(
            Instant::now() < deadline,
            "Claude test session did not open"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let native: Value = serde_json::from_slice(
        &fs::read(
            rig.path
                .join("claude-native")
                .join(format!("{session_id}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    let handler = &native["session_meta"]["claudeCode"]["options"]["settings"]["hooks"]["PermissionRequest"]
        [0]["hooks"][0];
    let capability = handler["args"][3]
        .as_str()
        .unwrap_or_else(|| panic!("Claude session metadata lacked the hook handler: {native}"))
        .to_owned();
    assert_eq!(handler["type"], "command");
    assert_eq!(handler["args"][2], "__experiment-hook");
    (pane, capability, generation)
}

#[cfg(unix)]
fn run_claude_hook_command(rig: &Rig, capability: &str) {
    let binary = sibling_binary("soyeht");
    let mut command = Command::new(binary)
        .arg("--data-dir")
        .arg(&rig.path)
        .arg("__experiment-hook")
        .arg(capability)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    command
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"hook_event_name":"PermissionRequest"}"#)
        .unwrap();
    let output = command.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "hook command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({})
    );
}

#[cfg(unix)]
fn wait_for_terminal_text(client: &mut Client, pane: &str, expected: &str) {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let (_, frames) = client.tail(pane);
        let bytes = frames
            .into_iter()
            .filter_map(|frame| match frame {
                Frame::Terminal { bytes, .. } => Some(bytes),
                _ => None,
            })
            .flatten()
            .collect::<Vec<_>>();
        if String::from_utf8_lossy(&bytes).contains(expected) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "terminal output did not reach the real stream"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn managed_terminal_hook_and_screen_observation_share_one_recording() {
    let mut rig = Rig::new();
    let terminal = rig.path.join("managed-terminal");
    write_script(
        &terminal,
        "#!/bin/sh\nprintf 'terminal-ready\\n'\nwhile IFS= read -r line; do printf 'screen:%s\\n' \"$line\"; done\n",
    );
    let mut client = rig.client();
    let workspace = create_workspace(
        &mut client,
        "Terminal bridge",
        &rig.path,
        "terminal-bridge-workspace",
    );
    let pane = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"terminal",
            "idempotencyKey":"terminal-bridge-pane"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    wait_until("initial terminal did not open", || {
        !client.pane(&pane)["pid"].is_null()
    });

    drop(client);
    rig.kill_engine();
    let capability = "c".repeat(64);
    let generation = rig.install_terminal_harness_session(&pane, &terminal, &capability);
    rig.start();
    let mut client = rig.client();
    wait_until("managed terminal did not open", || {
        !client.pane(&pane)["pid"].is_null()
    });
    client.call(
        "pane.input",
        json!({"paneId":pane,"bytes":STANDARD.encode(b"bridge-frame\n"),
            "idempotencyKey":"terminal-bridge-input"}),
    );
    wait_for_terminal_text(&mut client, &pane, "screen:bridge-frame");
    run_claude_hook_command(&rig, &capability);

    let root = rig.path.join("experiments/pane-state");
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let rows = experiment_rows(&root);
        let observation = rows.iter().find(|row| {
            row["kind"] == "observation"
                && row["pane_id"].as_str() == Some(&pane)
                && row["pane_kind"] == "terminal"
                && row["harness"]["id"] == "registry.claude-acp"
                && row["session_generation"].as_u64() == Some(u64::from(generation))
        });
        let label = rows.iter().find(|row| {
            row["kind"] == "label"
                && row["pane_id"].as_str() == Some(&pane)
                && row["pane_kind"] == "terminal"
                && row["harness"]["id"] == "registry.claude-acp"
                && row["state"] == "waiting_for_you"
                && row["provenance"]["sessionGeneration"].as_u64() == Some(u64::from(generation))
        });
        if let (Some(observation), Some(label)) = (observation, label) {
            assert!(observation["sequence"].as_u64() < label["sequence"].as_u64());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "terminal bridge rows did not land: {rows:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
fn wait_for_stamped_hook_label(rig: &Rig, pane: &str, generation: u64, capability: &str) {
    let root = rig.path.join("experiments/pane-state");
    let expected_digest = format!("{:x}", Sha256::digest(capability.as_bytes()));
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let rows = experiment_rows(&root);
        if rows.iter().any(|row| {
            row["kind"] == "label"
                && row["pane_id"] == pane
                && row["state"] == "waiting_for_you"
                && row["source"] == "hook"
                && row["provenance"]["channel"] == "session_hook_local_rpc"
                && row["provenance"]["sessionGeneration"] == generation
                && row["provenance"]["credentialSha256"] == expected_digest
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Hook label did not reach the dataset"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn claude_session_hook_records_only_engine_stamped_labels_on_its_private_channel() {
    let mut rig = Rig::new();
    rig.install_fake_claude_runner();
    let mut client = rig.client();
    let (pane, capability, generation) = open_claude_hook_session(&mut rig, &mut client);

    let ordinary = client.request(
        "pane.experiment.hook",
        json!({"event":"Stop","capability":capability,"idempotencyKey":"ordinary-hook"}),
    );
    assert_eq!(ordinary["error"]["data"]["kind"], "forbidden_role");
    let mut hook = rig.hook_client();
    let claimed_source = hook.request(
        "pane.experiment.hook",
        json!({"event":"Stop","capability":capability,"idempotencyKey":"claimed-hook","source":"hook"}),
    );
    assert_eq!(claimed_source["error"]["data"]["kind"], "invalid_params");
    let wrong_capability = hook.request(
        "pane.experiment.hook",
        json!({"event":"Stop","capability":"x".repeat(capability.len()),"idempotencyKey":"wrong-hook"}),
    );
    assert_eq!(wrong_capability["error"]["data"]["kind"], "unauthorized");
    let capture_off =
        json!({"event":"Stop","capability":capability,"idempotencyKey":"capture-off-hook"});
    let first = hook.call("pane.experiment.hook", capture_off.clone());
    assert_eq!(first["recorded"], false);
    assert_eq!(hook.call("pane.experiment.hook", capture_off), first);
    assert!(!rig.path.join("experiments/pane-state").exists());

    drop(hook);
    drop(client);
    rig.kill_engine();
    rig.database_probe()
        .set_experiment_capture(&pane, true)
        .unwrap();
    rig.start();
    let mut client = rig.client();
    wait_until("Claude test session did not recover", || {
        client.pane(&pane)["agent"]["sessionId"].is_string()
    });
    run_claude_hook_command(&rig, &capability);
    wait_for_stamped_hook_label(&rig, &pane, generation, &capability);

    client.call(
        "pane.close",
        json!({"paneId":pane,"idempotencyKey":"close-hook-label-pane"}),
    );
    let mut hook = rig.hook_client();
    let stale = hook.request(
        "pane.experiment.hook",
        json!({"event":"Stop","capability":capability,"idempotencyKey":"stale-hook"}),
    );
    assert_eq!(stale["error"]["data"]["kind"], "unauthorized");
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
            .set_read_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
            .unwrap();
        stream
            .set_write_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
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
        self.hello_with(key, token, Some([5; 32]))
    }
    /// The engine's nonce of this connection.
    fn nonce(&self) -> [u8; 32] {
        STANDARD
            .decode(self.challenge["nonce"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap()
    }
    /// A `hello` that carries `client_nonce` (a phone that verifies the engine always does).
    fn hello_with(
        &mut self,
        key: &ed25519_dalek::SigningKey,
        token: &str,
        client_nonce: Option<[u8; 32]>,
    ) -> Value {
        use ed25519_dalek::Signer;
        let nonce = self.nonce();
        let proof = STANDARD.encode(
            key.sign(&soyeht_engine::auth::hello_bytes(
                &nonce,
                &client_nonce.unwrap_or([0; 32]),
                self.challenge["engineId"].as_str().unwrap(),
                token,
            ))
            .to_bytes(),
        );
        let mut params = json!({"client":"phone-test","clientVersion":"1","protocolVersion":1,"deviceToken":token,"proof":proof});
        if let Some(client_nonce) = client_nonce {
            params["clientNonce"] = json!(STANDARD.encode(client_nonce));
        }
        self.request("hello", params)
    }
    /// Whether `welcome` carries the proof of the engine whose key the pairing link named.
    fn engine_proved(
        &self,
        engine: &ed25519_dalek::VerifyingKey,
        welcome: &Value,
        device: &str,
    ) -> bool {
        let nonce = self.nonce();
        let bytes = soyeht_engine::auth::engine_hello_bytes(
            &nonce,
            &[5; 32],
            self.challenge["engineId"].as_str().unwrap(),
            device,
        );
        welcome["result"]["engineProof"]
            .as_str()
            .and_then(|text| STANDARD.decode(text).ok())
            .and_then(|raw| ed25519_dalek::Signature::from_slice(&raw).ok())
            .is_some_and(|signature| engine.verify_strict(&bytes, &signature).is_ok())
    }
}

/// The proof a phone sends for a pairing attempt: an HMAC over the challenge and the device name.
fn pairing_proof(derived: &[u8], begin: &Value) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(derived).unwrap();
    mac.update(
        &STANDARD
            .decode(begin["challenge"].as_str().unwrap())
            .unwrap(),
    );
    mac.update(b"Test phone");
    STANDARD.encode(mac.finalize().into_bytes())
}

fn assert_unpaired_phone_is_refused(phone: &mut Phone, pane: &str) {
    assert_eq!(
        phone.request("workspace.list", json!({}))["error"]["data"]["kind"],
        "unauthorized"
    );
    for (method, params) in [
        (
            "pane.input",
            json!({"paneId":pane,"bytes":STANDARD.encode(b"echo no\n"),"idempotencyKey":"unpaired-input"}),
        ),
        (
            "settings.set",
            json!({"key":soyeht_protocol::setting::HARNESS_PREFERRED_ID,"value":"unpaired","idempotencyKey":"unpaired-setting"}),
        ),
        (
            "device.pair.start",
            json!({"idempotencyKey":"unpaired-pair-window"}),
        ),
    ] {
        assert_eq!(
            phone.request(method, params)["error"]["data"]["kind"],
            "unauthorized",
            "{method} before pairing"
        );
    }
}

fn assert_paired_phone_acts_like_the_local_human(
    phone: &mut Phone,
    local: &mut Client,
    pane: &str,
) {
    // A paired device is a full remote control: it types into terminals and
    // changes settings exactly like the local human.
    let typed = phone.request("pane.input",json!({"paneId":pane,"bytes":STANDARD.encode(b"echo from-phone\n"),"idempotencyKey":"phone-input"}));
    assert!(typed.get("error").is_none(), "{typed}");
    assert!(
        typed["result"]["acceptedBytes"].as_u64().unwrap() > 0,
        "{typed}"
    );
    let key_name = soyeht_protocol::setting::HARNESS_PREFERRED_ID;
    let set = phone.request(
        "settings.set",
        json!({"key":key_name,"value":"phone-choice","idempotencyKey":"phone-setting"}),
    );
    assert_eq!(set["result"]["value"], "phone-choice", "{set}");
    assert_eq!(
        local.call("settings.get", json!({"key":key_name}))["value"],
        "phone-choice"
    );
    let listed = phone.request("device.list", json!({}));
    assert_eq!(
        listed["result"]["items"].as_array().unwrap().len(),
        1,
        "{listed}"
    );
    assert_eq!(
        phone.request("workspace.list", json!({}))["result"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

/// The engine key the pairing link of `window` names, which a phone pins.
fn pinned_engine_key(window: &Value) -> ed25519_dalek::VerifyingKey {
    let text = window["uri"]
        .as_str()
        .unwrap()
        .split("engineKey=")
        .nth(1)
        .unwrap();
    ed25519_dalek::VerifyingKey::from_bytes(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(text)
            .unwrap()
            .try_into()
            .unwrap(),
    )
    .unwrap()
}

/// `pair.begin`'s answer must prove the engine the link named.
fn assert_pairing_proved_by(
    engine: &ed25519_dalek::VerifyingKey,
    begin: &Value,
    device: &ed25519_dalek::SigningKey,
) {
    let bytes = soyeht_engine::auth::engine_pair_bytes(
        begin["requestId"].as_str().unwrap(),
        &STANDARD
            .decode(begin["challenge"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap(),
        device.verifying_key().as_bytes(),
    );
    let proof = ed25519_dalek::Signature::from_slice(
        &STANDARD
            .decode(begin["engineProof"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(
        engine.verify_strict(&bytes, &proof).is_ok(),
        "pair.begin must prove the engine the link named"
    );
}

#[test]
fn phone_pairing_requires_consent_signature_and_revocation_closes_access() {
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
    let engine = pinned_engine_key(&window);
    let mut phone = Phone::connect(address);
    assert_unpaired_phone_is_refused(&mut phone, &pane);
    let key = ed25519_dalek::SigningKey::from_bytes(&[17; 32]);
    let begin=phone.request("pair.begin",json!({"code":window["code"],"deviceName":"Test phone","devicePublicKey":STANDARD.encode(key.verifying_key().to_bytes()),"idempotencyKey":"begin"}));
    assert!(begin.get("error").is_none(), "{begin}");
    let begin = &begin["result"];
    assert_pairing_proved_by(&engine, begin, &key);
    let derived = Sha256::digest(
        [
            window["code"].as_str().unwrap().as_bytes(),
            key.verifying_key().as_bytes(),
        ]
        .concat(),
    );
    let prove=phone.request("pair.prove",json!({"requestId":begin["requestId"],"proof":pairing_proof(&derived,begin),"idempotencyKey":"prove"}));
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
    recover.socket.send(tokio_tungstenite::tungstenite::Message::Text(json!({
        "jsonrpc":"2.0","id":900,"method":"pair.prove","params":{
            "requestId":begin["requestId"],"proof":pairing_proof(&derived,begin),"idempotencyKey":"prove"
        }
    }).to_string().into())).unwrap();
    let replayed = recover.read();
    assert_eq!(replayed["method"], "pair.result");
    assert_eq!(replayed["params"]["deviceToken"], token);
    assert_eq!(recover.read()["id"], 900);
    drop(recover);
    drop(phone);
    let mut phone = Phone::connect(address);
    let welcome = phone.hello(&key, token);
    assert_eq!(welcome["result"]["role"], "human_device");
    assert!(
        phone.engine_proved(&engine, &welcome, device),
        "welcome must carry the engine's proof bound to this connection: {welcome}"
    );
    // A phone that sends no nonce of its own cannot check the engine: it is refused and told why.
    let mut old_phone = Phone::connect(address);
    assert_eq!(
        old_phone.hello_with(&key, token, None)["error"]["data"]["kind"],
        "protocol_version_unsupported"
    );
    drop(old_phone);
    assert_paired_phone_acts_like_the_local_human(&mut phone, &mut local, &pane);
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
    assert!(
        matches!(
            phone.socket.read(),
            Ok(tokio_tungstenite::tungstenite::Message::Close(_)) | Err(_)
        ),
        "a revoked device keeps no connection to type through"
    );
}

#[test]
fn dispatch_uses_the_task_named_harness_and_delivers_the_complete_first_prompt() {
    let rig = Rig::new();
    rig.install_switch_harnesses();
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Dispatch", &rig.path, "dispatch-workspace");
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
        "preset":{"workspaceDefault":false,"harness":"next-agent","permission":"ask"},
        "idempotencyKey":"dispatch-once"
    });
    let dispatched = client.call("task.dispatch", params.clone());
    assert_eq!(client.call("task.dispatch", params), dispatched);
    let pane = dispatched["paneId"].as_str().unwrap();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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
    assert_eq!(shown["task"]["executor"]["harness"], "next-agent");
    assert_eq!(client.pane(pane)["agent"]["harness"], "next-agent");
    assert_eq!(shown["attempts"].as_array().unwrap().len(), 1);
    let transcript = wait_for_transcript(
        &mut client,
        pane,
        "first prompt was not delivered",
        |message| {
            message["role"] == "user"
                && message["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("## Must not change\nKeep prior methods"))
        },
    );
    let text = transcript["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "user")
        .unwrap()["text"]
        .as_str()
        .unwrap();
    let text = text.replace('\\', "/");
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
