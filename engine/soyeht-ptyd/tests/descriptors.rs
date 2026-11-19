//! A supervisor that has no descriptor left says so, in a kind of its own, and starts the next
//! process once a descriptor is free again.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_ptyd::{wire::*, wire_kind};
use std::{
    fs,
    os::unix::process::CommandExt as _,
    process::{Command, Stdio},
};
use uuid::Uuid;

mod support;
use support::*;

/// Few enough for a handful of terminals to use them all, and enough for the supervisor's own
/// socket, lock and log.
const LIMIT: libc::rlim_t = 40;

/// A supervisor that starts with the soft limit `soft` and, when `ceiling`, that as its hard limit
/// too, so it cannot raise its way out of it.
fn start_with(soft: libc::rlim_t, ceiling: bool) -> (Daemon, soyeht_ptyd::client::Client) {
    let directory =
        std::path::PathBuf::from("/tmp").join(format!("soyeht-next-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&directory).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_soyeht-ptyd"));
    command
        .arg("--data-dir")
        .arg(&directory)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    // SAFETY: the closure runs in the forked child before the exec and calls only `getrlimit` and
    // `setrlimit`, plain system calls that allocate nothing.
    unsafe {
        command.pre_exec(move || {
            let mut limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: `limit` is valid and writable for the call, which only fills it in.
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            limit.rlim_cur = soft;
            if ceiling {
                limit.rlim_max = soft;
            }
            // SAFETY: `limit` is valid for the call, which only reads it.
            if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let daemon = Daemon {
        child: command.spawn().unwrap(),
        directory,
    };
    let client = daemon.connect(1);
    (daemon, client)
}

fn shell_limits(flag: &str) -> String {
    let output = Command::new("sh")
        .args(["-c", &format!("ulimit {flag}")])
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn a_spawn_with_no_descriptor_left_says_so_and_the_next_one_works_after_a_close() {
    let (daemon, mut client) = start_with(LIMIT, true);
    let mut started = Vec::new();
    let refusal = loop {
        match client.request(Request::Spawn(daemon.shell())).unwrap() {
            Response::Instance(instance) => started.push(instance),
            Response::Error { kind, message } => break (kind, message),
            other => panic!("{other:?}"),
        }
        assert!(started.len() < 40, "the ceiling was never reached");
    };
    assert!(!started.is_empty(), "no terminal started at all");
    assert_eq!(refusal.0, wire_kind::TOO_MANY_OPEN_FILES, "{refusal:?}");
    assert!(refusal.1.to_lowercase().contains("too many open files"));

    // It is no failure of the supervisor: it answers, and a descriptor freed is one to start with.
    let freed = started.pop().unwrap();
    assert!(matches!(
        client
            .request(Request::Close {
                instance_id: freed.instance_id
            })
            .unwrap(),
        Response::Ok
    ));
    let again = spawn(&mut client, daemon.shell());
    assert!(!again.instance_id.is_empty());
}

#[test]
fn a_shell_under_the_supervisor_gets_the_limit_the_supervisor_started_with() {
    let (daemon, mut client) = start_with(256, false);
    let hard = shell_limits("-Hn");
    let terminal = spawn(&mut client, daemon.shell());
    input(
        &mut client,
        &terminal.instance_id,
        "echo LIMITS-$(ulimit -n)-$(ulimit -Hn)-END\n",
    );
    // The supervisor raised its own limit, which the terminal does not inherit; the ceiling is
    // untouched.
    let mut from = 0;
    wait_text(
        &mut client,
        &terminal.instance_id,
        &mut from,
        &format!("LIMITS-256-{hard}-END"),
    );
}
