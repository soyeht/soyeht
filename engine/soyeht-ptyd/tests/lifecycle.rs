//! Starting, replacing and fencing the supervisor; the same on every system.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_ptyd::{client::Client, wire::*};

mod support;
use support::*;

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
