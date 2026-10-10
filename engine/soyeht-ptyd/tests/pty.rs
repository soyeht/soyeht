//! The pseudo-terminal session, without the supervisor around it; the same on every system.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_ptyd::{io::Output, pty};
use std::{
    io::{Read, Write},
    process::Command,
    time::{Duration, Instant},
};

fn shell(script: &str) -> Command {
    if cfg!(windows) {
        let mut command = Command::new("cmd.exe");
        command.args(["/c", script]);
        command
    } else {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }
}

/// Reads until `needle` shows up, returning everything read.
fn read_until(output: &mut Output, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut text = Vec::new();
    let mut chunk = [0_u8; 0x1000];
    loop {
        match output.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => text.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                output.wait_readable(Duration::from_millis(100)).unwrap();
            }
            Err(error) => panic!("{error}"),
        }
        if String::from_utf8_lossy(&text).contains(needle) || Instant::now() > deadline {
            break;
        }
    }
    let text = String::from_utf8_lossy(&text).into_owned();
    assert!(text.contains(needle), "missing {needle:?} in {text:?}");
    text
}

#[test]
fn what_a_child_prints_before_it_exits_is_still_readable() {
    let mut session = pty::spawn(shell("echo printed-then-gone"), 24, 80).unwrap();
    read_until(&mut session.output, "printed-then-gone");
    assert_eq!(session.child.wait().unwrap().code(), Some(0));
}

#[test]
fn the_exit_code_of_the_child_is_reported() {
    let mut session = pty::spawn(shell("exit 7"), 24, 80).unwrap();
    assert_eq!(session.child.wait().unwrap().code(), Some(7));
}

#[test]
fn typed_input_reaches_the_child_and_a_resize_is_accepted() {
    let program = if cfg!(windows) { "cmd.exe" } else { "/bin/cat" };
    let mut session = pty::spawn(Command::new(program), 24, 80).unwrap();
    session.resizer.resize(100, 30).unwrap();
    assert!(session.resizer.resize(0, 30).is_err());
    session.input.write_all(b"typed-line\r\n").unwrap();
    read_until(&mut session.output, "typed-line");
    session.child.kill_group();
    session.child.wait().unwrap();
}

#[test]
fn a_killed_terminal_is_collected_within_a_bound_even_when_nobody_reads_its_output() {
    let program = if cfg!(windows) { "cmd.exe" } else { "/bin/cat" };
    let mut session = pty::spawn(Command::new(program), 24, 80).unwrap();
    session.input.write_all(b"unread-output\r\n").unwrap();
    session.child.kill_group();
    let exit = session
        .child
        .wait_within(std::time::Duration::from_secs(10))
        .unwrap();
    assert!(exit.is_some());
}
