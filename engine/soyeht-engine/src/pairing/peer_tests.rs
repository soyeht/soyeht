//! The rules about where a pairing may come from (ADR 0164), through the engine's own dispatch.
//! The listeners, the interfaces and Tailscale are answered by `FixturePeers`, so these run on
//! every system; the real ones are checked natively on Linux.

use super::code_tests::{
    Answer, Device, Rig, begin, code_of, decide, key, kind, pair_result, pair_with, phone_address,
    prove_params, text,
};
use super::*;
use crate::{
    dispatch::Outcome,
    outbox,
    pairing_peers::{ListenerKind, Peer},
    ports::Clock,
    test_hooks::{FIXTURE_NODE_KEY, FIXTURE_PHONE_NODE, FIXTURE_TAILNET, fixture_identity},
};
use ed25519_dalek::Signer as _;
use soyeht_protocol::ErrorKind;
use soyeht_store::pairing_nodes;
use std::{
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};

const OTHER_NODE: &str = "node-other";
const OTHER_ADDRESS: &str = "100.100.0.3";
const PHONE_V6: &str = "fd7a:115c:a1e0::2";

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

/// A connection from the network that the listener saw as coming from `address`.
fn peer_at(rig: &mut Rig, listener: ListenerKind, address: &str) -> String {
    let connection = rig.connect(false, false);
    crate::pairing_gate::note_peer(&mut rig.core, &connection, Peer::new(listener, ip(address)));
    connection
}

fn tailnet_peer(rig: &mut Rig, address: &str) -> String {
    peer_at(rig, ListenerKind::Tailnet, address)
}

/// A rig with the app's connection and its window.
fn rig_with_window() -> (Rig, String, String) {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let window = rig.open_window(&app);
    (rig, app, window)
}

fn refused_with(answer: &Answer, expected: Refusal) {
    assert_eq!(kind(answer), ErrorKind::Unauthorized, "{expected:?}");
    assert_eq!(text(answer), expected.message(), "{expected:?}");
}

/// The hello of a paired device on `connection`: its signed proof and its own nonce.
fn device_hello(rig: &mut Rig, connection: &str, seed: u8, token: &str) -> Answer {
    let nonce = rig.core.clients[connection].nonce;
    let client_nonce = [9u8; 32];
    let proof = key(seed).sign(&auth::hello_bytes(
        &nonce,
        &client_nonce,
        &rig.core.store.db_id,
        token,
    ));
    let hello = json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{
        "client":"phone","clientVersion":"1","protocolVersion":1,"deviceToken":token,
        "proof":STANDARD.encode(proof.to_bytes()),"clientNonce":STANDARD.encode(client_nonce)}});
    rig.core
        .dispatch_inline(connection, hello)
        .map(|outcome| match outcome {
            Outcome::Reply(value) => value,
            _ => panic!("hello did not reply"),
        })
}

/// A device paired from the phone's node, as its token and id.
async fn paired(rig: &mut Rig, app: &str, window: &str, seed: u8) -> (String, String) {
    let device: Device = pair_with(rig, window, seed, "Test phone");
    let code = code_of(&device);
    rig.notifications(&device.connection).await;
    decide(rig, app, &device.request, Some(&code)).unwrap();
    let result = pair_result(rig, &device.connection).await;
    (
        result["deviceToken"].as_str().unwrap().to_owned(),
        result["deviceId"].as_str().unwrap().to_owned(),
    )
}

/// A `pair.begin` from the phone, on a window the app opens now.
fn begin_from_phone(rig: &mut Rig, app: &str, seed: u8) -> Answer {
    let window = rig.open_window(app);
    let phone = tailnet_peer(rig, &phone_address().to_string());
    begin(rig, &phone, &window, "Test phone", seed)
}

/// A clock that runs ahead of the system's by a number of seconds the test sets.
struct Skewed(Arc<AtomicI64>);

impl Clock for Skewed {
    fn now_ms(&self) -> i64 {
        chrono::Utc::now().timestamp_millis() + self.0.load(Ordering::Relaxed) * 1000
    }
}

#[tokio::test]
async fn only_a_connection_from_a_tailnet_listener_pairs() {
    let (mut rig, app, window) = rig_with_window();
    // The local endpoint, even as the person's app; a listener of a fixed address; a connection
    // whose listener was not recorded.
    let fixed = peer_at(&mut rig, ListenerKind::Fixed, "100.100.0.2");
    let unmarked = rig.connect(false, false);
    rig.core.clients.get_mut(&unmarked).unwrap().peer = None;
    let (outbox, _receiver) = outbox::channel();
    let fresh = rig.core.connect(true, false, outbox).unwrap();
    for connection in [fixed, unmarked, fresh] {
        let answer = begin(&mut rig, &connection, &window, "Test phone", 1);
        refused_with(&answer, Refusal::Listener);
    }
    // The person's own connection has said hello: it is refused before any rule is asked.
    let answer = begin(&mut rig, &app, &window, "Test phone", 1);
    assert_eq!(kind(&answer), ErrorKind::Unauthorized);
    let phone = tailnet_peer(&mut rig, "100.100.0.2");
    assert!(begin(&mut rig, &phone, &window, "Test phone", 1).is_ok());
}

#[tokio::test]
async fn an_address_that_is_not_a_tailscale_one_is_refused_even_from_a_tailnet_listener() {
    let (mut rig, _app, window) = rig_with_window();
    for address in [
        "127.0.0.1",
        "::1",
        // The mapped form of loopback is read as loopback.
        "::ffff:127.0.0.1",
        "169.254.10.20",
        "fe80::1",
        // Container and private ranges, which a program of this machine may bind as a source.
        "172.17.0.2",
        "10.0.0.5",
        "192.168.1.5",
        "100.63.255.255",
        "100.128.0.1",
        "fd7a:115c:a1e1::1",
    ] {
        let phone = tailnet_peer(&mut rig, address);
        let answer = begin(&mut rig, &phone, &window, "Test phone", 2);
        refused_with(&answer, Refusal::Address);
    }
}

#[tokio::test]
async fn a_tailscale_address_of_this_machine_is_refused() {
    let (mut rig, _app, window) = rig_with_window();
    // A program that bound the machine's own tailnet address as the source of its connection.
    rig.peers
        .set_local(Some(vec![ip("127.0.0.1"), phone_address()]));
    let phone = tailnet_peer(&mut rig, &phone_address().to_string());
    refused_with(
        &begin(&mut rig, &phone, &window, "Test phone", 3),
        Refusal::LocalAddress,
    );
    // The same address, read from an interface in the mapped form.
    rig.peers.set_local(Some(vec![ip("::ffff:100.100.0.2")]));
    refused_with(
        &begin(&mut rig, &phone, &window, "Test phone", 3),
        Refusal::LocalAddress,
    );
    // The list of the machine's addresses cannot be read: any doubt refuses.
    rig.peers.set_local(None);
    refused_with(
        &begin(&mut rig, &phone, &window, "Test phone", 3),
        Refusal::LocalAddress,
    );
    rig.peers.set_local(Some(vec![ip("127.0.0.1")]));
    assert!(begin(&mut rig, &phone, &window, "Test phone", 3).is_ok());
}

#[tokio::test]
async fn pairing_fails_closed_when_tailscale_cannot_say_who_is_connecting() {
    let (mut rig, _app, window) = rig_with_window();
    let phone = tailnet_peer(&mut rig, &phone_address().to_string());
    // The daemon does not know the peer, or cannot be asked who this computer is.
    rig.peers.set_node(phone_address(), None);
    refused_with(
        &begin(&mut rig, &phone, &window, "Test phone", 4),
        Refusal::WhoisUnavailable,
    );
    rig.peers
        .set_node(phone_address(), Some(FIXTURE_PHONE_NODE));
    rig.peers.set_identity(None);
    refused_with(
        &begin(&mut rig, &phone, &window, "Test phone", 4),
        Refusal::WhoisUnavailable,
    );
    // Tailscale says the peer is this computer itself.
    rig.peers
        .set_identity(Some(fixture_identity(FIXTURE_TAILNET, FIXTURE_NODE_KEY)));
    rig.peers.set_node(
        phone_address(),
        Some(&fixture_identity(FIXTURE_TAILNET, FIXTURE_NODE_KEY).node),
    );
    refused_with(
        &begin(&mut rig, &phone, &window, "Test phone", 4),
        Refusal::SameNode,
    );
    rig.peers
        .set_node(phone_address(), Some(FIXTURE_PHONE_NODE));
    assert!(begin(&mut rig, &phone, &window, "Test phone", 4).is_ok());
}

#[tokio::test]
async fn a_replayed_begin_from_a_local_program_gets_no_stored_answer() {
    let (mut rig, app, window) = rig_with_window();
    let phone = tailnet_peer(&mut rig, &phone_address().to_string());
    let params = json!({"code":window,"deviceName":"Test phone",
        "devicePublicKey":STANDARD.encode(key(5).verifying_key().to_bytes()),
        "idempotencyKey":"replay-key"});
    let stored = rig.call(&phone, "pair.begin", params.clone()).unwrap();
    let local = rig.connect(true, false);
    let (outbox, _receiver) = outbox::channel();
    let fresh = rig.core.connect(true, false, outbox).unwrap();
    refused_with(
        &rig.call(&fresh, "pair.begin", params.clone()),
        Refusal::Listener,
    );
    for connection in [app, local] {
        let replayed = rig.call(&connection, "pair.begin", params.clone());
        assert_eq!(kind(&replayed), ErrorKind::Unauthorized);
    }
    // From the node that began it, the replay is the stored answer.
    let again = tailnet_peer(&mut rig, &phone_address().to_string());
    assert_eq!(rig.call(&again, "pair.begin", params).unwrap(), stored);
}

#[tokio::test]
async fn a_request_is_proved_by_the_node_that_began_it() {
    let (mut rig, _app, window) = rig_with_window();
    rig.peers.set_node(ip(OTHER_ADDRESS), Some(OTHER_NODE));
    let phone = tailnet_peer(&mut rig, &phone_address().to_string());
    let begun = begin(&mut rig, &phone, &window, "Test phone", 6).unwrap();
    let id = begun["requestId"].as_str().unwrap().to_owned();
    let params = prove_params(&window, 6, "Test phone", &begun);
    // Another node, even one that holds the request's id and its proof: whether by taking the
    // id on its own connection, or by replaying the begin.
    let other = tailnet_peer(&mut rig, OTHER_ADDRESS);
    rig.core.clients.get_mut(&other).unwrap().pair_request = Some(id);
    refused_with(
        &rig.call(&other, "pair.prove", params.clone()),
        Refusal::NodeChanged,
    );
    // The node that began it proves it.
    assert_eq!(
        rig.call(&phone, "pair.prove", params).unwrap()["status"],
        "pending"
    );
}

#[tokio::test]
async fn a_begin_replayed_from_another_node_is_refused_and_binds_nothing() {
    let (mut rig, _app, window) = rig_with_window();
    rig.peers.set_node(ip(OTHER_ADDRESS), Some(OTHER_NODE));
    let phone = tailnet_peer(&mut rig, &phone_address().to_string());
    let replay = json!({"code":window,"deviceName":"Test phone",
        "devicePublicKey":STANDARD.encode(key(6).verifying_key().to_bytes()),
        "idempotencyKey":"begin-6"});
    let first = rig.call(&phone, "pair.begin", replay.clone()).unwrap();
    let stranger = tailnet_peer(&mut rig, OTHER_ADDRESS);
    refused_with(
        &rig.call(&stranger, "pair.begin", replay.clone()),
        Refusal::NodeChanged,
    );
    assert!(rig.core.clients[&stranger].pair_request.is_none());
    let again = tailnet_peer(&mut rig, &phone_address().to_string());
    assert_eq!(rig.call(&again, "pair.begin", replay).unwrap(), first);
}

#[tokio::test]
async fn a_paired_device_s_node_cannot_pair_another_over_either_family() {
    let (mut rig, app, window) = rig_with_window();
    let (token, _) = paired(&mut rig, &app, &window, 7).await;
    // The device's own hello, from the phone's v4 address, records its node.
    let hello = tailnet_peer(&mut rig, &phone_address().to_string());
    let welcome = device_hello(&mut rig, &hello, 7, &token).unwrap();
    assert_eq!(welcome["role"], "human_device");
    let uses = pairing_nodes::uses_of(&rig.core.store, FIXTURE_PHONE_NODE).unwrap();
    assert_eq!(uses.len(), 1);
    // Another window; the same node begins a pairing from the v4 address or from the v6 one.
    rig.peers.set_node(ip(PHONE_V6), Some(FIXTURE_PHONE_NODE));
    let window = rig.open_window(&app);
    for address in [phone_address().to_string(), PHONE_V6.to_owned()] {
        let second = tailnet_peer(&mut rig, &address);
        refused_with(
            &begin(&mut rig, &second, &window, "Second phone", 8),
            Refusal::DeviceNode,
        );
    }
    // A node that is not a device's pairs.
    rig.peers.set_node(ip(OTHER_ADDRESS), Some(OTHER_NODE));
    let other = tailnet_peer(&mut rig, OTHER_ADDRESS);
    assert!(begin(&mut rig, &other, &window, "Second phone", 8).is_ok());
}

#[tokio::test]
async fn a_revoked_device_s_node_is_refused_for_a_day_and_allowed_after() {
    let (mut rig, app, window) = rig_with_window();
    let (token, device_id) = paired(&mut rig, &app, &window, 9).await;
    let hello = tailnet_peer(&mut rig, &phone_address().to_string());
    device_hello(&mut rig, &hello, 9, &token).unwrap();
    rig.call(&app, "device.revoke", json!({"deviceId":device_id}))
        .unwrap();
    refused_with(&begin_from_phone(&mut rig, &app, 10), Refusal::RevokedNode);
    // A little under a day later it is still refused; a little over, it pairs.
    let skew = Arc::new(AtomicI64::new(0));
    rig.core.clock = Arc::new(Skewed(Arc::clone(&skew)));
    skew.store(24 * 60 * 60 - 60, Ordering::Relaxed);
    refused_with(&begin_from_phone(&mut rig, &app, 10), Refusal::RevokedNode);
    skew.store(24 * 60 * 60 + 60, Ordering::Relaxed);
    assert!(begin_from_phone(&mut rig, &app, 10).is_ok());
}

#[tokio::test]
async fn a_device_hello_is_never_blocked_by_a_lookup_that_failed_and_records_no_node_then() {
    let (mut rig, app, window) = rig_with_window();
    let (token, _) = paired(&mut rig, &app, &window, 11).await;
    rig.peers.set_node(phone_address(), None);
    let hello = tailnet_peer(&mut rig, &phone_address().to_string());
    assert!(device_hello(&mut rig, &hello, 11, &token).is_ok());
    assert!(
        pairing_nodes::uses_of(&rig.core.store, FIXTURE_PHONE_NODE)
            .unwrap()
            .is_empty()
    );
    // A hello that comes from this computer's own node records nothing either.
    rig.peers
        .set_node(phone_address(), Some(&fixture_identity("t", "k").node));
    let hello = tailnet_peer(&mut rig, &phone_address().to_string());
    assert!(device_hello(&mut rig, &hello, 11, &token).is_ok());
    assert!(
        pairing_nodes::uses_of(&rig.core.store, &fixture_identity("t", "k").node)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn the_mac_mode_looks_up_no_node_and_writes_none() {
    let mut rig = Rig::new(Approval::SignedApp);
    let app = rig.connect(true, true);
    let window = rig.open_window(&app);
    let device = pair_with(&mut rig, &window, 12, "Test phone");
    decide(&mut rig, &app, &device.request, None).unwrap();
    let token = pair_result(&mut rig, &device.connection).await["deviceToken"]
        .as_str()
        .unwrap()
        .to_owned();
    let hello = tailnet_peer(&mut rig, &phone_address().to_string());
    assert!(device_hello(&mut rig, &hello, 12, &token).is_ok());
    let seen = pairing_nodes::uses_of(&rig.core.store, FIXTURE_PHONE_NODE).unwrap();
    assert!(seen.is_empty());
    let pinned = pairing_nodes::pinned(&rig.core.store).unwrap();
    assert!(pinned.is_none());
}

#[tokio::test]
async fn a_changed_tailnet_is_refused_until_the_person_confirms_it() {
    let (mut rig, app, window) = rig_with_window();
    let phone = tailnet_peer(&mut rig, &phone_address().to_string());
    // The first pairing served pins the tailnet and this computer's node key.
    assert!(begin(&mut rig, &phone, &window, "Test phone", 13).is_ok());
    let pinned = pairing_nodes::pinned(&rig.core.store).unwrap().unwrap();
    assert_eq!(
        (pinned.tailnet.as_str(), pinned.node_key.as_str()),
        (FIXTURE_TAILNET, FIXTURE_NODE_KEY)
    );
    for changed in [
        fixture_identity("attacker.example", FIXTURE_NODE_KEY),
        fixture_identity(FIXTURE_TAILNET, "nodekey:other"),
    ] {
        rig.peers.set_identity(Some(changed));
        let window = rig.open_window(&app);
        let phone = tailnet_peer(&mut rig, &phone_address().to_string());
        refused_with(
            &begin(&mut rig, &phone, &window, "Test phone", 14),
            Refusal::TailnetChanged,
        );
        // The app is told that a change waits for the person.
        let started = rig.call(&app, "device.pair.start", json!({})).unwrap();
        assert_eq!(started["tailnetChanged"], true);
    }
    // The person confirms: the identity the refusal saw is pinned, and a pairing is served.
    let confirmed = rig
        .call(&app, "device.pair.start", json!({"confirmTailnet":true}))
        .unwrap();
    assert!(confirmed.get("tailnetChanged").is_none(), "{confirmed}");
    let window = confirmed["code"].as_str().unwrap().to_owned();
    let phone = tailnet_peer(&mut rig, &phone_address().to_string());
    assert!(begin(&mut rig, &phone, &window, "Test phone", 15).is_ok());
    let pinned = pairing_nodes::pinned(&rig.core.store).unwrap().unwrap();
    assert_eq!(pinned.node_key, "nodekey:other");
}

#[tokio::test]
async fn confirming_with_nothing_refused_changes_nothing_and_a_prove_after_a_change_is_refused() {
    let (mut rig, app, window) = rig_with_window();
    let phone = tailnet_peer(&mut rig, &phone_address().to_string());
    let begun = begin(&mut rig, &phone, &window, "Test phone", 16).unwrap();
    let params = prove_params(&window, 16, "Test phone", &begun);
    rig.call(&app, "device.pair.start", json!({"confirmTailnet":true}))
        .unwrap();
    let pinned = pairing_nodes::pinned(&rig.core.store).unwrap().unwrap();
    assert_eq!(pinned.tailnet, FIXTURE_TAILNET);
    // The tailnet changes between the begin and the prove.
    rig.peers
        .set_identity(Some(fixture_identity("attacker.example", FIXTURE_NODE_KEY)));
    refused_with(
        &rig.call(&phone, "pair.prove", params),
        Refusal::TailnetChanged,
    );
}

#[tokio::test]
async fn a_forwarded_peer_is_never_read_from_the_connection() {
    // The peer is what the listener recorded when it accepted: nothing a client sends moves it.
    let (mut rig, _app, window) = rig_with_window();
    let local_peer = peer_at(&mut rig, ListenerKind::Fixed, "127.0.0.1");
    let params = json!({"code":window,"deviceName":"Test phone","forwardedFor":"100.100.0.2",
        "devicePublicKey":STANDARD.encode(key(17).verifying_key().to_bytes())});
    refused_with(
        &rig.call(&local_peer, "pair.begin", params),
        Refusal::Listener,
    );
    assert_eq!(
        rig.core.clients[&local_peer].peer,
        Some(Peer::new(ListenerKind::Fixed, ip("127.0.0.1")))
    );
}

#[tokio::test]
async fn the_mode_hook_of_a_test_does_not_change_what_a_shipped_engine_asks() {
    // The system's answers, with no tailnet listener: nothing a program of this computer does
    // reaches the rules that need a peer.
    let directory = std::env::temp_dir().join(format!("soyeht-peer-rules-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    struct OnlyApproval;
    impl crate::hooks::Hooks for OnlyApproval {
        fn pairing_approval(&self) -> Option<Approval> {
            Some(Approval::PersonsCode)
        }
    }
    let mut core = Core::open_with(&directory, Arc::new(OnlyApproval)).unwrap();
    let (outbox, _receiver) = outbox::channel();
    let connection = core.connect(false, false, outbox).unwrap();
    crate::pairing_gate::note_peer(
        &mut core,
        &connection,
        Peer::new(ListenerKind::Tailnet, ip("100.64.0.9")),
    );
    // A real lookup of a tailnet address that is not there: the daemon cannot say.
    let outcome = core.dispatch(
        &connection,
        json!({"jsonrpc":"2.0","id":1,"method":"pair.begin","params":{
            "code":"x","deviceName":"Phone",
            "devicePublicKey":STANDARD.encode(key(18).verifying_key().to_bytes()),
            "idempotencyKey":"k"}}),
    );
    let Ok(Outcome::Offload(offload)) = outcome else {
        panic!("a tailnet peer is looked up off the lock")
    };
    let answer = offload.run_inline(&mut core);
    assert_eq!(
        errors::kind(&answer.err().unwrap()),
        ErrorKind::Unauthorized
    );
    drop(core);
    std::fs::remove_dir_all(directory).unwrap();
}
