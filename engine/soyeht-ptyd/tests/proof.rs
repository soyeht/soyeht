//! Who may talk to the supervisor, and how far a generation may move it (ADR 0160): a process
//! that cannot read the secret file is refused before it can start or read anything, a refused
//! hello leaves the live engine and the registry as they were, and a jump past the step is
//! refused. The restarts that the real engine goes through still reconnect.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::local::connect;
use soyeht_protocol::frame::write_json;
use soyeht_ptyd::{
    Error,
    client::{Client, Watcher},
    wire::*,
    wire_kind,
};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

mod support;
use support::*;

const QUIET: Duration = Duration::from_millis(600);

fn socket(daemon: &Daemon) -> PathBuf {
    daemon.directory.join("ptyd.sock")
}

fn registry(daemon: &Daemon) -> Vec<u8> {
    fs::read(daemon.directory.join("ptyd-registry.json")).unwrap()
}

fn recorded_generation(daemon: &Daemon) -> u64 {
    let registry: serde_json::Value = serde_json::from_slice(&registry(daemon)).unwrap();
    registry["engine_generation"].as_u64().unwrap()
}

fn hello(generation: u64) -> Request {
    Request::Hello {
        version: VERSION,
        engine_generation: generation,
        role: Role::Request,
    }
}

/// A command that leaves `marker` behind when something runs it.
fn touch(daemon: &Daemon, marker: &Path) -> Spawn {
    Spawn {
        argv: vec![
            "/bin/sh".into(),
            "-c".into(),
            format!("touch {}", marker.display()),
        ],
        ..daemon.shell()
    }
}

/// Everything the supervisor says to a connection until it closes it or `QUIET` passes.
fn heard(mut stream: soyeht_platform::local::Stream) -> Vec<u8> {
    stream.set_read_timeout(Some(QUIET)).unwrap();
    let mut heard = Vec::new();
    // A reset is as good as a close: the supervisor said nothing and ended the connection.
    drop(stream.read_to_end(&mut heard));
    heard
}

fn assert_still_serving(client: &mut Client) {
    assert!(matches!(
        client.request(Request::List).unwrap(),
        Response::Instances(_)
    ));
}

#[test]
fn a_process_without_the_secret_is_refused_before_it_can_start_or_read_anything() {
    let (daemon, mut engine) = Daemon::start();
    let own = spawn(&mut engine, daemon.shell());
    let marker = daemon.directory.join("intruder-ran");

    // It reaches the socket, which is open to the user, and speaks the protocol from the first
    // frame: a hello that would take over, a command to run and a read of the engine's terminal.
    let mut intruder = connect(&socket(&daemon)).unwrap();
    for request in [
        hello(2),
        Request::Spawn(touch(&daemon, &marker)),
        Request::Drain {
            instance_id: own.instance_id.clone(),
            from_seq: 0,
        },
    ] {
        // The supervisor may have hung up already.
        drop(write_json(&mut intruder, &request));
    }

    // What a listener says to a nonce, which it takes the first bytes for: its own nonce and its
    // proof, which tell a stranger nothing. Never a welcome or a list.
    let answer = heard(intruder);
    assert!(answer.len() <= 64, "the supervisor said more: {answer:?}");
    thread::sleep(QUIET);
    assert!(!marker.exists(), "a command of the intruder ran");
    // The engine was not touched, and the intruder is not in the list.
    assert_still_serving(&mut engine);
    let Response::Instances(instances) = engine.request(Request::List).unwrap() else {
        panic!("not a list");
    };
    assert_eq!(instances.len(), 1);
    assert_eq!(recorded_generation(&daemon), 1);
}

#[test]
fn a_wrong_proof_changes_nothing_and_does_not_close_the_live_engine() {
    let (daemon, mut engine) = Daemon::start();
    let own = spawn(&mut engine, daemon.shell());
    let before = registry(&daemon);

    // It follows the exchange and answers with a proof made under a secret of its own, then
    // sends the hello that would raise the generation.
    let mut forger = connect(&socket(&daemon)).unwrap();
    forger
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    forger.write_all(&[0; 32]).unwrap();
    let mut reply = [0; 64];
    forger.read_exact(&mut reply).unwrap();
    forger.write_all(&[7; 32]).unwrap();
    drop(write_json(&mut forger, &hello(2)));
    assert!(heard(forger).is_empty(), "the supervisor answered");

    // The client's own code with a secret file that is not this supervisor's: the supervisor
    // proves nothing under that secret, so the client refuses it (a request and a watch), and
    // the live connection is not closed.
    let secret = daemon.directory.join("ptyd.sock.secret");
    let real = fs::read(&secret).unwrap();
    fs::write(
        &secret,
        real.iter().map(|byte| byte ^ 1).collect::<Vec<_>>(),
    )
    .unwrap();
    fs::set_permissions(&secret, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    assert!(Client::connect(&socket(&daemon), 2).is_err());
    assert!(Watcher::connect(&socket(&daemon), 1).is_err());
    fs::write(&secret, &real).unwrap();
    fs::set_permissions(&secret, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();

    assert_eq!(registry(&daemon), before);
    assert_still_serving(&mut engine);
    // The engine's terminal still takes input.
    input(&mut engine, &own.instance_id, "printf alive\n");
    let mut offset = 0;
    wait_text(&mut engine, &own.instance_id, &mut offset, "alive");
}

#[test]
fn a_generation_past_the_step_is_refused_and_one_at_the_step_is_not() {
    let (daemon, mut engine) = Daemon::start();
    let before = registry(&daemon);

    for generation in [1 + MAX_GENERATION_STEP + 1, 100, u64::MAX] {
        match Client::connect(&socket(&daemon), generation) {
            Err(Error::Remote { kind, .. }) => assert_eq!(kind, wire_kind::GENERATION_JUMP),
            other => panic!("generation {generation}: {:?}", other.err()),
        }
        // The same hello of a watch is turned away the same way.
        assert!(matches!(
            Watcher::connect(&socket(&daemon), generation),
            Err(Error::Remote { ref kind, .. }) if kind == wire_kind::GENERATION_JUMP
        ));
        // Nothing was saved and nothing was closed.
        assert_eq!(registry(&daemon), before);
        assert_still_serving(&mut engine);
    }

    // The last number the step allows is an engine that restarted that many times without
    // reaching the supervisor, and it takes over.
    let allowed = 1 + MAX_GENERATION_STEP;
    let mut newer = daemon.connect(allowed);
    assert_eq!(recorded_generation(&daemon), allowed);
    assert_still_serving(&mut newer);
    assert!(engine.request(Request::List).is_err());
}

#[test]
fn the_first_engine_a_supervisor_meets_is_not_held_to_the_step() {
    let (mut daemon, first) = Daemon::start();
    drop(first);
    // Starting over: a registry that recorded no engine has nothing to be ahead of.
    daemon.child.kill().unwrap();
    daemon.child.wait().unwrap();
    fs::remove_file(daemon.directory.join("ptyd-registry.json")).unwrap();
    daemon.child = Daemon::launch(&daemon.directory);
    let mut engine = daemon.connect(500);
    assert_still_serving(&mut engine);
    assert_eq!(recorded_generation(&daemon), 500);
    // From there it holds.
    assert!(matches!(
        Client::connect(&socket(&daemon), 500 + MAX_GENERATION_STEP + 1),
        Err(Error::Remote { ref kind, .. }) if kind == wire_kind::GENERATION_JUMP
    ));
}

#[test]
fn the_engine_reconnects_after_a_restart_of_the_supervisor_and_of_itself() {
    let (mut daemon, engine) = Daemon::start();
    let secret = daemon.directory.join("ptyd.sock.secret");
    let first_secret = fs::read(&secret).unwrap();
    assert_eq!(
        fs::metadata(&secret).unwrap().permissions().mode_bits(),
        0o600
    );

    // The engine restarts: it presents the next number, as its database gives it.
    drop(engine);
    let mut engine = daemon.connect(2);
    assert_still_serving(&mut engine);

    // The supervisor restarts: it kills nothing it was not asked to, writes a new secret, and
    // the engine, which reads the file at each connection, is let in without being told.
    drop(engine);
    daemon.child.kill().unwrap();
    daemon.child.wait().unwrap();
    daemon.child = Daemon::launch(&daemon.directory);
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while fs::read(&secret).is_ok_and(|current| current == first_secret) {
        assert!(Instant::now() < deadline, "no new secret");
        thread::sleep(Duration::from_millis(10));
    }
    let mut engine = daemon.connect(3);
    assert_still_serving(&mut engine);
    assert_eq!(recorded_generation(&daemon), 3);
}

trait ModeBits {
    fn mode_bits(&self) -> u32;
}

impl ModeBits for fs::Permissions {
    fn mode_bits(&self) -> u32 {
        std::os::unix::fs::PermissionsExt::mode(self) & 0o777
    }
}
