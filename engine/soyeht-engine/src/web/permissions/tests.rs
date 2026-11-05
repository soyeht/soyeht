use super::*;
use crate::web::test_support::{connect_local, next_json, reply, request, temp_root};
use crate::{
    dispatch::Outcome, model::RunningPane, outbox, ports::testing::FixedClock, screen::Screen,
};
use serde_json::json;
use soyeht_protocol::{Actor, ErrorKind, WebCommandKind};
use std::{path::PathBuf, sync::Arc};

async fn connect(core: &mut Core, host: bool) -> (String, outbox::Receiver) {
    let capabilities = if host { json!(["web_host"]) } else { json!([]) };
    connect_local(core, "web-permission-test", capabilities).await
}

async fn setup() -> (
    Core,
    String,
    outbox::Receiver,
    String,
    outbox::Receiver,
    String,
    PathBuf,
) {
    let root = temp_root();
    let mut core = Core::open(&root).unwrap();
    core.clock = Arc::new(FixedClock(1_000));
    let (host, host_receiver) = connect(&mut core, true).await;
    let (caller, caller_receiver) = connect(&mut core, false).await;
    let workspace = reply(
        &mut core,
        &caller,
        request(
            2,
            "workspace.create",
            json!({"name":"Web","path":root,
            "idempotencyKey":"workspace"}),
        ),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let pane = reply(
        &mut core,
        &caller,
        request(
            3,
            "web.open",
            json!({"workspaceId":workspace,
            "url":"https://example.test/path","idempotencyKey":"open"}),
        ),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut agent = core.panes[&pane].stored.clone();
    agent.pane.pane_id = "agent-pane".into();
    agent.pane.mode = PaneMode::Agent;
    agent.pane.url = None;
    agent.pane.title = None;
    agent.pane.viewport = None;
    agent.pane.incognito = None;
    agent.pane.web_session = None;
    let agent_screen = Screen::open(&root.join("sessions/agent-pane"), false).unwrap();
    core.panes.insert(
        agent.pane.pane_id.clone(),
        RunningPane {
            stored: agent,
            screen: agent_screen,
        },
    );
    core.clients.get_mut(&caller).unwrap().actor = Some(Actor {
        role: Role::Agent,
        client: "bound-agent".into(),
        device_id: None,
        pane_id: Some("agent-pane".into()),
    });
    (
        core,
        host,
        host_receiver,
        caller,
        caller_receiver,
        pane,
        root,
    )
}

/// The host's answer to `command`, a `web.command` request it received.
fn host_answers(core: &mut Core, host: &str, command: &Value, value: Value) {
    core.web_host_response(
        host,
        json!({"jsonrpc":"2.0","id":command["id"],
            "result":{"paneId":command["params"]["paneId"],"value":value}}),
    )
    .unwrap();
}

fn click(pane: &str, operation: &str) -> (Value, WebCommandParams) {
    let raw = json!({"paneId":pane,"ref":"node-1","idempotencyKey":operation});
    let params = WebCommandParams {
        pane_id: pane.into(),
        command: WebCommandKind::Click,
        args: json!({"ref":"node-1"}),
        timeout_ms: 30_000,
        operation_key: operation.into(),
    };
    (raw, params)
}

#[tokio::test]
async fn agent_web_commands_publish_one_bounded_driving_interval() {
    let (mut core, host, mut host_rx, caller, mut caller_rx, pane, root) = setup().await;
    let (viewer, mut viewer_rx) = connect(&mut core, false).await;
    core.clients.get_mut(&viewer).unwrap().events = true;

    let (raw, params) = click(&pane, "driving");
    let id = core
        .queue_web_command(&caller, json!(9), "web.click", &raw, params)
        .unwrap();
    let started = next_json(&mut viewer_rx).await;
    assert_eq!(started["method"], "web.agent_driving");
    assert_eq!(started["params"]["paneId"], pane);
    assert_eq!(started["params"]["on"], true);
    assert_eq!(started["params"]["domain"], "example.test");
    let command = next_json(&mut host_rx).await;
    assert_eq!(command["id"], id);

    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":id,"result":{
            "paneId":pane,"cookiesPresent":false,"value":{"completed":true}}}),
    )
    .unwrap();
    let stopped = next_json(&mut viewer_rx).await;
    assert_eq!(stopped["method"], "web.agent_driving");
    assert_eq!(stopped["params"]["paneId"], pane);
    assert_eq!(stopped["params"]["on"], false);
    assert!(stopped["params"].get("domain").is_none());
    assert_eq!(next_json(&mut caller_rx).await["result"]["completed"], true);
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn signed_in_session_waits_for_a_person_and_workspace_grant_is_reused() {
    let (mut core, host, mut host_rx, caller, mut caller_rx, pane, root) = setup().await;
    let (raw, params) = click(&pane, "click-one");
    let id = core
        .queue_web_command(&caller, json!(10), "web.click", &raw, params)
        .unwrap();
    let probe = next_json(&mut host_rx).await;
    assert_eq!(
        probe["params"]["args"][INTERNAL_ACCESS_KEY]["session"],
        "probe"
    );
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":id,"result":{"paneId":pane,"cookiesPresent":true}}),
    )
    .unwrap();
    let pending = core
        .store
        .open_web_permission(
            &pane,
            &PermissionRequestKind::WebSession,
            "example.test",
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(pending.public.options.len(), 2);
    assert_eq!(
        core.panes[&pane].stored.pane.web_session,
        Some(WebSessionState::PermissionRequired)
    );
    let answer = reply(
        &mut core,
        &host,
        request(
            11,
            "pane.permission.answer",
            json!({"paneId":pane,
            "requestId":pending.request_id,"optionId":"workspace","idempotencyKey":"answer"}),
        ),
    );
    assert_eq!(answer["by"], "mac");
    let resumed = next_json(&mut host_rx).await;
    assert_eq!(
        resumed["params"]["args"][INTERNAL_ACCESS_KEY]["session"],
        "granted"
    );
    host_answers(&mut core, &host, &resumed, json!({"completed":true}));
    assert_eq!(next_json(&mut caller_rx).await["result"]["completed"], true);

    let (raw, params) = click(&pane, "click-two");
    core.queue_web_command(&caller, json!(12), "web.click", &raw, params)
        .unwrap();
    let granted = next_json(&mut host_rx).await;
    assert_eq!(
        granted["params"]["args"][INTERNAL_ACCESS_KEY]["session"],
        "granted"
    );
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn evaluation_waits_for_one_time_permission_and_reuses_its_receipt() {
    let (mut core, host, mut host_rx, caller, mut caller_rx, pane, root) = setup().await;
    let raw = json!({"paneId":pane,"js":"document.title","idempotencyKey":"eval-one"});
    assert!(matches!(
        core.dispatch(&caller, request(20, "web.evaluate", raw.clone())),
        Ok(Outcome::Queued)
    ));
    let pending = core
        .store
        .web_permission_for_operation(&pane, &PermissionRequestKind::WebEvaluate, "eval-one")
        .unwrap()
        .unwrap();
    assert_eq!(pending.public.options[0].option_id, "allow");
    let answer = reply(
        &mut core,
        &host,
        request(
            21,
            "pane.permission.answer",
            json!({"paneId":pane,
            "requestId":pending.request_id,"optionId":"allow","idempotencyKey":"allow"}),
        ),
    );
    assert_eq!(answer["optionId"], "allow");
    let allowed = next_json(&mut host_rx).await;
    assert_eq!(
        allowed["params"]["args"][INTERNAL_ACCESS_KEY]["evaluation"],
        true
    );
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":allowed["id"],"result":{
            "paneId":pane,"cookiesPresent":false,"value":{"value":"Example"}}}),
    )
    .unwrap();
    assert_eq!(
        next_json(&mut caller_rx).await["result"]["value"],
        "Example"
    );
    let repeated = core.dispatch(&caller, request(22, "web.evaluate", raw));
    let Outcome::Reply(repeated) = repeated.unwrap() else {
        panic!("evaluation retry should use the durable mutation receipt")
    };
    assert_eq!(repeated["value"], "Example");
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn evaluation_denial_is_durable_for_the_operation() {
    let (mut core, host, _host_rx, caller, mut caller_rx, pane, root) = setup().await;
    let raw = json!({"paneId":pane,"js":"document.title","idempotencyKey":"eval-deny"});
    assert!(matches!(
        core.dispatch(&caller, request(24, "web.evaluate", raw.clone())),
        Ok(Outcome::Queued)
    ));
    let pending = core
        .store
        .web_permission_for_operation(&pane, &PermissionRequestKind::WebEvaluate, "eval-deny")
        .unwrap()
        .unwrap();
    reply(
        &mut core,
        &host,
        request(
            25,
            "pane.permission.answer",
            json!({"paneId":pane,"requestId":pending.request_id,
                "optionId":"deny","idempotencyKey":"deny"}),
        ),
    );
    assert_eq!(
        next_json(&mut caller_rx).await["error"]["data"]["kind"],
        "unauthorized"
    );
    let denied = core.queue_web_command(
        &caller,
        json!(26),
        "web.evaluate",
        &raw,
        WebCommandParams {
            pane_id: pane,
            command: WebCommandKind::Evaluate,
            args: json!({"js":"document.title"}),
            timeout_ms: 30_000,
            operation_key: "eval-deny".into(),
        },
    );
    assert!(denied.is_err_and(|error| errors::kind(&error) == ErrorKind::Unauthorized));
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn expired_permission_is_answered_by_the_system_and_fails_the_waiter() {
    let (mut core, host, mut host_rx, caller, mut caller_rx, pane, root) = setup().await;
    let (raw, params) = click(&pane, "expires");
    let id = core
        .queue_web_command(&caller, json!(30), "web.click", &raw, params)
        .unwrap();
    let _ = next_json(&mut host_rx).await;
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":id,"result":{"paneId":pane,"cookiesPresent":true}}),
    )
    .unwrap();
    core.clock = Arc::new(FixedClock(1_000 + QUESTION_TIMEOUT_MS));
    core.advance_web_permissions().unwrap();
    let expired = next_json(&mut caller_rx).await;
    assert_eq!(expired["error"]["data"]["kind"], "unauthorized");
    assert_eq!(
        core.panes[&pane].stored.pane.web_session,
        Some(WebSessionState::Anonymous)
    );
    assert!(
        core.store
            .expired_web_permissions(core.now_ms())
            .unwrap()
            .is_empty()
    );
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn durable_permission_answer_survives_a_host_disconnect() {
    let (mut core, host, _host_rx, caller, mut caller_rx, pane, root) = setup().await;
    let raw = json!({"paneId":pane,"js":"document.title","idempotencyKey":"host-gone"});
    assert!(matches!(
        core.dispatch(&caller, request(40, "web.evaluate", raw)),
        Ok(Outcome::Queued)
    ));
    let pending = core
        .store
        .web_permission_for_operation(&pane, &PermissionRequestKind::WebEvaluate, "host-gone")
        .unwrap()
        .unwrap();
    core.clients.get_mut(&host).unwrap().web_host = false;
    let answer = reply(
        &mut core,
        &host,
        request(
            41,
            "pane.permission.answer",
            json!({"paneId":pane,"requestId":pending.request_id,
                "optionId":"allow","idempotencyKey":"host-gone-answer"}),
        ),
    );
    assert_eq!(answer["optionId"], "allow");
    assert_eq!(
        next_json(&mut caller_rx).await["error"]["data"]["kind"],
        "web_host_unavailable"
    );
    assert!(
        core.store
            .web_permission_for_operation(&pane, &PermissionRequestKind::WebEvaluate, "host-gone",)
            .unwrap()
            .unwrap()
            .result
            .is_some()
    );
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn bound_agents_cannot_open_or_control_web_panes_in_another_workspace() {
    let (mut core, _host, _host_rx, caller, _caller_rx, pane, root) = setup().await;
    let open = core.dispatch(
        &caller,
        request(
            50,
            "web.open",
            json!({"workspaceId":"another-workspace","url":"example.test",
                "idempotencyKey":"cross-open"}),
        ),
    );
    assert!(open.is_err_and(|error| errors::kind(&error) == ErrorKind::ForbiddenRole));

    core.panes.get_mut(&pane).unwrap().stored.pane.workspace_id = "another-workspace".into();
    let command = core.dispatch(
        &caller,
        request(
            51,
            "web.click",
            json!({"paneId":pane,"ref":"e1","idempotencyKey":"cross-click"}),
        ),
    );
    assert!(command.is_err_and(|error| errors::kind(&error) == ErrorKind::ForbiddenRole));
    crate::test_paths::remove_after(core, root);
}

const OPENED: &str = "https://example.test/";

/// The pane a `web.open` made, which is the one that is not `known`.
fn opened_pane(core: &Core, known: &str) -> String {
    let opened: Vec<_> = core
        .panes
        .values()
        .filter(|running| running.stored.pane.mode == PaneMode::Web)
        .filter(|running| running.stored.pane.pane_id != known)
        .map(|running| running.stored.pane.pane_id.clone())
        .collect();
    assert_eq!(opened.len(), 1);
    opened[0].clone()
}

/// Whether nothing reaches `receiver` in a short while.
async fn is_silent(receiver: &mut outbox::Receiver) -> bool {
    tokio::time::timeout(std::time::Duration::from_millis(100), receiver.next())
        .await
        .is_err()
}

fn open_request(id: u64, key: &str) -> Value {
    request(
        id,
        "web.open",
        json!({"url":OPENED,"newPane":true,"idempotencyKey":key}),
    )
}

#[tokio::test]
async fn an_agent_opened_pane_asks_the_host_about_the_session_before_it_loads() {
    let (mut core, host, mut host_rx, caller, mut caller_rx, known, root) = setup().await;
    let (viewer, mut viewer_rx) = connect(&mut core, false).await;
    core.clients.get_mut(&viewer).unwrap().events = true;

    let queued = core.dispatch(&caller, open_request(60, "agent-open"));
    assert!(matches!(queued, Ok(Outcome::Queued)));
    let pane = opened_pane(&core, &known);
    let stored = &core.panes[&pane].stored.pane;
    assert_eq!(stored.url, None);
    assert_eq!(stored.incognito, Some(false));
    assert_eq!(stored.web_session, Some(WebSessionState::Anonymous));

    let probe = next_json(&mut host_rx).await;
    assert_eq!(probe["method"], "web.command");
    assert_eq!(probe["params"]["paneId"], pane);
    assert_eq!(probe["params"]["command"], "navigate");
    assert_eq!(probe["params"]["args"]["url"], OPENED);
    assert_eq!(
        probe["params"]["args"][INTERNAL_ACCESS_KEY]["session"],
        "probe"
    );
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":probe["id"],"result":{"paneId":pane,"cookiesPresent":true}}),
    )
    .unwrap();

    let mut asked = None;
    while asked.is_none() {
        let event = next_json(&mut viewer_rx).await;
        if event["method"] == "pane.permission_requested" {
            asked = Some(event["params"].clone());
        }
    }
    let asked = asked.unwrap();
    assert_eq!(asked["kind"], "web_session");
    assert_eq!(asked["domain"], "example.test");
    assert_eq!(
        core.panes[&pane].stored.pane.web_session,
        Some(WebSessionState::PermissionRequired)
    );
    assert_eq!(core.panes[&pane].stored.pane.url, None);

    reply(
        &mut core,
        &host,
        request(
            61,
            "pane.permission.answer",
            json!({"paneId":pane,"requestId":asked["requestId"],
                "optionId":"workspace","idempotencyKey":"answer-open"}),
        ),
    );
    let resumed = next_json(&mut host_rx).await;
    assert_eq!(resumed["params"]["args"]["url"], OPENED);
    assert_eq!(
        resumed["params"]["args"][INTERNAL_ACCESS_KEY]["session"],
        "granted"
    );
    host_answers(&mut core, &host, &resumed, json!({"accepted":true}));
    let answer = next_json(&mut caller_rx).await;
    assert_eq!(answer["id"], 60);
    assert_eq!(answer["result"], json!({"paneId":pane}));
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn an_agent_opened_pane_without_a_session_answers_once_the_host_accepts_the_address() {
    let (mut core, host, mut host_rx, caller, mut caller_rx, known, root) = setup().await;
    assert!(matches!(
        core.dispatch(&caller, open_request(62, "agent-open-anonymous")),
        Ok(Outcome::Queued)
    ));
    let pane = opened_pane(&core, &known);
    let probe = next_json(&mut host_rx).await;
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":probe["id"],"result":{
            "paneId":pane,"cookiesPresent":false,"value":{"accepted":true}}}),
    )
    .unwrap();
    assert_eq!(
        next_json(&mut caller_rx).await["result"],
        json!({"paneId":pane})
    );
    // The same call again is the receipt of the first: no second pane, no second navigation.
    let Ok(Outcome::Reply(repeated)) =
        core.dispatch(&caller, open_request(63, "agent-open-anonymous"))
    else {
        panic!("a repeated open should answer from its receipt")
    };
    assert_eq!(repeated, json!({"paneId":pane}));
    assert_eq!(opened_pane(&core, &known), pane);
    assert!(is_silent(&mut host_rx).await);
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn an_agent_opened_private_pane_and_a_persons_pane_load_directly() {
    let (mut core, _host, mut host_rx, caller, _caller_rx, known, root) = setup().await;
    let Ok(Outcome::Reply(private)) = core.dispatch(
        &caller,
        request(
            64,
            "web.open",
            json!({"url":OPENED,"incognito":true,"idempotencyKey":"private-open"}),
        ),
    ) else {
        panic!("a private pane needs no session question")
    };
    let pane = opened_pane(&core, &known);
    assert_eq!(private, json!({"paneId":pane}));
    assert_eq!(core.panes[&pane].stored.pane.url.as_deref(), Some(OPENED));
    assert!(is_silent(&mut host_rx).await);

    let (person, _person_rx) = connect(&mut core, false).await;
    let workspace = core.panes[&known].stored.pane.workspace_id.clone();
    let Ok(Outcome::Reply(opened)) = core.dispatch(
        &person,
        request(
            65,
            "web.open",
            json!({"workspaceId":workspace,"url":"https://person.test/","newPane":true,
                "idempotencyKey":"person-open"}),
        ),
    ) else {
        panic!("a person's pane loads at once")
    };
    let loaded = opened["paneId"].as_str().unwrap();
    assert_eq!(
        core.panes[loaded].stored.pane.url.as_deref(),
        Some("https://person.test/")
    );
    assert!(is_silent(&mut host_rx).await);
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn an_agent_open_with_no_web_host_makes_no_pane() {
    let (mut core, host, _host_rx, caller, _caller_rx, known, root) = setup().await;
    core.clients.get_mut(&host).unwrap().web_host = false;
    let before = core.panes.len();
    let opened = core.dispatch(&caller, open_request(66, "agent-open-no-host"));
    assert!(opened.is_err_and(|error| errors::kind(&error) == ErrorKind::WebHostUnavailable));
    assert_eq!(core.panes.len(), before);
    assert!(core.panes.contains_key(&known));
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn a_navigation_asks_about_the_site_it_goes_to_not_the_one_the_pane_is_on() {
    let (mut core, _host, mut host_rx, caller, _caller_rx, pane, root) = setup().await;
    let params = WebCommandParams {
        pane_id: pane,
        command: WebCommandKind::Navigate,
        args: json!({"url":"https://other.test/"}),
        timeout_ms: 30_000,
        operation_key: "navigate-other".into(),
    };
    let plan = core
        .web_permission_plan(&caller, "web.navigate", &params)
        .unwrap();
    assert_eq!(plan.domain, "other.test");
    let (_, click) = click(&params.pane_id, "click-current");
    let plan = core
        .web_permission_plan(&caller, "web.click", &click)
        .unwrap();
    assert_eq!(plan.domain, "example.test");
    assert!(is_silent(&mut host_rx).await);
    crate::test_paths::remove_after(core, root);
}
