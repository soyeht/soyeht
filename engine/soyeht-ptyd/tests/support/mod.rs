//! The supervisor test rig: a disposable daemon and the requests that drive it. Each test crate
//! uses the part it needs.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use soyeht_ptyd::{client::Client, wire::*};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Where test data folders go. A Unix socket path is limited to about a hundred bytes, which
/// the long per-user temporary folder of macOS can exceed.
static SCRATCH_ROOT: std::sync::LazyLock<PathBuf> = std::sync::LazyLock::new(|| {
    if cfg!(unix) {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    }
});

/// The interactive shell a terminal test starts.
#[cfg(unix)]
pub const SHELL: &str = "/bin/sh";
#[cfg(windows)]
pub const SHELL: &str = "cmd.exe";

/// How much of the latest output a text search keeps looking through.
const SEARCH_TAIL: usize = 0x200;

// Positive asynchronous assertions may use the full failure budget; passing
// runs stop as soon as the expected signal arrives.
pub const ASYNC_ASSERTION_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Daemon {
    pub child: Child,
    pub directory: PathBuf,
}

impl Daemon {
    pub fn start() -> (Self, Client) {
        let directory = SCRATCH_ROOT.join(format!("soyeht-next-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let child = Self::launch(&directory);
        let daemon = Self { child, directory };
        let client = daemon.connect(1);
        (daemon, client)
    }

    pub fn launch(directory: &PathBuf) -> Child {
        Command::new(env!("CARGO_BIN_EXE_soyeht-ptyd"))
            .arg("--data-dir")
            .arg(directory)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap()
    }

    pub fn connect(&self, generation: u64) -> Client {
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        loop {
            match Client::connect(&self.directory.join("ptyd.sock"), generation) {
                Ok(client) => return client,
                Err(error) => {
                    assert!(Instant::now() < deadline, "connect: {error}");
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    pub fn shell(&self) -> Spawn {
        Spawn {
            instance_key: Uuid::new_v4().to_string(),
            generation: 1,
            kind: ProcessKind::Pty,
            cwd: self.directory.to_string_lossy().into_owned(),
            argv: vec![SHELL.into()],
            env: BTreeMap::from([("PS1".into(), String::new())]),
            cols: 80,
            rows: 24,
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

pub fn spawn(client: &mut Client, config: Spawn) -> Instance {
    match client.request(Request::Spawn(config)).unwrap() {
        Response::Instance(instance) => instance,
        response => panic!("spawn failed: {response:?}"),
    }
}

pub fn input(client: &mut Client, id: &str, text: &str) {
    assert!(
        matches!(client.request(Request::Input { instance_id: id.into(), bytes: STANDARD.encode(text), operation_key:Uuid::new_v4().to_string(), fingerprint:"test".into() }).unwrap(), Response::Accepted { bytes } if bytes == text.len())
    );
}

pub fn wait_text(client: &mut Client, id: &str, from: &mut u64, needle: &str) -> String {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    let mut all = Vec::new();
    loop {
        let Response::Output(output) = client
            .request(Request::Drain {
                instance_id: id.into(),
                from_seq: *from,
            })
            .unwrap()
        else {
            panic!("expected output");
        };
        assert!(output.gap.is_none());
        *from = output.next_seq;
        all.extend(STANDARD.decode(output.bytes).unwrap());
        let text = String::from_utf8_lossy(&all).into_owned();
        if text.contains(needle) {
            return text;
        }
        if all.len() > SEARCH_TAIL {
            all.drain(..all.len() - SEARCH_TAIL);
        }
        assert!(
            Instant::now() < deadline,
            "missing {needle:?}, received {} bytes: {text:?}",
            text.len()
        );
        if output.next_seq == output.end_seq {
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Ends the instance's process twice, which must be harmless.
fn kill_twice(client: &mut Client, instance: &Instance) {
    for _ in 0..2 {
        assert!(matches!(
            client
                .request(Request::Kill {
                    instance_id: instance.instance_id.clone()
                })
                .unwrap(),
            Response::Ok
        ));
    }
}

/// Waits for the process to be reported ended, checking its retained output each time.
fn drain_until_exited(client: &mut Client, instance: &Instance, check: impl Fn(&[u8])) {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let Response::Output(output) = client
            .request(Request::Drain {
                instance_id: instance.instance_id.clone(),
                from_seq: 0,
            })
            .unwrap()
        else {
            panic!("retained output required");
        };
        check(&STANDARD.decode(output.bytes).unwrap());
        if output.exited {
            return;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
}

/// After the instance is closed its output is released: draining it is an error.
fn assert_released(client: &mut Client, instance: Instance) {
    assert!(matches!(
        client
            .request(Request::Drain {
                instance_id: instance.instance_id,
                from_seq: 0
            })
            .unwrap(),
        Response::Error { .. }
    ));
}

/// A command that printed `retained-output` and then ran on: killing it keeps what it printed
/// (checked by `check`) until the instance is closed, after which the output is released.
pub fn kill_and_release(client: &mut Client, instance: Instance, check: impl Fn(&[u8])) {
    wait_text(client, &instance.instance_id, &mut 0, "retained-output");
    kill_twice(client, &instance);
    drain_until_exited(client, &instance, check);
    close(client, &instance);
    assert_released(client, instance);
}

/// Changes the size of the instance's terminal.
pub fn resize(client: &mut Client, instance: &Instance, cols: u16, rows: u16) {
    assert!(matches!(
        client
            .request(Request::Resize {
                instance_id: instance.instance_id.clone(),
                cols,
                rows
            })
            .unwrap(),
        Response::Ok
    ));
}

/// Waits for the marker file to name a process id.
pub fn wait_for_pid(marker: &PathBuf) -> u32 {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        if let Ok(text) = fs::read_to_string(marker)
            && let Ok(pid) = text.trim().parse()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "the child never started");
        thread::sleep(Duration::from_millis(20));
    }
}

/// Waits for the process to end.
pub fn wait_until_ended(pid: u32) {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while is_running(pid) {
        assert!(Instant::now() < deadline, "process {pid} still runs");
        thread::sleep(Duration::from_millis(20));
    }
}

/// Whether a process with this id exists and has not ended.
/// Whether a process with this id exists and has not ended.
#[cfg(windows)]
pub fn is_running(pid: u32) -> bool {
    let listing = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&listing.stdout).contains(&format!("\"{pid}\""))
}

/// Whether a process with this id exists and has not ended.
#[cfg(unix)]
pub fn is_running(pid: u32) -> bool {
    let status = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let status = String::from_utf8(status.stdout).unwrap();
    !status.trim().is_empty() && !status.trim().starts_with('Z')
}

/// Waits for the instance to end and returns its exit code.
pub fn exit_code(client: &mut Client, instance: &Instance) -> Option<i32> {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let Response::Instances(instances) = client.request(Request::List).unwrap() else {
            panic!("expected a list");
        };
        if let Some(code) = instances
            .iter()
            .find(|candidate| candidate.instance_id == instance.instance_id)
            .and_then(|candidate| candidate.exit_code)
        {
            return Some(code);
        }
        assert!(Instant::now() < deadline, "the process never exited");
        thread::sleep(Duration::from_millis(20));
    }
}

pub fn close(client: &mut Client, instance: &Instance) {
    assert!(matches!(
        client
            .request(Request::Close {
                instance_id: instance.instance_id.clone()
            })
            .unwrap(),
        Response::Ok
    ));
}
