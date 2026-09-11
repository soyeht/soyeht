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
    fn agent(&self) -> (Client, String) {
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
        let mut client = self.client();
        let workspace = client.call(
            "workspace.create",
            json!({"name":"Test","path":self.path,
            "idempotencyKey":"agent-workspace"}),
        )["workspace"]["workspaceId"]
            .clone();
        let pane = client.call(
            "pane.create",
            json!({"workspaceId":workspace,"mode":"agent",
            "harness":"test-agent","idempotencyKey":"agent-pane"}),
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
    fn db(&self) -> rusqlite::Connection {
        let db = rusqlite::Connection::open(self.path.join("soyeht.db")).unwrap();
        db.busy_timeout(Duration::from_secs(5)).unwrap();
        db
    }
    // Shift only a stopped test engine's persisted timestamps. There is no
    // runtime clock override or shortened deadline in the shipped executable.
    fn age_agent(&self, pane: &str, cancel: bool) {
        assert!(self.engine.is_none());
        let db = self.db();
        let raw: String = db
            .query_row(
                "SELECT payload FROM agent_runtime WHERE pane_id=?1",
                [pane],
                |r| r.get(0),
            )
            .unwrap();
        let mut state: Value = serde_json::from_str(&raw).unwrap();
        state["last_update_ms"] = json!(0);
        for pending in state["client"]["pending"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            pending["sent_at_ms"] = json!(0);
        }
        if cancel {
            assert!(state["cancel_at_ms"].is_i64());
            state["cancel_at_ms"] = json!(0);
        }
        db.execute(
            "UPDATE agent_runtime SET payload=?1 WHERE pane_id=?2",
            rusqlite::params![state.to_string(), pane],
        )
        .unwrap();
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
        let ended: i64 = rig.db().query_row("SELECT count(*) FROM events WHERE method='pane.turn_ended' AND json_extract(params,'$.promptId')=?1", [waiting["promptId"].as_str().unwrap()], |r| r.get(0)).unwrap();
        if ended == 1 {
            break;
        }
        assert!(Instant::now() < deadline, "resumed queue did not run");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(client.pane(&pane)["agent"]["queuePaused"], false);
    let events: i64 = rig
        .db()
        .query_row(
            "SELECT count(*) FROM events WHERE method='pane.queue.resumed'",
            [],
            |r| r.get(0),
        )
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
        let ended: i64 = rig.db().query_row("SELECT count(*) FROM events WHERE method='pane.turn_ended' AND json_extract(params,'$.promptId')=?1", [prompt["promptId"].as_str().unwrap()], |r| r.get(0)).unwrap();
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
    let response: String = rig
        .db()
        .query_row(
            "SELECT result FROM agent_requests WHERE method='terminal/create' AND pane_id=?1",
            [&pane],
            |r| r.get(0),
        )
        .unwrap();
    let response: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(response["native"]["error"]["code"], -32603);
    let pending: i64 = rig
        .db()
        .query_row(
            "SELECT count(*) FROM agent_requests WHERE result IS NULL AND pane_id=?1",
            [&pane],
            |r| r.get(0),
        )
        .unwrap();
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
    let generation: String = rig
        .db()
        .query_row(
            "SELECT value FROM metadata WHERE key='engine_generation'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut supervisor =
        PtyClient::connect(&rig.path.join("ptyd.sock"), generation.parse().unwrap()).unwrap();
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
    let count: i64 = rig.db().query_row("SELECT count(*) FROM events WHERE method='pane.permission_requested' AND json_extract(params,'$.requestId')=?1", [request.as_str().unwrap()], |r| r.get(0)).unwrap();
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
    let generation: String = rig
        .db()
        .query_row(
            "SELECT value FROM metadata WHERE key='engine_generation'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut supervisor =
        PtyClient::connect(&rig.path.join("ptyd.sock"), generation.parse().unwrap()).unwrap();
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
    let persisted: i64 = rig
        .db()
        .query_row(
            "SELECT count(*) FROM agent_requests WHERE method='session/request_permission'",
            [],
            |r| r.get(0),
        )
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
    let answered: i64 = rig.db().query_row("SELECT count(*) FROM events WHERE method='pane.permission_answered' AND json_extract(params,'$.requestId')=?1", [id.as_str().unwrap()], |r| r.get(0)).unwrap();
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
        let db = rig.db();
        let done: i64 = db.query_row("SELECT count(*) FROM events WHERE method='pane.turn_ended' AND json_extract(params,'$.promptId')=?1", [prompt["promptId"].as_str().unwrap()], |r| r.get(0)).unwrap();
        if done == 1 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "automatic permission did not finish"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let requested: i64 = rig
        .db()
        .query_row(
            "SELECT count(*) FROM events WHERE method='pane.permission_requested'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        requested, 0,
        "automatic permission was published to connected clients"
    );
    let by_rule: i64 = rig.db().query_row("SELECT count(*) FROM events WHERE method='pane.permission_answered' AND json_extract(params,'$.by')='rule'", [], |r| r.get(0)).unwrap();
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
    rig.db()
        .execute(
            "UPDATE agent_requests SET expires_ms=0 WHERE request_id=?1",
            [question["requestId"].as_str().unwrap()],
        )
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
    let pid = client.pane(&pane)["pid"].clone();
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

#[test]
fn agent_turns_and_permission_survive_engine_adoption() {
    let mut rig = Rig::new();
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
    let native_session = ready["agent"]["sessionId"].clone();
    let native_pid = ready["pid"].clone();
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
    assert_eq!(client.pane(&pane)["pid"], native_pid);
    assert_eq!(client.pane(&pane)["agent"]["sessionId"], native_session);
    assert_eq!(
        client.pane(&pane)["agent"]["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["id"] == "model")
            .unwrap()["currentValue"],
        "model-b"
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"hello","idempotencyKey":"first-prompt"}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let view = client.pane(&pane);
        if view["state"] == "idle" {
            break;
        }
        assert!(Instant::now() < deadline, "turn did not end: {view}");
        thread::sleep(Duration::from_millis(20));
    }
    let transcript = client.call("pane.transcript", json!({"paneId":pane}));
    assert!(transcript["items"].as_array().unwrap().iter().any(
        |m| m["role"] == "assistant" && m["text"].as_str().is_some_and(|s| s.contains("hello"))
    ));
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission-null","idempotencyKey":"permission-prompt"}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let question = loop {
        let (_, parts) = client.snapshot();
        if let Some(question) = parts
            .iter()
            .filter(|p| p["section"] == "requests")
            .flat_map(|p| p["data"].as_array().unwrap())
            .find(|r| r["kind"] == "permission")
        {
            break question["request"].clone();
        }
        assert!(Instant::now() < deadline, "no permission question");
        thread::sleep(Duration::from_millis(20));
    };
    rig.kill_engine();
    rig.start();
    let mut first = rig.client();
    let mut second = rig.client();
    let restored = first.pane(&pane);
    assert_eq!(restored["pid"], native_pid);
    assert_eq!(restored["agent"]["sessionId"], native_session);
    let selected = first.call("pane.permission.answer", json!({"paneId":pane,"requestId":question["requestId"],"optionId":"once","idempotencyKey":"answer-once"}));
    let repeated = second.call("pane.permission.answer", json!({"paneId":pane,"requestId":question["requestId"],"optionId":"deny","idempotencyKey":"late-answer"}));
    assert_eq!(selected, repeated);
    assert_eq!(selected["optionId"], "once");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let view = first.pane(&pane);
        if view["state"] == "idle" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "permission turn did not end: {view}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let transcript = first.call("pane.transcript", json!({"paneId":pane}));
    let decisions: Vec<_> = transcript["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| {
            m["text"]
                .as_str()
                .is_some_and(|t| t.starts_with("Permission result:"))
        })
        .collect();
    assert_eq!(decisions.len(), 1);
    assert!(decisions[0]["text"].as_str().unwrap().contains("once"));
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
        let view = first.pane(&pane);
        if view["generation"].as_u64().unwrap() > ready["generation"].as_u64().unwrap()
            && view["state"] == "idle"
        {
            assert_ne!(view["pid"], native_pid);
            assert_eq!(view["agent"]["sessionId"], native_session);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native load did not complete: {view}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let transcript = first.call("pane.transcript", json!({"paneId":pane}));
    let items = transcript["items"].as_array().unwrap();
    assert_eq!(items.iter().filter(|m| m["role"] == "user").count(), 2);
    assert_eq!(items.iter().filter(|m| m["role"] == "assistant").count(), 2);
    assert!(
        items
            .iter()
            .any(|m| m["text"] == "hello" && m["role"] == "user")
    );
    assert_eq!(
        items
            .iter()
            .filter(|m| m["text"]
                .as_str()
                .is_some_and(|t| t.starts_with("Permission result:")))
            .count(),
        1
    );
    let toggled = first.call(
        "pane.yolo",
        json!({"paneId":pane,"on":true,"idempotencyKey":"yolo-on"}),
    );
    assert_eq!(toggled["effectiveMode"], "allow_all");
    assert_eq!(toggled["on"], true);
    first.call("pane.config.set", json!({"paneId":pane,"configId":"mode","value":"accept_edits","idempotencyKey":"manual-mode"}));
    let manual = first.pane(&pane);
    assert_eq!(manual["agent"]["permission"], "accept_edits");
    assert_eq!(manual["agent"]["yolo"], false);
    assert!(manual["agent"]["previousPermission"].is_null());
    first.call(
        "pane.yolo",
        json!({"paneId":pane,"on":true,"idempotencyKey":"yolo-after-manual"}),
    );
    let restored = first.call(
        "pane.yolo",
        json!({"paneId":pane,"on":false,"idempotencyKey":"restore-manual"}),
    );
    assert_eq!(restored["effectiveMode"], "accept_edits");
    first.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"ask","idempotencyKey":"restore-ask"}),
    );
    first.call(
        "pane.yolo",
        json!({"paneId":pane,"on":true,"idempotencyKey":"yolo-before-form"}),
    );
    first.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"elicitation","idempotencyKey":"form-prompt"}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let form = loop {
        let (_, parts) = first.snapshot();
        if let Some(form) = parts
            .iter()
            .filter(|p| p["section"] == "requests")
            .flat_map(|p| p["data"].as_array().unwrap())
            .find(|r| r["kind"] == "elicitation")
        {
            break form["request"].clone();
        }
        assert!(
            Instant::now() < deadline,
            "form was not presented while yolo was enabled"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(first.pane(&pane)["state"], "waiting_for_you");
    let invalid = first.request("pane.elicitation.answer", json!({"paneId":pane,"requestId":form["requestId"],"fields":{"answer":42},"idempotencyKey":"invalid-form"}));
    assert_eq!(invalid["error"]["data"]["kind"], "invalid_params");
    let answered = first.call("pane.elicitation.answer", json!({"paneId":pane,"requestId":form["requestId"],"fields":{"answer":"yes"},"idempotencyKey":"valid-form"}));
    assert_eq!(answered["result"], "submitted");
    let deadline = Instant::now() + Duration::from_secs(10);
    while first.pane(&pane)["state"] != "idle" {
        assert!(Instant::now() < deadline, "form turn did not end");
        thread::sleep(Duration::from_millis(20));
    }
    let off = first.call(
        "pane.yolo",
        json!({"paneId":pane,"on":false,"idempotencyKey":"yolo-off"}),
    );
    assert_eq!(off["effectiveMode"], "ask");
    assert_eq!(off["on"], false);
    let pid = first.pane(&pane)["pid"].clone();
    first.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"slow","idempotencyKey":"slow-prompt"}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while first.pane(&pane)["state"] != "working" {
        assert!(Instant::now() < deadline, "slow turn did not start");
        thread::sleep(Duration::from_millis(20));
    }
    first.call(
        "pane.cancel",
        json!({"paneId":pane,"idempotencyKey":"cancel-slow"}),
    );
    while first.pane(&pane)["state"] != "idle" {
        assert!(
            Instant::now() < deadline,
            "healthy cancellation exceeded deadline"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(first.pane(&pane)["pid"], pid);
    first.call(
        "pane.queue.resume",
        json!({"paneId":pane,"idempotencyKey":"resume-tools"}),
    );
    first.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"tools","idempotencyKey":"native-tools"}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let transcript = first.call("pane.transcript", json!({"paneId":pane}));
        let has_terminal = transcript["items"].as_array().unwrap().iter().any(|item| {
            item["kind"] == "terminal"
                && item["terminal"]["bytes"].as_str().is_some_and(|bytes| {
                    STANDARD
                        .decode(bytes)
                        .is_ok_and(|bytes| bytes == b"fake-terminal")
                })
        });
        if first.pane(&pane)["state"] == "idle" && has_terminal {
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
