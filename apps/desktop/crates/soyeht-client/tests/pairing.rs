//! The pairing sheet against a scripted engine on a real local endpoint: the window the engine
//! opens, a request that comes in on it, a wrong code and then the right one, a denial, a window
//! that runs out, a connection that is lost and the pairing that starts again, an engine that
//! refuses, and the code that is sent only where the engine asks for it (ADR 0164).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_client::{
    Command, Effect, Engine, Event, State,
    pairing::{Phase, Rejected},
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
        rig
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
    rig.state.pairing.ask_to_revoke("d1");
    let effects = rig.state.confirm_revoke();
    rig.dispatch(effects);
    let call = rig.remote.expect("device.revoke");
    assert_eq!(call["params"]["deviceId"], "d1");
    rig.remote.reply(&call, json!({"deviceId": "d1"}));
    rig.settle(|state| state.pairing.devices()[0].revoked);
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
