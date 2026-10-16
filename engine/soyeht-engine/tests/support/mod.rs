//! The engine integration rig: a disposable engine and supervisor, and a
//! JSON-RPC client. Each test crate uses the part it needs.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_platform::local::{Stream, connect};
use soyeht_protocol::frame::Frame;
use soyeht_ptyd::{
    client::Client as PtyClient,
    wire::{Request, Response},
};
use soyeht_store::test_support::DatabaseProbe;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, MutexGuard},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

// Process-heavy integration tests share the runner. Wait for the observable
// effect rather than assuming it will occur within one scheduling quantum.
pub const ASYNC_ASSERTION_TIMEOUT: Duration = Duration::from_secs(30);

// This is a negative observation window, so increasing it would slow the
// passing suite and weaken the assertion that backpressure remains bounded.
pub const BACKPRESSURE_OBSERVATION_WINDOW: Duration = Duration::from_secs(6);

// Every Rig owns an engine and supervisor process. Keep those process-heavy
// controls in one queue without reducing the parallelism of pure tests.
pub static RUNTIME_RIG_SLOT: Mutex<()> = Mutex::new(());

pub fn runtime_rig_slot() -> MutexGuard<'static, ()> {
    RUNTIME_RIG_SLOT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct Rig {
    pub _runtime_slot: MutexGuard<'static, ()>,
    pub path: PathBuf,
    pub engine: Option<Child>,
    pub ptyd: Child,
}

impl Rig {
    pub fn new() -> Self {
        let runtime_slot = runtime_rig_slot();
        warm_up_binaries();
        let path = rig_root().join(format!("sn-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        install_fake_worktrunk(&path);
        install_fake_lsp(&path);
        let ptyd = Command::new(sibling_binary("soyeht-ptyd"))
            .arg("--data-dir")
            .arg(&path)
            .env("PATH", test_path(&path))
            .env("ZDOTDIR", &path)
            .stdout(Stdio::null())
            .spawn()
            .expect("build workspace binaries before engine integration tests");
        let mut rig = Self {
            _runtime_slot: runtime_slot,
            path,
            engine: None,
            ptyd,
        };
        rig.start();
        rig
    }
    /// Where the engine looks for fake usage sources (the test engine only).
    pub fn usage_readers(&self) -> PathBuf {
        self.path.join("usage-readers")
    }
    pub fn start(&mut self) {
        warm_up_binaries();
        self.engine = Some(
            Command::new(env!("CARGO_BIN_EXE_soyeht-engine-test"))
                .args(["--listen", "127.0.0.1:0", "--data-dir"])
                .arg(&self.path)
                .env("PATH", test_path(&self.path))
                .env("ZDOTDIR", &self.path)
                // Usage sources read only the rig's fakes, never the machine's.
                .env("SOYEHT_TEST_USAGE_READER_DIR", self.usage_readers())
                // The engine runs in another process: it follows skill folders by
                // reading them, not by file events a loaded machine delivers late.
                .env("SOYEHT_TEST_SKILL_POLL", "1")
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        while connect(&self.path.join("engine.sock")).is_err() {
            let exit = self.engine.as_mut().unwrap().try_wait();
            assert!(
                Instant::now() < deadline,
                "engine ready timeout: engine {exit:?}, socket exists {}, ptyd {:?}",
                self.path.join("engine.sock").exists(),
                self.ptyd.try_wait()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn install_fake_harness(&self, modes: &str) {
        self.install_fake_harness_with(&["--modes", modes]);
    }
    /// The test agent, launched with extra fake-agent arguments.
    pub fn install_fake_harness_with(&self, arguments: &[&str]) {
        let mut args = vec![json!("--state-dir"), json!(self.path.join("native"))];
        args.extend(arguments.iter().map(|argument| json!(argument)));
        self.write_harnesses(&json!([{
            "id":"test-agent","name":"Test agent","command":sibling_binary("soyeht-fake-acp"),
            "args":args,"version":"test"
        }]));
    }
    /// Replaces the profile's custom harness registry.
    pub fn write_harnesses(&self, entries: &Value) {
        fs::write(
            self.path.join("harnesses.json"),
            serde_json::to_vec(entries).unwrap(),
        )
        .unwrap();
    }
    /// An `npx` that starts the fake agent, which the registry's package runner launches.
    #[cfg(unix)]
    pub fn install_fake_claude_runner(&self) {
        let binary = sibling_binary("soyeht-fake-acp");
        write_script(
            &self.path.join("bin/npx"),
            &format!(
                "#!/bin/sh\nexec '{}' --state-dir '{}'\n",
                binary.display(),
                self.path.join("claude-native").display()
            ),
        );
    }
    /// A program named `name` on the engine's search path that exists but is never run.
    pub fn install_stub_executable(&self, name: &str) {
        let bin = self.path.join("bin");
        fs::create_dir_all(&bin).unwrap();
        #[cfg(unix)]
        write_script(&bin.join(name), "#!/bin/sh\nexit 0\n");
        #[cfg(windows)]
        fs::write(bin.join(format!("{name}.exe")), "MZ").unwrap();
    }
    pub fn install_switch_harnesses(&self) {
        let binary = sibling_binary("soyeht-fake-acp");
        self.write_harnesses(&json!([
            {
                "id":"test-agent","name":"Test agent","command":binary,
                "args":["--state-dir",self.path.join("native")],"version":"test"
            },
            {
                "id":"next-agent","name":"Next agent","command":binary,
                "args":["--state-dir",self.path.join("next-native"),
                    "--initialize-padding-bytes","65000"],"version":"test"
            },
            {
                "id":"chunked-agent","name":"Chunked Agent","command":binary,
                "args":["--state-dir",self.path.join("chunked-native"),
                    "--replay-user-chunk-bytes","64"],"version":"test"
            },
            {
                "id":"broken-agent","name":"Broken agent","command":binary,
                "args":["--state-dir",self.path.join("broken-native"),"--require-login"],
                "version":"test"
            }
        ]));
    }
    pub fn open_agent(&self, client: Client, workspace: &Value) -> (Client, String) {
        self.open_agent_announcing(client, workspace, "config")
    }
    pub fn open_agent_announcing(
        &self,
        client: Client,
        workspace: &Value,
        modes: &str,
    ) -> (Client, String) {
        self.install_fake_harness(modes);
        self.open_installed_agent(client, workspace)
    }
    /// Opens a pane on the test agent already written to harnesses.json.
    pub fn open_installed_agent(&self, client: Client, workspace: &Value) -> (Client, String) {
        self.open_harness_agent(client, workspace, "test-agent")
    }
    /// Opens a pane on the harness `harness` already written to harnesses.json.
    pub fn open_harness_agent(
        &self,
        mut client: Client,
        workspace: &Value,
        harness: &str,
    ) -> (Client, String) {
        let pane = client.call(
            "pane.create",
            json!({"workspaceId":workspace,"mode":"agent",
            "harness":harness,"idempotencyKey":Uuid::new_v4().to_string()}),
        )["paneId"]
            .as_str()
            .unwrap()
            .to_owned();
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        loop {
            let view = client.pane(&pane);
            if view["agent"]["sessionId"].is_string() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "agent session did not open: {view}"
            );
            thread::sleep(Duration::from_millis(20));
        }
        (client, pane)
    }
    pub fn agent(&self) -> (Client, String) {
        self.agent_announcing("config")
    }
    /// An agent pane whose fake announces its modes as `config`, `legacy` or `none`.
    pub fn agent_announcing(&self, modes: &str) -> (Client, String) {
        self.agent_with(&["--modes", modes])
    }
    /// An agent pane whose fake runs with extra arguments.
    pub fn agent_with(&self, arguments: &[&str]) -> (Client, String) {
        let mut client = self.client();
        let workspace = client.call(
            "workspace.create",
            json!({"name":"Test","path":self.path,
            "idempotencyKey":"agent-workspace"}),
        )["workspace"]["workspaceId"]
            .clone();
        self.install_fake_harness_with(arguments);
        self.open_installed_agent(client, &workspace)
    }
    pub fn agent_in_workspace(&self, workspace: &str) -> (Client, String) {
        self.open_agent(self.client(), &json!(workspace))
    }
    pub fn database_probe(&self) -> DatabaseProbe {
        DatabaseProbe::open(&self.path).unwrap()
    }
    pub fn native_session(&self, session_id: &str) -> Value {
        serde_json::from_slice(
            &fs::read(self.path.join("native").join(format!("{session_id}.json"))).unwrap(),
        )
        .unwrap()
    }
    // Shift only a stopped test engine's persisted timestamps. There is no
    // runtime clock override or shortened deadline in the shipped executable.
    pub fn age_agent(&self, pane: &str, cancel: bool) {
        assert!(self.engine.is_none());
        self.database_probe().age_agent(pane, cancel).unwrap();
    }
    pub fn set_agent_silent_since(&self, pane: &str, value: i64) {
        assert!(self.engine.is_none());
        self.database_probe()
            .set_agent_silent_since(pane, value)
            .unwrap();
    }
    pub fn kill_engine(&mut self) {
        if let Some(mut child) = self.engine.take() {
            child.kill().unwrap();
            child.wait().unwrap();
        }
        let _ = fs::remove_file(self.path.join("engine.sock"));
    }
    pub fn restart_ptyd(&mut self) {
        self.ptyd.kill().unwrap();
        self.ptyd.wait().unwrap();
        let _ = fs::remove_file(self.path.join("ptyd.sock"));
        self.ptyd = Command::new(sibling_binary("soyeht-ptyd"))
            .arg("--data-dir")
            .arg(&self.path)
            .env("PATH", test_path(&self.path))
            .env("ZDOTDIR", &self.path)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        while connect(&self.path.join("ptyd.sock")).is_err() {
            assert!(Instant::now() < deadline, "ptyd ready timeout");
            thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn client(&self) -> Client {
        Client::new(self.path.join("engine.sock"))
    }
    pub fn hook_client(&self) -> Client {
        Client::new_named(self.path.join("engine.sock"), "soyeht-hook", json!([]))
    }
}

/// Runs each program a rig starts once before any deadline runs: the first run of a new
/// binary is assessed by the system, which takes seconds under load (see
/// `soyeht_platform::testing::warm_up`). The time spent shows with `--nocapture`.
fn warm_up_binaries() {
    for name in ["soyeht-ptyd", "soyeht-fake-acp"] {
        warm_up(&sibling_binary(name));
    }
    warm_up(Path::new(env!("CARGO_BIN_EXE_soyeht-engine-test")));
}

fn warm_up(binary: &Path) {
    let took = soyeht_platform::testing::warm_up(binary);
    if took > Duration::from_secs(1) {
        eprintln!("first run of {} took {took:?}", binary.display());
    }
}

/// A binary built next to the engine, by name; Windows adds the `.exe`.
pub fn sibling_binary(name: &str) -> PathBuf {
    soyeht_platform::testing::sibling_binary(env!("CARGO_BIN_EXE_soyeht-engine-test"), name)
}

/// Whether a terminal printed `text`: exactly it on Unix, while a Windows console frames what a
/// program prints with its own sequences.
pub fn printed(output: &[u8], text: &[u8]) -> bool {
    if cfg!(windows) {
        output.windows(text.len()).any(|part| part == text)
    } else {
        output == text
    }
}

/// Writes an executable shell script.
#[cfg(unix)]
pub fn write_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Where rigs live. Windows reports the temporary folder by its 8.3 short name, while the engine
/// names the same files by their long ones; only the long form compares equal in assertions.
/// Unix keeps the plain folder: a socket path has a short limit.
fn rig_root() -> PathBuf {
    let temp = std::env::temp_dir();
    if cfg!(windows) {
        soyeht_platform::fs::canonicalize(&temp).unwrap()
    } else {
        temp
    }
}

pub fn test_path(root: &std::path::Path) -> std::ffi::OsString {
    std::env::join_paths(
        std::iter::once(root.join("bin")).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )),
    )
    .unwrap()
}

/// Makes `binary` available on the engine's search path as `name`: a link where the system
/// allows one without privilege, a copy (with the `.exe` Windows needs) elsewhere.
fn install_fake(root: &Path, binary: &str, name: &str) {
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let binary = sibling_binary(binary);
    #[cfg(unix)]
    std::os::unix::fs::symlink(binary, bin.join(name)).unwrap();
    #[cfg(windows)]
    fs::copy(binary, bin.join(format!("{name}.exe"))).unwrap();
}

pub fn install_fake_lsp(root: &Path) {
    install_fake(root, "soyeht-fake-lsp", "rust-analyzer");
}

pub fn install_fake_worktrunk(root: &Path) {
    install_fake(root, "soyeht-fake-worktrunk", "wt");
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

pub struct Client {
    pub socket: Stream,
    pub next: u64,
    pub messages: Vec<Frame>,
}

pub fn transcript_items(client: &mut Client, pane: &str) -> Vec<Value> {
    client.call("pane.transcript", json!({"paneId":pane}))["items"]
        .as_array()
        .unwrap()
        .clone()
}

pub fn wait_for_user_message(client: &mut Client, pane: &str, text: &str) -> Vec<Value> {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let items = transcript_items(client, pane);
        if items
            .iter()
            .any(|message| message["role"] == "user" && message["text"] == text)
        {
            return items;
        }
        assert!(Instant::now() < deadline, "{text} was not recorded");
        thread::sleep(Duration::from_millis(20));
    }
}

/// Restarts the engine and its agents, then waits until the native replay has
/// replaced every message the live transcript held.
pub fn restart_and_wait_for_replay(rig: &mut Rig, pane: &str, live: &[Value]) -> Vec<Value> {
    rig.kill_engine();
    rig.restart_ptyd();
    rig.start();
    let mut restored = rig.client();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let items = transcript_items(&mut restored, pane);
        if !items.is_empty()
            && items
                .iter()
                .all(|item| live.iter().all(|old| old["localId"] != item["localId"]))
        {
            return items;
        }
        assert!(
            Instant::now() < deadline,
            "the native replay did not replace the transcript"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

impl Client {
    /// Parameters of the notifications `method` received so far.
    pub fn notifications(&self, method: &str) -> Vec<Value> {
        self.messages
            .iter()
            .filter_map(|frame| {
                let Frame::Json(bytes) = frame else {
                    return None;
                };
                let value: Value = serde_json::from_slice(bytes).ok()?;
                (value["method"] == method).then(|| value["params"].clone())
            })
            .collect()
    }
    /// Waits for a notification `method` whose parameters satisfy `accept`.
    pub fn wait_for_notification(
        &mut self,
        method: &str,
        accept: impl Fn(&Value) -> bool,
    ) -> Value {
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        loop {
            self.call("engine.status", json!({}));
            if let Some(params) = self.notifications(method).into_iter().find(&accept) {
                return params;
            }
            assert!(Instant::now() < deadline, "{method} was not published");
            thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn wait_state(&mut self, pane: &str, state: &str) -> Value {
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        loop {
            let view = self.pane(pane);
            if view["state"] == state {
                return view;
            }
            assert!(Instant::now() < deadline, "expected {state}: {view}");
            thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn section(&mut self, section: &str) -> Vec<Value> {
        self.snapshot()
            .1
            .into_iter()
            .filter(|p| p["section"] == section)
            .flat_map(|p| p["data"].as_array().unwrap().clone())
            .collect()
    }
    pub fn new(path: PathBuf) -> Self {
        Self::new_with_capabilities(path, json!([]))
    }
    pub fn new_with_capabilities(path: PathBuf, capabilities: Value) -> Self {
        Self::new_named(path, "runtime-test", capabilities)
    }
    pub fn new_named(path: PathBuf, name: &str, capabilities: Value) -> Self {
        let socket = connect(&path).unwrap();
        socket
            .set_read_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
            .unwrap();
        socket
            .set_write_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
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
            json!({"client":name,"clientVersion":"1","protocolVersion":1,
                "capabilities":capabilities}),
        );
        assert_eq!(welcome["role"], "human_local");
        client
    }
    pub fn request(&mut self, method: &str, params: Value) -> Value {
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
    pub fn call(&mut self, method: &str, params: Value) -> Value {
        let response = self.request(method, params);
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
    }
    pub fn snapshot(&mut self) -> (Value, Vec<Value>) {
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
    /// Whether the pane's agent has a session, so its process is running.
    pub fn agent_started(&mut self, pane: &str) -> bool {
        self.pane(pane)["agent"]["sessionId"].is_string()
    }
    pub fn pane(&mut self, id: &str) -> Value {
        self.snapshot()
            .1
            .into_iter()
            .filter(|p| p["section"] == "panes")
            .flat_map(|p| p["data"].as_array().unwrap().clone())
            .find(|p| p["paneId"] == id)
            .expect("pane in snapshot")
    }
    pub fn create(&mut self, rig: &Rig) -> (String, String) {
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
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        while self.pane(&pane)["pid"].is_null() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        (workspace, pane)
    }
    pub fn tail(&mut self, pane: &str) -> (Value, Vec<Frame>) {
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
