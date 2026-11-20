//! Closing many processes in one request: all of them are gone, collected, when the answer comes,
//! and it comes quickly (ADR 0171).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_ptyd::{wire::*, wire_kind};
use std::time::{Duration, Instant};
use uuid::Uuid;

mod support;
use support::*;

const TERMINALS: usize = 60;

/// Whether the system still has a process by this id. An ended child the supervisor has not
/// collected is a zombie, and still answers.
fn exists(pid: u32) -> bool {
    // SAFETY: signal 0 only checks whether the process exists.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[test]
fn sixty_terminals_close_in_one_request_and_leave_no_process_behind() {
    let (daemon, mut client) = Daemon::start();
    assert!(
        client
            .features
            .iter()
            .any(|feature| feature == FEATURE_CLOSE_MANY)
    );
    let mut started: Vec<Instance> = (0..TERMINALS)
        .map(|_| spawn(&mut client, daemon.shell()))
        .collect();
    // A terminal that ignores the polite signals: only the kill ends it.
    let mut stubborn = daemon.shell();
    stubborn.argv = vec![
        "/bin/sh".into(),
        "-c".into(),
        "trap '' TERM HUP INT; echo ready; while :; do sleep 1; done".into(),
    ];
    let stubborn = spawn(&mut client, stubborn);
    wait_text(&mut client, &stubborn.instance_id, &mut 0, "ready");
    started.push(stubborn);

    let mut ids: Vec<String> = started.iter().map(|i| i.instance_id.clone()).collect();
    // A process the supervisor does not know is not a failure.
    ids.push(Uuid::new_v4().to_string());
    let asked = Instant::now();
    let answer = client
        .request(Request::CloseMany {
            instance_ids: ids.clone(),
        })
        .unwrap();
    let took = asked.elapsed();

    assert!(matches!(answer, Response::Ok), "{answer:?}");
    assert!(
        took < Duration::from_secs(1),
        "closing {TERMINALS} terminals took {took:?}"
    );
    let remaining: Vec<u32> = started
        .iter()
        .map(|i| i.pid)
        .filter(|pid| exists(*pid))
        .collect();
    assert!(
        remaining.is_empty(),
        "still there or not collected: {remaining:?}"
    );
    let Response::Instances(listed) = client.request(Request::List).unwrap() else {
        panic!("expected a list");
    };
    assert!(listed.is_empty(), "{listed:?}");
    // Closing again is not an error.
    assert!(matches!(
        client
            .request(Request::CloseMany { instance_ids: ids })
            .unwrap(),
        Response::Ok
    ));
}

#[test]
fn a_close_of_more_processes_than_the_limit_is_refused() {
    let (_daemon, mut client) = Daemon::start();
    let ids = (0..=MAX_CLOSE_MANY).map(|n| n.to_string()).collect();
    let Response::Error { kind, .. } = client
        .request(Request::CloseMany { instance_ids: ids })
        .unwrap()
    else {
        panic!("expected a refusal");
    };
    assert_eq!(kind, wire_kind::INVALID_PARAMS);
}
