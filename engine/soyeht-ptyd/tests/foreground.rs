//! What a terminal runs in its foreground, asked of a real supervisor (ADR 0165).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_ptyd::{
    client::Client,
    wire::{FEATURE_FOREGROUND, Foreground, Request, Response},
};
use std::{
    thread,
    time::{Duration, Instant},
};

mod support;
use support::*;

fn foreground_of(client: &mut Client, instance_ids: &[&str]) -> Vec<Foreground> {
    let Response::Foregrounds(found) = client
        .request(Request::Foreground {
            instance_ids: instance_ids.iter().map(|id| (*id).to_owned()).collect(),
        })
        .unwrap()
    else {
        panic!("expected foregrounds");
    };
    found
}

/// The command the terminal ends up running: a long wait, so the test reads it while it runs.
fn waiting_command(daemon: &Daemon) -> soyeht_ptyd::wire::Spawn {
    let mut config = daemon.shell();
    config.argv = if cfg!(windows) {
        ["cmd.exe", "/c", "ping -n 60 127.0.0.1"]
    } else {
        ["/bin/sh", "-c", "exec sleep 60"]
    }
    .map(String::from)
    .to_vec();
    config
}

#[test]
fn a_terminal_reports_the_program_it_runs_and_an_unknown_one_is_left_out() {
    let (daemon, mut client) = Daemon::start();
    let instance = spawn(&mut client, waiting_command(&daemon));
    let wanted = if cfg!(windows) { "ping" } else { "sleep" };
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    let programs = loop {
        let found = foreground_of(&mut client, &[&instance.instance_id, "not-an-instance"]);
        assert!(
            found.len() <= 1,
            "an unknown instance was answered: {found:?}"
        );
        let ours = found
            .into_iter()
            .find(|foreground| foreground.instance_id == instance.instance_id)
            .map(|foreground| foreground.programs)
            .unwrap_or_default();
        // A program that has only just been started may show its file before its arguments (Linux
        // reads them from different places), so a Unix program counts once it has both.
        let running = ours.iter().any(|program| {
            (cfg!(windows) || !program.arguments.is_empty())
                && program.executable.as_deref().is_some_and(|path| {
                    let path = path.to_ascii_lowercase();
                    path.ends_with(wanted) || path.ends_with(&format!("{wanted}.exe"))
                })
        });
        if running {
            break ours;
        }
        assert!(
            Instant::now() < deadline,
            "the terminal never ran {wanted}: {ours:?}"
        );
        thread::sleep(Duration::from_millis(50));
    };
    if cfg!(unix) {
        // Only the leader of the foreground group, with the arguments it was started with.
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].pid, instance.pid);
        assert_eq!(programs[0].arguments, ["sleep", "60"]);
        assert_eq!(programs[0].parent_pid, None);
    } else {
        // The job also holds the command interpreter that started it.
        let ping = programs
            .iter()
            .find(|program| {
                program
                    .executable
                    .as_deref()
                    .is_some_and(|path| path.to_ascii_lowercase().ends_with("ping.exe"))
            })
            .unwrap();
        assert_eq!(ping.parent_pid, Some(instance.pid));
    }
    close(&mut client, &instance);
    // Closed, the terminal is no longer asked about.
    assert!(foreground_of(&mut client, &[&instance.instance_id]).is_empty());
}

#[test]
fn the_welcome_lists_the_foreground_feature_so_an_engine_may_ask() {
    let (_daemon, client) = Daemon::start();
    assert!(
        client
            .features
            .iter()
            .any(|name| name == FEATURE_FOREGROUND)
    );
}

#[test]
fn a_question_about_too_many_terminals_is_refused() {
    let (_daemon, mut client) = Daemon::start();
    let ids: Vec<String> = (0..=soyeht_ptyd::wire::MAX_FOREGROUND)
        .map(|number| number.to_string())
        .collect();
    let response = client
        .request(Request::Foreground { instance_ids: ids })
        .unwrap();
    assert!(matches!(response, Response::Error { .. }));
}
