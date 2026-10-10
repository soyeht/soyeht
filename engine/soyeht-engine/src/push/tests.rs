//! The engine path from a published question to the APNs request, with a
//! mock APNs sender in place of the HTTP/2 transport.

use super::{
    Notifier,
    apns::{self, ApnsRequest, ApnsResponse, Credentials, ProviderToken, Transport},
    payload::PERMISSION_CATEGORY,
    worker::Worker,
};
use crate::{
    core::Core, dispatch::Outcome, errors, model::RunningPane, outbox, ports::testing::FixedClock,
    screen::Screen,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use soyeht_protocol::{Actor, ErrorKind, PushEnvironment, Role};
use soyeht_store::domain::PaneRecord;
use std::{collections::VecDeque, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc;

const DEVICE: &str = "device-1";
const TOKEN: &str = "00fc13adff785122b4ad28809a3420982341241421348097878e577c991de8f0";
const PANE: &str = "pane-1";
const WORKSPACE: &str = "workspace-1";
const REQUEST: &str = "request-1";
const NOW_MS: i64 = 1_790_000_000_000;

/// Records each request and answers from a script, 200 when it runs out.
struct MockApns {
    sent: mpsc::UnboundedSender<ApnsRequest>,
    responses: VecDeque<ApnsResponse>,
}

impl Transport for MockApns {
    async fn send(&mut self, request: ApnsRequest) -> std::io::Result<ApnsResponse> {
        let _ = self.sent.send(request);
        Ok(self.responses.pop_front().unwrap_or(ApnsResponse {
            status: 200,
            reason: None,
        }))
    }
}

struct Rig {
    core: Core,
    root: PathBuf,
    local: String,
    device: String,
    sent: mpsc::UnboundedReceiver<ApnsRequest>,
    forgotten: mpsc::UnboundedReceiver<(String, String)>,
    _receivers: Vec<outbox::Receiver>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn response(status: u16, reason: &str) -> ApnsResponse {
    ApnsResponse {
        status,
        reason: Some(reason.into()),
    }
}

fn request(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
}

fn agent_pane(root: &std::path::Path) -> RunningPane {
    let pane = serde_json::from_value(json!({
        "paneId":PANE,"workspaceId":WORKSPACE,"name":"Claude","cwd":"/tmp","mode":"agent",
        "state":"waiting_for_you","source":"protocol","confidence":1.0,"phrase":"Waiting",
        "generation":1,"cols":80,"rows":24,"record":false
    }))
    .unwrap();
    RunningPane {
        stored: PaneRecord {
            pane,
            closed: false,
            instance_key: "instance".into(),
            instance_id: None,
            supervisor_id: None,
            sequence_base: 0,
        },
        screen: Screen::open(&root.join("sessions").join(PANE), false).unwrap(),
    }
}

impl Rig {
    fn new(responses: Vec<ApnsResponse>) -> Self {
        let root = std::env::temp_dir().join(format!("sn-push-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(apns::CONFIG_DIRECTORY)).unwrap();
        apns::tests::install(&root, false);
        let mut core = Core::open(&root).unwrap();
        core.clock = Arc::new(FixedClock(NOW_MS));
        let (sent_tx, sent) = mpsc::unbounded_channel();
        let (forget_tx, forgotten) = mpsc::unbounded_channel();
        core.push = Notifier::spawn(Worker {
            transport: MockApns {
                sent: sent_tx,
                responses: responses.into(),
            },
            token: ProviderToken::new(Credentials::load(&root).unwrap()),
            clock: Arc::clone(&core.clock),
            forget: Box::new(move |device, token| {
                let _ = forget_tx.send((device, token));
            }),
        });
        core.panes.insert(PANE.into(), agent_pane(&root));
        core.store
            .mutate("test", "pair", "pair", |change| {
                change.open_pair_window("code", i64::MAX, 0)?;
                change.consume_pair_window_and_insert_device(
                    "code",
                    DEVICE,
                    &json!({"deviceId":DEVICE,"name":"Phone","fingerprint":"f","revoked":false}),
                    b"key",
                    b"token-hash",
                )?;
                Ok((json!({}), vec![]))
            })
            .unwrap();
        let mut receivers = Vec::new();
        let mut connect = |local: bool, actor: Actor, scope: &str| {
            let (sender, receiver) = outbox::channel();
            receivers.push(receiver);
            let id = core.connect(local, false, sender).unwrap();
            let client = core.clients.get_mut(&id).unwrap();
            client.actor = Some(actor);
            client.scope = scope.into();
            id
        };
        let local = connect(
            true,
            Actor {
                role: Role::HumanLocal,
                client: "mac-dev".into(),
                device_id: None,
                pane_id: None,
            },
            "local:test",
        );
        let device = connect(
            false,
            Actor {
                role: Role::HumanDevice,
                client: "iphone-dev".into(),
                device_id: Some(DEVICE.into()),
                pane_id: None,
            },
            &format!("device:{DEVICE}"),
        );
        Self {
            core,
            root,
            local,
            device,
            sent,
            forgotten,
            _receivers: receivers,
        }
    }

    fn call(&mut self, connection: &str, method: &str, params: Value) -> errors::Result<Value> {
        match self.core.dispatch(connection, request(method, params))? {
            Outcome::Reply(value) => Ok(value),
            _ => panic!("expected a reply"),
        }
    }

    fn register(&mut self) {
        let device = self.device.clone();
        let result = self
            .call(
                &device,
                "push.register",
                json!({"platform":"apns","token":TOKEN,"environment":"sandbox","idempotencyKey":"register"}),
            )
            .unwrap();
        assert_eq!(result, json!({"deviceId":DEVICE}));
    }

    fn ask(&mut self) {
        self.core
            .system_event(
                "pane.permission_requested",
                json!({
                    "paneId":PANE,"requestId":REQUEST,"generation":1,"promptId":"prompt",
                    "title":"Run command","detail":"",
                    "presentation":{"command":"cargo publish","fileChanges":[],"previewOmitted":false},
                    "options":[
                        {"optionId":"once","kind":"allow_once","name":"Allow"},
                        {"optionId":"deny","kind":"reject_once","name":"Reject"}
                    ],
                    "expiresAt":"2026-09-21T15:00:00.000Z"
                }),
            )
            .unwrap();
    }

    fn answer(&mut self) {
        self.core
            .system_event(
                "pane.permission_answered",
                json!({"paneId":PANE,"requestId":REQUEST,"result":"selected","optionId":"once","by":"iphone"}),
            )
            .unwrap();
    }

    async fn next_request(&mut self) -> Option<ApnsRequest> {
        tokio::time::timeout(Duration::from_secs(300), self.sent.recv())
            .await
            .ok()
            .flatten()
    }
}

fn header<'a>(request: &'a ApnsRequest, name: &str) -> &'a str {
    request
        .headers
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.as_str())
        .unwrap_or_else(|| panic!("missing {name}"))
}

#[tokio::test(start_paused = true)]
async fn a_question_reaches_each_registered_device_once_with_apns_headers() {
    let mut rig = Rig::new(vec![]);
    rig.ask();
    assert!(
        rig.next_request().await.is_none(),
        "no device is registered yet"
    );
    rig.register();
    rig.ask();
    let sent = rig.next_request().await.unwrap();
    assert_eq!(sent.environment, PushEnvironment::Sandbox);
    assert_eq!(apns::host(&sent.environment), "api.sandbox.push.apple.com");
    assert_eq!(sent.path, format!("/3/device/{TOKEN}"));
    assert_eq!(header(&sent, "apns-push-type"), "alert");
    assert_eq!(header(&sent, "apns-topic"), "com.example.app");
    assert_eq!(header(&sent, "apns-priority"), "10");
    assert_eq!(header(&sent, "apns-collapse-id"), REQUEST);
    assert_eq!(header(&sent, "apns-expiration"), "1790002800");
    assert!(uuid::Uuid::parse_str(header(&sent, "apns-id")).is_ok());
    let bearer = header(&sent, "authorization")
        .strip_prefix("bearer ")
        .unwrap();
    let claims: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(bearer.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(claims, json!({"iss":"TEAM123456","iat":NOW_MS / 1_000}));
    let payload: Value = serde_json::from_slice(&sent.body).unwrap();
    assert_eq!(payload["aps"]["category"], PERMISSION_CATEGORY);
    assert_eq!(payload["aps"]["alert"]["title"], "Agent needs approval");
    assert_eq!(
        payload["aps"]["alert"]["body"],
        "Run command · cargo publish"
    );
    assert_eq!(
        payload["soyeht"],
        json!({
            "engineId":rig.core.store.db_id,"workspaceId":WORKSPACE,"paneId":PANE,
            "requestId":REQUEST,"kind":"permission","allowOptionId":"once","denyOptionId":"deny"
        })
    );
    rig.ask();
    assert!(
        rig.next_request().await.is_none(),
        "a repeated question is not pushed again"
    );
}

#[tokio::test(start_paused = true)]
async fn retries_stop_when_the_request_is_answered() {
    let mut rig = Rig::new(vec![
        response(503, "ServiceUnavailable"),
        response(503, "ServiceUnavailable"),
    ]);
    rig.register();
    rig.ask();
    assert!(rig.next_request().await.is_some());
    let retry = tokio::time::timeout(Duration::from_secs(10), rig.sent.recv()).await;
    assert!(retry.is_ok(), "a 5xx is retried");
    rig.answer();
    assert!(
        rig.next_request().await.is_none(),
        "no retry after the answer"
    );
}

#[tokio::test(start_paused = true)]
async fn an_expired_provider_token_is_renewed_and_a_stale_device_token_forgotten() {
    let mut rig = Rig::new(vec![
        response(403, "ExpiredProviderToken"),
        response(410, "Unregistered"),
    ]);
    rig.register();
    rig.ask();
    let first = rig.next_request().await.unwrap();
    let second = rig.next_request().await.unwrap();
    assert_ne!(
        header(&first, "authorization"),
        header(&second, "authorization")
    );
    assert_eq!(
        rig.forgotten.recv().await.unwrap(),
        (DEVICE.to_owned(), TOKEN.to_owned())
    );
    assert!(rig.next_request().await.is_none(), "410 is not retried");
    rig.core.forget_push_token(DEVICE, TOKEN).unwrap();
    assert!(rig.core.store.push_registrations().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn only_a_paired_device_registers_and_revocation_removes_it() {
    let mut rig = Rig::new(vec![]);
    let local = rig.local.clone();
    let refused = rig
        .call(
            &local,
            "push.register",
            json!({"platform":"apns","token":TOKEN,"environment":"production","idempotencyKey":"mac"}),
        )
        .unwrap_err();
    assert_eq!(errors::kind(&refused), ErrorKind::ForbiddenRole);
    rig.register();
    assert_eq!(rig.core.store.push_registrations().unwrap().len(), 1);
    let device = rig.device.clone();
    rig.call(&device, "push.unregister", json!({"idempotencyKey":"off"}))
        .unwrap();
    assert!(rig.core.store.push_registrations().unwrap().is_empty());
    rig.register();
    rig.call(
        &local,
        "device.revoke",
        json!({"deviceId":DEVICE,"idempotencyKey":"revoke"}),
    )
    .unwrap();
    assert!(rig.core.store.push_registrations().unwrap().is_empty());
    rig.ask();
    assert!(rig.next_request().await.is_none());
}
