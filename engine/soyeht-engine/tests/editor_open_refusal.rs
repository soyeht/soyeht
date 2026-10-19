//! What `file.open` answers on the wire for a file the editor cannot show as text. The kinds are
//! part of the contract: a client shows them in the pane, not in a dialog (ADR 0094).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::{Value, json};
use soyeht_protocol::{ErrorKind, error_code, limits::EDITOR_HISTORY_BYTES};
use std::fs;
use support::Rig;

fn refusal(response: &Value) -> (i64, String) {
    (
        response["error"]["code"].as_i64().expect("error code"),
        response["error"]["data"]["kind"]
            .as_str()
            .expect("error kind")
            .to_owned(),
    )
}

#[test]
fn file_open_refuses_a_binary_file_and_a_file_over_the_limit_with_their_own_codes() {
    let rig = Rig::new();
    let project = rig.path.join("refusal-project");
    fs::create_dir_all(project.join("a-folder")).unwrap();
    fs::write(project.join("blob.bin"), [0xff, 0xfe, 0x00, 0x9f]).unwrap();
    fs::write(project.join("ok.txt"), "text\n").unwrap();
    // A sparse file: the engine refuses on its size before it reads a byte.
    fs::File::create(project.join("huge.txt"))
        .unwrap()
        .set_len(EDITOR_HISTORY_BYTES as u64 + 1)
        .unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Refusal","path":project,"idempotencyKey":"refusal-workspace"}),
    )["workspace"]["workspaceId"]
        .clone();
    let mut open = |path: &str| {
        client.request(
            "file.open",
            json!({"workspaceId":workspace,"path":path,"idempotencyKey":format!("open-{path}")}),
        )
    };

    let (code, kind) = refusal(&open("blob.bin"));
    assert_eq!((code, kind.as_str()), (-32039, "file_not_text"));
    assert_eq!(
        kind,
        serde_json::to_value(ErrorKind::FileNotText)
            .unwrap()
            .as_str()
            .unwrap()
    );
    assert_eq!(code, error_code(&ErrorKind::FileNotText));

    let (code, kind) = refusal(&open("huge.txt"));
    assert_eq!((code, kind.as_str()), (-32040, "file_too_large"));
    assert_eq!(code, error_code(&ErrorKind::FileTooLarge));

    // Any other file the editor cannot read stays an ordinary refusal, and a text file opens.
    let (_, kind) = refusal(&open("a-folder"));
    assert_eq!(kind, "invalid_params");
    let opened = open("ok.txt");
    assert!(opened.get("error").is_none(), "{opened}");
}
