//! An upgraded profile whose panes lost their working folders still connects to the
//! supervisor: the panes that cannot be restarted are stopped, the others recover, and the
//! engine does not retry the whole reconciliation on every tick.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use std::{
    fs, thread,
    time::{Duration, Instant},
};

mod support;
use support::*;

fn wait_connected(client: &mut Client) {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while client.call("engine.status", json!({}))["ptyd"]["connected"] != true {
        assert!(
            Instant::now() < deadline,
            "the engine never connected to the supervisor"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn panes_whose_folder_is_gone_are_stopped_and_the_engine_still_connects() {
    let mut rig = Rig::new();
    rig.install_fake_harness("config");
    let mut client = rig.client();
    let gone = rig.path.join("gone-project");
    fs::create_dir(&gone).unwrap();
    let (lost_workspace, lost_terminal) = client.create_in(&gone, "lost-terminal");
    let (kept_workspace, kept_terminal) = client.create_in(&rig.path, "kept-terminal");
    let (client, lost_agent) = rig.open_installed_agent(client, &json!(lost_workspace));
    let (client, kept_agent) = rig.open_installed_agent(client, &json!(kept_workspace));

    drop(client);
    rig.kill_engine();
    rig.restart_ptyd();
    fs::remove_dir_all(&gone).unwrap();
    rig.start();

    let mut client = rig.client();
    wait_connected(&mut client);
    for pane in [&lost_terminal, &lost_agent] {
        client.wait_state(pane, "dead");
    }
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    for pane in [&kept_terminal, &kept_agent] {
        while client.pane(pane)["pid"].is_null() {
            assert!(Instant::now() < deadline, "pane {pane} was not restored");
            thread::sleep(Duration::from_millis(20));
        }
        assert_ne!(client.pane(pane)["state"], "dead");
    }
}
