//! The engine and supervisor rig shared by the integration test crates.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_protocol::frame::Frame;
use soyeht_ptyd::{
    client::Client as PtyClient,
    wire::{Request, Response},
};
use soyeht_store::test_support::DatabaseProbe;
use std::{
    fs,
    os::unix::{fs::PermissionsExt, fs::symlink, net::UnixStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Mutex, MutexGuard},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

// Process-heavy integration tests share the runner. Wait for the observable
// effect rather than assuming it will occur within one scheduling quantum.
pub const ASYNC_ASSERTION_TIMEOUT: Duration = Duration::from_secs(30);

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
        let path = std::env::temp_dir().join(format!("sn-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        install_fake_worktrunk(&path);
        install_fake_lsp(&path);
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine"));
        let ptyd = Command::new(binary.with_file_name("soyeht-ptyd"))
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
    pub fn start(&mut self) {
        self.engine = Some(
            Command::new(env!("CARGO_BIN_EXE_soyeht-engine"))
                .args(["--listen", "127.0.0.1:0", "--data-dir"])
                .arg(&self.path)
                .env("PATH", test_path(&self.path))
                .env("ZDOTDIR", &self.path)
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        while UnixStream::connect(self.path.join("engine.sock")).is_err() {
            assert!(Instant::now() < deadline, "engine ready timeout");
            thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn install_fake_harness(&self, modes: &str) {
        self.install_fake_harness_with(&["--modes", modes]);
    }
    /// The test agent, launched with extra fake-agent arguments.
    pub fn install_fake_harness_with(&self, arguments: &[&str]) {
        let binary =
            PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine")).with_file_name("soyeht-fake-acp");
        let mut args = vec![json!("--state-dir"), json!(self.path.join("native"))];
        args.extend(arguments.iter().map(|argument| json!(argument)));
        fs::write(
            self.path.join("harnesses.json"),
            serde_json::to_vec(&json!([{
                "id":"test-agent","name":"Test agent","command":binary,
                "args":args,"version":"test"
            }]))
            .unwrap(),
        )
        .unwrap();
    }
    pub fn install_fake_claude_runner(&self) {
        let binary =
            PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine")).with_file_name("soyeht-fake-acp");
        let executable = self.path.join("bin/npx");
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\nexec '{}' --state-dir '{}'\n",
                binary.display(),
                self.path.join("claude-native").display()
            ),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    }
    pub fn install_switch_harnesses(&self) {
        let binary =
            PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine")).with_file_name("soyeht-fake-acp");
        fs::write(
            self.path.join("harnesses.json"),
            serde_json::to_vec(&json!([
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
            ]))
            .unwrap(),
        )
        .unwrap();
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
    pub fn open_installed_agent(&self, mut client: Client, workspace: &Value) -> (Client, String) {
        let pane = client.call(
            "pane.create",
            json!({"workspaceId":workspace,"mode":"agent",
            "harness":"test-agent","idempotencyKey":Uuid::new_v4().to_string()}),
        )["paneId"]
            .as_str()
            .unwrap()
            .to_owned();
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        while !client.pane(&pane)["agent"]["sessionId"].is_string() {
            assert!(Instant::now() < deadline, "agent session did not open");
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
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine"));
        self.ptyd = Command::new(binary.with_file_name("soyeht-ptyd"))
            .arg("--data-dir")
            .arg(&self.path)
            .env("PATH", test_path(&self.path))
            .env("ZDOTDIR", &self.path)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        while UnixStream::connect(self.path.join("ptyd.sock")).is_err() {
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

pub fn test_path(root: &std::path::Path) -> std::ffi::OsString {
    std::env::join_paths(
        std::iter::once(root.join("bin")).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )),
    )
    .unwrap()
}

pub fn install_fake_lsp(root: &std::path::Path) {
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let binary =
        PathBuf::from(env!("CARGO_BIN_EXE_soyeht-engine")).with_file_name("soyeht-fake-lsp");
    symlink(binary, bin.join("rust-analyzer")).unwrap();
}

pub fn install_fake_worktrunk(root: &std::path::Path) {
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let executable = bin.join("wt");
    fs::write(
        &executable,
        r#"#!/bin/sh
repository="$2"
command="$3"
if [ "$command" = "list" ]; then
  printf '{"schema":2,"items":['
  separator=""
  for path in "$repository"/.test-worktrees/*; do
    [ -d "$path" ] || continue
    branch=$(cat "$path/.soyeht-test-branch")
    modified=false
    [ -e "$path/.dirty" ] && modified=true
    integration=',"default_branch":{"integration":{"reason":"ancestor"}}'
    [ -e "$path/.unreachable" ] && integration=""
    printf '%s{"branch":"%s","head":{"sha":"abc"},"worktree":{"path":"%s","changes":{"staged":false,"modified":%s,"untracked":false,"renamed":false,"deleted":false}}%s}' "$separator" "$branch" "$path" "$modified" "$integration"
    separator=,
  done
  printf ']}\n'
  exit 0
fi
branch=""
for argument in "$@"; do branch="$argument"; done
safe_branch=$(printf '%s' "$branch" | tr '/' '-')
path="$repository/.test-worktrees/$safe_branch"
if [ "$command" = "switch" ]; then
  mkdir -p "$path"
  printf '%s' "$branch" > "$path/.soyeht-test-branch"
  printf '{"path":"%s","branch":"%s"}\n' "$path" "$branch"
  exit 0
fi
if [ "$command" = "remove" ]; then
  rm -rf "$path"
  printf '{"kind":"worktree","branch":"%s","path":"%s","branch_outcome":"not_attempted"}\n' "$branch" "$path"
  exit 0
fi
exit 2
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(executable, permissions).unwrap();
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
    pub socket: UnixStream,
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
        let socket = UnixStream::connect(path).unwrap();
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
