use super::*;
use crate::web::test_support::{create_web_pane, next_json, reply, request, temp_root};
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
    let pane = create_web_pane(&mut core, &caller, &root);
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
    // An answer to a command the engine no longer waits for is late, and is ignored.
    let late = json!({"jsonrpc":"2.0","id":command_id,"error":{"code":-32603,"message":"late"}});
    core.web_host_response(&host, late).unwrap();

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
    let pane = create_web_pane(&mut core, &caller, &root);
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
    let response = image_precedes_response(
        "web.screenshot",
        |reference| json!({"image":reference,"endOfPage":false}),
    )
    .await;
    assert_eq!(response["result"]["image"]["name"], "screenshot.png");
    assert_eq!(response["result"]["endOfPage"], false);
}

#[tokio::test]
async fn read_returns_its_text_and_links_after_the_committed_image() {
    let response = image_precedes_response("web.read", |reference| {
        json!({"navigationGeneration":"3","title":"Feed","url":"https://example.test/",
            "screens":2,"text":"First post\nSecond post",
            "links":[{"text":"First","href":"https://example.test/1"}],
            "endOfPage":false,"truncated":false,"image":reference})
    })
    .await;
    assert_eq!(response["result"]["text"], "First post\nSecond post");
    assert_eq!(
        response["result"]["links"][0]["href"],
        "https://example.test/1"
    );
    assert_eq!(response["result"]["screens"], 2);
}

#[tokio::test]
async fn an_image_answer_that_comes_late_does_not_break_the_next_answer() {
    let response =
        image_after_late_answer("web.screenshot", |image| json!({"image":image}), true).await;
    assert_eq!(response["id"], 4);
    assert_eq!(response["result"]["image"]["bytes"], "11");
}

async fn image_precedes_response(method: &str, value: impl Fn(Value) -> Value) -> Value {
    image_after_late_answer(method, value, false).await
}

/// With `late`, the host first answers, with an image, a command the engine had stopped waiting
/// for; the answer that follows must still be accepted.
async fn image_after_late_answer(
    method: &str,
    value: impl Fn(Value) -> Value,
    late: bool,
) -> Value {
    let root = temp_root();
    let mut core = Core::open(&root).unwrap();
    let (host, mut host_receiver) = connect(&mut core, json!(["web_host"])).await;
    let (caller, mut caller_receiver) = connect(&mut core, json!([])).await;
    let pane = create_web_pane(&mut core, &caller, &root);
    if late {
        let Outcome::Queued = core
            .dispatch(&caller, request(5, method, json!({"paneId":pane})))
            .unwrap()
        else {
            panic!("the first command should wait for the host")
        };
        let first = next_json(&mut host_receiver).await;
        core.fail_web_waiter(first["id"].as_str().unwrap(), "Web host command timed out")
            .unwrap();
        assert_eq!(next_json(&mut caller_receiver).await["id"], 5);
        let stale = "ffeeddcc-bbaa-4977-8899-001122334455";
        core.attachment_chunk(
            &host,
            soyeht_protocol::attachment::attachment_header(stale).unwrap(),
            b"late",
        )
        .unwrap();
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":first["id"],"result":{"paneId":pane,"value":{"image":{
                "attachmentId":stale,"mimeType":"image/png","name":"screenshot.png","bytes":"4"}}}}),
        )
        .unwrap();
    }
    let Outcome::Queued = core
        .dispatch(&caller, request(4, method, json!({"paneId":pane})))
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
            "paneId":pane,"value":value(reference.clone())
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
    response
}
