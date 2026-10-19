use super::*;
use crate::web::test_support::{next_json, reply, request, temp_root};
use crate::{dispatch::Outcome, outbox, ports::testing::FixedClock};
use serde_json::json;
use soyeht_protocol::frame::Frame;
use std::sync::Arc;

fn command_params(pane_id: &str, key: &str) -> WebCommandParams {
    serde_json::from_value(json!({
        "paneId":pane_id,"command":"navigate","args":{"action":"reload"},
        "timeoutMs":100,"operationKey":key
    }))
    .unwrap()
}

fn navigate_params(pane_id: &str, key: &str) -> Value {
    json!({"paneId":pane_id,"action":"reload","idempotencyKey":key})
}

fn human_args(args: Value) -> Value {
    let mut args = args.as_object().unwrap().clone();
    args.insert(
        "_soyehtAccess".into(),
        json!({"session":"human","evaluation":false}),
    );
    Value::Object(args)
}

fn assert_human_args(command: &Value, args: Value) {
    assert_eq!(command["params"]["args"], human_args(args));
}

async fn connect(core: &mut Core, capabilities: Value) -> (String, outbox::Receiver) {
    let (sender, mut receiver) = outbox::channel();
    let connection = core.connect(true, false, sender).unwrap();
    let challenge = next_json(&mut receiver).await;
    assert_eq!(challenge["method"], "auth.challenge");
    let welcome = reply(
        core,
        &connection,
        request(
            1,
            "hello",
            json!({
                "client":"web-host-test","clientVersion":"1","protocolVersion":1,
                "capabilities":capabilities
            }),
        ),
    );
    assert_eq!(welcome["role"], "human_local");
    (connection, receiver)
}

#[tokio::test]
async fn one_local_host_routes_typed_bounded_results_and_disconnects_fail_waiters() {
    let root = temp_root();
    let mut core = Core::open(&root).unwrap();
    core.clock = Arc::new(FixedClock(10));
    let (host, mut host_receiver) = connect(&mut core, json!(["web_host"])).await;
    let (caller, mut caller_receiver) = connect(&mut core, json!([])).await;
    let workspace = reply(
        &mut core,
        &caller,
        request(
            2,
            "workspace.create",
            json!({"name":"Web","path":root,"idempotencyKey":"workspace"}),
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
            "pane.create",
            json!({"workspaceId":workspace,"mode":"web","idempotencyKey":"pane"}),
        ),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let raw = navigate_params(&pane, "operation");
    let params = command_params(&pane, "operation");
    let command_id = core
        .queue_web_command(&caller, json!(44), "web.navigate", &raw, params.clone())
        .unwrap();
    let repeated_id = core
        .queue_web_command(&caller, json!(440), "web.navigate", &raw, params)
        .unwrap();
    assert_eq!(repeated_id, command_id);
    let command = next_json(&mut host_receiver).await;
    assert_eq!(command["id"], command_id);
    assert_eq!(command["method"], "web.command");
    assert_eq!(command["params"]["paneId"], pane);
    assert!(
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":command_id,"result":{"paneId":"wrong"}}),
        )
        .is_err()
    );
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":command_id,"result":{"paneId":pane,"value":[]}}),
    )
    .unwrap();
    let response = next_json(&mut caller_receiver).await;
    assert_eq!(response["id"], 44);
    assert_eq!(response["result"], json!({"paneId":pane,"accepted":true}));
    let repeated = next_json(&mut caller_receiver).await;
    assert_eq!(repeated["id"], 440);
    assert_eq!(repeated["result"], response["result"]);
    let Outcome::Reply(stored) = core
        .dispatch(
            &caller,
            request(441, "web.navigate", navigate_params(&pane, "operation")),
        )
        .unwrap()
    else {
        panic!("stored web navigation should reply immediately")
    };
    assert_eq!(stored, response["result"]);

    let raw = navigate_params(&pane, "timeout");
    core.queue_web_command(
        &caller,
        json!(45),
        "web.navigate",
        &raw,
        command_params(&pane, "timeout"),
    )
    .unwrap();
    let _ = next_json(&mut host_receiver).await;
    core.clock = Arc::new(FixedClock(111));
    core.advance_web_commands().unwrap();
    let timeout = next_json(&mut caller_receiver).await;
    assert_eq!(timeout["id"], 45);
    assert_eq!(timeout["error"]["data"]["kind"], "web_host_unavailable");

    core.clock = Arc::new(FixedClock(200));
    let raw = navigate_params(&pane, "disconnect");
    core.queue_web_command(
        &caller,
        json!(46),
        "web.navigate",
        &raw,
        command_params(&pane, "disconnect"),
    )
    .unwrap();
    let _ = next_json(&mut host_receiver).await;
    core.disconnect_web_host(&host).unwrap();
    let disconnected = next_json(&mut caller_receiver).await;
    assert_eq!(disconnected["id"], 46);
    assert_eq!(
        disconnected["error"]["data"]["kind"],
        "web_host_unavailable"
    );
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn a_second_live_host_and_nonlocal_host_claims_are_rejected() {
    let root = temp_root();
    let mut core = Core::open(&root).unwrap();
    let (_host, _host_receiver) = connect(&mut core, json!(["web_host"])).await;
    let (sender, mut receiver) = outbox::channel();
    let second = core.connect(true, false, sender).unwrap();
    let _ = next_json(&mut receiver).await;
    let duplicate = core.dispatch(
        &second,
        request(
            1,
            "hello",
            json!({"client":"second","clientVersion":"1","protocolVersion":1,
                "capabilities":["web_host"]}),
        ),
    );
    let Err(duplicate) = duplicate else {
        panic!("second web host should be rejected")
    };
    assert_eq!(errors::kind(&duplicate), ErrorKind::WebHostUnavailable);
    let (sender, mut receiver) = outbox::channel();
    let remote = core.connect(false, false, sender).unwrap();
    let _ = next_json(&mut receiver).await;
    let rejected = core.dispatch(
        &remote,
        request(
            1,
            "hello",
            json!({"client":"remote","clientVersion":"1","protocolVersion":1,
                "deviceToken":"unused","capabilities":["web_host"]}),
        ),
    );
    let Err(rejected) = rejected else {
        panic!("remote web host should be rejected")
    };
    assert_eq!(errors::kind(&rejected), ErrorKind::Unauthorized);
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn reads_are_fresh_and_mutations_are_typed_bounded_and_idempotent() {
    let root = temp_root();
    let mut core = Core::open(&root).unwrap();
    let (host, mut host_receiver) = connect(&mut core, json!(["web_host"])).await;
    let (caller, mut caller_receiver) = connect(&mut core, json!([])).await;
    let workspace = reply(
        &mut core,
        &caller,
        request(
            2,
            "workspace.create",
            json!({"name":"Web","path":root,"idempotencyKey":"workspace"}),
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
            "pane.create",
            json!({"workspaceId":workspace,"mode":"web","idempotencyKey":"pane"}),
        ),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let Outcome::Queued = core
        .dispatch(
            &caller,
            request(4, "web.snapshot", json!({"paneId":pane,"screenshot":true})),
        )
        .unwrap()
    else {
        panic!("snapshot should wait for the host")
    };
    let snapshot_command = next_json(&mut host_receiver).await;
    assert_eq!(snapshot_command["params"]["command"], "snapshot");
    assert_human_args(&snapshot_command, json!({"screenshot":true}));
    assert!(snapshot_command["params"]["operationKey"].is_string());
    let oversized = "x".repeat(soyeht_protocol::limits::WEB_SNAPSHOT_BYTES);
    assert!(
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":snapshot_command["id"],"result":{
                "paneId":pane,"value":{"navigationGeneration":"1","title":"Page",
                    "url":"https://example.test/","nodes":[],"truncated":false,
                    "unknown":oversized}}}),
        )
        .is_err()
    );
    assert!(
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":snapshot_command["id"],"result":{
                "paneId":pane,"value":{"navigationGeneration":"1","title":"Page",
                    "url":"https://example.test/","truncated":false}}}),
        )
        .is_err()
    );
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":snapshot_command["id"],"result":{
            "paneId":pane,"value":{"navigationGeneration":"1","title":"Page",
                "url":"https://example.test/","nodes":[],"truncated":false}}}),
    )
    .unwrap();
    let snapshot = next_json(&mut caller_receiver).await;
    assert_eq!(snapshot["result"]["title"], "Page");
    assert_eq!(snapshot["result"]["paneId"], pane);
    assert!(matches!(
        core.dispatch(
            &caller,
            request(5, "web.snapshot", json!({"paneId":pane,"screenshot":true})),
        )
        .unwrap(),
        Outcome::Queued
    ));
    let fresh_snapshot = next_json(&mut host_receiver).await;
    assert_ne!(fresh_snapshot["id"], snapshot_command["id"]);
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":fresh_snapshot["id"],"result":{
            "paneId":pane,"value":{"navigationGeneration":"1","title":"Page 2",
                "url":"https://example.test/","nodes":[],"truncated":false}}}),
    )
    .unwrap();
    let _ = next_json(&mut caller_receiver).await;
    let click = json!({"paneId":pane,"ref":"e1","idempotencyKey":"click-1"});
    assert!(matches!(
        core.dispatch(&caller, request(6, "web.click", click.clone()))
            .unwrap(),
        Outcome::Queued
    ));
    let click_command = next_json(&mut host_receiver).await;
    assert_human_args(&click_command, json!({"ref":"e1"}));
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":click_command["id"],"result":{
            "paneId":pane,"value":{"completed":true}}}),
    )
    .unwrap();
    let clicked = next_json(&mut caller_receiver).await;
    assert_eq!(clicked["result"], json!({"paneId":pane,"completed":true}));
    let Outcome::Reply(repeated) = core
        .dispatch(&caller, request(7, "web.click", click))
        .unwrap()
    else {
        panic!("completed mutation should use its durable receipt")
    };
    assert_eq!(repeated, clicked["result"]);
    crate::test_paths::remove_after(core, root);
}

#[tokio::test]
async fn screenshot_bytes_commit_exactly_and_precede_the_json_response() {
    let root = temp_root();
    let mut core = Core::open(&root).unwrap();
    let (host, mut host_receiver) = connect(&mut core, json!(["web_host"])).await;
    let (caller, mut caller_receiver) = connect(&mut core, json!([])).await;
    let workspace = reply(
        &mut core,
        &caller,
        request(
            2,
            "workspace.create",
            json!({"name":"Web","path":root,"idempotencyKey":"workspace"}),
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
            "pane.create",
            json!({"workspaceId":workspace,"mode":"web","idempotencyKey":"pane"}),
        ),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let Outcome::Queued = core
        .dispatch(
            &caller,
            request(4, "web.screenshot", json!({"paneId":pane})),
        )
        .unwrap()
    else {
        panic!("screenshot should wait for the host")
    };
    let command = next_json(&mut host_receiver).await;
    let attachment_id = "00112233-4455-4677-8899-aabbccddeeff";
    let image = b"bounded-png";
    let reference = json!({
        "attachmentId":attachment_id,
        "mimeType":"image/png",
        "name":"screenshot.png",
        "bytes":image.len().to_string()
    });
    core.attachment_chunk(
        &host,
        soyeht_protocol::attachment::attachment_header(attachment_id).unwrap(),
        image,
    )
    .unwrap();
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":command["id"],"result":{
            "paneId":pane,"value":{"image":reference}
        }}),
    )
    .unwrap();

    let Frame::Attachment { header, bytes } = caller_receiver.next().await.unwrap().unwrap().frame
    else {
        panic!("attachment must precede its response")
    };
    assert_eq!(
        header,
        soyeht_protocol::attachment::attachment_header(attachment_id).unwrap()
    );
    assert_eq!(bytes, image);
    let response = next_json(&mut caller_receiver).await;
    assert_eq!(response["id"], 4);
    assert_eq!(response["result"]["image"], reference);

    crate::test_paths::remove_after(core, root);
}
