//! Prompt attachments end to end: staging through the engine, the ACP block
//! each agent receives for its advertised capabilities, limits, persistence
//! and the transcript after a restart and a native replay.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::{AttachmentRef, attachment::attachment_frames, limits};
use soyeht_store::database::Store;
use std::{fs, path::Path, thread, time::Duration, time::Instant};

mod support;
use support::*;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nimage-bytes";
const THUMBNAIL: &[u8] = b"\x89PNG\r\n\x1a\nthumb";

/// Sends `bytes` as type-3 frames ahead of the request that names `id`.
fn upload(client: &mut Client, id: &str, bytes: &[u8]) {
    let reference = AttachmentRef {
        attachment_id: id.into(),
        mime_type: "application/octet-stream".into(),
        name: "upload".into(),
        bytes: bytes.len().to_string(),
    };
    for frame in attachment_frames(&reference, bytes).unwrap() {
        frame.write_to(&mut client.socket).unwrap();
    }
}

fn uploaded_image(id: &str) -> Value {
    json!({"kind":"image","name":"screen shot.png","mimeType":"image/png",
        "bytes":PNG.len().to_string(),"attachmentId":id,"thumbnail":STANDARD.encode(THUMBNAIL)})
}

fn inline_notes() -> Value {
    json!({"kind":"text","name":"notes.md","mimeType":"text/markdown","bytes":"9",
        "data":STANDARD.encode("# Notes\n\n")})
}

fn host_file(path: &Path) -> Value {
    json!({"kind":"file","name":"report.pdf","mimeType":"application/pdf","bytes":"1",
        "path":path})
}

/// The blocks after the text that the fake agent reported for one turn.
fn reported_blocks(rig: &Rig, session: &str) -> Vec<Value> {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let reported = rig.native_session(session)["history"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|turn| {
                turn[0]
                    .as_str()?
                    .strip_prefix("block-report:")
                    .map(str::to_owned)
            })
            .next_back();
        if let Some(report) = reported {
            return serde_json::from_str::<Vec<Value>>(&report).unwrap();
        }
        assert!(Instant::now() < deadline, "the agent never reported blocks");
        thread::sleep(Duration::from_millis(20));
    }
}

fn session_id(client: &mut Client, pane: &str) -> String {
    client.pane(pane)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn staged_path(attachment: &Value) -> String {
    attachment["path"].as_str().unwrap().to_owned()
}

#[test]
fn each_attachment_reaches_the_agent_in_the_richest_form_it_advertised() {
    let rig = Rig::new();
    let report = rig.path.join("report.pdf");
    fs::write(&report, b"%").unwrap();
    let (mut client, pane) = rig.agent();
    let session = session_id(&mut client, &pane);
    let id = "10213243-5465-4768-99aa-bbccddeeff00";
    upload(&mut client, id, PNG);
    let params = json!({"paneId":pane,"text":"report-blocks","idempotencyKey":"rich",
        "attachments":[uploaded_image(id),inline_notes(),host_file(&report)]});
    let first = client.call("pane.prompt", params.clone());

    let blocks = reported_blocks(&rig, &session);
    assert_eq!(blocks.len(), 3, "{blocks:?}");
    assert_eq!(
        blocks[0],
        json!({"type":"image","mimeType":"image/png","data":STANDARD.encode(PNG)})
    );
    assert_eq!(blocks[1]["type"], "resource");
    assert_eq!(blocks[1]["resource"]["text"], "# Notes\n\n");
    assert_eq!(blocks[1]["resource"]["mimeType"], "text/markdown");
    assert_eq!(blocks[2]["type"], "resource_link");
    assert_eq!(blocks[2]["name"], "report.pdf");
    let canonical = soyeht_platform::fs::canonicalize(&report).unwrap();
    // A Windows drive path is `file:///C:/...`: the drive follows the slash.
    let text = canonical.to_str().unwrap().replace('\\', "/");
    assert_eq!(
        blocks[2]["uri"],
        format!("file://{}{text}", if cfg!(windows) { "/" } else { "" })
    );

    let transcript = wait_for_user_message(&mut client, &pane, "report-blocks");
    let user = transcript
        .iter()
        .find(|message| message["role"] == "user")
        .unwrap();
    let attachments = user["attachments"].as_array().unwrap();
    assert_eq!(attachments.len(), 3);
    assert_eq!(attachments[0]["name"], "screen shot.png");
    assert_eq!(attachments[0]["thumbnail"], STANDARD.encode(THUMBNAIL));
    assert!(
        attachments
            .iter()
            .all(|item| item.get("data").is_none() && item.get("attachmentId").is_none())
    );
    let image = staged_path(&attachments[0]);
    let image = image.replace('\\', "/");
    assert!(image.contains("/prompt-attachments/") && image.ends_with("/screen shot.png"));
    assert_eq!(fs::read(&image).unwrap(), PNG);
    assert_eq!(
        fs::read(staged_path(&attachments[1])).unwrap(),
        b"# Notes\n\n"
    );
    assert_eq!(staged_path(&attachments[2]), canonical.to_str().unwrap());

    // The same bytes repeat the prompt; different bytes under the key conflict.
    let mut retry = rig.client();
    upload(&mut retry, id, PNG);
    assert_eq!(retry.call("pane.prompt", params.clone()), first);
    let mut changed = rig.client();
    upload(&mut changed, id, b"\x89PNG\r\n\x1a\nimage-bytez");
    assert_eq!(
        changed.request("pane.prompt", params)["error"]["data"]["kind"],
        "idempotency_conflict"
    );
}

#[test]
fn an_agent_without_prompt_capabilities_receives_links_to_staged_files() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent_with(&["--prompt-capabilities", "none"]);
    let session = session_id(&mut client, &pane);
    let id = "20314253-6475-4869-aabb-ccddeeff0011";
    upload(&mut client, id, PNG);
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"report-blocks","idempotencyKey":"links",
            "attachments":[uploaded_image(id),inline_notes()]}),
    );
    let blocks = reported_blocks(&rig, &session);
    assert!(
        blocks.iter().all(|block| block["type"] == "resource_link"),
        "{blocks:?}"
    );
    assert_eq!(blocks[0]["name"], "screen shot.png");
    assert_eq!(blocks[0]["mimeType"], "image/png");
    assert_eq!(blocks[0]["size"], PNG.len());
    let uri = blocks[0]["uri"].as_str().unwrap();
    assert!(uri.starts_with("file://") && uri.ends_with("/screen%20shot.png"));
    let staged =
        soyeht_store::agents::transcript(&Store::open(&rig.path).unwrap(), &pane, None, 50)
            .unwrap()
            .items
            .into_iter()
            .find_map(|message| message.attachments)
            .unwrap();
    assert_eq!(fs::read(staged[0].path.as_ref().unwrap()).unwrap(), PNG);
}

#[test]
fn limits_are_refused_with_a_clear_error_and_the_connection_stays_usable() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let refused = |client: &mut Client, attachments: Value, kind: &str| {
        let response = client.request(
            "pane.prompt",
            json!({"paneId":pane,"text":"hello","attachments":attachments,
                "idempotencyKey":uuid::Uuid::new_v4().to_string()}),
        );
        assert_eq!(response["error"]["data"]["kind"], kind, "{response}");
        response["error"]["message"].as_str().unwrap().to_owned()
    };

    let many = vec![inline_notes(); limits::PROMPT_ATTACHMENTS + 1];
    let message = refused(&mut client, json!(many), "attachment_limit_exceeded");
    assert!(message.contains("at most 20"), "{message}");

    let large = "00112233-4455-4677-8899-aabbccddeeff";
    let bytes = vec![1u8; limits::PROMPT_ATTACHMENT_BYTES + 1];
    upload(&mut client, large, &bytes);
    let file = json!({"kind":"file","name":"big.bin","mimeType":"application/octet-stream",
        "bytes":bytes.len().to_string(),"attachmentId":large});
    let message = refused(&mut client, json!([file]), "attachment_limit_exceeded");
    assert!(
        message.contains("big.bin") && message.contains("10 MiB"),
        "{message}"
    );

    // Over the total, the frames are discarded and the request reports it.
    let first = "11112233-4455-4677-8899-aabbccddeeff";
    let second = "22112233-4455-4677-8899-aabbccddeeff";
    let half = vec![2u8; limits::PROMPT_ATTACHMENT_TOTAL_BYTES / 2 + 1];
    upload(&mut client, first, &half);
    upload(&mut client, second, &half);
    let pair = |id: &str| {
        json!({"kind":"file","name":"half.bin","mimeType":"application/octet-stream",
            "bytes":half.len().to_string(),"attachmentId":id})
    };
    let message = refused(
        &mut client,
        json!([pair(first), pair(second)]),
        "attachment_limit_exceeded",
    );
    assert!(message.contains("20 MiB"), "{message}");

    let inline = json!({"kind":"file","name":"inline.bin","mimeType":"application/octet-stream",
        "bytes":(limits::PROMPT_ATTACHMENT_INLINE_BYTES + 1).to_string(),
        "data":STANDARD.encode(vec![0u8; limits::PROMPT_ATTACHMENT_INLINE_BYTES + 1])});
    // The schema bounds inline data to the same limit.
    refused(&mut client, json!([inline]), "invalid_params");

    let mut wrong_size = inline_notes();
    wrong_size["bytes"] = json!("3");
    refused(&mut client, json!([wrong_size]), "invalid_params");
    let missing = rig.path.join("missing.pdf");
    refused(&mut client, json!([host_file(&missing)]), "invalid_params");
    let mut two_sources = inline_notes();
    two_sources["path"] = json!("/etc/hosts");
    refused(&mut client, json!([two_sources]), "invalid_params");
    let mut mislabeled = inline_notes();
    mislabeled["kind"] = json!("image");
    refused(&mut client, json!([mislabeled]), "invalid_params");

    let accepted = client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"hello","attachments":[inline_notes()],
            "idempotencyKey":"after-refusals"}),
    );
    assert!(accepted["promptId"].is_string());
    assert!(
        !rig.path.join("prompt-attachments").exists()
            || fs::read_dir(rig.path.join("prompt-attachments"))
                .unwrap()
                .count()
                == 1,
        "refused prompts stage nothing"
    );
}

#[test]
fn a_queued_attachment_survives_restart_and_native_replay_keeps_it_in_the_transcript() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let session = session_id(&mut client, &pane);
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"slow","idempotencyKey":"hold"}),
    );
    client.wait_state(&pane, "working");
    let id = "30415263-7485-496a-bbcc-ddeeff001122";
    upload(&mut client, id, PNG);
    let queued = client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"report-blocks","idempotencyKey":"queued-image",
            "attachments":[uploaded_image(id)]}),
    );
    rig.kill_engine();
    let queue = soyeht_store::agents::queue(&Store::open(&rig.path).unwrap(), &pane).unwrap();
    let item = queue
        .iter()
        .find(|item| Some(item.prompt_id.as_str()) == queued["promptId"].as_str())
        .unwrap();
    let path = item.attachments.as_ref().unwrap()[0].path.clone().unwrap();
    assert_eq!(fs::read(&path).unwrap(), PNG);

    rig.start();
    let mut restored = rig.client();
    let blocks = reported_blocks(&rig, &session);
    assert_eq!(blocks[0]["data"], STANDARD.encode(PNG));
    let live = wait_for_user_message(&mut restored, &pane, "report-blocks");
    drop(restored);

    let replayed = restart_and_wait_for_replay(&mut rig, &pane, &live);
    let attachments: Vec<_> = replayed
        .iter()
        .filter(|message| message["role"] == "user")
        .filter_map(|message| message["attachments"].as_array())
        .flatten()
        .collect();
    assert_eq!(attachments.len(), 1, "{replayed:?}");
    assert_eq!(attachments[0]["kind"], "image");
    assert_eq!(attachments[0]["mimeType"], "image/png");
    assert_eq!(attachments[0]["bytes"], PNG.len().to_string());
}
