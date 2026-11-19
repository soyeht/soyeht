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

fn start_with_a_ceiling() -> (Daemon, soyeht_ptyd::client::Client) {
    let directory =
        std::path::PathBuf::from("/tmp").join(format!("soyeht-next-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&directory).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_soyeht-ptyd"));
    command
        .arg("--data-dir")
        .arg(&directory)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    // SAFETY: the closure runs in the forked child before the exec and calls only `setrlimit`,
    // which is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            // The ceiling too, so the supervisor cannot raise its way out of it.
            let limit = libc::rlimit {
                rlim_cur: LIMIT,
                rlim_max: LIMIT,
            };
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

#[test]
fn a_spawn_with_no_descriptor_left_says_so_and_the_next_one_works_after_a_close() {
    let (daemon, mut client) = start_with_a_ceiling();
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
