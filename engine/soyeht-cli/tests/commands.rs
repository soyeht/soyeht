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
