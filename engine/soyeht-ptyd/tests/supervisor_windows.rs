//! The supervisor on Windows: terminals on a pseudo console, pipe processes in a job.
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_ptyd::wire::*;
use std::path::PathBuf;

mod support;
use support::*;

fn command(daemon: &Daemon, kind: ProcessKind, argv: &[&str]) -> Spawn {
    let mut config = daemon.shell();
    config.kind = kind;
    config.argv = argv.iter().map(|argument| (*argument).into()).collect();
    config
}

#[test]
fn a_terminal_keeps_its_output_and_reports_the_exit_code() {
    let (daemon, mut client) = Daemon::start();
    let config = command(
        &daemon,
        ProcessKind::Pty,
        &["cmd.exe", "/c", "echo retained-output & exit 7"],
    );
    let instance = spawn(&mut client, config);
    wait_text(
        &mut client,
        &instance.instance_id,
        &mut 0,
        "retained-output",
    );
    assert_eq!(exit_code(&mut client, &instance), Some(7));
    close(&mut client, &instance);
}

#[test]
fn typed_input_and_a_resize_reach_the_terminal() {
    let (daemon, mut client) = Daemon::start();
    let instance = spawn(&mut client, daemon.shell());
    let mut offset = 0;
    input(&mut client, &instance.instance_id, "echo first-line\r\n");
    wait_text(
        &mut client,
        &instance.instance_id,
        &mut offset,
        "first-line",
    );
    resize(&mut client, &instance, 92, 31);
    input(&mut client, &instance.instance_id, "mode con\r\n");
    wait_text(&mut client, &instance.instance_id, &mut offset, "92");
    input(&mut client, &instance.instance_id, "exit 3\r\n");
    assert_eq!(exit_code(&mut client, &instance), Some(3));
    close(&mut client, &instance);
}

#[test]
fn killing_a_command_preserves_its_output_until_explicit_release() {
    let (daemon, mut client) = Daemon::start();
    let config = command(
        &daemon,
        ProcessKind::Pty,
        &[
            "cmd.exe",
            "/c",
            "echo retained-output & ping -n 60 127.0.0.1 > nul",
        ],
    );
    let instance = spawn(&mut client, config);
    // The terminal adds control sequences around the text, so only its presence is checked.
    kill_and_release(&mut client, instance, |bytes| {
        assert!(String::from_utf8_lossy(bytes).contains("retained-output"));
    });
}

/// A pipe process that starts a child of its own, as an agent's shell tool does, and records the
/// child's id in a file.
fn adapter_with_child(daemon: &Daemon) -> (Spawn, PathBuf) {
    let marker = daemon.directory.join("child.pid");
    let script = format!(
        "$p = Start-Process -PassThru -WindowStyle Hidden ping -ArgumentList '-n','300','127.0.0.1'; \
         Set-Content -NoNewline -Path '{}' -Value $p.Id; Start-Sleep 300",
        marker.display()
    );
    let config = command(
        daemon,
        ProcessKind::PipesAcp,
        &[
            "powershell.exe",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ],
    );
    (config, marker)
}

#[test]
fn closing_a_pipe_process_ends_the_children_it_started() {
    let (daemon, mut client) = Daemon::start();
    let (config, marker) = adapter_with_child(&daemon);
    let instance = spawn(&mut client, config);
    let child = wait_for_pid(&marker);
    assert!(is_running(child) && is_running(instance.pid));
    close(&mut client, &instance);
    wait_until_ended(child);
    wait_until_ended(instance.pid);
}

#[test]
fn killing_the_supervisor_ends_a_pipe_process_and_its_children() {
    let (mut daemon, mut client) = Daemon::start();
    let (config, marker) = adapter_with_child(&daemon);
    let instance = spawn(&mut client, config);
    let child = wait_for_pid(&marker);
    drop(client);
    daemon.child.kill().unwrap();
    daemon.child.wait().unwrap();
    wait_until_ended(child);
    wait_until_ended(instance.pid);
}

#[test]
fn a_pipe_process_reports_its_exit_code_and_a_missing_program_fails_cleanly() {
    let (daemon, mut client) = Daemon::start();
    let config = command(&daemon, ProcessKind::PipesAcp, &["cmd.exe", "/c", "exit 7"]);
    let instance = spawn(&mut client, config);
    assert_eq!(exit_code(&mut client, &instance), Some(7));
    close(&mut client, &instance);

    let missing = command(
        &daemon,
        ProcessKind::PipesAcp,
        &["soyeht-no-such-program.exe"],
    );
    assert!(matches!(
        client.request(Request::Spawn(missing)).unwrap(),
        Response::Error { .. }
    ));
}

/// A batch file that prints its first argument between brackets, quoted so that its own
/// `echo` reads the argument's characters as text too.
fn echo_script(daemon: &Daemon) -> String {
    let script = daemon.directory.join("echo.cmd");
    std::fs::write(&script, "@echo off\r\necho \"[%~1]\"\r\n").unwrap();
    script.to_string_lossy().into_owned()
}

#[test]
fn a_batch_file_gets_command_characters_as_plain_text() {
    let (daemon, mut client) = Daemon::start();
    let script = echo_script(&daemon);
    let config = command(
        &daemon,
        ProcessKind::Pty,
        &[&script, "a&b|c & echo INJECTED"],
    );
    let instance = spawn(&mut client, config);
    wait_text(
        &mut client,
        &instance.instance_id,
        &mut 0,
        "[a&b|c & echo INJECTED]",
    );
    close(&mut client, &instance);
}

#[test]
fn a_batch_file_refuses_a_quote_or_a_percent_sign() {
    let (daemon, mut client) = Daemon::start();
    let script = echo_script(&daemon);
    for argument in ["say \"hi\"", "%PATH%"] {
        let config = command(&daemon, ProcessKind::Pty, &[&script, argument]);
        assert!(matches!(
            client.request(Request::Spawn(config)).unwrap(),
            Response::Error { .. }
        ));
    }
}
