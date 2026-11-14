use super::*;
use crate::ports::testing::{FixedClock, MemoryStore};
use soyeht_protocol::ErrorKind;

/// The signed app's rule, which every system can run.
struct SignedAppRule;

impl crate::hooks::Hooks for SignedAppRule {
    fn pairing_approval(&self) -> Option<Approval> {
        Some(Approval::SignedApp)
    }
}

fn pending(expires: i64) -> Pending {
    Pending {
        id: "request".into(),
        code: "123456".into(),
        name: "Phone".into(),
        key: "key".into(),
        challenge: [7; 32],
        proved: false,
        approved: None,
        token: None,
        device: None,
        expires,
        node: None,
    }
}

#[test]
fn pairing_rule_uses_the_clock_and_store_ports_for_expiry() {
    let mut store = MemoryStore::default();
    store
        .pair_requests
        .insert("request".into(), serde_json::to_value(pending(11)).unwrap());

    assert_eq!(
        active_pending(&store, &FixedClock(10_000), "request")
            .unwrap()
            .name,
        "Phone"
    );
    let error = active_pending(&store, &FixedClock(11_000), "request").unwrap_err();
    assert_eq!(errors::kind(&error), ErrorKind::Unauthorized);
}

#[tokio::test]
async fn pairing_is_decided_by_the_mac_app_and_never_by_a_paired_device() {
    use crate::outbox;
    use soyeht_protocol::Actor;
    let directory = std::env::temp_dir().join(format!("soyeht-pairing-roles-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let mut core = Core::open_with(&directory, std::sync::Arc::new(SignedAppRule)).unwrap();
    let (local_outbox, _local) = outbox::channel();
    // A shell or script on the local socket, and the signed app (a peer the system attests).
    let local = core.connect(true, false, local_outbox).unwrap();
    let (app_outbox, _app) = outbox::channel();
    let app = core.connect(true, true, app_outbox).unwrap();
    for connection in [&local, &app] {
        let hello = json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{
            "client":"roles-test","clientVersion":"1","protocolVersion":1}});
        core.dispatch(connection, hello).unwrap();
    }
    let (device_outbox, _device) = outbox::channel();
    let phone = core.connect(false, false, device_outbox).unwrap();
    core.clients.get_mut(&phone).unwrap().actor = Some(Actor {
        role: Role::HumanDevice,
        client: "phone".into(),
        device_id: Some("device-1".into()),
        pane_id: None,
    });
    // The error kind of each call, or `None` when it went through.
    let mut kinds = |connection: &str| {
        let calls = [
            ("device.pair.start", json!({})),
            (
                "device.pair.decide",
                json!({"requestId":"r","approve":true}),
            ),
            ("device.revoke", json!({"deviceId":"device-2"})),
            ("device.revoke", json!({"deviceId":"device-1"})),
        ];
        calls.map(|(method, mut params)| {
            params["idempotencyKey"] = json!(Uuid::new_v4().to_string());
            let request = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
            core.dispatch(connection, request)
                .err()
                .map(|error| errors::kind(&error))
        })
    };

    // A device is refused all but its own revocation, which gets as far as the lookup.
    assert_eq!(
        kinds(&phone),
        [
            Some(ErrorKind::ForbiddenRole),
            Some(ErrorKind::ForbiddenRole),
            Some(ErrorKind::ForbiddenRole),
            Some(ErrorKind::NotFound)
        ]
    );
    // The app keeps all of them: the window opens, the rest find nothing to act on.
    let app_kinds = || {
        [
            None,
            Some(ErrorKind::Unauthorized),
            Some(ErrorKind::NotFound),
            Some(ErrorKind::NotFound),
        ]
    };
    assert_eq!(kinds(&app), app_kinds());
    // A shell cannot open a window, decide a request or revoke a device, on any system.
    let refused = || Some(ErrorKind::ForbiddenRole);
    assert_eq!(kinds(&local), [refused(), refused(), refused(), refused()]);
    drop(core);
    std::fs::remove_dir_all(directory).unwrap();
}

/// An engine that is not signed refuses even the app, and says why.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn an_unsigned_engine_says_why_it_cannot_tell_the_app_from_a_shell() {
    let (client, _server) = tokio::net::UnixStream::pair().unwrap();
    let token = ui_attestation::peer_audit_token(&client).unwrap();
    assert!(!ui_attestation::signed_ui_peer(
        token,
        &soyeht_protocol::profile::DEV
    ));
    assert!(ui_attestation::engine_cannot_attest());
    assert!(
        refusal(true, Approval::SignedApp)
            .to_string()
            .contains("does not carry the Soyeht signature")
    );
    assert!(
        !refusal(false, Approval::SignedApp)
            .to_string()
            .contains("does not carry the Soyeht signature")
    );
}

/// Where nothing is attested, the refusal says the app cannot yet prove itself, not that the
/// person should use an app that has no way to be believed.
#[cfg(not(target_os = "macos"))]
#[test]
fn a_system_without_attestation_says_the_app_cannot_yet_prove_itself() {
    assert!(!ui_attestation::available());
    let message = refusal(true, Approval::SignedApp).to_string();
    assert!(message.contains("cannot yet prove itself"), "{message}");
    assert!(message.contains("Soyeht app"), "{message}");
}

#[tokio::test]
async fn tailnet_reachability_reaches_status_pairing_and_live_clients() {
    use crate::{dispatch::Outcome, network, outbox};
    use soyeht_protocol::{EngineReachabilityReason, frame::Frame};
    let directory = std::env::temp_dir().join(format!("soyeht-reachability-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let mut core = Core::open(&directory).unwrap();
    let (outbox, mut receiver) = outbox::channel();
    let connection = core.connect(true, true, outbox).unwrap();
    let _challenge = receiver.next().await.unwrap().unwrap();
    let hello = json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{
        "client":"reachability-test","clientVersion":"1","protocolVersion":1}});
    core.dispatch(&connection, hello).unwrap();
    core.clients.get_mut(&connection).unwrap().events = true;
    let call = |core: &mut Core, id: u32, method: &str| {
        let params = json!({"idempotencyKey":Uuid::new_v4().to_string()});
        let request = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        match core.dispatch(&connection, request).unwrap() {
            Outcome::Reply(result) => result,
            _ => panic!("{method} did not reply"),
        }
    };
    let loopback = core.pairing.address.clone();

    let missing = network::unreachable(EngineReachabilityReason::TailnetAddressNotFound);
    let update = |reachability, pair_address| TailnetUpdate {
        reachability,
        pair_address,
    };
    core.apply_tailnet(update(missing.clone(), None), &loopback)
        .unwrap();
    let status = call(&mut core, 2, "engine.status");
    assert_eq!(
        status["reachability"],
        json!({"remote":false,"reason":"tailnet_address_not_found"})
    );
    let pair = call(&mut core, 3, "device.pair.start");
    assert_eq!(pair["reachability"]["remote"], false);
    assert!(pair["uri"].as_str().unwrap().contains(&loopback));

    let tailnet: std::net::SocketAddr = "100.64.0.9:7777".parse().unwrap();
    core.apply_tailnet(update(network::reachable(), Some(tailnet)), &loopback)
        .unwrap();
    let mut announced = vec![];
    while let Ok(Some(packet)) =
        tokio::time::timeout(std::time::Duration::from_millis(50), receiver.next())
            .await
            .unwrap_or(Ok(None))
    {
        if let Frame::Json(bytes) = packet.frame {
            let message: Value = serde_json::from_slice(&bytes).unwrap();
            if message["method"] == "engine.reachability_changed" {
                announced.push(message["params"].clone());
            }
        }
    }
    assert_eq!(
        announced,
        [
            json!({"remote":false,"reason":"tailnet_address_not_found"}),
            json!({"remote":true})
        ]
    );
    assert_eq!(
        call(&mut core, 4, "engine.status")["reachability"]["remote"],
        true
    );
    let pair = call(&mut core, 5, "device.pair.start");
    assert!(
        pair["uri"]
            .as_str()
            .unwrap()
            .contains("address=100.64.0.9:7777")
    );
    drop(core);
    std::fs::remove_dir_all(directory).unwrap();
}
