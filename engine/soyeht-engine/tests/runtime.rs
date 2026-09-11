use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::frame::Frame;
use soyeht_ptyd::{
    client::Client as PtyClient,
    wire::{Request, Response},
};
use std::{
    fs,
    os::unix::net::UnixStream,
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
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine"));
        let ptyd = Command::new(binary.with_file_name("soyeht-ptyd"))
            .arg("--data-dir")
            .arg(&path)
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
        self.engine = Some(
            Command::new(env!("CARGO_BIN_EXE_soyeht-engine"))
                .args(["--listen", "127.0.0.1:0", "--data-dir"])
                .arg(&self.path)
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
impl Client {
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
