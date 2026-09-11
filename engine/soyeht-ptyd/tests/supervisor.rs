use base64::{Engine as _, engine::general_purpose::STANDARD};
use soyeht_ptyd::{client::Client, wire::*};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

struct Daemon {
    child: Child,
    directory: PathBuf,
}

impl Daemon {
    fn start() -> (Self, Client) {
        let directory = PathBuf::from("/tmp").join(format!("soyeht-next-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let child = Self::launch(&directory);
        let daemon = Self { child, directory };
        let client = daemon.connect(1);
        (daemon, client)
    }

    fn launch(directory: &PathBuf) -> Child {
        Command::new(env!("CARGO_BIN_EXE_soyeht-ptyd"))
            .arg("--data-dir")
            .arg(directory)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap()
    }

    fn connect(&self, generation: u64) -> Client {
        let deadline = Instant::now() + Duration::from_secs(5);
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

    fn shell(&self) -> Spawn {
        Spawn {
            instance_key: Uuid::new_v4().to_string(),
            generation: 1,
            kind: ProcessKind::Pty,
            cwd: self.directory.to_string_lossy().into_owned(),
            argv: vec!["/bin/sh".into()],
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

fn spawn(client: &mut Client, config: Spawn) -> Instance {
    match client.request(Request::Spawn(config)).unwrap() {
        Response::Instance(instance) => instance,
        response => panic!("spawn failed: {response:?}"),
    }
}

fn input(client: &mut Client, id: &str, text: &str) {
    assert!(
        matches!(client.request(Request::Input { instance_id: id.into(), bytes: STANDARD.encode(text), operation_key:Uuid::new_v4().to_string(), fingerprint:"test".into() }).unwrap(), Response::Accepted { bytes } if bytes == text.len())
    );
}

fn wait_text(client: &mut Client, id: &str, from: &mut u64, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
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
        if all.len() > 512 {
            all.drain(..all.len() - 512);
        }
        assert!(
            Instant::now() < deadline,
            "missing {needle:?}, received {} bytes",
            text.len()
        );
        if output.next_seq == output.end_seq {
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn reconnect_adopts_the_same_shell_and_spawn_and_close_are_idempotent() {
    let (daemon, mut client) = Daemon::start();
    let config = daemon.shell();
    let instance = spawn(&mut client, config.clone());
    let repeated = spawn(&mut client, config.clone());
    assert_eq!(instance.instance_id, repeated.instance_id);
    assert_eq!(instance.pid, repeated.pid);
    input(&mut client, &instance.instance_id, "stty -echo\n");
    input(
        &mut client,
        &instance.instance_id,
        "printf 'first:%s\\n' $$\n",
    );
    let mut offset = 0;
    wait_text(
        &mut client,
        &instance.instance_id,
        &mut offset,
        &format!("first:{}", instance.pid),
    );
    drop(client);
    let mut client = daemon.connect(2);
    let adopted = spawn(&mut client, config.clone());
    assert_eq!(adopted.pid, instance.pid);
    assert!(matches!(
        client
            .request(Request::Resize {
                instance_id: instance.instance_id.clone(),
                cols: 92,
                rows: 31
            })
            .unwrap(),
        Response::Ok
    ));
    input(&mut client, &instance.instance_id, "stty size\n");
    wait_text(&mut client, &instance.instance_id, &mut offset, "31 92");
    for _ in 0..2 {
        assert!(matches!(
            client
                .request(Request::Close {
                    instance_id: instance.instance_id.clone()
                })
                .unwrap(),
            Response::Ok
        ));
    }
    let closed = spawn(&mut client, config);
    assert!(closed.closed);
    assert_eq!(closed.instance_id, instance.instance_id);
    let Response::Instances(instances) = client.request(Request::List).unwrap() else {
        panic!("expected list");
    };
    assert_eq!(instances.len(), 1);
    assert_eq!(
        fs::metadata(daemon.directory.join("ptyd.sock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&daemon.directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn killing_a_command_preserves_its_output_until_explicit_release() {
    let (daemon, mut client) = Daemon::start();
    let mut config = daemon.shell();
    config.argv = vec![
        "/bin/sh".into(),
        "-c".into(),
        "printf retained-output; sleep 30".into(),
    ];
    let instance = spawn(&mut client, config);
    wait_text(
        &mut client,
        &instance.instance_id,
        &mut 0,
        "retained-output",
    );
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
    let deadline = Instant::now() + Duration::from_secs(5);
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
        assert_eq!(STANDARD.decode(output.bytes).unwrap(), b"retained-output");
        if output.exited {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(
        client
            .request(Request::Close {
                instance_id: instance.instance_id.clone()
            })
            .unwrap(),
        Response::Ok
    ));
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

#[test]
fn generation_fencing_and_registry_survive_daemon_restart() {
    let (mut daemon, mut old) = Daemon::start();
    let config = daemon.shell();
    let instance = spawn(&mut old, config.clone());
    let supervisor_id = old.supervisor_id.clone();
    assert!(Client::connect(&daemon.directory.join("ptyd.sock"), 0).is_err());
    assert!(Client::connect(&daemon.directory.join("ptyd.sock"), 1).is_err());
    let mut newer = daemon.connect(2);
    assert!(old.request(Request::List).is_err());
    assert!(matches!(
        newer
            .request(Request::Close {
                instance_id: instance.instance_id.clone()
            })
            .unwrap(),
        Response::Ok
    ));
    drop(newer);
    daemon.child.kill().unwrap();
    daemon.child.wait().unwrap();
    daemon.child = Daemon::launch(&daemon.directory);
    let mut newer = daemon.connect(3);
    assert_ne!(supervisor_id, newer.supervisor_id);
    let closed = spawn(&mut newer, config);
    assert!(closed.closed);
    assert_eq!(closed.instance_id, instance.instance_id);
    assert!(Client::connect(&daemon.directory.join("ptyd.sock"), 2).is_err());
}

#[test]
fn output_overflow_is_explicit_and_does_not_block_the_child() {
    let (daemon, mut client) = Daemon::start();
    let mut config = daemon.shell();
    config.argv = vec![
        "/bin/sh".into(),
        "-c".into(),
        "head -c 9437184 /dev/zero; printf 'DONE'; sleep 30".into(),
    ];
    let instance = spawn(&mut client, config);
    drop(client);
    thread::sleep(Duration::from_millis(500));
    let mut client = daemon.connect(2);
    let deadline = Instant::now() + Duration::from_secs(10);
    let gap = loop {
        let Response::Output(output) = client
            .request(Request::Drain {
                instance_id: instance.instance_id.clone(),
                from_seq: 0,
            })
            .unwrap()
        else {
            panic!("expected output");
        };
        if let Some(gap) = output.gap
            && gap.to_seq >= 1_048_580
        {
            break gap;
        }
        assert!(
            Instant::now() < deadline,
            "retention did not overflow: from={} next={} exited={} sample={:?}",
            output.from_seq,
            output.end_seq,
            output.exited,
            &STANDARD.decode(output.bytes).unwrap()
                [..64.min((output.next_seq - output.from_seq) as usize)]
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(gap.from_seq, 0);
    assert!(gap.to_seq > 0);
    // Reach the marker emitted after more than the retention limit: the child
    // kept writing while no engine was connected.
    let mut from = gap.to_seq;
    wait_text(&mut client, &instance.instance_id, &mut from, "DONE");
    assert!(matches!(
        client
            .request(Request::Ack {
                instance_id: instance.instance_id.clone(),
                seq: from
            })
            .unwrap(),
        Response::Ok
    ));
    assert!(matches!(
        client
            .request(Request::Close {
                instance_id: instance.instance_id
            })
            .unwrap(),
        Response::Ok
    ));
}

#[test]
fn second_supervisor_cannot_replace_the_live_socket() {
    let (daemon, mut client) = Daemon::start();
    let mut second = Daemon::launch(&daemon.directory);
    let status = second.wait().unwrap();
    assert!(!status.success());
    assert!(matches!(
        client.request(Request::List).unwrap(),
        Response::Instances(_)
    ));
}

#[test]
fn registry_write_failure_does_not_close_a_live_process_or_advance_authority() {
    let (daemon, mut client) = Daemon::start();
    let config = daemon.shell();
    let instance = spawn(&mut client, config.clone());
    let blocked = daemon.directory.join("ptyd-registry.new");
    fs::create_dir(&blocked).unwrap();
    assert!(matches!(
        client
            .request(Request::Close {
                instance_id: instance.instance_id.clone()
            })
            .unwrap(),
        Response::Error { .. }
    ));
    assert!(!spawn(&mut client, config).closed);
    assert!(Client::connect(&daemon.directory.join("ptyd.sock"), 2).is_err());
    input(
        &mut client,
        &instance.instance_id,
        "printf 'still:%s\\n' $$\n",
    );
    wait_text(
        &mut client,
        &instance.instance_id,
        &mut 0,
        &format!("still:{}", instance.pid),
    );
    fs::remove_dir(blocked).unwrap();
    let mut newer = daemon.connect(2);
    assert!(matches!(
        newer
            .request(Request::Close {
                instance_id: instance.instance_id
            })
            .unwrap(),
        Response::Ok
    ));
}

#[test]
fn pipes_recover_native_requests_after_ack_and_reconnect_without_echoing_stdin() {
    let (daemon, mut client) = Daemon::start();
    let mut config = daemon.shell();
    config.kind = ProcessKind::PipesAcp;
    config.argv = vec!["/bin/sh".into(), "-c".into(), r#"
        [ ! -t 0 ] && [ ! -t 1 ] || exit 2
        printf '%s\n' '{"jsonrpc":"2.0","id":"native-1","method":"session/request_permission","params":{"options":[]}}'
        IFS= read -r response
        printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"received":true}}'
        printf 'private diagnostic\n' >&2
        sleep 30
    "#.into()];
    let instance = spawn(&mut client, config.clone());
    let mut cursor = 0;
    let output = wait_text(
        &mut client,
        &instance.instance_id,
        &mut cursor,
        "request_permission",
    );
    assert!(!output.contains('\r'));
    assert!(matches!(
        client
            .request(Request::Ack {
                instance_id: instance.instance_id.clone(),
                seq: cursor
            })
            .unwrap(),
        Response::Ok
    ));
    drop(client);
    let mut client = daemon.connect(2);
    assert_eq!(spawn(&mut client, config).pid, instance.pid);
    let Response::PendingRequests {
        items,
        recovery_gap,
    } = client
        .request(Request::PendingRequests {
            instance_id: instance.instance_id.clone(),
        })
        .unwrap()
    else {
        panic!("expected pending requests");
    };
    assert!(!recovery_gap);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].to_seq, cursor);
    let Response::PendingRequestChunk(chunk) = client
        .request(Request::PendingRequestChunk {
            instance_id: instance.instance_id.clone(),
            request_key: items[0].request_key,
            offset: 0,
        })
        .unwrap()
    else {
        panic!("expected chunk");
    };
    let native: serde_json::Value =
        serde_json::from_slice(&STANDARD.decode(chunk.bytes).unwrap()).unwrap();
    assert_eq!(native["id"], "native-1");
    let response = "{\"jsonrpc\":\"2.0\",\"id\":\"native-1\",\"result\":{}}";
    input(&mut client, &instance.instance_id, response);
    let Response::PendingRequests { items, .. } = client
        .request(Request::PendingRequests {
            instance_id: instance.instance_id.clone(),
        })
        .unwrap()
    else {
        panic!("expected requests");
    };
    assert_eq!(items.len(), 1, "partial input cannot answer the request");
    input(&mut client, &instance.instance_id, "\n");
    let output = wait_text(&mut client, &instance.instance_id, &mut cursor, "received");
    assert!(
        !output.contains("native-1"),
        "pipes do not echo engine responses"
    );
    assert!(
        !output.contains("private diagnostic"),
        "stderr cannot corrupt ACP"
    );
    let Response::PendingRequests {
        items,
        recovery_gap,
    } = client
        .request(Request::PendingRequests {
            instance_id: instance.instance_id.clone(),
        })
        .unwrap()
    else {
        panic!("expected requests");
    };
    assert!(items.is_empty());
    assert!(!recovery_gap);
    assert!(
        matches!(client.request(Request::Resize { instance_id: instance.instance_id.clone(), cols: 80, rows: 24 }).unwrap(), Response::Error { kind, .. } if kind == "wrong_process_kind")
    );
    let log = daemon
        .directory
        .join("harness-logs")
        .join(format!("{}.stderr.log", instance.instance_id));
    let deadline = Instant::now() + Duration::from_secs(3);
    while !fs::read_to_string(&log)
        .unwrap()
        .contains("private diagnostic")
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fs::metadata(log).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(matches!(
        client
            .request(Request::Close {
                instance_id: instance.instance_id
            })
            .unwrap(),
        Response::Ok
    ));
}

#[test]
fn killing_ptyd_kills_a_pipe_harness_that_ignores_stdin_eof() {
    let (mut daemon, mut client) = Daemon::start();
    let mut config = daemon.shell();
    config.kind = ProcessKind::PipesAcp;
    config.argv = vec![
        "/bin/sh".into(),
        "-c".into(),
        "trap '' HUP; while :; do sleep 1; done".into(),
    ];
    let instance = spawn(&mut client, config);
    let group = Command::new("ps")
        .args(["-o", "pgid=", "-p", &instance.pid.to_string()])
        .output()
        .unwrap();
    let guardian: u32 = String::from_utf8(group.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_ne!(guardian, instance.pid);
    daemon.child.kill().unwrap();
    daemon.child.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    for pid in [instance.pid, guardian] {
        loop {
            let status = Command::new("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            let state = String::from_utf8(status.stdout).unwrap();
            if state.trim().is_empty() || state.trim().starts_with('Z') {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "pipe process {pid} survived ptyd: {state}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn pipe_guard_preserves_native_exit_code_and_reports_spawn_failure() {
    let (daemon, mut client) = Daemon::start();
    let mut config = daemon.shell();
    config.kind = ProcessKind::PipesAcp;
    config.argv = vec!["/bin/sh".into(), "-c".into(), "exit 7".into()];
    let instance = spawn(&mut client, config.clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let Response::Instances(instances) = client.request(Request::List).unwrap() else {
            panic!("expected list");
        };
        if let Some(code) = instances
            .iter()
            .find(|item| item.instance_id == instance.instance_id)
            .unwrap()
            .exit_code
        {
            assert_eq!(code, 7);
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    config.instance_key = Uuid::new_v4().to_string();
    config.argv = vec![
        daemon
            .directory
            .join("missing-program")
            .to_string_lossy()
            .into_owned(),
    ];
    assert!(matches!(
        client.request(Request::Spawn(config)).unwrap(),
        Response::Error { .. }
    ));
}
