//! The notice of a device added and the list of the panes a device made (ADR 0164), through the
//! engine's own dispatch.

use super::{
    code_tests::{Rig, code_of, decide, kind, pair_with, phone_address},
    *,
};
use crate::{
    ports::{Clock, SystemClock},
    test_hooks::{FIXTURE_PHONE_NODE, fixture_node_name},
};
use soyeht_protocol::{Actor, ErrorKind};
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

/// The system's time moved forward by what a test says, so that two devices are added a few seconds
/// apart, which the notice orders them by.
struct Moved(AtomicI64);

impl Clock for Moved {
    fn now_ms(&self) -> i64 {
        SystemClock.now_ms() + self.0.load(Ordering::SeqCst) * 1_000
    }
}

fn move_clock(rig: &mut Rig, seconds: i64) {
    let clock = Arc::new(Moved(AtomicI64::new(seconds)));
    rig.core.clock = clock;
}

/// A connection that has said `hello` as the paired device `id`.
fn as_device(rig: &mut Rig, id: &str) -> String {
    let (outbox, receiver) = crate::outbox::channel();
    // The test never reads this connection's notifications.
    std::mem::forget(receiver);
    let phone = rig.core.connect(false, false, outbox).unwrap();
    rig.core.clients.get_mut(&phone).unwrap().scope = format!("device:{id}");
    rig.core.clients.get_mut(&phone).unwrap().actor = Some(Actor {
        role: Role::HumanDevice,
        client: "phone".into(),
        device_id: Some(id.into()),
        pane_id: None,
    });
    phone
}

/// Pairs a device and approves it with the right code; the new device's record.
fn paired(rig: &mut Rig, app: &str, seed: u8, name: &str) -> Value {
    let window = rig.open_window(app);
    let device = pair_with(rig, &window, seed, name);
    let code = code_of(&device);
    decide(rig, app, &device.request, Some(&code)).unwrap();
    let listed = rig.call(app, "device.list", json!({})).unwrap();
    listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap()
        .clone()
}

fn noticed(rig: &mut Rig, connection: &str) -> Vec<String> {
    let listed = rig
        .call(connection, "device.notice.list", json!({}))
        .unwrap();
    let devices: Vec<Device> = serde_json::from_value(listed["items"].clone()).unwrap();
    devices.into_iter().map(|device| device.name).collect()
}

#[tokio::test]
async fn a_device_paired_where_the_code_is_asked_for_is_dated_and_names_its_node() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let before = rig.core.now_unix();
    let device = paired(&mut rig, &app, 41, "Test phone");
    let at = device["addedAt"].as_str().unwrap();
    let at = chrono::DateTime::parse_from_rfc3339(at)
        .unwrap()
        .timestamp();
    assert!((before..=rig.core.now_unix()).contains(&at), "{device}");
    assert_eq!(device["node"], fixture_node_name(FIXTURE_PHONE_NODE));
    // The event a live client hears carries the same fields.
    let events: Vec<Value> = rig
        .core
        .store
        .replay(0)
        .unwrap()
        .1
        .into_iter()
        .filter(|event| event.method == "device.paired")
        .map(|event| event.params)
        .collect();
    assert_eq!(events[0]["device"]["addedAt"], device["addedAt"]);
}

#[tokio::test]
async fn a_pairing_with_a_mac_records_no_date_and_no_node() {
    let mut rig = Rig::new(Approval::SignedApp);
    let app = rig.connect(true, true);
    let window = rig.open_window(&app);
    let device = pair_with(&mut rig, &window, 42, "Test phone");
    decide(&mut rig, &app, &device.request, None).unwrap();
    let listed = rig.call(&app, "device.list", json!({})).unwrap();
    let item = &listed["items"][0];
    assert_eq!(item["name"], "Test phone");
    assert!(
        item.get("addedAt").is_none() && item.get("node").is_none(),
        "{item}"
    );
    // So the Mac has nothing to tell anyone.
    assert!(noticed(&mut rig, &app).is_empty());
}

#[tokio::test]
async fn the_notice_lists_what_was_added_since_it_was_dismissed_and_each_viewer_has_its_own() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let first = paired(&mut rig, &app, 43, "First phone");
    // The person at the computer is told, and so is nobody else yet.
    assert_eq!(noticed(&mut rig, &app), ["First phone"]);
    let first_id = first["deviceId"].as_str().unwrap().to_owned();
    let first_phone = as_device(&mut rig, &first_id);
    assert!(noticed(&mut rig, &first_phone).is_empty());

    // A second device, from another node, a few seconds later.
    move_clock(&mut rig, 5);
    rig.peers.set_node(phone_address(), Some("node-second"));
    let second = paired(&mut rig, &app, 44, "Second phone");
    let second_id = second["deviceId"].as_str().unwrap().to_owned();
    assert_eq!(noticed(&mut rig, &app), ["First phone", "Second phone"]);
    // The first phone hears of the second and not of itself; the second hears of nothing before it.
    assert_eq!(noticed(&mut rig, &first_phone), ["Second phone"]);
    let second_phone = as_device(&mut rig, &second_id);
    assert!(noticed(&mut rig, &second_phone).is_empty());

    // The person dismisses through the first: the second is still news.
    let dismissed = rig
        .call(
            &app,
            "device.notice.dismiss",
            json!({"through": first["addedAt"]}),
        )
        .unwrap();
    assert_eq!(dismissed["through"], first["addedAt"]);
    assert_eq!(noticed(&mut rig, &app), ["Second phone"]);
    // That was the person's notice, not the first phone's.
    assert_eq!(noticed(&mut rig, &first_phone), ["Second phone"]);
    // Dismissing through a time that has not come dismisses what is there and no more.
    let future = rig
        .call(
            &app,
            "device.notice.dismiss",
            json!({"through": "2999-01-01T00:00:00Z"}),
        )
        .unwrap();
    assert!(future["through"].as_str().unwrap() < "2999", "{future}");
    assert!(noticed(&mut rig, &app).is_empty());
    // A device added later is news again, to the person and to the phone that dismissed nothing.
    move_clock(&mut rig, 10);
    rig.peers.set_node(phone_address(), Some("node-third"));
    paired(&mut rig, &app, 45, "Third phone");
    assert_eq!(noticed(&mut rig, &app), ["Third phone"]);
    assert_eq!(
        noticed(&mut rig, &first_phone),
        ["Second phone", "Third phone"]
    );
    // A revoked device is no longer news.
    let third_id = rig.call(&app, "device.list", json!({})).unwrap()["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Third phone")
        .unwrap()["deviceId"]
        .clone();
    rig.call(&app, "device.revoke", json!({"deviceId": third_id}))
        .unwrap();
    assert!(noticed(&mut rig, &app).is_empty());
}

#[tokio::test]
async fn a_device_dismisses_its_own_notice_and_a_wrong_time_or_a_chief_is_refused() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let first = paired(&mut rig, &app, 46, "First phone");
    move_clock(&mut rig, 5);
    rig.peers.set_node(phone_address(), Some("node-second"));
    let second = paired(&mut rig, &app, 47, "Second phone");
    let first_phone = as_device(&mut rig, first["deviceId"].as_str().unwrap());
    assert_eq!(noticed(&mut rig, &first_phone), ["Second phone"]);
    let key = json!({"through": second["addedAt"], "idempotencyKey": "dismiss-once"});
    let done = rig
        .call(&first_phone, "device.notice.dismiss", key.clone())
        .unwrap();
    // A replay of the call is its stored answer.
    assert_eq!(
        rig.call(&first_phone, "device.notice.dismiss", key)
            .unwrap(),
        done
    );
    assert!(noticed(&mut rig, &first_phone).is_empty());
    // The person at the computer was not told by that.
    assert_eq!(noticed(&mut rig, &app), ["First phone", "Second phone"]);
    let bad = rig.call(
        &app,
        "device.notice.dismiss",
        json!({"through": "yesterday"}),
    );
    assert_eq!(kind(&bad), ErrorKind::InvalidParams);

    let (outbox, _receiver) = crate::outbox::channel();
    let chief = rig.core.connect(false, false, outbox).unwrap();
    rig.core.clients.get_mut(&chief).unwrap().actor = Some(Actor {
        role: Role::Chief,
        client: "chief".into(),
        device_id: None,
        pane_id: None,
    });
    for method in ["device.notice.list", "device.panes.list"] {
        let answer = rig.call(&chief, method, json!({"deviceId": "x"}));
        assert_eq!(kind(&answer), ErrorKind::ForbiddenRole, "{method}");
    }
}

#[tokio::test]
async fn a_device_dismissing_the_notice_moves_its_own_mark_and_nobody_else_s() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let first = paired(&mut rig, &app, 48, "First phone");
    move_clock(&mut rig, 5);
    rig.peers.set_node(phone_address(), Some("node-second"));
    let second = paired(&mut rig, &app, 49, "Second phone");
    move_clock(&mut rig, 10);
    rig.peers.set_node(phone_address(), Some("node-third"));
    let third = paired(&mut rig, &app, 50, "Third phone");
    let first_phone = as_device(&mut rig, first["deviceId"].as_str().unwrap());
    let second_phone = as_device(&mut rig, second["deviceId"].as_str().unwrap());
    assert_eq!(
        noticed(&mut rig, &first_phone),
        ["Second phone", "Third phone"]
    );
    assert_eq!(noticed(&mut rig, &second_phone), ["Third phone"]);
    // The first phone dismisses through everything it was shown.
    rig.call(
        &first_phone,
        "device.notice.dismiss",
        json!({"through": third["addedAt"]}),
    )
    .unwrap();
    assert!(noticed(&mut rig, &first_phone).is_empty());
    // The second phone and the person at the computer are told as before.
    assert_eq!(noticed(&mut rig, &second_phone), ["Third phone"]);
    assert_eq!(
        noticed(&mut rig, &app),
        ["First phone", "Second phone", "Third phone"]
    );
    // Dismissing for the person at the computer does not move the phones' marks either.
    rig.call(
        &app,
        "device.notice.dismiss",
        json!({"through": third["addedAt"]}),
    )
    .unwrap();
    assert!(noticed(&mut rig, &app).is_empty());
    assert_eq!(noticed(&mut rig, &second_phone), ["Third phone"]);
}

#[tokio::test]
async fn an_agent_and_a_chief_are_refused_the_dismissal_and_it_moves_no_mark() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    paired(&mut rig, &app, 51, "First phone");
    for role in [Role::Agent, Role::Chief] {
        let (outbox, _receiver) = crate::outbox::channel();
        let connection = rig.core.connect(false, false, outbox).unwrap();
        rig.core.clients.get_mut(&connection).unwrap().scope = format!("scope-{role:?}");
        rig.core.clients.get_mut(&connection).unwrap().actor = Some(Actor {
            role: role.clone(),
            client: "program".into(),
            device_id: None,
            pane_id: None,
        });
        let answer = rig.call(
            &connection,
            "device.notice.dismiss",
            json!({"through": "2999-01-01T00:00:00Z"}),
        );
        assert_eq!(kind(&answer), ErrorKind::ForbiddenRole, "{role:?}");
    }
    // Nothing was dismissed: the person is still told.
    assert_eq!(noticed(&mut rig, &app), ["First phone"]);
}
