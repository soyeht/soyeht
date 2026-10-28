#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use soyeht_ptyd::{client::Client, wire::*};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

mod support;
use support::*;

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
    resize(&mut client, &instance, 92, 31);
    input(&mut client, &instance.instance_id, "stty size\n");
    wait_text(&mut client, &instance.instance_id, &mut offset, "31 92");
    close(&mut client, &instance);
    close(&mut client, &instance);
    let closed = spawn(&mut client, config);
    assert!(closed.closed);
    assert_eq!(closed.instance_id, instance.instance_id);
    let Response::Instances(instances) = client.request(Request::List).unwrap() else {
        panic!("expected list");
    };
    assert!(instances.is_empty());
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
    kill_and_release(&mut client, instance, |bytes| {
        assert_eq!(bytes, b"retained-output");
    });
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
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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
    close(&mut client, &instance);
}

#[test]
fn registry_write_failure_does_not_close_a_live_process_or_advance_authority() {
    let (daemon, mut client) = Daemon::start();
    let config = daemon.shell();
    let instance = spawn(&mut client, config.clone());
    // A directory in the registry's place: the rename of a save over it fails.
    let blocked = daemon.directory.join("ptyd-registry.json");
    fs::remove_file(&blocked).unwrap();
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

fn assert_private_diagnostic(daemon: &Daemon, instance_id: &str) {
    let log = daemon
        .directory
        .join("harness-logs")
        .join(format!("{instance_id}.stderr.log"));
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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
}

#[test]
fn a_spawn_the_system_refuses_is_reported_as_spawn_refused() {
    let (daemon, mut client) = Daemon::start();
    for kind in [
        ProcessKind::Pty,
        ProcessKind::PipesAcp,
        ProcessKind::PipesLsp,
    ] {
        let mut config = daemon.shell();
        config.kind = kind;
        config.cwd = daemon.directory.join("gone").to_string_lossy().into_owned();
        assert!(matches!(
            client.request(Request::Spawn(config)).unwrap(),
            Response::Error { kind, .. } if kind == soyeht_ptyd::wire_kind::SPAWN_REFUSED
        ));
    }
    spawn(&mut client, daemon.shell());
}

#[test]
fn a_pipe_process_whose_log_folder_cannot_be_made_is_not_a_refusal() {
    let (daemon, mut client) = Daemon::start();
    let mut config = daemon.shell();
    config.kind = ProcessKind::PipesAcp;
    // The data folder is where `harness-logs` is made; the working folder is fine.
    let mut permissions = fs::metadata(&daemon.directory).unwrap().permissions();
    permissions.set_mode(0o500);
    fs::set_permissions(&daemon.directory, permissions.clone()).unwrap();
    let response = client.request(Request::Spawn(config)).unwrap();
    permissions.set_mode(0o700);
    fs::set_permissions(&daemon.directory, permissions).unwrap();
    assert!(
        matches!(&response, Response::Error { kind, .. } if kind == soyeht_ptyd::wire_kind::OPERATION_FAILED),
        "{response:?}"
    );
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
    assert_private_diagnostic(&daemon, &instance.instance_id);
    close(&mut client, &instance);
}

#[test]
fn lsp_pipes_preserve_framed_bytes_and_do_not_apply_acp_request_tracking() {
    let (daemon, mut client) = Daemon::start();
    let mut config = daemon.shell();
    config.kind = ProcessKind::PipesLsp;
    config.argv = vec!["/bin/cat".into()];
    let instance = spawn(&mut client, config.clone());
    let framed = "Content-Length: 36\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}";
    input(&mut client, &instance.instance_id, framed);
    let mut cursor = 0;
    let output = wait_text(
        &mut client,
        &instance.instance_id,
        &mut cursor,
        "Content-Length: 36",
    );
    assert_eq!(output, framed);
    drop(client);
    let mut client = daemon.connect(2);
    let adopted = spawn(&mut client, config);
    assert_eq!(adopted.instance_id, instance.instance_id);
    assert_eq!(adopted.pid, instance.pid);
    assert!(matches!(
        client
            .request(Request::PendingRequests {
                instance_id: instance.instance_id.clone()
            })
            .unwrap(),
        Response::Error { kind, .. } if kind == "wrong_process_kind"
    ));
    assert!(matches!(
        client
            .request(Request::Resize {
                instance_id: instance.instance_id,
                cols: 80,
                rows: 24
            })
            .unwrap(),
        Response::Error { kind, .. } if kind == "wrong_process_kind"
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
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
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
    assert_eq!(exit_code(&mut client, &instance), Some(7));
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

/// An adapter whose child starts its own session, as a shell tool does. The
/// child leaves the adapter's process group, so killing the group misses it.
fn adapter_with_escaped_child(daemon: &Daemon, kind: ProcessKind) -> (Spawn, PathBuf) {
    let marker = daemon.directory.join("escaped.pid");
    let mut config = daemon.shell();
    config.kind = kind;
    config.argv = vec![
        "/usr/bin/perl".into(),
        "-e".into(),
        r#"use POSIX qw(setsid);
        if (fork() == 0) {
            setsid();
            open(my $file, '>', $ARGV[0]); print $file $$; close $file;
            exec('sleep', '300');
        }
        sleep 300;"#
            .into(),
        marker.to_string_lossy().into_owned(),
    ];
    (config, marker)
}

#[test]
fn closing_an_agent_adapter_ends_children_that_left_its_process_group() {
    let (daemon, mut client) = Daemon::start();
    let (config, marker) = adapter_with_escaped_child(&daemon, ProcessKind::PipesAcp);
    let instance = spawn(&mut client, config);
    let escaped = wait_for_pid(&marker);
    assert!(is_running(escaped) && is_running(instance.pid));

    close(&mut client, &instance);

    wait_until_ended(escaped);
    wait_until_ended(instance.pid);
}

#[test]
fn closing_a_terminal_keeps_the_group_only_rule() {
    let (daemon, mut client) = Daemon::start();
    let (config, marker) = adapter_with_escaped_child(&daemon, ProcessKind::Pty);
    let instance = spawn(&mut client, config);
    let escaped = wait_for_pid(&marker);

    close(&mut client, &instance);

    wait_until_ended(instance.pid);
    // A daemon the user detached from a terminal is not the supervisor's to end.
    assert!(is_running(escaped));
    let _ = Command::new("kill").arg(escaped.to_string()).status();
}

#[test]
fn a_process_reported_as_exited_already_has_its_exit_status() {
    let (daemon, mut client) = Daemon::start();
    for expected in [0, 3] {
        let mut config = daemon.shell();
        config.instance_key = Uuid::new_v4().to_string();
        config.argv = vec!["/bin/sh".into(), "-c".into(), format!("exit {expected}")];
        let instance = spawn(&mut client, config);
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        loop {
            let Response::Output(output) = client
                .request(Request::Drain {
                    instance_id: instance.instance_id.clone(),
                    from_seq: 0,
                })
                .unwrap()
            else {
                panic!("expected output");
            };
            if output.exited {
                break;
            }
            assert!(Instant::now() < deadline, "the process never exited");
        }
        // Read at the first moment the exit is visible: no wait for the status to follow.
        let Response::Instances(instances) = client.request(Request::List).unwrap() else {
            panic!("expected a list");
        };
        let listed = instances
            .iter()
            .find(|candidate| candidate.instance_id == instance.instance_id)
            .unwrap();
        assert_eq!(listed.exit_code, Some(expected));
    }
}
