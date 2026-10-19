use super::*;
use crate::web::test_support::{next_json, reply, request, temp_root};
use crate::{
    dispatch::Outcome, model::RunningPane, outbox, ports::testing::FixedClock, screen::Screen,
};
use serde_json::json;
use soyeht_protocol::{Actor, ErrorKind, WebCommandKind};
use std::{path::PathBuf, sync::Arc};

async fn connect(core: &mut Core, host: bool) -> (String, outbox::Receiver) {
    let (sender, mut receiver) = outbox::channel();
    let connection = core.connect(true, false, sender).unwrap();
    let _ = next_json(&mut receiver).await;
    reply(
        core,
        &connection,
        request(
            1,
            "hello",
            json!({"client":"web-permission-test","clientVersion":"1",
                "protocolVersion":1,"capabilities":if host { json!(["web_host"]) } else { json!([]) }}),
        ),
    );
    (connection, receiver)
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
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":resumed["id"],"result":{"paneId":pane,"value":{"completed":true}}}),
    )
    .unwrap();
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
