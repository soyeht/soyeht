//! The pairing sheet against a scripted engine on a real local endpoint: the window the engine
//! opens, a request that comes in on it, a wrong code and then the right one, a denial, a window
//! that runs out, a connection that is lost and the pairing that starts again, an engine that
//! refuses, and the code that is sent only where the engine asks for it (ADR 0164).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_client::{
    Command, Effect, Engine, Event, State,
    pairing::{DevicePanes, Event as PairingEvent, Phase, Rejected},
    timestamp,
};
use soyeht_platform::local::Listener;
use soyeht_protocol::{ErrorKind, error_code};
use std::path::PathBuf;

#[path = "support/fake_engine.rs"]
mod fake_engine;
use fake_engine::{FakeEngine, listen, notification, wait_for};

const FINGERPRINT: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const LINK: &str = "soyeht-next://pair?engineId=e1&address=100.64.0.7:7777&code=CODE&engineKey=KEY";

/// A window of the client against the scripted engine, and what its replica holds.
struct Rig {
    engine: Engine,
    inbox: async_channel::Receiver<Event>,
    state: State,
    remote: FakeEngine,
    listener: Listener,
    path: PathBuf,
}

impl Rig {
    /// A connected window, to an engine that offers `capabilities` and has paired `devices`.
    fn connected(name: &str, capabilities: &[&str], devices: Value) -> Self {
        Self::connected_noticing(name, capabilities, devices, json!([]))
    }

    /// [`connected`](Self::connected), to an engine that says `notice` of the devices added.
    fn connected_noticing(
        name: &str,
        capabilities: &[&str],
        devices: Value,
        notice: Value,
    ) -> Self {
        let (listener, path) = listen(name);
        let (engine, inbox) = Engine::start(path.clone());
        let mut remote = FakeEngine::accept(&listener);
        remote.handshake_offering(capabilities, &sections(capabilities, devices));
        let mut rig = Self {
            engine,
            inbox,
            state: State::default(),
            remote,
            listener,
            path,
        };
        rig.settle(|state| state.connection == soyeht_client::Connection::Connected);
        // An engine that asks for the person's code is asked which devices were added.
        if capabilities.contains(&"pairing-code") {
            rig.answer_notice(notice);
        }
        rig
    }

    /// The engine tells the window which devices were added since the notice was dismissed.
    fn answer_notice(&mut self, devices: Value) {
        let call = self.remote.expect("device.notice.list");
        self.remote.reply(&call, json!({"items": devices}));
        let event = wait_for(&self.inbox, |event| {
            matches!(event, Event::Pairing(PairingEvent::Noticed { .. }))
        });
        let effects = self.state.apply(event);
        self.dispatch(effects);
    }

    /// Sends the pairing commands the replica asks for; everything else it asks is not this
    /// test's business.
    fn dispatch(&self, effects: Vec<Effect>) {
        for effect in effects {
            if let Effect::Send(command @ Command::Pairing(_)) = effect {
                self.engine.send(command).unwrap();
            }
        }
    }

    /// Applies what arrives until `done` holds.
    fn settle(&mut self, done: impl Fn(&State) -> bool) {
        let mut seen = Vec::new();
        while !done(&self.state) {
            let event = wait_for(&self.inbox, |_| true);
            let effects = self.state.apply(event);
            seen.extend(effects);
        }
        self.dispatch(seen);
    }

    /// The person chooses "Pair a phone" and the engine opens a window that ends at `expires`.
    fn open(&mut self, expires: &str) {
        let effects = self.state.open_pairing();
        self.dispatch(effects);
        let start = self.remote.expect("device.pair.start");
        self.remote.reply(
            &start,
            json!({"code": "CODE", "expiresAt": expires, "uri": LINK,
                "qr": LINK.replace("soyeht-next://pair?", "SOYEHT-PAIR 1 "),
                "reachability": {"remote": true}}),
        );
        self.settle(|state| matches!(state.pairing.phase(), Phase::Showing(_)));
    }

    /// A phone proved its pairing request on this window.
    fn request_arrives(&mut self, id: &str) {
        self.remote.send(&notification(
            "device.pair_requested",
            json!({"eventId": "8", "at": "2030-01-01T00:00:00Z", "requestId": id,
                "deviceName": "Caio's phone", "fingerprint": FINGERPRINT}),
        ));
        self.settle(|state| !state.pairing.requests().is_empty());
    }

    /// What the person did, sent to the engine and read there.
    fn decide(&mut self, id: &str, approve: bool, typed: &str) -> Value {
        let effects = self.state.decide_pairing(id, approve, typed).unwrap();
        self.dispatch(effects);
        self.remote.expect("device.pair.decide")
    }

    fn finish(self) {
        self.engine.stop();
        drop(self.remote);
        let _ = std::fs::remove_file(self.path);
    }
}

fn sections(capabilities: &[&str], devices: Value) -> Vec<(&'static str, Value)> {
    vec![
        ("capabilities", json!(capabilities)),
        ("devices", devices),
        ("requests", json!([])),
    ]
}

fn refused(kind: &ErrorKind, message: &str) -> Value {
    json!({"code": error_code(kind), "message": message, "data": {"kind": kind}})
}

fn answer_error(remote: &mut FakeEngine, request: &Value, kind: &ErrorKind, message: &str) {
    remote.send(&json!({"jsonrpc": "2.0", "id": request["id"],
        "error": refused(kind, message)}));
}

#[test]
fn a_phone_is_paired_with_the_code_it_shows_after_a_wrong_one() {
    let mut rig = Rig::connected("pair-flow", &["pairing-code"], json!([]));
    assert!(rig.state.pairing.offers_code());
    rig.open("2999-01-01T00:00:00Z");
    let Phase::Showing(offer) = rig.state.pairing.phase() else {
        panic!("showing");
    };
    assert!(offer.matrix.is_some());
    assert_eq!(offer.link, LINK);

    rig.request_arrives("r1");
    let request = &rig.state.pairing.requests()[0];
    assert_eq!(request.device_name, "Caio's phone");
    assert_eq!(request.comparison.as_deref(), Some("E3B0 C442 98FC 1C14"));

    // Not six digits: nothing goes to the engine.
    assert_eq!(
        rig.state.decide_pairing("r1", true, "12 34").err(),
        Some(Rejected::MalformedCode)
    );

    // A wrong code: the engine says how many tries are left and the request stays.
    let call = rig.decide("r1", true, "111 111");
    assert_eq!(call["params"]["approve"], true);
    assert_eq!(call["params"]["approvalCode"], "111111");
    answer_error(
        &mut rig.remote,
        &call,
        &ErrorKind::PairingCodeInvalid,
        "That code is not right. 2 tries left",
    );
    rig.settle(|state| state.pairing.requests()[0].note.is_some());
    let request = &rig.state.pairing.requests()[0];
    assert_eq!(
        request.note.as_deref(),
        Some("That code is not right. 2 tries left")
    );
    assert!(!request.deciding);
    assert!(
        rig.state.banner().is_none(),
        "the refusal is in the request, not a banner"
    );

    // The right one: approved, the phone becomes a device and the sheet closes.
    let call = rig.decide("r1", true, "482913");
    assert_eq!(call["params"]["approvalCode"], "482913");
    rig.remote
        .reply(&call, json!({"requestId": "r1", "approved": true}));
    rig.remote.send(&notification(
        "device.paired",
        json!({"eventId": "9", "at": "2030-01-01T00:00:01Z", "device": {
            "deviceId": "d1", "name": "Caio's phone", "fingerprint": FINGERPRINT,
            "revoked": false}}),
    ));
    rig.settle(|state| !state.pairing.shown() && state.pairing.devices().len() == 1);
    assert!(rig.state.pairing.requests().is_empty());
    rig.finish();
}

#[test]
fn the_request_closes_on_the_engines_denial_and_not_on_a_count_of_refusals() {
    let mut rig = Rig::connected("pair-engine-denial", &["pairing-code"], json!([]));
    rig.open("2999-01-01T00:00:00Z");
    rig.request_arrives("r1");
    // The engine uses one error kind for a code it has none for, so these never close the card.
    for _ in 0..4 {
        let call = rig.decide("r1", true, "111111");
        answer_error(
            &mut rig.remote,
            &call,
            &ErrorKind::PairingCodeInvalid,
            "No code is waiting for this request; pair again on the phone",
        );
        rig.settle(|state| !state.pairing.requests()[0].deciding);
        assert!(!rig.state.pairing.requests()[0].closed);
    }
    // The engine denies it at its own third wrong code: it says so before it refuses the call.
    let call = rig.decide("r1", true, "222222");
    rig.remote.send(&notification(
        "pair.result",
        json!({"requestId": "r1", "approved": false, "reason": "Denied on the computer"}),
    ));
    answer_error(
        &mut rig.remote,
        &call,
        &ErrorKind::PairingCodeInvalid,
        "Too many wrong codes. The request was denied",
    );
    rig.settle(|state| state.pairing.requests()[0].closed && !state.pairing.requests()[0].deciding);
    let request = &rig.state.pairing.requests()[0];
    assert_eq!(
        request.note.as_deref(),
        Some("Too many wrong codes. The request was denied")
    );
    rig.finish();
}

#[test]
fn a_denial_carries_no_code_and_the_sheet_stays() {
    let mut rig = Rig::connected("pair-deny", &["pairing-code"], json!([]));
    rig.open("2999-01-01T00:00:00Z");
    rig.request_arrives("r1");
    let call = rig.decide("r1", false, "");
    assert_eq!(call["params"]["approve"], false);
    assert!(call["params"].get("approvalCode").is_none(), "{call}");
    rig.remote
        .reply(&call, json!({"requestId": "r1", "approved": false}));
    rig.settle(|state| state.pairing.requests().is_empty());
    assert!(matches!(rig.state.pairing.phase(), Phase::Showing(_)));
    rig.finish();
}

#[test]
fn an_engine_without_pairing_code_is_never_sent_one() {
    let mut rig = Rig::connected("pair-no-code", &[], json!([]));
    assert!(!rig.state.pairing.offers_code());
    rig.open("2999-01-01T00:00:00Z");
    rig.request_arrives("r1");
    // Whatever the field holds, and even nothing, the approval goes with no code.
    let call = rig.decide("r1", true, "123456");
    assert!(call["params"].get("approvalCode").is_none(), "{call}");
    rig.finish();
}

#[test]
fn a_window_that_ran_out_asks_for_a_new_one() {
    let mut rig = Rig::connected("pair-expiry", &["pairing-code"], json!([]));
    rig.open("2000-01-01T00:00:00Z");
    assert!(rig.state.pairing.tick(timestamp::now()));
    assert_eq!(rig.state.pairing.phase(), &Phase::Expired);
    rig.open("2999-01-01T00:00:00Z");
    rig.finish();
}

#[test]
fn a_lost_connection_loses_the_window_and_the_pairing_starts_again_on_the_next() {
    let mut rig = Rig::connected("pair-reconnect", &["pairing-code"], json!([]));
    rig.open("2999-01-01T00:00:00Z");
    rig.request_arrives("r1");
    // The engine goes away and comes back: the replica sees the connection end first.
    let Rig {
        engine,
        inbox,
        state,
        remote,
        listener,
        path,
    } = rig;
    drop(remote);
    let mut rig = Rig {
        engine,
        inbox,
        state,
        remote: FakeEngine::accept(&listener),
        listener,
        path,
    };
    // The connection that carried the window is gone, and with it the request.
    rig.settle(|state| state.pairing.phase() == &Phase::Interrupted);
    assert!(rig.state.pairing.requests().is_empty());
    assert_eq!(
        rig.state.decide_pairing("r1", false, "").err(),
        Some(Rejected::Unavailable)
    );
    rig.remote
        .handshake_offering(&["pairing-code"], &sections(&["pairing-code"], json!([])));
    // The new connection's snapshot starts the pairing again, without the person doing anything.
    rig.settle(|state| state.connection == soyeht_client::Connection::Connected);
    let start = rig.remote.expect("device.pair.start");
    rig.remote.reply(
        &start,
        json!({"code": "NEW", "expiresAt": "2999-01-01T00:00:00Z", "uri": LINK}),
    );
    rig.settle(|state| matches!(state.pairing.phase(), Phase::Showing(_)));
    rig.finish();
}

#[test]
fn an_engine_that_refuses_is_shown_with_its_own_words() {
    let mut rig = Rig::connected("pair-refused", &[], json!([]));
    let effects = rig.state.open_pairing();
    rig.dispatch(effects);
    let start = rig.remote.expect("device.pair.start");
    let words = "Pairing is started and approved only in the signed Soyeht app";
    answer_error(&mut rig.remote, &start, &ErrorKind::ForbiddenRole, words);
    rig.settle(|state| matches!(state.pairing.phase(), Phase::Refused(_)));
    let Phase::Refused(refusal) = rig.state.pairing.phase() else {
        panic!("refused");
    };
    assert_eq!(refusal.message, words);
    assert_eq!(refusal.kind, Some(ErrorKind::ForbiddenRole));
    assert!(
        rig.state.banner().is_none(),
        "the sheet says it, not a banner"
    );
    rig.finish();
}

#[test]
fn a_device_is_revoked_after_the_person_confirms() {
    let device = json!({"deviceId": "d1", "name": "Phone", "fingerprint": FINGERPRINT,
        "revoked": false});
    let mut rig = Rig::connected("pair-revoke", &["pairing-code"], json!([device]));
    assert_eq!(rig.state.pairing.devices().len(), 1);
    let effects = rig.state.ask_to_revoke("d1");
    rig.dispatch(effects);
    // What the device made is asked first, so that the question can offer to close it.
    let panes = rig.remote.expect("device.panes.list");
    assert_eq!(panes["params"]["deviceId"], "d1");
    rig.remote.reply(&panes, json!({"items": []}));
    rig.settle(|state| state.pairing.revoking_panes() != &DevicePanes::Loading);
    let effects = rig.state.confirm_revoke(true);
    rig.dispatch(effects);
    let call = rig.remote.expect("device.revoke");
    assert_eq!(call["params"]["deviceId"], "d1");
    rig.remote.reply(&call, json!({"deviceId": "d1"}));
    rig.settle(|state| state.pairing.devices()[0].revoked);
    rig.finish();
}

/// A window of an engine that has paired `d1`, whose question before a revocation lists `panes`.
fn asked_to_revoke(name: &str, panes: Value) -> Rig {
    let device = json!({"deviceId": "d1", "name": "Phone", "fingerprint": FINGERPRINT,
        "revoked": false});
    let mut rig = Rig::connected(name, &["pairing-code"], json!([device]));
    let effects = rig.state.ask_to_revoke("d1");
    rig.dispatch(effects);
    let listing = rig.remote.expect("device.panes.list");
    rig.remote.reply(&listing, json!({"items": panes}));
    rig.settle(|state| matches!(state.pairing.revoking_panes(), DevicePanes::Listed(_)));
    rig
}

/// The person revokes `d1` and chooses to close its panes; the engine revokes it.
fn revoke_and_close(rig: &mut Rig) {
    let effects = rig.state.confirm_revoke(true);
    rig.dispatch(effects);
    let revoke = rig.remote.expect("device.revoke");
    rig.remote.reply(&revoke, json!({"deviceId": "d1"}));
}

fn pane(id: &str, name: &str) -> Value {
    json!({"paneId": id, "workspaceId": "w1", "name": name, "cwd": "/work", "mode": "terminal",
        "state": "idle", "source": "none", "confidence": 0.0, "phrase": "", "generation": 1,
        "cols": 80, "rows": 24, "record": false})
}

#[test]
fn the_panes_a_device_has_when_it_is_revoked_are_closed_and_not_the_ones_the_question_listed() {
    // The question listed p1 and p2; by the time the device is revoked it has made p3 and p2 is gone.
    let panes = json!([pane("p1", "shell"), pane("p2", "logs")]);
    let mut rig = asked_to_revoke("pair-revoke-panes", panes);
    let effects = rig.state.confirm_revoke(true);
    rig.dispatch(effects);
    // The device first: what it made is closed only once it can make no more.
    let revoke = rig.remote.expect("device.revoke");
    rig.remote.reply(&revoke, json!({"deviceId": "d1"}));
    // Its panes are listed again, since it may have made some after the question listed them.
    let again = rig.remote.expect("device.panes.list");
    assert_eq!(again["params"]["deviceId"], "d1");
    rig.remote.reply(
        &again,
        json!({"items": [pane("p1", "shell"), pane("p3", "new")]}),
    );
    for id in ["p1", "p3"] {
        let close = rig.remote.expect("pane.close");
        assert_eq!(close["params"]["paneId"], id);
        if id == "p1" {
            rig.remote.reply(&close, json!({"paneId": id}));
        } else {
            answer_error(&mut rig.remote, &close, &ErrorKind::PaneBusy, "busy");
        }
    }
    rig.settle(|state| state.pairing.revoke_failure().is_some());
    // The one pane that would not close is said, and the device is revoked all the same.
    let failure = rig.state.pairing.revoke_failure().unwrap();
    assert!(failure.contains("1 of its panes"), "{failure}");
    assert!(rig.state.pairing.devices()[0].revoked);
    rig.finish();
}

#[test]
fn a_pane_that_is_gone_when_it_is_closed_is_closed_and_a_list_that_fails_is_said() {
    let mut rig = asked_to_revoke("pair-revoke-gone", json!([pane("p1", "shell")]));
    revoke_and_close(&mut rig);
    let again = rig.remote.expect("device.panes.list");
    rig.remote
        .reply(&again, json!({"items": [pane("p1", "shell")]}));
    // The pane closed by itself in the meantime: the close says it is not found, which is done.
    let close = rig.remote.expect("pane.close");
    answer_error(&mut rig.remote, &close, &ErrorKind::NotFound, "gone");
    rig.settle(|state| state.pairing.devices()[0].revoked);
    assert!(rig.state.pairing.revoke_failure().is_none());
    rig.finish();

    // The panes of a revoked device that cannot be listed are not closed, and the person is told.
    let mut rig = asked_to_revoke("pair-revoke-nolist", json!([pane("p1", "shell")]));
    revoke_and_close(&mut rig);
    let again = rig.remote.expect("device.panes.list");
    answer_error(&mut rig.remote, &again, &ErrorKind::DiskFull, "no");
    rig.settle(|state| state.pairing.revoke_failure().is_some());
    assert!(
        rig.state
            .pairing
            .revoke_failure()
            .unwrap()
            .contains("could not be listed")
    );
    rig.finish();
}

#[test]
fn a_revocation_that_keeps_the_panes_closes_none() {
    let mut rig = asked_to_revoke("pair-revoke-keep", json!([pane("p1", "shell")]));
    let effects = rig.state.confirm_revoke(false);
    rig.dispatch(effects);
    let revoke = rig.remote.expect("device.revoke");
    rig.remote.reply(&revoke, json!({"deviceId": "d1"}));
    rig.settle(|state| state.pairing.devices()[0].revoked);
    assert!(rig.state.pairing.revoke_failure().is_none());
    rig.finish();
}

#[test]
fn the_notice_of_devices_added_is_read_on_connecting_and_stays_until_dismissed() {
    let rig = Rig::connected("pair-notice", &[], json!([]));
    // An engine that does not ask for the person's code dates no device: it is not asked.
    assert!(rig.state.pairing.notice().is_empty());
    rig.finish();

    let device = |id: &str, at: &str| {
        json!({"deviceId": id, "name": id, "fingerprint": FINGERPRINT, "revoked": false,
            "addedAt": at, "node": "phone-node"})
    };
    let notice = json!([
        device("d1", "2026-11-15T10:00:00Z"),
        device("d2", "2026-11-15T10:05:00Z")
    ]);
    let mut rig =
        Rig::connected_noticing("pair-notice-dismiss", &["pairing-code"], json!([]), notice);
    assert_eq!(rig.state.pairing.notice().len(), 2);
    // The person dismisses it: the engine is told through the newest device that was shown.
    let effects = rig.state.dismiss_notice();
    rig.dispatch(effects);
    let call = rig.remote.expect("device.notice.dismiss");
    assert_eq!(call["params"]["through"], "2026-11-15T10:05:00Z");
    rig.remote
        .reply(&call, json!({"through": "2026-11-15T10:05:00Z"}));
    rig.settle(|state| state.pairing.notice().is_empty());
    assert!(rig.state.pairing.notice_failure().is_none());
    rig.finish();
}

#[test]
fn an_engine_older_than_the_notice_is_not_blamed_and_another_failure_is_said() {
    let mut rig = Rig::connected("pair-notice-old", &["pairing-code"], json!([]));
    for (kind, said) in [
        (ErrorKind::InvalidParams, false),
        (ErrorKind::DiskFull, true),
    ] {
        let load = rig.state.pairing.load_notice().unwrap();
        rig.engine.send(Command::Pairing(load)).unwrap();
        let call = rig.remote.expect("device.notice.list");
        answer_error(&mut rig.remote, &call, &kind, "refused");
        let event = wait_for(&rig.inbox, |event| {
            matches!(event, Event::Pairing(PairingEvent::NoticeFailed { .. }))
        });
        rig.state.apply(event);
        assert_eq!(
            rig.state.pairing.notice_failure().is_some(),
            said,
            "{kind:?}"
        );
    }
    rig.finish();
}

#[test]
fn a_changed_tailnet_is_confirmed_with_a_new_window_and_the_confirmation_is_sent_only_then() {
    let mut rig = Rig::connected("pair-tailnet", &["pairing-code"], json!([]));
    let effects = rig.state.open_pairing();
    rig.dispatch(effects);
    let start = rig.remote.expect("device.pair.start");
    // A window opened without the confirmation, which the engine says is blocked by a change.
    assert!(start["params"].get("confirmTailnet").is_none(), "{start}");
    rig.remote.reply(
        &start,
        json!({"code": "CODE", "expiresAt": "2999-01-01T00:00:00Z", "uri": LINK,
            "tailnetChanged": true}),
    );
    rig.settle(|state| state.pairing.tailnet_changed());
    // The person confirms: a new window is asked for with `confirmTailnet`.
    let effects = rig.state.confirm_tailnet();
    rig.dispatch(effects);
    let confirm = rig.remote.expect("device.pair.start");
    assert_eq!(confirm["params"]["confirmTailnet"], true);
    rig.remote.reply(
        &confirm,
        json!({"code": "NEW", "expiresAt": "2999-01-01T00:00:00Z", "uri": LINK}),
    );
    rig.settle(|state| !state.pairing.tailnet_changed());
    assert!(matches!(rig.state.pairing.phase(), Phase::Showing(_)));
    // Nothing waits any more, so a second press sends nothing.
    assert!(rig.state.confirm_tailnet().is_empty());
    rig.finish();
}

#[test]
fn an_open_sheet_hears_of_a_changed_tailnet_from_the_engine() {
    let mut rig = Rig::connected("pair-tailnet-event", &["pairing-code"], json!([]));
    rig.open("2999-01-01T00:00:00Z");
    rig.remote.send(&notification(
        "device.pair_tailnet_changed",
        json!({"tailnetChanged": true}),
    ));
    rig.settle(|state| state.pairing.tailnet_changed());
    rig.finish();
}

#[test]
fn a_request_that_waits_in_the_snapshot_is_listed_and_can_be_denied() {
    let (listener, path) = listen("pair-snapshot");
    let (engine, inbox) = Engine::start(path.clone());
    let mut remote = FakeEngine::accept(&listener);
    let pending = json!([{"kind": "device_pair", "requestId": "r9", "deviceName": "Tablet",
        "fingerprint": FINGERPRINT, "expiresAt": "2999-01-01T00:00:00Z"}]);
    remote.handshake_offering(
        &[],
        &[
            ("capabilities", json!([])),
            ("devices", json!([])),
            ("requests", pending),
        ],
    );
    let mut state = State::default();
    let connected = wait_for(&inbox, |event| matches!(event, Event::Connected(_)));
    state.apply(connected);
    assert_eq!(state.pairing.requests()[0].request_id, "r9");
    engine.stop();
    drop(remote);
    let _ = std::fs::remove_file(path);
}
