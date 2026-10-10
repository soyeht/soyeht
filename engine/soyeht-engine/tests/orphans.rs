//! An agent process that no pane, retirement ledger or harness switch owns is
//! closed when the engine reconciles with the supervisor, so a leaked adapter
//! does not live until the supervisor restarts.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_ptyd::{
    client::Client as PtyClient,
    wire::{Instance, ProcessKind, Request, Response, Spawn},
};
use std::{
    collections::BTreeMap,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

mod support;
use support::*;

fn open_instances(supervisor: &mut PtyClient) -> Vec<Instance> {
    let Response::Instances(instances) = supervisor.request(Request::List).unwrap() else {
        panic!("process registry");
    };
    instances
}

#[test]
fn an_agent_process_no_pane_owns_is_closed_when_the_engine_reconciles() {
    let mut rig = Rig::new();
    rig.kill_engine();
    let generation = rig.database_probe().engine_generation().unwrap();
    let mut supervisor = PtyClient::connect(&rig.path.join("ptyd.sock"), generation).unwrap();
    let orphan = Spawn {
        instance_key: Uuid::new_v4().to_string(),
        generation: 1,
        kind: ProcessKind::PipesAcp,
        cwd: rig.path.to_string_lossy().into_owned(),
        argv: if cfg!(windows) {
            vec![
                "cmd.exe".into(),
                "/c".into(),
                "ping -n 300 127.0.0.1 > nul".into(),
            ]
        } else {
            vec!["/bin/sh".into(), "-c".into(), "sleep 300".into()]
        },
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
    };
    let Response::Instance(orphan) = supervisor.request(Request::Spawn(orphan)).unwrap() else {
        panic!("spawn");
    };
    assert!(
        open_instances(&mut supervisor)
            .iter()
            .any(|i| i.instance_id == orphan.instance_id)
    );
    drop(supervisor);

    rig.start();

    let generation = rig.database_probe().engine_generation().unwrap();
    let mut supervisor = PtyClient::connect(&rig.path.join("ptyd.sock"), generation + 1).unwrap();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while open_instances(&mut supervisor)
        .iter()
        .any(|i| i.instance_id == orphan.instance_id)
    {
        assert!(
            Instant::now() < deadline,
            "the unowned agent process was never closed"
        );
        thread::sleep(Duration::from_millis(20));
    }
}
