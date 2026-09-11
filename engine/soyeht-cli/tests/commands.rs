use serde_json::Value;
use soyeht_ptyd::{
    client::Client,
    wire::{Request, Response},
};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

struct Rig {
    path: PathBuf,
    engine: Child,
    ptyd: Child,
}
impl Rig {
    fn start() -> Self {
        let path = PathBuf::from("/tmp").join(format!("sn-cli-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        let path = path.canonicalize().unwrap();
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_soyeht"));
        let ptyd = Command::new(binary.with_file_name("soyeht-ptyd"))
            .arg("--data-dir")
            .arg(&path)
            .stdout(Stdio::null())
            .spawn()
            .expect("build workspace binaries first");
        let engine = Command::new(binary.with_file_name("soyeht-engine"))
            .args(["--listen", "127.0.0.1:0", "--data-dir"])
            .arg(&path)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let rig = Self { path, engine, ptyd };
        let deadline = Instant::now() + Duration::from_secs(15);
        while soyeht_cli::Client::connect(&rig.path).is_err() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        rig
    }
    fn command(&self, args: &[&str]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_soyeht"))
            .args(["--profile", "dev", "--data-dir"])
            .arg(&self.path)
            .args(args)
            .current_dir(&self.path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.engine.kill();
        let _ = self.engine.wait();
        if let Ok(mut client) = Client::connect(&self.path.join("ptyd.sock"), u64::MAX)
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

#[test]
fn cli_creates_lists_and_closes_a_real_shell_and_starts_pairing() {
    let rig = Rig::start();
    assert_eq!(rig.command(&["status"])["ptyd"]["connected"], true);
    let first = rig.command(&[
        "pane",
        "create",
        "--shell",
        "--idempotency-key",
        "cli-test-create",
    ]);
    assert_eq!(
        first,
        rig.command(&[
            "pane",
            "create",
            "--shell",
            "--idempotency-key",
            "cli-test-create"
        ])
    );
    let list = rig.command(&["pane", "list"]);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["cwd"], rig.path.to_str().unwrap());
    let pairing = rig.command(&["device", "pair-start"]);
    assert!(
        pairing["uri"]
            .as_str()
            .unwrap()
            .starts_with("soyeht-next://pair?")
    );
    assert!(!pairing["code"].as_str().unwrap().is_empty());
    assert!(
        rig.command(&["device", "list"])["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    rig.command(&["pane", "close", first["paneId"].as_str().unwrap()]);
    assert!(
        rig.command(&["pane", "list"])["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn another_profile_is_refused_before_connecting() {
    let output = Command::new(env!("CARGO_BIN_EXE_soyeht"))
        .args(["--profile", "other", "status"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("only the new dev profile"));
    assert!(output.stdout.is_empty());
}

#[test]
fn client_reconnects_after_engine_restart() {
    let mut rig = Rig::start();
    let mut client = soyeht_cli::Client::connect(&rig.path).unwrap();
    let before: soyeht_protocol::EngineStatusResult = client
        .request("engine.status", serde_json::json!({}))
        .unwrap();
    rig.engine.kill().unwrap();
    rig.engine.wait().unwrap();
    rig.engine =
        Command::new(PathBuf::from(env!("CARGO_BIN_EXE_soyeht")).with_file_name("soyeht-engine"))
            .args(["--listen", "127.0.0.1:0", "--data-dir"])
            .arg(&rig.path)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
    let after: soyeht_protocol::EngineStatusResult = client
        .request("engine.status", serde_json::json!({}))
        .unwrap();
    assert!(after.generation > before.generation);
    assert_eq!(after.db.id, before.db.id);
}

#[test]
fn cli_agent_controls_use_the_engine_session_and_native_permission_options() {
    let rig = Rig::start();
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_soyeht")).with_file_name("soyeht-fake-acp");
    fs::write(
        rig.path.join("harnesses.json"),
        serde_json::to_vec(&serde_json::json!([{
            "id":"test-agent","name":"Test agent","command":binary,
            "args":["--state-dir",rig.path.join("native")],"version":"1"
        }]))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        rig.command(&["harness", "list"])["items"][0]["id"],
        "test-agent"
    );
    let created = rig.command(&[
        "pane",
        "create",
        "--agent",
        "test-agent",
        "--model",
        "model-b",
        "--effort",
        "high",
        "--permission",
        "ask",
        "--idempotency-key",
        "agent-create",
    ]);
    let pane = created["paneId"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let before = loop {
        let list = rig.command(&["pane", "list"]);
        let current = &list["items"][0];
        let ready = current["agent"]["options"]
            .as_array()
            .is_some_and(|options| {
                options
                    .iter()
                    .any(|o| o["id"] == "model" && o["currentValue"] == "model-b")
                    && options
                        .iter()
                        .any(|o| o["id"] == "effort" && o["currentValue"] == "high")
            });
        if ready {
            break current.clone();
        }
        assert!(
            Instant::now() < deadline,
            "initial CLI selectors not applied"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(
        rig.command(&["model", "list", "test-agent", "--pane", pane])["source"],
        "native_session"
    );
    assert_eq!(
        rig.command(&["pane", "config", pane, "effort", "low"])["effective"],
        "low"
    );
    assert_eq!(
        rig.command(&["pane", "config", pane, "fast", "true"])["effective"],
        true
    );
    let prompt = rig.command(&[
        "pane",
        "prompt",
        pane,
        "permission",
        "--idempotency-key",
        "ask",
    ]);
    assert_eq!(
        prompt,
        rig.command(&[
            "pane",
            "prompt",
            pane,
            "permission",
            "--idempotency-key",
            "ask"
        ])
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while rig.command(&["pane", "list"])["items"][0]["state"] != "waiting_for_you" {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        rig.command(&["pane", "permission", pane, "allow"])["optionId"],
        "once"
    );
    while rig.command(&["pane", "list"])["items"][0]["state"] != "idle" {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(rig.command(&["pane", "yolo", pane, "on"])["on"], true);
    assert_eq!(rig.command(&["pane", "yolo", pane, "off"])["on"], false);
    let transcript = rig.command(&["pane", "transcript", pane, "--limit", "1"]);
    assert_eq!(transcript["items"].as_array().unwrap().len(), 1);
    let cursor = transcript["nextCursor"]
        .as_str()
        .expect("older transcript page");
    let older = rig.command(&[
        "pane",
        "transcript",
        pane,
        "--before",
        cursor,
        "--limit",
        "1",
    ]);
    assert_ne!(older["items"][0]["seq"], transcript["items"][0]["seq"]);
    let after = rig.command(&["pane", "list"])["items"][0].clone();
    assert_eq!(after["pid"], before["pid"]);
    assert_eq!(after["agent"]["sessionId"], before["agent"]["sessionId"]);
    rig.command(&["pane", "close", pane]);
}
